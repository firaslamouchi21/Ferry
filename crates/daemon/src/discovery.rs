use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferry_core::dispatcher::drain_outbox_for_peer_reporting;
use ferry_core::expiry::ExpiryClock;
use ferry_core::ports::{OutboundSource, Store};
use ferry_net::discovery::{peer_from_resolved, Discovery, DiscoveredPeer};
use ferry_net::transport::{connect_initiator, AuthenticatedChannel, StaticKeypair};
use ferry_proto::ipc::{IpcEvent, IpcResource};
use mdns_sd::{RecvTimeoutError, ServiceEvent};

use crate::channel_adapter::NetChannel;
use crate::event_bus::EventSink;
use crate::file_source::StoreBackedSource;
use crate::presence::Presence;
use crate::store_adapter::SqliteStore;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const REDRIVE_INTERVAL: Duration = Duration::from_secs(5);
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const PROBE_TICK: Duration = Duration::from_secs(5);
const PROBE_INTERVAL_MILLIS: i64 = 20_000;
const MAX_DRAIN_ROUNDS: usize = 8;
const MAX_KNOWN_PEERS: usize = 1024;

fn usable_peer_address(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
        std::net::IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80,
    }
}

fn candidate_addresses(peer: &DiscoveredPeer) -> Vec<SocketAddr> {
    let mut ips: Vec<std::net::IpAddr> = peer.addresses.iter().copied().filter(usable_peer_address).collect();
    ips.sort_by_key(|ip| ip.is_ipv6());
    ips.into_iter().map(|ip| SocketAddr::new(ip, peer.port)).collect()
}

fn resolve_peer_host(peer: &DiscoveredPeer) -> Vec<SocketAddr> {
    use std::net::ToSocketAddrs;
    format!("{}:{}", peer.host.trim_end_matches('.'), peer.port)
        .to_socket_addrs()
        .map(|resolved| resolved.collect())
        .unwrap_or_default()
}

fn try_each_address<T>(
    peer: &DiscoveredPeer,
    mut attempt: impl FnMut(SocketAddr) -> Result<T, String>,
) -> Result<T, Vec<String>> {
    let mut tried = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for resolve in [candidate_addresses, resolve_peer_host] {
        for sock in resolve(peer) {
            if !seen.insert(sock) {
                continue;
            }
            match attempt(sock) {
                Ok(value) => return Ok(value),
                Err(err) => tried.push(format!("{sock} ({err})")),
            }
        }
    }
    if tried.is_empty() {
        tried.push(format!("{} advertised no usable address", peer.host));
    }
    Err(tried)
}

