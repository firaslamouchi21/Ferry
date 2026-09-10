use sha2::{Digest, Sha256};
use thiserror::Error;

use ferry_proto::states::ItemKind;

use crate::expiry::ExpiryClock;
use crate::policy::{self, PolicyError};
use crate::ports::{NewOutboundItem, OutboundSource, Store, StoreError};

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("policy denied this send: {0}")]
    Policy(#[from] PolicyError),
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("failed to read the source: {0}")]
    Io(#[from] std::io::Error),
}

pub fn enqueue_send(
    store: &mut impl Store,
    clock: &ExpiryClock,
    item: &NewOutboundItem,
    actor: &str,
) -> Result<String, OutboxError> {
    policy::authorize_send_item(store, item)?;
    let item_id = store.create_and_enqueue_outbound(item, actor)?;
    store.set_outbox_expiry(&item_id, &clock.compute_deadline(item.ttl_secs))?;
    Ok(item_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRequest {
    pub peer_id: String,
    pub source_path: String,
    pub name: String,
    pub kind: ItemKind,
    pub ttl_secs: u32,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
}

pub fn enqueue_send_from_path(
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    request: &SendRequest,
    actor: &str,
) -> Result<String, OutboxError> {
    policy::authorize_send(store, &request.peer_id)?;

    let mut reader = source.open(&request.source_path)?;
    let mut hasher = Sha256::new();
    let size_bytes = std::io::copy(&mut reader, &mut hasher)?;
    let hash = hex::encode(hasher.finalize());

    let item = NewOutboundItem {
        peer_id: request.peer_id.clone(),
        kind: request.kind,
        name: request.name.clone(),
        size_bytes,
        hash,
        ttl_secs: request.ttl_secs,
        is_burn_after_read: request.is_burn_after_read,
        notify_on_open: request.notify_on_open,
        source_path: request.source_path.clone(),
    };
    enqueue_send(store, clock, &item, actor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::states::{ItemKind, TransferState};
    use std::collections::{HashMap, HashSet};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeOutboundItem {
        peer_id: String,
        state: TransferState,
        hash: String,
        size_bytes: u64,
    }

    #[derive(Default)]
    struct FakeStore {
        roster: HashSet<String>,
        items: HashMap<String, FakeOutboundItem>,
        outbox_expiry: HashMap<String, crate::expiry::ExpiryDeadline>,
        next_id: u64,
        create_calls: u32,
    }

    impl FakeStore {
        fn with_peer(peer_id: &str) -> Self {
            let mut store = Self::default();
            store.roster.insert(peer_id.to_string());
            store
        }
    }

    impl Store for FakeStore {
        fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, StoreError> {
            Ok(self.roster.contains(peer_id))
        }

        fn create_and_enqueue_outbound(
            &mut self,
            item: &NewOutboundItem,
            _actor: &str,
        ) -> Result<String, StoreError> {
            self.create_calls += 1;
            self.next_id += 1;
            let id = format!("item-{}", self.next_id);
            self.items.insert(
                id.clone(),
                FakeOutboundItem {
                    peer_id: item.peer_id.clone(),
                    state: TransferState::Queued,
                    hash: item.hash.clone(),
                    size_bytes: item.size_bytes,
                },
            );
            Ok(id)
        }

        fn get_outbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
            Ok(self.items.get(item_id).map(|i| i.state))
        }

        fn set_outbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn create_inbound(
            &mut self,
            _offer: &ferry_proto::envelope::Offer,
            _peer_id: &str,
            _actor: &str,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn get_inbound_state(&self, _item_id: &str) -> Result<Option<TransferState>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn set_inbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn inbound_bytes_received(&self, _item_id: &str) -> Result<u64, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn append_inbound_chunk(&mut self, _item_id: &str, _seq: u64, _bytes: &[u8]) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn inbound_full_hash_so_far(&self, _item_id: &str) -> Result<String, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn finalize_inbound_delivered(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn mark_inbound_opened(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn get_outbound_item(&self, _item_id: &str) -> Result<Option<NewOutboundItem>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn list_outbox_for_peer(&self, _peer_id: &str) -> Result<Vec<crate::ports::OutboxItem>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn record_outbox_attempt(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn remove_outbox_entry(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn set_inbound_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn get_inbound_expiry(&self, _item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn set_outbox_expiry(
            &mut self,
            item_id: &str,
            deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            self.outbox_expiry.insert(item_id.to_string(), deadline.clone());
            Ok(())
        }

        fn get_outbox_expiry(&self, item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            Ok(self.outbox_expiry.get(item_id).cloned())
        }

        fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str, _cause: &str) -> Result<(), StoreError> {
            Ok(())
        }

        fn read_inbound_plaintext(&self, _item_id: &str) -> Result<Vec<u8>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn is_healthy(&self) -> Result<bool, StoreError> {
            Ok(true)
        }

        fn list_roster_peers(&self) -> Result<Vec<crate::ports::RosterPeer>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn export_signed_roster(&self) -> Result<String, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<crate::ports::RosterImportSummary, StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn add_paired_peer(&mut self, _peer: &crate::ports::NewRosterPeer) -> Result<(), StoreError> {
            unimplemented!("not needed for outbox tests")
        }

        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
            unimplemented!("not needed for outbox tests")
        }
    }

    fn sample_item(peer_id: &str) -> NewOutboundItem {
        NewOutboundItem {
            peer_id: peer_id.into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 1024,
            hash: "deadbeef".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/notes.txt".into(),
        }
    }

    #[test]
    fn enqueue_send_succeeds_for_a_rostered_peer_and_lands_in_queued_state() {
        let mut store = FakeStore::with_peer("peer-1");
        let clock = ExpiryClock::new();
        let item_id = enqueue_send(&mut store, &clock, &sample_item("peer-1"), "local").unwrap();
        assert_eq!(
            store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Queued)
        );
        assert!(store.outbox_expiry.contains_key(&item_id), "enqueue must also set an outbox-TTL deadline");
    }

    #[test]
    fn enqueue_send_is_rejected_for_an_unrostered_peer_before_ever_touching_storage() {
        let mut store = FakeStore::default();
        let clock = ExpiryClock::new();
        let result = enqueue_send(&mut store, &clock, &sample_item("stranger"), "local");
        assert!(matches!(
            result,
            Err(OutboxError::Policy(PolicyError::PeerNotAuthorized))
        ));
        assert_eq!(
            store.create_calls, 0,
            "policy must reject before storage is ever written to"
        );
    }

    struct FakeSource {
        contents: HashMap<String, Vec<u8>>,
    }

    impl OutboundSource for FakeSource {
        type Reader = std::io::Cursor<Vec<u8>>;

        fn open(&mut self, key: &str) -> std::io::Result<Self::Reader> {
            self.contents
                .get(key)
                .cloned()
                .map(std::io::Cursor::new)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such source"))
        }
    }

    fn sample_send_request(peer_id: &str, source_path: &str) -> SendRequest {
        SendRequest {
            peer_id: peer_id.into(),
            source_path: source_path.into(),
            name: "notes.txt".into(),
            kind: ItemKind::File,
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
        }
    }

    #[test]
    fn enqueue_send_from_path_hashes_and_sizes_the_real_source_bytes() {
        let mut store = FakeStore::with_peer("peer-1");
        let mut source = FakeSource {
            contents: HashMap::from([("/tmp/notes.txt".to_string(), b"hello ferry".to_vec())]),
        };
        let clock = ExpiryClock::new();

        let item_id = enqueue_send_from_path(
            &mut store,
            &clock,
            &mut source,
            &sample_send_request("peer-1", "/tmp/notes.txt"),
            "local",
        )
        .unwrap();

        let item = store.items.get(&item_id).unwrap();
        assert_eq!(item.peer_id, "peer-1");
        assert_eq!(
            store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Queued)
        );

        let mut hasher = Sha256::new();
        hasher.update(b"hello ferry");
        let expected_hash = hex::encode(hasher.finalize());
        assert_eq!(item.hash, expected_hash);
        assert_eq!(item.size_bytes, 11);
    }

    #[test]
    fn enqueue_send_from_path_is_rejected_for_an_unrostered_peer_before_reading_the_source() {
        let mut store = FakeStore::default();
        let mut source = FakeSource {
            contents: HashMap::from([("/tmp/notes.txt".to_string(), b"hello ferry".to_vec())]),
        };
        let clock = ExpiryClock::new();

        let result = enqueue_send_from_path(
            &mut store,
            &clock,
            &mut source,
            &sample_send_request("stranger", "/tmp/notes.txt"),
            "local",
        );

        assert!(matches!(result, Err(OutboxError::Policy(PolicyError::PeerNotAuthorized))));
        assert_eq!(store.create_calls, 0);
    }

    #[test]
    fn enqueue_send_from_path_surfaces_a_missing_source_file_as_an_io_error() {
        let mut store = FakeStore::with_peer("peer-1");
        let mut source = FakeSource { contents: HashMap::new() };
        let clock = ExpiryClock::new();

        let result = enqueue_send_from_path(
            &mut store,
            &clock,
            &mut source,
            &sample_send_request("peer-1", "/tmp/missing.txt"),
            "local",
        );

        assert!(matches!(result, Err(OutboxError::Io(_))));
        assert_eq!(store.create_calls, 0, "a failed read must never reach storage");
    }
}
