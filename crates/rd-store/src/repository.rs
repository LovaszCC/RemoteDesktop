//! Reading and writing the library: the Swift `ConnectionRepository` API in snake case.

use rd_core::{Connection, Credential, Folder, Library, PortForward, SavedSplit, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::rows::{
    CONNECTION_COLUMNS, CREDENTIAL_COLUMNS, FOLDER_COLUMNS, PORT_FORWARD_COLUMNS, SAVED_SPLIT_COLUMNS, Text,
    connection_from_row, credential_from_row, folder_from_row, ids_json, port_forward_from_row,
    saved_split_from_row, sealed_sql,
};
use crate::{Database, StoreError};

impl Database {
    /// Every record, in the Swift app's sort orders.
    ///
    /// # Errors
    /// If SQLite fails or a row holds a value this build can't read.
    pub fn library(&self) -> Result<Library, StoreError> {
        let conn = self.conn();
        let all = |sql: String| conn.prepare(&sql);
        Ok(Library {
            folders: all(format!(
                "SELECT {FOLDER_COLUMNS} FROM folder ORDER BY sortOrder, name COLLATE NOCASE"
            ))?
            .query_map([], folder_from_row)?
            .collect::<Result<_, _>>()?,
            connections: all(format!(
                "SELECT {CONNECTION_COLUMNS} FROM connection ORDER BY name COLLATE NOCASE"
            ))?
            .query_map([], connection_from_row)?
            .collect::<Result<_, _>>()?,
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

    // Connections

    /// Inserts or updates a connection, stamping `updated_at`. Returns the saved record.
    ///
    /// # Errors
    /// If SQLite fails (for example, an unknown folder or credential).
    pub fn save_connection(&self, connection: &Connection) -> Result<Connection, StoreError> {
        let mut saved = connection.clone();
        saved.updated_at = Timestamp::now();
        upsert_connection(&self.conn(), &saved)?;
        Ok(saved)
    }

    /// One connection.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn connection(&self, id: &str) -> Result<Option<Connection>, StoreError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {CONNECTION_COLUMNS} FROM connection WHERE id = ?1"),
                [id],
                connection_from_row,
            )
            .optional()?)
    }

    /// Deletes a connection and its port forwards.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn delete_connection(&self, id: &str) -> Result<(), StoreError> {
        self.conn()
            .execute("DELETE FROM connection WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Moves a connection into a folder (the root with `None`), stamping `updated_at`.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn move_connection(&self, id: &str, folder_id: Option<&str>) -> Result<(), StoreError> {
        let changed = self.conn().execute(
            "UPDATE connection SET folderId = ?1, updatedAt = ?2 WHERE id = ?3",
            params![folder_id, Timestamp::now().to_string(), id],
        )?;
        if changed == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(())
        }
    }

    /// Stars or unstars a connection; `updated_at` is left alone, as in the Swift app.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or a SQLite failure.
    pub fn set_favorite(&self, id: &str, is_favorite: bool) -> Result<(), StoreError> {
        let changed = self.conn().execute(
            "UPDATE connection SET isFavorite = ?1 WHERE id = ?2",
            params![is_favorite, id],
        )?;
        if changed == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(())
        }
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
        Ok(statement
            .query_map([pattern], connection_from_row)?
            .collect::<Result<_, _>>()?)
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

/// Writes a connection exactly as given (no timestamp changes); for saves and bulk imports.
pub(crate) fn upsert_connection(conn: &rusqlite::Connection, c: &Connection) -> Result<(), StoreError> {
    conn.execute(
        &format!(
            "INSERT INTO connection ({CONNECTION_COLUMNS}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22) \
             ON CONFLICT(id) DO UPDATE SET folderId = excluded.folderId, credentialId = excluded.credentialId, \
             type = excluded.type, name = excluded.name, host = excluded.host, port = excluded.port, \
             username = excluded.username, domain = excluded.domain, passwordEnc = excluded.passwordEnc, \
             sshKeyPath = excluded.sshKeyPath, sshKeyPassphraseEnc = excluded.sshKeyPassphraseEnc, \
             isFavorite = excluded.isFavorite, rdpAdminSession = excluded.rdpAdminSession, \
             browserBundleId = excluded.browserBundleId, mikrotikMode = excluded.mikrotikMode, \
             sshTerminal = excluded.sshTerminal, rdpClient = excluded.rdpClient, color = excluded.color, \
             notes = excluded.notes, createdAt = excluded.createdAt, updatedAt = excluded.updatedAt"
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
            c.updated_at.to_string()
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
