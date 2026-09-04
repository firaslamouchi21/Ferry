use ferry_proto::envelope::PROTOCOL_VERSION;
use ferry_proto::errors::{ErrorCode, FerryError};
use ferry_proto::ipc::{
    AuditEventView, DaemonStatus, IdentityView, InboxItemView, IpcOutcome, IpcRequest, IpcResponse,
    IpcResult, RosterImportSummaryView, RosterPeerView, SentItemView, IPC_PROTOCOL_VERSION,
};
use ferry_proto::states::ItemKind;

use base64::Engine;

use crate::expiry::ExpiryClock;
use crate::outbox::{enqueue_send_from_path, OutboxError, SendRequest};
use crate::policy::PolicyError;
use crate::ports::{NewRosterPeer, OutboundSource, Store};
use crate::transfer::{
    accept_inbound_offer, check_openable, open_item_locally, reject_inbound_offer, TransferError,
};

#[derive(Debug, Clone, Default)]
pub struct RuntimeIdentity {
    pub fingerprint: String,
    pub signing_key_hex: String,
    pub sealing_key: String,
    pub display_name: String,
    pub listen_port: u16,
    pub data_dir: String,
    pub auto_accept_from_roster: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    pub transport_ok: bool,
    pub discovery_ok: bool,
    pub identity: RuntimeIdentity,
}

pub fn handle(
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    runtime: &RuntimeStatus,
    request_id: ferry_proto::ipc::RequestId,
    ipc_protocol_version: u16,
    request: IpcRequest,
) -> IpcResponse {
    if ipc_protocol_version != IPC_PROTOCOL_VERSION {
        return IpcResponse {
            request_id,
            outcome: IpcOutcome::Err {
                error: FerryError {
                    code: ErrorCode::ProtocolVersionMismatch,
                    message: format!(
                        "client speaks IPC protocol {ipc_protocol_version}, daemon speaks {IPC_PROTOCOL_VERSION}"
                    ),
                },
            },
        };
    }

    let outcome = match request {
        IpcRequest::Status => status_outcome(store, runtime),
        IpcRequest::Quit => IpcOutcome::Ok { value: IpcResult::Ack },
        IpcRequest::RosterList => roster_list_outcome(store),
        IpcRequest::RosterExport => roster_export_outcome(store),
        IpcRequest::RosterImport { signed_roster_json } => roster_import_outcome(store, &signed_roster_json),
        IpcRequest::Send {
            peer_id,
            source_path,
            name,
            ttl_secs,
            is_burn_after_read,
            notify_on_open,
        } => send_outcome(
            store,
            clock,
            source,
            SendRequest {
                peer_id,
                source_path,
                name,
                kind: ItemKind::File,
                ttl_secs,
                is_burn_after_read,
                notify_on_open,
            },
        ),
        IpcRequest::PairComplete {
            peer_id,
            display_name,
            signing_key_hex,
            sealing_key,
        } => pair_complete_outcome(
            store,
            &NewRosterPeer {
                peer_id,
                display_name,
                signing_key_hex,
                sealing_key,
            },
        ),
        IpcRequest::SendInline {
            peer_id,
            name,
            kind,
            content_base64,
            ttl_secs,
            is_burn_after_read,
            notify_on_open,
        } => send_inline_outcome(
            store,
            clock,
            peer_id,
            name,
            kind,
            &content_base64,
            ttl_secs,
            is_burn_after_read,
            notify_on_open,
        ),
        IpcRequest::InboxList => inbox_list_outcome(store),
        IpcRequest::InboxAccept { item_id } => match accept_inbound_offer(store, &item_id) {
            Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
            Err(err) => transfer_error_outcome(&item_id, err),
        },
        IpcRequest::InboxReject { item_id } => match reject_inbound_offer(store, &item_id) {
            Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
            Err(err) => transfer_error_outcome(&item_id, err),
        },
        IpcRequest::Open { item_id } => open_outcome(store, clock, &item_id),
        IpcRequest::ConfirmOpened { item_id } => confirm_opened_outcome(store, clock, &item_id),
        IpcRequest::ExportSealed { item_id } => export_sealed_outcome(store, &item_id),
        IpcRequest::ImportSealed { blob_base64 } => import_sealed_outcome(store, clock, &blob_base64),
        IpcRequest::Identity => identity_outcome(runtime),
        IpcRequest::SentList => sent_list_outcome(store),
        IpcRequest::SentAbort { item_id } => match store.abort_outbound(&item_id, "local") {
            Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
            Err(err) => internal_error(err.to_string()),
        },
        IpcRequest::SentRetry { item_id } => match store.retry_outbound(&item_id, "local") {
            Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
            Err(err) => internal_error(err.to_string()),
        },
        IpcRequest::AuditList { limit, before_millis } => audit_list_outcome(store, limit, before_millis),
        IpcRequest::PeerRemove { peer_id } => match store.remove_peer(&peer_id) {
            Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
            Err(err) => internal_error(err.to_string()),
        },
        IpcRequest::Subscribe => internal_error(
            "Subscribe is handled by the IPC server's event stream, not the request dispatcher".into(),
        ),
        IpcRequest::PairBegin { .. }
        | IpcRequest::PairStatus { .. }
        | IpcRequest::PairConfirm { .. }
        | IpcRequest::PairCancel { .. } => internal_error(
            "pairing requests are handled by the daemon's pairing registry, not the request dispatcher".into(),
        ),
    };

    IpcResponse { request_id, outcome }
}

fn status_outcome(store: &impl Store, runtime: &RuntimeStatus) -> IpcOutcome {
    let store_ok = store.is_healthy().unwrap_or(false);
    IpcOutcome::Ok {
        value: IpcResult::Status(DaemonStatus {
            protocol_version: PROTOCOL_VERSION,
            discovery_ok: runtime.discovery_ok,
            transport_ok: runtime.transport_ok,
            store_ok,
        }),
    }
}

