//! CSV import: the sample's save panel, the open panel, the preview and the commit. The commit emits
//! `library-changed`.

use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{Core, blocking, emit_library};
use crate::core::CommandError;
use crate::dto::{CsvPreviewDto, TransferDecisionDto, TransferResultDto};

/// The file name the sample's save panel suggests.
const SAMPLE_FILE_NAME: &str = "RemoteDesktop-sample.csv";

fn panel_failed(e: impl ToString) -> CommandError {
    CommandError::new("internal", "The command failed.").with_detail(e.to_string())
}

/// Asks where to save the sample CSV and writes it; `false` when cancelled.
#[tauri::command]
pub async fn save_csv_sample(app: AppHandle, core: State<'_, Core>) -> Result<bool, CommandError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(SAMPLE_FILE_NAME);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(panel_failed)?;
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(false);
    };
    blocking(&core, move |c| c.save_csv_sample(&path)).await?;
    Ok(true)
}

/// Shows an open panel for a CSV file; the chosen path, or `null` when cancelled.
#[tauri::command]
pub async fn pick_csv_file(app: AppHandle) -> Result<Option<String>, CommandError> {
    let dialog = app.dialog().file().add_filter("CSV", &["csv", "txt"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(panel_failed)?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Reads a CSV file: its entries with conflict flags and the rows left out.
#[tauri::command]
pub async fn csv_preview(core: State<'_, Core>, path: PathBuf) -> Result<CsvPreviewDto, CommandError> {
    blocking(&core, move |c| c.csv_preview(&path)).await
}

/// Imports a CSV file with a decision per row; returns the counts.
#[tauri::command]
pub async fn csv_commit(
    app: AppHandle,
    core: State<'_, Core>,
    path: PathBuf,
    decisions: Vec<TransferDecisionDto>,
) -> Result<TransferResultDto, CommandError> {
    let result = blocking(&core, move |c| c.csv_commit(&path, &decisions)).await?;
    emit_library(&app, &core);
    Ok(result)
}
