use rusqlite::{params, Connection};

use crate::time::now_millis;

pub fn queue(conn: &Connection, item_id: &str, peer_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO open_receipts (item_id, peer_id, queued_at_millis) VALUES (?1, ?2, ?3)",
        params![item_id, peer_id, now_millis()],
    )?;
    Ok(())
}

pub fn list_for_peer(conn: &Connection, peer_id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT item_id FROM open_receipts WHERE peer_id = ?1 ORDER BY queued_at_millis")?
        .query_map(params![peer_id], |row| row.get(0))?
        .collect()
}

pub fn remove(conn: &Connection, item_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM open_receipts WHERE item_id = ?1", params![item_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;
    use crate::inbox::{insert_with_id, NewInboxItem};
    use crate::roster::insert_peer;
    use ferry_proto::states::ItemKind;

    fn conn_with_item(item_id: &str) -> Connection {
        let conn = open_in_memory().unwrap();
        insert_peer(&conn, "peer-1", "laptop", "sk", "xk").unwrap();
        insert_with_id(
            &conn,
            item_id,
            &NewInboxItem {
                peer_id: "peer-1".into(),
                kind: ItemKind::File,
                name: "n".into(),
                size_bytes: 1,
                hash: "h".into(),
                is_burn_after_read: false,
                notify_on_open: true,
            },
        )
        .unwrap();
        conn
    }

    #[test]
    fn queue_list_and_remove_round_trip() {
        let conn = conn_with_item("item-1");
        queue(&conn, "item-1", "peer-1").unwrap();
        queue(&conn, "item-1", "peer-1").unwrap();

        assert_eq!(list_for_peer(&conn, "peer-1").unwrap(), vec!["item-1".to_string()]);
        assert!(list_for_peer(&conn, "other").unwrap().is_empty());

        remove(&conn, "item-1").unwrap();
        assert!(list_for_peer(&conn, "peer-1").unwrap().is_empty());
    }
}
