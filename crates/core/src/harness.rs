use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use ferry_net::chunk::{Chunker, DEFAULT_CHUNK_BYTES};
use ferry_proto::envelope::Offer;
use ferry_proto::states::{ItemKind, TransferState};
use sha2::{Digest, Sha256};

use crate::expiry::{ExpiryClock, ExpiryDeadline};
use crate::outbox::{enqueue_send, OutboxError};
use crate::policy::PolicyError;
use crate::ports::{Channel, ChannelError, NewOutboundItem, OutboxItem, Store, StoreError};
use crate::state::TRANSFER_TRANSITIONS;
use crate::transfer::{
    open_item, receive_item, recv_message, send_accept, send_chunk, send_item, send_offer,
    TransferError,
};

struct OutboundRecord {
    item: NewOutboundItem,
    state: TransferState,
}

struct InboundRecord {
    state: TransferState,
    bytes: Vec<u8>,
    is_burn_after_read: bool,
}

#[derive(Default)]
pub struct InMemoryStore {
    roster: HashSet<String>,
    outbound: HashMap<String, OutboundRecord>,
    inbound: HashMap<String, InboundRecord>,
    inbound_expiry: HashMap<String, ExpiryDeadline>,
    outbox_expiry: HashMap<String, ExpiryDeadline>,
    outbox: Vec<OutboxItem>,
    outbox_attempts: HashMap<String, u32>,
    next_id: u64,
}

impl InMemoryStore {
    fn with_roster(peers: &[&str]) -> Self {
        let mut store = Self::default();
        for peer in peers {
            store.roster.insert(peer.to_string());
        }
        store
    }

    pub fn add_roster_peer(&mut self, peer_id: &str) {
        self.roster.insert(peer_id.to_string());
    }
}

impl Store for InMemoryStore {
    fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, StoreError> {
        Ok(self.roster.contains(peer_id))
    }

    fn create_and_enqueue_outbound(
        &mut self,
        item: &NewOutboundItem,
        _actor: &str,
    ) -> Result<String, StoreError> {
        self.next_id += 1;
        let id = format!("item-{}", self.next_id);
        self.outbound.insert(
            id.clone(),
            OutboundRecord {
                item: item.clone(),
                state: TransferState::Queued,
            },
        );
        self.outbox.push(OutboxItem {
            outbox_id: format!("outbox-{id}"),
            item_id: id.clone(),
            peer_id: item.peer_id.clone(),
            attempts: 0,
            last_attempted_at_millis: None,
        });
        Ok(id)
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
            is_burn_after_read: offer.burn_after_read,
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
        let record = self
            .inbound
            .get_mut(item_id)
            .ok_or_else(|| StoreError("no such inbound item".into()))?;
        if record.is_burn_after_read {
            record.bytes.clear();
        }
        record.state = TransferState::Opened;
        Ok(())
    }

    fn get_outbound_item(&self, item_id: &str) -> Result<Option<NewOutboundItem>, StoreError> {
        Ok(self.outbound.get(item_id).map(|r| r.item.clone()))
    }

    fn list_outbox_for_peer(&self, peer_id: &str) -> Result<Vec<OutboxItem>, StoreError> {
        Ok(self.outbox.iter().filter(|e| e.peer_id == peer_id).cloned().collect())
    }

    fn record_outbox_attempt(&mut self, outbox_id: &str) -> Result<(), StoreError> {
        *self.outbox_attempts.entry(outbox_id.to_string()).or_insert(0) += 1;
        Ok(())
    }

    fn remove_outbox_entry(&mut self, outbox_id: &str) -> Result<(), StoreError> {
        self.outbox.retain(|e| e.outbox_id != outbox_id);
        Ok(())
    }

    fn set_inbound_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError> {
        self.inbound_expiry.insert(item_id.to_string(), deadline.clone());
        Ok(())
    }

    fn get_inbound_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError> {
        Ok(self.inbound_expiry.get(item_id).cloned())
    }

    fn set_outbox_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError> {
        self.outbox_expiry.insert(item_id.to_string(), deadline.clone());
        Ok(())
    }

    fn get_outbox_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError> {
        Ok(self.outbox_expiry.get(item_id).cloned())
    }

    fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
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
        unimplemented!("not needed for harness cases")
    }

    fn export_signed_roster(&self) -> Result<String, StoreError> {
        unimplemented!("not needed for harness cases")
    }

    fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<crate::ports::RosterImportSummary, StoreError> {
        unimplemented!("not needed for harness cases")
    }

    fn add_paired_peer(&mut self, _peer: &crate::ports::NewRosterPeer) -> Result<(), StoreError> {
        unimplemented!("not needed for harness cases")
    }

    fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
        unimplemented!("not needed for harness cases")
    }
}

