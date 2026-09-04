use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::FerryError;
use crate::states::{ItemKind, TransferState};

pub const IPC_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct RequestId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    Quit,
    RosterList,
    RosterExport,
    RosterImport { signed_roster_json: String },
    Send {
        peer_id: String,
        source_path: String,
        name: String,
        ttl_secs: u32,
        is_burn_after_read: bool,
        notify_on_open: bool,
    },
    PairComplete {
        peer_id: String,
        display_name: String,
        signing_key_hex: String,
        sealing_key: String,
    },
    PairBegin {
        mode: PairMode,
    },
    PairStatus {
        pairing_id: String,
    },
    PairConfirm {
        pairing_id: String,
        accept: bool,
    },
    PairCancel {
        pairing_id: String,
    },
    SendInline {
        peer_id: String,
        name: String,
        kind: ItemKind,
        content_base64: String,
        ttl_secs: u32,
        is_burn_after_read: bool,
        notify_on_open: bool,
    },
    InboxList,
    InboxAccept { item_id: String },
    InboxReject { item_id: String },
    Open { item_id: String },
    ConfirmOpened { item_id: String },
    ExportSealed { item_id: String },
    ImportSealed { blob_base64: String },
    Identity,
    SentList,
    SentAbort { item_id: String },
    SentRetry { item_id: String },
    AuditList { limit: u32, before_millis: Option<i64> },
    PeerRemove { peer_id: String },
    Subscribe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "role", content = "params", rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum PairMode {
    Listen {
        display_name: String,
    },
    Connect {
        addr: String,
        code: String,
        display_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum PairPhase {
    AwaitingPeer,
    AwaitingConfirmation,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct PairBeginView {
    pub pairing_id: String,
    pub listen_addr: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct PairStatusView {
    pub phase: PairPhase,
    pub phrase: Option<String>,
    pub peer_fingerprint: Option<String>,
    pub peer_display_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum IpcResource {
    Transfer,
    Message,
    Peer,
    Roster,
    Audit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "event", content = "params", rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum IpcEvent {
    Changed {
        resource: IpcResource,
        id: Option<String>,
    },
    Progress {
        item_id: String,
        bytes: u64,
        total: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct IpcEnvelope {
    pub ipc_protocol_version: u16,
    pub request_id: RequestId,
    pub request: IpcRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct DaemonStatus {
    pub protocol_version: u16,
    pub discovery_ok: bool,
    pub transport_ok: bool,
    pub store_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct RosterPeerView {
    pub peer_id: String,
    pub display_name: String,
    pub paired_at_millis: i64,
    pub fingerprint_short: String,
    pub reachable: bool,
    pub last_seen_millis: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct RosterImportSummaryView {
    pub signer_verifying_key_hex: String,
    pub peer_count: u32,
    pub added: u32,
    pub skipped_existing: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct InboxItemView {
    pub item_id: String,
    pub peer_id: String,
    pub origin_display_name: String,
    pub kind: ItemKind,
    pub name: String,
    pub state: TransferState,
    pub size_bytes: u64,
    pub is_burn_after_read: bool,
    pub hash_hex: Option<String>,
    pub received_at_millis: Option<i64>,
    pub expires_at_millis: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct SealedImportView {
    pub item_id: String,
    pub origin_peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct IdentityView {
    pub fingerprint: String,
    pub signing_key_hex: String,
    pub sealing_key: String,
    pub display_name: String,
    pub listen_port: u16,
    pub data_dir: String,
    pub protocol_version: u16,
    pub auto_accept_from_roster: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct SentItemView {
    pub item_id: String,
    pub peer_id: String,
    pub peer_display_name: String,
    pub name: String,
    pub hash_hex: String,
    pub kind: ItemKind,
    pub state: TransferState,
    pub size_bytes: u64,
    pub queued_at_millis: Option<i64>,
    pub last_attempt_at_millis: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct AuditEventView {
    pub id: String,
    pub actor: String,
    pub kind: String,
    pub item_id: Option<String>,
    pub occurred_at_millis: i64,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum IpcResult {
    Status(DaemonStatus),
    Ack,
    RosterList(Vec<RosterPeerView>),
    RosterExport { signed_roster_json: String },
    RosterImport(RosterImportSummaryView),
    Send { item_id: String },
    InboxList(Vec<InboxItemView>),
    Open { content_base64: String },
    ExportSealed { blob_base64: String },
    ImportSealed(SealedImportView),
    Identity(IdentityView),
    SentList(Vec<SentItemView>),
    AuditList(Vec<AuditEventView>),
    PairBegin(PairBeginView),
    PairStatus(PairStatusView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum IpcOutcome {
    Ok { value: IpcResult },
    Err { error: FerryError },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct IpcResponse {
    pub request_id: RequestId,
    pub outcome: IpcOutcome,
}
