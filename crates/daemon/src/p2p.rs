use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferry_core::expiry::ExpiryClock;
use ferry_core::ports::Store;
use ferry_core::transfer::{receive_next_inbound, InboundEvent, TransferError};
use ferry_net::transport::{accept_responder, StaticKeypair};
use ferry_proto::ipc::{IpcEvent, IpcResource};
use ferry_proto::states::ItemKind;
use thiserror::Error;

use crate::channel_adapter::NetChannel;
use crate::discovery::PeerLink;
use crate::event_bus::EventSink;
use crate::presence::Presence;

const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(200);
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 100;
const MAX_ACTIVE_CONNECTIONS: usize = 64;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Error)]
pub enum P2pError {
    #[error("failed to bind the P2P listener on port {port}: {source}")]
    Bind { port: u16, source: std::io::Error },
    #[error("failed to accept a connection: {0}")]
    Accept(std::io::Error),
}

pub fn bind(port: u16) -> Result<TcpListener, P2pError> {
    TcpListener::bind(("0.0.0.0", port)).map_err(|source| P2pError::Bind { port, source })
}

pub fn serve_incoming(listener: &TcpListener, link: &PeerLink, auto_accept: bool) -> Result<(), P2pError> {
    let locks = Arc::new(PeerLocks::default());
    let active = Arc::new(AtomicUsize::new(0));
    let mut consecutive_failures = 0u32;
    loop {
        let stream = match listener.accept() {
            Ok((stream, _addr)) => stream,
            Err(err) => {
                consecutive_failures += 1;
                if consecutive_failures > MAX_CONSECUTIVE_ACCEPT_FAILURES {
                    return Err(P2pError::Accept(err));
                }
                eprintln!("ferry-daemon: could not accept an incoming P2P connection: {err}");
                std::thread::sleep(ACCEPT_RETRY_DELAY);
                continue;
            }
        };
        consecutive_failures = 0;
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_ACTIVE_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let link = link.clone();
        let locks = locks.clone();
        let slot = ActiveSlot(active.clone());
        let spawned = std::thread::Builder::new().name("ferry-p2p".into()).spawn(move || {
            let _slot = slot;
            match (link.open_store)() {
                Ok((mut store, _)) => {
                    let roster = match (link.open_store)() {
                        Ok((roster, _)) => roster,
                        Err(err) => {
                            eprintln!("ferry-daemon: could not open the roster for an incoming connection: {err}");
                            return;
                        }
                    };
                    handle_incoming_connection(
                        stream,
                        &link.local_keys,
                        &mut store,
                        &link.clock,
                        &|remote: &[u8; 32]| roster.peer_id_for_static(remote),
                        auto_accept,
                        CONNECTION_IDLE_TIMEOUT,
                        link.emit.as_ref(),
                        &link.presence,
                        &locks,
                    );
                }
                Err(err) => eprintln!("ferry-daemon: could not open the store for an incoming connection: {err}"),
            }
        });
        if let Err(err) = spawned {
            eprintln!("ferry-daemon: could not start a thread for an incoming connection: {err}");
        }
    }
}

struct ActiveSlot(Arc<AtomicUsize>);