struct HarnessChannel {
    peer_id: String,
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    partitioned: Arc<AtomicBool>,
}

impl Channel for HarnessChannel {
    fn send(&mut self, bytes: &[u8]) -> Result<(), ChannelError> {
        if self.partitioned.load(Ordering::SeqCst) {
            return Err(ChannelError("partitioned — peer unreachable".into()));
        }
        self.tx.send(bytes.to_vec()).map_err(|e| ChannelError(e.to_string()))
    }

    fn recv(&mut self) -> Result<Vec<u8>, ChannelError> {
        if self.partitioned.load(Ordering::SeqCst) {
            return Err(ChannelError("partitioned — peer unreachable".into()));
        }
        self.rx.recv().map_err(|e| ChannelError(e.to_string()))
    }

    fn remote_peer_id(&self) -> &str {
        &self.peer_id
    }
}

fn network(a_id: &str, b_id: &str) -> (HarnessChannel, HarnessChannel, Arc<AtomicBool>) {
    let (tx_a, rx_b) = mpsc::channel();
    let (tx_b, rx_a) = mpsc::channel();
    let partitioned = Arc::new(AtomicBool::new(false));
    (
        HarnessChannel {
            peer_id: b_id.into(),
            tx: tx_a,
            rx: rx_a,
            partitioned: partitioned.clone(),
        },
        HarnessChannel {
            peer_id: a_id.into(),
            tx: tx_b,
            rx: rx_b,
            partitioned: partitioned.clone(),
        },
        partitioned,
    )
}

