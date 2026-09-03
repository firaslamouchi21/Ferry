use ferry_proto::states::{ItemKind, TransferState};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInboxItem {
    pub peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub size_bytes: i64,
    pub hash: String,
    pub is_burn_after_read: bool,
    pub notify_on_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItem {
    pub id: String,
    pub peer_id: String,
    pub kind: ItemKind,
    pub name: String,
    pub state: TransferState,
    pub size_bytes: i64,
    pub hash: String,
    pub payload_ref: Option<String>,
    pub received_at_millis: i64,
    pub delivered_at_millis: Option<i64>,
    pub expires_at_monotonic: Option<i64>,
    pub expires_at_wall_estimate_millis: Option<i64>,
    pub expiry_session_id: Option<String>,
    pub is_burn_after_read: bool,
    pub bytes_received_count: i64,
    pub is_opened: bool,
    pub wrapped_dek: Option<String>,
    pub notify_on_open: bool,
}

#[derive(Debug, Error)]
pub enum InboxError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("stored state/kind value is not a recognized enum variant: {0}")]
    UnrecognizedEnumValue(String),
}

pub fn insert(conn: &Connection, item: &NewInboxItem) -> Result<String, InboxError> {
    let id = Uuid::now_v7().to_string();
    insert_with_id(conn, &id, item)?;
    Ok(id)
}

pub fn insert_with_id(conn: &Connection, id: &str, item: &NewInboxItem) -> Result<(), InboxError> {
    conn.execute(
        "INSERT OR IGNORE INTO inbox_items
            (id, peer_id, kind, name, state, size_bytes, hash, received_at_millis, is_burn_after_read, notify_on_open)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            item.peer_id,
            serde_plain::to_string(&item.kind).unwrap(),
            item.name,
            serde_plain::to_string(&TransferState::Offered).unwrap(),
            item.size_bytes,
            item.hash,
            now_millis(),
            item.is_burn_after_read as i64,
            item.notify_on_open as i64,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<InboxItem>, InboxError> {
    conn.query_row(
        "SELECT id, peer_id, kind, name, state, size_bytes, hash, payload_ref,
                received_at_millis, delivered_at_millis, expires_at_monotonic,
                expires_at_wall_estimate_millis, is_burn_after_read,
                bytes_received_count, is_opened, expiry_session_id, wrapped_dek, notify_on_open
         FROM inbox_items WHERE id = ?1",
        params![id],
        row_to_item,
    )
    .optional()?
    .transpose()
}

pub fn list(conn: &Connection) -> Result<Vec<InboxItem>, InboxError> {
    let rows: Result<Vec<Result<InboxItem, InboxError>>, rusqlite::Error> = conn
        .prepare(
            "SELECT id, peer_id, kind, name, state, size_bytes, hash, payload_ref,
                    received_at_millis, delivered_at_millis, expires_at_monotonic,
                    expires_at_wall_estimate_millis, is_burn_after_read,
                    bytes_received_count, is_opened, expiry_session_id, wrapped_dek, notify_on_open
             FROM inbox_items ORDER BY received_at_millis",
        )?
        .query_map([], row_to_item)?
        .collect();
    rows?.into_iter().collect()
}

pub fn set_state(conn: &Connection, id: &str, state: TransferState) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET state = ?2 WHERE id = ?1",
        params![id, serde_plain::to_string(&state).unwrap()],
    )?;
    Ok(())
}

pub fn set_payload_ref(conn: &Connection, id: &str, payload_ref: &str) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET payload_ref = ?2 WHERE id = ?1",
        params![id, payload_ref],
    )?;
    Ok(())
}

pub fn set_bytes_received_count(conn: &Connection, id: &str, count: i64) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET bytes_received_count = ?2 WHERE id = ?1",
        params![id, count],
    )?;
    Ok(())
}

pub fn set_opened(conn: &Connection, id: &str) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET is_opened = 1, state = ?2 WHERE id = ?1",
        params![id, serde_plain::to_string(&TransferState::Opened).unwrap()],
    )?;
    Ok(())
}

pub fn set_delivered(conn: &Connection, id: &str) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET delivered_at_millis = ?2, state = ?3 WHERE id = ?1",
        params![
            id,
            now_millis(),
            serde_plain::to_string(&TransferState::Delivered).unwrap()
        ],
    )?;
    Ok(())
}

pub fn set_expiry(
    conn: &Connection,
    id: &str,
    expires_at_monotonic: i64,
    expires_at_wall_estimate_millis: i64,
    expiry_session_id: &str,
) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items
         SET expires_at_monotonic = ?2, expires_at_wall_estimate_millis = ?3, expiry_session_id = ?4
         WHERE id = ?1",
        params![id, expires_at_monotonic, expires_at_wall_estimate_millis, expiry_session_id],
    )?;
    Ok(())
}

pub fn set_wrapped_dek(conn: &Connection, id: &str, wrapped_dek: &str) -> Result<(), InboxError> {
    conn.execute(
        "UPDATE inbox_items SET wrapped_dek = ?2 WHERE id = ?1",
        params![id, wrapped_dek],
    )?;
    Ok(())
}