pub fn open_authenticated_channel(
    peer: &DiscoveredPeer,
    local_keys: &StaticKeypair,
    remote_static: &[u8; 32],
) -> Result<AuthenticatedChannel<TcpStream>, Vec<String>> {
    try_each_address(peer, |sock| {
        let stream = TcpStream::connect_timeout(&sock, PEER_CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
        let control = stream.try_clone().map_err(|e| e.to_string())?;
        control.set_nodelay(true).map_err(|e| e.to_string())?;
        let set_timeouts = |timeout: Duration| {
            control.set_read_timeout(Some(timeout))?;
            control.set_write_timeout(Some(timeout))
        };
        set_timeouts(HANDSHAKE_TIMEOUT).map_err(|e| e.to_string())?;
        let raw = connect_initiator(stream, local_keys, remote_static, |_| true).map_err(|e| e.to_string())?;
        set_timeouts(CONNECTION_IDLE_TIMEOUT).map_err(|e| e.to_string())?;
        Ok(raw)
    })
}

pub type SharedSink = Arc<dyn Fn(IpcEvent) + Send + Sync>;
pub type WorkerStoreFactory = Arc<dyn Fn() -> Result<(SqliteStore, StoreBackedSource), String> + Send + Sync>;

#[derive(Clone)]
pub struct PeerLink {
    pub local_keys: StaticKeypair,
    pub clock: ExpiryClock,
    pub presence: Arc<Presence>,
    pub emit: SharedSink,
    pub open_store: WorkerStoreFactory,
}

pub type PeerDirectory = Arc<Mutex<HashMap<String, DiscoveredPeer>>>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn announce_peer(emit: &EventSink, peer_id: &str) {
    emit(IpcEvent::Changed {
        resource: IpcResource::Peer,
        id: Some(peer_id.to_string()),
    });
}

fn remember_peer(directory: &PeerDirectory, peer: DiscoveredPeer) {
    let Some(fingerprint) = peer.fingerprint.clone() else {
        return;
    };
    let mut known = lock(directory);
    if known.len() < MAX_KNOWN_PEERS || known.contains_key(&fingerprint) {
        known.insert(fingerprint, peer);
    }
}

pub fn run_reappearance_loop(discovery: &Discovery, link: &PeerLink, directory: &PeerDirectory) {
    let events = match discovery.browse() {
        Ok(events) => events,
        Err(err) => {
            eprintln!("ferry-daemon: mDNS browse failed to start: {err}");
            return;
        }
    };
    let (store, _) = match (link.open_store)() {
        Ok(opened) => opened,
        Err(err) => {
            eprintln!("ferry-daemon: discovery could not open the store: {err}");
            return;
        }
    };
    let draining: Arc<Mutex<HashSet<String>>> = Arc::default();

    let mut next_redrive = Instant::now() + REDRIVE_INTERVAL;
    loop {
        match events.recv_timeout(next_redrive.saturating_duration_since(Instant::now())) {
            Ok(ServiceEvent::ServiceResolved(resolved)) => {
                let peer = peer_from_resolved(&resolved);
                remember_peer(directory, peer.clone());
                spawn_drain(peer, link, &draining);
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                let gone: Vec<String> = lock(directory)
                    .iter()
                    .filter(|(_, peer)| peer.fullname == fullname)
                    .map(|(fingerprint, _)| fingerprint.clone())
                    .collect();
                for fingerprint in gone {
                    if link.presence.contact_failed(&fingerprint) {
                        announce_peer(link.emit.as_ref(), &fingerprint);
                    }
                }
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_redrive {
            next_redrive = Instant::now() + REDRIVE_INTERVAL;
            let known: Vec<DiscoveredPeer> = lock(directory).values().cloned().collect();
            for peer in known {
                if peer_has_pending_work(&store, &peer) {
                    spawn_drain(peer, link, &draining);
                }
            }
        }
    }
}

fn spawn_drain(peer: DiscoveredPeer, link: &PeerLink, draining: &Arc<Mutex<HashSet<String>>>) {
    let Some(fingerprint) = peer.fingerprint.clone() else {
        return;
    };
    if !lock(draining).insert(fingerprint.clone()) {
        return;
    }
    let link = link.clone();
    let release = draining.clone();
    let released = fingerprint.clone();
    let spawned = std::thread::Builder::new().name("ferry-drain".into()).spawn(move || {
        match (link.open_store)() {
            Ok((mut store, mut source)) => {
                let remote_static = store.sealing_key_for_peer_id(&fingerprint);
                handle_reappeared_peer(
                    &peer,
                    &link.local_keys,
                    &mut store,
                    &link.clock,
                    &mut source,
                    &|_: &str| remote_static,
                    link.emit.as_ref(),
                    &link.presence,
                );
            }
            Err(err) => eprintln!("ferry-daemon: could not open the store to reach {fingerprint}: {err}"),
        }
        lock(&release).remove(&fingerprint);
    });
    if let Err(err) = spawned {
        eprintln!("ferry-daemon: could not start a delivery thread for {released}: {err}");
        lock(draining).remove(&released);
    }
}

pub fn run_presence_loop(link: &PeerLink, directory: &PeerDirectory) {
    let (store, _) = match (link.open_store)() {
        Ok(opened) => opened,
        Err(err) => {
            eprintln!("ferry-daemon: the presence probe could not open the store: {err}");
            return;
        }
    };
    let mut was_reachable: HashMap<String, bool> = HashMap::new();
    loop {
        let known: Vec<DiscoveredPeer> = lock(directory).values().cloned().collect();
        let due: Vec<(String, DiscoveredPeer, [u8; 32])> = known
            .into_iter()
            .filter_map(|peer| {
                let fingerprint = peer.fingerprint.clone()?;
                let remote_static = store.sealing_key_for_peer_id(&fingerprint)?;
                link.presence
                    .contact_is_due(&fingerprint, PROBE_INTERVAL_MILLIS)
                    .then_some((fingerprint, peer, remote_static))
            })
            .collect();

        std::thread::scope(|scope| {
            for (fingerprint, peer, remote_static) in &due {
                scope.spawn(move || {
                    match open_authenticated_channel(peer, &link.local_keys, remote_static) {
                        Ok(_) => link.presence.seen(fingerprint),
                        Err(_) => link.presence.contact_failed(fingerprint),
                    };
                });
            }
        });

        if let Ok(peers) = ferry_store::roster::list_peers(store.conn()) {
            for peer in peers {
                let reachable = link.presence.is_reachable(&peer.peer_id);
                if was_reachable.insert(peer.peer_id.clone(), reachable).unwrap_or(false) != reachable {
                    announce_peer(link.emit.as_ref(), &peer.peer_id);
                }
            }
        }
        std::thread::sleep(PROBE_TICK);
    }
}

fn peer_has_pending_work(store: &impl Store, peer: &DiscoveredPeer) -> bool {
    let Some(fingerprint) = &peer.fingerprint else {
        return false;
    };
    store
        .list_outbox_for_peer(fingerprint)
        .map(|entries| !entries.is_empty())
        .unwrap_or(false)
        || store
            .list_open_receipts_for_peer(fingerprint)
            .map(|receipts| !receipts.is_empty())
            .unwrap_or(false)
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
    for _ in 0..MAX_DRAIN_ROUNDS {
        if !drain_round(peer, fingerprint, &remote_static, local_keys, store, clock, source, emit, presence) {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drain_round(
    peer: &DiscoveredPeer,
    fingerprint: &str,
    remote_static: &[u8; 32],
    local_keys: &StaticKeypair,
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    emit: &EventSink,
    presence: &Presence,
) -> bool {
    let raw = match open_authenticated_channel(peer, local_keys, remote_static) {
        Ok(raw) => raw,
        Err(tried) => {
            eprintln!("ferry-daemon: could not reach peer {fingerprint}: tried {}", tried.join(", "));
            if presence.contact_failed(fingerprint) {
                announce_peer(emit, fingerprint);
            }
            return false;
        }
    };
    if presence.seen(fingerprint) {
        announce_peer(emit, fingerprint);
    }
    let mut channel = NetChannel::new(raw);

    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let mut on_progress = |item_id: &str, bytes: u64, total: u64| {
        let now = Instant::now();
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
                .chain(outcome.failed.iter().map(|(item_id, _)| item_id))
                .chain(outcome.deferred.iter())
            {
                emit(IpcEvent::Changed {
                    resource: IpcResource::Transfer,
                    id: Some(item_id.clone()),
                });
            }
            !outcome.awaiting_decision.is_empty() && !outcome.deferred.is_empty()
        }
        Err(err) => {
            eprintln!("ferry-daemon: failed to drain outbox to {fingerprint}: {err}");
            false
        }
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

    fn connect_to_peer(peer: &DiscoveredPeer) -> Result<TcpStream, Vec<String>> {
        try_each_address(peer, |sock| {
            TcpStream::connect_timeout(&sock, PEER_CONNECT_TIMEOUT).map_err(|e| e.to_string())
        })
    }

    #[test]
    fn an_address_that_answers_with_the_wrong_identity_does_not_hide_the_one_that_is_the_peer() {
        let Some(lan_ip) = lan_ip() else { return };
        let real = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = real.local_addr().unwrap().port();
        let Ok(impostor) = TcpListener::bind((lan_ip, port)) else { return };

        let local_keys = generate_keypair();
        let local_public = local_keys.public;
        let remote_keys = generate_keypair();
        let remote_public = remote_keys.public;
        let stranger_keys = generate_keypair();

        std::thread::spawn(move || {
            let (stream, _) = impostor.accept().unwrap();
            let _ = accept_responder(stream, &stranger_keys, |_| true);
        });
        let responder = std::thread::spawn(move || {
            let (stream, _) = real.accept().unwrap();
            accept_responder(stream, &remote_keys, move |remote: &[u8; 32]| *remote == local_public).is_ok()
        });

        let peer = DiscoveredPeer {
            fullname: "peer._ferry._tcp.local.".into(),
            host: "127.0.0.1".into(),
            addresses: vec![lan_ip],
            port,
            fingerprint: Some("peer".into()),
            protocol_version: None,
        };
        let channel = open_authenticated_channel(&peer, &local_keys, &remote_public)
            .expect("the second address is the real peer and must still be tried");
        assert_eq!(channel.remote_static, remote_public);
        assert!(responder.join().unwrap());
    }

    fn lan_ip() -> Option<std::net::IpAddr> {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("8.8.8.8:80").ok()?;
        s.local_addr().ok().map(|a| a.ip()).filter(usable_peer_address)
    }

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
            addresses: vec![addr.ip()],
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
            addresses: vec![],
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

    #[test]
    fn peer_has_pending_work_gates_the_periodic_redrive_on_a_non_empty_outbox() {
        use ferry_core::ports::Store;

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-with-mail", "laptop", "sk", "xk").unwrap();
        let mut store = crate::store_adapter::SqliteStore::new(
            conn,
            std::env::temp_dir().join(format!("ferry-redrive-test-{}", uuid::Uuid::now_v7())),
            ferry_crypto::identity::Identity::generate(),
        );

        let peer = |fp: &str| DiscoveredPeer {
            fullname: format!("{fp}._ferry._tcp.local."),
            host: "127.0.0.1".into(),
            addresses: vec![],
            port: 1,
            fingerprint: Some(fp.into()),
            protocol_version: None,
        };

        assert!(!peer_has_pending_work(&store, &peer("peer-with-mail")));

        store
            .create_and_enqueue_outbound(
                &NewOutboundItem {
                    peer_id: "peer-with-mail".into(),
                    kind: ItemKind::File,
                    name: "notes.txt".into(),
                    size_bytes: 3,
                    hash: "deadbeef".into(),
                    ttl_secs: 600,
                    is_burn_after_read: false,
                    notify_on_open: false,
                    source_path: "/tmp/notes.txt".into(),
                },
                "local",
            )
            .unwrap();

        assert!(peer_has_pending_work(&store, &peer("peer-with-mail")));
        assert!(!peer_has_pending_work(&store, &peer("some-other-peer")));
    }

    #[test]
    fn an_unreachable_advertised_address_fails_fast_instead_of_stalling_the_discovery_thread() {
        let peer = DiscoveredPeer {
            fullname: "ghost._ferry._tcp.local.".into(),
            host: "198.51.100.2".into(),
            addresses: vec!["198.51.100.1".parse().unwrap()],
            port: 47821,
            fingerprint: Some("ghost".into()),
            protocol_version: Some("2".into()),
        };
        let started = Instant::now();
        let result = connect_to_peer(&peer);
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(PEER_CONNECT_TIMEOUT.as_secs() * 2 + 4),
            "a dead address must be bounded by the connect timeout, not the OS default (took {:?})",
            started.elapsed()
        );
    }

    #[test]
    fn loopback_and_link_local_addresses_are_never_dialled_and_a_real_one_wins() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let lan_ip = {
            let s = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
            s.connect("8.8.8.8:80").ok().and_then(|_| s.local_addr().ok()).map(|a| a.ip())
        };
        let Some(lan_ip) = lan_ip.filter(usable_peer_address) else {
            return;
        };
        let peer = DiscoveredPeer {
            fullname: "self._ferry._tcp.local.".into(),
            host: "self.local.".into(),
            addresses: vec!["127.0.0.1".parse().unwrap(), "169.254.10.10".parse().unwrap(), lan_ip],
            port,
            fingerprint: Some("self".into()),
            protocol_version: Some("2".into()),
        };
        let accept = std::thread::spawn(move || listener.accept().map(|(_, from)| from.ip()));
        let stream = connect_to_peer(&peer).expect("the LAN address should connect");
        drop(stream);
        let from = accept.join().unwrap().unwrap();
        assert!(!from.is_loopback(), "must have connected over the real interface, got {from}");
    }
}
