//! Opening the database file with the Swift app's connection settings.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;

use crate::{StoreError, migrations};

/// One open `connections.sqlite`. Access is serialized through a mutex; the app has a single user.
#[derive(Debug)]
pub struct Database {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl Database {
    /// Opens (or creates) the file and migrates it. Rollback journal instead of WAL, so the database stays
    /// a single file that can live in a synced folder.
    ///
    /// # Errors
    /// If the folder can't be created, the file can't be opened, or a migration fails.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::configure(conn, Some(path.to_path_buf()))
    }

    /// A migrated database in memory, for tests and previews.
    ///
    /// # Errors
    /// If SQLite or a migration fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::configure(Connection::open_in_memory()?, None)
    }

    fn configure(mut conn: Connection, path: Option<PathBuf>) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        conn.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
        migrations::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// The file this database was opened from; `None` in memory.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The connection, for the repository and the vault. A poisoned lock is recovered: SQLite rolls back
    /// any transaction a panicking holder left open.
    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
