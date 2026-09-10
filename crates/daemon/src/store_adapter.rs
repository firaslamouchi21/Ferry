use std::path::PathBuf;

use ferry_core::expiry::ExpiryDeadline;
use ferry_core::ports::{NewOutboundItem, Store, StoreError};
use ferry_crypto::identity::Identity;
use ferry_crypto::secret::{self, DataEncryptionKey};
use ferry_proto::envelope::Offer;
use ferry_proto::states::{ItemKind, TransferState};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub struct SqliteStore {
    conn: Connection,
    payload_dir: PathBuf,
    local_identity: Identity,
    presence: Option<std::sync::Arc<crate::presence::Presence>>,
}

impl SqliteStore {
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn new(conn: Connection, payload_dir: PathBuf, local_identity: Identity) -> Self {
        Self { conn, payload_dir, local_identity, presence: None }
    }

    pub fn with_presence(mut self, presence: std::sync::Arc<crate::presence::Presence>) -> Self {
        self.presence = Some(presence);
        self
    }

    pub fn identity(&self) -> &Identity {
        &self.local_identity
    }

    pub fn static_keypair(&self) -> ferry_net::transport::StaticKeypair {
        ferry_net::transport::StaticKeypair {
            private: self.local_identity.sealing_key_x25519_bytes(),
            public: self
                .local_identity
                .public()
                .sealing_key_x25519_bytes()
                .expect("own identity's sealing key must decode"),
        }
    }

    pub fn is_authorized_by_roster(&self, remote_static: &[u8; 32]) -> bool {
        crate::roster_authorizer::is_authorized_by_roster(&self.conn, remote_static)
    }

    pub fn peer_id_for_static(&self, remote_static: &[u8; 32]) -> Option<String> {
        crate::roster_authorizer::peer_id_for_static(&self.conn, remote_static)
    }

    pub fn sealing_key_for_peer_id(&self, peer_id: &str) -> Option<[u8; 32]> {
        crate::roster_authorizer::sealing_key_for_peer_id(&self.conn, peer_id)
    }

    fn display_name_for(&self, peer_id: &str) -> Result<String, StoreError> {
        Ok(ferry_store::roster::get_peer(&self.conn, peer_id)
            .map_err(|e| StoreError(e.to_string()))?
            .map(|p| p.display_name)
            .unwrap_or_else(|| peer_id.to_string()))
    }

    fn unwrap_dek_for(&self, item_id: &str) -> Result<DataEncryptionKey, StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;
        let wrapped_hex = item
            .wrapped_dek
            .ok_or_else(|| StoreError("item has no wrapped key — it may already be burned".into()))?;
        let wrapped = hex::decode(&wrapped_hex).map_err(|e| StoreError(e.to_string()))?;
        secret::unwrap_dek(self.local_identity.sealing_identity(), &wrapped).map_err(|e| StoreError(e.to_string()))
    }
}

impl Store for SqliteStore {
    fn is_peer_authorized(&self, peer_id: &str) -> Result<bool, StoreError> {
        ferry_store::roster::get_peer(&self.conn, peer_id)
            .map(|found| found.is_some())
            .map_err(|e| StoreError(e.to_string()))
    }

    fn create_and_enqueue_outbound(
        &mut self,
        item: &NewOutboundItem,
        actor: &str,
    ) -> Result<String, StoreError> {
        let store_item = ferry_store::outbound::NewOutboundItem {
            peer_id: item.peer_id.clone(),
            kind: item.kind,
            name: item.name.clone(),
            size_bytes: item.size_bytes as i64,
            hash: item.hash.clone(),
            ttl_secs: item.ttl_secs as i64,
            is_burn_after_read: item.is_burn_after_read,
            notify_on_open: item.notify_on_open,
            source_path: item.source_path.clone(),
        };
        ferry_store::outbound::create_and_enqueue(&mut self.conn, &store_item, actor)
            .map_err(|e| StoreError(e.to_string()))
    }

