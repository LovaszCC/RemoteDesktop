//! Library reads and writes: searches, saves with the secret rules, bulk delete and move, links, splits
//! and the password for "Copy password".

use std::collections::HashSet;

use rd_core::draft::{ConnectionDraft, EntryDraft, LegacyConnectionDraft};
use rd_core::tree::{self, SidebarTag};
use rd_core::{
    Connection, Credential, Entry, Folder, LabelColor, Library, SavedSplit, Sealed, Timestamp, new_id,
};
use rd_store::{StoreError, Vault};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::ssh_unlock::{self, UnlockError};
use super::{AppCore, CommandError};
use crate::dto::{DeleteCountsDto, LibraryDto, MergeResultDto, MoveResultDto, SearchResultDto};

/// The most panes a split holds.
pub const MAX_SPLIT_PANES: usize = 4;

/// The credential editor's secret-free fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CredentialInput {
    /// Display name; required.
    pub name: String,
    /// User name.
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// SSH key path; empty means none.
    pub ssh_key_path: String,
    /// Notes.
    pub notes: String,
}

/// The folder editor's fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FolderInput {
    /// Existing folder to rename; `None` creates one.
    pub id: Option<String>,
    /// Parent of a new folder (ignored for an existing one; use `move_items`).
    pub parent_id: Option<String>,
    /// Display name; required.
    pub name: String,
    /// Tag color.
    pub color: Option<LabelColor>,
}

/// One connection of the entry editor: `id` `None` creates it. `password` / `passphrase`: `None` keeps the
/// stored value, `""` clears it, anything else is sealed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSave {
    /// The connection being edited; it must belong to the entry.
    #[serde(default)]
    pub id: Option<String>,
    /// Its secret-free fields.
    pub draft: ConnectionDraft,
    /// Password update.
    #[serde(default)]
    pub password: Option<String>,
    /// Key passphrase update.
    #[serde(default)]
    pub passphrase: Option<String>,
    /// Becomes the entry's default connection (at most one per save).
    #[serde(default)]
    pub is_default: bool,
}

/// Swift `.whitespaces` trim (spaces and tabs).
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

/// The secret rule shared by the editors: `None` keeps the stored value, `""` clears it, anything else is
/// sealed.
fn update_secret(slot: &mut Option<Sealed>, update: Option<&str>, vault: &Vault) {
    if let Some(value) = update {
        *slot = vault.seal_optional(Some(value));
    }
}

/// Swift `unlockedKeyEnc`: the passphrase-free copy of the key, sealed; `None` without a path or
/// passphrase. An unreadable key file keeps the stored copy when path and passphrase didn't change.
fn unlocked_key(
    path: Option<&str>,
    passphrase: &str,
    existing: Option<&Credential>,
    vault: &Vault,
) -> Result<Option<Sealed>, CommandError> {
    let Some(path) = path.filter(|_| !passphrase.is_empty()) else {
        return Ok(None);
    };
    match ssh_unlock::unlock(path, passphrase) {
        Ok(pem) => Ok(Some(vault.seal(&pem))),
        Err(UnlockError::WrongPassphrase) => {
            Err(CommandError::new("wrongKeyPassphrase", "Wrong key passphrase."))
        }
        Err(UnlockError::FileUnreadable) => {
            if let Some(existing) = existing
                && let Some(stored) = &existing.ssh_key_unlocked_enc
                && existing.ssh_key_path.as_deref() == Some(path)
                && vault
                    .open_optional(existing.ssh_key_passphrase_enc.as_ref())?
                    .is_some_and(|old| old.expose_secret() == passphrase)
            {
                return Ok(Some(stored.clone()));
            }
            Err(CommandError::new("cannotReadKey", "Can't read the SSH key: %@").with_detail(path))
        }
        Err(UnlockError::Failed(reason)) => {
            Err(CommandError::new("cannotUnlockKey", "Couldn't unlock the SSH key: %@").with_detail(reason))
        }
    }
}

fn ids(connections: &[&Connection]) -> Vec<String> {
    connections.iter().map(|c| c.id.clone()).collect()
}

fn no_connections() -> CommandError {
    CommandError::new("noConnections", "An entry needs at least one connection.")
}

/// The entry with `id`: its record, or the one a lone connection row without a record stands for.
fn entry_of(library: &Library, id: &str) -> Result<Entry, CommandError> {
    library
        .entry(id)
        .cloned()
        .or_else(|| {
            library
                .connections_of(id)
                .first()
                .map(|c| Entry::for_connection(c))
        })
        .ok_or_else(|| CommandError::not_found(id))
}

