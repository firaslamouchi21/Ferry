use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Offered,
    Accepted,
    Transferring,
    Delivered,
    Opened,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum PeerState {
    PendingVerification,
    Paired,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum MessageState {
    Queued,
    Sent,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum ItemKind {
    File,
    Secret,
    Message,
}
