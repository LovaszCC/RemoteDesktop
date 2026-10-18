//! The Swift app's GRDB migrations, replayed with the same ids, SQL text and bookkeeping.
//!
//! SQLite keeps each `CREATE` statement as written and appends `ADD COLUMN` definitions verbatim, so the
//! statements here are exactly what GRDB generated. A schema-dump test compares the result with a database
//! the Swift app created.

use rusqlite::Connection;

use crate::StoreError;

struct Migration {
    id: &'static str,
    sql: &'static str,
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
    },
    Migration {
        id: "v2-credentials",
        sql: r#"
CREATE TABLE "credential" ("id" TEXT PRIMARY KEY NOT NULL, "name" TEXT NOT NULL, "username" TEXT NOT NULL DEFAULT '', "domain" TEXT NOT NULL DEFAULT '', "passwordEnc" BLOB, "sshKeyPath" TEXT, "sshKeyPassphraseEnc" BLOB, "notes" TEXT NOT NULL DEFAULT '', "createdAt" DATETIME NOT NULL, "updatedAt" DATETIME NOT NULL);
ALTER TABLE "connection" ADD COLUMN "credentialId" TEXT REFERENCES "credential"("id") ON DELETE SET NULL;
CREATE INDEX "connection_on_credentialId" ON "connection"("credentialId");
"#,
    },
    Migration {
        id: "v3-rdp-admin",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "rdpAdminSession" BOOLEAN NOT NULL DEFAULT 0;"#,
    },
    Migration {
        id: "v4-unlocked-key",
        sql: r#"ALTER TABLE "credential" ADD COLUMN "sshKeyUnlockedEnc" BLOB;"#,
    },
    Migration {
        id: "v5-web",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "browserBundleId" TEXT;"#,
    },
    Migration {
        id: "v6-mikrotik",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "mikrotikMode" TEXT NOT NULL DEFAULT 'winbox';"#,
    },
    Migration {
        id: "v7-ssh-terminal",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "sshTerminal" TEXT NOT NULL DEFAULT 'embedded';"#,
    },
    Migration {
        id: "v8-rdp-client",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "rdpClient" TEXT NOT NULL DEFAULT 'embedded';"#,
    },
    Migration {
        id: "v9-port-forwards",
        sql: r#"
CREATE TABLE "portForward" ("id" TEXT PRIMARY KEY NOT NULL, "connectionId" TEXT NOT NULL REFERENCES "connection"("id") ON DELETE CASCADE, "name" TEXT NOT NULL DEFAULT '', "localPort" INTEGER NOT NULL, "remoteHost" TEXT NOT NULL, "remotePort" INTEGER NOT NULL, "createdAt" DATETIME NOT NULL);
CREATE INDEX "portForward_on_connectionId" ON "portForward"("connectionId");
"#,
    },
    Migration {
        // Which external terminal became a global setting; Ghostty connections keep opening externally.
        id: "v10-external-terminal",
        sql: "UPDATE connection SET sshTerminal = 'external' WHERE sshTerminal = 'ghostty';",
    },
    Migration {
        id: "v11-folder-color",
        sql: r#"ALTER TABLE "folder" ADD COLUMN "color" TEXT;"#,
    },
    Migration {
        id: "v12-connection-color",
        sql: r#"ALTER TABLE "connection" ADD COLUMN "color" TEXT;"#,
    },
    Migration {
        id: "v13-saved-split",
        sql: r#"CREATE TABLE "savedSplit" ("id" TEXT PRIMARY KEY NOT NULL, "name" TEXT NOT NULL, "connectionIds" TEXT NOT NULL, "sortOrder" INTEGER NOT NULL DEFAULT 0, "createdAt" DATETIME NOT NULL);"#,
    },
];

/// Migration ids in the order they apply, identical to the Swift app's.
pub const MIGRATION_IDS: [&str; 13] = [
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
];

/// Applies every migration that isn't recorded yet. Ids recorded by another build but unknown here are
/// left alone, as GRDB does.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    migrate_up_to(conn, MIGRATION_IDS[MIGRATION_IDS.len() - 1])
}

/// Applies migrations up to and including `last`, the way GRDB's `DatabaseMigrator` does: each one in
/// its own immediate transaction with foreign keys off, recorded in `grdb_migrations`, and checked with
/// `PRAGMA foreign_key_check` before commit.
pub(crate) fn migrate_up_to(conn: &mut Connection, last: &str) -> Result<(), StoreError> {
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
        assert_eq!(count, 14);
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
