use serde::{Deserialize, Serialize};

use crate::ids::ItemId;
use crate::states::ItemKind;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: u64,
    pub hash: String,
    pub ttl_secs: u32,
    pub burn_after_read: bool,
    pub notify_on_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accept {
    pub item_id: ItemId,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub item_id: ItemId,
    pub seq: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    pub item_id: ItemId,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivered {
    pub item_id: ItemId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub item_id: ItemId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireMessage {
    Offer(Offer),
    Accept(Accept),
    Chunk(Chunk),
    Done(Done),
    Delivered(Delivered),
    Opened(Opened),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: u16,
    pub message: WireMessage,
}
