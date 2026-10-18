//! Connections handed to other apps: the browser, WinBox and the system RDP client.
//!
//! Event: `launch-notice` ([`LaunchNoticeDto`](crate::core::LaunchNoticeDto)) when a launch falls back
//! after the command returned (the macOS autofill found no password field; the password is on the
//! clipboard).

use rd_platform::web::Browser;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use super::{Core, blocking};
use crate::core::{AppCore, CommandError, LaunchNotifier, LaunchOutcomeDto, WinboxStatusDto};

/// Opens a web connection in its browser, then copies the password and the username. Returns the outcome
/// for the toast. Errors: `locked`, `notFound`, `wrongType`, `invalidUrl`, `launchFailed`, `clipboard`.
#[tauri::command]
pub async fn connect_web(
    core: State<'_, Core>,
    connection_id: String,
) -> Result<LaunchOutcomeDto, CommandError> {
    blocking(&core, move |c| c.connect_web(&connection_id)).await
}

/// The browsers registered for `https` here (`[{id, name}]`, sorted by name).
#[tauri::command]
pub async fn installed_browsers() -> Vec<Browser> {
    tauri::async_runtime::spawn_blocking(AppCore::installed_browsers)
        .await
        .unwrap_or_default()
}

/// Starts WinBox for a MikroTik connection. Errors: `locked`, `notFound`, `wrongType`, `appNotInstalled`,
/// `launchFailed`.
#[tauri::command]
pub async fn connect_winbox(
    core: State<'_, Core>,
    connection_id: String,
) -> Result<LaunchOutcomeDto, CommandError> {
    blocking(&core, move |c| c.connect_winbox(&connection_id)).await
}

/// Where WinBox is, for Settings.
#[tauri::command]
pub async fn winbox_status(core: State<'_, Core>) -> Result<WinboxStatusDto, CommandError> {
    blocking(&core, |c| Ok(c.winbox_status())).await
}

/// Shows an open panel for the WinBox app or executable; the chosen path, or `null` when cancelled.
#[tauri::command]
pub async fn pick_winbox(app: AppHandle) -> Result<Option<String>, CommandError> {
    let mut dialog = app.dialog().file();
    if cfg!(target_os = "macos") {
        dialog = dialog.set_directory("/Applications");
    } else if let Some(home) = dirs::home_dir() {
        dialog = dialog.set_directory(home);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.display().to_string()))
}

/// Event: a launch fell back after the command returned.
pub const LAUNCH_NOTICE: &str = "launch-notice";

/// Sends the core's launch notices to the webview.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: LaunchNotifier = Arc::new(move |notice| {
        let _ = handle.emit(LAUNCH_NOTICE, notice);
    });
    core.set_launch_notifier(notifier);
}

/// Opens an RDP connection in the system RDP client. Errors: `locked`, `notFound`, `wrongType`,
/// `appNotInstalled`, `launchFailed`, `clipboard`.
#[tauri::command]
pub async fn connect_rdp_external(
    core: State<'_, Core>,
    connection_id: String,
) -> Result<LaunchOutcomeDto, CommandError> {
    blocking(&core, move |c| c.connect_rdp_external(&connection_id)).await
}

/// The macOS Accessibility permission for the RDP autofill (`true`/`false`); `null` on the other OSes.
#[tauri::command]
pub fn accessibility_status() -> Option<bool> {
    AppCore::accessibility_status()
}

/// Asks for the Accessibility permission and opens System Settings there (macOS). Errors: `launchFailed`.
#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), CommandError> {
    AppCore::open_accessibility_settings()
}
