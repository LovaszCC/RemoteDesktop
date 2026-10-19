//! The Swift app's GRDB migrations, replayed with the same ids, SQL text and bookkeeping.
//!
//! SQLite keeps each `CREATE` statement as written and appends `ADD COLUMN` definitions verbatim, so the
//! statements here are exactly what GRDB generated. A schema-dump test compares the result with a database
//! the Swift app created. Migrations after `v13-saved-split` are this app's own; the database version guard
//! keeps older builds from opening a file that has them.

use rusqlite::{Connection, Transaction, params};

use rd_core::draft::split_stored_web_url;

use crate::StoreError;

/// A data step that runs after a migration's SQL, inside the same transaction.
type PostStep = fn(&Transaction<'_>) -> Result<(), StoreError>;

struct Migration {
    id: &'static str,
    sql: &'static str,
    post: Option<PostStep>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "v1",
        sql: r#"
CREATE TABLE "meta" ("key" TEXT PRIMARY KEY NOT NULL, "value" BLOB NOT NULL);
CREATE TABLE "folder" ("id" TEXT PRIMARY KEY NOT NULL, "parentId" TEXT REFERENCES "folder"("id") ON DELETE CASCADE, "name" TEXT NOT NULL, "sortOrder" INTEGER NOT NULL DEFAULT 0);
CREATE INDEX "folder_on_parentId" ON "folder"("parentId");
CREATE TABLE "connection" ("id" TEXT PRIMARY KEY NOT NULL, "folderId" TEXT REFERENCES "folder"("id") ON DELETE SET NULL, "type" TEXT NOT NULL, "name" TEXT NOT NULL, "host" TEXT NOT NULL, "port" INTEGER, "username" TEXT NOT NULL DEFAULT '', "domain" TEXT NOT NULL DEFAULT '', "passwordEnc" BLOB, "sshKeyPath" TEXT, "sshKeyPassphraseEnc" BLOB, "isFavorite" BOOLEAN NOT NULL DEFAULT 0, "notes" TEXT NOT NULL DEFAULT '', "createdAt" DATETIME NOT NULL, "updatedAt" DATETIME NOT NULL);
CREATE INDEX "connection_on_folderId" ON "connection"("folderId");
"#,
        post: None,
    },
    Migration {
        id: "v2-credentials",
        sql: r#"
CREATE TABLE "credential" ("id" TEXT PRIMARY KEY NOT NULL, "name" TEXT NOT NULL, "username" TEXT NOT NULL DEFAULT '', "domain" TEXT NOT NULL DEFAULT '', "passwordEnc" BLOB, "sshKeyPath" TEXT, "sshKeyPassphraseEnc" BLOB, "notes" TEXT NOT NULL DEFAULT '', "createdAt" DATETIME NOT NULL, "updatedAt" DATETIME NOT NULL);
ALTER TABLE "connection" ADD COLUMN "credentialId" TEXT REFERENCES "credential"("id") ON DELETE SET NULL;
CREATE INDEX "connection_on_credentialId" ON "connection"("credentialId");
"#,
        post: None,
    },
    Migration {
        id: "v3-rdp-admin",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "rdpAdminSession" BOOLEAN NOT NULL DEFAULT 0;"#,
        post: None,
    },
    Migration {
        id: "v4-unlocked-key",
        sql: r#"ALTER TABLE "credential" ADD COLUMN "sshKeyUnlockedEnc" BLOB;"#,
        post: None,
    },
    Migration {
        id: "v5-web",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "browserBundleId" TEXT;"#,
        post: None,
    },
    Migration {
        id: "v6-mikrotik",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "mikrotikMode" TEXT NOT NULL DEFAULT 'winbox';"#,
        post: None,
    },
    Migration {
        id: "v7-ssh-terminal",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "sshTerminal" TEXT NOT NULL DEFAULT 'embedded';"#,
        post: None,
    },
    Migration {
        id: "v8-rdp-client",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "rdpClient" TEXT NOT NULL DEFAULT 'embedded';"#,
        post: None,
    },
    Migration {
        id: "v9-port-forwards",
        sql: r#"
CREATE TABLE "portForward" ("id" TEXT PRIMARY KEY NOT NULL, "connectionId" TEXT NOT NULL REFERENCES "connection"("id") ON DELETE CASCADE, "name" TEXT NOT NULL DEFAULT '', "localPort" INTEGER NOT NULL, "remoteHost" TEXT NOT NULL, "remotePort" INTEGER NOT NULL, "createdAt" DATETIME NOT NULL);
CREATE INDEX "portForward_on_connectionId" ON "portForward"("connectionId");
"#,
        post: None,
    },
    Migration {
        // Which external terminal became a global setting; Ghostty connections keep opening externally.
        id: "v10-external-terminal",
        sql: "UPDATE connection SET sshTerminal = 'external' WHERE sshTerminal = 'ghostty';",
        post: None,
    },
    Migration {
        id: "v11-folder-color",
        sql: r#"ALTER TABLE "folder" ADD COLUMN "color" TEXT;"#,
        post: None,
    },
    Migration {
        id: "v12-connection-color",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "color" TEXT;"#,
        post: None,
    },
    Migration {
        id: "v13-saved-split",
        sql: r#"CREATE TABLE "savedSplit" ("id" TEXT PRIMARY KEY NOT NULL, "name" TEXT NOT NULL, "connectionIds" TEXT NOT NULL, "sortOrder" INTEGER NOT NULL DEFAULT 0, "createdAt" DATETIME NOT NULL);"#,
        post: None,
    },
    Migration {
        // Every connection becomes an entry with that one connection, under the same id.
        id: "v14-entries",
        sql: r#"
CREATE TABLE "entry" ("id" TEXT PRIMARY KEY NOT NULL, "folderId" TEXT REFERENCES "folder"("id") ON DELETE SET NULL, "name" TEXT NOT NULL, "host" TEXT NOT NULL, "notes" TEXT NOT NULL DEFAULT '', "color" TEXT, "isFavorite" BOOLEAN NOT NULL DEFAULT 0, "defaultConnectionId" TEXT, "createdAt" DATETIME NOT NULL, "updatedAt" DATETIME NOT NULL);
CREATE INDEX "entry_on_folderId" ON "entry"("folderId");
ALTER TABLE "connection" ADD COLUMN "entryId" TEXT REFERENCES "entry"("id") ON DELETE CASCADE;
ALTER TABLE "connection" ADD COLUMN "label" TEXT NOT NULL DEFAULT '';
ALTER TABLE "connection" ADD COLUMN "showAsButton" BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE "connection" ADD COLUMN "sortOrder" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "connection" ADD COLUMN "webScheme" TEXT NOT NULL DEFAULT 'https';
ALTER TABLE "connection" ADD COLUMN "webPath" TEXT NOT NULL DEFAULT '';
CREATE INDEX "connection_on_entryId" ON "connection"("entryId");
INSERT INTO entry SELECT id, folderId, name, host, notes, color, isFavorite, id, createdAt, updatedAt FROM connection;
UPDATE connection SET entryId = id;
"#,
        post: Some(split_web_urls),
    },
];

