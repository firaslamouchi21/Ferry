use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub id: String,
    pub item_id: String,
    pub peer_id: String,
    pub enqueued_at_millis: i64,
    pub outbox_expires_at_monotonic: Option<i64>,
    pub outbox_expires_at_wall_estimate_millis: Option<i64>,
    pub outbox_expiry_session_id: Option<String>,
    pub attempts: i64,
    pub last_attempted_at_millis: Option<i64>,
}

pub fn enqueue(
    conn: &Connection,
    item_id: &str,
    peer_id: &str,
    outbox_expires_at_monotonic: Option<i64>,
) -> rusqlite::Result<String> {
    let id = Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO outbox_entries (id, item_id, peer_id, enqueued_at_millis, outbox_expires_at_monotonic, attempts)
         VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![id, item_id, peer_id, now_millis(), outbox_expires_at_monotonic],
    )?;
    Ok(id)
}

pub fn set_expiry_for_item(
    conn: &Connection,
    item_id: &str,
    outbox_expires_at_monotonic: i64,
    outbox_expires_at_wall_estimate_millis: i64,
    outbox_expiry_session_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE outbox_entries
         SET outbox_expires_at_monotonic = ?2,
             outbox_expires_at_wall_estimate_millis = ?3,
             outbox_expiry_session_id = ?4
         WHERE item_id = ?1",
        params![
            item_id,
            outbox_expires_at_monotonic,
            outbox_expires_at_wall_estimate_millis,
            outbox_expiry_session_id
        ],
    )?;
    Ok(())
}

pub fn get_for_item(conn: &Connection, item_id: &str) -> rusqlite::Result<Option<OutboxEntry>> {
    conn.query_row(
        "SELECT id, item_id, peer_id, enqueued_at_millis, outbox_expires_at_monotonic,
                outbox_expires_at_wall_estimate_millis, outbox_expiry_session_id,
                attempts, last_attempted_at_millis
         FROM outbox_entries WHERE item_id = ?1",
        params![item_id],
        row_to_entry,
    )
    .optional()
}

pub fn list_for_peer(conn: &Connection, peer_id: &str) -> rusqlite::Result<Vec<OutboxEntry>> {
    conn.prepare(
        "SELECT id, item_id, peer_id, enqueued_at_millis, outbox_expires_at_monotonic,
                outbox_expires_at_wall_estimate_millis, outbox_expiry_session_id,
                attempts, last_attempted_at_millis
         FROM outbox_entries WHERE peer_id = ?1 ORDER BY enqueued_at_millis",
    )?
    .query_map(params![peer_id], row_to_entry)?
    .collect()
}

pub fn record_attempt(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE outbox_entries SET attempts = attempts + 1, last_attempted_at_millis = ?2 WHERE id = ?1",
        params![id, now_millis()],
    )?;
    Ok(())
}

pub fn remove(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM outbox_entries WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<OutboxEntry> {
    Ok(OutboxEntry {
        id: row.get(0)?,
        item_id: row.get(1)?,
        peer_id: row.get(2)?,
        enqueued_at_millis: row.get(3)?,
        outbox_expires_at_monotonic: row.get(4)?,
        outbox_expires_at_wall_estimate_millis: row.get(5)?,
        outbox_expiry_session_id: row.get(6)?,
        attempts: row.get(7)?,
        last_attempted_at_millis: row.get(8)?,
    })
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

    #[test]
    fn enqueue_and_list_round_trips() {
        let conn = conn_with_peer();
        let id = enqueue(&conn, "item-1", "peer-1", None).unwrap();

        let entries = list_for_peer(&conn, "peer-1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].attempts, 0);
    }

    #[test]
    fn record_attempt_increments_counter() {
        let conn = conn_with_peer();
        let id = enqueue(&conn, "item-1", "peer-1", None).unwrap();
        record_attempt(&conn, &id).unwrap();
        record_attempt(&conn, &id).unwrap();

        let entries = list_for_peer(&conn, "peer-1").unwrap();
        assert_eq!(entries[0].attempts, 2);
        assert!(entries[0].last_attempted_at_millis.is_some());
    }

    #[test]
    fn remove_drains_the_entry() {
        let conn = conn_with_peer();
        let id = enqueue(&conn, "item-1", "peer-1", None).unwrap();
        remove(&conn, &id).unwrap();
        assert!(list_for_peer(&conn, "peer-1").unwrap().is_empty());
    }

    #[test]
    fn enqueue_for_unknown_peer_is_rejected_by_foreign_key() {
        let conn = open_in_memory().unwrap();
        let result = enqueue(&conn, "item-1", "no-such-peer", None);
        assert!(result.is_err());
    }

    #[test]
    fn set_expiry_for_item_stores_both_clocks_and_the_session_id() {
        let conn = conn_with_peer();
        enqueue(&conn, "item-1", "peer-1", None).unwrap();
        set_expiry_for_item(&conn, "item-1", 111, 222, "session-a").unwrap();

        let entry = get_for_item(&conn, "item-1").unwrap().unwrap();
        assert_eq!(entry.outbox_expires_at_monotonic, Some(111));
        assert_eq!(entry.outbox_expires_at_wall_estimate_millis, Some(222));
        assert_eq!(entry.outbox_expiry_session_id.as_deref(), Some("session-a"));
    }

    #[test]
    fn get_for_item_returns_none_when_missing() {
        let conn = conn_with_peer();
        assert!(get_for_item(&conn, "no-such-item").unwrap().is_none());
    }
}
