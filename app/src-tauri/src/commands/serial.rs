//! Serial console commands. Output arrives on the `onData` channel like an SSH session's; the terminal
//! acknowledges it with [`serial_ack`]. The `exit` event (`ssh-event` with the session id, `code: null`)
//! means the port went away: the tab shows "Connection lost" and [`serial_reconnect`].

use std::sync::Arc;

use rd_serial::SerialSettings;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

use super::ssh::{InputData, OpenSessionDto, WebviewSink};
use super::{Core, blocking, emit_library};
use crate::core::{CommandError, SerialPortDto};

/// The plugged-in USB-serial adapters, with the tab holding each. Errors: `serialFailed`.
#[tauri::command]
pub async fn serial_ports(core: State<'_, Core>) -> Result<Vec<SerialPortDto>, CommandError> {
    blocking(&core, crate::core::AppCore::serial_ports).await
}

/// Opens a serial console on `port`: a saved connection's (`connection_id`) or ad-hoc `settings`.
/// `take_over` disconnects a tab holding the port. Errors: `locked`, `notFound`, `notSerial`,
/// `invalidSerialSettings`, `portInUse` (detail: the holder's title), `portMissing`, `portBusy`,
/// `portPermission`, `serialFailed`.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "Tauri command arguments")]
pub async fn open_serial_session(
    app: AppHandle,
    core: State<'_, Core>,
    connection_id: Option<String>,
    port: String,
    settings: Option<SerialSettings>,
    title: String,
    take_over: bool,
    on_data: Channel<InvokeResponseBody>,
) -> Result<OpenSessionDto, CommandError> {
    blocking(&core, move |c| {
        let session_id = c.open_serial_session(
            connection_id.as_deref(),
            &port,
            settings,
            &title,
            take_over,
            |session_id| {
                Arc::new(WebviewSink {
                    app,
                    session_id: session_id.to_owned(),
                    channel: on_data,
                })
            },
        )?;
        Ok(OpenSessionDto { session_id })
    })
    .await
}

/// Typed input. Errors: `sessionNotFound`, `sessionEnded`.
#[tauri::command]
pub fn serial_input(core: State<'_, Core>, session_id: String, data: InputData) -> Result<(), CommandError> {
    core.serial_input(&session_id, data.as_bytes())
}

/// The terminal parsed `bytes` of output. Errors: `sessionNotFound`.
#[tauri::command]
pub fn serial_ack(core: State<'_, Core>, session_id: String, bytes: usize) -> Result<(), CommandError> {
    core.serial_ack(&session_id, bytes)
}

/// Closes the port and deletes the tab's output file.
#[tauri::command]
pub async fn serial_close(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    blocking(&core, move |c| {
        c.serial_close(&session_id);
        Ok(())
    })
    .await
}

/// Opens the port again in the same tab: `port`, else the previous one. Errors: `sessionNotFound`,
/// `portMissing` (pick a port), and those of [`open_serial_session`].
#[tauri::command]
pub async fn serial_reconnect(
    core: State<'_, Core>,
    session_id: String,
    port: Option<String>,
    take_over: bool,
) -> Result<(), CommandError> {
    blocking(&core, move |c| {
        c.serial_reconnect(&session_id, port.as_deref(), take_over)
    })
    .await
}

/// The settings the tab runs with. Errors: `sessionNotFound`.
#[tauri::command]
pub fn serial_session_settings(
    core: State<'_, Core>,
    session_id: String,
) -> Result<SerialSettings, CommandError> {
    core.serial_session_settings(&session_id)
}

/// Applies new settings to the open port; with `save` also to the saved connection (emits the library).
/// Errors: `sessionNotFound`, `invalidSerialSettings`, `readOnly`, `locked`, `notFound`, `serialFailed`.
#[tauri::command]
pub async fn serial_apply_settings(
    app: AppHandle,
    core: State<'_, Core>,
    session_id: String,
    settings: SerialSettings,
    save: bool,
) -> Result<(), CommandError> {
    let changed = blocking(&core, move |c| {
        c.serial_apply_settings(&session_id, settings, save)
    })
    .await?;
    if changed {
        emit_library(&app, &core);
    }
    Ok(())
}

/// Puts the tab's whole output (plain text) on the clipboard. Errors: `sessionNotFound`, `serialFailed`.
#[tauri::command]
pub async fn serial_copy_output(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    let text = blocking(&core, move |c| c.serial_output_text(&session_id)).await?;
    crate::clipboard::copy_text(&text)
}

/// Writes the tab's whole output (plain text) to `path` (chosen in the save dialog). Errors:
/// `sessionNotFound`, `serialFailed`.
#[tauri::command]
pub async fn serial_save_output(
    core: State<'_, Core>,
    session_id: String,
    path: String,
) -> Result<(), CommandError> {
    blocking(&core, move |c| {
        let text = c.serial_output_text(&session_id)?;
        std::fs::write(&path, text).map_err(|e| {
            CommandError::new("serialFailed", "Couldn't save the output.").with_detail(format!("{path}: {e}"))
        })
    })
    .await
}
