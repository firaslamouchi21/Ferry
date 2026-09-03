use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub actor: String,
    pub kind: String,
    pub item_id: Option<String>,
    pub occurred_at_millis: i64,
    pub outcome: String,
}

pub fn append(
    conn: &Connection,
    actor: &str,
    kind: &str,
    item_id: Option<&str>,
    outcome: &str,
) -> rusqlite::Result<String> {
    let id = Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO audit_events (id, actor, kind, item_id, occurred_at_millis, outcome)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, actor, kind, item_id, now_millis(), outcome],
    )?;
    Ok(id)
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<AuditEvent>> {
    conn.prepare(
        "SELECT id, actor, kind, item_id, occurred_at_millis, outcome
         FROM audit_events ORDER BY occurred_at_millis",
    )?
    .query_map([], |row| {
        Ok(AuditEvent {
            id: row.get(0)?,
            actor: row.get(1)?,
            kind: row.get(2)?,
            item_id: row.get(3)?,
            occurred_at_millis: row.get(4)?,
            outcome: row.get(5)?,
        })
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;

    #[test]
    fn append_and_list_round_trips() {
        let conn = open_in_memory().unwrap();
        append(&conn, "peer-1", "transfer.offered", Some("item-1"), "accepted").unwrap();

        let events = list(&conn).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].actor, "peer-1");
        assert_eq!(events[0].kind, "transfer.offered");
        assert_eq!(events[0].item_id.as_deref(), Some("item-1"));
    }

    #[test]
    fn update_is_rejected_by_the_append_only_trigger() {
        let conn = open_in_memory().unwrap();
        let id = append(&conn, "peer-1", "transfer.offered", None, "accepted").unwrap();

        let result = conn.execute(
            "UPDATE audit_events SET outcome = 'tampered' WHERE id = ?1",
            params![id],
        );
        assert!(result.is_err());

        let events = list(&conn).unwrap();
        assert_eq!(events[0].outcome, "accepted");
    }

    #[test]
    fn delete_is_rejected_by_the_append_only_trigger() {
        let conn = open_in_memory().unwrap();
        let id = append(&conn, "peer-1", "transfer.offered", None, "accepted").unwrap();

        let result = conn.execute("DELETE FROM audit_events WHERE id = ?1", params![id]);
        assert!(result.is_err());
        assert_eq!(list(&conn).unwrap().len(), 1);
    }
}
