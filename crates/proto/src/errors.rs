use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/", rename_all = "snake_case")]
pub enum ErrorCode {
    ProtocolVersionMismatch,
    PeerNotAuthorized,
    ItemRejectedByPolicy,
    IllegalStateTransition,
    ItemNotFound,
    ItemExpired,
    HashMismatch,
    FrameTooLarge,
    ConfigInvalid,
    RosterInvalid,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct FerryError {
    pub code: ErrorCode,
    pub message: String,
}