fn sample_item(hash: &str, size_bytes: u64) -> NewOutboundItem {
    NewOutboundItem {
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
fn send_while_receiver_offline_stays_queued_then_delivers_once_reachable() {
    let mut sender_store = InMemoryStore::with_roster(&["peer-b"]);
    let mut hasher = Sha256::new();
    hasher.update(b"abc");
    let item = sample_item(&hex::encode(hasher.finalize()), 3);
    let sender_clock = ExpiryClock::new();
    let item_id = enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();
    assert_eq!(
        sender_store.get_outbound_state(&item_id).unwrap(),
        Some(TransferState::Queued),
        "an item enqueued while the peer is offline must sit in Queued, not fail"
    );

    let (mut sender_channel, mut receiver_channel, partition) = network("peer-a", "peer-b");
    partition.store(true, Ordering::SeqCst);

    let offline_attempt = send_item(
        &mut sender_channel,
        &mut sender_store,
        &item_id,
        &item,
        Cursor::new(b"abc".to_vec()),
    );
    assert!(offline_attempt.is_err(), "sending while partitioned must fail, not silently succeed");

    partition.store(false, Ordering::SeqCst);

    let mut receiver_store = InMemoryStore::default();
    let receiver_thread = std::thread::spawn(move || {
        let clock = ExpiryClock::new();
        let offer = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        (offer, receiver_store)
    });

    send_item(
        &mut sender_channel,
        &mut sender_store,
        &item_id,
        &item,
        Cursor::new(b"abc".to_vec()),
    )
    .unwrap();

    let (offer, receiver_store) = receiver_thread.join().unwrap();

    assert_eq!(offer.item_id.0, item_id);
    assert_eq!(sender_store.get_outbound_state(&item_id).unwrap(), Some(TransferState::Delivered));
    assert_eq!(receiver_store.get_inbound_state(&item_id).unwrap(), Some(TransferState::Delivered));
}

#[test]
fn disconnect_mid_transfer_resumes_from_the_correct_offset_not_from_zero() {
    let payload: Vec<u8> = (0..50_000u32).map(|n| (n % 256) as u8).collect();
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let expected_hash = hex::encode(hasher.finalize());
    let mut item = sample_item(&expected_hash, payload.len() as u64);
    item.hash = expected_hash;

    let mut receiver_store = InMemoryStore::default();

    {
        let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let item_first = item.clone();
        let sender_thread = std::thread::spawn(move || {
            send_offer(&mut sender_channel, "item-1", &item_first).unwrap();
            recv_message(&mut sender_channel).unwrap();
            for chunk in Chunker::new(Cursor::new(sender_payload), DEFAULT_CHUNK_BYTES, 0).take(1) {
                let (seq, bytes) = chunk.unwrap();
                send_chunk(&mut sender_channel, "item-1", seq, bytes).unwrap();
            }
        });

        let offer = match recv_message(&mut receiver_channel).unwrap() {
            ferry_proto::envelope::WireMessage::Offer(offer) => offer,
            other => panic!("expected Offer, got {other:?}"),
        };
        receiver_store.create_inbound(&offer, "peer-a", "local").unwrap();
        let offset = receiver_store.inbound_bytes_received("item-1").unwrap();
        TRANSFER_TRANSITIONS.validate(TransferState::Offered, TransferState::Accepted).unwrap();
        receiver_store.set_inbound_state("item-1", TransferState::Accepted).unwrap();
        TRANSFER_TRANSITIONS.validate(TransferState::Accepted, TransferState::Transferring).unwrap();
        receiver_store.set_inbound_state("item-1", TransferState::Transferring).unwrap();
        send_accept(&mut receiver_channel, "item-1", offset).unwrap();

        match recv_message(&mut receiver_channel).unwrap() {
            ferry_proto::envelope::WireMessage::Chunk(chunk) => {
                receiver_store.append_inbound_chunk("item-1", chunk.seq, &chunk.bytes).unwrap();
            }
            other => panic!("expected Chunk, got {other:?}"),
        }
        sender_thread.join().unwrap();
    }

    let partial = receiver_store.inbound_bytes_received("item-1").unwrap();
    assert!(partial > 0 && partial < payload.len() as u64);

    let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
    let mut sender_store = InMemoryStore::default();
    sender_store.outbound.insert(
        "item-1".into(),
        OutboundRecord { item: item.clone(), state: TransferState::Transferring },
    );
    let resumed_item = item.clone();
    let sender_payload = payload.clone();
    let sender_thread = std::thread::spawn(move || {
        send_item(&mut sender_channel, &mut sender_store, "item-1", &resumed_item, Cursor::new(sender_payload)).unwrap();
        sender_store
    });

    let clock = ExpiryClock::new();
    receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
    let sender_store = sender_thread.join().unwrap();

    assert_eq!(receiver_store.inbound.get("item-1").unwrap().bytes, payload);
    assert_eq!(sender_store.get_outbound_state("item-1").unwrap(), Some(TransferState::Delivered));
}

#[test]
fn daemon_restart_mid_transfer_on_either_side_recovers_correctly() {
    let payload: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let expected_hash = hex::encode(hasher.finalize());
    let mut item = sample_item(&expected_hash, payload.len() as u64);
    item.hash = expected_hash;

    let mut receiver_store = InMemoryStore::default();
    let mut sender_store = InMemoryStore::default();
    sender_store.outbound.insert(
        "item-1".into(),
        OutboundRecord { item: item.clone(), state: TransferState::Queued },
    );

    {
        let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let item_first = item.clone();
        let sender_thread = std::thread::spawn(move || {
            send_offer(&mut sender_channel, "item-1", &item_first).unwrap();
            recv_message(&mut sender_channel).unwrap();
            for chunk in Chunker::new(Cursor::new(sender_payload), DEFAULT_CHUNK_BYTES, 0).take(1) {
                let (seq, bytes) = chunk.unwrap();
                send_chunk(&mut sender_channel, "item-1", seq, bytes).unwrap();
            }
        });

        let offer = match recv_message(&mut receiver_channel).unwrap() {
            ferry_proto::envelope::WireMessage::Offer(offer) => offer,
            other => panic!("expected Offer, got {other:?}"),
        };
        receiver_store.create_inbound(&offer, "peer-a", "local").unwrap();
        receiver_store.set_inbound_state("item-1", TransferState::Accepted).unwrap();
        receiver_store.set_inbound_state("item-1", TransferState::Transferring).unwrap();
        send_accept(&mut receiver_channel, "item-1", 0).unwrap();
        match recv_message(&mut receiver_channel).unwrap() {
            ferry_proto::envelope::WireMessage::Chunk(chunk) => {
                receiver_store.append_inbound_chunk("item-1", chunk.seq, &chunk.bytes).unwrap();
            }
            other => panic!("expected Chunk, got {other:?}"),
        }
        sender_store.set_outbound_state("item-1", TransferState::Offered).unwrap();
        sender_store.set_outbound_state("item-1", TransferState::Accepted).unwrap();
        sender_store.set_outbound_state("item-1", TransferState::Transferring).unwrap();
        sender_thread.join().unwrap();
    }

    let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
    let resumed_item = item.clone();
    let sender_payload = payload.clone();
    let sender_thread = std::thread::spawn(move || {
        send_item(&mut sender_channel, &mut sender_store, "item-1", &resumed_item, Cursor::new(sender_payload)).unwrap();
        sender_store
    });
    let clock = ExpiryClock::new();
    receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
    let sender_store = sender_thread.join().unwrap();

    assert_eq!(receiver_store.inbound.get("item-1").unwrap().bytes, payload);
    assert_eq!(sender_store.get_outbound_state("item-1").unwrap(), Some(TransferState::Delivered));
}

#[test]
fn burn_after_read_second_open_fails() {
    let mut store = InMemoryStore::default();
    let offer = Offer {
        item_id: ferry_proto::ids::ItemId("item-1".into()),
        kind: ItemKind::Secret,
        name: ".env".into(),
        size_bytes: 3,
        hash: "irrelevant".into(),
        ttl_secs: 60,
        burn_after_read: true,
        notify_on_open: false,
    };
    store.create_inbound(&offer, "peer-a", "local").unwrap();
    store.append_inbound_chunk("item-1", 0, b"abc").unwrap();
    store.set_inbound_state("item-1", TransferState::Accepted).unwrap();
    store.set_inbound_state("item-1", TransferState::Transferring).unwrap();
    store.finalize_inbound_delivered("item-1").unwrap();

    let (mut channel, peer_channel, _partition) = network("peer-b", "peer-a");
    let drain = std::thread::spawn(move || {
        let mut peer_channel = peer_channel;
        let mut count = 0;
        while recv_message(&mut peer_channel).is_ok() {
            count += 1;
        }
        count
    });

    let clock = ExpiryClock::new();
    open_item(&mut channel, &mut store, &clock, "item-1").unwrap();
    assert!(store.inbound.get("item-1").unwrap().bytes.is_empty());

    let second = open_item(&mut channel, &mut store, &clock, "item-1");
    assert!(matches!(second, Err(TransferError::IllegalTransition(_))));

    drop(channel);
    assert_eq!(drain.join().unwrap(), 1);
}

#[test]
fn unpaired_peer_is_rejected_before_any_storage_write() {
    let mut store = InMemoryStore::default();
    let clock = ExpiryClock::new();
    let result = enqueue_send(&mut store, &clock, &sample_item("deadbeef", 3), "local");
    assert!(matches!(result, Err(OutboxError::Policy(PolicyError::PeerNotAuthorized))));
    assert!(store.outbound.is_empty(), "policy must reject before any outbound row exists");
}

#[test]
fn an_item_stays_readable_for_its_full_ttl_from_delivery_then_expires() {
    let payload = b"abc".to_vec();
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let hash = hex::encode(hasher.finalize());

    let mut item = sample_item(&hash, payload.len() as u64);
    item.ttl_secs = 3600;

    let mut sender_store = InMemoryStore::with_roster(&["peer-b"]);
    let sender_clock = ExpiryClock::new();
    let item_id = enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();

    let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
    let sender_payload = payload.clone();
    let sender_item_id = item_id.clone();
    let sender_thread = std::thread::spawn(move || {
        send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload))
    });

    let mut receiver_store = InMemoryStore::default();
    let clock = ExpiryClock::new();
    receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
    sender_thread.join().unwrap().unwrap();

    let (mut open_channel, _peer_channel, _partition2) = network("peer-b", "peer-a");
    open_item(&mut open_channel, &mut receiver_store, &clock, &item_id).unwrap();
    assert_eq!(
        receiver_store.get_inbound_state(&item_id).unwrap(),
        Some(TransferState::Opened),
        "an item well within its TTL must still be openable"
    );
}

