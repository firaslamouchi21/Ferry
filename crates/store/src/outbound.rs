use ferry_proto::states::{ItemKind, TransferState};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOutboundItem {
    pub peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: i64,
    pub hash: String,
    pub ttl_secs: i64,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundItem {
    pub id: String,
    pub peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub state: TransferState,
    pub size_bytes: i64,
    pub hash: String,
    pub ttl_secs: i64,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
    pub created_at_millis: i64,
    pub source_path: String,
}

#[derive(Debug, Error)]
pub enum OutboundError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("stored kind/state value is not a recognized enum variant: {0}")]
    UnrecognizedEnumValue(String),
}

pub fn create_and_enqueue(
    conn: &mut Connection,
    item: &NewOutboundItem,
    actor: &str,
) -> Result<String, OutboundError> {
    let item_id = Uuid::now_v7().to_string();
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO outbound_items
            (id, peer_id, kind, name, state, size_bytes, hash, ttl_secs, is_burn_after_read, notify_on_open, created_at_millis, source_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            item_id,
            item.peer_id,
            serde_plain::to_string(&item.kind).unwrap(),
            item.name,
            serde_plain::to_string(&TransferState::Queued).unwrap(),
            item.size_bytes,
            item.hash,
            item.ttl_secs,
            item.is_burn_after_read as i64,
            item.notify_on_open as i64,
            now_millis(),
            item.source_path,
        ],
    )?;

    let outbox_id = Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO outbox_entries (id, item_id, peer_id, enqueued_at_millis, outbox_expires_at_monotonic, attempts)
         VALUES (?1, ?2, ?3, ?4, NULL, 0)",
        params![outbox_id, item_id, item.peer_id, now_millis()],
    )?;

    let audit_id = Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO audit_events (id, actor, kind, item_id, occurred_at_millis, outcome)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            audit_id,
            actor,
            "item.queued",
            item_id,
            now_millis(),
            "queued"
        ],
    )?;

    tx.commit()?;
    Ok(item_id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<OutboundItem>, OutboundError> {
    conn.query_row(
        "SELECT id, peer_id, kind, name, state, size_bytes, hash, ttl_secs, is_burn_after_read, notify_on_open, created_at_millis, source_path
         FROM outbound_items WHERE id = ?1",
        params![id],
        row_to_item,
    )
    .optional()?
    .transpose()
}

pub fn list_all(conn: &Connection) -> Result<Vec<OutboundItem>, OutboundError> {
    let rows: Vec<Result<OutboundItem, OutboundError>> = conn
        .prepare(
            "SELECT id, peer_id, kind, name, state, size_bytes, hash, ttl_secs, is_burn_after_read, notify_on_open, created_at_millis, source_path
             FROM outbound_items ORDER BY created_at_millis DESC",
        )?
        .query_map([], row_to_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().collect()
}

pub fn set_state(conn: &Connection, id: &str, state: TransferState) -> Result<(), OutboundError> {
    conn.execute(
        "UPDATE outbound_items SET state = ?2 WHERE id = ?1",
        params![id, serde_plain::to_string(&state).unwrap()],
    )?;
    Ok(())
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Result<OutboundItem, OutboundError>> {
    let kind_raw: String = row.get(2)?;
    let state_raw: String = row.get(4)?;

    let kind = match serde_plain::from_str::<ItemKind>(&kind_raw) {
        Ok(k) => k,
        Err(_) => return Ok(Err(OutboundError::UnrecognizedEnumValue(kind_raw))),
    };
    let state = match serde_plain::from_str::<TransferState>(&state_raw) {
        Ok(s) => s,
        Err(_) => return Ok(Err(OutboundError::UnrecognizedEnumValue(state_raw))),
    };

    Ok(Ok(OutboundItem {
        id: row.get(0)?,
        peer_id: row.get(1)?,
        kind,
        name: row.get(3)?,
        state,
        size_bytes: row.get(5)?,
        hash: row.get(6)?,
        ttl_secs: row.get(7)?,
        is_burn_after_read: row.get::<_, i64>(8)? != 0,
        notify_on_open: row.get::<_, i64>(9)? != 0,
        created_at_millis: row.get(10)?,
        source_path: row.get(11)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit;
    use crate::connection::open_in_memory;
    use crate::outbox;
    use crate::roster::insert_peer;

    fn conn_with_peer() -> Connection {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "laptop", "sk", "xk").unwrap();
        conn
    }

    fn sample_item() -> NewOutboundItem {
        NewOutboundItem {
            peer_id: "peer-1".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 2048,
            hash: "cafef00d".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: "/home/user/notes.txt".into(),
        }
    }

    #[test]
    fn create_and_enqueue_writes_item_outbox_row_and_audit_entry_together() {
        let mut conn = conn_with_peer();
        let item_id = create_and_enqueue(&mut conn, &sample_item(), "local").unwrap();

        let item = get(&conn, &item_id).unwrap().unwrap();
        assert_eq!(item.state, TransferState::Queued);
        assert_eq!(item.peer_id, "peer-1");
        assert!(!item.notify_on_open);
        assert_eq!(item.source_path, "/home/user/notes.txt");

        let outbox_rows = outbox::list_for_peer(&conn, "peer-1").unwrap();
        assert_eq!(outbox_rows.len(), 1);
        assert_eq!(outbox_rows[0].item_id, item_id);

        let audit_events = audit::list(&conn).unwrap();
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].item_id.as_deref(), Some(item_id.as_str()));
        assert_eq!(audit_events[0].outcome, "queued");
    }

    #[test]
    fn create_and_enqueue_for_unknown_peer_leaves_no_partial_rows_in_any_table() {
        let mut conn = open_in_memory().unwrap();
        let mut bad_item = sample_item();
        bad_item.peer_id = "no-such-peer".into();

        let result = create_and_enqueue(&mut conn, &bad_item, "local");
        assert!(result.is_err());

        let outbound_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbound_items", [], |r| r.get(0))
            .unwrap();
        let outbox_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox_entries", [], |r| r.get(0))
            .unwrap();
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_events", [], |r| r.get(0))
            .unwrap();

        assert_eq!(outbound_count, 0, "outbound_items must be empty after a rolled-back transaction");
        assert_eq!(outbox_count, 0, "outbox_entries must be empty after a rolled-back transaction");
        assert_eq!(audit_count, 0, "audit_events must be empty after a rolled-back transaction");
    }

    #[test]
    fn set_state_updates_stored_state() {
        let mut conn = conn_with_peer();
        let item_id = create_and_enqueue(&mut conn, &sample_item(), "local").unwrap();
        set_state(&conn, &item_id, TransferState::Offered).unwrap();

        let item = get(&conn, &item_id).unwrap().unwrap();
        assert_eq!(item.state, TransferState::Offered);
    }

    #[test]
    fn missing_item_returns_none() {
        let conn = conn_with_peer();
        assert!(get(&conn, "no-such-id").unwrap().is_none());
    }
}
