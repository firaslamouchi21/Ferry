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
    Migration {
        version: 9,
        name: "0009_outbound_last_error",
        sql: include_str!("migrations/0009_outbound_last_error.sql"),
    },
    Migration {
        version: 10,
        name: "0010_remote_jobs",
        sql: include_str!("migrations/0010_remote_jobs.sql"),
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
    apply_migrations_up_to(conn, i64::MAX)
}

pub fn apply_migrations_up_to(conn: &mut Connection, max_version: i64) -> Result<(), MigrateError> {
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

    for migration in MIGRATIONS
        .iter()
        .filter(|m| m.version > applied && m.version <= max_version)
    {
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
    fn a_populated_pre_head_database_migrates_forward_without_losing_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations_up_to(&mut conn, 8).unwrap();

        conn.execute(
            "INSERT INTO roster_entries (peer_id, display_name, signing_key, sealing_key, paired_at_millis)
             VALUES ('peer-1', 'laptop', 'sk', 'xk', 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO outbound_items (id, peer_id, kind, name, state, size_bytes, hash, ttl_secs, is_burn_after_read, created_at_millis, source_path)
             VALUES ('it-1', 'peer-1', 'file', 'notes.txt', 'queued', 10, 'cafe', 600, 0, 2000, '/tmp/notes.txt')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO audit_events (id, actor, kind, item_id, occurred_at_millis, outcome)
             VALUES ('a-1', 'local', 'item.queued', 'it-1', 2000, 'queued')",
            [],
        )
        .unwrap();

        apply_migrations(&mut conn).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().version);

        let (name, last_error): (String, Option<String>) = conn
            .query_row(
                "SELECT name, last_error FROM outbound_items WHERE id = 'it-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "notes.txt");
        assert_eq!(last_error, None, "the new column defaults NULL on rows that predate it");

        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 1, "pre-existing audit rows survive the migration");

        let remote_jobs_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remote_jobs_count, 0, "the new remote_jobs table exists and is empty");
    }

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_v9.sqlite")
    }

    #[test]
    #[ignore = "regenerates the checked-in fixture; run with --ignored after changing the v9 schema"]
    fn regenerate_schema_v9_fixture() {
        let path = fixture_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut conn = Connection::open(&path).unwrap();
        apply_migrations_up_to(&mut conn, 9).unwrap();
        conn.execute(
            "INSERT INTO roster_entries (peer_id, display_name, signing_key, sealing_key, paired_at_millis)
             VALUES ('peer-legacy', 'old-laptop', 'sk', 'xk', 111)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO outbound_items (id, peer_id, kind, name, state, size_bytes, hash, ttl_secs, is_burn_after_read, created_at_millis, source_path, last_error)
             VALUES ('legacy-item', 'peer-legacy', 'file', 'archive.zip', 'queued', 4096, 'beef', 3600, 0, 222, '/tmp/archive.zip', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO audit_events (id, actor, kind, item_id, occurred_at_millis, outcome)
             VALUES ('legacy-audit', 'local', 'item.queued', 'legacy-item', 222, 'queued')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn the_checked_in_pre_v10_database_file_migrates_to_head_with_its_rows_intact() {
        let src = fixture_path();
        assert!(
            src.exists(),
            "missing {} — run `cargo test -p ferry-store -- --ignored regenerate_schema_v9_fixture`",
            src.display()
        );
        let tmp = std::env::temp_dir().join(format!(
            "ferry-migrate-fixture-{}.sqlite",
            std::process::id()
        ));
        std::fs::copy(&src, &tmp).unwrap();
        let mut conn = Connection::open(&tmp).unwrap();

        let start_version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(start_version, 9, "the fixture must be a real v9 database");

        apply_migrations(&mut conn).unwrap();

        let end_version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(end_version, MIGRATIONS.last().unwrap().version);

        let name: String = conn
            .query_row(
                "SELECT name FROM outbound_items WHERE id = 'legacy-item'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "archive.zip");
        let audit_kind: String = conn
            .query_row(
                "SELECT kind FROM audit_events WHERE id = 'legacy-audit'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audit_kind, "item.queued");
        conn.query_row("SELECT COUNT(*) FROM remote_jobs", [], |r| r.get::<_, i64>(0))
            .unwrap();

        drop(conn);
        let _ = std::fs::remove_file(&tmp);
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
