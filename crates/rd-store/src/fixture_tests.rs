//! Round-trips through the database the Swift app wrote (`tests/fixtures/swift-v13.*`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::types::Value;
use secrecy::ExposeSecret;
use serde_json::{Value as Json, json};

use crate::repository::{upsert_connection, upsert_credential};
use crate::{Database, Vault};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn expected() -> Json {
    serde_json::from_slice(&std::fs::read(fixtures().join("swift-v13.expected.json")).unwrap()).unwrap()
}

fn swift_copy() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    std::fs::copy(fixtures().join("swift-v13.sqlite"), &path).unwrap();
    let db = Database::open(&path).unwrap();
    (dir, db)
}

/// Every table as the JSON the Swift generator wrote: rows in a stable order, blobs as `hex:…`.
fn dump(db: &Database) -> BTreeMap<String, Json> {
    let conn = db.conn();
    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let order = match table.as_str() {
                "meta" => "key",
                "grdb_migrations" => "rowid",
                _ => "id",
            };
            let mut statement = conn
                .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY {order}"))
                .unwrap();
            let names: Vec<String> = statement.column_names().into_iter().map(str::to_owned).collect();
            let rows: Vec<Json> = statement
                .query_map([], |row| {
                    let mut object = serde_json::Map::new();
                    for (i, name) in names.iter().enumerate() {
                        let value = match row.get::<_, Value>(i)? {
                            Value::Null => Json::Null,
                            Value::Integer(v) => json!(v),
                            Value::Real(v) => json!(v),
                            Value::Text(v) => json!(v),
                            Value::Blob(v) => json!(format!("hex:{}", hex(&v))),
                        };
                        object.insert(name.clone(), value);
                    }
                    Ok(Json::Object(object))
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            (table, Json::Array(rows))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The v13 rows are unchanged by the upgrade: the Swift columns hold the same values, and the migration
/// list only gains `v14-entries`.
#[test]
fn reads_swift_rows_exactly() {
    let (_dir, db) = swift_copy();
    let expected = expected();
    let ours = dump(&db);
    for (table, rows) in expected["tables"].as_object().unwrap() {
        let rows = rows.as_array().unwrap();
        let mut ours = ours[table].as_array().unwrap().clone();
        if table == "grdb_migrations" {
            assert_eq!(ours.pop(), Some(json!({ "identifier": "v14-entries" })));
        }
        let projected: Vec<Json> = ours
            .iter()
            .zip(rows)
            .map(|(row, swift)| {
                Json::Object(
                    swift
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(|k| (k.clone(), row[k].clone()))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(ours.len(), rows.len(), "table {table}");
        assert_eq!(&projected, rows, "table {table}");
    }
}

#[test]
fn swift_library_upgrades_to_entries() {
    let (_dir, db) = swift_copy();
    let expected = expected();
    let library = db.library().unwrap();
    let swift = expected["tables"]["connection"].as_array().unwrap();
    assert_eq!(library.entries.len(), swift.len());
    assert_eq!(library.connections.len(), swift.len());
    for row in swift {
        let id = row["id"].as_str().unwrap();
        let entry = library.entry(id).unwrap();
        assert_eq!(entry.name, row["name"].as_str().unwrap(), "{id}");
        assert_eq!(entry.host, row["host"].as_str().unwrap(), "{id}");
        assert_eq!(entry.folder_id.as_deref(), row["folderId"].as_str(), "{id}");
        assert_eq!(entry.notes, row["notes"].as_str().unwrap(), "{id}");
        assert_eq!(
            entry.is_favorite,
            row["isFavorite"].as_i64().unwrap() == 1,
            "{id}"
        );
        assert_eq!(entry.default_connection_id.as_deref(), Some(id));
        let connections = library.connections_of(id);
        assert_eq!(connections.len(), 1, "{id}");
        assert_eq!(connections[0].id, id);
        assert_eq!(connections[0].entry_id, id);
        assert!(connections[0].show_as_button);
        assert_eq!(library.default_connection(id).map(|c| c.id.as_str()), Some(id));
    }
    // Splits and forwards still point at existing connections.
    for split in &library.saved_splits {
        for id in &split.connection_ids {
            assert!(library.connections.iter().any(|c| &c.id == id), "split {id}");
        }
    }
    for forward in &library.port_forwards {
        assert!(library.connections.iter().any(|c| c.id == forward.connection_id));
    }
    // And the secrets still open.
    let vault = Vault::unlock(&db, expected["password"].as_str().unwrap()).unwrap();
    let sealed: Vec<_> = library
        .connections
        .iter()
        .filter_map(|c| c.password_enc.clone())
        .collect();
    assert!(!sealed.is_empty());
    for secret in sealed {
        vault.open(&secret).unwrap();
    }
}

#[test]
fn decrypts_every_swift_secret() {
    let (_dir, db) = swift_copy();
    let expected = expected();
    let vault = Vault::unlock(&db, expected["password"].as_str().unwrap()).unwrap();
    let library = db.library().unwrap();
    let secrets = expected["secrets"].as_object().unwrap();
    assert_eq!(secrets.len(), 8);
    for (key, plain) in secrets {
        let mut parts = key.split('.');
        let (table, id, column) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let sealed = match (table, column) {
            ("connection", "passwordEnc") => library
                .connections
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .password_enc
                .clone(),
            ("connection", "sshKeyPassphraseEnc") => library
                .connections
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .ssh_key_passphrase_enc
                .clone(),
            ("credential", "passwordEnc") => library
                .credentials
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .password_enc
                .clone(),
            ("credential", "sshKeyPassphraseEnc") => library
                .credentials
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .ssh_key_passphrase_enc
                .clone(),
            ("credential", "sshKeyUnlockedEnc") => library
                .credentials
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .ssh_key_unlocked_enc
                .clone(),
            other => panic!("unexpected secret column {other:?}"),
        };
        let opened = vault.open(&sealed.unwrap()).unwrap();
        assert_eq!(opened.expose_secret(), plain.as_str().unwrap(), "{key}");
    }
    let empty = library
        .connections
        .iter()
        .find(|c| c.name == "No password")
        .unwrap();
    assert_eq!(empty.password_enc, None);
}

#[test]
fn library_written_back_is_byte_identical() {
    let (_dir, swift) = swift_copy();
    let library = swift.library().unwrap();

    let ours = Database::open_in_memory().unwrap();
    for key in ["kdf_salt", "kdf_iterations", "verifier"] {
        ours.set_meta(key, &swift.meta(key).unwrap().unwrap()).unwrap();
    }
    // Parents before children, as the foreign keys require.
    let mut pending = library.folders.clone();
    while !pending.is_empty() {
        let (ready, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|f| {
            f.parent_id
                .as_ref()
                .is_none_or(|p| ours.library().unwrap().folders.iter().any(|g| &g.id == p))
        });
        for folder in &ready {
            ours.save_folder(folder).unwrap();
        }
        pending = rest;
    }
    for credential in &library.credentials {
        upsert_credential(&ours.conn(), credential).unwrap();
    }
    for connection in &library.connections {
        upsert_connection(&ours.conn(), connection).unwrap();
    }
    for forward in &library.port_forwards {
        ours.save_port_forward(forward).unwrap();
    }
    for split in &library.saved_splits {
        ours.save_saved_split(split).unwrap();
    }

    assert_eq!(dump(&ours), dump(&swift));
    assert_eq!(ours.library().unwrap(), library);
}
