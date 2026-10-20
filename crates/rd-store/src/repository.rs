//! Reading and writing the library: the Swift `ConnectionRepository` API in snake case.

use std::collections::HashMap;

use rd_core::{Connection, Credential, Entry, Folder, Library, PortForward, SavedSplit, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::rows::{
    CONNECTION_COLUMNS, CREDENTIAL_COLUMNS, ENTRY_COLUMNS, FOLDER_COLUMNS, PORT_FORWARD_COLUMNS,
    SAVED_SPLIT_COLUMNS, Text, connection_from_row, credential_from_row, entry_from_row, folder_from_row,
    ids_json, port_forward_from_row, saved_split_from_row, sealed_sql,
};
use crate::{Database, StoreError};

/// What [`Database::write_entry`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryWrite {
    /// The saved entry, with its new `updated_at`.
    pub entry: Entry,
    /// Connections deleted with their port forwards (dropped from the entry, or inside deleted entries).
    pub deleted_connections: Vec<String>,
    /// Entries deleted: emptied by the move of their connections, or asked for.
    pub deleted_entries: Vec<String>,
}

impl Database {
    /// Every record, in the Swift app's sort orders. Each connection carries its entry's shared fields.
    ///
    /// # Errors
    /// If SQLite fails or a row holds a value this build can't read.
    pub fn library(&self) -> Result<Library, StoreError> {
        let conn = self.conn();
        let all = |sql: String| conn.prepare(&sql);
        let entries: Vec<Entry> = all(format!(
            "SELECT {ENTRY_COLUMNS} FROM entry ORDER BY name COLLATE NOCASE"
        ))?
        .query_map([], entry_from_row)?
        .collect::<Result<_, _>>()?;
        let mut connections: Vec<Connection> = all(format!(
            "SELECT {CONNECTION_COLUMNS} FROM connection ORDER BY name COLLATE NOCASE"
        ))?
        .query_map([], connection_from_row)?
        .collect::<Result<_, _>>()?;
        let by_id: HashMap<&str, &Entry> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
        let mut missing: Vec<Entry> = Vec::new();
        for connection in &mut connections {
            if let Some(entry) = by_id.get(connection.entry_id.as_str()) {
                entry.overlay(connection);
            } else if !missing.iter().any(|e| e.id == connection.entry_id) {
                // A row written without an entry (by another app) stands for its own entry.
                missing.push(Entry::for_connection(connection));
            }
        }
        let mut entries = entries;
        entries.extend(missing);
        Ok(Library {
            folders: all(format!(
                "SELECT {FOLDER_COLUMNS} FROM folder ORDER BY sortOrder, name COLLATE NOCASE"
            ))?
            .query_map([], folder_from_row)?
            .collect::<Result<_, _>>()?,
            entries,
            connections,
            credentials: all(format!(
                "SELECT {CREDENTIAL_COLUMNS} FROM credential ORDER BY name COLLATE NOCASE"
            ))?
            .query_map([], credential_from_row)?
            .collect::<Result<_, _>>()?,
            port_forwards: all(format!(
                "SELECT {PORT_FORWARD_COLUMNS} FROM portForward ORDER BY localPort"
            ))?
            .query_map([], port_forward_from_row)?
            .collect::<Result<_, _>>()?,
            saved_splits: all(format!(
                "SELECT {SAVED_SPLIT_COLUMNS} FROM savedSplit ORDER BY sortOrder, createdAt"
            ))?
            .query_map([], saved_split_from_row)?
            .collect::<Result<_, _>>()?,
        })
    }

