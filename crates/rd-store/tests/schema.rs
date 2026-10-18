//! A database created here must be indistinguishable from one the Swift app created.

use std::path::PathBuf;

use rd_store::{Database, MIGRATION_IDS};
use rusqlite::Connection;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn schema(conn: &Connection) -> Vec<(String, String, String, Option<String>)> {
    conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn migrations(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT identifier FROM grdb_migrations ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn new_database_matches_swift_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/connections.sqlite");
    drop(Database::open(&path).unwrap());

    let ours = Connection::open(&path).unwrap();
    let swift = Connection::open(fixture("swift-v13-empty.sqlite")).unwrap();
    assert_eq!(schema(&ours), schema(&swift));
    assert_eq!(migrations(&ours), migrations(&swift));
    assert_eq!(migrations(&ours), MIGRATION_IDS);
}

#[test]
fn file_database_uses_rollback_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    let db = Database::open(&path).unwrap();
    assert_eq!(db.path(), Some(path.as_path()));
    drop(db);
    let mode: String = Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    assert!(!dir.path().join("connections.sqlite-wal").exists());
}

#[test]
fn opening_a_swift_database_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    std::fs::copy(fixture("swift-v13.sqlite"), &path).unwrap();
    let before = schema(&Connection::open(&path).unwrap());
    drop(Database::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    assert_eq!(schema(&conn), before);
    assert_eq!(migrations(&conn), MIGRATION_IDS);
    assert!(Database::open_in_memory().unwrap().path().is_none());
}
