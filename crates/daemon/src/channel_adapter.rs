use std::io::{Read, Write};

use ferry_core::ports::{Channel, ChannelError};
use ferry_net::transport::AuthenticatedChannel;

pub struct NetChannel<S: Read + Write> {
    inner: AuthenticatedChannel<S>,
    remote_peer_id: String,
}

impl<S: Read + Write> NetChannel<S> {
    pub fn new(inner: AuthenticatedChannel<S>) -> Self {
        let remote_peer_id = hex::encode(inner.remote_static);
        Self {
            inner,
            remote_peer_id,
        }
    }
}

impl<S: Read + Write> Channel for NetChannel<S> {
    fn send(&mut self, bytes: &[u8]) -> Result<(), ChannelError> {
        self.inner
            .send(bytes)
            .map_err(|e| ChannelError(e.to_string()))
    }

    fn recv(&mut self) -> Result<Vec<u8>, ChannelError> {
        self.inner.recv().map_err(|e| ChannelError(e.to_string()))
    }

    fn remote_peer_id(&self) -> &str {
        &self.remote_peer_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::transfer::{recv_message, send_offer};
    use ferry_proto::envelope::WireMessage;
    use ferry_proto::states::{ItemKind, TransferState};
    use std::io::Cursor;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::thread;

    fn temp_payload_dir() -> PathBuf {
        std::env::temp_dir().join(format!("ferry-channel-adapter-test-{}", uuid::Uuid::now_v7()))
    }

    #[test]
    fn core_transfer_send_offer_works_over_a_real_noise_xk_channel() {
        let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();
        let initiator_keys = ferry_net::transport::generate_keypair();
        let responder_keys = ferry_net::transport::generate_keypair();
        let responder_public = responder_keys.public;
        let initiator_public = initiator_keys.public;

        let responder_thread = thread::spawn(move || {
            let raw = ferry_net::transport::accept_responder(responder_sock, &responder_keys, move |remote| {
                *remote == initiator_public
            })
            .unwrap();
            let mut channel = NetChannel::new(raw);

            match recv_message(&mut channel).unwrap() {
                WireMessage::Offer(offer) => offer,
                other => panic!("expected Offer, got {other:?}"),
            }
        });

        let raw = ferry_net::transport::connect_initiator(
            initiator_sock,
            &initiator_keys,
            &responder_public,
            |_| true,
        )
        .unwrap();
        let mut channel = NetChannel::new(raw);
        assert_eq!(channel.remote_peer_id(), hex::encode(responder_public));

        let item = ferry_core::ports::NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::Secret,
            name: ".env".into(),
            size_bytes: 256,
            hash: "abad1dea".into(),
            ttl_secs: 300,
            is_burn_after_read: true,
            notify_on_open: false,
            source_path: "/tmp/.env".into(),
        };
        send_offer(&mut channel, "item-42", &item).unwrap();

        let received_offer = responder_thread.join().unwrap();
        assert_eq!(received_offer.item_id.0, "item-42");
        assert_eq!(received_offer.name, ".env");
        assert_eq!(received_offer.size_bytes, 256);
        assert!(received_offer.burn_after_read);
    }