    /// Runs several writes atomically; the closure's error rolls everything back.
    ///
    /// # Errors
    /// The closure's error, or a SQLite failure.
    pub fn transaction<T>(
        &self,
        work: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let value = work(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    // Folders

    /// Inserts or updates a folder.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown parent).
    pub fn save_folder(&self, folder: &Folder) -> Result<(), StoreError> {
        self.conn().execute(
            &format!(
                "INSERT INTO folder ({FOLDER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET \
                 parentId = excluded.parentId, name = excluded.name, sortOrder = excluded.sortOrder, \
                 color = excluded.color"
            ),
            params![folder.id, folder.parent_id, folder.name, folder.sort_order, folder.color.map(Text)],
        )?;
        Ok(())
    }

    /// Deletes a folder and its subfolders; connections inside move to the root.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_folder(&self, id: &str) -> Result<(), StoreError> {
        self.conn().execute("DELETE FROM folder WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Moves a folder under `parent_id` (the root with `None`).
    ///
    /// # Errors
    /// [`StoreError::FolderCycle`] if the target is the folder or one of its descendants,
    /// [`StoreError::NotFound`] if the folder doesn't exist.
    pub fn move_folder(&self, id: &str, parent_id: Option<&str>) -> Result<(), StoreError> {
        self.transaction(|tx| {
            let mut cursor = parent_id.map(str::to_owned);
            while let Some(current) = cursor {
                if current == id {
                    return Err(StoreError::FolderCycle);
                }
                cursor = tx
                    .query_row("SELECT parentId FROM folder WHERE id = ?1", [&current], |r| {
                        r.get(0)
                    })
                    .optional()?
                    .flatten();
            }
            match tx.execute(
                "UPDATE folder SET parentId = ?1 WHERE id = ?2",
                params![parent_id, id],
            )? {
                0 => Err(StoreError::NotFound),
                _ => Ok(()),
            }
        })
    }

    // Entries

    /// Inserts or updates an entry, stamping `updated_at`, and copies its shared fields onto the stored
    /// rows of its connections. Returns the saved record.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown folder).
    pub fn save_entry(&self, entry: &Entry) -> Result<Entry, StoreError> {
        let mut saved = entry.clone();
        saved.updated_at = Timestamp::now();
        self.transaction(|tx| {
            upsert_entry(tx, &saved)?;
            mirror_entry(tx, &saved.id)
        })?;
        Ok(saved)
    }

    /// One entry.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn entry(&self, id: &str) -> Result<Option<Entry>, StoreError> {
        entry_in(&self.conn(), id)
    }

    /// Deletes an entry with its connections and their port forwards.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_entry(&self, id: &str) -> Result<(), StoreError> {
        self.conn().execute("DELETE FROM entry WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Saves an entry together with exactly `connections`, in one transaction, stamping `updated_at` on the
    /// entry and on every listed connection. The connections are written as given apart from their
    /// `entry_id` (set to the entry's) and their shared fields (the entry's), so their port forwards stay.
    ///
    /// Connections the entry had that are not listed are deleted with their forwards. A listed connection
    /// that belonged to another entry moves here; an entry left without connections that way, and every
    /// entry in `remove_entries`, is deleted.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown folder or credential); nothing changes then.
    pub fn write_entry(
        &self,
        entry: &Entry,
        connections: &[Connection],
        remove_entries: &[String],
    ) -> Result<EntryWrite, StoreError> {
        let now = Timestamp::now();
        let mut saved = entry.clone();
        saved.updated_at = now;
        self.transaction(|tx| {
            let mut previous: Vec<String> = remove_entries.to_vec();
            for connection in connections {
                if let Some(owner) = tx
                    .query_row(
                        "SELECT COALESCE(entryId, id) FROM connection WHERE id = ?1",
                        [&connection.id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                    && owner != saved.id
                    && !previous.contains(&owner)
                {
                    previous.push(owner);
                }
            }
            upsert_entry(tx, &saved)?;
            let existing: Vec<String> = tx
                .prepare("SELECT id FROM connection WHERE COALESCE(entryId, id) = ?1 ORDER BY id")?
                .query_map([&saved.id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let mut deleted_connections = Vec::new();
            for id in existing {
                if !connections.iter().any(|c| c.id == id) {
                    tx.execute("DELETE FROM connection WHERE id = ?1", [&id])?;
                    deleted_connections.push(id);
                }
            }
            for connection in connections {
                let mut c = connection.clone();
                c.entry_id.clone_from(&saved.id);
                c.updated_at = now;
                upsert_connection(tx, &c)?;
            }
            let mut deleted_entries = Vec::new();
            for id in previous.iter().filter(|id| **id != saved.id) {
                let gone: Vec<String> = tx
                    .prepare("SELECT id FROM connection WHERE COALESCE(entryId, id) = ?1 ORDER BY id")?
                    .query_map([id], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                let removed = tx.execute(
                    "DELETE FROM entry WHERE id = ?1 AND (?2 OR NOT EXISTS (SELECT 1 FROM connection WHERE entryId = ?1))",
                    params![id, remove_entries.contains(id)],
                )?;
                if removed > 0 {
                    deleted_entries.push(id.clone());
                    deleted_connections.extend(gone);
                }
            }
            Ok(EntryWrite {
                entry: saved.clone(),
                deleted_connections,
                deleted_entries,
            })
        })
    }

    /// Moves an entry into a folder (the root with `None`), stamping `updated_at`.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn move_entry(&self, id: &str, folder_id: Option<&str>) -> Result<(), StoreError> {
        let now = Timestamp::now().to_string();
        self.transaction(|tx| {
            if tx.execute(
                "UPDATE entry SET folderId = ?1, updatedAt = ?2 WHERE id = ?3",
                params![folder_id, now, id],
            )? == 0
            {
                return Err(StoreError::NotFound);
            }
            tx.execute(
                "UPDATE connection SET updatedAt = ?1 WHERE entryId = ?2",
                params![now, id],
            )?;
            mirror_entry(tx, id)
        })
    }

    /// Stars or unstars an entry; `updated_at` is left alone, as in the Swift app.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn set_entry_favorite(&self, id: &str, is_favorite: bool) -> Result<(), StoreError> {
        self.transaction(|tx| {
            if tx.execute(
                "UPDATE entry SET isFavorite = ?1 WHERE id = ?2",
                params![is_favorite, id],
            )? == 0
            {
                return Err(StoreError::NotFound);
            }
            mirror_entry(tx, id)
        })
    }

    // Connections

    /// Inserts or updates a connection, stamping `updated_at`. Returns the saved record.
    ///
    /// The connection's shared fields (name, host, folder, notes, color, favorite) are written to its entry,
    /// which is created when missing, and from there to the entry's other connections.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown folder or credential).
    pub fn save_connection(&self, connection: &Connection) -> Result<Connection, StoreError> {
        let mut saved = connection.clone();
        saved.updated_at = Timestamp::now();
        self.transaction(|tx| {
            let previous: Option<String> = tx
                .query_row(
                    "SELECT COALESCE(entryId, id) FROM connection WHERE id = ?1",
                    [&saved.id],
                    |r| r.get(0),
                )
                .optional()?;
            let existing = entry_in(tx, &saved.entry_id)?;
            let entry = Entry {
                default_connection_id: existing
                    .as_ref()
                    .map_or_else(|| Some(saved.id.clone()), |e| e.default_connection_id.clone()),
                created_at: existing.as_ref().map_or(saved.created_at, |e| e.created_at),
                ..Entry::for_connection(&saved)
            };
            upsert_entry(tx, &entry)?;
            mirror_entry(tx, &entry.id)?;
            upsert_connection(tx, &saved)?;
            // A connection moved to another entry leaves no empty entry behind.
            if let Some(previous) = previous.filter(|p| *p != saved.entry_id) {
                tx.execute(
                    "DELETE FROM entry WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM connection WHERE entryId = ?1)",
                    [&previous],
                )?;
            }
            Ok(())
        })?;
        Ok(saved)
    }

    /// One connection, with its entry's shared fields.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn connection(&self, id: &str) -> Result<Option<Connection>, StoreError> {
        let conn = self.conn();
        let connection = conn
            .query_row(
                &format!("SELECT {CONNECTION_COLUMNS} FROM connection WHERE id = ?1"),
                [id],
                connection_from_row,
            )
            .optional()?;
        connection.map(|c| overlaid(&conn, c)).transpose()
    }

    /// Deletes a connection and its port forwards. The entry's last connection takes the entry with it; a
    /// deleted default leaves the entry to fall back to its first connection.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_connection(&self, id: &str) -> Result<(), StoreError> {
        self.transaction(|tx| {
            let entry_id: Option<Option<String>> = tx
                .query_row("SELECT entryId FROM connection WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            tx.execute("DELETE FROM connection WHERE id = ?1", [id])?;
            if let Some(Some(entry_id)) = entry_id {
                tx.execute(
                    "DELETE FROM entry WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM connection WHERE entryId = ?1)",
                    [&entry_id],
                )?;
                tx.execute(
                    "UPDATE entry SET defaultConnectionId = NULL WHERE id = ?1 AND defaultConnectionId = ?2",
                    params![entry_id, id],
                )?;
            }
            Ok(())
        })
    }

    /// Moves a connection's entry into a folder (the root with `None`); see
    /// [`move_entry`](Self::move_entry).
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn move_connection(&self, id: &str, folder_id: Option<&str>) -> Result<(), StoreError> {
        let entry_id = entry_id_of(&self.conn(), id)?;
        self.ensure_entry(id, &entry_id)?;
        self.move_entry(&entry_id, folder_id)
    }

    /// Stars or unstars a connection's entry; see [`set_entry_favorite`](Self::set_entry_favorite).
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn set_favorite(&self, id: &str, is_favorite: bool) -> Result<(), StoreError> {
        let entry_id = entry_id_of(&self.conn(), id)?;
        self.ensure_entry(id, &entry_id)?;
        self.set_entry_favorite(&entry_id, is_favorite)
    }

    /// Writes the entry a row without one stands for, so entry-level writes have a record to change.
    fn ensure_entry(&self, connection_id: &str, entry_id: &str) -> Result<(), StoreError> {
        if self.entry(entry_id)?.is_none()
            && let Some(connection) = self.connection(connection_id)?
        {
            let conn = self.conn();
            upsert_entry(&conn, &Entry::for_connection(&connection))?;
            conn.execute(
                "UPDATE connection SET entryId = ?1 WHERE id = ?2 AND entryId IS NULL",
                params![entry_id, connection_id],
            )?;
        }
        Ok(())
    }

    /// Links connections to a credential, or unlinks them with `None`. Unknown ids and connections that
    /// already have that link are skipped. Returns how many connections changed.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown credential); nothing changes then.
    pub fn set_credential(
        &self,
        credential_id: Option<&str>,
        connection_ids: &[String],
    ) -> Result<usize, StoreError> {
        self.transaction(|tx| {
            let now = Timestamp::now().to_string();
            let mut changed = 0;
            for id in connection_ids {
                changed += tx.execute(
                    "UPDATE connection SET credentialId = ?1, updatedAt = ?2 WHERE id = ?3 AND credentialId IS NOT ?1",
                    params![credential_id, now, id],
                )?;
            }
            Ok(changed)
        })
    }

    /// Connections whose name, host, username or notes contain `text` (ASCII case-insensitive), by name.
    /// Blank text returns every connection in library order.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn search(&self, text: &str) -> Result<Vec<Connection>, StoreError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(self.library()?.connections);
        }
        let pattern = format!("%{trimmed}%");
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {CONNECTION_COLUMNS} FROM connection \
             WHERE name LIKE ?1 OR host LIKE ?1 OR username LIKE ?1 OR notes LIKE ?1 ORDER BY name"
        ))?;
        let found: Vec<Connection> = statement
            .query_map([pattern], connection_from_row)?
            .collect::<Result<_, _>>()?;
        found.into_iter().map(|c| overlaid(&conn, c)).collect()
    }

    // Credentials

    /// Inserts or updates a credential, stamping `updated_at`. Returns the saved record.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn save_credential(&self, credential: &Credential) -> Result<Credential, StoreError> {
        let mut saved = credential.clone();
        saved.updated_at = Timestamp::now();
        upsert_credential(&self.conn(), &saved)?;
        Ok(saved)
    }

