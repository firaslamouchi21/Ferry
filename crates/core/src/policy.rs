use thiserror::Error;

use ferry_proto::states::ItemKind;

use crate::ports::{NewOutboundItem, Store, StoreError};

pub const MAX_MESSAGE_BYTES: u64 = 16 * 1024;
pub const MAX_SECRET_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("peer is not in the roster — send denied")]
    PeerNotAuthorized,
    #[error("a {kind:?} item may not exceed {limit} bytes ({actual} given)")]
    ItemTooLargeForKind {
        kind: ItemKind,
        limit: u64,
        actual: u64,
    },
    #[error("could not evaluate policy: {0}")]
    Store(#[from] StoreError),
}

pub fn authorize_send(store: &impl Store, peer_id: &str) -> Result<(), PolicyError> {
    if store.is_peer_authorized(peer_id)? {
        Ok(())
    } else {
        Err(PolicyError::PeerNotAuthorized)
    }
}

pub fn kind_size_limit(kind: ItemKind) -> Option<u64> {
    match kind {
        ItemKind::Message => Some(MAX_MESSAGE_BYTES),
        ItemKind::Secret => Some(MAX_SECRET_BYTES),
        ItemKind::File => None,
    }
}

pub fn authorize_item_kind(kind: ItemKind, size_bytes: u64) -> Result<(), PolicyError> {
    if let Some(limit) = kind_size_limit(kind) {
        if size_bytes > limit {
            return Err(PolicyError::ItemTooLargeForKind {
                kind,
                limit,
                actual: size_bytes,
            });
        }
    }
    Ok(())
}

pub fn authorize_send_item(store: &impl Store, item: &NewOutboundItem) -> Result<(), PolicyError> {
    authorize_send(store, &item.peer_id)?;
    authorize_item_kind(item.kind, item.size_bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::states::TransferState;
    use std::collections::HashSet;

    #[derive(Default)]
    struct FakeStore {
        roster: HashSet<String>,
    }

    impl Store for FakeStore {
        fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, StoreError> {
            Ok(self.roster.contains(peer_id))
        }

        fn create_and_enqueue_outbound(
            &mut self,
            _item: &crate::ports::NewOutboundItem,
            _actor: &str,
        ) -> Result<String, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn get_outbound_state(&self, _item_id: &str) -> Result<Option<TransferState>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn set_outbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn create_inbound(
            &mut self,
            _offer: &ferry_proto::envelope::Offer,
            _peer_id: &str,
            _actor: &str,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn get_inbound_state(&self, _item_id: &str) -> Result<Option<TransferState>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn set_inbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn inbound_bytes_received(&self, _item_id: &str) -> Result<u64, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn append_inbound_chunk(&mut self, _item_id: &str, _seq: u64, _bytes: &[u8]) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn inbound_full_hash_so_far(&self, _item_id: &str) -> Result<String, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn finalize_inbound_delivered(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn mark_inbound_opened(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn get_outbound_item(
            &self,
            _item_id: &str,
        ) -> Result<Option<crate::ports::NewOutboundItem>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn list_outbox_for_peer(&self, _peer_id: &str) -> Result<Vec<crate::ports::OutboxItem>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn record_outbox_attempt(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn remove_outbox_entry(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn set_inbound_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn get_inbound_expiry(&self, _item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn set_outbox_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn get_outbox_expiry(&self, _item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn read_inbound_plaintext(&self, _item_id: &str) -> Result<Vec<u8>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn is_healthy(&self) -> Result<bool, StoreError> {
            Ok(true)
        }

        fn list_roster_peers(&self) -> Result<Vec<crate::ports::RosterPeer>, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn export_signed_roster(&self) -> Result<String, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<crate::ports::RosterImportSummary, StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn add_paired_peer(&mut self, _peer: &crate::ports::NewRosterPeer) -> Result<(), StoreError> {
            unimplemented!("not needed for policy tests")
        }

        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
            unimplemented!("not needed for policy tests")
        }
    }

    #[test]
    fn a_message_over_the_kind_cap_is_rejected() {
        assert_eq!(
            authorize_item_kind(ItemKind::Message, MAX_MESSAGE_BYTES + 1),
            Err(PolicyError::ItemTooLargeForKind {
                kind: ItemKind::Message,
                limit: MAX_MESSAGE_BYTES,
                actual: MAX_MESSAGE_BYTES + 1,
            })
        );
        assert!(authorize_item_kind(ItemKind::Message, MAX_MESSAGE_BYTES).is_ok());
    }

    #[test]
    fn a_secret_over_the_kind_cap_is_rejected() {
        assert!(authorize_item_kind(ItemKind::Secret, MAX_SECRET_BYTES).is_ok());
        assert!(matches!(
            authorize_item_kind(ItemKind::Secret, MAX_SECRET_BYTES + 1),
            Err(PolicyError::ItemTooLargeForKind { .. })
        ));
    }

    #[test]
    fn a_file_has_no_kind_size_cap() {
        assert_eq!(kind_size_limit(ItemKind::File), None);
        assert!(authorize_item_kind(ItemKind::File, u64::MAX).is_ok());
    }

    #[test]
    fn authorize_send_item_checks_the_peer_before_the_kind_cap() {
        let store = FakeStore::default();
        let item = crate::ports::NewOutboundItem {
            peer_id: "stranger".into(),
            kind: ItemKind::Message,
            name: "m".into(),
            size_bytes: MAX_MESSAGE_BYTES + 1,
            hash: "h".into(),
            ttl_secs: 60,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: String::new(),
        };
        assert_eq!(
            authorize_send_item(&store, &item),
            Err(PolicyError::PeerNotAuthorized)
        );
    }

    #[test]
    fn rostered_peer_is_authorized() {
        let mut store = FakeStore::default();
        store.roster.insert("peer-1".into());
        assert!(authorize_send(&store, "peer-1").is_ok());
    }

    #[test]
    fn unrostered_peer_is_denied() {
        let store = FakeStore::default();
        assert_eq!(
            authorize_send(&store, "stranger"),
            Err(PolicyError::PeerNotAuthorized)
        );
    }
}