pub fn clear_wrapped_dek(conn: &Connection, id: &str) -> Result<(), InboxError> {
    conn.execute("UPDATE inbox_items SET wrapped_dek = NULL WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), InboxError> {
    conn.execute("DELETE FROM inbox_items WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Result<InboxItem, InboxError>> {
    let kind_raw: String = row.get(2)?;
    let state_raw: String = row.get(4)?;

    let kind = match serde_plain::from_str::<ItemKind>(&kind_raw) {
        Ok(k) => k,
        Err(_) => return Ok(Err(InboxError::UnrecognizedEnumValue(kind_raw))),
    };
    let state = match serde_plain::from_str::<TransferState>(&state_raw) {
        Ok(s) => s,
        Err(_) => return Ok(Err(InboxError::UnrecognizedEnumValue(state_raw))),
    };

    Ok(Ok(InboxItem {
        id: row.get(0)?,
        peer_id: row.get(1)?,
        kind,
        name: row.get(3)?,
        state,
        size_bytes: row.get(5)?,
        hash: row.get(6)?,
        payload_ref: row.get(7)?,
        received_at_millis: row.get(8)?,
        delivered_at_millis: row.get(9)?,
        expires_at_monotonic: row.get(10)?,
        expires_at_wall_estimate_millis: row.get(11)?,
        is_burn_after_read: row.get::<_, i64>(12)? != 0,
        bytes_received_count: row.get(13)?,
        is_opened: row.get::<_, i64>(14)? != 0,
        expiry_session_id: row.get(15)?,
        wrapped_dek: row.get(16)?,
        notify_on_open: row.get::<_, i64>(17)? != 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;
    use crate::roster::insert_peer;

    fn conn_with_peer() -> Connection {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "laptop", "sk", "xk").unwrap();
        conn
    }

    fn sample_item() -> NewInboxItem {
        NewInboxItem {
            peer_id: "peer-1".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 1024,
            hash: "deadbeef".into(),
            is_burn_after_read: false,
            notify_on_open: false,
        }
    }

    #[test]
    fn list_returns_all_items_in_arrival_order() {
        let conn = conn_with_peer();
        let mut first = sample_item();
        first.name = "first.txt".into();
        let mut second = sample_item();
        second.name = "second.txt".into();

        let first_id = insert(&conn, &first).unwrap();
        let second_id = insert(&conn, &second).unwrap();

        let items = list(&conn).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, first_id);
        assert_eq!(items[1].id, second_id);
    }

    #[test]
    fn list_is_empty_when_nothing_has_arrived() {
        let conn = conn_with_peer();
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn insert_defaults_to_offered_state() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert_eq!(item.state, TransferState::Offered);
        assert_eq!(item.kind, ItemKind::File);
        assert!(!item.is_burn_after_read);
        assert_eq!(item.bytes_received_count, 0);
        assert!(!item.is_opened);
    }

    #[test]
    fn insert_with_id_is_idempotent_on_a_repeated_wire_item_id() {
        let conn = conn_with_peer();
        insert_with_id(&conn, "item-1", &sample_item()).unwrap();
        set_bytes_received_count(&conn, "item-1", 512).unwrap();
        insert_with_id(&conn, "item-1", &sample_item()).unwrap();

        let item = get(&conn, "item-1").unwrap().unwrap();
        assert_eq!(
            item.bytes_received_count, 512,
            "a repeated insert (e.g. a re-offer during resume) must not reset progress"
        );
    }

    #[test]
    fn set_state_updates_stored_state() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        set_state(&conn, &id, TransferState::Transferring).unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert_eq!(item.state, TransferState::Transferring);
    }

    #[test]
    fn set_delivered_stamps_time_and_state() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        set_delivered(&conn, &id).unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert_eq!(item.state, TransferState::Delivered);
        assert!(item.delivered_at_millis.is_some());
    }

    #[test]
    fn set_opened_stamps_flag_and_state() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        set_opened(&conn, &id).unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert!(item.is_opened);
        assert_eq!(item.state, TransferState::Opened);
    }

    #[test]
    fn burn_after_read_flag_round_trips() {
        let conn = conn_with_peer();
        let mut new_item = sample_item();
        new_item.is_burn_after_read = true;
        let id = insert(&conn, &new_item).unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert!(item.is_burn_after_read);
    }

    #[test]
    fn set_expiry_stores_both_clocks_and_the_session_id() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        set_expiry(&conn, &id, 12_345, 67_890, "session-a").unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert_eq!(item.expires_at_monotonic, Some(12_345));
        assert_eq!(item.expires_at_wall_estimate_millis, Some(67_890));
        assert_eq!(item.expiry_session_id.as_deref(), Some("session-a"));
    }

    #[test]
    fn set_and_clear_wrapped_dek_round_trips_and_clears() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        set_wrapped_dek(&conn, &id, "age-wrapped-key-material").unwrap();

        let item = get(&conn, &id).unwrap().unwrap();
        assert_eq!(item.wrapped_dek.as_deref(), Some("age-wrapped-key-material"));

        clear_wrapped_dek(&conn, &id).unwrap();
        let item = get(&conn, &id).unwrap().unwrap();
        assert!(
            item.wrapped_dek.is_none(),
            "clearing the wrapped key must leave no recoverable key material behind"
        );
    }

    #[test]
    fn delete_removes_the_row() {
        let conn = conn_with_peer();
        let id = insert(&conn, &sample_item()).unwrap();
        delete(&conn, &id).unwrap();
        assert!(get(&conn, &id).unwrap().is_none());
    }

    #[test]
    fn missing_item_returns_none() {
        let conn = conn_with_peer();
        assert!(get(&conn, "no-such-id").unwrap().is_none());
    }
}