#[test]
fn an_item_delivered_with_an_already_elapsed_ttl_cannot_be_opened() {
    let payload = b"abc".to_vec();
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let hash = hex::encode(hasher.finalize());

    let mut item = sample_item(&hash, payload.len() as u64);
    item.ttl_secs = 0;

    let mut sender_store = InMemoryStore::with_roster(&["peer-b"]);
    let sender_clock = ExpiryClock::new();
    let item_id = enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();

    let (mut sender_channel, mut receiver_channel, _partition) = network("peer-a", "peer-b");
    let sender_payload = payload.clone();
    let sender_item_id = item_id.clone();
    let sender_thread = std::thread::spawn(move || {
        send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload))
    });

    let mut receiver_store = InMemoryStore::default();
    let clock = ExpiryClock::new();
    receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
    sender_thread.join().unwrap().unwrap();

    let (mut open_channel, _peer_channel, _partition2) = network("peer-b", "peer-a");
    let result = open_item(&mut open_channel, &mut receiver_store, &clock, &item_id);
    assert!(matches!(result, Err(TransferError::Expired(_))));
    assert_eq!(receiver_store.get_inbound_state(&item_id).unwrap(), Some(TransferState::Expired));
}

#[test]
fn outbox_ttl_expiry_edge_remains_reachable_from_queued() {
    assert!(
        TRANSFER_TRANSITIONS.validate(TransferState::Queued, TransferState::Expired).is_ok(),
        "an item still Queued (never offered because the peer never reappeared) must \
         remain expirable — real outbox-TTL arithmetic and the visible drop notice are \
         dispatcher work for a later phase"
    );
}