/// Rewrites each web connection's URL into the entry host and the connection's port, scheme and path.
fn split_web_urls(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let rows: Vec<(String, String, Option<i64>)> = tx
        .prepare("SELECT id, host, port FROM connection WHERE type = 'web'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, host, port) in rows {
        // The stored value was the whole URL; one that doesn't parse stays the host as typed.
        let (scheme, host, port, path) = match split_stored_web_url(&host) {
            Some(parts) => (parts.scheme, parts.host, parts.port.map(i64::from), parts.path),
            None => ("https".to_owned(), host, port, String::new()),
        };
        tx.execute(
            "UPDATE connection SET host = ?1, port = ?2, webScheme = ?3, webPath = ?4 WHERE id = ?5",
            params![host, port, scheme, path, id],
        )?;
        tx.execute(
            "UPDATE entry SET host = ?1 WHERE id = (SELECT entryId FROM connection WHERE id = ?2)",
            params![host, id],
        )?;
    }
    Ok(())
}

/// Migration ids in the order they apply: the Swift app's, then this app's own.
pub const MIGRATION_IDS: [&str; 14] = [
    "v1",
    "v2-credentials",
    "v3-rdp-admin",
    "v4-unlocked-key",
    "v5-web",
    "v6-mikrotik",
    "v7-ssh-terminal",
    "v8-rdp-client",
    "v9-port-forwards",
    "v10-external-terminal",
    "v11-folder-color",
    "v12-connection-color",
    "v13-saved-split",
    "v14-entries",
];

/// The known migration ids that `conn` hasn't recorded, in order; all of them for a database without
/// the bookkeeping table. Reads only.
pub(crate) fn pending(conn: &Connection) -> Result<Vec<&'static str>, StoreError> {
    let has_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'grdb_migrations')",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(MIGRATION_IDS.to_vec());
    }
    let applied: Vec<String> = conn
        .prepare("SELECT identifier FROM grdb_migrations")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(MIGRATION_IDS
        .into_iter()
        .filter(|id| !applied.iter().any(|a| a == id))
        .collect())
}

