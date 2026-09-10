use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::time::now_millis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteJob {
    pub id: String,
    pub kind: String,
    pub params: String,
    pub state: String,
    pub created_at_millis: i64,
    pub finished_at_millis: Option<i64>,
    pub result: Option<String>,
    pub error: Option<String>,
}

pub fn enqueue(conn: &Connection, kind: &str, params: &str) -> rusqlite::Result<String> {
    let id = Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO remote_jobs (id, kind, params, state, created_at_millis)
         VALUES (?1, ?2, ?3, 'queued', ?4)",
        params![id, kind, params, now_millis()],
    )?;
    Ok(id)
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<RemoteJob>> {
    conn.query_row(
        "SELECT id, kind, params, state, created_at_millis, finished_at_millis, result, error
         FROM remote_jobs WHERE id = ?1",
        params![id],
        row_to_job,
    )
    .optional()
}

pub fn claim_next(conn: &Connection) -> rusqlite::Result<Option<RemoteJob>> {
    let job = conn
        .query_row(
            "SELECT id, kind, params, state, created_at_millis, finished_at_millis, result, error
             FROM remote_jobs WHERE state = 'queued' ORDER BY created_at_millis LIMIT 1",
            [],
            row_to_job,
        )
        .optional()?;
    if let Some(job) = &job {
        conn.execute(
            "UPDATE remote_jobs SET state = 'running' WHERE id = ?1",
            params![job.id],
        )?;
    }
    Ok(job)
}

pub fn finish(conn: &Connection, id: &str, result: Option<&str>, error: Option<&str>) -> rusqlite::Result<()> {
    let state = if error.is_some() { "failed" } else { "done" };
    conn.execute(
        "UPDATE remote_jobs SET state = ?2, finished_at_millis = ?3, result = ?4, error = ?5 WHERE id = ?1",
        params![id, state, now_millis(), result, error],
    )?;
    Ok(())
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<RemoteJob> {
    Ok(RemoteJob {
        id: row.get(0)?,
        kind: row.get(1)?,
        params: row.get(2)?,
        state: row.get(3)?,
        created_at_millis: row.get(4)?,
        finished_at_millis: row.get(5)?,
        result: row.get(6)?,
        error: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;

    #[test]
    fn enqueue_claim_finish_round_trip() {
        let conn = open_in_memory().unwrap();
        let id = enqueue(&conn, "gist_publish", "{\"item_id\":\"IT_1\"}").unwrap();
        assert_eq!(get(&conn, &id).unwrap().unwrap().state, "queued");

        let claimed = claim_next(&conn).unwrap().unwrap();
        assert_eq!(claimed.id, id);
        assert_eq!(get(&conn, &id).unwrap().unwrap().state, "running");
        assert!(claim_next(&conn).unwrap().is_none(), "a running job is not re-claimed");

        finish(&conn, &id, Some("{\"url\":\"x\"}"), None).unwrap();
        let done = get(&conn, &id).unwrap().unwrap();
        assert_eq!(done.state, "done");
        assert_eq!(done.result.as_deref(), Some("{\"url\":\"x\"}"));
        assert!(done.finished_at_millis.is_some());
    }

    #[test]
    fn a_failed_job_records_the_error_and_no_result() {
        let conn = open_in_memory().unwrap();
        let id = enqueue(&conn, "roster_apply", "{}").unwrap();
        claim_next(&conn).unwrap();
        finish(&conn, &id, None, Some("network unreachable")).unwrap();
        let job = get(&conn, &id).unwrap().unwrap();
        assert_eq!(job.state, "failed");
        assert_eq!(job.error.as_deref(), Some("network unreachable"));
        assert!(job.result.is_none());
    }

    #[test]
    fn claim_order_is_oldest_first() {
        let conn = open_in_memory().unwrap();
        let a = enqueue(&conn, "k", "1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _b = enqueue(&conn, "k", "2").unwrap();
        assert_eq!(claim_next(&conn).unwrap().unwrap().id, a);
    }
}
