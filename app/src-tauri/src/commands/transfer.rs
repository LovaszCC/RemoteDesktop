//! Export and import of the app's own format: the file panels, the export, the preview and the commit.
//! The commit emits `library-changed`.

use std::path::PathBuf;

use rd_store::EXPORT_EXTENSION;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{Core, blocking, emit_library};
use crate::core::CommandError;
use crate::dto::{ExportResultDto, TransferDecisionDto, TransferPreviewDto, TransferResultDto};

/// The file name the save panel suggests.
const DEFAULT_FILE_NAME: &str = "RemoteDesktop.rdexport";

fn panel_failed(e: impl ToString) -> CommandError {
    CommandError::new("internal", "The command failed.").with_detail(e.to_string())
}

/// Shows a save panel for an export; the chosen path, or `null` when cancelled.
#[tauri::command]
pub async fn pick_export_path(app: AppHandle) -> Result<Option<String>, CommandError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("RemoteDesktop export", &[EXPORT_EXTENSION])
        .set_file_name(DEFAULT_FILE_NAME);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(panel_failed)?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Shows an open panel for an export; the chosen path, or `null` when cancelled.
#[tauri::command]
pub async fn pick_transfer_file(app: AppHandle) -> Result<Option<String>, CommandError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("RemoteDesktop export", &[EXPORT_EXTENSION]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(panel_failed)?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Writes the picked entries and credentials to `path`; encrypted (with the passwords) when `password` is
/// given.
#[tauri::command]
pub async fn export_library(
    core: State<'_, Core>,
    path: PathBuf,
    entry_ids: Vec<String>,
    credential_ids: Vec<String>,
    password: Option<String>,
) -> Result<ExportResultDto, CommandError> {
    blocking(&core, move |c| {
        c.export_library(&path, &entry_ids, &credential_ids, password.as_deref())
    })
    .await
}

/// Reads an export: secret-free rows with conflict flags.
#[tauri::command]
pub async fn transfer_preview(
    core: State<'_, Core>,
    path: PathBuf,
    password: Option<String>,
) -> Result<TransferPreviewDto, CommandError> {
    blocking(&core, move |c| c.transfer_preview(&path, password.as_deref())).await
}

/// Imports an export with a decision per row; returns the counts.
#[tauri::command]
pub async fn transfer_commit(
    app: AppHandle,
    core: State<'_, Core>,
    path: PathBuf,
    password: Option<String>,
    decisions: Vec<TransferDecisionDto>,
) -> Result<TransferResultDto, CommandError> {
    let result = blocking(&core, move |c| {
        c.transfer_commit(&path, password.as_deref(), &decisions)
    })
    .await?;
    emit_library(&app, &core);
    Ok(result)
}