    fn get_outbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
        ferry_store::outbound::get(&self.conn, item_id)
            .map(|found| found.map(|item| item.state))
            .map_err(|e| StoreError(e.to_string()))
    }

    fn set_outbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError> {
        ferry_store::outbound::set_state(&self.conn, item_id, state).map_err(|e| StoreError(e.to_string()))
    }

    fn create_inbound(&mut self, offer: &Offer, peer_id: &str, _actor: &str) -> Result<(), StoreError> {
        let new_item = ferry_store::inbox::NewInboxItem {
            peer_id: peer_id.to_string(),
            kind: offer.kind,
            name: offer.name.clone(),
            size_bytes: offer.size_bytes as i64,
            hash: offer.hash.clone(),
            is_burn_after_read: offer.burn_after_read,
            notify_on_open: offer.notify_on_open,
        };
        ferry_store::inbox::insert_with_id(&self.conn, &offer.item_id.0, &new_item)
            .map_err(|e| StoreError(e.to_string()))?;

        if offer.kind == ItemKind::Secret {
            let already_has_key = ferry_store::inbox::get(&self.conn, &offer.item_id.0)
                .map_err(|e| StoreError(e.to_string()))?
                .and_then(|item| item.wrapped_dek)
                .is_some();
            if !already_has_key {
                let dek = DataEncryptionKey::generate();
                let recipient = self.local_identity.sealing_identity().to_public();
                let wrapped = secret::wrap_dek(&recipient, &dek).map_err(|e| StoreError(e.to_string()))?;
                ferry_store::inbox::set_wrapped_dek(&self.conn, &offer.item_id.0, &hex::encode(wrapped))
                    .map_err(|e| StoreError(e.to_string()))?;
            }
        }

        Ok(())
    }

    fn get_inbound_state(&self, item_id: &str) -> Result<Option<TransferState>, StoreError> {
        ferry_store::inbox::get(&self.conn, item_id)
            .map(|found| found.map(|item| item.state))
            .map_err(|e| StoreError(e.to_string()))
    }

    fn set_inbound_state(&mut self, item_id: &str, state: TransferState) -> Result<(), StoreError> {
        ferry_store::inbox::set_state(&self.conn, item_id, state).map_err(|e| StoreError(e.to_string()))
    }

    fn inbound_bytes_received(&self, item_id: &str) -> Result<u64, StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;
        Ok(item.bytes_received_count as u64)
    }

    fn append_inbound_chunk(&mut self, item_id: &str, _seq: u64, bytes: &[u8]) -> Result<(), StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;

        if item.kind == ItemKind::Secret {
            let dek = self.unwrap_dek_for(item_id)?;
            let framed = secret::encrypt_chunk(&dek, bytes).map_err(|e| StoreError(e.to_string()))?;
            ferry_store::payload::append_framed(&self.payload_dir, item_id, &framed)
                .map_err(|e| StoreError(e.to_string()))?;
        } else {
            ferry_store::payload::truncate_to(
                &self.payload_dir,
                item_id,
                item.bytes_received_count as u64,
            )
            .map_err(|e| StoreError(e.to_string()))?;
            ferry_store::payload::append(&self.payload_dir, item_id, bytes)
                .map_err(|e| StoreError(e.to_string()))?;
        }

        ferry_store::inbox::set_bytes_received_count(
            &self.conn,
            item_id,
            item.bytes_received_count + bytes.len() as i64,
        )
        .map_err(|e| StoreError(e.to_string()))
    }

    fn inbound_full_hash_so_far(&self, item_id: &str) -> Result<String, StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;

        let mut hasher = Sha256::new();
        if item.kind == ItemKind::Secret {
            let dek = self.unwrap_dek_for(item_id)?;
            let units = ferry_store::payload::read_all_framed_units(&self.payload_dir, item_id)
                .map_err(|e| StoreError(e.to_string()))?;
            for unit in units {
                let plaintext = secret::decrypt_chunk(&dek, &unit).map_err(|e| StoreError(e.to_string()))?;
                hasher.update(plaintext.expose());
            }
        } else {
            let bytes = ferry_store::payload::read_all(&self.payload_dir, item_id)
                .map_err(|e| StoreError(e.to_string()))?;
            hasher.update(&bytes);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    fn finalize_inbound_delivered(&mut self, item_id: &str) -> Result<(), StoreError> {
        ferry_store::inbox::set_payload_ref(&self.conn, item_id, item_id)
            .map_err(|e| StoreError(e.to_string()))?;
        ferry_store::inbox::set_delivered(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))?;
        ferry_store::audit::append(&self.conn, "local", "item.delivered", Some(item_id), "delivered")
            .map_err(|e| StoreError(e.to_string()))?;
        Ok(())
    }

    fn mark_inbound_opened(&mut self, item_id: &str) -> Result<(), StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;
        if item.is_burn_after_read {
            if item.kind == ItemKind::Secret {
                ferry_store::inbox::clear_wrapped_dek(&self.conn, item_id)
                    .map_err(|e| StoreError(e.to_string()))?;
            }
            ferry_store::payload::delete(&self.payload_dir, item_id).map_err(|e| StoreError(e.to_string()))?;
        }
        ferry_store::inbox::set_opened(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))?;

        let outcome = if item.is_burn_after_read { "opened_and_burned" } else { "opened" };
        if let Err(e) = ferry_store::audit::append(&self.conn, "local", "item.opened", Some(item_id), outcome) {
            eprintln!("ferry-daemon: WARNING: item {item_id} was opened but the audit row could not be written: {e}");
        }

        if item.notify_on_open {
            ferry_store::receipts::queue(&self.conn, item_id, &item.peer_id)
                .map_err(|e| StoreError(e.to_string()))?;
        }
        Ok(())
    }

    fn stage_outbound_content(&mut self, content: &[u8]) -> Result<String, StoreError> {
        let staging_dir = crate::staged::staging_dir(&self.payload_dir);
        std::fs::create_dir_all(&staging_dir)
            .map_err(|e| StoreError(format!("could not prepare the inline staging directory: {e}")))?;

        let recipient = self.local_identity.sealing_identity().to_public();
        let sealed = ferry_crypto::seal::seal(&recipient, content).map_err(|e| StoreError(e.to_string()))?;

        let path = staging_dir.join(uuid::Uuid::now_v7().to_string());
        crate::staged::write_owner_only(&path, &sealed)
            .map_err(|e| StoreError(format!("could not stage the inline content: {e}")))?;
        Ok(path.to_string_lossy().into_owned())
    }

    fn discard_staged_source(&mut self, item_id: &str) -> Result<(), StoreError> {
        let Some(item) = ferry_store::outbound::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
        else {
            return Ok(());
        };
        let path = std::path::Path::new(&item.source_path);
        let is_ferry_staged = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|name| name == ferry_core::config::INLINE_STAGING_DIR)
            .unwrap_or(false);
        if is_ferry_staged {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    fn list_open_receipts_for_peer(&self, peer_id: &str) -> Result<Vec<String>, StoreError> {
        ferry_store::receipts::list_for_peer(&self.conn, peer_id).map_err(|e| StoreError(e.to_string()))
    }

    fn remove_open_receipt(&mut self, item_id: &str) -> Result<(), StoreError> {
        ferry_store::receipts::remove(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))
    }

    fn get_outbound_item(&self, item_id: &str) -> Result<Option<NewOutboundItem>, StoreError> {
        let found = ferry_store::outbound::get(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))?;
        Ok(found.map(|item| NewOutboundItem {
            peer_id: item.peer_id,
            kind: item.kind,
            name: item.name,
            size_bytes: item.size_bytes as u64,
            hash: item.hash,
            ttl_secs: item.ttl_secs as u32,
            is_burn_after_read: item.is_burn_after_read,
            notify_on_open: item.notify_on_open,
            source_path: item.source_path,
        }))
    }

    fn list_outbox_for_peer(&self, peer_id: &str) -> Result<Vec<ferry_core::ports::OutboxItem>, StoreError> {
        let entries =
            ferry_store::outbox::list_for_peer(&self.conn, peer_id).map_err(|e| StoreError(e.to_string()))?;
        Ok(entries
            .into_iter()
            .map(|e| ferry_core::ports::OutboxItem {
                outbox_id: e.id,
                item_id: e.item_id,
                peer_id: e.peer_id,
                attempts: e.attempts.max(0) as u32,
                last_attempted_at_millis: e.last_attempted_at_millis,
            })
            .collect())
    }

    fn record_outbox_attempt(&mut self, outbox_id: &str) -> Result<(), StoreError> {
        ferry_store::outbox::record_attempt(&self.conn, outbox_id).map_err(|e| StoreError(e.to_string()))
    }

    fn remove_outbox_entry(&mut self, outbox_id: &str) -> Result<(), StoreError> {
        ferry_store::outbox::remove(&self.conn, outbox_id).map_err(|e| StoreError(e.to_string()))
    }

    fn set_inbound_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError> {
        ferry_store::inbox::set_expiry(
            &self.conn,
            item_id,
            deadline.expires_at_monotonic_millis,
            deadline.expires_at_wall_estimate_millis,
            &deadline.session_id,
        )
        .map_err(|e| StoreError(e.to_string()))
    }

    fn get_inbound_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError> {
        let found = ferry_store::inbox::get(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))?;
        Ok(found.and_then(|item| {
            Some(ExpiryDeadline {
                session_id: item.expiry_session_id?,
                expires_at_monotonic_millis: item.expires_at_monotonic?,
                expires_at_wall_estimate_millis: item.expires_at_wall_estimate_millis?,
            })
        }))
    }

    fn set_outbox_expiry(&mut self, item_id: &str, deadline: &ExpiryDeadline) -> Result<(), StoreError> {
        ferry_store::outbox::set_expiry_for_item(
            &self.conn,
            item_id,
            deadline.expires_at_monotonic_millis,
            deadline.expires_at_wall_estimate_millis,
            &deadline.session_id,
        )
        .map_err(|e| StoreError(e.to_string()))
    }

    fn get_outbox_expiry(&self, item_id: &str) -> Result<Option<ExpiryDeadline>, StoreError> {
        let found = ferry_store::outbox::get_for_item(&self.conn, item_id).map_err(|e| StoreError(e.to_string()))?;
        Ok(found.and_then(|entry| {
            Some(ExpiryDeadline {
                session_id: entry.outbox_expiry_session_id?,
                expires_at_monotonic_millis: entry.outbox_expires_at_monotonic?,
                expires_at_wall_estimate_millis: entry.outbox_expires_at_wall_estimate_millis?,
            })
        }))
    }

    fn record_outbound_dropped(&mut self, item_id: &str, actor: &str, cause: &str) -> Result<(), StoreError> {
        let outcome = if cause.is_empty() { "dropped" } else { cause };
        ferry_store::audit::append(&self.conn, actor, "item.outbox_expired", Some(item_id), outcome)
            .map(|_| ())
            .map_err(|e| StoreError(e.to_string()))
    }

    fn set_outbound_last_error(&mut self, item_id: &str, reason: &str) -> Result<(), StoreError> {
        ferry_store::outbound::set_last_error(&self.conn, item_id, reason)
            .map_err(|e| StoreError(e.to_string()))
    }

    fn record_provider_event(&mut self, kind: &str, outcome: &str) -> Result<(), StoreError> {
        ferry_store::audit::append(&self.conn, "local", kind, None, outcome)
            .map(|_| ())
            .map_err(|e| StoreError(e.to_string()))
    }

    fn enqueue_remote_job(&mut self, kind: &str, params_json: &str) -> Result<String, StoreError> {
        ferry_store::remote_jobs::enqueue(&self.conn, kind, params_json).map_err(|e| StoreError(e.to_string()))
    }

    fn get_remote_job(&self, job_id: &str) -> Result<Option<ferry_core::ports::RemoteJobRow>, StoreError> {
        Ok(ferry_store::remote_jobs::get(&self.conn, job_id)
            .map_err(|e| StoreError(e.to_string()))?
            .map(|j| ferry_core::ports::RemoteJobRow {
                job_id: j.id,
                kind: j.kind,
                phase: j.state,
                result: j.result,
                error: j.error,
            }))
    }

    fn roster_signing_keys_hex(&self) -> Result<Vec<String>, StoreError> {
        ferry_store::roster::list_peers(&self.conn)
            .map(|peers| peers.into_iter().map(|p| p.signing_key).collect())
            .map_err(|e| StoreError(e.to_string()))
    }

    fn read_outbound_content(&self, item_id: &str) -> Result<Vec<u8>, StoreError> {
        use std::io::Read;
        let item = ferry_store::outbound::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such outbound item".into()))?;
        let mut reader = crate::staged::open_source(
            std::path::Path::new(&item.source_path),
            &self.local_identity,
        )
        .map_err(|e| StoreError(e.to_string()))?;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map_err(|e| StoreError(e.to_string()))?;
        Ok(buf)
    }

    fn read_inbound_plaintext(&self, item_id: &str) -> Result<Vec<u8>, StoreError> {
        let item = ferry_store::inbox::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError("no such inbound item".into()))?;

        if item.kind == ItemKind::Secret {
            let dek = self.unwrap_dek_for(item_id)?;
            let units = ferry_store::payload::read_all_framed_units(&self.payload_dir, item_id)
                .map_err(|e| StoreError(e.to_string()))?;
            let mut plaintext = Vec::new();
            for unit in units {
                let chunk = secret::decrypt_chunk(&dek, &unit).map_err(|e| StoreError(e.to_string()))?;
                plaintext.extend_from_slice(chunk.expose());
            }
            Ok(plaintext)
        } else {
            ferry_store::payload::read_all(&self.payload_dir, item_id).map_err(|e| StoreError(e.to_string()))
        }
    }

    fn is_healthy(&self) -> Result<bool, StoreError> {
        self.conn
            .query_row("SELECT 1", [], |_| Ok(()))
            .map(|()| true)
            .map_err(|e| StoreError(e.to_string()))
    }

    fn list_roster_peers(&self) -> Result<Vec<ferry_core::ports::RosterPeer>, StoreError> {
        let peers = ferry_store::roster::list_peers(&self.conn).map_err(|e| StoreError(e.to_string()))?;
        Ok(peers
            .into_iter()
            .map(|p| {
                let (reachable, last_seen_millis) = match &self.presence {
                    Some(presence) => (presence.is_reachable(&p.peer_id), presence.last_seen(&p.peer_id)),
                    None => (false, None),
                };
                ferry_core::ports::RosterPeer {
                    peer_id: p.peer_id,
                    display_name: p.display_name,
                    paired_at_millis: p.paired_at_millis,
                    reachable,
                    last_seen_millis,
                }
            })
            .collect())
    }

    fn export_signed_roster(&self) -> Result<String, StoreError> {
        let peers = ferry_store::roster::list_peers(&self.conn).map_err(|e| StoreError(e.to_string()))?;
        let entries = peers
            .into_iter()
            .map(|p| {
                let signing_key: [u8; 32] = hex::decode(&p.signing_key)
                    .map_err(|e| StoreError(format!("stored signing key is not valid hex: {e}")))?
                    .try_into()
                    .map_err(|_| StoreError("stored signing key is not 32 bytes".into()))?;
                Ok(ferry_crypto::roster::RosterEntry {
                    peer_id: ferry_proto::ids::PeerId(p.peer_id),
                    display_name: p.display_name,
                    signing_key,
                    sealing_key: p.sealing_key,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        let signed = ferry_crypto::roster::SignedRoster::sign(&self.local_identity, entries)
            .map_err(|e| StoreError(e.to_string()))?;
        serde_json::to_string(&signed).map_err(|e| StoreError(e.to_string()))
    }

    fn import_signed_roster(&mut self, signed_roster_json: &str) -> Result<ferry_core::ports::RosterImportSummary, StoreError> {
        let signed: ferry_crypto::roster::SignedRoster = serde_json::from_str(signed_roster_json)
            .map_err(|e| StoreError(format!("malformed roster file: {e}")))?;
        let entries = signed.verify().map_err(|e| StoreError(e.to_string()))?;
        let peer_count = entries.len();

        let mut added = 0;
        let mut skipped_existing = 0;
        for entry in entries {
            let exists = ferry_store::roster::get_peer(&self.conn, &entry.peer_id.0)
                .map_err(|e| StoreError(e.to_string()))?
                .is_some();
            if exists {
                skipped_existing += 1;
                continue;
            }
            ferry_store::roster::insert_peer(
                &self.conn,
                &entry.peer_id.0,
                &entry.display_name,
                &hex::encode(entry.signing_key),
                &entry.sealing_key,
            )
            .map_err(|e| StoreError(e.to_string()))?;
            added += 1;
        }

        ferry_store::audit::append(
            &self.conn,
            "local",
            "roster.imported",
            None,
            &format!("added {added} of {peer_count}, signed by {}", signed.signer.0),
        )
        .map_err(|e| StoreError(e.to_string()))?;

        Ok(ferry_core::ports::RosterImportSummary {
            signer_verifying_key_hex: signed.signer.0,
            peer_count,
            added,
            skipped_existing,
        })
    }

    fn add_paired_peer(&mut self, peer: &ferry_core::ports::NewRosterPeer) -> Result<(), StoreError> {
        if ferry_store::roster::get_peer(&self.conn, &peer.peer_id)
            .map_err(|e| StoreError(e.to_string()))?
            .is_some()
        {
            return Err(StoreError(format!("already paired with {}", peer.peer_id)));
        }
        ferry_store::roster::insert_peer(
            &self.conn,
            &peer.peer_id,
            &peer.display_name,
            &peer.signing_key_hex,
            &peer.sealing_key,
        )
        .map_err(|e| StoreError(e.to_string()))?;
        ferry_store::audit::append(&self.conn, "local", "peer.paired", Some(&peer.peer_id), "ok")
            .map_err(|e| StoreError(e.to_string()))?;
        Ok(())
    }

    fn list_inbox_items(&self) -> Result<Vec<ferry_core::ports::InboxItemSummary>, StoreError> {
        let items = ferry_store::inbox::list(&self.conn).map_err(|e| StoreError(e.to_string()))?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let origin_display_name = self.display_name_for(&item.peer_id)?;
            out.push(ferry_core::ports::InboxItemSummary {
                origin_display_name,
                item_id: item.id,
                peer_id: item.peer_id,
                kind: item.kind,
                name: item.name,
                state: item.state,
                size_bytes: item.size_bytes as u64,
                is_burn_after_read: item.is_burn_after_read,
                hash_hex: Some(item.hash),
                received_at_millis: Some(item.received_at_millis),
                expires_at_millis: item.expires_at_wall_estimate_millis,
            });
        }
        Ok(out)
    }

    fn list_sent_items(&self) -> Result<Vec<ferry_core::ports::SentItem>, StoreError> {
        let items = ferry_store::outbound::list_all(&self.conn).map_err(|e| StoreError(e.to_string()))?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let entry = ferry_store::outbox::get_for_item(&self.conn, &item.id)
                .map_err(|e| StoreError(e.to_string()))?;
            let peer_display_name = self.display_name_for(&item.peer_id)?;
            out.push(ferry_core::ports::SentItem {
                item_id: item.id,
                peer_display_name,
                peer_id: item.peer_id,
                name: item.name,
                hash_hex: item.hash,
                kind: item.kind,
                state: item.state,
                size_bytes: item.size_bytes.max(0) as u64,
                queued_at_millis: Some(item.created_at_millis),
                last_attempt_at_millis: entry.as_ref().and_then(|e| e.last_attempted_at_millis),
                last_error: item.last_error,
            });
        }
        Ok(out)
    }

    fn list_audit(&self, limit: u32, before_millis: Option<i64>) -> Result<Vec<ferry_core::ports::AuditRecord>, StoreError> {
        let events = ferry_store::audit::list(&self.conn).map_err(|e| StoreError(e.to_string()))?;
        Ok(events
            .into_iter()
            .rev()
            .filter(|e| before_millis.map(|b| e.occurred_at_millis < b).unwrap_or(true))
            .take(limit.max(1) as usize)
            .map(|e| ferry_core::ports::AuditRecord {
                id: e.id,
                actor: e.actor,
                kind: e.kind,
                item_id: e.item_id,
                occurred_at_millis: e.occurred_at_millis,
                outcome: e.outcome,
            })
            .collect())
    }

    fn remove_peer(&mut self, peer_id: &str) -> Result<(), StoreError> {
        ferry_store::roster::remove_peer(&self.conn, peer_id).map_err(|e| StoreError(e.to_string()))?;
        let _ = ferry_store::audit::append(&self.conn, "local", "peer.removed", Some(peer_id), "ok");
        Ok(())
    }

    fn abort_outbound(&mut self, item_id: &str, actor: &str) -> Result<(), StoreError> {
        self.record_outbound_dropped(item_id, actor, "aborted")?;
        let _ = ferry_store::outbound::set_last_error(&self.conn, item_id, "you cancelled this transfer");
        if let Some(entry) = ferry_store::outbox::get_for_item(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
        {
            ferry_store::outbox::remove(&self.conn, &entry.id).map_err(|e| StoreError(e.to_string()))?;
        }
        Ok(())
    }

    fn retry_outbound(&mut self, item_id: &str, _actor: &str) -> Result<(), StoreError> {
        let item = ferry_store::outbound::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError(format!("no outbound item {item_id}")))?;
        if ferry_store::outbox::get_for_item(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .is_none()
        {
            ferry_store::outbox::enqueue(&self.conn, item_id, &item.peer_id, None)
                .map_err(|e| StoreError(e.to_string()))?;
        }
        ferry_store::outbound::set_state(&self.conn, item_id, TransferState::Queued)
            .map_err(|e| StoreError(e.to_string()))?;
        let _ = ferry_store::outbound::set_last_error(&self.conn, item_id, "");
        let _ = ferry_store::audit::append(&self.conn, "local", "outbox.retry", Some(item_id), "ok");
        Ok(())
    }

    fn build_sealed_blob(&self, item_id: &str) -> Result<Vec<u8>, StoreError> {
        let item = ferry_store::outbound::get(&self.conn, item_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError(format!("no outbound item {item_id}")))?;

        let peer = ferry_store::roster::get_peer(&self.conn, &item.peer_id)
            .map_err(|e| StoreError(e.to_string()))?
            .ok_or_else(|| StoreError(format!("recipient {} is not in the roster", item.peer_id)))?;

        let payload = std::fs::read(&item.source_path)
            .map_err(|e| StoreError(format!("could not read the item's source file: {e}")))?;

        let manifest = ferry_proto::blob::BlobManifest {
            item_id: item.id,
            origin_peer_id: self.local_identity.fingerprint(),
            kind: item.kind,
            name: item.name,
            size_bytes: payload.len() as u64,
            hash: hex::encode(Sha256::digest(&payload)),
            ttl_secs: item.ttl_secs.max(0) as u32,
            is_burn_after_read: item.is_burn_after_read,
            notify_on_open: item.notify_on_open,
            payload,
        };

        ferry_crypto::blob::seal_blob(&peer.sealing_key, &manifest).map_err(|e| StoreError(e.to_string()))
    }

    fn open_sealed_blob(&self, blob: &[u8]) -> Result<ferry_proto::blob::BlobManifest, StoreError> {
        ferry_crypto::blob::open_blob(&self.local_identity, blob).map_err(|e| StoreError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::outbox::{enqueue_send, OutboxError};
    use ferry_core::policy::PolicyError;
    use ferry_core::transfer::{open_item, receive_item, send_item};
    use ferry_proto::states::ItemKind;
    use std::io::Cursor;
    use std::sync::mpsc::{self, Receiver, Sender};

    struct FakeChannel {
        peer_id: String,
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
    }

    impl ferry_core::ports::Channel for FakeChannel {
        fn send(&mut self, bytes: &[u8]) -> Result<(), ferry_core::ports::ChannelError> {
            self.tx
                .send(bytes.to_vec())
                .map_err(|e| ferry_core::ports::ChannelError(e.to_string()))
        }

        fn recv(&mut self) -> Result<Vec<u8>, ferry_core::ports::ChannelError> {
            self.rx
                .recv()
                .map_err(|e| ferry_core::ports::ChannelError(e.to_string()))
        }

        fn remote_peer_id(&self) -> &str {
            &self.peer_id
        }
    }

    fn paired_channels(a_id: &str, b_id: &str) -> (FakeChannel, FakeChannel) {
        let (tx_a, rx_b) = mpsc::channel();
        let (tx_b, rx_a) = mpsc::channel();
        (
            FakeChannel {
                peer_id: b_id.into(),
                tx: tx_a,
                rx: rx_a,
            },
            FakeChannel {
                peer_id: a_id.into(),
                tx: tx_b,
                rx: rx_b,
            },
        )
    }

    fn temp_payload_dir() -> PathBuf {
        std::env::temp_dir().join(format!("ferry-store-adapter-test-{}", uuid::Uuid::now_v7()))
    }

    fn sample_item(peer_id: &str) -> NewOutboundItem {
        NewOutboundItem {
            peer_id: peer_id.into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 4096,
            hash: "beefcafe".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/tmp/notes.txt".into(),
        }
    }

    fn store_with_peer(peer_id: &str, display_name: &str) -> SqliteStore {
        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, peer_id, display_name, "sk", "xk").unwrap();
        SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate())
    }

    #[test]
    fn discard_staged_source_deletes_ferry_staged_content_but_never_a_users_own_file() {
        let dir = temp_payload_dir();
        let staged_dir = dir.join(ferry_core::config::INLINE_STAGING_DIR);
        std::fs::create_dir_all(&staged_dir).unwrap();
        let staged = staged_dir.join("inline-payload");
        std::fs::write(&staged, b"secret bytes").unwrap();

        let user_file = dir.join("my-own-report.pdf");
        std::fs::write(&user_file, b"the user's own file").unwrap();

        let mut store = store_with_peer("peer-b", "bob");

        let mut staged_item = sample_item("peer-b");
        staged_item.source_path = staged.to_string_lossy().into_owned();
        let staged_id = store.create_and_enqueue_outbound(&staged_item, "local").unwrap();

        let mut user_item = sample_item("peer-b");
        user_item.source_path = user_file.to_string_lossy().into_owned();
        let user_id = store.create_and_enqueue_outbound(&user_item, "local").unwrap();

        store.discard_staged_source(&staged_id).unwrap();
        store.discard_staged_source(&user_id).unwrap();

        assert!(!staged.exists(), "ferry-staged inline content must be removed once the item leaves the outbox");
        assert!(
            user_file.exists(),
            "a source path the user chose must never be deleted by the daemon"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_sent_items_joins_the_roster_display_name_and_reflects_state() {
        let mut store = store_with_peer("peer-b", "bob-desktop");
        let id = store.create_and_enqueue_outbound(&sample_item("peer-b"), "local").unwrap();
        store.set_outbound_state(&id, TransferState::Offered).unwrap();

        let sent = store.list_sent_items().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].item_id, id);
        assert_eq!(sent[0].peer_display_name, "bob-desktop");
        assert_eq!(sent[0].state, TransferState::Offered);
        assert_eq!(sent[0].hash_hex, "beefcafe");
    }

    #[test]
    fn list_audit_applies_limit_and_before_cursor_newest_first() {
        let store = store_with_peer("peer-b", "bob");
        for kind in ["a.one", "a.two", "a.three", "a.four"] {
            ferry_store::audit::append(&store.conn, "local", kind, None, "ok").unwrap();
        }
        let all = store.list_audit(10, None).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].kind, "a.four", "audit list is newest-first");

        let limited = store.list_audit(2, None).unwrap();
        assert_eq!(limited.len(), 2);

        let cursor = all[1].occurred_at_millis;
        let before = store.list_audit(10, Some(cursor)).unwrap();
        assert!(before.iter().all(|e| e.occurred_at_millis < cursor));
    }

    #[test]
    fn remove_peer_deletes_the_row_and_writes_a_content_free_audit_entry() {
        let mut store = store_with_peer("peer-b", "bob");
        store.remove_peer("peer-b").unwrap();
        assert!(store.list_roster_peers().unwrap().is_empty());
        let audit = store.list_audit(10, None).unwrap();
        assert!(audit.iter().any(|e| e.kind == "peer.removed" && e.item_id.as_deref() == Some("peer-b")));
    }

    #[test]
    fn abort_outbound_drops_the_outbox_entry_and_retry_requeues_it() {
        let mut store = store_with_peer("peer-b", "bob");
        let clock = ferry_core::expiry::ExpiryClock::new();
        let id = enqueue_send(&mut store, &clock, &sample_item("peer-b"), "local").unwrap();

        store.abort_outbound(&id, "local").unwrap();
        assert!(ferry_store::outbox::list_for_peer(&store.conn, "peer-b").unwrap().is_empty());

        store.retry_outbound(&id, "local").unwrap();
        assert_eq!(ferry_store::outbox::list_for_peer(&store.conn, "peer-b").unwrap().len(), 1);
        assert_eq!(store.get_outbound_state(&id).unwrap(), Some(TransferState::Queued));
    }

    #[test]
    fn list_roster_peers_reports_reachability_from_the_presence_tracker() {
        use crate::presence::Presence;
        use std::sync::Arc;

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-online", "a", "sk", "xk").unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-offline", "b", "sk", "xk").unwrap();

        let presence = Arc::new(Presence::new());
        presence.seen("peer-online");
        let store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate())
            .with_presence(presence);

        let peers = store.list_roster_peers().unwrap();
        let online = peers.iter().find(|p| p.peer_id == "peer-online").unwrap();
        let offline = peers.iter().find(|p| p.peer_id == "peer-offline").unwrap();
        assert!(online.reachable && online.last_seen_millis.is_some());
        assert!(!offline.reachable && offline.last_seen_millis.is_none());
    }

    #[test]
    fn a_sealed_blob_round_trips_from_exporter_to_recipient_through_the_standard_inbound_path() {
        use ferry_crypto::identity::Identity;

        let exporter_identity = Identity::generate();
        let recipient_identity = Identity::generate();
        let recipient_peer_id = recipient_identity.fingerprint();
        let exporter_peer_id = exporter_identity.fingerprint();

        let exporter_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(
            &exporter_conn,
            &recipient_peer_id,
            "recipient-laptop",
            &hex::encode(recipient_identity.verifying_key().to_bytes()),
            &recipient_identity.public().sealing_key,
        )
        .unwrap();
        let mut exporter = SqliteStore::new(exporter_conn, temp_payload_dir(), exporter_identity.clone());

        let payload = b"credentials that must travel by usb stick".to_vec();
        let dir = temp_payload_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let source_path = dir.join("secret.env");
        std::fs::write(&source_path, &payload).unwrap();

        let clock = ferry_core::expiry::ExpiryClock::new();
        let item = NewOutboundItem {
            peer_id: recipient_peer_id.clone(),
            kind: ItemKind::File,
            name: "secret.env".into(),
            size_bytes: payload.len() as u64,
            hash: "recomputed-on-export".into(),
            ttl_secs: 3600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: source_path.to_string_lossy().into_owned(),
        };
        let item_id = enqueue_send(&mut exporter, &clock, &item, "local").unwrap();

        let blob = exporter.build_sealed_blob(&item_id).unwrap();

        let recipient_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(
            &recipient_conn,
            &exporter_peer_id,
            "exporter-laptop",
            &hex::encode(exporter_identity.verifying_key().to_bytes()),
            &exporter_identity.public().sealing_key,
        )
        .unwrap();
        let mut recipient = SqliteStore::new(recipient_conn, temp_payload_dir(), recipient_identity);

        let manifest = recipient.open_sealed_blob(&blob).unwrap();
        assert_eq!(manifest.origin_peer_id, exporter_peer_id);

        let summary = ferry_core::blob::import_manifest(&mut recipient, &clock, &manifest, "local").unwrap();
        assert_eq!(summary.name, "secret.env");
        assert_eq!(
            recipient.get_inbound_state(&item_id).unwrap(),
            Some(TransferState::Delivered)
        );
        assert_eq!(recipient.read_inbound_plaintext(&item_id).unwrap(), payload);
        assert!(
            recipient.get_inbound_expiry(&item_id).unwrap().is_some(),
            "expiry must be armed at import time"
        );

        let stranger = SqliteStore::new(
            ferry_store::connection::open_in_memory().unwrap(),
            temp_payload_dir(),
            Identity::generate(),
        );
        assert!(stranger.open_sealed_blob(&blob).is_err());
    }

    #[test]
    fn core_outbox_enqueue_send_works_against_a_real_sqlite_backed_store() {
        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-1", "laptop", "sk", "xk").unwrap();
        let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let clock = ferry_core::expiry::ExpiryClock::new();

        let item_id = enqueue_send(&mut store, &clock, &sample_item("peer-1"), "local").unwrap();

        assert_eq!(
            store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Queued)
        );

        let outbox_rows = ferry_store::outbox::list_for_peer(&store.conn, "peer-1").unwrap();
        assert_eq!(outbox_rows.len(), 1);
        assert_eq!(outbox_rows[0].item_id, item_id);
        assert!(
            outbox_rows[0].outbox_expiry_session_id.is_some(),
            "enqueue must also set an outbox-TTL deadline"
        );

        let audit_events = ferry_store::audit::list(&store.conn).unwrap();
        assert_eq!(audit_events.len(), 1);
    }

    #[test]
    fn core_policy_rejects_an_unrostered_peer_through_the_real_store() {
        let conn = ferry_store::connection::open_in_memory().unwrap();
        let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let clock = ferry_core::expiry::ExpiryClock::new();

        let result = enqueue_send(&mut store, &clock, &sample_item("stranger"), "local");

        assert!(matches!(
            result,
            Err(OutboxError::Policy(PolicyError::PeerNotAuthorized))
        ));

        let outbound_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM outbound_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            outbound_count, 0,
            "policy must reject before any row is written to the real database"
        );
    }

    #[test]
    fn full_transfer_engine_round_trip_against_two_real_sqlite_stores_including_open_and_burn() {
        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store = SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store = SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let payload = b"the quick brown fox jumps over the lazy dog".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let mut item = sample_item("peer-b");
        item.hash = expected_hash.clone();
        item.size_bytes = payload.len() as u64;
        item.is_burn_after_read = true;
        item.notify_on_open = true;

        let sender_clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = enqueue_send(&mut sender_store, &sender_clock, &item, "local").unwrap();

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let sender_item_id = item_id.clone();
        let sender_thread = std::thread::spawn(move || {
            send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload))
                .unwrap();
            (sender_channel, sender_store)
        });

        let clock = ferry_core::expiry::ExpiryClock::new();
        let offer = receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        assert_eq!(offer.item_id.0, item_id);
        assert_eq!(
            ferry_store::payload::read_all(&receiver_store.payload_dir, &item_id).unwrap(),
            payload
        );

        open_item(&mut receiver_channel, &mut receiver_store, &clock, &item_id).unwrap();

        let (_sender_channel, sender_store) = sender_thread.join().unwrap();

        assert_eq!(
            sender_store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Opened)
        );
        assert_eq!(
            receiver_store.get_inbound_state(&item_id).unwrap(),
            Some(TransferState::Opened)
        );
        assert!(
            ferry_store::payload::read_all(&receiver_store.payload_dir, &item_id)
                .unwrap()
                .is_empty(),
            "burn-after-read must clear the on-disk payload once opened"
        );

        let second_open = open_item(&mut receiver_channel, &mut receiver_store, &clock, &item_id);
        assert!(matches!(
            second_open,
            Err(ferry_core::transfer::TransferError::AlreadyOpened(_))
        ));

        let audit = receiver_store.list_audit(50, None).unwrap();
        assert!(audit.iter().any(|e| e.kind == "item.delivered" && e.item_id.as_deref() == Some(item_id.as_str())));
        assert!(audit.iter().any(|e| e.kind == "item.opened"
            && e.item_id.as_deref() == Some(item_id.as_str())
            && e.outcome == "opened_and_burned"));
        assert!(
            audit.iter().all(|e| !e.outcome.contains("fox") && !e.kind.contains("fox")),
            "audit rows must never carry payload bytes"
        );
    }

    #[test]
    fn a_notify_on_open_item_opened_offline_queues_a_receipt_that_later_drains_to_the_sender() {
        use ferry_core::dispatcher::drain_outbox_for_peer;
        use ferry_core::transfer::{open_item_locally, receive_next_inbound, InboundEvent};

        struct NoSource;
        impl ferry_core::ports::OutboundSource for NoSource {
            type Reader = Cursor<Vec<u8>>;
            fn open(&mut self, _item_id: &str) -> std::io::Result<Self::Reader> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no outbox entries expected"))
            }
        }

        let clock = ferry_core::expiry::ExpiryClock::new();

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "receiver-peer", "laptop", "sk", "xk").unwrap();
        let mut sender_store =
            SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let mut outbound = sample_item("receiver-peer");
        outbound.notify_on_open = true;
        let item_id = sender_store.create_and_enqueue_outbound(&outbound, "local").unwrap();
        for state in [TransferState::Offered, TransferState::Accepted, TransferState::Transferring, TransferState::Delivered] {
            sender_store.set_outbound_state(&item_id, state).unwrap();
        }

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "sender-peer", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let offer = ferry_proto::envelope::Offer {
            item_id: ferry_proto::ids::ItemId(item_id.clone()),
            kind: ItemKind::File,
            name: "n".into(),
            size_bytes: 0,
            hash: hex::encode(Sha256::digest(b"")),
            ttl_secs: 3600,
            burn_after_read: false,
            notify_on_open: true,
        };
        receiver_store.create_inbound(&offer, "sender-peer", "local").unwrap();
        receiver_store.set_inbound_state(&item_id, TransferState::Accepted).unwrap();
        receiver_store.set_inbound_state(&item_id, TransferState::Transferring).unwrap();
        receiver_store.finalize_inbound_delivered(&item_id).unwrap();

        open_item_locally(&mut receiver_store, &clock, &item_id).unwrap();
        assert_eq!(
            receiver_store.list_open_receipts_for_peer("sender-peer").unwrap(),
            vec![item_id.clone()],
            "opening a notify_on_open item offline must queue a receipt for its sender"
        );

        let (mut receiver_side, mut sender_side) = paired_channels("sender-peer", "receiver-peer");
        let sender_item_id = item_id.clone();
        let sender_thread = std::thread::spawn(move || {
            let event = receive_next_inbound(&mut sender_side, &mut sender_store, &clock, "receiver-peer", "local", true, &mut |_, _, _| {}).unwrap();
            (event, sender_store.get_outbound_state(&sender_item_id).unwrap())
        });

        let mut no_source = NoSource;
        let outcome = drain_outbox_for_peer(
            &mut receiver_store,
            &mut receiver_side,
            &clock,
            "sender-peer",
            "local",
            &mut no_source,
        )
        .unwrap();
        assert_eq!(outcome.receipts_sent, vec![item_id.clone()]);
        assert!(
            receiver_store.list_open_receipts_for_peer("sender-peer").unwrap().is_empty(),
            "a delivered receipt must be removed from the queue"
        );

        let (event, sender_final_state) = sender_thread.join().unwrap();
        assert_eq!(event, InboundEvent::OpenReceipt(item_id));
        assert_eq!(
            sender_final_state,
            Some(TransferState::Opened),
            "receiving the deferred Opened receipt must move the sender's item to Opened"
        );
    }

    #[test]
    fn outbox_survives_a_simulated_daemon_restart_and_the_dispatcher_drains_it_on_reopen() {
        use ferry_core::dispatcher::drain_outbox_for_peer;
        use ferry_core::ports::OutboundSource;

        let db_path = std::env::temp_dir().join(format!("ferry-restart-test-{}.sqlite", uuid::Uuid::now_v7()));
        let source_dir = temp_payload_dir();
        std::fs::create_dir_all(&source_dir).unwrap();
        let source_file = source_dir.join("item-payload.bin");
        std::fs::write(&source_file, b"outbox survives restart").unwrap();

        let mut hasher = Sha256::new();
        hasher.update(b"outbox survives restart");
        let expected_hash = hex::encode(hasher.finalize());

        let item_id = {
            let conn = ferry_store::connection::open(&db_path).unwrap();
            ferry_store::roster::insert_peer(&conn, "peer-b", "laptop", "sk", "xk").unwrap();
            let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
            let item = NewOutboundItem {
                peer_id: "peer-b".into(),
                kind: ItemKind::File,
                name: "restart-test.bin".into(),
                size_bytes: 24,
                hash: expected_hash.clone(),
                ttl_secs: 600,
                is_burn_after_read: false,
                notify_on_open: false,
                source_path: source_file.to_string_lossy().into_owned(),
            };
            let clock = ferry_core::expiry::ExpiryClock::new();
            enqueue_send(&mut store, &clock, &item, "local").unwrap()
        };

        let conn = ferry_store::connection::open(&db_path).unwrap();
        let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        assert_eq!(
            store.get_outbound_state(&item_id).unwrap(),
            Some(TransferState::Queued),
            "the outbox entry must have survived the restart"
        );

        struct FileSource {
            dir: PathBuf,
        }
        impl OutboundSource for FileSource {
            type Reader = std::fs::File;
            fn open(&mut self, _item_id: &str) -> std::io::Result<Self::Reader> {
                std::fs::File::open(self.dir.join("item-payload.bin"))
            }
        }
        let mut source = FileSource { dir: source_dir };

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let receiver_thread = std::thread::spawn(move || {
            let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
            ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
            let mut receiver_store = SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
            let clock = ferry_core::expiry::ExpiryClock::new();
            receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap()
        });

        let post_restart_clock = ferry_core::expiry::ExpiryClock::new();
        let outcome = drain_outbox_for_peer(
            &mut store,
            &mut sender_channel,
            &post_restart_clock,
            "peer-b",
            "local",
            &mut source,
        )
        .unwrap();
        let offer = receiver_thread.join().unwrap();

        assert_eq!(outcome.delivered, vec![item_id.clone()]);
        assert_eq!(offer.item_id.0, item_id);
        assert_eq!(store.get_outbound_state(&item_id).unwrap(), Some(TransferState::Delivered));

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn an_item_past_its_outbox_ttl_is_dropped_with_a_real_durable_audit_notice() {
        use ferry_core::dispatcher::drain_outbox_for_peer;
        use ferry_core::ports::OutboundSource;

        let conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let mut item = sample_item("peer-b");
        item.ttl_secs = 0;
        let clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = enqueue_send(&mut store, &clock, &item, "local").unwrap();

        struct EmptySource;
        impl OutboundSource for EmptySource {
            type Reader = std::io::Cursor<Vec<u8>>;
            fn open(&mut self, _item_id: &str) -> std::io::Result<Self::Reader> {
                Ok(std::io::Cursor::new(Vec::new()))
            }
        }
        let mut source = EmptySource;

        let (mut sender_channel, _receiver_channel) = paired_channels("peer-a", "peer-b");
        let outcome =
            drain_outbox_for_peer(&mut store, &mut sender_channel, &clock, "peer-b", "local", &mut source).unwrap();

        assert_eq!(outcome.dropped, vec![item_id.clone()]);
        assert_eq!(store.get_outbound_state(&item_id).unwrap(), Some(TransferState::Expired));

        let audit_events = ferry_store::audit::list(&store.conn).unwrap();
        let drop_event = audit_events
            .iter()
            .find(|e| e.kind == "item.outbox_expired")
            .expect("the drop must leave a real, durable audit_events row");
        assert_eq!(drop_event.item_id.as_deref(), Some(item_id.as_str()));
        assert_eq!(drop_event.outcome, "outbox_ttl");
        assert_eq!(drop_event.actor, "local");

        let sent = store.list_sent_items().unwrap();
        let row = sent.iter().find(|s| s.item_id == item_id).unwrap();
        assert_eq!(
            row.last_error.as_deref(),
            Some("the peer did not reappear before the outbox TTL expired"),
            "the sender must see why the item was dropped"
        );
    }

    #[test]
    fn full_secret_transfer_round_trip_with_real_sealed_storage_and_wrapped_key_first_burn() {
        use ferry_core::ports::Store;
        use ferry_core::transfer::{open_item, receive_item, send_item};

        let payload: Vec<u8> = (0..80_000u32).map(|n| (n % 251) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store =
            SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let mut item = sample_item("peer-b");
        item.kind = ItemKind::Secret;
        item.hash = expected_hash.clone();
        item.size_bytes = payload.len() as u64;
        item.is_burn_after_read = true;
        item.notify_on_open = true;

        let clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = enqueue_send(&mut sender_store, &clock, &item, "local").unwrap();

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let sender_item_id = item_id.clone();
        let sender_thread = std::thread::spawn(move || {
            send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload))
                .unwrap();
            sender_store
        });

        receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();

        let inbound = ferry_store::inbox::get(&receiver_store.conn, &item_id).unwrap().unwrap();
        assert!(
            inbound.wrapped_dek.is_some(),
            "a secret item must have a wrapped per-item key once delivered"
        );
        let on_disk = ferry_store::payload::read_all(&receiver_store.payload_dir, &item_id).unwrap();
        assert!(
            !on_disk
                .windows(64.min(payload.len()))
                .any(|w| w == &payload[..w.len()]),
            "the payload on disk must never contain a recognizable run of the plaintext"
        );

        let decrypted = receiver_store.read_inbound_plaintext(&item_id).unwrap();
        assert_eq!(decrypted, payload, "sealed local storage must be readable fully offline before burn");

        open_item(&mut receiver_channel, &mut receiver_store, &clock, &item_id).unwrap();
        sender_thread.join().unwrap();

        let inbound_after_burn = ferry_store::inbox::get(&receiver_store.conn, &item_id).unwrap().unwrap();
        assert!(
            inbound_after_burn.wrapped_dek.is_none(),
            "burn must clear the wrapped key"
        );
        assert_eq!(
            ferry_store::payload::len(&receiver_store.payload_dir, &item_id).unwrap(),
            0,
            "burn must also delete the ciphertext"
        );
        assert!(
            receiver_store.read_inbound_plaintext(&item_id).is_err(),
            "a burned item must no longer be readable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_fault_during_burn_leaves_unopenable_ciphertext_and_never_records_opened() {
        use ferry_core::ports::Store;
        use ferry_core::transfer::{receive_item, send_item};
        use std::os::unix::fs::PermissionsExt;

        let payload = b"a secret that must not survive an interrupted burn".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store =
            SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let mut item = sample_item("peer-b");
        item.kind = ItemKind::Secret;
        item.hash = expected_hash;
        item.size_bytes = payload.len() as u64;
        item.is_burn_after_read = true;

        let clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = enqueue_send(&mut sender_store, &clock, &item, "local").unwrap();

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let sender_item_id = item_id.clone();
        let sender_thread = std::thread::spawn(move || {
            send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload))
                .unwrap();
        });
        receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        sender_thread.join().unwrap();

        assert!(receiver_store.read_inbound_plaintext(&item_id).is_ok());

        let dir = receiver_store.payload_dir.clone();
        let locked = std::fs::Permissions::from_mode(0o500);
        let unlocked = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&dir, locked).unwrap();
        let burn_result = receiver_store.mark_inbound_opened(&item_id);
        std::fs::set_permissions(&dir, unlocked).unwrap();

        assert!(
            burn_result.is_err(),
            "a filesystem fault mid-burn must surface as an error, not be swallowed"
        );

        let inbound = ferry_store::inbox::get(&receiver_store.conn, &item_id).unwrap().unwrap();
        assert!(
            inbound.wrapped_dek.is_none(),
            "the wrapped key must be cleared before the ciphertext delete is attempted"
        );
        assert_ne!(
            inbound.state,
            TransferState::Opened,
            "an interrupted burn must never record the item as cleanly Opened"
        );
        assert!(
            receiver_store.read_inbound_plaintext(&item_id).is_err(),
            "leftover ciphertext without its key must be unopenable, never an openable orphan"
        );
        let audit = ferry_store::audit::list(&receiver_store.conn).unwrap();
        assert!(
            !audit.iter().any(|e| e.kind == "item.opened" && e.item_id.as_deref() == Some(item_id.as_str())),
            "no item.opened row may be written when the burn did not complete"
        );
    }

    #[test]
    fn exported_roster_round_trips_into_a_second_real_store_and_lists_correctly() {
        use ferry_core::ports::Store;

        let exporter_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&exporter_conn, "peer-a", "laptop-a", &hex::encode([1u8; 32]), "xk-a")
            .unwrap();
        ferry_store::roster::insert_peer(&exporter_conn, "peer-b", "laptop-b", &hex::encode([2u8; 32]), "xk-b")
            .unwrap();
        let exporter_identity = ferry_crypto::identity::Identity::generate();
        let exporter_store = SqliteStore::new(exporter_conn, temp_payload_dir(), exporter_identity.clone());

        let listed = exporter_store.list_roster_peers().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].peer_id, "peer-a");

        let signed_roster_json = exporter_store.export_signed_roster().unwrap();

        let importer_conn = ferry_store::connection::open_in_memory().unwrap();
        let mut importer_store =
            SqliteStore::new(importer_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let summary = importer_store.import_signed_roster(&signed_roster_json).unwrap();
        assert_eq!(summary.peer_count, 2);
        assert_eq!(summary.added, 2);
        assert_eq!(summary.skipped_existing, 0);
        assert_eq!(
            summary.signer_verifying_key_hex,
            hex::encode(exporter_identity.verifying_key().to_bytes())
        );

        let imported = importer_store.list_roster_peers().unwrap();
        assert_eq!(imported.len(), 2);
        assert!(imported.iter().any(|p| p.peer_id == "peer-a"));
        assert!(imported.iter().any(|p| p.peer_id == "peer-b"));

        let audit = importer_store.list_audit(50, None).unwrap();
        assert!(
            audit.iter().any(|e| e.kind == "roster.imported" && e.outcome.starts_with("added 2 of 2")),
            "a roster import must leave one summary audit row"
        );
    }

    #[test]
    fn importing_the_same_signed_roster_twice_skips_already_present_peers() {
        use ferry_core::ports::Store;

        let exporter_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&exporter_conn, "peer-a", "laptop-a", &hex::encode([1u8; 32]), "xk-a")
            .unwrap();
        let exporter_store =
            SqliteStore::new(exporter_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let signed_roster_json = exporter_store.export_signed_roster().unwrap();

        let importer_conn = ferry_store::connection::open_in_memory().unwrap();
        let mut importer_store =
            SqliteStore::new(importer_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        importer_store.import_signed_roster(&signed_roster_json).unwrap();
        let second = importer_store.import_signed_roster(&signed_roster_json).unwrap();

        assert_eq!(second.added, 0);
        assert_eq!(second.skipped_existing, 1);
        assert_eq!(importer_store.list_roster_peers().unwrap().len(), 1);
    }

    #[test]
    fn importing_a_tampered_signed_roster_is_rejected_and_writes_nothing() {
        use ferry_core::ports::Store;

        let exporter_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&exporter_conn, "peer-a", "laptop-a", &hex::encode([1u8; 32]), "xk-a")
            .unwrap();
        let exporter_store =
            SqliteStore::new(exporter_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let signed_roster_json = exporter_store.export_signed_roster().unwrap();

        let tampered = signed_roster_json.replace("laptop-a", "attacker-renamed");
        assert_ne!(tampered, signed_roster_json, "the tamper must actually change the payload");

        let importer_conn = ferry_store::connection::open_in_memory().unwrap();
        let mut importer_store =
            SqliteStore::new(importer_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let result = importer_store.import_signed_roster(&tampered);
        assert!(result.is_err());
        assert!(importer_store.list_roster_peers().unwrap().is_empty());
    }

    #[test]
    fn add_paired_peer_persists_a_real_roster_row_and_rejects_a_duplicate() {
        use ferry_core::ports::{NewRosterPeer, Store};

        let conn = ferry_store::connection::open_in_memory().unwrap();
        let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let peer = NewRosterPeer {
            peer_id: "aabbccdd".into(),
            display_name: "phone".into(),
            signing_key_hex: hex::encode([7u8; 32]),
            sealing_key: "age1stub".into(),
        };
        store.add_paired_peer(&peer).unwrap();

        let listed = store.list_roster_peers().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].peer_id, "aabbccdd");
        assert_eq!(listed[0].display_name, "phone");

        let result = store.add_paired_peer(&peer);
        assert!(result.is_err(), "pairing with an already-paired peer_id must be rejected");
        assert_eq!(store.list_roster_peers().unwrap().len(), 1);

        let audit = store.list_audit(50, None).unwrap();
        assert!(
            audit.iter().any(|e| e.kind == "peer.paired" && e.item_id.as_deref() == Some("aabbccdd")),
            "a completed pairing must leave a peer.paired audit row"
        );
    }

    #[test]
    fn interrupted_burn_on_the_real_store_leaves_unopenable_ciphertext_not_an_openable_orphan() {
        use ferry_core::ports::Store;
        use ferry_core::transfer::{receive_item, send_item};

        let payload: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_hash = hex::encode(hasher.finalize());

        let sender_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&sender_conn, "peer-b", "laptop", "sk", "xk").unwrap();
        let mut sender_store =
            SqliteStore::new(sender_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let receiver_conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&receiver_conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let mut receiver_store =
            SqliteStore::new(receiver_conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());

        let mut item = sample_item("peer-b");
        item.kind = ItemKind::Secret;
        item.hash = expected_hash.clone();
        item.size_bytes = payload.len() as u64;
        item.is_burn_after_read = true;

        let clock = ferry_core::expiry::ExpiryClock::new();
        let item_id = enqueue_send(&mut sender_store, &clock, &item, "local").unwrap();

        let (mut sender_channel, mut receiver_channel) = paired_channels("peer-a", "peer-b");
        let sender_payload = payload.clone();
        let sender_item_id = item_id.clone();
        let sender_thread = std::thread::spawn(move || {
            send_item(&mut sender_channel, &mut sender_store, &sender_item_id, &item, Cursor::new(sender_payload)).unwrap();
        });
        receive_item(&mut receiver_channel, &mut receiver_store, &clock, "peer-a", "local").unwrap();
        sender_thread.join().unwrap();

        assert_eq!(
            receiver_store.read_inbound_plaintext(&item_id).unwrap(),
            payload,
            "sanity: the sealed item is readable before any burn step runs"
        );

        ferry_store::inbox::clear_wrapped_dek(&receiver_store.conn, &item_id).unwrap();

        assert!(
            ferry_store::payload::len(&receiver_store.payload_dir, &item_id).unwrap() > 0,
            "the interrupted burn stopped before the ciphertext was deleted — it is still on disk"
        );
        assert!(
            receiver_store.read_inbound_plaintext(&item_id).is_err(),
            "with the wrapped key gone the ciphertext is undecryptable — an interrupted burn must never leave an openable orphan"
        );
        assert_eq!(
            receiver_store.get_inbound_state(&item_id).unwrap(),
            Some(TransferState::Delivered),
            "set_opened never ran, so check_openable would still permit an open attempt — which then fails on the missing key, never returns plaintext"
        );
    }

    #[test]
    fn audit_rows_survive_a_daemon_restart() {
        use ferry_core::ports::{NewRosterPeer, Store};

        let db_path = std::env::temp_dir().join(format!("ferry-audit-restart-{}.sqlite", uuid::Uuid::now_v7()));

        {
            let conn = ferry_store::connection::open(&db_path).unwrap();
            let mut store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
            store
                .add_paired_peer(&NewRosterPeer {
                    peer_id: "cafef00d".into(),
                    display_name: "workstation".into(),
                    signing_key_hex: hex::encode([9u8; 32]),
                    sealing_key: "age1stub".into(),
                })
                .unwrap();
        }

        let conn = ferry_store::connection::open(&db_path).unwrap();
        let store = SqliteStore::new(conn, temp_payload_dir(), ferry_crypto::identity::Identity::generate());
        let audit = store.list_audit(50, None).unwrap();
        assert!(
            audit.iter().any(|e| e.kind == "peer.paired" && e.item_id.as_deref() == Some("cafef00d")),
            "an audit row written before a restart must still be there after reopening the same store file"
        );

        let _ = std::fs::remove_file(&db_path);
    }
}
