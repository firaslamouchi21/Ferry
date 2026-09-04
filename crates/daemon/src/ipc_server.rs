use std::path::{Path, PathBuf};

use ferry_net::local_ipc::{self, Listener, Stream};

use ferry_core::expiry::ExpiryClock;
use ferry_core::ipc::RuntimeStatus;
use ferry_core::ports::Store;
use ferry_proto::errors::{ErrorCode, FerryError};
use ferry_proto::ipc::{
    IpcEnvelope, IpcEvent, IpcOutcome, IpcRequest, IpcResource, IpcResponse, IpcResult, RequestId,
};
use thiserror::Error;

use crate::event_bus::EventBus;
use crate::pairing::PairingRegistry;

#[derive(Debug, Error)]
pub enum IpcServerError {
    #[error("failed to bind IPC socket at {path}: {source}")]
    Bind { path: PathBuf, source: std::io::Error },
    #[error("failed to accept a connection: {0}")]
    Accept(std::io::Error),
}

pub fn bind(socket_path: &Path) -> Result<Listener, IpcServerError> {
    local_ipc::bind(socket_path).map_err(|source| IpcServerError::Bind {
        path: socket_path.to_path_buf(),
        source,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn serve(
    listener: &Listener,
    store: &mut impl Store,
    clock: &ExpiryClock,
    runtime: &RuntimeStatus,
    event_bus: &EventBus,
    pairing: &PairingRegistry,
    source: &mut impl ferry_core::ports::OutboundSource,
) -> Result<(), IpcServerError> {
    loop {
        let stream = local_ipc::accept(listener).map_err(IpcServerError::Accept)?;
        if handle_connection(stream, store, clock, source, runtime, event_bus, pairing) {
            return Ok(());
        }
    }
}

fn spawn_event_stream(mut stream: Stream, event_bus: &EventBus) {
    let events = event_bus.subscribe();
    std::thread::spawn(move || {
        for event in events {
            let Ok(bytes) = serde_json::to_vec(&event) else { continue };
            if ferry_net::framing::write_frame(&mut stream, &bytes).is_err() {
                break;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_connection(
    mut stream: Stream,
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl ferry_core::ports::OutboundSource,
    runtime: &RuntimeStatus,
    event_bus: &EventBus,
    pairing: &PairingRegistry,
) -> bool {
    let frame = match ferry_net::framing::read_frame(&mut stream) {
        Ok(frame) => frame,
        Err(err) => {
            eprintln!("ferry-daemon: dropping IPC connection — failed to read frame: {err}");
            return false;
        }
    };

    let envelope: IpcEnvelope = match serde_json::from_slice(&frame) {
        Ok(envelope) => envelope,
        Err(err) => {
            eprintln!("ferry-daemon: dropping IPC connection — malformed request: {err}");
            return false;
        }
    };

    if matches!(envelope.request, IpcRequest::Subscribe) {
        spawn_event_stream(stream, event_bus);
        return false;
    }

    if is_pairing_request(&envelope.request) {
        let response = handle_pairing(&envelope.request_id, envelope.request.clone(), pairing, store, event_bus);
        write_response(&mut stream, &response);
        return false;
    }

    let quit = ferry_core::ipc::is_quit(&envelope.request);
    let request = envelope.request.clone();
    let response = ferry_core::ipc::handle(
        store,
        clock,
        source,
        runtime,
        envelope.request_id,
        envelope.ipc_protocol_version,
        envelope.request,
    );

    write_response(&mut stream, &response);

    if let Some(event) = event_for(&request, &response) {
        event_bus.emit(event);
    }

    quit
}

fn write_response(stream: &mut Stream, response: &IpcResponse) {
    match serde_json::to_vec(response) {
        Ok(bytes) => {
            if let Err(err) = ferry_net::framing::write_frame(stream, &bytes) {
                eprintln!("ferry-daemon: failed to write IPC response: {err}");
            }
        }
        Err(err) => eprintln!("ferry-daemon: failed to encode IPC response: {err}"),
    }
}

fn is_pairing_request(request: &IpcRequest) -> bool {
    matches!(
        request,
        IpcRequest::PairBegin { .. }
            | IpcRequest::PairStatus { .. }
            | IpcRequest::PairConfirm { .. }
            | IpcRequest::PairCancel { .. }
    )
}

fn ok(request_id: &RequestId, value: IpcResult) -> IpcResponse {
    IpcResponse {
        request_id: request_id.clone(),
        outcome: IpcOutcome::Ok { value },
    }
}

fn pairing_err(request_id: &RequestId, message: String) -> IpcResponse {
    IpcResponse {
        request_id: request_id.clone(),
        outcome: IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::Internal,
                message,
            },
        },
    }
}

fn handle_pairing(
    request_id: &RequestId,
    request: IpcRequest,
    pairing: &PairingRegistry,
    store: &mut impl Store,
    event_bus: &EventBus,
) -> IpcResponse {
    match request {
        IpcRequest::PairBegin { mode } => match pairing.begin(mode) {
            Ok(view) => ok(request_id, IpcResult::PairBegin(view)),
            Err(e) => pairing_err(request_id, e),
        },
        IpcRequest::PairStatus { pairing_id } => {
            if let Some(peer) = pairing.take_pending_persist(&pairing_id) {
                if let Err(e) = store.add_paired_peer(&peer) {
                    return pairing_err(request_id, format!("pairing succeeded but the peer could not be saved: {e}"));
                }
                event_bus.emit(IpcEvent::Changed {
                    resource: IpcResource::Roster,
                    id: None,
                });
            }
            match pairing.status_view(&pairing_id) {
                Some(view) => ok(request_id, IpcResult::PairStatus(view)),
                None => pairing_err(request_id, "no such pairing session".into()),
            }
        }
        IpcRequest::PairConfirm { pairing_id, accept } => match pairing.confirm(&pairing_id, accept) {
            Ok(()) => ok(request_id, IpcResult::Ack),
            Err(e) => pairing_err(request_id, e),
        },
        IpcRequest::PairCancel { pairing_id } => {
            pairing.cancel(&pairing_id);
            ok(request_id, IpcResult::Ack)
        }
        _ => pairing_err(request_id, "not a pairing request".into()),
    }
}

fn event_for(request: &IpcRequest, response: &IpcResponse) -> Option<IpcEvent> {
    let value = match &response.outcome {
        IpcOutcome::Ok { value } => value,
        IpcOutcome::Err { .. } => return None,
    };

    let (resource, id) = match (request, value) {
        (IpcRequest::Send { .. }, IpcResult::Send { item_id }) => {
            (IpcResource::Transfer, Some(item_id.clone()))
        }
        (IpcRequest::ConfirmOpened { item_id }, _) => (IpcResource::Transfer, Some(item_id.clone())),
        (IpcRequest::ImportSealed { .. }, IpcResult::ImportSealed(view)) => {
            (IpcResource::Transfer, Some(view.item_id.clone()))
        }
        (IpcRequest::RosterImport { .. }, _) | (IpcRequest::PairComplete { .. }, _) => {
            (IpcResource::Roster, None)
        }
        (IpcRequest::PeerRemove { peer_id }, _) => (IpcResource::Peer, Some(peer_id.clone())),
        (IpcRequest::InboxAccept { item_id }, _) | (IpcRequest::InboxReject { item_id }, _) => {
            (IpcResource::Transfer, Some(item_id.clone()))
        }
        (IpcRequest::SentAbort { item_id }, _) | (IpcRequest::SentRetry { item_id }, _) => {
            (IpcResource::Transfer, Some(item_id.clone()))
        }
        (IpcRequest::SendInline { .. }, IpcResult::Send { item_id }) => {
            (IpcResource::Transfer, Some(item_id.clone()))
        }
        _ => return None,
    };

    Some(IpcEvent::Changed { resource, id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;
    use ferry_core::ports::{NewOutboundItem, OutboxItem, StoreError};
    use ferry_proto::ipc::{IpcOutcome, IpcRequest, IpcResponse, IpcResult, RequestId, IPC_PROTOCOL_VERSION};
    use ferry_proto::states::TransferState;
    use std::io::{Read, Write};

    struct FakeStore;

    impl Store for FakeStore {
        fn is_peer_authorized(&self, _peer_id: &str) -> Result<bool, StoreError> {
            unimplemented!()
        }
        fn create_and_enqueue_outbound(&mut self, _item: &NewOutboundItem, _actor: &str) -> Result<String, StoreError> {
            unimplemented!()
        }
        fn get_outbound_state(&self, _item_id: &str) -> Result<Option<TransferState>, StoreError> {
            unimplemented!()
        }
        fn set_outbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn create_inbound(
            &mut self,
            _offer: &ferry_proto::envelope::Offer,
            _peer_id: &str,
            _actor: &str,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn get_inbound_state(&self, _item_id: &str) -> Result<Option<TransferState>, StoreError> {
            unimplemented!()
        }
        fn set_inbound_state(&mut self, _item_id: &str, _state: TransferState) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn inbound_bytes_received(&self, _item_id: &str) -> Result<u64, StoreError> {
            unimplemented!()
        }
        fn append_inbound_chunk(&mut self, _item_id: &str, _seq: u64, _bytes: &[u8]) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn inbound_full_hash_so_far(&self, _item_id: &str) -> Result<String, StoreError> {
            unimplemented!()
        }
        fn finalize_inbound_delivered(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn mark_inbound_opened(&mut self, _item_id: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn get_outbound_item(&self, _item_id: &str) -> Result<Option<NewOutboundItem>, StoreError> {
            unimplemented!()
        }
        fn list_outbox_for_peer(&self, _peer_id: &str) -> Result<Vec<OutboxItem>, StoreError> {
            unimplemented!()
        }
        fn record_outbox_attempt(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn remove_outbox_entry(&mut self, _outbox_id: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn set_inbound_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &ferry_core::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn get_inbound_expiry(&self, _item_id: &str) -> Result<Option<ferry_core::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!()
        }
        fn set_outbox_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &ferry_core::expiry::ExpiryDeadline,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn get_outbox_expiry(&self, _item_id: &str) -> Result<Option<ferry_core::expiry::ExpiryDeadline>, StoreError> {
            unimplemented!()
        }
        fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
            unimplemented!()
        }
        fn read_inbound_plaintext(&self, _item_id: &str) -> Result<Vec<u8>, StoreError> {
            unimplemented!()
        }
        fn is_healthy(&self) -> Result<bool, StoreError> {
            Ok(true)
        }

        fn list_roster_peers(&self) -> Result<Vec<ferry_core::ports::RosterPeer>, StoreError> {
            unimplemented!()
        }

        fn export_signed_roster(&self) -> Result<String, StoreError> {
            unimplemented!()
        }

        fn import_signed_roster(&mut self, _signed_roster_json: &str) -> Result<ferry_core::ports::RosterImportSummary, StoreError> {
            unimplemented!()
        }

        fn add_paired_peer(&mut self, _peer: &ferry_core::ports::NewRosterPeer) -> Result<(), StoreError> {
            unimplemented!()
        }

        fn list_inbox_items(&self) -> Result<Vec<ferry_core::ports::InboxItemSummary>, StoreError> {
            unimplemented!()
        }
    }

    fn temp_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("ferry-ipc-test-{}.sock", uuid::Uuid::now_v7()))
    }

    fn send_request(socket_path: &Path, request: IpcRequest, ipc_protocol_version: u16) -> IpcResponse {
        let mut stream = local_ipc::connect(socket_path).unwrap();
        let envelope = IpcEnvelope {
            ipc_protocol_version,
            request_id: RequestId("req-1".into()),
            request,
        };
        let bytes = serde_json::to_vec(&envelope).unwrap();
        ferry_net::framing::write_frame(&mut stream, &bytes).unwrap();
        stream.flush().unwrap();

        let response_bytes = ferry_net::framing::read_frame(&mut stream).unwrap();
        serde_json::from_slice(&response_bytes).unwrap()
    }

    #[test]
    fn a_real_client_gets_a_real_status_response_over_a_real_unix_socket() {
        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut store = FakeStore;
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &EventBus::new(), &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
        });

        let response = send_request(&socket_path, IpcRequest::Status, IPC_PROTOCOL_VERSION);
        assert_eq!(response.request_id, RequestId("req-1".into()));
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Status(status) } => assert!(status.store_ok),
            other => panic!("expected Ok(Status), got {other:?}"),
        }

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn a_client_speaking_the_wrong_ipc_protocol_version_gets_a_clear_error_not_a_dropped_connection() {
        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut store = FakeStore;
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &EventBus::new(), &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
        });

        let response = send_request(&socket_path, IpcRequest::Status, IPC_PROTOCOL_VERSION + 1);
        assert!(matches!(response.outcome, IpcOutcome::Err { .. }));

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn a_stale_socket_file_from_a_previous_run_is_replaced_not_fatal() {
        let socket_path = temp_socket_path();
        std::fs::write(&socket_path, b"not a socket").unwrap();

        let listener = bind(&socket_path).unwrap();
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn a_malformed_frame_drops_the_connection_without_crashing_the_server() {
        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut store = FakeStore;
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &EventBus::new(), &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
        });

        let mut stream = local_ipc::connect(&socket_path).unwrap();
        ferry_net::framing::write_frame(&mut stream, b"not json").unwrap();
        stream.flush().unwrap();
        let mut buf = [0u8; 1];
        let closed = matches!(stream.read(&mut buf), Ok(0) | Err(_));
        assert!(closed, "the server must close the connection, not respond to garbage");

        let response = send_request(&socket_path, IpcRequest::Status, IPC_PROTOCOL_VERSION);
        assert!(matches!(
            response.outcome,
            IpcOutcome::Ok { value: IpcResult::Status(_) }
        ));

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn a_real_send_request_hashes_a_real_file_and_lands_in_a_real_sqlite_outbox() {
        use crate::store_adapter::SqliteStore;

        let source_path = std::env::temp_dir().join(format!("ferry-ipc-send-test-{}.bin", uuid::Uuid::now_v7()));
        std::fs::write(&source_path, b"ferry over ipc").unwrap();

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let store_dir = std::env::temp_dir().join(format!("ferry-ipc-send-payloads-{}", uuid::Uuid::now_v7()));

        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut store = SqliteStore::new(conn, store_dir, ferry_crypto::identity::Identity::generate());
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &EventBus::new(), &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
            store
        });

        let response = send_request(
            &socket_path,
            IpcRequest::Send {
                peer_id: "peer-b".into(),
                source_path: source_path.to_string_lossy().into_owned(),
                name: "message.bin".into(),
                ttl_secs: 600,
                is_burn_after_read: false,
                notify_on_open: false,
            },
            IPC_PROTOCOL_VERSION,
        );

        let item_id = match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Send { item_id } } => item_id,
            other => panic!("expected Ok(Send), got {other:?}"),
        };
        assert!(!item_id.is_empty());

        let unauthorized_response = send_request(
            &socket_path,
            IpcRequest::Send {
                peer_id: "no-such-peer".into(),
                source_path: source_path.to_string_lossy().into_owned(),
                name: "message.bin".into(),
                ttl_secs: 600,
                is_burn_after_read: false,
                notify_on_open: false,
            },
            IPC_PROTOCOL_VERSION,
        );
        match unauthorized_response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ferry_proto::errors::ErrorCode::PeerNotAuthorized),
            other => panic!("expected Err(PeerNotAuthorized), got {other:?}"),
        }

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        let store = server_thread.join().unwrap();

        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"ferry over ipc");
        let expected_hash = hex::encode(hasher.finalize());

        let stored = store.get_outbound_item(&item_id).unwrap().unwrap();
        assert_eq!(stored.hash, expected_hash);
        assert_eq!(stored.size_bytes, 14);
        assert_eq!(stored.source_path, source_path.to_string_lossy());
        assert_eq!(store.get_outbound_state(&item_id).unwrap(), Some(TransferState::Queued));

        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_file(&source_path);
    }

    #[test]
    fn a_subscribed_client_receives_a_changed_event_when_another_client_makes_a_state_change() {
        use crate::store_adapter::SqliteStore;
        use std::sync::Arc;

        let source_path = std::env::temp_dir().join(format!("ferry-ipc-evt-{}.bin", uuid::Uuid::now_v7()));
        std::fs::write(&source_path, b"watch me change").unwrap();

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let store_dir = std::env::temp_dir().join(format!("ferry-ipc-evt-payloads-{}", uuid::Uuid::now_v7()));

        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let bus = Arc::new(EventBus::new());
        let bus_for_server = bus.clone();
        let server_thread = std::thread::spawn(move || {
            let mut store = SqliteStore::new(conn, store_dir, ferry_crypto::identity::Identity::generate());
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &bus_for_server, &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
        });

        let mut sub = local_ipc::connect(&socket_path).unwrap();
        let sub_envelope = IpcEnvelope {
            ipc_protocol_version: IPC_PROTOCOL_VERSION,
            request_id: RequestId("sub".into()),
            request: IpcRequest::Subscribe,
        };
        ferry_net::framing::write_frame(&mut sub, &serde_json::to_vec(&sub_envelope).unwrap()).unwrap();
        sub.flush().unwrap();

        while bus.subscriber_count() == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let send = send_request(
            &socket_path,
            IpcRequest::Send {
                peer_id: "peer-b".into(),
                source_path: source_path.to_string_lossy().into_owned(),
                name: "watched.bin".into(),
                ttl_secs: 600,
                is_burn_after_read: false,
                notify_on_open: false,
            },
            IPC_PROTOCOL_VERSION,
        );
        let item_id = match send.outcome {
            IpcOutcome::Ok { value: IpcResult::Send { item_id } } => item_id,
            other => panic!("expected Ok(Send), got {other:?}"),
        };

        let event_bytes = ferry_net::framing::read_frame(&mut sub).unwrap();
        let event: IpcEvent = serde_json::from_slice(&event_bytes).unwrap();
        assert_eq!(
            event,
            IpcEvent::Changed {
                resource: IpcResource::Transfer,
                id: Some(item_id),
            }
        );

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_file(&source_path);
    }

    #[test]
    fn a_real_inbox_list_and_open_request_round_trips_through_a_real_sqlite_store() {
        use crate::store_adapter::SqliteStore;

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let payload_dir = std::env::temp_dir().join(format!("ferry-ipc-inbox-test-{}", uuid::Uuid::now_v7()));

        let item_id = ferry_store::inbox::insert(
            &conn,
            &ferry_store::inbox::NewInboxItem {
                peer_id: "peer-a".into(),
                kind: ferry_proto::states::ItemKind::File,
                name: "arrived.txt".into(),
                size_bytes: 11,
                hash: "irrelevant".into(),
                is_burn_after_read: true,
                notify_on_open: false,
            },
        )
        .unwrap();
        ferry_store::inbox::set_delivered(&conn, &item_id).unwrap();
        ferry_store::payload::append(&payload_dir, &item_id, b"hello ferry").unwrap();

        let socket_path = temp_socket_path();
        let listener = bind(&socket_path).unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut store = SqliteStore::new(conn, payload_dir, ferry_crypto::identity::Identity::generate());
            let clock = ExpiryClock::new();
            serve(&listener, &mut store, &clock, &RuntimeStatus::default(), &EventBus::new(), &crate::pairing::PairingRegistry::new(ferry_crypto::identity::Identity::generate()), &mut crate::file_source::FilePathSource::new(ferry_crypto::identity::Identity::generate())).unwrap();
        });

        let list_response = send_request(&socket_path, IpcRequest::InboxList, IPC_PROTOCOL_VERSION);
        match list_response.outcome {
            IpcOutcome::Ok { value: IpcResult::InboxList(items) } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].item_id, item_id);
                assert_eq!(items[0].name, "arrived.txt");
                assert_eq!(items[0].state, TransferState::Delivered);
            }
            other => panic!("expected Ok(InboxList), got {other:?}"),
        }

        let open_response = send_request(&socket_path, IpcRequest::Open { item_id: item_id.clone() }, IPC_PROTOCOL_VERSION);
        match open_response.outcome {
            IpcOutcome::Ok { value: IpcResult::Open { content_base64 } } => {
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD.decode(&content_base64).unwrap();
                assert_eq!(decoded, b"hello ferry");
            }
            other => panic!("expected Ok(Open), got {other:?}"),
        }

        let repeat_peek = send_request(&socket_path, IpcRequest::Open { item_id: item_id.clone() }, IPC_PROTOCOL_VERSION);
        assert!(
            matches!(repeat_peek.outcome, IpcOutcome::Ok { .. }),
            "a peek must be repeatable before the client confirms it safely delivered the content"
        );

        let confirm = send_request(&socket_path, IpcRequest::ConfirmOpened { item_id: item_id.clone() }, IPC_PROTOCOL_VERSION);
        assert!(matches!(confirm.outcome, IpcOutcome::Ok { .. }));

        let second_confirm = send_request(&socket_path, IpcRequest::ConfirmOpened { item_id: item_id.clone() }, IPC_PROTOCOL_VERSION);
        assert!(
            matches!(second_confirm.outcome, IpcOutcome::Err { .. }),
            "a burn-after-read item must not be confirmed-open twice"
        );

        send_request(&socket_path, IpcRequest::Quit, IPC_PROTOCOL_VERSION);
        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }
}
