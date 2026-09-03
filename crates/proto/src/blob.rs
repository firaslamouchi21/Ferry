use serde::{Deserialize, Serialize};

use crate::states::ItemKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobManifest {
    pub item_id: String,
    pub origin_peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: u64,
    pub hash: String,
    pub ttl_secs: u32,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
    pub payload: Vec<u8>,
}
