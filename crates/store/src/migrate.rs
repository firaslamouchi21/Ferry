use rusqlite::Connection;
use thiserror::Error;

use crate::time::now_millis;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_initial",
        sql: include_str!("migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "0002_outbound_items",
        sql: include_str!("migrations/0002_outbound_items.sql"),
    },
    Migration {
        version: 3,
        name: "0003_transfer_engine",
        sql: include_str!("migrations/0003_transfer_engine.sql"),
    },
    Migration {
        version: 4,
        name: "0004_expiry_session",
        sql: include_str!("migrations/0004_expiry_session.sql"),
    },
    Migration {
        version: 5,
        name: "0005_outbox_expiry",
        sql: include_str!("migrations/0005_outbox_expiry.sql"),
    },
    Migration {
        version: 6,
        name: "0006_sealed_secrets",
        sql: include_str!("migrations/0006_sealed_secrets.sql"),
    },
    Migration {
        version: 7,
        name: "0007_outbound_source_path",
        sql: include_str!("migrations/0007_outbound_source_path.sql"),
    },
    Migration {
        version: 8,
        name: "0008_open_receipts",
        sql: include_str!("migrations/0008_open_receipts.sql"),
    },
];

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("migration {name} (version {version}) failed: {source}")]
    Apply {
        version: i64,
        name: &'static str,
        source: rusqlite::Error,
    },
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

pub fn apply_migrations(conn: &mut Connection) -> Result<(), MigrateError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_millis INTEGER NOT NULL
        )",
    )?;

    let applied: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for migration in MIGRATIONS.iter().filter(|m| m.version > applied) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)
            .map_err(|source| MigrateError::Apply {
                version: migration.version,
                name: migration.name,
                source,
            })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_millis) VALUES (?1, ?2, ?3)",
            rusqlite::params![migration.version, migration.name, now_millis()],
        )?;
        tx.commit()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_cleanly_to_a_fresh_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(tables.contains(&"roster_entries".to_string()));
        assert!(tables.contains(&"outbox_entries".to_string()));
        assert!(tables.contains(&"inbox_items".to_string()));
        assert!(tables.contains(&"outbound_items".to_string()));
        assert!(tables.contains(&"audit_events".to_string()));
    }

    #[test]
    fn later_migrations_apply_on_top_of_earlier_ones() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();

        let applied_versions: Vec<i64> = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let expected: Vec<i64> = MIGRATIONS.iter().map(|m| m.version).collect();
        assert_eq!(applied_versions, expected);
    }

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();
        apply_migrations(&mut conn).unwrap();

        let applied_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied_count, MIGRATIONS.len() as i64);
    }
}
