//! Library commands. Every write refuses with `locked` or `readOnly` and emits `library-changed` after
//! success.

use rd_core::draft::EntryDraft;
use tauri::{AppHandle, State};

use super::{Core, blocking, emit_library};
use crate::clipboard;
use crate::core::{AppCore, CommandError, ConnectionSave, CredentialInput, FolderInput};
use crate::dto::{DeleteCountsDto, LibraryDto, MergeResultDto, MoveResultDto, SearchResultDto};

/// Runs a write on the blocking pool, then emits `library-changed`.
pub(super) async fn write<T: Send + 'static>(
    app: &AppHandle,
    core: &Core,
    work: impl FnOnce(&AppCore) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    let value = blocking(core, work).await?;
    emit_library(app, core);
    Ok(value)
}

/// The whole library (empty while locked).
#[tauri::command]
pub async fn library(core: State<'_, Core>) -> Result<LibraryDto, CommandError> {
    blocking(&core, |c| Ok(c.library())).await
}

/// Sidebar search: `{entries, credentials}` as ids.
#[tauri::command]
pub async fn search(core: State<'_, Core>, query: String) -> Result<SearchResultDto, CommandError> {
    blocking(&core, move |c| Ok(c.search(&query))).await
}

/// Link sheet rows for a credential, as connection ids.
#[tauri::command]
pub async fn link_candidates(
    core: State<'_, Core>,
    credential_id: String,
    query: String,
) -> Result<Vec<String>, CommandError> {
    blocking(&core, move |c| Ok(c.link_candidates(&credential_id, &query))).await
}

/// The "Used by" filter over the given connection ids.
#[tauri::command]
pub async fn matching(
    core: State<'_, Core>,
    connection_ids: Vec<String>,
    query: String,
) -> Result<Vec<String>, CommandError> {
    blocking(&core, move |c| Ok(c.matching(&connection_ids, &query))).await
}

/// Saves the entry editor: the shared fields and exactly `connections` in their order (each with
/// `password`/`passphrase`: `null` keeps, `""` clears, a value sets). Returns the entry id.
#[tauri::command]
pub async fn save_entry(
    app: AppHandle,
    core: State<'_, Core>,
    id: Option<String>,
    entry: EntryDraft,
    connections: Vec<ConnectionSave>,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| {
        c.save_entry(id.as_deref(), &entry, &connections)
    })
    .await
}

/// Merges the `sources` entries into `target`; returns `{movedConnections}`.
#[tauri::command]
pub async fn merge_entries(
    app: AppHandle,
    core: State<'_, Core>,
    target: String,
    sources: Vec<String>,
) -> Result<MergeResultDto, CommandError> {
    write(&app, &core, move |c| c.merge_entries(&target, &sources)).await
}

/// Duplicates an entry with all its connections and the localized `copySuffix` (" (copy)"); returns the
/// new entry id.
#[tauri::command]
pub async fn duplicate_entry(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
    copy_suffix: String,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| c.duplicate_entry(&id, &copy_suffix)).await
}

/// Saves the credential editor (same secret rule) and regenerates the unlocked key. Returns the id.
#[tauri::command]
pub async fn save_credential(
    app: AppHandle,
    core: State<'_, Core>,
    id: Option<String>,
    credential: CredentialInput,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| {
        c.save_credential(
            id.as_deref(),
            &credential,
            password.as_deref(),
            passphrase.as_deref(),
        )
    })
    .await
}

/// Creates or renames/recolors a folder. Returns the id.
#[tauri::command]
pub async fn save_folder(
    app: AppHandle,
    core: State<'_, Core>,
    folder: FolderInput,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| c.save_folder(&folder)).await
}

/// Deletes the items behind sidebar tags; with `deleteFolderEntries` the entries inside deleted folders go
/// too (otherwise they move to the root). Returns the counts.
#[tauri::command]
pub async fn delete_items(
    app: AppHandle,
    core: State<'_, Core>,
    tags: Vec<String>,
    delete_folder_entries: Option<bool>,
) -> Result<DeleteCountsDto, CommandError> {
    let delete_folder_entries = delete_folder_entries.unwrap_or(false);
    write(&app, &core, move |c| c.delete_items(&tags, delete_folder_entries)).await
}

/// Moves the items behind sidebar tags into a folder (`null` = root); returns `{moved, cycleRefused}`.
#[tauri::command]
pub async fn move_items(
    app: AppHandle,
    core: State<'_, Core>,
    tags: Vec<String>,
    target_folder_id: Option<String>,
) -> Result<MoveResultDto, CommandError> {
    write(&app, &core, move |c| {
        c.move_items(&tags, target_folder_id.as_deref())
    })
    .await
}

/// Makes a connection its entry's default. Errors: `notFound`, `readOnly`, `locked`.
#[tauri::command]
pub async fn set_default_connection(
    app: AppHandle,
    core: State<'_, Core>,
    entry_id: String,
    connection_id: String,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| {
        c.set_default_connection(&entry_id, &connection_id)
    })
    .await
}

/// Stars or unstars the entry behind `tag` (`e:`, `fav:` or `c:`).
#[tauri::command]
pub async fn set_favorite(
    app: AppHandle,
    core: State<'_, Core>,
    tag: String,
    is_favorite: bool,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| c.set_favorite(&tag, is_favorite)).await
}

/// Links (`credentialId`) or unlinks (`null`) the connections behind `tags` and `connectionIds`; returns
/// how many changed.
#[tauri::command]
pub async fn set_credential(
    app: AppHandle,
    core: State<'_, Core>,
    credential_id: Option<String>,
    tags: Option<Vec<String>>,
    connection_ids: Option<Vec<String>>,
) -> Result<usize, CommandError> {
    write(&app, &core, move |c| {
        c.set_credential(
            credential_id.as_deref(),
            &tags.unwrap_or_default(),
            &connection_ids.unwrap_or_default(),
        )
    })
    .await
}

/// Saves a new split (or replaces one with `id`) from up to four connections; returns the id.
#[tauri::command]
pub async fn save_saved_split(
    app: AppHandle,
    core: State<'_, Core>,
    id: Option<String>,
    name: String,
    connection_ids: Vec<String>,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| {
        c.save_saved_split(id.as_deref(), &name, &connection_ids)
    })
    .await
}

/// Renames a saved split.
#[tauri::command]
pub async fn rename_saved_split(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
    name: String,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| c.rename_saved_split(&id, &name)).await
}

/// Deletes a saved split.
#[tauri::command]
pub async fn delete_saved_split(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| c.delete_saved_split(&id)).await
}

/// Stores the saved splits' order.
#[tauri::command]
pub async fn reorder_saved_splits(
    app: AppHandle,
    core: State<'_, Core>,
    ids: Vec<String>,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| c.reorder_saved_splits(&ids)).await
}

/// Copies the connection's (resolved) password and clears it after 30 s if unchanged. Errors:
/// `noPassword`, `locked`, `clipboard`.
#[tauri::command]
pub async fn copy_password(core: State<'_, Core>, connection_id: String) -> Result<(), CommandError> {
    blocking(&core, move |c| {
        clipboard::copy_secret(
            &c.password_for_copy(&connection_id)?,
            clipboard::PASSWORD_LIFETIME,
        )
    })
    .await
}

/// Copies plain text (host, user, URL).
#[tauri::command]
pub fn copy_text(text: String) -> Result<(), CommandError> {
    clipboard::copy_text(&text)
}