fn roster_list_outcome(store: &impl Store) -> IpcOutcome {
    match store.list_roster_peers() {
        Ok(peers) => IpcOutcome::Ok {
            value: IpcResult::RosterList(
                peers
                    .into_iter()
                    .map(|p| RosterPeerView {
                        fingerprint_short: short_fingerprint(&p.peer_id),
                        peer_id: p.peer_id,
                        display_name: p.display_name,
                        paired_at_millis: p.paired_at_millis,
                        reachable: p.reachable,
                        last_seen_millis: p.last_seen_millis,
                    })
                    .collect(),
            ),
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn roster_export_outcome(store: &impl Store) -> IpcOutcome {
    match store.export_signed_roster() {
        Ok(signed_roster_json) => IpcOutcome::Ok {
            value: IpcResult::RosterExport { signed_roster_json },
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn roster_import_outcome(store: &mut impl Store, signed_roster_json: &str) -> IpcOutcome {
    match store.import_signed_roster(signed_roster_json) {
        Ok(summary) => IpcOutcome::Ok {
            value: IpcResult::RosterImport(RosterImportSummaryView {
                signer_verifying_key_hex: summary.signer_verifying_key_hex,
                peer_count: summary.peer_count as u32,
                added: summary.added as u32,
                skipped_existing: summary.skipped_existing as u32,
            }),
        },
        Err(err) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::RosterInvalid,
                message: err.to_string(),
            },
        },
    }
}

fn send_outcome(
    store: &mut impl Store,
    clock: &ExpiryClock,
    source: &mut impl OutboundSource,
    request: SendRequest,
) -> IpcOutcome {
    match enqueue_send_from_path(store, clock, source, &request, "local") {
        Ok(item_id) => IpcOutcome::Ok {
            value: IpcResult::Send { item_id },
        },
        Err(OutboxError::Policy(PolicyError::PeerNotAuthorized)) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::PeerNotAuthorized,
                message: "peer is not in the roster — send denied".into(),
            },
        },
        Err(OutboxError::Policy(err @ PolicyError::ItemTooLargeForKind { .. })) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemRejectedByPolicy,
                message: err.to_string(),
            },
        },
        Err(OutboxError::Policy(PolicyError::Store(err))) => internal_error(err.to_string()),
        Err(OutboxError::Store(err)) => internal_error(err.to_string()),
        Err(OutboxError::Io(err)) => internal_error(format!("failed to read {}: {err}", request.source_path)),
    }
}

#[allow(clippy::too_many_arguments)]
fn send_inline_outcome(
    store: &mut impl Store,
    clock: &ExpiryClock,
    peer_id: String,
    name: String,
    kind: ItemKind,
    content_base64: &str,
    ttl_secs: u32,
    is_burn_after_read: bool,
    notify_on_open: bool,
) -> IpcOutcome {
    use sha2::{Digest, Sha256};

    let content = match base64::engine::general_purpose::STANDARD.decode(content_base64) {
        Ok(bytes) => bytes,
        Err(err) => return internal_error(format!("inline content is not valid base64: {err}")),
    };

    if let Err(err) = crate::policy::authorize_item_kind(kind, content.len() as u64) {
        return IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemRejectedByPolicy,
                message: err.to_string(),
            },
        };
    }

    let source_path = match store.stage_outbound_content(&content) {
        Ok(path) => path,
        Err(err) => return internal_error(err.to_string()),
    };

    let item = crate::ports::NewOutboundItem {
        peer_id,
        kind,
        name,
        size_bytes: content.len() as u64,
        hash: hex::encode(Sha256::digest(&content)),
        ttl_secs,
        is_burn_after_read,
        notify_on_open,
        source_path,
    };

    match crate::outbox::enqueue_send(store, clock, &item, "local") {
        Ok(item_id) => IpcOutcome::Ok {
            value: IpcResult::Send { item_id },
        },
        Err(OutboxError::Policy(PolicyError::PeerNotAuthorized)) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::PeerNotAuthorized,
                message: "peer is not in the roster — send denied".into(),
            },
        },
        Err(OutboxError::Policy(err @ PolicyError::ItemTooLargeForKind { .. })) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemRejectedByPolicy,
                message: err.to_string(),
            },
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn pair_complete_outcome(store: &mut impl Store, peer: &NewRosterPeer) -> IpcOutcome {
    match store.add_paired_peer(peer) {
        Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
        Err(err) => internal_error(err.to_string()),
    }
}

fn inbox_list_outcome(store: &impl Store) -> IpcOutcome {
    match store.list_inbox_items() {
        Ok(items) => IpcOutcome::Ok {
            value: IpcResult::InboxList(
                items
                    .into_iter()
                    .map(|item| InboxItemView {
                        origin_display_name: item.origin_display_name,
                        item_id: item.item_id,
                        peer_id: item.peer_id,
                        kind: item.kind,
                        name: item.name,
                        state: item.state,
                        size_bytes: item.size_bytes,
                        is_burn_after_read: item.is_burn_after_read,
                        hash_hex: item.hash_hex,
                        received_at_millis: item.received_at_millis,
                        expires_at_millis: item.expires_at_millis,
                    })
                    .collect(),
            ),
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn transfer_error_outcome(item_id: &str, err: TransferError) -> IpcOutcome {
    match err {
        TransferError::Expired(_) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemExpired,
                message: format!("item {item_id} has expired"),
            },
        },
        TransferError::UnknownItem(_) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemNotFound,
                message: format!("no such inbox item: {item_id}"),
            },
        },
        TransferError::IllegalTransition(err) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::IllegalStateTransition,
                message: err.to_string(),
            },
        },
        err => internal_error(err.to_string()),
    }
}

