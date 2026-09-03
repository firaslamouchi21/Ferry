use rusqlite::{params, Connection, OptionalExtension};

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecord {
    pub peer_id: String,
    pub display_name: String,
    pub signing_key: String,
    pub sealing_key: String,
    pub paired_at_millis: i64,
}

pub fn insert_peer(
    conn: &Connection,
    peer_id: &str,
    display_name: &str,
    signing_key: &str,
    sealing_key: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO roster_entries (peer_id, display_name, signing_key, sealing_key, paired_at_millis)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![peer_id, display_name, signing_key, sealing_key, now_millis()],
    )?;
    Ok(())
}

pub fn get_peer(conn: &Connection, peer_id: &str) -> rusqlite::Result<Option<PeerRecord>> {
    conn.query_row(
        "SELECT peer_id, display_name, signing_key, sealing_key, paired_at_millis
         FROM roster_entries WHERE peer_id = ?1",
        params![peer_id],
        row_to_peer,
    )
    .optional()
}

pub fn list_peers(conn: &Connection) -> rusqlite::Result<Vec<PeerRecord>> {
    conn.prepare(
        "SELECT peer_id, display_name, signing_key, sealing_key, paired_at_millis
         FROM roster_entries ORDER BY paired_at_millis",
    )?
    .query_map([], row_to_peer)?
    .collect()
}

pub fn remove_peer(conn: &Connection, peer_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM roster_entries WHERE peer_id = ?1",
        params![peer_id],
    )?;
    Ok(())
}

fn row_to_peer(row: &rusqlite::Row) -> rusqlite::Result<PeerRecord> {
    Ok(PeerRecord {
        peer_id: row.get(0)?,
        display_name: row.get(1)?,
        signing_key: row.get(2)?,
        sealing_key: row.get(3)?,
        paired_at_millis: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;

    #[test]
    fn insert_and_fetch_round_trips() {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "laptop", "sk1", "xk1").unwrap();

        let fetched = get_peer(&conn, "peer-1").unwrap().unwrap();
        assert_eq!(fetched.display_name, "laptop");
        assert_eq!(fetched.signing_key, "sk1");
    }

    #[test]
    fn missing_peer_returns_none() {
        let conn = open_in_memory().unwrap();
        assert!(get_peer(&conn, "nonexistent").unwrap().is_none());
    }

    #[test]
    fn list_returns_all_peers_in_pairing_order() {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "a", "sk1", "xk1").unwrap();
        insert_peer(&conn, "peer-2", "b", "sk2", "xk2").unwrap();

        let peers = list_peers(&conn).unwrap();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].peer_id, "peer-1");
        assert_eq!(peers[1].peer_id, "peer-2");
    }

    #[test]
    fn remove_peer_deletes_it() {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "a", "sk1", "xk1").unwrap();
        remove_peer(&conn, "peer-1").unwrap();
        assert!(get_peer(&conn, "peer-1").unwrap().is_none());
    }
}
