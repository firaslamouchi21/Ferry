use std::net::TcpStream;

use ferry_core::dispatcher::drain_outbox_for_peer_reporting;
use ferry_core::expiry::ExpiryClock;
use ferry_core::ports::{OutboundSource, Store};
use ferry_net::discovery::{peer_from_resolved, Discovery, DiscoveredPeer};
use ferry_net::transport::{connect_initiator, StaticKeypair};
use ferry_proto::ipc::{IpcEvent, IpcResource};
use mdns_sd::ServiceEvent;

use crate::channel_adapter::NetChannel;
use crate::event_bus::EventSink;
use crate::presence::Presence;

const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

#[allow(clippy::too_many_arguments)]
pub fn run_reappearance_loop(
    discovery: &Discovery,
    local_keys: &StaticKeypair,
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    roster_lookup: impl Fn(&str) -> Option<[u8; 32]>,
    emit: &EventSink,
    presence: &Presence,
) {
    let events = match discovery.browse() {
        Ok(events) => events,
        Err(err) => {
            eprintln!("ferry-daemon: mDNS browse failed to start: {err}");
            return;
        }
    };

    for event in events {
        let ServiceEvent::ServiceResolved(resolved) = event else {
            continue;
        };
        handle_reappeared_peer(&peer_from_resolved(&resolved), local_keys, store, clock, source, &roster_lookup, emit, presence);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_reappeared_peer(
    peer: &DiscoveredPeer,
    local_keys: &StaticKeypair,
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    roster_lookup: &impl Fn(&str) -> Option<[u8; 32]>,
    emit: &EventSink,
    presence: &Presence,
) {
    let Some(fingerprint) = &peer.fingerprint else {
        return;
    };
    let Some(remote_static) = roster_lookup(fingerprint) else {
        return;
    };
    presence.seen(fingerprint);

    let addr = format!("{}:{}", peer.host.trim_end_matches('.'), peer.port);
    let stream = match TcpStream::connect(&addr) {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("ferry-daemon: could not connect to reappeared peer {fingerprint} at {addr}: {err}");
            return;
        }
    };
    let raw = match connect_initiator(stream, local_keys, &remote_static, |_| true) {
        Ok(raw) => raw,
        Err(err) => {
            eprintln!("ferry-daemon: handshake with reappeared peer {fingerprint} failed: {err}");
            return;
        }
    };
    let mut channel = NetChannel::new(raw);

    let mut last_progress = std::time::Instant::now() - PROGRESS_INTERVAL;
    let mut on_progress = |item_id: &str, bytes: u64, total: u64| {
        let now = std::time::Instant::now();
        if bytes >= total || now.duration_since(last_progress) >= PROGRESS_INTERVAL {
            last_progress = now;
            emit(IpcEvent::Progress { item_id: item_id.to_string(), bytes, total });
        }
    };
    match drain_outbox_for_peer_reporting(store, &mut channel, clock, fingerprint, fingerprint, source, &mut on_progress) {
        Ok(outcome) => {
            if !outcome.delivered.is_empty()
                || !outcome.dropped.is_empty()
                || !outcome.failed.is_empty()
                || !outcome.deferred.is_empty()
            {
                println!(
                    "ferry-daemon: drained outbox to {fingerprint}: {} delivered, {} dropped, {} failed, {} deferred (backoff)",
                    outcome.delivered.len(),
                    outcome.dropped.len(),
                    outcome.failed.len(),
                    outcome.deferred.len()
                );
            }
            for item_id in outcome
                .delivered
                .iter()
                .chain(outcome.dropped.iter())
                .chain(outcome.receipts_sent.iter())
            {
                emit(IpcEvent::Changed {
                    resource: IpcResource::Transfer,
                    id: Some(item_id.clone()),
                });
            }
        }
        Err(err) => eprintln!("ferry-daemon: failed to drain outbox to {fingerprint}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::ports::NewOutboundItem;
    use ferry_net::transport::{accept_responder, generate_keypair};
    use ferry_proto::states::{ItemKind, TransferState};
    use std::io::Cursor;
    use std::net::TcpListener;

    struct FileSource;

    impl OutboundSource for FileSource {
        type Reader = Cursor<Vec<u8>>;

        fn open(&mut self, _key: &str) -> std::io::Result<Self::Reader> {
            Ok(Cursor::new(b"discovered and delivered".to_vec()))
        }
    }

    #[test]
    fn a_reappeared_rostered_peer_gets_its_queued_outbox_drained_over_a_real_connection() {
        use sha2::{Digest, Sha256};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let local_keys = generate_keypair();
        let remote_keys = generate_keypair();
        let remote_public = remote_keys.public;
        let local_public = local_keys.public;

        let payload = b"discovered and delivered".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let receiver_thread = std::thread::spawn(move || {
            let (stream, _addr) = listener.accept().unwrap();
            let raw = accept_responder(stream, &remote_keys, move |remote: &[u8; 32]| *remote == local_public).unwrap();
            let mut channel = NetChannel::new(raw);
            let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
            ferry_store::roster::insert_peer(&receiver_conn, "sender-fp", "laptop", "sk", "xk").unwrap();
            let mut receiver_store = crate::store_adapter::SqliteStore::new(
                receiver_conn,
                std::env::temp_dir().join(format!("ferry-discovery-test-{}", uuid::Uuid::now_v7())),
                ferry_crypto::identity::Identity::generate(),
            );
            let clock = ExpiryClock::new();
            ferry_core::transfer::receive_item(&mut channel, &mut receiver_store, &clock, "sender-fp", "local").unwrap()
        });

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "receiver-fp", "laptop", "sk", "xk").unwrap();
        let mut sender_store = crate::store_adapter::SqliteStore::new(
            sender_conn,
            std::env::temp_dir().join(format!("ferry-discovery-test-sender-{}", uuid::Uuid::now_v7())),
            ferry_crypto::identity::Identity::generate(),
        );
        let item = NewOutboundItem {
            peer_id: "receiver-fp".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: payload.len() as u64,
            hash: expected_hash,
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/notes.txt".into(),
        };
        let clock = ExpiryClock::new();
        let item_id = ferry_core::outbox::enqueue_send(&mut sender_store, &clock, &item, "local").unwrap();

        let peer = DiscoveredPeer {
            fullname: "receiver._ferry._tcp.local.".into(),
            host: addr.ip().to_string(),
            port: addr.port(),
            fingerprint: Some("receiver-fp".into()),
            protocol_version: None,
        };
        let mut source = FileSource;
        handle_reappeared_peer(
            &peer,
            &local_keys,
            &mut sender_store,
            &clock,
            &mut source,
            &move |fingerprint| (fingerprint == "receiver-fp").then_some(remote_public),
            &crate::event_bus::noop_sink,
            &crate::presence::Presence::new(),
        );

        let offer = receiver_thread.join().unwrap();
        assert_eq!(offer.name, "notes.txt");
        assert_eq!(
            sender_store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Delivered)
        );
    }

    #[test]
    fn an_unrostered_reappeared_peer_is_ignored_without_attempting_a_connection() {
        let peer = DiscoveredPeer {
            fullname: "stranger._ferry._tcp.local.".into(),
            host: "127.0.0.1".into(),
            port: 1,
            fingerprint: Some("stranger-fp".into()),
            protocol_version: None,
        };
        let local_keys = generate_keypair();
        let mut source = FileSource;
        let conn = ferry_store::connection::open_in_memory().unwrap();
        let mut store = crate::store_adapter::SqliteStore::new(
            conn,
            std::env::temp_dir().join(format!("ferry-discovery-test-{}", uuid::Uuid::now_v7())),
            ferry_crypto::identity::Identity::generate(),
        );
        let clock = ExpiryClock::new();

        handle_reappeared_peer(&peer, &local_keys, &mut store, &clock, &mut source, &|_| None, &crate::event_bus::noop_sink, &crate::presence::Presence::new());
    }
}
