use std::io::{Read, Seek, SeekFrom};

use ferry_net::chunk::{resume_seq_from_offset, Chunker, DEFAULT_CHUNK_BYTES};
use ferry_proto::envelope::{
    Accept, Chunk as ChunkMsg, Delivered, Done, Envelope, Offer, Opened, Reject, WireMessage,
    PROTOCOL_VERSION,
};
use ferry_proto::ids::ItemId;
use ferry_proto::states::TransferState;
use thiserror::Error;

use crate::expiry::ExpiryClock;
use crate::ports::{Channel, ChannelError, NewOutboundItem, Store, StoreError};
use crate::state::{TransitionError, TRANSFER_TRANSITIONS};

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("channel error: {0}")]
    Channel(#[from] ChannelError),
    #[error("failed to encode wire message: {0}")]
    Encode(String),
    #[error("failed to decode wire message: {0}")]
    Decode(String),
    #[error("protocol version mismatch: local={local}, remote={remote}")]
    ProtocolVersionMismatch { local: u16, remote: u16 },
    #[error("received an unexpected message: {0}")]
    UnexpectedMessage(String),
    #[error("hash mismatch — expected {expected}, computed {computed}")]
    HashMismatch { expected: String, computed: String },
    #[error("local I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("illegal state transition: {0}")]
    IllegalTransition(#[from] TransitionError<TransferState>),
    #[error("unknown item: {0}")]
    UnknownItem(String),
    #[error("item {0} has already been opened")]
    AlreadyOpened(String),
    #[error("item {0} has expired")]
    Expired(String),
    #[error("peer sent more data than the declared size of {declared} bytes for item {item_id}")]
    DeclaredSizeExceeded { item_id: String, declared: u64 },
    #[error("inbound item rejected by policy: {0}")]
    Policy(#[from] crate::policy::PolicyError),
    #[error("the receiver declined item {0}")]
    DeclinedByReceiver(String),
    #[error("item {0} is awaiting a local accept/reject decision")]
    AwaitingDecision(String),
}

pub fn send_message(channel: &mut impl Channel, message: WireMessage) -> Result<(), TransferError> {
    let envelope = Envelope {
        protocol_version: PROTOCOL_VERSION,
        message,
    };
    let bytes = bincode::serialize(&envelope).map_err(|e| TransferError::Encode(e.to_string()))?;
    channel.send(&bytes)?;
    Ok(())
}

pub fn recv_message(channel: &mut impl Channel) -> Result<WireMessage, TransferError> {
    let bytes = channel.recv()?;
    let envelope: Envelope =
        bincode::deserialize(&bytes).map_err(|e| TransferError::Decode(e.to_string()))?;

    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(TransferError::ProtocolVersionMismatch {
            local: PROTOCOL_VERSION,
            remote: envelope.protocol_version,
        });
    }

    Ok(envelope.message)
}

pub fn send_offer(
    channel: &mut impl Channel,
    item_id: &str,
    item: &NewOutboundItem,
) -> Result<(), TransferError> {
    let offer = Offer {
        item_id: ItemId(item_id.to_string()),
        kind: item.kind,
        name: item.name.clone(),
        size_bytes: item.size_bytes,
        hash: item.hash.clone(),
        ttl_secs: item.ttl_secs,
        burn_after_read: item.is_burn_after_read,
        notify_on_open: item.notify_on_open,
    };
    send_message(channel, WireMessage::Offer(offer))
}

pub fn send_accept(
    channel: &mut impl Channel,
    item_id: &str,
    offset: u64,
) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Accept(Accept {
            item_id: ItemId(item_id.to_string()),
            offset,
        }),
    )
}

pub fn send_chunk(
    channel: &mut impl Channel,
    item_id: &str,
    seq: u64,
    bytes: Vec<u8>,
) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Chunk(ChunkMsg {
            item_id: ItemId(item_id.to_string()),
            seq,
            bytes,
        }),
    )
}

fn send_done(channel: &mut impl Channel, item_id: &str, hash: &str) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Done(Done {
            item_id: ItemId(item_id.to_string()),
            hash: hash.to_string(),
        }),
    )
}

pub fn send_delivered(channel: &mut impl Channel, item_id: &str) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Delivered(Delivered {
            item_id: ItemId(item_id.to_string()),
        }),
    )
}

pub fn send_opened(channel: &mut impl Channel, item_id: &str) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Opened(Opened {
            item_id: ItemId(item_id.to_string()),
        }),
    )
}

