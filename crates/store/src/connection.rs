use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::migrate::{apply_migrations, MigrateError};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("failed to open database: {0}")]
    Open(#[from] rusqlite::Error),
    #[error("failed to apply migrations: {0}")]
    Migrate(#[from] MigrateError),
}

pub fn open(path: &Path) -> Result<Connection, OpenError> {
    let mut conn = Connection::open(path)?;
    configure(&mut conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection, OpenError> {
    let mut conn = Connection::open_in_memory()?;
    configure(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &mut Connection) -> Result<(), OpenError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    apply_migrations(conn)?;
    Ok(())
}
