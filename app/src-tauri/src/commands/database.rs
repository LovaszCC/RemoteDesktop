//! Database file commands: switch, create, retry, the lock conflict, file dialogs (including the SSH key
//! picker) and reveal.

use std::path::PathBuf;

use rd_platform::settings::DATABASE_FILE;
use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use super::{Core, blocking, emit_status};
use crate::core::ssh_unlock::home_relative;
use crate::core::{CommandError, LockChoice};
use crate::dto::AppStatusDto;

/// Which file dialog [`pick_database_file`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PickMode {
    /// "Open existing…": an open panel for `.sqlite` files.
    Open,
    /// "Create new…": a save panel proposing `connections.sqlite`.
    Create,
}

/// Switches to an existing database file.
#[tauri::command]
pub async fn open_database(
    app: AppHandle,
    core: State<'_, Core>,
    path: PathBuf,
) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, move |c| Ok(c.open_database(&path))).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Switches to a new database file.
#[tauri::command]
pub async fn create_database(
    app: AppHandle,
    core: State<'_, Core>,
    path: PathBuf,
) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, move |c| Ok(c.create_database(&path))).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Opens the configured database again ("Retry").
#[tauri::command]
pub async fn reopen_database(app: AppHandle, core: State<'_, Core>) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, |c| Ok(c.open_current())).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Answers the lock conflict: `readOnly`, `openAnyway` or `cancel`. Errors: `noLockConflict`.
#[tauri::command]
pub async fn resolve_db_lock(
    app: AppHandle,
    core: State<'_, Core>,
    choice: LockChoice,
) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, move |c| c.resolve_db_lock(choice)).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Shows the database file in Finder, Explorer or the file manager. Errors: `noDatabase`, `revealFailed`.
#[tauri::command]
pub fn reveal_database(app: AppHandle, core: State<'_, Core>) -> Result<(), CommandError> {
    let path = core.database_path().ok_or_else(CommandError::no_database)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| CommandError::new("revealFailed", "Couldn't show the file.").with_detail(e.to_string()))
}

/// Shows an open or save panel for a database file; the chosen path, or `null` when cancelled.
#[tauri::command]
pub async fn pick_database_file(
    app: AppHandle,
    core: State<'_, Core>,
    mode: PickMode,
) -> Result<Option<String>, CommandError> {
    let mut dialog = app.dialog().file().add_filter("SQLite", &["sqlite"]);
    if let Some(dir) = core.database_path().as_deref().and_then(std::path::Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || match mode {
        PickMode::Open => dialog.blocking_pick_file(),
        PickMode::Create => dialog.set_file_name(DATABASE_FILE).blocking_save_file(),
    })
    .await
    .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Shows an open panel for an SSH private key, starting in `~/.ssh`; the chosen path made `~`-relative
/// when it is inside the home folder, or `null` when cancelled.
#[tauri::command]
pub async fn pick_ssh_key(app: AppHandle) -> Result<Option<String>, CommandError> {
    let home = dirs::home_dir();
    let mut dialog = app.dialog().file();
    if let Some(ssh) = home.as_ref().map(|h| h.join(".ssh")).filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(ssh);
    } else if let Some(home) = &home {
        dialog = dialog.set_directory(home);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| home_relative(&path, home.as_deref())))
}
