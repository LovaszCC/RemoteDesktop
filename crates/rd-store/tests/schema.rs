//! A database created here must be indistinguishable from one the Swift app created.

use std::path::PathBuf;

use rd_store::{Database, MIGRATION_IDS, migrate_up_to};
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

/// Blanks the `host` and `port` of the web rows, which the v14 migration rewrites.
fn mask_web_url(rows: &mut [Vec<rusqlite::types::Value>], columns: &[String]) {
    use rusqlite::types::Value;
    let index = |name: &str| columns.iter().position(|c| c == name).unwrap();
    let (kind, host, port) = (index("type"), index("host"), index("port"));
    for row in rows
        .iter_mut()
        .filter(|r| r[kind] == Value::from("web".to_owned()))
    {
        row[host] = Value::Null;
        row[port] = Value::Null;
    }
}

fn migrations(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT identifier FROM grdb_migrations ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!(
        "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
    ))
    .unwrap()
    .query_map([], |r| r.get(0))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// Every row of `table` as text, limited to `columns`, by id (rowid for the migration list).
fn rows(conn: &Connection, table: &str, columns: &[String]) -> Vec<Vec<rusqlite::types::Value>> {
    let order = if table == "grdb_migrations" { "rowid" } else { "1" };
    let list = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut statement = conn
        .prepare(&format!("SELECT {list} FROM \"{table}\" ORDER BY {order}"))
        .unwrap();
    statement
        .query_map([], |r| (0..columns.len()).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn new_database_matches_swift_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    let mut ours = Connection::open(&path).unwrap();
    migrate_up_to(&mut ours, "v13-saved-split").unwrap();

    let swift = Connection::open(fixture("swift-v13-empty.sqlite")).unwrap();
    assert_eq!(schema(&ours), schema(&swift));
    assert_eq!(migrations(&ours), migrations(&swift));
    assert_eq!(migrations(&ours), MIGRATION_IDS[..13]);
}

#[test]
fn v14_adds_entry_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/connections.sqlite");
    drop(Database::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();

    assert_eq!(
        columns(&conn, "entry")[..10],
        [
            "id",
            "folderId",
            "name",
            "host",
            "notes",
            "color",
            "isFavorite",
            "defaultConnectionId",
            "createdAt",
            "updatedAt"
        ]
    );
    let connection = columns(&conn, "connection");
    assert_eq!(
        connection[connection.len() - 6..],
        [
            "entryId",
            "label",
            "showAsButton",
            "sortOrder",
            "webScheme",
            "webPath"
        ]
    );
    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL AND name LIKE '%entry%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(indexes, ["connection_on_entryId", "entry_on_folderId"]);
    assert_eq!(migrations(&conn), MIGRATION_IDS);
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
fn opening_a_swift_database_only_adds_this_apps_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    std::fs::copy(fixture("swift-v13.sqlite"), &path).unwrap();
    let swift = Connection::open(fixture("swift-v13.sqlite")).unwrap();
    drop(Database::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();

    assert_eq!(migrations(&conn), MIGRATION_IDS);
    let tables: Vec<String> = swift
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for table in tables {
        let old = columns(&swift, &table);
        let mut ours = rows(&conn, &table, &old);
        if table == "grdb_migrations" {
            assert_eq!(ours.pop(), Some(vec!["v15-entry-vpn".to_owned().into()]));
            assert_eq!(ours.pop(), Some(vec!["v14-entries".to_owned().into()]));
        }
        let mut theirs = rows(&swift, &table, &old);
        if table == "connection" {
            // Web rows' URL is split into host and port now; everything else is unchanged.
            for set in [&mut ours, &mut theirs] {
                mask_web_url(set, &old);
            }
        }
        assert_eq!(ours, theirs, "table {table}");
        assert!(columns(&conn, &table).starts_with(&old), "table {table}");
    }
    assert!(Database::open_in_memory().unwrap().path().is_none());
}

#[test]
fn v15_adds_entry_vpn_columns_with_defaults() {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate_up_to(&mut conn, "v14-entries").unwrap();
    conn.execute(
        "INSERT INTO entry (id, name, host, createdAt, updatedAt) \
         VALUES ('E', 'Old', 'old.example.com', '2025-01-01 00:00:00', '2025-01-01 00:00:00')",
        [],
    )
    .unwrap();
    migrate_up_to(&mut conn, "v15-entry-vpn").unwrap();

    assert_eq!(
        columns(&conn, "entry")[10..],
        ["vpnConnectionId", "vpnTestMode", "vpnTestAddress"]
    );
    let row: (Option<String>, String, String) = conn
        .query_row(
            "SELECT vpnConnectionId, vpnTestMode, vpnTestAddress FROM entry WHERE id = 'E'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, (None, "host".to_owned(), String::new()));
    assert_eq!(MIGRATION_IDS.last(), Some(&"v15-entry-vpn"));
}
