use thiserror::Error;

use ferry_proto::states::TransferState;

use crate::expiry::ExpiryClock;
use crate::ports::{Channel, OutboundSource, Store, StoreError};
use crate::state::TRANSFER_TRANSITIONS;
use crate::transfer::TransferError;

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

#[derive(Debug, Default)]
pub struct DrainOutcome {
    pub delivered: Vec<String>,
    pub failed: Vec<(String, TransferError)>,
    pub dropped: Vec<String>,
    pub deferred: Vec<String>,
    pub receipts_sent: Vec<String>,
}

pub const RETRY_BASE_SECS: u64 = 5;
pub const RETRY_MAX_SECS: u64 = 300;

pub fn retry_backoff_secs(attempts: u32) -> u64 {
    match attempts {
        0 => 0,
        n => RETRY_BASE_SECS
            .checked_shl(n - 1)
            .map(|v| v.min(RETRY_MAX_SECS))
            .unwrap_or(RETRY_MAX_SECS),
    }
}

pub fn is_retry_eligible(attempts: u32, last_attempted_at_millis: Option<i64>, now_millis: i64) -> bool {
    match last_attempted_at_millis {
        None => true,
        Some(last) => now_millis.saturating_sub(last) >= (retry_backoff_secs(attempts) as i64) * 1000,
    }
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn drop_expired_outbound(store: &mut impl Store, item_id: &str) -> Result<(), StoreError> {
    if let Some(current) = store.get_outbound_state(item_id)? {
        if TRANSFER_TRANSITIONS.validate(current, TransferState::Expired).is_ok() {
            store.set_outbound_state(item_id, TransferState::Expired)?;
        }
    }
    Ok(())
}

pub fn drain_outbox_for_peer(
    store: &mut impl Store,
    channel: &mut impl Channel,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
    source: &mut impl OutboundSource,
) -> Result<DrainOutcome, DispatchError> {
    drain_outbox_for_peer_reporting(store, channel, clock, peer_id, actor, source, &mut |_, _, _| {})
}

pub fn drain_outbox_for_peer_reporting(
    store: &mut impl Store,
    channel: &mut impl Channel,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
    source: &mut impl OutboundSource,
    on_progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<DrainOutcome, DispatchError> {
    let entries = store.list_outbox_for_peer(peer_id)?;
    let mut outcome = DrainOutcome::default();

    for entry in entries {
        if let Some(deadline) = store.get_outbox_expiry(&entry.item_id)? {
            if clock.is_expired(&deadline) {
                store.remove_outbox_entry(&entry.outbox_id)?;
                drop_expired_outbound(store, &entry.item_id)?;
                store.set_outbound_last_error(
                    &entry.item_id,
                    "the peer did not reappear before the outbox TTL expired",
                )?;
                store.record_outbound_dropped(&entry.item_id, actor, "outbox_ttl")?;
                store.discard_staged_source(&entry.item_id)?;
                outcome.dropped.push(entry.item_id);
                continue;
            }
        }

        if !is_retry_eligible(entry.attempts, entry.last_attempted_at_millis, now_millis()) {
            outcome.deferred.push(entry.item_id);
            continue;
        }

        let item = match store.get_outbound_item(&entry.item_id)? {
            Some(item) => item,
            None => continue,
        };

        store.record_outbox_attempt(&entry.outbox_id)?;

        let reader = match source.open(&entry.item_id) {
            Ok(reader) => reader,
            Err(io_err) => {
                outcome.failed.push((entry.item_id, TransferError::Io(io_err)));
                continue;
            }
        };

        match crate::transfer::send_item_reporting(channel, store, &entry.item_id, &item, reader, on_progress) {
            Ok(()) => {
                store.remove_outbox_entry(&entry.outbox_id)?;
                store.discard_staged_source(&entry.item_id)?;
                outcome.delivered.push(entry.item_id);
            }
            Err(TransferError::DeclinedByReceiver(reason)) => {
                store.remove_outbox_entry(&entry.outbox_id)?;
                store.set_outbound_last_error(
                    &entry.item_id,
                    &format!("the receiver declined the transfer: {reason}"),
                )?;
                store.record_outbound_dropped(&entry.item_id, actor, "declined")?;
                store.discard_staged_source(&entry.item_id)?;
                outcome.dropped.push(entry.item_id);
            }
            Err(err) => {
                store.set_outbound_last_error(&entry.item_id, &err.to_string())?;
                outcome.failed.push((entry.item_id, err));
            }
        }
    }

    for item_id in store.list_open_receipts_for_peer(peer_id)? {
        let opened = ferry_proto::envelope::WireMessage::Opened(ferry_proto::envelope::Opened {
            item_id: ferry_proto::ids::ItemId(item_id.clone()),
        });
        if crate::transfer::send_message(channel, opened).is_ok() {
            store.remove_open_receipt(&item_id)?;
            outcome.receipts_sent.push(item_id);
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::envelope::Offer;
    use ferry_proto::states::{ItemKind, TransferState};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::mpsc::{self, Receiver, Sender};

    struct FakeChannel {
        peer_id: String,
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
    }

    impl Channel for FakeChannel {
        fn send(&mut self, bytes: &[u8]) -> Result<(), crate::ports::ChannelError> {
            self.tx
                .send(bytes.to_vec())
                .map_err(|e| crate::ports::ChannelError(e.to_string()))
        }

        fn recv(&mut self) -> Result<Vec<u8>, crate::ports::ChannelError> {
            self.rx.recv().map_err(|e| crate::ports::ChannelError(e.to_string()))
        }

        fn remote_peer_id(&self) -> &str {
            &self.peer_id
        }
    }

    fn paired_channels(a_id: &str, b_id: &str) -> (FakeChannel, FakeChannel) {
        let (tx_a, rx_b) = mpsc::channel();
        let (tx_b, rx_a) = mpsc::channel();
        (
            FakeChannel { peer_id: b_id.into(), tx: tx_a, rx: rx_a },
            FakeChannel { peer_id: a_id.into(), tx: tx_b, rx: rx_b },
        )
    }

    struct InMemorySource {
        contents: HashMap<String, Vec<u8>>,
        fail_for: Option<String>,
    }

    impl OutboundSource for InMemorySource {
        type Reader = Cursor<Vec<u8>>;

        fn open(&mut self, item_id: &str) -> std::io::Result<Self::Reader> {
            if self.fail_for.as_deref() == Some(item_id) {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing source"));
            }
            Ok(Cursor::new(self.contents.get(item_id).cloned().unwrap_or_default()))
        }
    }

    struct OutboundRecord {
        item: crate::ports::NewOutboundItem,
        state: TransferState,
    }

    struct InboundRecord {
        state: TransferState,
        bytes: Vec<u8>,
    }

    #[derive(Default)]
    struct FakeStore {
        roster: std::collections::HashSet<String>,
        outbound: HashMap<String, OutboundRecord>,
        inbound: HashMap<String, InboundRecord>,
        inbound_expiry: HashMap<String, crate::expiry::ExpiryDeadline>,
        outbox_expiry: HashMap<String, crate::expiry::ExpiryDeadline>,
        outbox: Vec<crate::ports::OutboxItem>,
        attempts: HashMap<String, u32>,
        dropped_notices: Vec<(String, String)>,
        drop_causes: Vec<(String, String)>,
        last_errors: HashMap<String, String>,
        open_receipts: HashMap<String, Vec<String>>,
    }

    impl FakeStore {
        fn last_error_for(&self, item_id: &str) -> Option<String> {
            self.last_errors.get(item_id).cloned()
        }
    }

    impl FakeStore {
        fn with_peer(peer_id: &str) -> Self {
            let mut store = Self::default();
            store.roster.insert(peer_id.to_string());
            store
        }

        fn enqueue(&mut self, item_id: &str, item: crate::ports::NewOutboundItem) {
            self.outbound.insert(
                item_id.to_string(),
                OutboundRecord { item, state: TransferState::Queued },
            );
            self.outbox.push(crate::ports::OutboxItem {
                outbox_id: format!("outbox-{item_id}"),
                item_id: item_id.to_string(),
                peer_id: self.outbound[item_id].item.peer_id.clone(),
                attempts: 0,
                last_attempted_at_millis: None,
            });
        }
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
            unimplemented!("dispatcher tests enqueue directly via FakeStore::enqueue")
        }

        fn get_outbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
            Ok(self.outbound.get(item_id).map(|r| r.state))
        }

        fn set_outbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError> {
            self.outbound
                .get_mut(item_id)
                .ok_or_else(|| StoreError("no such outbound item".into()))?
                .state = state;
            Ok(())
        }

        fn create_inbound(&mut self, offer: &Offer, _peer_id: &str, _actor: &str) -> Result<(), StoreError> {
            self.inbound.entry(offer.item_id.0.clone()).or_insert(InboundRecord {
                state: TransferState::Offered,
                bytes: Vec::new(),
            });
            Ok(())
        }

        fn get_inbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
            Ok(self.inbound.get(item_id).map(|r| r.state))
        }

        fn set_inbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError> {
            self.inbound
                .get_mut(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?
                .state = state;
            Ok(())
        }

        fn inbound_bytes_received(&self, item_id: &str) -> Result<u64, StoreError> {
            Ok(self.inbound.get(item_id).map(|r| r.bytes.len() as u64).unwrap_or(0))
        }

        fn append_inbound_chunk(&mut self, item_id: &str, _seq: u64, bytes: &[u8]) -> Result<(), StoreError> {
            self.inbound
                .get_mut(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?
                .bytes
                .extend_from_slice(bytes);
            Ok(())
        }

        fn inbound_full_hash_so_far(&self, item_id: &str) -> Result<String, StoreError> {
            use sha2::{Digest, Sha256};
            let record = self
                .inbound
                .get(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?;
            let mut hasher = Sha256::new();
            hasher.update(&record.bytes);
            Ok(hex::encode(hasher.finalize()))
        }

        fn finalize_inbound_delivered(&mut self, item_id: &str) -> Result<(), StoreError> {
            self.set_inbound_state(item_id, TransferState::Delivered)
        }

        fn mark_inbound_opened(&mut self, item_id: &str) -> Result<(), StoreError> {
            self.set_inbound_state(item_id, TransferState::Opened)
        }

        fn get_outbound_item(&self, item_id: &str) -> Result<Option<crate::ports::NewOutboundItem>, StoreError> {
            Ok(self.outbound.get(item_id).map(|r| r.item.clone()))
        }

        fn list_outbox_for_peer(&self, peer_id: &str) -> Result<Vec<crate::ports::OutboxItem>, StoreError> {
            Ok(self.outbox.iter().filter(|e| e.peer_id == peer_id).cloned().collect())
        }

        fn record_outbox_attempt(&mut self, outbox_id: &str) -> Result<(), StoreError> {
            *self.attempts.entry(outbox_id.to_string()).or_insert(0) += 1;
            if let Some(entry) = self.outbox.iter_mut().find(|e| e.outbox_id == outbox_id) {
                entry.attempts += 1;
                entry.last_attempted_at_millis = Some(super::now_millis());
            }
            Ok(())
        }

        fn remove_outbox_entry(&mut self, outbox_id: &str) -> Result<(), StoreError> {
            self.outbox.retain(|e| e.outbox_id != outbox_id);
            Ok(())
        }

        fn set_inbound_expiry(
            &mut self,
            item_id: &str,
            deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            self.inbound_expiry.insert(item_id.to_string(), deadline.clone());
            Ok(())
        }

        fn get_inbound_expiry(&self, item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            Ok(self.inbound_expiry.get(item_id).cloned())
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

        fn record_outbound_dropped(&mut self, item_id: &str, actor: &str, cause: &str) -> Result<(), StoreError> {
            self.dropped_notices.push((item_id.to_string(), actor.to_string()));
            self.drop_causes.push((item_id.to_string(), cause.to_string()));
            Ok(())
        }

        fn set_outbound_last_error(&mut self, item_id: &str, reason: &str) -> Result<(), StoreError> {
            self.last_errors.insert(item_id.to_string(), reason.to_string());
            Ok(())
        }

        fn read_inbound_plaintext(&self, item_id: &str) -> Result<Vec<u8>, StoreError> {
            Ok(self
                .inbound
                .get(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?
                .bytes
                .clone())
        }

        fn is_healthy(&self) -> Result<bool, StoreError> {
            Ok(true)
        }

        fn list_roster_peers(&self) -> Result<Vec<crate::ports::RosterPeer>, StoreError> {
            unimplemented!("not needed for dispatcher tests")
        }

        fn export_signed_roster(&self) -> Result<String, StoreError> {
            unimplemented!("not needed for dispatcher tests")
        }

        fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<crate::ports::RosterImportSummary, StoreError> {
            unimplemented!("not needed for dispatcher tests")
        }

        fn add_paired_peer(&mut self, _peer: &crate::ports::NewRosterPeer) -> Result<(), StoreError> {
            unimplemented!("not needed for dispatcher tests")
        }

        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
            unimplemented!("not needed for dispatcher tests")
        }

        fn list_open_receipts_for_peer(&self, peer_id: &str) -> Result<Vec<String>, StoreError> {
            Ok(self.open_receipts.get(peer_id).cloned().unwrap_or_default())
        }

        fn remove_open_receipt(&mut self, item_id: &str) -> Result<(), StoreError> {
            for owed in self.open_receipts.values_mut() {
                owed.retain(|id| id != item_id);
            }
            Ok(())
        }
    }

    fn sample_item(hash: &str, size_bytes: u64) -> crate::ports::NewOutboundItem {
        crate::ports::NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes,
            hash: hash.into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/notes.txt".into(),
        }
    }

    #[test]
    fn drains_a_queued_item_once_the_peer_is_reachable() {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        let hash = hex::encode(hasher.finalize());

        let mut sender_store = FakeStore::with_peer("peer-b");
        sender_store.enqueue("item-1", sample_item(&hash, 3));

        let mut source = InMemorySource {
            contents: HashMap::from([("item-1".to_string(), b"abc".to_vec())]),
            fail_for: None,
        };

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let receiver_thread = std::thread::spawn(move || {
            let mut receiver_store = FakeStore::default();
            let clock = crate::expiry::ExpiryClock::new();
            let offer =
                crate::transfer::receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local")
                    .unwrap();
            (offer, receiver_store)
        });

        let clock = crate::expiry::ExpiryClock::new();
        let outcome = drain_outbox_for_peer(&mut sender_store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        let (offer, receiver_store) = receiver_thread.join().unwrap();
        assert_eq!(outcome.delivered, vec!["item-1".to_string()]);
        assert!(outcome.failed.is_empty());
        assert!(sender_store.list_outbox_for_peer("peer-b").unwrap().is_empty());
        assert_eq!(offer.item_id.0, "item-1");
        assert_eq!(receiver_store.get_inbound_state("item-1").unwrap(), Some(TransferState::Delivered));
    }

    #[test]
    fn a_failed_delivery_leaves_the_entry_queued_for_at_least_once_redelivery() {
        let mut sender_store = FakeStore::with_peer("peer-b");
        sender_store.enqueue("item-1", sample_item("irrelevant", 3));

        let mut source = InMemorySource {
            contents: HashMap::new(),
            fail_for: Some("item-1".to_string()),
        };

        let (mut sender_channel, _receiver_channel) = paired_channels("peer-a", "peer-b");
        let clock = crate::expiry::ExpiryClock::new();
        let outcome = drain_outbox_for_peer(&mut sender_store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        assert!(outcome.delivered.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(sender_store.list_outbox_for_peer("peer-b").unwrap().len(), 1, "a failed send must not lose the item — it stays queued for the next drain attempt");
        assert_eq!(sender_store.attempts.get("outbox-item-1"), Some(&1));
    }

    #[test]
    fn drains_multiple_entries_for_a_peer_in_enqueue_order() {
        use sha2::{Digest, Sha256};

        let hash_of = |bytes: &[u8]| {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            hex::encode(hasher.finalize())
        };

        let mut sender_store = FakeStore::with_peer("peer-b");
        sender_store.enqueue("item-1", sample_item(&hash_of(b"first"), 5));
        sender_store.enqueue("item-2", sample_item(&hash_of(b"second"), 6));

        let mut source = InMemorySource {
            contents: HashMap::from([
                ("item-1".to_string(), b"first".to_vec()),
                ("item-2".to_string(), b"second".to_vec()),
            ]),
            fail_for: None,
        };

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let receiver_thread = std::thread::spawn(move || {
            let mut receiver_store = FakeStore::default();
            let clock = crate::expiry::ExpiryClock::new();
            let mut received = Vec::new();
            for _ in 0..2 {
                let offer =
                    crate::transfer::receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local")
                        .unwrap();
                received.push(offer.item_id.0);
            }
            received
        });

        let clock = crate::expiry::ExpiryClock::new();
        let outcome = drain_outbox_for_peer(&mut sender_store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();
        let received_order = receiver_thread.join().unwrap();

        assert_eq!(outcome.delivered, vec!["item-1".to_string(), "item-2".to_string()]);
        assert_eq!(received_order, vec!["item-1".to_string(), "item-2".to_string()]);
    }

    #[test]
    fn retry_backoff_grows_exponentially_and_caps() {
        assert_eq!(retry_backoff_secs(0), 0);
        assert_eq!(retry_backoff_secs(1), 5);
        assert_eq!(retry_backoff_secs(2), 10);
        assert_eq!(retry_backoff_secs(3), 20);
        assert_eq!(retry_backoff_secs(6), 160);
        assert_eq!(retry_backoff_secs(7), RETRY_MAX_SECS);
        assert_eq!(retry_backoff_secs(64), RETRY_MAX_SECS);
    }

    #[test]
    fn retry_eligibility_respects_the_backoff_window() {
        assert!(is_retry_eligible(0, None, 1_000));
        assert!(!is_retry_eligible(1, Some(1_000), 1_000 + 4_999));
        assert!(is_retry_eligible(1, Some(1_000), 1_000 + 5_000));
        assert!(!is_retry_eligible(3, Some(0), 19_999));
        assert!(is_retry_eligible(3, Some(0), 20_000));
    }

    #[test]
    fn a_recently_attempted_entry_is_deferred_not_hammered() {
        let mut store = FakeStore::with_peer("peer-b");
        store.enqueue("item-1", sample_item("irrelevant", 3));
        if let Some(entry) = store.outbox.iter_mut().find(|e| e.item_id == "item-1") {
            entry.attempts = 2;
            entry.last_attempted_at_millis = Some(super::now_millis());
        }

        let mut source = InMemorySource {
            contents: HashMap::from([("item-1".to_string(), b"abc".to_vec())]),
            fail_for: None,
        };
        let (mut sender_channel, _receiver_channel) = paired_channels("peer-a", "peer-b");
        let clock = crate::expiry::ExpiryClock::new();

        let outcome =
            drain_outbox_for_peer(&mut store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        assert_eq!(outcome.deferred, vec!["item-1".to_string()]);
        assert!(outcome.delivered.is_empty());
        assert!(outcome.failed.is_empty());
        assert_eq!(
            store.attempts.get("outbox-item-1"),
            None,
            "a deferred entry must not even be counted as an attempt"
        );
        assert_eq!(store.list_outbox_for_peer("peer-b").unwrap().len(), 1);
    }

    #[test]
    fn an_item_past_its_outbox_ttl_is_dropped_visibly_instead_of_attempted() {
        let mut store = FakeStore::with_peer("peer-b");
        store.enqueue("item-1", sample_item("irrelevant", 3));
        let clock = crate::expiry::ExpiryClock::new();
        store.set_outbox_expiry("item-1", &clock.compute_deadline(0)).unwrap();

        let mut source = InMemorySource {
            contents: HashMap::from([("item-1".to_string(), b"abc".to_vec())]),
            fail_for: None,
        };
        let (mut sender_channel, _receiver_channel) = paired_channels("peer-a", "peer-b");

        let outcome = drain_outbox_for_peer(&mut store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        assert_eq!(outcome.dropped, vec!["item-1".to_string()]);
        assert!(outcome.delivered.is_empty());
        assert!(outcome.failed.is_empty());
        assert!(
            store.list_outbox_for_peer("peer-b").unwrap().is_empty(),
            "a dropped item must leave the outbox, not linger for a future retry"
        );
        assert_eq!(
            store.get_outbound_state("item-1").unwrap(),
            Some(TransferState::Expired)
        );
        assert_eq!(
            store.dropped_notices,
            vec![("item-1".to_string(), "local".to_string())],
            "a drop must record a durable, visible notice — not just vanish silently"
        );
        assert_eq!(
            store.drop_causes,
            vec![("item-1".to_string(), "outbox_ttl".to_string())],
            "an outbox-TTL drop must be distinguishable from an abort or a decline"
        );
        assert_eq!(
            store.last_error_for("item-1").as_deref(),
            Some("the peer did not reappear before the outbox TTL expired"),
            "the sender must see a human-readable reason for the drop"
        );
    }

    #[test]
    fn an_owed_open_receipt_is_sent_and_cleared_when_the_peer_is_next_reached() {
        use ferry_proto::envelope::WireMessage;

        let mut store = FakeStore::with_peer("peer-b");
        store
            .open_receipts
            .insert("peer-b".to_string(), vec!["item-9".to_string()]);

        let mut source = InMemorySource { contents: HashMap::new(), fail_for: None };
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let clock = crate::expiry::ExpiryClock::new();

        let outcome =
            drain_outbox_for_peer(&mut store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        assert_eq!(outcome.receipts_sent, vec!["item-9".to_string()]);
        assert!(
            store.list_open_receipts_for_peer("peer-b").unwrap().is_empty(),
            "a sent receipt must be removed from the owed queue"
        );

        match crate::transfer::recv_message(&mut receiver_channel).unwrap() {
            WireMessage::Opened(opened) => assert_eq!(opened.item_id.0, "item-9"),
            other => panic!("expected a bare Opened on the wire, got {other:?}"),
        }
    }
}
