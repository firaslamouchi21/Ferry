use ferry_proto::envelope::Offer;
use ferry_proto::states::{ItemKind, TransferState};

use crate::expiry::ExpiryDeadline;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOutboundItem {
    pub peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: u64,
    pub hash: String,
    pub ttl_secs: u32,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub outbox_id: String,
    pub item_id: String,
    pub peer_id: String,
    pub attempts: u32,
    pub last_attempted_at_millis: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterPeer {
    pub peer_id: String,
    pub display_name: String,
    pub paired_at_millis: i64,
    pub reachable: bool,
    pub last_seen_millis: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterImportSummary {
    pub signer_verifying_key_hex: String,
    pub peer_count: usize,
    pub added: usize,
    pub skipped_existing: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRosterPeer {
    pub peer_id: String,
    pub display_name: String,
    pub signing_key_hex: String,
    pub sealing_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemSummary {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentItem {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub id: String,
    pub actor: String,
    pub kind: String,
    pub item_id: Option<String>,
    pub occurred_at_millis: i64,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("storage port error: {0}")]
pub struct StoreError(pub String);

pub trait Store {
    fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, StoreError>;

    fn create_and_enqueue_outbound(
        &mut self,
        item: &NewOutboundItem,
        actor: &str,
    ) -> Result<String, StoreError>;

    fn get_outbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError>;

    fn set_outbound_state(
        &mut self,
        item_id: &str,
        state: TransferState,
    ) -> Result<(), StoreError>;

    fn create_inbound(&mut self, offer: &Offer, peer_id: &str, actor: &str) -> Result<(), StoreError>;

    fn get_inbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError>;

    fn set_inbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError>;

    fn inbound_bytes_received(&self, item_id: &str) -> Result<u64, StoreError>;

    fn append_inbound_chunk(&mut self, item_id: &str, seq: u64, bytes: &[u8]) -> Result<(), StoreError>;

    fn inbound_full_hash_so_far(&self, item_id: &str) -> Result<String, StoreError>;

    fn finalize_inbound_delivered(&mut self, item_id: &str) -> Result<(), StoreError>;

    fn mark_inbound_opened(&mut self, item_id: &str) -> Result<(), StoreError>;

    fn get_outbound_item(&self, item_id: &str) -> Result<Option<NewOutboundItem>, StoreError>;

    fn list_outbox_for_peer(&self, peer_id: &str) -> Result<Vec<OutboxItem>, StoreError>;

    fn record_outbox_attempt(&mut self, outbox_id: &str) -> Result<(), StoreError>;

    fn remove_outbox_entry(&mut self, outbox_id: &str) -> Result<(), StoreError>;

    fn set_inbound_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError>;

    fn get_inbound_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError>;

    fn set_outbox_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError>;

    fn get_outbox_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError>;

    fn record_outbound_dropped(&mut self, item_id: &str, actor: &str, cause: &str) -> Result<(), StoreError>;

    fn set_outbound_last_error(&mut self, _item_id: &str, _reason: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn read_inbound_plaintext(&self, item_id: &str) -> Result<Vec<u8>, StoreError>;

    fn read_outbound_content(&self, _item_id: &str) -> Result<Vec<u8>, StoreError> {
        Err(StoreError("reading outbound content is not supported by this store".into()))
    }

    fn is_healthy(&self) -> Result<bool, StoreError>;

    fn list_roster_peers(&self) -> Result<Vec<RosterPeer>, StoreError>;

    fn export_signed_roster(&self) -> Result<String, StoreError>;

    fn import_signed_roster(&mut self, signed_roster_json: &str) -> Result<RosterImportSummary, StoreError>;

    fn add_paired_peer(&mut self, peer: &NewRosterPeer) -> Result<(), StoreError>;

    fn list_inbox_items(&self) -> Result<Vec<InboxItemSummary>, StoreError>;

    fn roster_signing_keys_hex(&self) -> Result<Vec<String>, StoreError> {
        Ok(Vec::new())
    }

    fn record_provider_event(&mut self, _kind: &str, _outcome: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn enqueue_remote_job(&mut self, _kind: &str, _params_json: &str) -> Result<String, StoreError> {
        Err(StoreError("remote jobs are not supported by this store".into()))
    }

    fn get_remote_job(&self, _job_id: &str) -> Result<Option<RemoteJobRow>, StoreError> {
        Ok(None)
    }

    fn build_sealed_blob(&self, _item_id: &str) -> Result<Vec<u8>, StoreError> {
        Err(StoreError("sealed blob export is not supported by this store".into()))
    }

    fn open_sealed_blob(
        &self,
        _blob: &[u8],
    ) -> Result<ferry_proto::blob::BlobManifest, StoreError> {
        Err(StoreError("sealed blob import is not supported by this store".into()))
    }

    fn stage_outbound_content(&mut self, _content: &[u8]) -> Result<String, StoreError> {
        Err(StoreError("inline content staging is not supported by this store".into()))
    }

    fn discard_staged_source(&mut self, _item_id: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn list_open_receipts_for_peer(&self, _peer_id: &str) -> Result<Vec<String>, StoreError> {
        Ok(Vec::new())
    }

    fn remove_open_receipt(&mut self, _item_id: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn list_sent_items(&self) -> Result<Vec<SentItem>, StoreError> {
        Ok(Vec::new())
    }

    fn list_audit(&self, _limit: u32, _before_millis: Option<i64>) -> Result<Vec<AuditRecord>, StoreError> {
        Ok(Vec::new())
    }

    fn remove_peer(&mut self, _peer_id: &str) -> Result<(), StoreError> {
        Err(StoreError("peer removal is not supported by this store".into()))
    }

    fn abort_outbound(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
        Err(StoreError("outbound abort is not supported by this store".into()))
    }

    fn retry_outbound(&mut self, _item_id: &str, _actor: &str) -> Result<(), StoreError> {
        Err(StoreError("outbound retry is not supported by this store".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("channel port error: {0}")]
pub struct ChannelError(pub String);

pub trait Channel {
    fn send(&mut self, bytes: &[u8]) -> Result<(), ChannelError>;
    fn recv(&mut self) -> Result<Vec<u8>, ChannelError>;
    fn remote_peer_id(&self) -> &str;
}

pub trait OutboundSource {
    type Reader: std::io::Read + std::io::Seek;
    fn open(&mut self, item_id: &str) -> std::io::Result<Self::Reader>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("remote provider error: {0}")]
pub struct RemoteError(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLocator {
    pub provider: String,
    pub host: Option<String>,
    pub path: String,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedRef {
    pub url: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteJobRow {
    pub job_id: String,
    pub kind: String,
    pub phase: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

pub trait RemoteFetch {
    fn fetch(&self, locator: &RemoteLocator) -> Result<Vec<u8>, RemoteError>;
}

pub trait SnippetPublisher {
    fn publish_private(&self, name: &str, bytes: &[u8]) -> Result<PublishedRef, RemoteError>;
}
