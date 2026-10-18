//! Import commands: the file panel, the preview and the commit. Preview and commit refuse with `locked` or
//! `readOnly`; the commit emits `library-changed`.

use std::path::PathBuf;

use rd_core::import::ImportKind;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{Core, blocking, emit_library};
use crate::core::CommandError;
use crate::dto::{ImportPreviewDto, ImportResultDto};

/// Shows an open panel for an export of `kind` (`.rdm`/`.xml` or `.cdb`); the chosen path, or `null` when
/// cancelled.
#[tauri::command]
pub async fn pick_import_file(app: AppHandle, kind: ImportKind) -> Result<Option<String>, CommandError> {
    let name = match kind {
        ImportKind::Rdm => "Remote Desktop Manager",
        ImportKind::Winbox => "WinBox",
    };
    let dialog = app.dialog().file().add_filter(name, kind.extensions());
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Parses the file: secret-free rows with duplicate flags, the skipped entries and counts.
#[tauri::command]
pub async fn import_preview(
    core: State<'_, Core>,
    kind: ImportKind,
    path: PathBuf,
) -> Result<ImportPreviewDto, CommandError> {
    blocking(&core, move |c| c.import_preview(kind, &path)).await
}

/// Imports the rows whose ids are in `selected`; returns the counts.
#[tauri::command]
pub async fn import_commit(
    app: AppHandle,
    core: State<'_, Core>,
    kind: ImportKind,
    path: PathBuf,
    selected: Vec<String>,
) -> Result<ImportResultDto, CommandError> {
    let result = blocking(&core, move |c| c.import_commit(kind, &path, &selected)).await?;
    emit_library(&app, &core);
    Ok(result)
}