    /// One credential.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn credential(&self, id: &str) -> Result<Option<Credential>, StoreError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {CREDENTIAL_COLUMNS} FROM credential WHERE id = ?1"),
                [id],
                credential_from_row,
            )
            .optional()?)
    }

    /// Deletes a credential; linked connections fall back to their own login.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_credential(&self, id: &str) -> Result<(), StoreError> {
        self.conn()
            .execute("DELETE FROM credential WHERE id = ?1", [id])?;
        Ok(())
    }

    // Saved splits

    /// Inserts or updates a saved split; a new one goes to the end of the list. Returns the saved record.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn save_saved_split(&self, split: &SavedSplit) -> Result<SavedSplit, StoreError> {
        self.transaction(|tx| {
            let mut saved = split.clone();
            let exists = tx.query_row("SELECT 1 FROM savedSplit WHERE id = ?1", [&saved.id], |_| Ok(())).optional()?;
            if exists.is_none() {
                let max: Option<i64> = tx.query_row("SELECT MAX(sortOrder) FROM savedSplit", [], |r| r.get(0))?;
                saved.sort_order = max.map_or(0, |m| m + 1);
            }
            tx.execute(
                &format!(
                    "INSERT INTO savedSplit ({SAVED_SPLIT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, connectionIds = excluded.connectionIds, \
                     sortOrder = excluded.sortOrder, createdAt = excluded.createdAt"
                ),
                params![
                    saved.id,
                    saved.name,
                    ids_json(&saved.connection_ids),
                    saved.sort_order,
                    saved.created_at.to_string()
                ],
            )?;
            Ok(saved)
        })
    }

    /// Deletes a saved split.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_saved_split(&self, id: &str) -> Result<(), StoreError> {
        self.conn()
            .execute("DELETE FROM savedSplit WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Stores the order of the saved splits as given; unknown ids are ignored.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn reorder_saved_splits(&self, ids: &[String]) -> Result<(), StoreError> {
        self.transaction(|tx| {
            for (index, id) in (0_i64..).zip(ids) {
                tx.execute(
                    "UPDATE savedSplit SET sortOrder = ?1 WHERE id = ?2",
                    params![index, id],
                )?;
            }
            Ok(())
        })
    }

    // Port forwards

    /// Inserts or updates a port forward.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown connection).
    pub fn save_port_forward(&self, forward: &PortForward) -> Result<(), StoreError> {
        self.conn().execute(
            &format!(
                "INSERT INTO portForward ({PORT_FORWARD_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(id) DO UPDATE SET connectionId = excluded.connectionId, name = excluded.name, \
                 localPort = excluded.localPort, remoteHost = excluded.remoteHost, \
                 remotePort = excluded.remotePort, createdAt = excluded.createdAt"
            ),
            params![
                forward.id,
                forward.connection_id,
                forward.name,
                forward.local_port,
                forward.remote_host,
                forward.remote_port,
                forward.created_at.to_string()
            ],
        )?;
        Ok(())
    }

    /// Deletes a port forward.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_port_forward(&self, id: &str) -> Result<(), StoreError> {
        self.conn()
            .execute("DELETE FROM portForward WHERE id = ?1", [id])?;
        Ok(())
    }
}