impl Drop for ActiveSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct PeerLocks {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl PeerLocks {
    fn for_peer(&self, peer_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(peer_id.to_string())
            .or_default()
            .clone()
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_incoming_connection(
    stream: TcpStream,
    local_keys: &StaticKeypair,
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id_for: &impl Fn(&[u8; 32]) -> Option<String>,
    auto_accept: bool,
    idle_timeout: Duration,
    emit: &EventSink,
    presence: &Presence,
    locks: &PeerLocks,
) {
    let control = match stream.try_clone() {
        Ok(control) => control,
        Err(err) => {
            eprintln!("ferry-daemon: failed to prepare an incoming P2P connection: {err}");
            return;
        }
    };
    if let Err(err) = control.set_nodelay(true) {
        eprintln!("ferry-daemon: failed to disable write coalescing on an incoming P2P connection: {err}");
    }
    if let Err(err) = stream.set_read_timeout(Some(idle_timeout.min(HANDSHAKE_TIMEOUT))) {
        eprintln!("ferry-daemon: failed to set a read timeout on an incoming P2P connection: {err}");
        return;
    }
    if let Err(err) = stream.set_write_timeout(Some(idle_timeout)) {
        eprintln!("ferry-daemon: failed to set a write timeout on an incoming P2P connection: {err}");
        return;
    }

    let raw = match accept_responder(stream, local_keys, |remote: &[u8; 32]| peer_id_for(remote).is_some()) {
        Ok(raw) => raw,
        Err(err) => {
            eprintln!("ferry-daemon: rejected an incoming P2P connection: {err}");
            return;
        }
    };

    let Some(peer_id) = peer_id_for(&raw.remote_static) else {
        eprintln!("ferry-daemon: incoming connection passed the handshake but its roster entry vanished — dropping");
        return;
    };

    if let Err(err) = control.set_read_timeout(Some(idle_timeout)) {
        eprintln!("ferry-daemon: failed to set a read timeout on a connection from {peer_id}: {err}");
        return;
    }
    if presence.seen(&peer_id) {
        emit(IpcEvent::Changed {
            resource: IpcResource::Peer,
            id: Some(peer_id.clone()),
        });
    }

    let mut channel = NetChannel::new(raw);
    let peer_lock = locks.for_peer(&peer_id);
    let _exclusive = peer_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut handled_any = false;

    let mut last_progress = std::time::Instant::now() - PROGRESS_INTERVAL;
    loop {
        let mut on_progress = |item_id: &str, bytes: u64, total: u64| {
            let now = std::time::Instant::now();
            if bytes >= total || now.duration_since(last_progress) >= PROGRESS_INTERVAL {
                last_progress = now;
                emit(IpcEvent::Progress { item_id: item_id.to_string(), bytes, total });
            }
        };
        let received = receive_next_inbound(&mut channel, store, clock, &peer_id, &peer_id, auto_accept, &mut on_progress);
        if received.is_ok() {
            presence.seen(&peer_id);
        }
        match received {
            Ok(InboundEvent::ItemDelivered(offer)) => {
                println!("ferry-daemon: received item {} from {peer_id}", offer.item_id.0);
                let resource = if offer.kind == ItemKind::Message {
                    IpcResource::Message
                } else {
                    IpcResource::Transfer
                };
                emit(IpcEvent::Changed { resource, id: Some(offer.item_id.0) });
            }
            Ok(InboundEvent::OfferPending(offer)) => {
                println!("ferry-daemon: offer {} from {peer_id} is held for a local accept/reject", offer.item_id.0);
                let resource = if offer.kind == ItemKind::Message {
                    IpcResource::Message
                } else {
                    IpcResource::Transfer
                };
                emit(IpcEvent::Changed { resource, id: Some(offer.item_id.0) });
                break;
            }
            Ok(InboundEvent::OfferDeclined(item_id)) => {
                println!("ferry-daemon: offer {item_id} from {peer_id} was declined locally");
                emit(IpcEvent::Changed { resource: IpcResource::Transfer, id: Some(item_id) });
                break;
            }
            Ok(InboundEvent::OpenReceipt(item_id)) => {
                println!("ferry-daemon: {peer_id} reported opening item {item_id}");
                emit(IpcEvent::Changed { resource: IpcResource::Transfer, id: Some(item_id) });
            }
            Err(TransferError::Channel(_)) if !handled_any => break,
            Err(err) => {
                println!("ferry-daemon: P2P connection from {peer_id} ended: {err}");
                emit(IpcEvent::Changed {
                    resource: IpcResource::Transfer,
                    id: None,
                });
                break;
            }
        }
        handled_any = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::ports::NewOutboundItem;
    use ferry_core::transfer::send_item;
    use ferry_net::transport::{connect_initiator, generate_keypair};
    use ferry_proto::states::{ItemKind, TransferState};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn temp_payload_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ferry-p2p-test-{}", uuid::Uuid::now_v7()))
    }

    #[test]
    fn a_real_client_over_a_real_tcp_socket_is_accepted_authorized_and_its_item_is_received() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let responder_keys = generate_keypair();
        let initiator_keys = generate_keypair();
        let initiator_public = initiator_keys.public;
        let responder_public = responder_keys.public;

        let payload = b"hello over a real p2p socket".to_vec();
        let payload_len = payload.len() as u64;
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let server_thread = std::thread::spawn(move || {
            let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
            ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
            let mut store =
                crate::store_adapter::SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
            let clock = ExpiryClock::new();
            let (stream, _addr) = listener.accept().unwrap();
            let events = std::cell::RefCell::new(Vec::new());
            handle_incoming_connection(
                stream,
                &responder_keys,
                &mut store,
                &clock,
                &move |remote: &[u8; 32]| (*remote == initiator_public).then(|| "peer-a".to_string()),
                true,
                CONNECTION_IDLE_TIMEOUT,
                &|event| events.borrow_mut().push(event),
                &Presence::new(),
                &PeerLocks::default(),
            );
            (store, events.into_inner())
        });

        let stream = TcpStream::connect(addr).unwrap();
        let raw = connect_initiator(stream, &initiator_keys, &responder_public, |_| true).unwrap();
        let mut channel = NetChannel::new(raw);

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store =
            crate::store_adapter::SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let item = NewOutboundItem {
            peer_id: "peer-b".into(),
            kind: ItemKind::File,
            name: "hello.txt".into(),
            size_bytes: payload.len() as u64,
            hash: expected_hash.clone(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/hello.txt".into(),
        };
        let item_id = sender_store.create_and_enqueue_outbound(&item, "local").unwrap();
        send_item(&mut channel, &mut sender_store, &item_id, &item, Cursor::new(payload)).unwrap();

        drop(channel);
        let (receiver_store, events) = server_thread.join().unwrap();
        assert_eq!(
            receiver_store.get_inbound_state(&item_id).unwrap(),
            Some(TransferState::Delivered)
        );
        assert_eq!(receiver_store.inbound_full_hash_so_far(&item_id).unwrap(), expected_hash);
        use ferry_proto::ipc::{IpcEvent, IpcResource};
        assert!(
            events.contains(&IpcEvent::Changed {
                resource: IpcResource::Transfer,
                id: Some(item_id.clone()),
            }),
            "a real inbound delivery must emit a Changed event for the UI (got {events:?})"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                IpcEvent::Progress { item_id: id, bytes, total }
                    if id == &item_id && bytes == total && *total == payload_len
            )),
            "streaming must emit at least a final Progress event (got {events:?})"
        );
    }

