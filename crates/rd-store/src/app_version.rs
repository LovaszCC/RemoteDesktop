//! The database's app version stamp: the newest app version that has opened the file.
//!
//! An app older than the stamp must not open the file, because a newer build may have changed the schema
//! in ways it doesn't know. The stamp is peeked at through a read-only connection, before the lock and
//! before migrating, so a refused open leaves the file untouched.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use semver::Version;

use crate::{Database, StoreError};

/// The `meta` key of the stamp (UTF-8 text, e.g. `1.6.6`).
pub const APP_VERSION_KEY: &str = "app_version";

/// The stamp of the database at `path`, read without creating, migrating or writing anything. `None` for a
/// missing file, a database without a `meta` table or without a stamp.
///
/// # Errors
/// If the file exists but SQLite can't read it.
pub fn stored_app_version(path: &Path) -> Result<Option<String>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
        [],
        |r| r.get(0),
    )?;
    if !has_meta {
        return Ok(None);
    }
    let value: Option<Vec<u8>> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [APP_VERSION_KEY], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(value.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

/// The migrations the database at `path` still needs, read without creating, migrating or writing
/// anything; every one for a missing file or a file without the bookkeeping table.
///
/// # Errors
/// If the file exists but SQLite can't read it.
pub fn pending_migrations(path: &Path) -> Result<Vec<&'static str>, StoreError> {
    if !path.exists() {
        return Ok(crate::MIGRATION_IDS.to_vec());
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    crate::migrations::pending(&conn)
}

/// Whether `stored` is a newer version than `running` (semantic versioning: `1.6.6-beta.3` < `1.6.6`).
/// Anything that doesn't parse counts as not newer.
#[must_use]
pub fn is_newer(stored: &str, running: &str) -> bool {
    match (Version::parse(stored.trim()), Version::parse(running.trim())) {
        (Ok(stored), Ok(running)) => stored > running,
        _ => false,
    }
}

impl Database {
    /// Raises the stamp to `running` when it is missing, unreadable or older; never lowers it. A running
    /// version that doesn't parse writes nothing.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn record_app_version(&self, running: &str) -> Result<(), StoreError> {
        let Ok(running_version) = Version::parse(running.trim()) else {
            return Ok(());
        };
        let stored = self
            .meta(APP_VERSION_KEY)?
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| Version::parse(text.trim()).ok());
        if stored.is_none_or(|stored| stored < running_version) {
            self.set_meta(APP_VERSION_KEY, running.trim().as_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(db: &Database) -> Option<String> {
        db.meta(APP_VERSION_KEY)
            .unwrap()
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn newer_follows_semantic_versioning() {
        assert!(is_newer("1.6.7", "1.6.6"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("1.6.6", "1.6.6-beta.3"));
        assert!(is_newer("1.6.6-beta.10", "1.6.6-beta.9"));
        assert!(!is_newer("1.6.6", "1.6.6"));
        assert!(!is_newer("1.6.5", "1.6.6"));
        assert!(!is_newer("garbage", "1.6.6"));
        assert!(!is_newer("9.0.0", "dev"));
    }

    #[test]
    fn records_only_upwards() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(stamp(&db), None);
        db.record_app_version("1.6.6").unwrap();
        assert_eq!(stamp(&db).as_deref(), Some("1.6.6"));
        db.record_app_version("1.6.5").unwrap();
        db.record_app_version("1.6.6-beta.2").unwrap();
        assert_eq!(stamp(&db).as_deref(), Some("1.6.6"));
        db.record_app_version("1.7.0").unwrap();
        assert_eq!(stamp(&db).as_deref(), Some("1.7.0"));
        db.record_app_version("not a version").unwrap();
        assert_eq!(stamp(&db).as_deref(), Some("1.7.0"));
        db.set_meta(APP_VERSION_KEY, b"broken").unwrap();
        db.record_app_version("1.6.6").unwrap();
        assert_eq!(stamp(&db).as_deref(), Some("1.6.6"));
    }

    #[test]
    fn peeks_without_creating_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.sqlite");
        assert_eq!(stored_app_version(&missing).unwrap(), None);
        assert!(!missing.exists(), "the peek must not create the file");

        let empty = dir.path().join("empty.sqlite");
        Connection::open(&empty)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert_eq!(stored_app_version(&empty).unwrap(), None);

        let path = dir.path().join("connections.sqlite");
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(stored_app_version(&path).unwrap(), None);
            db.record_app_version("1.6.6").unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert_eq!(stored_app_version(&path).unwrap().as_deref(), Some("1.6.6"));
        assert_eq!(std::fs::read(&path).unwrap(), before, "the peek must not write");

        let garbage = dir.path().join("garbage.sqlite");
        std::fs::write(
            &garbage,
            b"not a database at all, just some bytes that are long enough",
        )
        .unwrap();
        assert!(stored_app_version(&garbage).is_err());
    }

    #[test]
    fn pending_migrations_are_peeked_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.sqlite");
        assert_eq!(
            pending_migrations(&missing).unwrap(),
            crate::MIGRATION_IDS.to_vec()
        );
        assert!(!missing.exists());

        let path = dir.path().join("v13.sqlite");
        {
            let mut conn = Connection::open(&path).unwrap();
            crate::migrate_up_to(&mut conn, "v13-saved-split").unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            pending_migrations(&path).unwrap(),
            vec!["v14-entries", "v15-entry-vpn"]
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "the peek must not write");

        let db = Database::open_unmigrated(&path).unwrap();
        assert_eq!(
            db.pending_migrations().unwrap(),
            vec!["v14-entries", "v15-entry-vpn"]
        );
        db.migrate().unwrap();
        assert!(db.pending_migrations().unwrap().is_empty());
        drop(db);
        assert!(pending_migrations(&path).unwrap().is_empty());
    }
}