    #[test]
    fn full_offer_accept_stream_done_delivered_opened_round_trip_works_over_a_real_noise_xk_channel_and_real_sqlite_stores(
    ) {
        use ferry_core::ports::Store;
        use ferry_core::transfer::{open_item, receive_item, send_item};
        use sha2::{Digest, Sha256};

        let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();
        let initiator_keys = ferry_net::transport::generate_keypair();
        let responder_keys = ferry_net::transport::generate_keypair();
        let responder_public = responder_keys.public;
        let initiator_public = initiator_keys.public;

        let payload: Vec<u8> = (0..200_000u32).map(|n| (n % 256) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            crate::store_adapter::SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store = crate::store_adapter::SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let item = ferry_core::ports::NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::File,
            name: "large-file.bin".into(),
            size_bytes: payload.len() as u64,
            hash: expected_hash.clone(),
            ttl_secs: 600,
            is_burn_after_read: true,
            notify_on_open: true,
            source_path: "/tmp/large-file.bin".into(),
        };
        let sender_clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = ferry_core::outbox::enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();

        let responder_thread = thread::spawn(move || {
            let raw = ferry_net::transport::accept_responder(
                responder_sock,
                &responder_keys,
                move |remote| *remote == initiator_public,
            )
            .unwrap();
            let mut channel = NetChannel::new(raw);

            let clock = ferry_core::expiry::ExpiryClock::new();
            let offer = receive_item(&mut channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
            open_item(&mut channel, &mut receiver_store, &clock, &offer.item_id.0).unwrap();
            (offer, receiver_store)
        });

        let raw = ferry_net::transport::connect_initiator(
            initiator_sock,
            &initiator_keys,
            &responder_public,
            |_| true,
        )
        .unwrap();
        let mut channel = NetChannel::new(raw);

        let sender_payload = payload.clone();
        let sender_item_id = item_id.clone();
        send_item(&mut channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload)).unwrap();

        let (offer, receiver_store) = responder_thread.join().unwrap();
        assert_eq!(offer.item_id.0, item_id);
        assert_eq!(offer.hash, expected_hash);
        let empty_hash = hex::encode(Sha256::digest(b""));
        assert_eq!(
            receiver_store.inbound_full_hash_so_far(&item_id).unwrap(),
            empty_hash,
            "burn-after-read must have cleared the payload once opened"
        );
        assert_eq!(
            receiver_store.get_inbound_state(&item_id).unwrap(),
            Some(TransferState::Opened)
        );
        assert_eq!(
            sender_store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Opened)
        );
    }

    #[test]
    fn resume_after_a_real_disconnect_continues_from_the_correct_offset_on_disk() {
        use ferry_core::ports::Store;
        use ferry_core::transfer::{receive_item, send_accept, send_chunk, send_item};
        use sha2::{Digest, Sha256};

        let payload: Vec<u8> = (0..50_000u32).map(|n| (n % 256) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let item = ferry_core::ports::NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::File,
            name: "resume-me.bin".into(),
            size_bytes: payload.len() as u64,
            hash: expected_hash.clone(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/resume-me.bin".into(),
        };

        let sender_keys = ferry_net::transport::generate_keypair();
        let responder_keys = ferry_net::transport::generate_keypair();
        let responder_public = responder_keys.public;

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            crate::store_adapter::SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store = crate::store_adapter::SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let sender_clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = ferry_core::outbox::enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();

        {
            let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();
            let sender_payload = payload.clone();
            let item_first = item.clone();
            let item_id_first = item_id.clone();
            let responder_thread = thread::spawn(move || {
                let raw = ferry_net::transport::accept_responder(responder_sock, &responder_keys, |_| true).unwrap();
                let mut channel = NetChannel::new(raw);
                let offer = match recv_message(&mut channel).unwrap() {
                    WireMessage::Offer(offer) => offer,
                    other => panic!("expected Offer, got {other:?}"),
                };
                (channel, offer)
            });

            let raw =
                ferry_net::transport::connect_initiator(initiator_sock, &sender_keys, &responder_public, |_| true)
                    .unwrap();
            let mut sender_channel = NetChannel::new(raw);
            send_offer(&mut sender_channel, &item_id_first, &item_first).unwrap();

            let (mut receiver_channel, offer) = responder_thread.join().unwrap();
            receiver_store.create_inbound(&offer, "peer-a", "local").unwrap();
            let offset = receiver_store.inbound_bytes_received(&item_id_first).unwrap();
            assert_eq!(offset, 0);
            receiver_store.set_inbound_state(&item_id_first, TransferState::Accepted).unwrap();
            receiver_store.set_inbound_state(&item_id_first, TransferState::Transferring).unwrap();
            send_accept(&mut receiver_channel, &item_id_first, offset).unwrap();

            match recv_message(&mut sender_channel).unwrap() {
                WireMessage::Accept(_) => {}
                other => panic!("expected Accept, got {other:?}"),
            }
            for chunk in
                ferry_net::chunk::Chunker::new(Cursor::new(sender_payload), ferry_net::chunk::DEFAULT_CHUNK_BYTES, 0)
                    .take(1)
            {
                let (seq, bytes) = chunk.unwrap();
                send_chunk(&mut sender_channel, &item_id_first, seq, bytes).unwrap();
            }

            match recv_message(&mut receiver_channel).unwrap() {
                WireMessage::Chunk(chunk) => {
                    receiver_store
                        .append_inbound_chunk(&item_id_first, chunk.seq, &chunk.bytes)
                        .unwrap();
                }
                other => panic!("expected Chunk, got {other:?}"),
            }
            sender_store.set_outbound_state(&item_id_first, TransferState::Offered).unwrap();
            sender_store.set_outbound_state(&item_id_first, TransferState::Accepted).unwrap();
            sender_store.set_outbound_state(&item_id_first, TransferState::Transferring).unwrap();
        }

        let partial = receiver_store.inbound_bytes_received(&item_id).unwrap();
        assert!(
            partial > 0 && partial < payload.len() as u64,
            "the first session must have landed a real partial prefix on disk"
        );

        let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();
        let item_resumed = item.clone();
        let item_id_resumed = item_id.clone();
        let sender_payload = payload.clone();
        let responder_thread = thread::spawn(move || {
            let raw = ferry_net::transport::accept_responder(responder_sock, &responder_keys, |_| true).unwrap();
            let mut channel = NetChannel::new(raw);
            let clock = ferry_core::expiry::ExpiryClock::new();
            let offer = receive_item(&mut channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
            (offer, receiver_store)
        });

        let raw = ferry_net::transport::connect_initiator(initiator_sock, &sender_keys, &responder_public, |_| true)
            .unwrap();
        let mut channel = NetChannel::new(raw);
        send_item(&mut channel, &mut sender_store, &item_id_resumed, &item_resumed, Cursor::new(sender_payload))
            .unwrap();

        let (offer, receiver_store) = responder_thread.join().unwrap();
        assert_eq!(offer.item_id.0, item_id);
        assert_eq!(
            receiver_store.inbound_full_hash_so_far(&item_id).unwrap(),
            expected_hash,
            "the resumed session must have produced the full, correct file"
        );
        assert_eq!(
            sender_store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Delivered)
        );
    }
}
