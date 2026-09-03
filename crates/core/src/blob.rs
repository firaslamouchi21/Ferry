use ferry_proto::blob::BlobManifest;
use ferry_proto::envelope::Offer;
use ferry_proto::ids::ItemId;
use ferry_proto::states::{ItemKind, TransferState};
use thiserror::Error;

use crate::expiry::ExpiryClock;
use crate::policy::{self, PolicyError};
use crate::ports::{Store, StoreError};
use crate::transfer::transition_inbound;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("policy denied this import: {0}")]
    Policy(#[from] PolicyError),
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("the sealed blob's contents do not match its manifest hash")]
    HashMismatch,
    #[error("item {0} has already been imported")]
    AlreadyImported(String),
    #[error("illegal state during import: item {item_id} is {state:?}")]
    IllegalState { item_id: String, state: TransferState },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub item_id: String,
    pub origin_peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: u64,
}

pub fn import_manifest(
    store: &mut impl Store,
    clock: &ExpiryClock,
    manifest: &BlobManifest,
    actor: &str,
) -> Result<ImportSummary, ImportError> {
    policy::authorize_send(store, &manifest.origin_peer_id)?;
    policy::authorize_item_kind(manifest.kind, manifest.size_bytes)?;

    let item_id = manifest.item_id.clone();

    if matches!(
        store.get_inbound_state(&item_id)?,
        Some(TransferState::Delivered)
            | Some(TransferState::Opened)
            | Some(TransferState::Expired)
    ) {
        return Err(ImportError::AlreadyImported(item_id));
    }

    let offer = Offer {
        item_id: ItemId(item_id.clone()),
        kind: manifest.kind,
        name: manifest.name.clone(),
        size_bytes: manifest.size_bytes,
        hash: manifest.hash.clone(),
        ttl_secs: manifest.ttl_secs,
        burn_after_read: manifest.is_burn_after_read,
        notify_on_open: manifest.notify_on_open,
    };
    store.create_inbound(&offer, &manifest.origin_peer_id, actor)?;

    let current = store
        .get_inbound_state(&item_id)?
        .ok_or_else(|| StoreError("inbound row vanished immediately after creation".into()))?;
    match current {
        TransferState::Offered => {
            transition_inbound(store, &item_id, TransferState::Accepted).map_err(map_transition)?;
            transition_inbound(store, &item_id, TransferState::Transferring).map_err(map_transition)?;
        }
        TransferState::Transferring => {}
        other => return Err(ImportError::IllegalState { item_id, state: other }),
    }

    if store.inbound_bytes_received(&item_id)? == 0 && !manifest.payload.is_empty() {
        store.append_inbound_chunk(&item_id, 0, &manifest.payload)?;
    }

    let computed = store.inbound_full_hash_so_far(&item_id)?;
    if computed != manifest.hash {
        store.set_inbound_state(&item_id, TransferState::Failed)?;
        return Err(ImportError::HashMismatch);
    }

    transition_inbound(store, &item_id, TransferState::Delivered).map_err(map_transition)?;
    store.finalize_inbound_delivered(&item_id)?;
    store.set_inbound_expiry(&item_id, &clock.compute_deadline(manifest.ttl_secs))?;

    Ok(ImportSummary {
        item_id,
        origin_peer_id: manifest.origin_peer_id.clone(),
        kind: manifest.kind,
        name: manifest.name.clone(),
        size_bytes: manifest.size_bytes,
    })
}

fn map_transition(err: crate::transfer::TransferError) -> ImportError {
    ImportError::Store(StoreError(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::InMemoryStore;
    use sha2::{Digest, Sha256};

    fn manifest_for(payload: &[u8], origin: &str) -> BlobManifest {
        BlobManifest {
            item_id: "item-1".into(),
            origin_peer_id: origin.into(),
            kind: ItemKind::File,
            name: "carried.bin".into(),
            size_bytes: payload.len() as u64,
            hash: hex::encode(Sha256::digest(payload)),
            ttl_secs: 3600,
            is_burn_after_read: false,
            notify_on_open: false,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn imports_a_rostered_origins_blob_through_the_standard_inbound_path_to_delivered() {
        let mut store = InMemoryStore::default();
        store.add_roster_peer("peer-a");
        let clock = ExpiryClock::new();

        let summary = import_manifest(&mut store, &clock, &manifest_for(b"payload bytes", "peer-a"), "local").unwrap();

        assert_eq!(summary.item_id, "item-1");
        assert_eq!(store.get_inbound_state("item-1").unwrap(), Some(TransferState::Delivered));
        assert_eq!(store.read_inbound_plaintext("item-1").unwrap(), b"payload bytes");
        assert!(store.get_inbound_expiry("item-1").unwrap().is_some());
    }

    #[test]
    fn an_unrostered_origin_is_rejected() {
        let mut store = InMemoryStore::default();
        let clock = ExpiryClock::new();
        assert!(matches!(
            import_manifest(&mut store, &clock, &manifest_for(b"x", "stranger"), "local"),
            Err(ImportError::Policy(PolicyError::PeerNotAuthorized))
        ));
    }

    #[test]
    fn a_tampered_payload_fails_the_hash_check_and_never_reaches_delivered() {
        let mut store = InMemoryStore::default();
        store.add_roster_peer("peer-a");
        let clock = ExpiryClock::new();

        let mut manifest = manifest_for(b"the real bytes", "peer-a");
        manifest.payload = b"tampered".to_vec();

        assert!(matches!(
            import_manifest(&mut store, &clock, &manifest, "local"),
            Err(ImportError::HashMismatch)
        ));
        assert_eq!(store.get_inbound_state("item-1").unwrap(), Some(TransferState::Failed));
    }

    #[test]
    fn importing_the_same_blob_twice_is_rejected_the_second_time() {
        let mut store = InMemoryStore::default();
        store.add_roster_peer("peer-a");
        let clock = ExpiryClock::new();
        let manifest = manifest_for(b"once", "peer-a");

        import_manifest(&mut store, &clock, &manifest, "local").unwrap();
        assert!(matches!(
            import_manifest(&mut store, &clock, &manifest, "local"),
            Err(ImportError::AlreadyImported(_))
        ));
    }

    #[test]
    fn an_over_cap_message_blob_is_rejected_by_kind_policy() {
        let mut store = InMemoryStore::default();
        store.add_roster_peer("peer-a");
        let clock = ExpiryClock::new();

        let mut manifest = manifest_for(b"hi", "peer-a");
        manifest.kind = ItemKind::Message;
        manifest.size_bytes = crate::policy::MAX_MESSAGE_BYTES + 1;

        assert!(matches!(
            import_manifest(&mut store, &clock, &manifest, "local"),
            Err(ImportError::Policy(PolicyError::ItemTooLargeForKind { .. }))
        ));
    }
}
