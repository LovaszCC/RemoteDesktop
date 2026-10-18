//! Library reads and writes: searches, saves with the secret rules, bulk delete and move, links, splits
//! and the password for "Copy password".

use std::collections::HashSet;

use rd_core::draft::ConnectionDraft;
use rd_core::tree::{self, SidebarTag};
use rd_core::{Connection, Credential, Folder, LabelColor, Library, SavedSplit, Sealed, Timestamp, new_id};
use rd_store::{StoreError, Vault};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::ssh_unlock::{self, UnlockError};
use super::{AppCore, CommandError};
use crate::dto::{DeleteCountsDto, LibraryDto, MoveResultDto, SearchResultDto};

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

    /// The sidebar search: connection ids in natural order and credential ids. Empty while locked.
    #[must_use]
    pub fn search(&self, query: &str) -> SearchResultDto {
        let Ok(library) = self.load_library() else {
            return SearchResultDto::default();
        };
        SearchResultDto {
            connections: ids(&tree::search(&library, query)),
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

    /// Saves the connection editor. `password` / `passphrase`: `None` keeps, `""` clears, a value is
    /// sealed. Returns the connection id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, a draft code (`emptyName`, `invalidUrl`, `emptyHost`, `invalidPort`),
    /// `notFound` or `database`.
    pub fn save_connection(
        &self,
        id: Option<&str>,
        draft: &ConnectionDraft,
        password: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::writable(&inner)?;
        draft.validate()?;
        let mut connection = match id {
            Some(id) => db.connection(id)?.ok_or_else(|| CommandError::not_found(id))?,
            None => Connection::new(draft.kind, "", ""),
        };
        update_secret(&mut connection.password_enc, password, vault);
        update_secret(&mut connection.ssh_key_passphrase_enc, passphrase, vault);
        draft.apply_to(&mut connection)?;
        Ok(db.save_connection(&connection)?.id)
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

    /// Deletes every item behind the sidebar tags (connections, folders with their subfolders,
    /// credentials, saved splits). Connections inside deleted folders move to the root. Returns how many
    /// existing items of each kind were named.
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
        for id in &items.connection_ids {
            if library.connections.iter().any(|c| &c.id == id) {
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

    /// Moves the connections and folders behind the tags into `target` (the root with `None`). A folder
    /// dropped onto itself is skipped; one that would go into its own subtree is refused and counted, and
    /// the other items still move.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn move_items(&self, tags: &[String], target: Option<&str>) -> Result<MoveResultDto, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let items = tree::movable(tags);
        let mut result = MoveResultDto::default();
        for id in &items.connection_ids {
            match db.move_connection(id, target) {
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

    /// Stars or unstars a connection.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` or `database`.
    pub fn set_favorite(&self, connection_id: &str, is_favorite: bool) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        Ok(db.set_favorite(connection_id, is_favorite)?)
    }

    /// Links (or with `None` unlinks) a credential to the connections behind `tags` (folders count with
    /// their subtree) plus `connection_ids`. Returns how many connections changed.
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

    /// Copies a connection with a new id, `name + copy_suffix` and the current creation time; everything
    /// else (credential, favorite, secrets) is kept. Returns the new id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` or `database`.
    pub fn duplicate_connection(&self, id: &str, copy_suffix: &str) -> Result<String, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        let original = db.connection(id)?.ok_or_else(|| CommandError::not_found(id))?;
        Ok(db.save_connection(&original.duplicated(copy_suffix))?.id)
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