/// `notes` with a "Merged from" block per source: the source's notes, or only the header when its host
/// differs from `host`, so the replaced host is recorded.
fn merged_notes(notes: &str, host: &str, sources: &[Entry]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !notes.is_empty() {
        parts.push(notes.to_owned());
    }
    for source in sources {
        let other_host = !source.host.eq_ignore_ascii_case(host);
        if source.notes.is_empty() && !other_host {
            continue;
        }
        let mut block = format!("Merged from {} ({}):", source.name, source.host);
        if !source.notes.is_empty() {
            block.push('\n');
            block.push_str(&source.notes);
        }
        parts.push(block);
    }
    parts.join("\n\n")
}

impl AppCore {
    /// The library for the UI; empty while locked.
    #[must_use]
    pub fn library(&self) -> LibraryDto {
        LibraryDto::new(&self.load_library().unwrap_or_default())
    }

    fn load_library(&self) -> Result<Library, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::unlocked(&inner)?;
        Ok(db.library()?)
    }

    /// The sidebar search: entry ids in natural order and credential ids. Empty while locked.
    #[must_use]
    pub fn search(&self, query: &str) -> SearchResultDto {
        let Ok(library) = self.load_library() else {
            return SearchResultDto::default();
        };
        SearchResultDto {
            entries: tree::search(&library, query)
                .iter()
                .map(|e| e.id.clone())
                .collect(),
            credentials: tree::search_credentials(&library, query)
                .iter()
                .map(|c| c.id.clone())
                .collect(),
        }
    }

    /// The link sheet's rows: connections not linked to `credential_id`, filtered by `query`.
    #[must_use]
    pub fn link_candidates(&self, credential_id: &str, query: &str) -> Vec<String> {
        self.load_library()
            .map(|library| ids(&tree::link_candidates(&library, credential_id, query)))
            .unwrap_or_default()
    }

    /// The "Used by" filter: `connection_ids` that match `query`, in natural name order.
    #[must_use]
    pub fn matching(&self, connection_ids: &[String], query: &str) -> Vec<String> {
        let Ok(library) = self.load_library() else {
            return Vec::new();
        };
        let wanted: HashSet<&str> = connection_ids.iter().map(String::as_str).collect();
        ids(&tree::matching(
            library
                .connections
                .iter()
                .filter(|c| wanted.contains(c.id.as_str())),
            query,
            &library,
        ))
    }

    /// Saves the entry editor: the shared fields and exactly `connections`, in their order, in one
    /// transaction. Connections of the entry missing from the list are deleted and their forwards
    /// stopped. The connection flagged `isDefault` (new or existing) becomes the default; with none
    /// flagged the stored default is kept while it is still listed, otherwise the first one is. Returns the
    /// entry id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `emptyName`, `noConnections`, `multipleDefaults`, a draft code (`invalidUrl`, `emptyHost`,
    /// `invalidPort`), `notFound` (the entry, or a connection that isn't the entry's or is listed twice) or
    /// `database`.
    pub fn save_entry(
        &self,
        id: Option<&str>,
        entry: &EntryDraft,
        connections: &[ConnectionSave],
    ) -> Result<String, CommandError> {
        self.save_entry_with_ids(id, entry, connections)
            .map(|(entry_id, _)| entry_id)
    }

    /// [`save_entry`](Self::save_entry), also returning the saved connection ids in order.
    fn save_entry_with_ids(
        &self,
        id: Option<&str>,
        draft: &EntryDraft,
        connections: &[ConnectionSave],
    ) -> Result<(String, Vec<String>), CommandError> {
        let (write, connection_ids) = {
            let inner = self.inner();
            let (db, vault) = Self::writable(&inner)?;
            draft.validate()?;
            if connections.is_empty() {
                return Err(no_connections());
            }
            if connections.iter().filter(|c| c.is_default).count() > 1 {
                return Err(CommandError::new(
                    "multipleDefaults",
                    "Only one connection can be the default.",
                ));
            }
            for save in connections {
                save.draft.validate(&draft.host)?;
            }
            let library = db.library()?;
            let mut entry = match id {
                Some(id) => entry_of(&library, id)?,
                None => Entry::new("", ""),
            };
            draft.apply_to(&mut entry);
            let mut saved = Vec::with_capacity(connections.len());
            for (index, save) in connections.iter().enumerate() {
                let mut connection = match &save.id {
                    Some(connection_id) => library
                        .connections
                        .iter()
                        .find(|c| &c.id == connection_id && c.entry_id == entry.id)
                        .filter(|_| !saved.iter().any(|s: &Connection| &s.id == connection_id))
                        .cloned()
                        .ok_or_else(|| CommandError::not_found(connection_id))?,
                    None => Connection::new(save.draft.kind, "", ""),
                };
                update_secret(&mut connection.password_enc, save.password.as_deref(), vault);
                update_secret(
                    &mut connection.ssh_key_passphrase_enc,
                    save.passphrase.as_deref(),
                    vault,
                );
                save.draft.apply_to(&entry.host, &mut connection)?;
                connection.entry_id.clone_from(&entry.id);
                connection.sort_order = i64::try_from(index).unwrap_or(i64::MAX);
                saved.push(connection);
            }
            let flagged = connections.iter().position(|c| c.is_default);
            let stored = entry
                .default_connection_id
                .as_ref()
                .and_then(|id| saved.iter().position(|c| &c.id == id));
            let default = flagged.or(stored).unwrap_or(0);
            entry.default_connection_id = Some(saved[default].id.clone());
            let write = db.write_entry(&entry, &saved, &[])?;
            (write, saved.into_iter().map(|c| c.id).collect::<Vec<_>>())
        };
        // Their forwards are gone with them (cascade): stop the tunnels.
        self.stop_forwards_of(&write.deleted_connections);
        Ok((write.entry.id, connection_ids))
    }

    // Compatibility with the pre-entries UI; removed in the entries UI task.
    /// The pre-entries editor's save: one connection with its entry's shared fields. A new connection gets
    /// its own entry; an existing one updates its entry's shared fields and keeps the entry's other
    /// connections, its label and button flag. Returns the connection id.
    ///
    /// # Errors
    /// As [`save_entry`](Self::save_entry).
    pub fn save_connection(
        &self,
        id: Option<&str>,
        draft: &LegacyConnectionDraft,
        password: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<String, CommandError> {
        let (entry, mut connection) = draft.split()?;
        let edited = |draft: ConnectionDraft, id: Option<&str>| ConnectionSave {
            id: id.map(str::to_owned),
            draft,
            password: password.map(str::to_owned),
            passphrase: passphrase.map(str::to_owned),
            is_default: false,
        };
        let Some(id) = id else {
            let (_, ids) = self.save_entry_with_ids(None, &entry, &[edited(connection, None)])?;
            return Ok(ids.into_iter().next().unwrap_or_default());
        };
        let library = self.load_library()?;
        let existing = library
            .connections
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CommandError::not_found(id))?;
        connection.label.clone_from(&existing.label);
        connection.show_as_button = existing.show_as_button;
        let saves: Vec<ConnectionSave> = library
            .connections_of(&existing.entry_id)
            .into_iter()
            .map(|c| {
                if c.id == id {
                    edited(connection.clone(), Some(id))
                } else {
                    ConnectionSave {
                        id: Some(c.id.clone()),
                        draft: ConnectionDraft::from_connection(c),
                        password: None,
                        passphrase: None,
                        is_default: false,
                    }
                }
            })
            .collect();
        self.save_entry_with_ids(Some(&existing.entry_id), &entry, &saves)?;
        Ok(id.to_owned())
    }

    /// Merges the `sources` entries into `target`, in the order given: their connections (with their
    /// credentials, secrets and forwards) move to the end of the target's, the target's shared fields and
    /// default win, and each source's notes are appended as `Merged from <name> (<host>):` — the header
    /// alone when a source on another host has no notes. The sources are deleted.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `mergeIntoSelf`, `notFound` or `database`.
    pub fn merge_entries(&self, target: &str, sources: &[String]) -> Result<MergeResultDto, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        if sources.iter().any(|s| s == target) {
            return Err(CommandError::new(
                "mergeIntoSelf",
                "An entry can't be merged into itself.",
            ));
        }
        let library = db.library()?;
        let mut entry = entry_of(&library, target)?;
        if sources.is_empty() {
            return Ok(MergeResultDto::default());
        }
        let mut source_ids: Vec<String> = Vec::new();
        for id in sources {
            if !source_ids.contains(id) {
                source_ids.push(id.clone());
            }
        }
        let source_entries = source_ids
            .iter()
            .map(|id| entry_of(&library, id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut connections: Vec<Connection> = library.connections_of(target).into_iter().cloned().collect();
        let kept = connections.len();
        for id in &source_ids {
            connections.extend(library.connections_of(id).into_iter().cloned());
        }
        for (index, connection) in connections.iter_mut().enumerate() {
            connection.sort_order = i64::try_from(index).unwrap_or(i64::MAX);
        }
        entry.notes = merged_notes(&entry.notes, &entry.host, &source_entries);
        entry.default_connection_id = library.default_connection(target).map(|c| c.id.clone());
        let write = db.write_entry(&entry, &connections, &source_ids)?;
        drop(inner);
        // A source row's connections all move, but stop whatever the write did delete.
        self.stop_forwards_of(&write.deleted_connections);
        Ok(MergeResultDto {
            moved_connections: connections.len() - kept,
        })
    }

    /// Saves the credential editor with the same secret rule as
    /// [`save_connection`](Self::save_connection), regenerating the unlocked key copy. Returns the id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `emptyName`, `notFound`, `wrongKeyPassphrase`, `cannotReadKey` (detail: the
    /// path), `cannotUnlockKey` or `database`.
    pub fn save_credential(
        &self,
        id: Option<&str>,
        input: &CredentialInput,
        password: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::writable(&inner)?;
        let name = trim(&input.name);
        if name.is_empty() {
            return Err(CommandError::empty_name());
        }
        let existing = match id {
            Some(id) => Some(db.credential(id)?.ok_or_else(|| CommandError::not_found(id))?),
            None => None,
        };
        let mut credential = existing.clone().unwrap_or_else(|| Credential::new(""));
        name.clone_into(&mut credential.name);
        trim(&input.username).clone_into(&mut credential.username);
        trim(&input.domain).clone_into(&mut credential.domain);
        let key_path = trim(&input.ssh_key_path);
        credential.ssh_key_path = (!key_path.is_empty()).then(|| key_path.to_owned());
        credential.notes.clone_from(&input.notes);

        let passphrase_text: SecretString = match passphrase {
            Some(text) => SecretString::from(text),
            None => vault
                .open_optional(credential.ssh_key_passphrase_enc.as_ref())?
                .unwrap_or_else(|| SecretString::from("")),
        };
        update_secret(&mut credential.password_enc, password, vault);
        update_secret(&mut credential.ssh_key_passphrase_enc, passphrase, vault);
        credential.ssh_key_unlocked_enc = unlocked_key(
            credential.ssh_key_path.as_deref(),
            passphrase_text.expose_secret(),
            existing.as_ref(),
            vault,
        )?;
        Ok(db.save_credential(&credential)?.id)
    }

    /// Creates a folder, or renames and recolors an existing one. Returns the id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `emptyName`, `notFound` or `database`.
    pub fn save_folder(&self, input: &FolderInput) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let name = trim(&input.name);
        if name.is_empty() {
            return Err(CommandError::empty_name());
        }
        let folder = match &input.id {
            Some(id) => {
                let mut folder = db
                    .library()?
                    .folders
                    .into_iter()
                    .find(|f| &f.id == id)
                    .ok_or_else(|| CommandError::not_found(id))?;
                name.clone_into(&mut folder.name);
                folder.color = input.color;
                folder
            }
            None => Folder {
                parent_id: input.parent_id.clone(),
                color: input.color,
                ..Folder::new(name)
            },
        };
        db.save_folder(&folder)?;
        Ok(folder.id)
    }

    /// Deletes every item behind the sidebar tags (entries with their connections, single connections,
    /// folders with their subfolders, credentials, saved splits). An entry's last connection takes the
    /// entry with it. Entries inside deleted folders move to the root. Returns how many existing items of
    /// each kind were named; `connections` counts every connection deleted.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn delete_items(&self, tags: &[String]) -> Result<DeleteCountsDto, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let library = db.library()?;
        let items = tree::sidebar_items(tags);
        let mut splits: Vec<String> = tags
            .iter()
            .filter_map(|t| match SidebarTag::parse(t) {
                Some(SidebarTag::Split(id)) => Some(id),
                _ => None,
            })
            .collect();
        splits.dedup();
        let mut counts = DeleteCountsDto::default();
        let mut deleted = Vec::new();
        for id in &items.entry_ids {
            let members = ids(&library.connections_of(id));
            if members.is_empty() && library.entry(id).is_none() {
                continue;
            }
            db.delete_entry(id)?;
            // A row without an entry record isn't reached by the cascade.
            for member in &members {
                db.delete_connection(member)?;
            }
            counts.entries += 1;
            counts.connections += members.len();
            deleted.extend(members);
        }
        for id in &items.connection_ids {
            if !deleted.contains(id) && library.connections.iter().any(|c| &c.id == id) {
                db.delete_connection(id)?;
                deleted.push(id.clone());
                counts.connections += 1;
            }
        }
        // Their forwards are gone with them (cascade): stop the tunnels.
        self.stop_forwards_of(&deleted);
        for id in &items.folder_ids {
            if library.folders.iter().any(|f| &f.id == id) {
                db.delete_folder(id)?;
                counts.folders += 1;
            }
        }
        for id in &items.credential_ids {
            if library.credentials.iter().any(|c| &c.id == id) {
                db.delete_credential(id)?;
                counts.credentials += 1;
            }
        }
        for id in &splits {
            if library.saved_splits.iter().any(|s| &s.id == id) {
                db.delete_saved_split(id)?;
                counts.splits += 1;
            }
        }
        Ok(counts)
    }

    /// Moves the entries (a connection row stands for its entry) and folders behind the tags into
    /// `target` (the root with `None`). A folder dropped onto itself is skipped; one that would go into its
    /// own subtree is refused and counted, and the other items still move.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn move_items(&self, tags: &[String], target: Option<&str>) -> Result<MoveResultDto, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let items = tree::movable(tags);
        let library = db.library()?;
        let mut entries: Vec<String> = Vec::new();
        let owners = items.connection_ids.iter().filter_map(|id| {
            library
                .connections
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.entry_id.clone())
        });
        for id in items.entry_ids.iter().cloned().chain(owners) {
            if !entries.contains(&id) {
                entries.push(id);
            }
        }
        let mut result = MoveResultDto::default();
        for id in &entries {
            // A row without an entry record is moved through its connection.
            let moved = match library.connections_of(id).first() {
                Some(c) if library.entry(id).is_none() => db.move_connection(&c.id, target),
                _ => db.move_entry(id, target),
            };
            match moved {
                Ok(()) => result.moved += 1,
                Err(StoreError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        for id in items.folder_ids.iter().filter(|id| Some(id.as_str()) != target) {
            match db.move_folder(id, target) {
                Ok(()) => result.moved += 1,
                Err(StoreError::FolderCycle) => result.cycle_refused += 1,
                Err(StoreError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(result)
    }

    /// Stars or unstars the entry behind a sidebar tag: `e:` or `fav:` (the entry) or `c:` (its entry).
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` (also for any other tag) or `database`.
    pub fn set_favorite(&self, tag: &str, is_favorite: bool) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let written = match SidebarTag::parse(tag) {
            Some(SidebarTag::Connection(id)) => db.set_favorite(&id, is_favorite),
            Some(SidebarTag::Entry(id) | SidebarTag::Favorite(id)) => {
                let library = db.library()?;
                match library.connections_of(&id).first() {
                    // A row without an entry record is starred through its connection.
                    Some(c) if library.entry(&id).is_none() => db.set_favorite(&c.id, is_favorite),
                    _ => db.set_entry_favorite(&id, is_favorite),
                }
            }
            _ => Err(StoreError::NotFound),
        };
        written.map_err(|e| match e {
            StoreError::NotFound => CommandError::not_found(tag),
            e => e.into(),
        })
    }

    /// Links (or with `None` unlinks) a credential to the connections behind `tags` (an entry counts with
    /// its default connection, a folder with every connection in its subtree) plus `connection_ids`.
    /// Returns how many connections changed.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database` (for example, an unknown credential).
    pub fn set_credential(
        &self,
        credential_id: Option<&str>,
        tags: &[String],
        connection_ids: &[String],
    ) -> Result<usize, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let mut ids = if tags.is_empty() {
            Vec::new()
        } else {
            db.library()?.connection_ids_for_tags(tags)
        };
        let mut seen: HashSet<String> = ids.iter().cloned().collect();
        ids.extend(
            connection_ids
                .iter()
                .filter(|id| seen.insert((*id).clone()))
                .cloned(),
        );
        Ok(db.set_credential(credential_id, &ids)?)
    }

    /// Copies an entry with all its connections: new ids, `name + copy_suffix`, created now, the default
    /// pointing at the copy of the original's. Everything else (folder, favorite, labels, credentials and
    /// secrets) is kept, as Duplicate always did; port forwards are not copied. Returns the new entry id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` or `database`.
    pub fn duplicate_entry(&self, id: &str, copy_suffix: &str) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let library = db.library()?;
        let original = entry_of(&library, id)?;
        let now = Timestamp::now();
        let mut entry = Entry {
            id: new_id(),
            name: format!("{}{copy_suffix}", original.name),
            created_at: now,
            updated_at: now,
            ..original
        };
        let default = library.default_connection(id).map(|c| c.id.clone());
        let mut copies = Vec::new();
        for connection in library.connections_of(id) {
            let copy = connection.duplicated(copy_suffix);
            if default.as_ref() == Some(&connection.id) {
                entry.default_connection_id = Some(copy.id.clone());
            }
            copies.push(copy);
        }
        Ok(db.write_entry(&entry, &copies, &[])?.entry.id)
    }

    // Compatibility with the pre-entries UI; removed in the entries UI task.
    /// Duplicates the entry of connection `id` (see [`duplicate_entry`](Self::duplicate_entry)). Returns the
    /// copy of that connection.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` or `database`.
    pub fn duplicate_connection(&self, id: &str, copy_suffix: &str) -> Result<String, CommandError> {
        let library = self.load_library()?;
        let original = library
            .connections
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CommandError::not_found(id))?;
        let index = library
            .connections_of(&original.entry_id)
            .iter()
            .position(|c| c.id == id)
            .unwrap_or_default();
        let copy = self.duplicate_entry(&original.entry_id, copy_suffix)?;
        let library = self.load_library()?;
        Ok(library
            .connections_of(&copy)
            .get(index)
            .map(|c| c.id.clone())
            .unwrap_or_default())
    }

    /// Saves a new split from up to [`MAX_SPLIT_PANES`] connections, or with `id` replaces an existing
    /// one's name and panes. Returns the id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `emptyName`, `notFound` or `database`.
    pub fn save_saved_split(
        &self,
        id: Option<&str>,
        name: &str,
        connection_ids: &[String],
    ) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let name = trim(name);
        if name.is_empty() {
            return Err(CommandError::empty_name());
        }
        let panes: Vec<String> = connection_ids.iter().take(MAX_SPLIT_PANES).cloned().collect();
        let split = match id {
            Some(id) => {
                let mut split = Self::find_split(db, id)?;
                name.clone_into(&mut split.name);
                split.connection_ids = panes;
                split
            }
            None => SavedSplit {
                id: new_id(),
                name: name.to_owned(),
                connection_ids: panes,
                sort_order: 0,
                created_at: Timestamp::now(),
            },
        };
        Ok(db.save_saved_split(&split)?.id)
    }

    fn find_split(db: &rd_store::Database, id: &str) -> Result<SavedSplit, CommandError> {
        db.library()?
            .saved_splits
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| CommandError::not_found(id))
    }

    /// Renames a saved split.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `emptyName`, `notFound` or `database`.
    pub fn rename_saved_split(&self, id: &str, name: &str) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let name = trim(name);
        if name.is_empty() {
            return Err(CommandError::empty_name());
        }
        let mut split = Self::find_split(db, id)?;
        name.clone_into(&mut split.name);
        db.save_saved_split(&split)?;
        Ok(())
    }

    /// Deletes a saved split.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn delete_saved_split(&self, id: &str) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        Ok(db.delete_saved_split(id)?)
    }

    /// Stores the saved splits' order.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn reorder_saved_splits(&self, ids: &[String]) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        Ok(db.reorder_saved_splits(ids)?)
    }

    /// The password "Copy password" puts on the clipboard: the connection's, or its credential's.
    ///
    /// # Errors
    /// `locked`, `notFound`, `noPassword` ("No saved password"), `corruptData` or `database`.
    pub fn password_for_copy(&self, connection_id: &str) -> Result<SecretString, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::unlocked(&inner)?;
        let library = db.library()?;
        let connection = library
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| CommandError::not_found(connection_id))?;
        vault
            .open_optional(library.resolved(connection).password_enc.as_ref())?
            .ok_or_else(|| CommandError::new("noPassword", "No saved password"))
    }
}
