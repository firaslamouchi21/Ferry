use ferry_crypto::roster::SignedRoster;

use crate::ports::{
    PublishedRef, RemoteError, RemoteFetch, RemoteLocator, RosterImportSummary, SnippetPublisher, Store,
    StoreError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntryPreview {
    pub peer_id: String,
    pub display_name: String,
    pub change: RosterChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterChange {
    Add,
    AlreadyPresent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterFetchPreview {
    pub signer_verifying_key_hex: String,
    pub known_signer: bool,
    pub adds: u32,
    pub already_present: u32,
    pub entries: Vec<RosterEntryPreview>,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteRosterError {
    #[error(transparent)]
    Fetch(#[from] RemoteError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the fetched roster is not valid JSON: {0}")]
    Parse(String),
    #[error("the fetched roster failed signature verification: {0}")]
    Verify(String),
}

fn parse_and_verify(bytes: &[u8]) -> Result<SignedRoster, RemoteRosterError> {
    let roster: SignedRoster =
        serde_json::from_slice(bytes).map_err(|e| RemoteRosterError::Parse(e.to_string()))?;
    roster
        .verify()
        .map_err(|e| RemoteRosterError::Verify(e.to_string()))?;
    Ok(roster)
}

pub fn preview_roster(
    fetch: &impl RemoteFetch,
    store: &impl Store,
    locator: &RemoteLocator,
) -> Result<RosterFetchPreview, RemoteRosterError> {
    let bytes = fetch.fetch(locator)?;
    let roster = parse_and_verify(&bytes)?;

    let signer_hex = hex::encode(roster.signer_signing_key);
    let known_keys = store.roster_signing_keys_hex()?;
    let known_signer = known_keys.iter().any(|k| k.eq_ignore_ascii_case(&signer_hex));

    let current: std::collections::HashSet<String> = store
        .list_roster_peers()?
        .into_iter()
        .map(|p| p.peer_id)
        .collect();

    let mut adds = 0u32;
    let mut already_present = 0u32;
    let entries = roster
        .entries
        .iter()
        .map(|e| {
            let peer_id = e.peer_id.0.clone();
            let change = if current.contains(&peer_id) {
                already_present += 1;
                RosterChange::AlreadyPresent
            } else {
                adds += 1;
                RosterChange::Add
            };
            RosterEntryPreview {
                peer_id,
                display_name: e.display_name.clone(),
                change,
            }
        })
        .collect();

    Ok(RosterFetchPreview {
        signer_verifying_key_hex: signer_hex,
        known_signer,
        adds,
        already_present,
        entries,
    })
}

pub fn apply_roster(
    fetch: &impl RemoteFetch,
    store: &mut impl Store,
    locator: &RemoteLocator,
) -> Result<RosterImportSummary, RemoteRosterError> {
    let bytes = fetch.fetch(locator)?;
    let _ = parse_and_verify(&bytes)?;
    let json = String::from_utf8(bytes).map_err(|e| RemoteRosterError::Parse(e.to_string()))?;
    Ok(store.import_signed_roster(&json)?)
}

#[derive(Debug, thiserror::Error)]
pub enum GistPublishError {
    #[error(transparent)]
    Publish(#[from] RemoteError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub fn publish_item_as_snippet(
    publisher: &impl SnippetPublisher,
    store: &impl Store,
    item_id: &str,
) -> Result<PublishedRef, GistPublishError> {
    let blob = store.build_sealed_blob(item_id)?;
    let name = format!("ferry-{item_id}.sealed");
    Ok(publisher.publish_private(&name, &blob)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{NewRosterPeer, RosterPeer};
    use ferry_crypto::identity::Identity;
    use ferry_crypto::roster::RosterEntry;
    use ferry_proto::ids::PeerId;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeRemote {
        payload: Vec<u8>,
        published: RefCell<Vec<(String, Vec<u8>)>>,
    }

    impl RemoteFetch for FakeRemote {
        fn fetch(&self, _locator: &RemoteLocator) -> Result<Vec<u8>, RemoteError> {
            Ok(self.payload.clone())
        }
    }

    impl SnippetPublisher for FakeRemote {
        fn publish_private(&self, name: &str, bytes: &[u8]) -> Result<PublishedRef, RemoteError> {
            self.published.borrow_mut().push((name.to_string(), bytes.to_vec()));
            Ok(PublishedRef {
                url: "https://example.invalid/gist/abc".into(),
                id: "abc".into(),
            })
        }
    }

    #[derive(Default)]
    struct RosterStore {
        peers: Vec<RosterPeer>,
        signing_keys: Vec<String>,
        imported: RefCell<Vec<String>>,
        blobs: HashMap<String, Vec<u8>>,
    }

    impl Store for RosterStore {
        fn is_peer_authorized(&self, _peer_id: &str) -> Result<bool, StoreError> {
            Ok(true)
        }
        fn create_and_enqueue_outbound(
            &mut self,
            _item: &crate::ports::NewOutboundItem,
            _actor: &str,
        ) -> Result<String, StoreError> {
            unimplemented!()
        }
        fn get_outbound_state(&self, _id: &str) -> Result<Option<ferry_proto::states::TransferState>, StoreError> {
            Ok(None)
        }
        fn set_outbound_state(&mut self, _id: &str, _s: ferry_proto::states::TransferState) -> Result<(), StoreError> {
            Ok(())
        }
        fn create_inbound(&mut self, _o: &ferry_proto::envelope::Offer, _p: &str, _a: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn get_inbound_state(&self, _id: &str) -> Result<Option<ferry_proto::states::TransferState>, StoreError> {
            Ok(None)
        }
        fn set_inbound_state(&mut self, _id: &str, _s: ferry_proto::states::TransferState) -> Result<(), StoreError> {
            Ok(())
        }
        fn inbound_bytes_received(&self, _id: &str) -> Result<u64, StoreError> {
            Ok(0)
        }
        fn append_inbound_chunk(&mut self, _id: &str, _seq: u64, _b: &[u8]) -> Result<(), StoreError> {
            Ok(())
        }
        fn inbound_full_hash_so_far(&self, _id: &str) -> Result<String, StoreError> {
            Ok(String::new())
        }
        fn finalize_inbound_delivered(&mut self, _id: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn mark_inbound_opened(&mut self, _id: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn get_outbound_item(&self, _id: &str) -> Result<Option<crate::ports::NewOutboundItem>, StoreError> {
            Ok(None)
        }
        fn list_outbox_for_peer(&self, _p: &str) -> Result<Vec<crate::ports::OutboxItem>, StoreError> {
            Ok(vec![])
        }
        fn record_outbox_attempt(&mut self, _id: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn remove_outbox_entry(&mut self, _id: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn set_inbound_expiry(&mut self, _id: &str, _d: &crate::expiry::ExpiryDeadline) -> Result<(), StoreError> {
            Ok(())
        }
        fn get_inbound_expiry(&self, _id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            Ok(None)
        }
        fn set_outbox_expiry(&mut self, _id: &str, _d: &crate::expiry::ExpiryDeadline) -> Result<(), StoreError> {
            Ok(())
        }
        fn get_outbox_expiry(&self, _id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            Ok(None)
        }
        fn record_outbound_dropped(&mut self, _id: &str, _a: &str, _c: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn read_inbound_plaintext(&self, _id: &str) -> Result<Vec<u8>, StoreError> {
            Ok(vec![])
        }
        fn is_healthy(&self) -> Result<bool, StoreError> {
            Ok(true)
        }
        fn list_roster_peers(&self) -> Result<Vec<RosterPeer>, StoreError> {
            Ok(self.peers.clone())
        }
        fn export_signed_roster(&self) -> Result<String, StoreError> {
            Ok(String::new())
        }
        fn import_signed_roster(&mut self, json: &str) -> Result<RosterImportSummary, StoreError> {
            self.imported.borrow_mut().push(json.to_string());
            Ok(RosterImportSummary {
                signer_verifying_key_hex: "sig".into(),
                peer_count: 1,
                added: 1,
                skipped_existing: 0,
            })
        }
        fn add_paired_peer(&mut self, _p: &NewRosterPeer) -> Result<(), StoreError> {
            Ok(())
        }
        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
            Ok(vec![])
        }
        fn roster_signing_keys_hex(&self) -> Result<Vec<String>, StoreError> {
            Ok(self.signing_keys.clone())
        }
        fn build_sealed_blob(&self, item_id: &str) -> Result<Vec<u8>, StoreError> {
            self.blobs
                .get(item_id)
                .cloned()
                .ok_or_else(|| StoreError("no such item".into()))
        }
    }

    fn signed_roster_json(seeds: &[u8]) -> (String, String) {
        let signer = Identity::generate();
        let entries: Vec<RosterEntry> = seeds
            .iter()
            .map(|s| RosterEntry {
                peer_id: PeerId(format!("peer-{s}")),
                display_name: format!("device-{s}"),
                signing_key: [*s; 32],
                sealing_key: "age1xxx".into(),
            })
            .collect();
        let roster = SignedRoster::sign(&signer, entries).unwrap();
        (
            serde_json::to_string(&roster).unwrap(),
            hex::encode(signer.verifying_key().to_bytes()),
        )
    }

    #[test]
    fn preview_reports_the_diff_and_flags_an_unknown_signer() {
        let (json, signer_hex) = signed_roster_json(&[1, 2]);
        let remote = FakeRemote {
            payload: json.into_bytes(),
            ..Default::default()
        };
        let store = RosterStore {
            peers: vec![RosterPeer {
                peer_id: "peer-1".into(),
                display_name: "existing".into(),
                paired_at_millis: 0,
                reachable: true,
                last_seen_millis: None,
            }],
            ..Default::default()
        };

        let locator = RemoteLocator {
            provider: "github".into(),
            host: None,
            path: "team/roster/roster.json".into(),
            reference: None,
        };
        let preview = preview_roster(&remote, &store, &locator).unwrap();
        assert_eq!(preview.signer_verifying_key_hex, signer_hex);
        assert!(!preview.known_signer);
        assert_eq!(preview.adds, 1);
        assert_eq!(preview.already_present, 1);
    }

    #[test]
    fn preview_flags_a_known_signer_when_its_key_is_already_rostered() {
        let (json, signer_hex) = signed_roster_json(&[3]);
        let remote = FakeRemote {
            payload: json.into_bytes(),
            ..Default::default()
        };
        let store = RosterStore {
            signing_keys: vec![signer_hex],
            ..Default::default()
        };
        let locator = RemoteLocator {
            provider: "github".into(),
            host: None,
            path: "p".into(),
            reference: None,
        };
        assert!(preview_roster(&remote, &store, &locator).unwrap().known_signer);
    }

    #[test]
    fn apply_reverifies_before_importing_and_rejects_tampered_json() {
        let remote = FakeRemote {
            payload: b"{\"entries\":[],\"signer\":\"x\",\"signer_signing_key\":[0],\"signature\":\"\"}".to_vec(),
            ..Default::default()
        };
        let mut store = RosterStore::default();
        let locator = RemoteLocator {
            provider: "github".into(),
            host: None,
            path: "p".into(),
            reference: None,
        };
        assert!(apply_roster(&remote, &mut store, &locator).is_err());
        assert!(store.imported.borrow().is_empty());
    }

    #[test]
    fn apply_imports_a_valid_signed_roster() {
        let (json, _) = signed_roster_json(&[9]);
        let remote = FakeRemote {
            payload: json.into_bytes(),
            ..Default::default()
        };
        let mut store = RosterStore::default();
        let locator = RemoteLocator {
            provider: "github".into(),
            host: None,
            path: "p".into(),
            reference: None,
        };
        apply_roster(&remote, &mut store, &locator).unwrap();
        assert_eq!(store.imported.borrow().len(), 1);
    }

    #[test]
    fn publish_builds_the_sealed_blob_and_hands_it_to_the_publisher() {
        let remote = FakeRemote::default();
        let mut store = RosterStore::default();
        store.blobs.insert("item-1".into(), b"FERRYSEALEDBLOB-bytes".to_vec());

        let published = publish_item_as_snippet(&remote, &store, "item-1").unwrap();
        assert_eq!(published.id, "abc");
        assert_eq!(remote.published.borrow()[0].1, b"FERRYSEALEDBLOB-bytes");
    }
}