fn open_outcome(store: &mut impl Store, clock: &ExpiryClock, item_id: &str) -> IpcOutcome {
    if let Err(err) = check_openable(store, clock, item_id) {
        return transfer_error_outcome(item_id, err);
    }

    match store.read_inbound_plaintext(item_id) {
        Ok(plaintext) => IpcOutcome::Ok {
            value: IpcResult::Open {
                content_base64: base64::engine::general_purpose::STANDARD.encode(plaintext),
            },
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn confirm_opened_outcome(store: &mut impl Store, clock: &ExpiryClock, item_id: &str) -> IpcOutcome {
    match open_item_locally(store, clock, item_id) {
        Ok(()) => IpcOutcome::Ok { value: IpcResult::Ack },
        Err(err) => transfer_error_outcome(item_id, err),
    }
}

fn export_sealed_outcome(store: &impl Store, item_id: &str) -> IpcOutcome {
    match store.build_sealed_blob(item_id) {
        Ok(blob) => IpcOutcome::Ok {
            value: IpcResult::ExportSealed {
                blob_base64: base64::engine::general_purpose::STANDARD.encode(blob),
            },
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn import_sealed_outcome(store: &mut impl Store, clock: &ExpiryClock, blob_base64: &str) -> IpcOutcome {
    let blob = match base64::engine::general_purpose::STANDARD.decode(blob_base64) {
        Ok(bytes) => bytes,
        Err(err) => return internal_error(format!("sealed blob is not valid base64: {err}")),
    };

    let manifest = match store.open_sealed_blob(&blob) {
        Ok(manifest) => manifest,
        Err(err) => {
            return IpcOutcome::Err {
                error: FerryError {
                    code: ErrorCode::RosterInvalid,
                    message: err.to_string(),
                },
            }
        }
    };

    match crate::blob::import_manifest(store, clock, &manifest, "local") {
        Ok(summary) => IpcOutcome::Ok {
            value: IpcResult::ImportSealed(ferry_proto::ipc::SealedImportView {
                item_id: summary.item_id,
                origin_peer_id: summary.origin_peer_id,
                kind: summary.kind,
                name: summary.name,
                size_bytes: summary.size_bytes,
            }),
        },
        Err(crate::blob::ImportError::Policy(PolicyError::PeerNotAuthorized)) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::PeerNotAuthorized,
                message: "the sealed blob's origin peer is not in this machine's roster".into(),
            },
        },
        Err(err @ crate::blob::ImportError::Policy(PolicyError::ItemTooLargeForKind { .. })) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::ItemRejectedByPolicy,
                message: err.to_string(),
            },
        },
        Err(err @ crate::blob::ImportError::HashMismatch) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::HashMismatch,
                message: err.to_string(),
            },
        },
        Err(err @ crate::blob::ImportError::AlreadyImported(_)) => IpcOutcome::Err {
            error: FerryError {
                code: ErrorCode::IllegalStateTransition,
                message: err.to_string(),
            },
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn internal_error(message: String) -> IpcOutcome {
    IpcOutcome::Err {
        error: FerryError {
            code: ErrorCode::Internal,
            message,
        },
    }
}

fn short_fingerprint(peer_id: &str) -> String {
    let hex: String = peer_id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    if hex.len() == 8 {
        format!("{} {}", &hex[..4], &hex[4..])
    } else {
        peer_id.chars().take(9).collect()
    }
}

fn identity_outcome(runtime: &RuntimeStatus) -> IpcOutcome {
    let id = &runtime.identity;
    IpcOutcome::Ok {
        value: IpcResult::Identity(IdentityView {
            fingerprint: id.fingerprint.clone(),
            signing_key_hex: id.signing_key_hex.clone(),
            sealing_key: id.sealing_key.clone(),
            display_name: id.display_name.clone(),
            listen_port: id.listen_port,
            data_dir: id.data_dir.clone(),
            protocol_version: PROTOCOL_VERSION,
            auto_accept_from_roster: id.auto_accept_from_roster,
        }),
    }
}

fn sent_list_outcome(store: &impl Store) -> IpcOutcome {
    match store.list_sent_items() {
        Ok(items) => IpcOutcome::Ok {
            value: IpcResult::SentList(
                items
                    .into_iter()
                    .map(|s| SentItemView {
                        item_id: s.item_id,
                        peer_display_name: s.peer_display_name,
                        peer_id: s.peer_id,
                        name: s.name,
                        hash_hex: s.hash_hex,
                        kind: s.kind,
                        state: s.state,
                        size_bytes: s.size_bytes,
                        queued_at_millis: s.queued_at_millis,
                        last_attempt_at_millis: s.last_attempt_at_millis,
                        last_error: s.last_error,
                    })
                    .collect(),
            ),
        },
        Err(err) => internal_error(err.to_string()),
    }
}

fn audit_list_outcome(store: &impl Store, limit: u32, before_millis: Option<i64>) -> IpcOutcome {
    match store.list_audit(limit, before_millis) {
        Ok(events) => IpcOutcome::Ok {
            value: IpcResult::AuditList(
                events
                    .into_iter()
                    .map(|e| AuditEventView {
                        id: e.id,
                        actor: e.actor,
                        kind: e.kind,
                        item_id: e.item_id,
                        occurred_at_millis: e.occurred_at_millis,
                        outcome: e.outcome,
                    })
                    .collect(),
            ),
        },
        Err(err) => internal_error(err.to_string()),
    }
}

pub fn is_quit(request: &IpcRequest) -> bool {
    matches!(request, IpcRequest::Quit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::ipc::RequestId;

    struct FakeInboundItem {
        peer_id: String,
        kind: ItemKind,
        name: String,
        state: ferry_proto::states::TransferState,
        size_bytes: u64,
        is_burn_after_read: bool,
        bytes: Vec<u8>,
    }

    #[derive(Default)]
    struct FakeStore {
        healthy: bool,
        roster: Vec<crate::ports::RosterPeer>,
        authorized_peers: std::collections::HashSet<String>,
        outbound: std::collections::HashMap<String, crate::ports::NewOutboundItem>,
        inbound: std::collections::HashMap<String, FakeInboundItem>,
        next_id: u64,
        paired_peers: Vec<crate::ports::NewRosterPeer>,
        sent_items: Vec<crate::ports::SentItem>,
        audit: Vec<crate::ports::AuditRecord>,
        removed_peers: Vec<String>,
        staged: Vec<Vec<u8>>,
        aborted: Vec<String>,
        retried: Vec<String>,
    }

    impl Store for FakeStore {
        fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, crate::ports::StoreError> {
            Ok(self.authorized_peers.contains(peer_id))
        }
        fn create_and_enqueue_outbound(
            &mut self,
            item: &crate::ports::NewOutboundItem,
            _actor: &str,
        ) -> Result<String, crate::ports::StoreError> {
            self.next_id += 1;
            let id = format!("item-{}", self.next_id);
            self.outbound.insert(id.clone(), item.clone());
            Ok(id)
        }
        fn get_outbound_state(
            &self,
            _item_id: &str,
        ) -> Result<Option<ferry_proto::states::TransferState>, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn set_outbound_state(
            &mut self,
            _item_id: &str,
            _state: ferry_proto::states::TransferState,
        ) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn create_inbound(
            &mut self,
            _offer: &ferry_proto::envelope::Offer,
            _peer_id: &str,
            _actor: &str,
        ) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn get_inbound_state(
            &self,
            item_id: &str,
        ) -> Result<Option<ferry_proto::states::TransferState>, crate::ports::StoreError> {
            Ok(self.inbound.get(item_id).map(|i| i.state))
        }
        fn set_inbound_state(
            &mut self,
            item_id: &str,
            state: ferry_proto::states::TransferState,
        ) -> Result<(), crate::ports::StoreError> {
            self.inbound
                .get_mut(item_id)
                .ok_or_else(|| crate::ports::StoreError("no such inbound item".into()))?
                .state = state;
            Ok(())
        }
        fn inbound_bytes_received(&self, _item_id: &str) -> Result<u64, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn append_inbound_chunk(
            &mut self,
            _item_id: &str,
            _seq: u64,
            _bytes: &[u8],
        ) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn inbound_full_hash_so_far(&self, _item_id: &str) -> Result<String, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn finalize_inbound_delivered(&mut self, _item_id: &str) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn mark_inbound_opened(&mut self, item_id: &str) -> Result<(), crate::ports::StoreError> {
            let item = self
                .inbound
                .get_mut(item_id)
                .ok_or_else(|| crate::ports::StoreError("no such inbound item".into()))?;
            if item.is_burn_after_read {
                item.bytes.clear();
            }
            item.state = ferry_proto::states::TransferState::Opened;
            Ok(())
        }
        fn get_outbound_item(
            &self,
            _item_id: &str,
        ) -> Result<Option<crate::ports::NewOutboundItem>, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn list_outbox_for_peer(
            &self,
            _peer_id: &str,
        ) -> Result<Vec<crate::ports::OutboxItem>, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn record_outbox_attempt(&mut self, _outbox_id: &str) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn remove_outbox_entry(&mut self, _outbox_id: &str) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn set_inbound_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn get_inbound_expiry(
            &self,
            _item_id: &str,
        ) -> Result<Option<crate::expiry::ExpiryDeadline>, crate::ports::StoreError> {
            Ok(None)
        }
        fn set_outbox_expiry(
            &mut self,
            _item_id: &str,
            _deadline: &crate::expiry::ExpiryDeadline,
        ) -> Result<(), crate::ports::StoreError> {
            Ok(())
        }
        fn get_outbox_expiry(
            &self,
            _item_id: &str,
        ) -> Result<Option<crate::expiry::ExpiryDeadline>, crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn record_outbound_dropped(&mut self, _item_id: &str, _actor: &str) -> Result<(), crate::ports::StoreError> {
            unimplemented!("not needed for ipc tests")
        }
        fn read_inbound_plaintext(&self, item_id: &str) -> Result<Vec<u8>, crate::ports::StoreError> {
            Ok(self
                .inbound
                .get(item_id)
                .ok_or_else(|| crate::ports::StoreError("no such inbound item".into()))?
                .bytes
                .clone())
        }
        fn is_healthy(&self) -> Result<bool, crate::ports::StoreError> {
            Ok(self.healthy)
        }
        fn list_roster_peers(&self) -> Result<Vec<crate::ports::RosterPeer>, crate::ports::StoreError> {
            Ok(self.roster.clone())
        }
        fn export_signed_roster(&self) -> Result<String, crate::ports::StoreError> {
            Ok("fake-signed-roster-json".into())
        }
        fn import_signed_roster(
            &mut self,
            signed_roster_json: &str,
        ) -> Result<crate::ports::RosterImportSummary, crate::ports::StoreError> {
            if signed_roster_json == "malformed" {
                return Err(crate::ports::StoreError("roster signature is invalid".into()));
            }
            Ok(crate::ports::RosterImportSummary {
                signer_verifying_key_hex: "fp-1".into(),
                peer_count: 1,
                added: 1,
                skipped_existing: 0,
            })
        }
        fn add_paired_peer(&mut self, peer: &crate::ports::NewRosterPeer) -> Result<(), crate::ports::StoreError> {
            if self.paired_peers.iter().any(|p| p.peer_id == peer.peer_id) {
                return Err(crate::ports::StoreError(format!("already paired with {}", peer.peer_id)));
            }
            self.paired_peers.push(peer.clone());
            Ok(())
        }

        fn stage_outbound_content(&mut self, content: &[u8]) -> Result<String, crate::ports::StoreError> {
            self.staged.push(content.to_vec());
            Ok(format!("staged://{}", self.staged.len()))
        }
        fn list_sent_items(&self) -> Result<Vec<crate::ports::SentItem>, crate::ports::StoreError> {
            Ok(self.sent_items.clone())
        }
        fn list_audit(
            &self,
            limit: u32,
            before_millis: Option<i64>,
        ) -> Result<Vec<crate::ports::AuditRecord>, crate::ports::StoreError> {
            Ok(self
                .audit
                .iter()
                .filter(|e| before_millis.map(|b| e.occurred_at_millis < b).unwrap_or(true))
                .take(limit.max(1) as usize)
                .cloned()
                .collect())
        }
        fn remove_peer(&mut self, peer_id: &str) -> Result<(), crate::ports::StoreError> {
            if !self.roster.iter().any(|p| p.peer_id == peer_id) {
                return Err(crate::ports::StoreError(format!("no such peer {peer_id}")));
            }
            self.roster.retain(|p| p.peer_id != peer_id);
            self.removed_peers.push(peer_id.to_string());
            Ok(())
        }
        fn abort_outbound(&mut self, item_id: &str, _actor: &str) -> Result<(), crate::ports::StoreError> {
            if !self.outbound.contains_key(item_id) {
                return Err(crate::ports::StoreError(format!("no outbound item {item_id}")));
            }
            self.aborted.push(item_id.to_string());
            Ok(())
        }
        fn retry_outbound(&mut self, item_id: &str, _actor: &str) -> Result<(), crate::ports::StoreError> {
            if !self.outbound.contains_key(item_id) {
                return Err(crate::ports::StoreError(format!("no outbound item {item_id}")));
            }
            self.retried.push(item_id.to_string());
            Ok(())
        }

        fn list_inbox_items(&self) -> Result<Vec<crate::ports::InboxItemSummary>, crate::ports::StoreError> {
            Ok(self
                .inbound
                .iter()
                .map(|(item_id, item)| crate::ports::InboxItemSummary {
                    item_id: item_id.clone(),
                    peer_id: item.peer_id.clone(),
                    origin_display_name: item.peer_id.clone(),
                    kind: item.kind,
                    name: item.name.clone(),
                    state: item.state,
                    size_bytes: item.size_bytes,
                    is_burn_after_read: item.is_burn_after_read,
                    hash_hex: None,
                    received_at_millis: None,
                    expires_at_millis: None,
                })
                .collect())
        }
    }

    fn healthy_store() -> FakeStore {
        FakeStore {
            healthy: true,
            ..Default::default()
        }
    }

    struct FakeSource {
        contents: std::collections::HashMap<String, Vec<u8>>,
    }

    impl crate::ports::OutboundSource for FakeSource {
        type Reader = std::io::Cursor<Vec<u8>>;

        fn open(&mut self, key: &str) -> std::io::Result<Self::Reader> {
            self.contents
                .get(key)
                .cloned()
                .map(std::io::Cursor::new)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such source"))
        }
    }

    fn empty_source() -> FakeSource {
        FakeSource { contents: std::collections::HashMap::new() }
    }

    fn handle_with_defaults(store: &mut FakeStore, request: IpcRequest) -> IpcResponse {
        let clock = ExpiryClock::new();
        let mut source = empty_source();
        let runtime = RuntimeStatus::default();
        handle(store, &clock, &mut source, &runtime, RequestId("req-1".into()), IPC_PROTOCOL_VERSION, request)
    }

    #[test]
    fn status_reports_real_store_health() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::Status);
        assert_eq!(response.request_id, RequestId("req-1".into()));
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Status(status) } => {
                assert!(status.store_ok);
                assert_eq!(status.protocol_version, PROTOCOL_VERSION);
            }
            other => panic!("expected Ok(Status), got {other:?}"),
        }
    }

    #[test]
    fn status_reports_the_real_runtime_state_of_transport_and_discovery() {
        let mut store = healthy_store();
        let clock = ExpiryClock::new();
        let mut source = empty_source();
        let runtime = RuntimeStatus { transport_ok: true, discovery_ok: false, ..Default::default() };
        let response = handle(
            &mut store,
            &clock,
            &mut source,
            &runtime,
            RequestId("req-1".into()),
            IPC_PROTOCOL_VERSION,
            IpcRequest::Status,
        );
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Status(status) } => {
                assert!(status.transport_ok);
                assert!(!status.discovery_ok);
            }
            other => panic!("expected Ok(Status), got {other:?}"),
        }
    }

    #[test]
    fn status_reports_an_unhealthy_store_honestly_rather_than_hardcoding_ok() {
        let mut store = FakeStore { healthy: false, ..Default::default() };
        let response = handle_with_defaults(&mut store, IpcRequest::Status);
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Status(status) } => assert!(!status.store_ok),
            other => panic!("expected Ok(Status), got {other:?}"),
        }
    }

    #[test]
    fn a_mismatched_ipc_protocol_version_is_rejected_before_touching_the_store() {
        let mut store = healthy_store();
        let clock = ExpiryClock::new();
        let mut source = empty_source();
        let runtime = RuntimeStatus::default();
        let response = handle(
            &mut store,
            &clock,
            &mut source,
            &runtime,
            RequestId("req-1".into()),
            IPC_PROTOCOL_VERSION + 1,
            IpcRequest::Status,
        );
        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::ProtocolVersionMismatch),
            other => panic!("expected Err(ProtocolVersionMismatch), got {other:?}"),
        }
    }

    #[test]
    fn quit_is_acknowledged_and_flagged_for_the_caller_to_act_on() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::Quit);
        assert!(matches!(
            response.outcome,
            IpcOutcome::Ok { value: IpcResult::Ack }
        ));
        assert!(is_quit(&IpcRequest::Quit));
        assert!(!is_quit(&IpcRequest::Status));
    }

    #[test]
    fn roster_list_maps_store_peers_into_the_wire_view() {
        let mut store = FakeStore {
            healthy: true,
            roster: vec![crate::ports::RosterPeer {
                peer_id: "peer-1".into(),
                display_name: "laptop".into(),
                paired_at_millis: 1000,
                reachable: false,
                last_seen_millis: None,
            }],
            ..Default::default()
        };
        let response = handle_with_defaults(&mut store, IpcRequest::RosterList);
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::RosterList(peers) } => {
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].peer_id, "peer-1");
                assert_eq!(peers[0].display_name, "laptop");
            }
            other => panic!("expected Ok(RosterList), got {other:?}"),
        }
    }

    #[test]
    fn roster_export_returns_the_signed_roster_json_from_the_store() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::RosterExport);
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::RosterExport { signed_roster_json } } => {
                assert_eq!(signed_roster_json, "fake-signed-roster-json");
            }
            other => panic!("expected Ok(RosterExport), got {other:?}"),
        }
    }

    #[test]
    fn roster_import_returns_a_summary_on_success() {
        let mut store = healthy_store();
        let response = handle_with_defaults(
            &mut store,
            IpcRequest::RosterImport { signed_roster_json: "valid".into() },
        );
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::RosterImport(summary) } => {
                assert_eq!(summary.added, 1);
                assert_eq!(summary.signer_verifying_key_hex, "fp-1");
            }
            other => panic!("expected Ok(RosterImport), got {other:?}"),
        }
    }

    #[test]
    fn roster_import_maps_a_verification_failure_to_a_distinct_error_code() {
        let mut store = healthy_store();
        let response = handle_with_defaults(
            &mut store,
            IpcRequest::RosterImport { signed_roster_json: "malformed".into() },
        );
        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::RosterInvalid),
            other => panic!("expected Err(RosterInvalid), got {other:?}"),
        }
    }

    fn send_request(peer_id: &str, source_path: &str) -> IpcRequest {
        IpcRequest::Send {
            peer_id: peer_id.into(),
            source_path: source_path.into(),
            name: "notes.txt".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
        }
    }

    #[test]
    fn send_enqueues_a_real_hashed_item_for_a_rostered_peer() {
        let mut store = FakeStore {
            healthy: true,
            authorized_peers: std::collections::HashSet::from(["peer-1".to_string()]),
            ..Default::default()
        };
        let clock = ExpiryClock::new();
        let mut source = FakeSource {
            contents: std::collections::HashMap::from([("/tmp/notes.txt".to_string(), b"hello ferry".to_vec())]),
        };
        let runtime = RuntimeStatus::default();

        let response = handle(
            &mut store,
            &clock,
            &mut source,
            &runtime,
            RequestId("req-1".into()),
            IPC_PROTOCOL_VERSION,
            send_request("peer-1", "/tmp/notes.txt"),
        );

        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Send { item_id } } => {
                let item = store.outbound.get(&item_id).expect("item must have been enqueued");
                assert_eq!(item.peer_id, "peer-1");
                assert_eq!(item.size_bytes, 11);
                assert_eq!(item.source_path, "/tmp/notes.txt");
            }
            other => panic!("expected Ok(Send), got {other:?}"),
        }
    }

    #[test]
    fn send_to_an_unrostered_peer_is_rejected_with_a_distinct_error_code() {
        let mut store = healthy_store();
        let clock = ExpiryClock::new();
        let mut source = FakeSource {
            contents: std::collections::HashMap::from([("/tmp/notes.txt".to_string(), b"hello ferry".to_vec())]),
        };
        let runtime = RuntimeStatus::default();

        let response = handle(
            &mut store,
            &clock,
            &mut source,
            &runtime,
            RequestId("req-1".into()),
            IPC_PROTOCOL_VERSION,
            send_request("stranger", "/tmp/notes.txt"),
        );

        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::PeerNotAuthorized),
            other => panic!("expected Err(PeerNotAuthorized), got {other:?}"),
        }
        assert!(store.outbound.is_empty(), "a rejected send must never reach storage");
    }

    #[test]
    fn send_with_a_missing_source_file_reports_an_error_instead_of_enqueuing() {
        let mut store = FakeStore {
            healthy: true,
            authorized_peers: std::collections::HashSet::from(["peer-1".to_string()]),
            ..Default::default()
        };
        let response = handle_with_defaults(&mut store, send_request("peer-1", "/tmp/missing.txt"));

        assert!(matches!(response.outcome, IpcOutcome::Err { .. }));
        assert!(store.outbound.is_empty());
    }

    #[test]
    fn pair_complete_adds_the_peer_and_acknowledges() {
        let mut store = healthy_store();
        let response = handle_with_defaults(
            &mut store,
            IpcRequest::PairComplete {
                peer_id: "peer-9".into(),
                display_name: "phone".into(),
                signing_key_hex: "aa".repeat(32),
                sealing_key: "age1stub".into(),
            },
        );
        assert!(matches!(response.outcome, IpcOutcome::Ok { value: IpcResult::Ack }));
        assert_eq!(store.paired_peers.len(), 1);
        assert_eq!(store.paired_peers[0].peer_id, "peer-9");
    }

    #[test]
    fn pair_complete_rejects_a_peer_already_paired() {
        let mut store = healthy_store();
        let request = || IpcRequest::PairComplete {
            peer_id: "peer-9".into(),
            display_name: "phone".into(),
            signing_key_hex: "aa".repeat(32),
            sealing_key: "age1stub".into(),
        };
        handle_with_defaults(&mut store, request());
        let second = handle_with_defaults(&mut store, request());

        assert!(matches!(second.outcome, IpcOutcome::Err { .. }));
        assert_eq!(store.paired_peers.len(), 1, "a duplicate pairing must not add a second entry");
    }

    fn store_with_inbound_item(item_id: &str, is_burn_after_read: bool, bytes: &[u8]) -> FakeStore {
        let mut store = healthy_store();
        store.inbound.insert(
            item_id.to_string(),
            FakeInboundItem {
                peer_id: "peer-a".into(),
                kind: ItemKind::File,
                name: "notes.txt".into(),
                state: ferry_proto::states::TransferState::Delivered,
                size_bytes: bytes.len() as u64,
                is_burn_after_read,
                bytes: bytes.to_vec(),
            },
        );
        store
    }

    #[test]
    fn inbox_list_reports_a_real_delivered_item() {
        let mut store = store_with_inbound_item("item-1", false, b"hello ferry");
        let response = handle_with_defaults(&mut store, IpcRequest::InboxList);
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::InboxList(items) } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].item_id, "item-1");
                assert_eq!(items[0].name, "notes.txt");
                assert_eq!(items[0].state, ferry_proto::states::TransferState::Delivered);
            }
            other => panic!("expected Ok(InboxList), got {other:?}"),
        }
    }

    #[test]
    fn open_returns_the_real_plaintext_without_mutating_state_yet() {
        let mut store = store_with_inbound_item("item-1", false, b"hello ferry");
        let response = handle_with_defaults(&mut store, IpcRequest::Open { item_id: "item-1".into() });

        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Open { content_base64 } } => {
                let decoded = base64::engine::general_purpose::STANDARD.decode(&content_base64).unwrap();
                assert_eq!(decoded, b"hello ferry");
            }
            other => panic!("expected Ok(Open), got {other:?}"),
        }
        assert_eq!(
            store.inbound["item-1"].state,
            ferry_proto::states::TransferState::Delivered,
            "Open must peek without committing — the client confirms separately, only after it has safely delivered the content"
        );
        assert!(!store.inbound["item-1"].bytes.is_empty(), "Open must never burn the payload on its own");
    }

    #[test]
    fn open_can_be_repeated_before_confirm_and_confirm_marks_opened_and_burns() {
        let mut store = store_with_inbound_item("item-1", true, b"secret plan");

        let first = handle_with_defaults(&mut store, IpcRequest::Open { item_id: "item-1".into() });
        assert!(matches!(first.outcome, IpcOutcome::Ok { .. }));
        let second = handle_with_defaults(&mut store, IpcRequest::Open { item_id: "item-1".into() });
        assert!(
            matches!(second.outcome, IpcOutcome::Ok { .. }),
            "a peek must be repeatable — e.g. after a local write failure the client can just try again"
        );
        assert!(!store.inbound["item-1"].bytes.is_empty(), "peeking alone must never burn the payload");

        let confirmed = handle_with_defaults(&mut store, IpcRequest::ConfirmOpened { item_id: "item-1".into() });
        assert!(matches!(confirmed.outcome, IpcOutcome::Ok { .. }));
        assert!(store.inbound["item-1"].bytes.is_empty(), "confirming must burn a burn-after-read item");
        assert_eq!(store.inbound["item-1"].state, ferry_proto::states::TransferState::Opened);

        let confirm_again = handle_with_defaults(&mut store, IpcRequest::ConfirmOpened { item_id: "item-1".into() });
        assert!(matches!(confirm_again.outcome, IpcOutcome::Err { .. }), "confirming a burned item twice must fail");
    }

    #[test]
    fn open_reports_a_distinct_error_for_an_unknown_item() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::Open { item_id: "no-such-item".into() });
        assert!(matches!(response.outcome, IpcOutcome::Err { .. }));
    }

    #[test]
    fn confirm_opened_reports_a_distinct_error_for_an_unknown_item() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::ConfirmOpened { item_id: "no-such-item".into() });
        assert!(matches!(response.outcome, IpcOutcome::Err { .. }));
    }

    fn sample_outbound() -> crate::ports::NewOutboundItem {
        crate::ports::NewOutboundItem {
            peer_id: "peer-1".into(),
            kind: ItemKind::File,
            name: "f".into(),
            size_bytes: 3,
            hash: "abc".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/f".into(),
        }
    }

    fn roster_peer(peer_id: &str, name: &str) -> crate::ports::RosterPeer {
        crate::ports::RosterPeer {
            peer_id: peer_id.into(),
            display_name: name.into(),
            paired_at_millis: 1_000,
            reachable: false,
            last_seen_millis: None,
        }
    }

    fn offered_item(store: &mut FakeStore, item_id: &str, peer_id: &str) {
        store.inbound.insert(
            item_id.into(),
            FakeInboundItem {
                peer_id: peer_id.into(),
                kind: ItemKind::File,
                name: "f".into(),
                state: ferry_proto::states::TransferState::Offered,
                size_bytes: 3,
                is_burn_after_read: false,
                bytes: b"abc".to_vec(),
            },
        );
    }

    #[test]
    fn identity_returns_the_runtime_identity_and_the_wire_protocol_version() {
        let mut store = healthy_store();
        let clock = ExpiryClock::new();
        let mut source = empty_source();
        let runtime = RuntimeStatus {
            transport_ok: true,
            discovery_ok: false,
            identity: RuntimeIdentity {
                fingerprint: "a1b2c3d4".into(),
                signing_key_hex: "deadbeef".into(),
                sealing_key: "age1xyz".into(),
                display_name: "firas-laptop".into(),
                listen_port: 47821,
                data_dir: "/data".into(),
                auto_accept_from_roster: true,
            },
        };
        let response = handle(&mut store, &clock, &mut source, &runtime, RequestId("r".into()), IPC_PROTOCOL_VERSION, IpcRequest::Identity);
        match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Identity(v) } => {
                assert_eq!(v.fingerprint, "a1b2c3d4");
                assert_eq!(v.display_name, "firas-laptop");
                assert_eq!(v.listen_port, 47821);
                assert_eq!(v.protocol_version, PROTOCOL_VERSION);
                assert!(v.auto_accept_from_roster);
            }
            other => panic!("expected Ok(Identity), got {other:?}"),
        }
    }

    #[test]
    fn sent_list_maps_every_stored_outbound_item() {
        let mut store = healthy_store();
        store.sent_items.push(crate::ports::SentItem {
            item_id: "item-1".into(),
            peer_id: "peer-1".into(),
            peer_display_name: "bob".into(),
            name: "report.pdf".into(),
            hash_hex: "abcd".into(),
            kind: ItemKind::File,
            state: ferry_proto::states::TransferState::Delivered,
            size_bytes: 42,
            queued_at_millis: Some(10),
            last_attempt_at_millis: Some(20),
            last_error: None,
        });
        match handle_with_defaults(&mut store, IpcRequest::SentList).outcome {
            IpcOutcome::Ok { value: IpcResult::SentList(items) } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].peer_display_name, "bob");
                assert_eq!(items[0].state, ferry_proto::states::TransferState::Delivered);
            }
            other => panic!("expected Ok(SentList), got {other:?}"),
        }
    }

    #[test]
    fn audit_list_honors_the_limit_and_before_cursor() {
        let mut store = healthy_store();
        for millis in [100_i64, 200, 300, 400] {
            store.audit.push(crate::ports::AuditRecord {
                id: format!("e{millis}"),
                actor: "local".into(),
                kind: "item.sent".into(),
                item_id: None,
                occurred_at_millis: millis,
                outcome: "ok".into(),
            });
        }
        match handle_with_defaults(&mut store, IpcRequest::AuditList { limit: 2, before_millis: Some(350) }).outcome {
            IpcOutcome::Ok { value: IpcResult::AuditList(events) } => {
                assert_eq!(events.len(), 2);
                assert!(events.iter().all(|e| e.occurred_at_millis < 350));
            }
            other => panic!("expected Ok(AuditList), got {other:?}"),
        }
    }

    #[test]
    fn inbox_accept_moves_an_offered_item_to_accepted() {
        let mut store = healthy_store();
        offered_item(&mut store, "item-1", "peer-1");
        let response = handle_with_defaults(&mut store, IpcRequest::InboxAccept { item_id: "item-1".into() });
        assert!(matches!(response.outcome, IpcOutcome::Ok { value: IpcResult::Ack }));
        assert_eq!(store.inbound["item-1"].state, ferry_proto::states::TransferState::Accepted);
    }

    #[test]
    fn inbox_reject_moves_an_offered_item_to_failed() {
        let mut store = healthy_store();
        offered_item(&mut store, "item-1", "peer-1");
        let response = handle_with_defaults(&mut store, IpcRequest::InboxReject { item_id: "item-1".into() });
        assert!(matches!(response.outcome, IpcOutcome::Ok { value: IpcResult::Ack }));
        assert_eq!(store.inbound["item-1"].state, ferry_proto::states::TransferState::Failed);
    }

    #[test]
    fn inbox_accept_on_an_unknown_item_is_a_distinct_error() {
        let mut store = healthy_store();
        let response = handle_with_defaults(&mut store, IpcRequest::InboxAccept { item_id: "nope".into() });
        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::ItemNotFound),
            other => panic!("expected Err(ItemNotFound), got {other:?}"),
        }
    }

    #[test]
    fn peer_remove_deletes_the_roster_entry() {
        let mut store = healthy_store();
        store.roster.push(roster_peer("peer-1", "bob"));
        let response = handle_with_defaults(&mut store, IpcRequest::PeerRemove { peer_id: "peer-1".into() });
        assert!(matches!(response.outcome, IpcOutcome::Ok { value: IpcResult::Ack }));
        assert_eq!(store.removed_peers, vec!["peer-1".to_string()]);
        assert!(store.roster.is_empty());
    }

    #[test]
    fn sent_abort_and_retry_reach_the_store_only_for_known_items() {
        let mut store = healthy_store();
        store.outbound.insert("item-1".into(), sample_outbound());
        assert!(matches!(
            handle_with_defaults(&mut store, IpcRequest::SentAbort { item_id: "item-1".into() }).outcome,
            IpcOutcome::Ok { .. }
        ));
        assert!(matches!(
            handle_with_defaults(&mut store, IpcRequest::SentRetry { item_id: "item-1".into() }).outcome,
            IpcOutcome::Ok { .. }
        ));
        assert_eq!(store.aborted, vec!["item-1".to_string()]);
        assert_eq!(store.retried, vec!["item-1".to_string()]);

        assert!(matches!(
            handle_with_defaults(&mut store, IpcRequest::SentAbort { item_id: "ghost".into() }).outcome,
            IpcOutcome::Err { .. }
        ));
    }

    #[test]
    fn send_inline_hashes_the_decoded_content_and_enqueues_it_from_the_staged_path() {
        use base64::Engine;

        let mut store = healthy_store();
        store.authorized_peers.insert("peer-1".into());

        let payload = b"DB_URL=postgres://x\nDB_PASS=hunter2\n";
        let response = handle_with_defaults(
            &mut store,
            IpcRequest::SendInline {
                peer_id: "peer-1".into(),
                name: ".env".into(),
                kind: ItemKind::Secret,
                content_base64: base64::engine::general_purpose::STANDARD.encode(payload),
                ttl_secs: 600,
                is_burn_after_read: true,
                notify_on_open: false,
            },
        );

        let item_id = match response.outcome {
            IpcOutcome::Ok { value: IpcResult::Send { item_id } } => item_id,
            other => panic!("expected Ok(Send), got {other:?}"),
        };
        let stored = &store.outbound[&item_id];
        assert_eq!(stored.name, ".env");
        assert_eq!(stored.kind, ItemKind::Secret);
        assert!(stored.is_burn_after_read);
        assert_eq!(stored.size_bytes, payload.len() as u64);
        assert_eq!(
            stored.hash,
            {
                use sha2::Digest;
                hex::encode(sha2::Sha256::digest(payload))
            },
            "the hash must be computed from the caller's bytes, not re-read from disk"
        );
        assert_eq!(
            stored.source_path, "staged://1",
            "the item must point at whatever path the store staged it to"
        );
        assert_eq!(store.staged, vec![payload.to_vec()]);
    }

    #[test]
    fn send_inline_to_an_unrostered_peer_is_rejected_and_stages_nothing_durable() {
        use base64::Engine;

        let mut store = healthy_store();
        let response = handle_with_defaults(
            &mut store,
            IpcRequest::SendInline {
                peer_id: "stranger".into(),
                name: "m".into(),
                kind: ItemKind::Message,
                content_base64: base64::engine::general_purpose::STANDARD.encode(b"hi"),
                ttl_secs: 60,
                is_burn_after_read: false,
                notify_on_open: false,
            },
        );
        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::PeerNotAuthorized),
            other => panic!("expected Err(PeerNotAuthorized), got {other:?}"),
        }
        assert!(store.outbound.is_empty(), "a denied send must not create an outbound item");
    }

    #[test]
    fn send_inline_rejects_content_over_the_per_kind_cap_before_staging_it() {
        use base64::Engine;

        let mut store = healthy_store();
        store.authorized_peers.insert("peer-1".into());
        let oversized = vec![b'x'; (crate::policy::MAX_MESSAGE_BYTES + 1) as usize];

        let response = handle_with_defaults(
            &mut store,
            IpcRequest::SendInline {
                peer_id: "peer-1".into(),
                name: "m".into(),
                kind: ItemKind::Message,
                content_base64: base64::engine::general_purpose::STANDARD.encode(&oversized),
                ttl_secs: 60,
                is_burn_after_read: false,
                notify_on_open: false,
            },
        );
        match response.outcome {
            IpcOutcome::Err { error } => assert_eq!(error.code, ErrorCode::ItemRejectedByPolicy),
            other => panic!("expected Err(ItemRejectedByPolicy), got {other:?}"),
        }
        assert!(store.staged.is_empty(), "over-cap content must be rejected before it is ever written");
    }
}