    #[test]
    fn an_unauthorized_connection_is_rejected_before_any_item_is_ever_parsed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let responder_keys = generate_keypair();
        let stranger_keys = generate_keypair();

        let server_thread = std::thread::spawn(move || {
            let mut store = crate::store_adapter::SqliteStore::new(
                ferry_store::connection::open_in_memory().unwrap(),
                temp_payload_dir(),
                ferry_crypto::identity::Identity::generate(),
            );
            let clock = ExpiryClock::new();
            let (stream, _addr) = listener.accept().unwrap();
            handle_incoming_connection(stream, &responder_keys, &mut store, &clock, &|_: &[u8; 32]| None, true, CONNECTION_IDLE_TIMEOUT, &crate::event_bus::noop_sink, &Presence::new(), &PeerLocks::default());
        });

        let stream = TcpStream::connect(addr).unwrap();
        let _ = connect_initiator(stream, &stranger_keys, &responder_keys.public, |_| true);

        server_thread.join().unwrap();
    }

    #[test]
    fn a_silently_connected_peer_is_dropped_after_the_idle_timeout_instead_of_hanging_forever() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let responder_keys = generate_keypair();

        let _silent_client = TcpStream::connect(addr).unwrap();

        let mut store = crate::store_adapter::SqliteStore::new(
            ferry_store::connection::open_in_memory().unwrap(),
            temp_payload_dir(),
            ferry_crypto::identity::Identity::generate(),
        );
        let clock = ExpiryClock::new();
        let (stream, _addr) = listener.accept().unwrap();

        let started = std::time::Instant::now();
        handle_incoming_connection(
            stream,
            &responder_keys,
            &mut store,
            &clock,
            &|_: &[u8; 32]| None,
            true,
            std::time::Duration::from_millis(200),
            &crate::event_bus::noop_sink,
            &Presence::new(),
            &PeerLocks::default(),
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "a connection that never sends anything must be dropped by the idle timeout, not block forever (took {elapsed:?})"
        );
    }
}