/// Applies every migration that isn't recorded yet. Ids recorded by another build but unknown here are
/// left alone, as GRDB does.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    migrate_up_to(conn, MIGRATION_IDS[MIGRATION_IDS.len() - 1])
}

/// Applies migrations up to and including `last`, the way GRDB's `DatabaseMigrator` does: each one in
/// its own immediate transaction with foreign keys off, recorded in `grdb_migrations`, and checked with
/// `PRAGMA foreign_key_check` before commit. A migration's data step runs before that check.
///
/// Public for the schema tests, which compare a database migrated up to `v13-saved-split` with one the
/// Swift app created.
///
/// # Errors
/// If SQLite fails, or a migration leaves a foreign key violation; that migration is rolled back.
#[doc(hidden)]
pub fn migrate_up_to(conn: &mut Connection, last: &str) -> Result<(), StoreError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS grdb_migrations (identifier TEXT NOT NULL PRIMARY KEY)")?;
    let applied: Vec<String> = conn
        .prepare("SELECT identifier FROM grdb_migrations")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    for migration in MIGRATIONS {
        if !applied.iter().any(|id| id == migration.id) {
            apply(conn, migration)?;
        }
        if migration.id == last {
            break;
        }
    }
    Ok(())
}

fn apply(conn: &mut Connection, migration: &Migration) -> Result<(), StoreError> {
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    let result = (|| {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(migration.sql)?;
        if let Some(post) = migration.post {
            post(&tx)?;
        }
        tx.execute(
            "INSERT INTO grdb_migrations (identifier) VALUES (?1)",
            [migration.id],
        )?;
        let violations: bool = tx.prepare("PRAGMA foreign_key_check")?.exists([])?;
        if violations {
            return Err(StoreError::ForeignKeyViolation { id: migration.id });
        }
        tx.commit()?;
        Ok(())
    })();
    conn.execute_batch("PRAGMA foreign_keys = ON")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_migration_list() {
        assert_eq!(MIGRATIONS.iter().map(|m| m.id).collect::<Vec<_>>(), MIGRATION_IDS);
    }

    #[test]
    fn v1_rows_survive_the_upgrade() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_up_to(&mut conn, "v1").unwrap();
        conn.execute(
            "INSERT INTO connection (id, type, name, host, createdAt, updatedAt) \
             VALUES ('A', 'rdp', 'Old', 'old.example.com', '2025-01-01 00:00:00', '2025-01-01 00:00:00')",
            [],
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        let (credential, mode, terminal): (Option<String>, String, String) = conn
            .query_row(
                "SELECT credentialId, mikrotikMode, sshTerminal FROM connection WHERE id = 'A'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (credential, mode.as_str(), terminal.as_str()),
            (None, "winbox", "embedded")
        );
    }

    #[test]
    fn ghostty_connections_become_external() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_up_to(&mut conn, "v9-port-forwards").unwrap();
        conn.execute(
            "INSERT INTO connection (id, type, name, host, createdAt, updatedAt, sshTerminal) \
             VALUES ('S', 'ssh', 's', 'h', '2025-01-01 00:00:00', '2025-01-01 00:00:00', 'ghostty')",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let terminal: String = conn
            .query_row("SELECT sshTerminal FROM connection WHERE id = 'S'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(terminal, "external");
    }

    #[test]
    fn unknown_applied_ids_are_tolerated_and_reruns_are_noops() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO grdb_migrations (identifier) VALUES ('v99-future')",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM grdb_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 15);
    }

    /// id, folderId, name, host, notes, color, isFavorite, defaultConnectionId, createdAt, updatedAt.
    type EntryRow = (
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        bool,
        Option<String>,
        String,
        String,
    );

    /// id, host, port, webScheme, webPath, entry host.
    type WebRow = (String, String, Option<i64>, String, String, String);

    #[test]
    fn v14_splits_web_urls() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_up_to(&mut conn, "v13-saved-split").unwrap();
        for (id, kind, host, port) in [
            ("A", "web", "https://192.168.1.10:8443/admin?x=1", None),
            ("B", "web", "smb://homeserver.local/share", None),
            ("C", "web", "router.example.com", None),
            ("D", "web", "not a url", Some(99)),
            ("E", "ssh", "https://kept.example.com/x", None),
            ("F", "web", "https://user:pass@host.example.com/", None),
        ] {
            conn.execute(
                "INSERT INTO connection (id, type, name, host, port, createdAt, updatedAt) \
                 VALUES (?1, ?2, 'n', ?3, ?4, '2025-01-01 00:00:00.000', '2025-01-01 00:00:00.000')",
                params![id, kind, host, port],
            )
            .unwrap();
        }
        migrate(&mut conn).unwrap();
        let rows: Vec<WebRow> = conn
            .prepare(
                "SELECT c.id, c.host, c.port, c.webScheme, c.webPath, e.host \
                 FROM connection c JOIN entry e ON e.id = c.entryId ORDER BY c.id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let text = |s: &str| s.to_owned();
        assert_eq!(
            rows,
            vec![
                (
                    text("A"),
                    text("192.168.1.10"),
                    Some(8443),
                    text("https"),
                    text("/admin?x=1"),
                    text("192.168.1.10")
                ),
                (
                    text("B"),
                    text("homeserver.local"),
                    None,
                    text("smb"),
                    text("/share"),
                    text("homeserver.local")
                ),
                (
                    text("C"),
                    text("router.example.com"),
                    None,
                    text("https"),
                    String::new(),
                    text("router.example.com")
                ),
                (
                    text("D"),
                    text("not a url"),
                    Some(99),
                    text("https"),
                    String::new(),
                    text("not a url")
                ),
                (
                    text("E"),
                    text("https://kept.example.com/x"),
                    None,
                    text("https"),
                    String::new(),
                    text("https://kept.example.com/x")
                ),
                (
                    text("F"),
                    text("user:pass@host.example.com"),
                    None,
                    text("https"),
                    text("/"),
                    text("user:pass@host.example.com")
                ),
            ]
        );
    }

    #[test]
    fn v14_creates_one_entry_per_connection() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_up_to(&mut conn, "v13-saved-split").unwrap();
        conn.execute_batch(
            "INSERT INTO folder (id, name) VALUES ('F', 'Clients');
             INSERT INTO connection (id, folderId, type, name, host, notes, color, createdAt, updatedAt) \
             VALUES ('A', 'F', 'ssh', 'Alpha', 'a.example.com', 'note', 'blue', \
             '2025-01-01 00:00:00.000', '2025-01-02 00:00:00.000');
             INSERT INTO connection (id, type, name, host, isFavorite, createdAt, updatedAt) \
             VALUES ('B', 'rdp', 'Beta', 'b.example.com', 1, '2025-02-01 00:00:00.000', '2025-02-02 00:00:00.000');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        let entries: Vec<EntryRow> = conn
            .prepare(
                "SELECT id, folderId, name, host, notes, color, isFavorite, defaultConnectionId, createdAt, updatedAt \
                 FROM entry ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let text = |s: &str| s.to_owned();
        assert_eq!(
            entries,
            vec![
                (
                    text("A"),
                    Some(text("F")),
                    text("Alpha"),
                    text("a.example.com"),
                    text("note"),
                    Some(text("blue")),
                    false,
                    Some(text("A")),
                    text("2025-01-01 00:00:00.000"),
                    text("2025-01-02 00:00:00.000"),
                ),
                (
                    text("B"),
                    None,
                    text("Beta"),
                    text("b.example.com"),
                    String::new(),
                    None,
                    true,
                    Some(text("B")),
                    text("2025-02-01 00:00:00.000"),
                    text("2025-02-02 00:00:00.000"),
                ),
            ]
        );
        let connections: Vec<(String, String, String, bool, i64, String, String)> = conn
            .prepare(
                "SELECT id, entryId, label, showAsButton, sortOrder, webScheme, webPath FROM connection ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            connections,
            vec![
                (
                    text("A"),
                    text("A"),
                    String::new(),
                    true,
                    0,
                    text("https"),
                    String::new()
                ),
                (
                    text("B"),
                    text("B"),
                    String::new(),
                    true,
                    0,
                    text("https"),
                    String::new()
                ),
            ]
        );
    }

    #[test]
    fn foreign_key_violations_abort_the_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_up_to(&mut conn, "v8-rdp-client").unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        conn.execute(
            "INSERT INTO connection (id, type, name, host, createdAt, updatedAt, folderId) \
             VALUES ('X', 'ssh', 'x', 'h', '2025-01-01 00:00:00', '2025-01-01 00:00:00', 'missing')",
            [],
        )
        .unwrap();
        let error = migrate_up_to(&mut conn, "v9-port-forwards").unwrap_err();
        assert!(matches!(
            error,
            StoreError::ForeignKeyViolation {
                id: "v9-port-forwards"
            }
        ));
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM grdb_migrations WHERE identifier = 'v9-port-forwards'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(applied, 0);
    }
}