pub fn send_reject(channel: &mut impl Channel, item_id: &str) -> Result<(), TransferError> {
    send_message(
        channel,
        WireMessage::Reject(Reject {
            item_id: ItemId(item_id.to_string()),
        }),
    )
}

fn transition_outbound(
    store: &mut impl Store,
    item_id: &str,
    to: TransferState,
) -> Result<(), TransferError> {
    let current = store
        .get_outbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    TRANSFER_TRANSITIONS.validate(current, to)?;
    store.set_outbound_state(item_id, to)?;
    Ok(())
}

pub(crate) fn transition_inbound(
    store: &mut impl Store,
    item_id: &str,
    to: TransferState,
) -> Result<(), TransferError> {
    let current = store
        .get_inbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    TRANSFER_TRANSITIONS.validate(current, to)?;
    store.set_inbound_state(item_id, to)?;
    Ok(())
}

pub fn send_item(
    channel: &mut impl Channel,
    store: &mut impl Store,
    item_id: &str,
    item: &NewOutboundItem,
    source: impl Read + Seek,
) -> Result<(), TransferError> {
    send_item_reporting(channel, store, item_id, item, source, &mut |_, _, _| {})
}

pub fn send_item_reporting(
    channel: &mut impl Channel,
    store: &mut impl Store,
    item_id: &str,
    item: &NewOutboundItem,
    mut source: impl Read + Seek,
    on_progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<(), TransferError> {
    let starting_state = store
        .get_outbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    if starting_state == TransferState::Queued {
        transition_outbound(store, item_id, TransferState::Offered)?;
    }

    send_offer(channel, item_id, item)?;

    let accept = match recv_message(channel)? {
        WireMessage::Accept(accept) if accept.item_id.0 == item_id => accept,
        WireMessage::Reject(reject) if reject.item_id.0 == item_id => {
            let current = store
                .get_outbound_state(item_id)?
                .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
            if TRANSFER_TRANSITIONS.validate(current, TransferState::Failed).is_ok() {
                store.set_outbound_state(item_id, TransferState::Failed)?;
            }
            return Err(TransferError::DeclinedByReceiver(item_id.to_string()));
        }
        other => return Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
    };

    let state_before_chunks = store
        .get_outbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    if state_before_chunks == TransferState::Offered {
        transition_outbound(store, item_id, TransferState::Accepted)?;
        transition_outbound(store, item_id, TransferState::Transferring)?;
    } else if state_before_chunks != TransferState::Transferring {
        TRANSFER_TRANSITIONS.validate(state_before_chunks, TransferState::Transferring)?;
    }

    let resume_seq = resume_seq_from_offset(accept.offset, DEFAULT_CHUNK_BYTES);
    source.seek(SeekFrom::Start(accept.offset))?;

    let mut sent = accept.offset;
    for chunk in Chunker::new(source, DEFAULT_CHUNK_BYTES, resume_seq) {
        let (seq, bytes) = chunk?;
        let n = bytes.len() as u64;
        send_chunk(channel, item_id, seq, bytes)?;
        sent += n;
        on_progress(item_id, sent, item.size_bytes);
    }
    send_done(channel, item_id, &item.hash)?;

    match recv_message(channel)? {
        WireMessage::Delivered(delivered) if delivered.item_id.0 == item_id => {
            transition_outbound(store, item_id, TransferState::Delivered)?;
        }
        other => return Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
    }

    if item.notify_on_open {
        match recv_message(channel)? {
            WireMessage::Opened(opened) if opened.item_id.0 == item_id => {
                transition_outbound(store, item_id, TransferState::Opened)?;
            }
            other => return Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
        }
    }

    Ok(())
}

pub fn receive_item(
    channel: &mut impl Channel,
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
) -> Result<Offer, TransferError> {
    receive_item_reporting(channel, store, clock, peer_id, actor, &mut |_, _, _| {})
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundEvent {
    ItemDelivered(Offer),
    OfferPending(Offer),
    OfferDeclined(String),
    OpenReceipt(String),
}

enum OfferedOutcome {
    Delivered(Offer),
    Pending(Offer),
    Declined(String),
}

pub fn receive_next_inbound(
    channel: &mut impl Channel,
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
    auto_accept: bool,
    on_progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<InboundEvent, TransferError> {
    match recv_message(channel)? {
        WireMessage::Offer(offer) => {
            match receive_offered_item(channel, store, clock, peer_id, actor, offer, auto_accept, on_progress)? {
                OfferedOutcome::Delivered(offer) => Ok(InboundEvent::ItemDelivered(offer)),
                OfferedOutcome::Pending(offer) => Ok(InboundEvent::OfferPending(offer)),
                OfferedOutcome::Declined(item_id) => Ok(InboundEvent::OfferDeclined(item_id)),
            }
        }
        WireMessage::Opened(opened) => {
            let item_id = opened.item_id.0;
            match store.get_outbound_state(&item_id)? {
                Some(TransferState::Opened) => {}
                Some(_) => transition_outbound(store, &item_id, TransferState::Opened)?,
                None => return Err(TransferError::UnknownItem(item_id)),
            }
            Ok(InboundEvent::OpenReceipt(item_id))
        }
        other => Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
    }
}

pub fn receive_item_reporting(
    channel: &mut impl Channel,
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
    on_progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<Offer, TransferError> {
    let offer = match recv_message(channel)? {
        WireMessage::Offer(offer) => offer,
        other => return Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
    };
    match receive_offered_item(channel, store, clock, peer_id, actor, offer, true, on_progress)? {
        OfferedOutcome::Delivered(offer) => Ok(offer),
        OfferedOutcome::Pending(offer) => Err(TransferError::AwaitingDecision(offer.item_id.0)),
        OfferedOutcome::Declined(item_id) => Err(TransferError::DeclinedByReceiver(item_id)),
    }
}

#[allow(clippy::too_many_arguments)]
fn receive_offered_item(
    channel: &mut impl Channel,
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id: &str,
    actor: &str,
    offer: Offer,
    auto_accept: bool,
    on_progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<OfferedOutcome, TransferError> {
    let item_id = offer.item_id.0.clone();

    crate::policy::authorize_item_kind(offer.kind, offer.size_bytes)?;

    store.create_inbound(&offer, peer_id, actor)?;

    let current = store
        .get_inbound_state(&item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.clone()))?;
    match current {
        TransferState::Offered => {
            if !auto_accept {
                return Ok(OfferedOutcome::Pending(offer));
            }
            transition_inbound(store, &item_id, TransferState::Accepted)?;
            transition_inbound(store, &item_id, TransferState::Transferring)?;
        }
        TransferState::Accepted => {
            transition_inbound(store, &item_id, TransferState::Transferring)?;
        }
        TransferState::Transferring => {}
        TransferState::Failed => {
            send_reject(channel, &item_id)?;
            return Ok(OfferedOutcome::Declined(item_id));
        }
        other => {
            TRANSFER_TRANSITIONS.validate(other, TransferState::Transferring)?;
        }
    }

    let offset = store.inbound_bytes_received(&item_id)?;
    send_accept(channel, &item_id, offset)?;

    loop {
        match recv_message(channel)? {
            WireMessage::Chunk(chunk) if chunk.item_id.0 == item_id => {
                let already_received = store.inbound_bytes_received(&item_id)?;
                let incoming = chunk.bytes.len() as u64;
                if already_received.saturating_add(incoming) > offer.size_bytes {
                    store.set_inbound_state(&item_id, TransferState::Failed)?;
                    return Err(TransferError::DeclaredSizeExceeded {
                        item_id: item_id.clone(),
                        declared: offer.size_bytes,
                    });
                }
                store.append_inbound_chunk(&item_id, chunk.seq, &chunk.bytes)?;
                let received = store.inbound_bytes_received(&item_id)?;
                on_progress(&item_id, received, offer.size_bytes);
            }
            WireMessage::Done(done) if done.item_id.0 == item_id => {
                let computed = store.inbound_full_hash_so_far(&item_id)?;
                if computed != done.hash {
                    store.set_inbound_state(&item_id, TransferState::Failed)?;
                    return Err(TransferError::HashMismatch {
                        expected: done.hash,
                        computed,
                    });
                }
                break;
            }
            other => return Err(TransferError::UnexpectedMessage(format!("{other:?}"))),
        }
    }

    let current = store
        .get_inbound_state(&item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.clone()))?;
    TRANSFER_TRANSITIONS.validate(current, TransferState::Delivered)?;
    store.finalize_inbound_delivered(&item_id)?;
    store.set_inbound_expiry(&item_id, &clock.compute_deadline(offer.ttl_secs))?;
    send_delivered(channel, &item_id)?;
    Ok(OfferedOutcome::Delivered(offer))
}

pub fn expire_if_due(
    store: &mut impl Store,
    clock: &ExpiryClock,
    item_id: &str,
) -> Result<bool, TransferError> {
    let Some(deadline) = store.get_inbound_expiry(item_id)? else {
        return Ok(false);
    };
    if !clock.is_expired(&deadline) {
        return Ok(false);
    }

    let current = store
        .get_inbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    if current == TransferState::Expired {
        return Ok(true);
    }
    if TRANSFER_TRANSITIONS.validate(current, TransferState::Expired).is_err() {
        return Ok(false);
    }

    store.set_inbound_state(item_id, TransferState::Expired)?;
    Ok(true)
}

pub fn check_openable(
    store: &mut impl Store,
    clock: &ExpiryClock,
    item_id: &str,
) -> Result<(), TransferError> {
    if expire_if_due(store, clock, item_id)? {
        return Err(TransferError::Expired(item_id.to_string()));
    }
    let current = store
        .get_inbound_state(item_id)?
        .ok_or_else(|| TransferError::UnknownItem(item_id.to_string()))?;
    if current == TransferState::Opened {
        return Err(TransferError::AlreadyOpened(item_id.to_string()));
    }
    TRANSFER_TRANSITIONS.validate(current, TransferState::Opened)?;
    Ok(())
}

pub fn open_item_locally(
    store: &mut impl Store,
    clock: &ExpiryClock,
    item_id: &str,
) -> Result<(), TransferError> {
    check_openable(store, clock, item_id)?;
    Ok(store.mark_inbound_opened(item_id)?)
}

pub fn accept_inbound_offer(store: &mut impl Store, item_id: &str) -> Result<(), TransferError> {
    transition_inbound(store, item_id, TransferState::Accepted)
}

pub fn reject_inbound_offer(store: &mut impl Store, item_id: &str) -> Result<(), TransferError> {
    transition_inbound(store, item_id, TransferState::Failed)
}

pub fn open_item(
    channel: &mut impl Channel,
    store: &mut impl Store,
    clock: &ExpiryClock,
    item_id: &str,
) -> Result<(), TransferError> {
    open_item_locally(store, clock, item_id)?;
    send_opened(channel, item_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::states::ItemKind;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::mpsc::{self, Receiver, Sender};

    struct FakeChannel {
        peer_id: String,
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
    }

    impl Channel for FakeChannel {
        fn send(&mut self, bytes: &[u8]) -> Result<(), ChannelError> {
            self.tx
                .send(bytes.to_vec())
                .map_err(|e| ChannelError(e.to_string()))
        }

        fn recv(&mut self) -> Result<Vec<u8>, ChannelError> {
            self.rx.recv().map_err(|e| ChannelError(e.to_string()))
        }

        fn remote_peer_id(&self) -> &str {
            &self.peer_id
        }
    }

    fn paired_channels(a_id: &str, b_id: &str) -> (FakeChannel, FakeChannel) {
        let (tx_a, rx_b) = mpsc::channel();
        let (tx_b, rx_a) = mpsc::channel();
        (
            FakeChannel {
                peer_id: b_id.into(),
                tx: tx_a,
                rx: rx_a,
            },
            FakeChannel {
                peer_id: a_id.into(),
                tx: tx_b,
                rx: rx_b,
            },
        )
    }

    struct FakeInboundItem {
        state: TransferState,
        bytes: Vec<u8>,
        is_burn_after_read: bool,
    }

    #[derive(Default)]
    struct FakeStore {
        outbound: HashMap<String, TransferState>,
        inbound: HashMap<String, FakeInboundItem>,
        inbound_expiry: HashMap<String, crate::expiry::ExpiryDeadline>,
    }

    impl FakeStore {
        fn with_outbound(item_id: &str, state: TransferState) -> Self {
            let mut store = Self::default();
            store.outbound.insert(item_id.to_string(), state);
            store
        }
    }

    impl Store for FakeStore {
        fn is_peer_authorized(&self, _peer_id: &str) -> Result<bool, StoreError> {
            Ok(true)
        }

        fn create_and_enqueue_outbound(
            &mut self,
            _item: &NewOutboundItem,
            _actor: &str,
        ) -> Result<String, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn get_outbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
            Ok(self.outbound.get(item_id).copied())
        }

        fn set_outbound_state(
            &mut self,
            item_id: &str,
            state: TransferState,
        ) -> Result<(), StoreError> {
            self.outbound.insert(item_id.to_string(), state);
            Ok(())
        }

        fn create_inbound(
            &mut self,
            offer: &Offer,
            _peer_id: &str,
            _actor: &str,
        ) -> Result<(), StoreError> {
            self.inbound.entry(offer.item_id.0.clone()).or_insert(FakeInboundItem {
                state: TransferState::Offered,
                bytes: Vec::new(),
                is_burn_after_read: offer.burn_after_read,
            });
            Ok(())
        }

        fn get_inbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
            Ok(self.inbound.get(item_id).map(|i| i.state))
        }

        fn set_inbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError> {
            self.inbound
                .get_mut(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?
                .state = state;
            Ok(())
        }

        fn inbound_bytes_received(&self, item_id: &str) -> Result<u64, StoreError> {
            Ok(self.inbound.get(item_id).map(|i| i.bytes.len() as u64).unwrap_or(0))
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
            let item = self
                .inbound
                .get(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?;
            let mut hasher = Sha256::new();
            hasher.update(&item.bytes);
            Ok(hex::encode(hasher.finalize()))
        }

        fn finalize_inbound_delivered(&mut self, item_id: &str) -> Result<(), StoreError> {
            self.set_inbound_state(item_id, TransferState::Delivered)
        }

        fn mark_inbound_opened(&mut self, item_id: &str) -> Result<(), StoreError> {
            let item = self
                .inbound
                .get_mut(item_id)
                .ok_or_else(|| StoreError("no such inbound item".into()))?;
            if item.is_burn_after_read {
                item.bytes.clear();
            }
            item.state = TransferState::Opened;
            Ok(())
        }

        fn get_outbound_item(&self, _item_id: &str) -> Result<Option<NewOutboundItem>, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn list_outbox_for_peer(&self, _peer_id: &str) -> Result<Vec<crate::ports::OutboxItem>, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn record_outbox_attempt(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn remove_outbox_entry(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for transfer tests")
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
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn get_outbox_expiry(&self, _item_id: &str) -> Result<Option<crate::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
            unimplemented!("not needed for transfer tests")
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
            unimplemented!("not needed for transfer tests")
        }

        fn export_signed_roster(&self) -> Result<String, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<crate::ports::RosterImportSummary, StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn add_paired_peer(&mut self, _peer: &crate::ports::NewRosterPeer) -> Result<(), StoreError> {
            unimplemented!("not needed for transfer tests")
        }

        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, StoreError> {
            unimplemented!("not needed for transfer tests")
        }
    }

    fn sample_item() -> NewOutboundItem {
        NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 4096,
            hash: "deadbeef".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/notes.txt".into(),
        }
    }

    #[test]
    fn send_offer_round_trips_through_a_fake_channel() {
        let (mut a, mut b) = paired_channels("peer-a", "peer-b");
        send_offer(&mut a, "item-1", &sample_item()).unwrap();

        match recv_message(&mut b).unwrap() {
            WireMessage::Offer(offer) => {
                assert_eq!(offer.item_id.0, "item-1");
                assert_eq!(offer.name, "notes.txt");
                assert_eq!(offer.size_bytes, 4096);
                assert!(!offer.burn_after_read);
            }
            other => panic!("expected Offer, got {other:?}"),
        }
    }

    #[test]
    fn protocol_version_mismatch_is_rejected() {
        let (mut a, mut b) = paired_channels("peer-a", "peer-b");
        let envelope = Envelope {
            protocol_version: PROTOCOL_VERSION + 1,
            message: WireMessage::Delivered(Delivered {
                item_id: ItemId("x".into()),
            }),
        };
        let bytes = bincode::serialize(&envelope).unwrap();
        a.send(&bytes).unwrap();

        assert!(matches!(
            recv_message(&mut b),
            Err(TransferError::ProtocolVersionMismatch { .. })
        ));
    }

    #[test]
    fn channel_reports_the_remote_peer_id() {
        let (a, _b) = paired_channels("peer-a", "peer-b");
        assert_eq!(a.remote_peer_id(), "peer-b");
    }

    #[test]
    fn send_item_and_receive_item_round_trip_with_correct_content_and_hash_and_state() {
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");

        let payload = b"the quick brown fox jumps over the lazy dog, many times over so it spans more than one chunk boundary when chunk size is small".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let mut item = sample_item();
        item.hash = expected_hash.clone();
        item.size_bytes = payload.len() as u64;

        let mut sender_store = FakeStore::with_outbound("item-1", TransferState::Queued);
        let sender_payload = payload.clone();
        let sender_thread = std::thread::spawn(move || {
            let result = send_item(
                &mut sender_channel,
                &mut sender_store,
                "item-1",
                &item,
                Cursor::new(sender_payload),
            );
            (result, sender_store)
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        let offer = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();

        let (result, sender_store) = sender_thread.join().unwrap();
        result.unwrap();

        assert_eq!(offer.item_id.0, "item-1");
        assert_eq!(offer.hash, expected_hash);
        assert_eq!(receiver_store.inbound["item-1"].bytes, payload);
        assert_eq!(receiver_store.inbound["item-1"].state, TransferState::Delivered);
        assert_eq!(sender_store.outbound["item-1"], TransferState::Delivered);
    }

    #[test]
    fn receive_item_rejects_a_corrupted_stream_via_hash_mismatch() {
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let item = sample_item();

        let sender_thread = std::thread::spawn(move || {
            send_offer(&mut sender_channel, "item-1", &item).unwrap();
            match recv_message(&mut sender_channel).unwrap() {
                WireMessage::Accept(_) => {}
                other => panic!("expected Accept, got {other:?}"),
            }
            send_chunk(&mut sender_channel, "item-1", 0, b"tampered bytes".to_vec()).unwrap();
            send_done(
                &mut sender_channel,
                "item-1",
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap();
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        let result = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local");
        sender_thread.join().unwrap();

        assert!(matches!(result, Err(TransferError::HashMismatch { .. })));
        assert_eq!(
            receiver_store.inbound["item-1"].state,
            TransferState::Failed
        );
    }

    #[test]
    fn receive_item_rejects_a_peer_that_sends_more_than_the_declared_size() {
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let mut item = sample_item();
        item.size_bytes = 3;

        let sender_thread = std::thread::spawn(move || {
            send_offer(&mut sender_channel, "item-1", &item).unwrap();
            match recv_message(&mut sender_channel).unwrap() {
                WireMessage::Accept(_) => {}
                other => panic!("expected Accept, got {other:?}"),
            }
            send_chunk(&mut sender_channel, "item-1", 0, b"way more than three bytes".to_vec()).unwrap();
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        let result = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local");
        sender_thread.join().unwrap();

        assert!(matches!(result, Err(TransferError::DeclaredSizeExceeded { .. })));
        assert_eq!(receiver_store.inbound["item-1"].state, TransferState::Failed);
    }

    #[test]
    fn receive_item_rejects_an_over_cap_secret_offer_before_persisting_anything() {
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let mut item = sample_item();
        item.kind = ferry_proto::states::ItemKind::Secret;
        item.size_bytes = crate::policy::MAX_SECRET_BYTES + 1;

        let sender_thread = std::thread::spawn(move || {
            send_offer(&mut sender_channel, "item-1", &item).unwrap();
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        let result = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local");
        sender_thread.join().unwrap();

        assert!(matches!(
            result,
            Err(TransferError::Policy(crate::policy::PolicyError::ItemTooLargeForKind { .. }))
        ));
        assert!(
            !receiver_store.inbound.contains_key("item-1"),
            "a policy-rejected offer must never create an inbound row"
        );
    }

    #[test]
    fn resume_after_disconnect_continues_from_the_correct_offset_and_produces_the_full_file() {
        let payload: Vec<u8> = (0..50_000u32).map(|n| (n % 256) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let mut item = sample_item();
        item.hash = expected_hash.clone();
        item.size_bytes = payload.len() as u64;

        let mut receiver_store = FakeStore::default();

        {
            let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
            let item_first = item.clone();
            let sender_payload = payload.clone();
            let sender_thread = std::thread::spawn(move || {
                send_offer(&mut sender_channel, "item-1", &item_first).unwrap();
                match recv_message(&mut sender_channel).unwrap() {
                    WireMessage::Accept(accept) => assert_eq!(accept.offset, 0),
                    other => panic!("expected Accept, got {other:?}"),
                }
                for chunk in Chunker::new(Cursor::new(sender_payload), DEFAULT_CHUNK_BYTES, 0).take(1) {
                    let (seq, bytes) = chunk.unwrap();
                    send_chunk(&mut sender_channel, "item-1", seq, bytes).unwrap();
                }
            });

            let offer = match recv_message(&mut receiver_channel).unwrap() {
                WireMessage::Offer(offer) => offer,
                other => panic!("expected Offer, got {other:?}"),
            };
            receiver_store.create_inbound(&offer, "peer-a", "local").unwrap();
            let offset = receiver_store.inbound_bytes_received("item-1").unwrap();
            assert_eq!(offset, 0);
            transition_inbound(&mut receiver_store, "item-1", TransferState::Accepted).unwrap();
            transition_inbound(&mut receiver_store, "item-1", TransferState::Transferring).unwrap();
            send_accept(&mut receiver_channel, "item-1", offset).unwrap();

            match recv_message(&mut receiver_channel).unwrap() {
                WireMessage::Chunk(chunk) => {
                    receiver_store
                        .append_inbound_chunk("item-1", chunk.seq, &chunk.bytes)
                        .unwrap();
                }
                other => panic!("expected Chunk, got {other:?}"),
            }
            sender_thread.join().unwrap();
        }

        let bytes_before_resume = receiver_store.inbound_bytes_received("item-1").unwrap();
        assert!(
            bytes_before_resume > 0 && bytes_before_resume < payload.len() as u64,
            "session 1 must have landed a real partial prefix, not nothing and not everything"
        );

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let mut sender_store = FakeStore::with_outbound("item-1", TransferState::Transferring);
        let resumed_item = item.clone();
        let sender_payload = payload.clone();
        let sender_thread = std::thread::spawn(move || {
            let result = send_item(
                &mut sender_channel,
                &mut sender_store,
                "item-1",
                &resumed_item,
                Cursor::new(sender_payload),
            );
            (result, sender_store)
        });

        let clock = ExpiryClock::new();
        let received_offer =
            receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        let (result, sender_store) = sender_thread.join().unwrap();
        result.unwrap();

        assert_eq!(received_offer.item_id.0, "item-1");
        assert_eq!(receiver_store.inbound["item-1"].bytes, payload);
        assert_eq!(receiver_store.inbound["item-1"].state, TransferState::Delivered);
        assert_eq!(sender_store.outbound["item-1"], TransferState::Delivered);
    }

    #[test]
    fn burn_after_read_makes_a_second_open_fail() {
        let mut store = FakeStore::default();
        let offer = Offer {
            item_id: ItemId("item-1".into()),
            kind: ItemKind::File,
            name: "secret-plan.txt".into(),
            size_bytes: 3,
            hash: "irrelevant".into(),
            ttl_secs: 60,
            burn_after_read: true,
            notify_on_open: false,
        };
        store.create_inbound(&offer, "peer-a", "local").unwrap();
        store.append_inbound_chunk("item-1", 0, b"abc").unwrap();
        transition_inbound(&mut store, "item-1", TransferState::Accepted).unwrap();
        transition_inbound(&mut store, "item-1", TransferState::Transferring).unwrap();
        store.finalize_inbound_delivered("item-1").unwrap();

        let (mut channel, peer_channel) = paired_channels("peer-b", "peer-a");
        let drain = std::thread::spawn(move || {
            let mut peer_channel = peer_channel;
            let mut seen = Vec::new();
            while let Ok(msg) = recv_message(&mut peer_channel) {
                seen.push(msg);
            }
            seen
        });

        let clock = ExpiryClock::new();
        open_item(&mut channel, &mut store, &clock, "item-1").unwrap();
        assert!(
            store.inbound["item-1"].bytes.is_empty(),
            "burn must clear the payload once opened"
        );
        assert_eq!(store.inbound["item-1"].state, TransferState::Opened);

        let second = open_item(&mut channel, &mut store, &clock, "item-1");
        assert!(matches!(second, Err(TransferError::AlreadyOpened(_))));

        drop(channel);
        let seen = drain.join().unwrap();
        assert_eq!(
            seen.len(),
            1,
            "only the first open should have sent an Opened message over the wire"
        );
    }

    #[test]
    fn open_item_locally_marks_opened_and_burns_without_needing_a_channel() {
        let mut store = FakeStore::default();
        let offer = Offer {
            item_id: ItemId("item-1".into()),
            kind: ItemKind::File,
            name: "secret-plan.txt".into(),
            size_bytes: 3,
            hash: "irrelevant".into(),
            ttl_secs: 60,
            burn_after_read: true,
            notify_on_open: false,
        };
        store.create_inbound(&offer, "peer-a", "local").unwrap();
        store.append_inbound_chunk("item-1", 0, b"abc").unwrap();
        transition_inbound(&mut store, "item-1", TransferState::Accepted).unwrap();
        transition_inbound(&mut store, "item-1", TransferState::Transferring).unwrap();
        store.finalize_inbound_delivered("item-1").unwrap();

        let clock = ExpiryClock::new();
        open_item_locally(&mut store, &clock, "item-1").unwrap();

        assert_eq!(store.inbound["item-1"].state, TransferState::Opened);
        assert!(store.inbound["item-1"].bytes.is_empty(), "burn must clear the payload once opened");

        let second = open_item_locally(&mut store, &clock, "item-1");
        assert!(matches!(second, Err(TransferError::AlreadyOpened(_))));
    }

    #[test]
    fn an_item_delivered_with_zero_ttl_cannot_be_opened_and_ends_up_expired() {
        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        let mut item = sample_item();
        item.hash = hex::encode(hasher.finalize());
        item.ttl_secs = 0;
        let mut sender_store = FakeStore::with_outbound("item-1", TransferState::Queued);
        let sender_thread = std::thread::spawn(move || {
            send_item(&mut sender_channel, &mut sender_store, "item-1", &item, Cursor::new(b"abc".to_vec()))
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        sender_thread.join().unwrap().unwrap();

        let (mut channel, _peer) = paired_channels("peer-b", "peer-a");
        let result = open_item(&mut channel, &mut receiver_store, &clock, "item-1");
        assert!(matches!(result, Err(TransferError::Expired(_))));
        assert_eq!(receiver_store.inbound["item-1"].state, TransferState::Expired);
    }

    #[test]
    fn receive_next_inbound_treats_a_bare_opened_as_an_open_receipt_and_advances_the_sender() {
        let (mut peer_channel, mut our_channel) = paired_channels("peer-a", "peer-b");
        peer_channel
            .send(&bincode::serialize(&Envelope {
                protocol_version: PROTOCOL_VERSION,
                message: WireMessage::Opened(Opened { item_id: ItemId("item-1".into()) }),
            }).unwrap())
            .unwrap();

        let mut store = FakeStore::with_outbound("item-1", TransferState::Delivered);
        let clock = ExpiryClock::new();
        let event = receive_next_inbound(&mut our_channel, &mut store, &clock, "peer-a", "local", true, &mut |_, _, _| {}).unwrap();

        assert_eq!(event, InboundEvent::OpenReceipt("item-1".to_string()));
        assert_eq!(store.outbound["item-1"], TransferState::Opened);
    }

    #[test]
    fn a_bare_opened_for_an_already_opened_item_is_accepted_idempotently() {
        let (mut peer_channel, mut our_channel) = paired_channels("peer-a", "peer-b");
        peer_channel
            .send(&bincode::serialize(&Envelope {
                protocol_version: PROTOCOL_VERSION,
                message: WireMessage::Opened(Opened { item_id: ItemId("item-1".into()) }),
            }).unwrap())
            .unwrap();

        let mut store = FakeStore::with_outbound("item-1", TransferState::Opened);
        let clock = ExpiryClock::new();
        let event = receive_next_inbound(&mut our_channel, &mut store, &clock, "peer-a", "local", true, &mut |_, _, _| {}).unwrap();
        assert_eq!(event, InboundEvent::OpenReceipt("item-1".to_string()));
    }

    #[test]
    fn a_bare_opened_for_an_unknown_outbound_item_is_an_error() {
        let (mut peer_channel, mut our_channel) = paired_channels("peer-a", "peer-b");
        peer_channel
            .send(&bincode::serialize(&Envelope {
                protocol_version: PROTOCOL_VERSION,
                message: WireMessage::Opened(Opened { item_id: ItemId("ghost".into()) }),
            }).unwrap())
            .unwrap();

        let mut store = FakeStore::default();
        let clock = ExpiryClock::new();
        let result = receive_next_inbound(&mut our_channel, &mut store, &clock, "peer-a", "local", true, &mut |_, _, _| {});
        assert!(matches!(result, Err(TransferError::UnknownItem(_))));
    }

    #[test]
    fn send_item_reporting_emits_progress_for_each_chunk_ending_at_the_full_size() {
        use ferry_net::chunk::DEFAULT_CHUNK_BYTES;

        let payload: Vec<u8> = (0..(DEFAULT_CHUNK_BYTES as u32 + 500)).map(|n| n as u8).collect();
        let total = payload.len() as u64;
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let mut item = sample_item();
        item.hash = hex::encode(hasher.finalize());
        item.size_bytes = total;

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let item_for_thread = item.clone();
        let payload_for_thread = payload.clone();
        let sender_thread = std::thread::spawn(move || {
            let mut store = FakeStore::with_outbound("item-1", TransferState::Queued);
            let mut seen: Vec<(String, u64, u64)> = Vec::new();
            send_item_reporting(
                &mut sender_channel,
                &mut store,
                "item-1",
                &item_for_thread,
                Cursor::new(payload_for_thread),
                &mut |id, bytes, tot| seen.push((id.to_string(), bytes, tot)),
            )
            .unwrap();
            seen
        });

        let mut receiver_store = FakeStore::default();
        let clock = ExpiryClock::new();
        receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        let seen = sender_thread.join().unwrap();

        assert_eq!(seen.len(), 2, "a payload of one full chunk + a tail must report twice");
        assert!(seen.iter().all(|(id, _, tot)| id == "item-1" && *tot == total));
        assert_eq!(seen.last().unwrap().1, total, "the final progress report must equal the full size");
        assert!(seen[0].1 < total && seen[0].1 > 0);
    }
}