/// The entry with `id`.
fn entry_in(conn: &rusqlite::Connection, id: &str) -> Result<Option<Entry>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM entry WHERE id = ?1"),
            [id],
            entry_from_row,
        )
        .optional()?)
}

/// The entry a connection belongs to; a row without one stands for its own entry.
fn entry_id_of(conn: &rusqlite::Connection, connection_id: &str) -> Result<String, StoreError> {
    conn.query_row(
        "SELECT COALESCE(entryId, id) FROM connection WHERE id = ?1",
        [connection_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

/// The connection with its entry's shared fields; unchanged when the entry is missing.
fn overlaid(conn: &rusqlite::Connection, mut connection: Connection) -> Result<Connection, StoreError> {
    if let Some(entry) = entry_in(conn, &connection.entry_id)? {
        entry.overlay(&mut connection);
    }
    Ok(connection)
}

/// Copies an entry's shared fields onto the stored rows of its connections.
fn mirror_entry(conn: &rusqlite::Connection, entry_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE connection SET (folderId, name, host, notes, color, isFavorite) = \
         (SELECT folderId, name, host, notes, color, isFavorite FROM entry WHERE entry.id = connection.entryId) \
         WHERE entryId = ?1",
        [entry_id],
    )?;
    Ok(())
}

/// Writes an entry exactly as given (no timestamp changes).
pub(crate) fn upsert_entry(conn: &rusqlite::Connection, e: &Entry) -> Result<(), StoreError> {
    conn.execute(
        &format!(
            "INSERT INTO entry ({ENTRY_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(id) DO UPDATE SET folderId = excluded.folderId, name = excluded.name, \
             host = excluded.host, notes = excluded.notes, color = excluded.color, \
             isFavorite = excluded.isFavorite, defaultConnectionId = excluded.defaultConnectionId, \
             createdAt = excluded.createdAt, updatedAt = excluded.updatedAt"
        ),
        params![
            e.id,
            e.folder_id,
            e.name,
            e.host,
            e.notes,
            e.color.map(Text),
            e.is_favorite,
            e.default_connection_id,
            e.created_at.to_string(),
            e.updated_at.to_string()
        ],
    )?;
    Ok(())
}

/// Writes a connection exactly as given (no timestamp changes); for saves and bulk imports. Its entry is
/// created from the connection when missing (as its default); the stored shared columns are the entry's.
pub(crate) fn upsert_connection(
    conn: &rusqlite::Connection,
    connection: &Connection,
) -> Result<(), StoreError> {
    let entry = if let Some(entry) = entry_in(conn, &connection.entry_id)? {
        entry
    } else {
        let entry = Entry::for_connection(connection);
        upsert_entry(conn, &entry)?;
        entry
    };
    let mut c = connection.clone();
    entry.overlay(&mut c);
    conn.execute(
        &format!(
            "INSERT INTO connection ({CONNECTION_COLUMNS}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, \
             ?23, ?24, ?25, ?26, ?27, ?28) \
             ON CONFLICT(id) DO UPDATE SET folderId = excluded.folderId, credentialId = excluded.credentialId, \
             type = excluded.type, name = excluded.name, host = excluded.host, port = excluded.port, \
             username = excluded.username, domain = excluded.domain, passwordEnc = excluded.passwordEnc, \
             sshKeyPath = excluded.sshKeyPath, sshKeyPassphraseEnc = excluded.sshKeyPassphraseEnc, \
             isFavorite = excluded.isFavorite, rdpAdminSession = excluded.rdpAdminSession, \
             browserBundleId = excluded.browserBundleId, mikrotikMode = excluded.mikrotikMode, \
             sshTerminal = excluded.sshTerminal, rdpClient = excluded.rdpClient, color = excluded.color, \
             notes = excluded.notes, createdAt = excluded.createdAt, updatedAt = excluded.updatedAt, \
             entryId = excluded.entryId, label = excluded.label, showAsButton = excluded.showAsButton, \
             sortOrder = excluded.sortOrder, webScheme = excluded.webScheme, webPath = excluded.webPath"
        ),
        params![
            c.id,
            c.folder_id,
            c.credential_id,
            Text(c.kind),
            c.name,
            c.host,
            c.port,
            c.username,
            c.domain,
            sealed_sql(c.password_enc.as_ref()),
            c.ssh_key_path,
            sealed_sql(c.ssh_key_passphrase_enc.as_ref()),
            c.is_favorite,
            c.rdp_admin_session,
            c.browser_bundle_id,
            Text(c.mikrotik_mode),
            Text(c.ssh_terminal),
            Text(c.rdp_client),
            c.color.map(Text),
            c.notes,
            c.created_at.to_string(),
            c.updated_at.to_string(),
            c.entry_id,
            c.label,
            c.show_as_button,
            c.sort_order,
            c.web_scheme,
            c.web_path
        ],
    )?;
    Ok(())
}

/// Writes a credential exactly as given (no timestamp changes).
pub(crate) fn upsert_credential(conn: &rusqlite::Connection, c: &Credential) -> Result<(), StoreError> {
    conn.execute(
        &format!(
            "INSERT INTO credential ({CREDENTIAL_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, username = excluded.username, \
             domain = excluded.domain, passwordEnc = excluded.passwordEnc, sshKeyPath = excluded.sshKeyPath, \
             sshKeyPassphraseEnc = excluded.sshKeyPassphraseEnc, sshKeyUnlockedEnc = excluded.sshKeyUnlockedEnc, \
             notes = excluded.notes, createdAt = excluded.createdAt, updatedAt = excluded.updatedAt"
        ),
        params![
            c.id,
            c.name,
            c.username,
            c.domain,
            sealed_sql(c.password_enc.as_ref()),
            c.ssh_key_path,
            sealed_sql(c.ssh_key_passphrase_enc.as_ref()),
            sealed_sql(c.ssh_key_unlocked_enc.as_ref()),
            c.notes,
            c.created_at.to_string(),
            c.updated_at.to_string()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rd_core::{ConnectionType, Entry, LabelColor, PortForward};

    use super::*;

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    /// An entry with an SSH and an RDP connection, saved through `save_connection` and `upsert_connection`.
    fn two_way_entry(db: &Database) -> (Connection, Connection) {
        let ssh = db
            .save_connection(&Connection::new(
                ConnectionType::Ssh,
                "Home Server",
                "home.example.com",
            ))
            .unwrap();
        let mut rdp = Connection::new(ConnectionType::Rdp, "ignored", "ignored.example.com");
        rdp.entry_id.clone_from(&ssh.entry_id);
        rdp.label = "RDP kiki".into();
        rdp.sort_order = 1;
        upsert_connection(&db.conn(), &rdp).unwrap();
        (ssh, rdp)
    }

    fn mirrored(db: &Database, id: &str) -> (String, String, Option<String>, bool) {
        db.conn()
            .query_row(
                "SELECT name, host, folderId, isFavorite FROM connection WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn save_connection_creates_its_entry() {
        let db = db();
        let (ssh, rdp) = two_way_entry(&db);
        assert_eq!(ssh.entry_id, ssh.id);
        let entry = db.entry(&ssh.id).unwrap().unwrap();
        assert_eq!(
            (entry.name.as_str(), entry.host.as_str()),
            ("Home Server", "home.example.com")
        );
        assert_eq!(entry.default_connection_id.as_deref(), Some(ssh.id.as_str()));
        // The second connection takes the entry's shared fields, in the row too.
        assert_eq!(mirrored(&db, &rdp.id).0, "Home Server");
        let library = db.library().unwrap();
        assert_eq!(library.entries, vec![entry]);
        let ids: Vec<_> = library
            .connections_of(&ssh.id)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        assert_eq!(ids, [ssh.id.clone(), rdp.id.clone()]);
        assert_eq!(library.default_connection(&ssh.id).unwrap().id, ssh.id);
    }

    #[test]
    fn library_overlays_entry_fields() {
        let db = db();
        let folder = rd_core::Folder::new("Clients");
        db.save_folder(&folder).unwrap();
        let (ssh, rdp) = two_way_entry(&db);
        let mut entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        entry.name = "Renamed".into();
        entry.host = "new.example.com".into();
        entry.folder_id = Some(folder.id.clone());
        entry.notes = "shared".into();
        entry.color = Some(LabelColor::Blue);
        entry.is_favorite = true;
        let saved = db.save_entry(&entry).unwrap();
        assert!(saved.updated_at >= entry.updated_at);

        let library = db.library().unwrap();
        assert_eq!(library.entries, vec![saved]);
        for connection in &library.connections {
            assert_eq!(connection.name, "Renamed");
            assert_eq!(connection.host, "new.example.com");
            assert_eq!(connection.folder_id.as_deref(), Some(folder.id.as_str()));
            assert_eq!(connection.notes, "shared");
            assert_eq!(connection.color, Some(LabelColor::Blue));
            assert!(connection.is_favorite);
        }
        assert_eq!(db.connection(&rdp.id).unwrap().unwrap().name, "Renamed");
        assert_eq!(
            mirrored(&db, &rdp.id),
            ("Renamed".into(), "new.example.com".into(), Some(folder.id), true)
        );
        let rdp = library.connections.iter().find(|c| c.id == rdp.id).unwrap();
        assert_eq!((rdp.label.as_str(), rdp.sort_order), ("RDP kiki", 1));
    }

    #[test]
    fn library_falls_back_to_the_first_connection_without_a_stored_default() {
        let db = db();
        let (ssh, _rdp) = two_way_entry(&db);
        let mut entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        entry.default_connection_id = Some("gone".into());
        db.save_entry(&entry).unwrap();
        let library = db.library().unwrap();
        assert_eq!(library.default_connection(&ssh.entry_id).unwrap().id, ssh.id);
        assert!(library.default_connection("missing").is_none());
        assert!(library.entry("missing").is_none());
    }

    #[test]
    fn shared_field_writes_go_to_the_entry() {
        let db = db();
        let folder = rd_core::Folder::new("F");
        db.save_folder(&folder).unwrap();
        let (ssh, rdp) = two_way_entry(&db);

        db.move_connection(&rdp.id, Some(&folder.id)).unwrap();
        db.set_favorite(&rdp.id, true).unwrap();
        let entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        assert_eq!(entry.folder_id.as_deref(), Some(folder.id.as_str()));
        assert!(entry.is_favorite);
        assert_eq!(mirrored(&db, &ssh.id).2.as_deref(), Some(folder.id.as_str()));

        let mut renamed = db.connection(&ssh.id).unwrap().unwrap();
        renamed.name = "Edited".into();
        db.save_connection(&renamed).unwrap();
        assert_eq!(db.entry(&ssh.entry_id).unwrap().unwrap().name, "Edited");
        assert_eq!(db.connection(&rdp.id).unwrap().unwrap().name, "Edited");
        assert!(matches!(
            db.move_connection("missing", None),
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            db.set_favorite("missing", true),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn deleting_the_last_connection_deletes_its_entry() {
        let db = db();
        let (ssh, rdp) = two_way_entry(&db);
        db.delete_connection(&ssh.id).unwrap();
        let entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        assert_eq!(entry.default_connection_id, None);
        assert_eq!(
            db.library().unwrap().default_connection(&entry.id).unwrap().id,
            rdp.id
        );
        db.delete_connection(&rdp.id).unwrap();
        assert_eq!(db.entry(&ssh.entry_id).unwrap(), None);
        assert!(db.library().unwrap().entries.is_empty());
    }

    #[test]
    fn deleting_an_entry_deletes_its_connections_and_forwards() {
        let db = db();
        let (ssh, rdp) = two_way_entry(&db);
        let other = db
            .save_connection(&Connection::new(
                ConnectionType::Ssh,
                "Other",
                "other.example.com",
            ))
            .unwrap();
        for (id, connection) in [("P1", &ssh.id), ("P2", &other.id)] {
            db.save_port_forward(&PortForward {
                id: id.into(),
                connection_id: connection.clone(),
                name: String::new(),
                local_port: 8080,
                remote_host: "localhost".into(),
                remote_port: 80,
                created_at: Timestamp::now(),
            })
            .unwrap();
        }

        db.delete_entry(&ssh.entry_id).unwrap();
        let library = db.library().unwrap();
        assert_eq!(
            library.entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            [&other.id]
        );
        assert_eq!(
            library.connections.iter().map(|c| &c.id).collect::<Vec<_>>(),
            [&other.id]
        );
        assert_eq!(
            library
                .port_forwards
                .iter()
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>(),
            ["P2"]
        );
        assert_eq!(db.connection(&rdp.id).unwrap(), None);
    }

    fn forward(db: &Database, id: &str, connection: &str) {
        db.save_port_forward(&PortForward {
            id: id.into(),
            connection_id: connection.into(),
            name: String::new(),
            local_port: 8080,
            remote_host: "localhost".into(),
            remote_port: 80,
            created_at: Timestamp::now(),
        })
        .unwrap();
    }

    #[test]
    fn write_entry_replaces_the_connection_list() {
        let db = db();
        let (ssh, rdp) = two_way_entry(&db);
        forward(&db, "P1", &ssh.id);
        forward(&db, "P2", &rdp.id);
        let mut entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        entry.name = "Renamed".into();
        let mut kept = db.connection(&ssh.id).unwrap().unwrap();
        kept.label = "root".into();
        let added = Connection::new(ConnectionType::Web, "ignored", "ignored");
        let write = db.write_entry(&entry, &[kept, added.clone()], &[]).unwrap();
        assert_eq!(write.deleted_connections, std::slice::from_ref(&rdp.id));
        assert!(write.deleted_entries.is_empty());
        assert!(write.entry.updated_at >= entry.updated_at);

        let library = db.library().unwrap();
        let ids: Vec<_> = library
            .connections_of(&entry.id)
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        assert_eq!(
            ids,
            [
                (ssh.id.clone(), "Renamed".into()),
                (added.id.clone(), "Renamed".into())
            ]
        );
        assert_eq!(mirrored(&db, &added.id).0, "Renamed");
        assert_eq!(
            library.connections.iter().find(|c| c.id == ssh.id).unwrap().label,
            "root"
        );
        // The kept connection's forward stays; the deleted one's is gone.
        let forwards: Vec<_> = library.port_forwards.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(forwards, ["P1"]);
    }

    #[test]
    fn write_entry_moves_connections_and_deletes_emptied_entries() {
        let db = db();
        let (ssh, rdp) = two_way_entry(&db);
        let lone = db
            .save_connection(&Connection::new(ConnectionType::Ssh, "Lone", "lone.example.com"))
            .unwrap();
        let spare = db
            .save_connection(&Connection::new(
                ConnectionType::Ssh,
                "Spare",
                "spare.example.com",
            ))
            .unwrap();
        forward(&db, "P1", &lone.id);
        let entry = db.entry(&ssh.entry_id).unwrap().unwrap();
        let connections = [ssh.clone(), rdp.clone(), lone.clone()];
        let write = db
            .write_entry(&entry, &connections, std::slice::from_ref(&spare.entry_id))
            .unwrap();
        assert_eq!(
            write.deleted_entries,
            [spare.entry_id.clone(), lone.entry_id.clone()]
        );
        assert_eq!(write.deleted_connections, std::slice::from_ref(&spare.id));
        let library = db.library().unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.connections_of(&entry.id).len(), 3);
        let moved = library.connections.iter().find(|c| c.id == lone.id).unwrap();
        assert_eq!(
            (moved.name.as_str(), moved.host.as_str()),
            ("Home Server", "home.example.com")
        );
        assert_eq!(library.port_forwards_for(&lone.id).count(), 1);
    }

    #[test]
    fn rows_without_an_entry_read_and_write_as_their_own_entry() {
        let db = db();
        let lone = db
            .save_connection(&Connection::new(ConnectionType::Ssh, "Lone", "lone.example.com"))
            .unwrap();
        db.conn()
            .execute("UPDATE connection SET entryId = NULL", [])
            .unwrap();
        db.conn().execute("DELETE FROM entry", []).unwrap();
        let library = db.library().unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].id, lone.id);
        assert_eq!(library.default_connection(&lone.id).unwrap().id, lone.id);

        db.set_favorite(&lone.id, true).unwrap();
        let entry = db.entry(&lone.id).unwrap().unwrap();
        assert!(entry.is_favorite);
        db.move_connection(&lone.id, None).unwrap();
        assert!(matches!(
            db.move_entry("missing", None),
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            db.set_entry_favorite("missing", true),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn save_connection_into_another_entry_leaves_no_empty_entry() {
        let db = db();
        let (ssh, _rdp) = two_way_entry(&db);
        let mut lone = db
            .save_connection(&Connection::new(ConnectionType::Ssh, "Lone", "lone.example.com"))
            .unwrap();
        let old_entry = lone.entry_id.clone();
        lone.entry_id.clone_from(&ssh.entry_id);
        db.save_connection(&lone).unwrap();
        assert_eq!(db.entry(&old_entry).unwrap(), None);
        assert_eq!(db.library().unwrap().connections_of(&ssh.entry_id).len(), 3);
    }

    #[test]
    fn new_entries_get_fresh_ids() {
        let entry = Entry::new("Box", "box.example.com");
        assert_ne!(entry.id, Entry::new("Box", "box.example.com").id);
        let db = db();
        let saved = db.save_entry(&entry).unwrap();
        assert_eq!(db.entry(&entry.id).unwrap(), Some(saved));
    }
}
