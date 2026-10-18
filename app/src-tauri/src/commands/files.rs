//! File browser commands. Each browser has its own window, labelled with its browser id
//! (`files-<connection id>`, 1100×680), whose page shows the Files view for that label. OS file drops are
//! enabled for these windows only (the main window keeps `dragDropEnabled: false`).
//!
//! Events (emitted to the browser's window; payloads carry `browserId`):
//! - `files-status` ([`FilesStatusDto`]): connecting, ready (with the mode) or failed;
//! - `files-host-key-prompt` ([`FilesHostKeyDto`](crate::core::FilesHostKeyDto)): an unknown host key,
//!   answered with `host_key_answer`;
//! - `files-progress` ([`FilesProgressDto`](crate::core::FilesProgressDto)): the running transfer's item
//!   and bytes;
//! - `files-conflict` ([`FilesConflictDto`](crate::core::FilesConflictDto)): "already exists", answered
//!   with [`files_conflict_answer`];
//! - `files-done` ([`FilesDoneDto`](crate::core::FilesDoneDto)): per item results.

use std::sync::Arc;

use rd_ssh::files::{ConflictChoice, Direction};
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewUrl, WebviewWindowBuilder};

use super::{Core, blocking};
use crate::core::{
    AppCore, CommandError, FilesNotice, FilesNotifier, FilesOpenDto, FilesStatusDto, ListingDto,
    TransferInput,
};
use crate::menu::tr;

/// Event: a browser's state changed.
pub const FILES_STATUS: &str = "files-status";
/// Event: an unknown host key needs an answer.
pub const FILES_HOST_KEY_PROMPT: &str = "files-host-key-prompt";
/// Event: transfer progress.
pub const FILES_PROGRESS: &str = "files-progress";
/// Event: a destination already exists.
pub const FILES_CONFLICT: &str = "files-conflict";
/// Event: a transfer finished.
pub const FILES_DONE: &str = "files-done";

/// The Files window's size.
pub const WINDOW_SIZE: (f64, f64) = (1100.0, 680.0);

fn emit_to<S: serde::Serialize + Clone>(app: &AppHandle, label: &str, event: &str, payload: S) {
    let _ = app.emit_to(EventTarget::webview_window(label), event, payload);
}

/// Connects the file browser notices to events and windows. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: FilesNotifier = Arc::new(move |notice| match notice {
        FilesNotice::Status(status) => emit_to(&handle, &status.browser_id.clone(), FILES_STATUS, status),
        FilesNotice::HostKeyPrompt(prompt) => {
            emit_to(&handle, &prompt.browser_id.clone(), FILES_HOST_KEY_PROMPT, prompt);
        }
        FilesNotice::Conflict(conflict) => {
            emit_to(&handle, &conflict.browser_id.clone(), FILES_CONFLICT, conflict);
        }
        FilesNotice::Progress(progress) => {
            emit_to(&handle, &progress.browser_id.clone(), FILES_PROGRESS, progress);
        }
        FilesNotice::Done(done) => emit_to(&handle, &done.browser_id.clone(), FILES_DONE, done),
        FilesNotice::Closed(label) => {
            if let Some(window) = handle.get_webview_window(&label) {
                let _ = window.destroy();
            }
        }
    });
    core.set_files_hooks(notifier);
}

/// Opens the file browser of an SSH or `MikroTik` connection in its own window, or focuses the open one.
/// `startPath` wins; else the shell directory of the terminal `sessionId`; else the remote home folder.
/// Errors: `locked`, `notFound`, `notSsh`, `emptyHost`, `windowFailed`.
#[tauri::command]
pub async fn files_open(
    app: AppHandle,
    core: State<'_, Core>,
    connection_id: String,
    start_path: Option<String>,
    session_id: Option<String>,
) -> Result<FilesOpenDto, CommandError> {
    let opened = blocking(&core, move |c| {
        c.files_open(&connection_id, start_path, session_id.as_deref())
    })
    .await?;
    if let Some(window) = app.get_webview_window(&opened.browser_id) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(opened);
    }
    let language = core.settings_get().language;
    let title = tr(&language, "Files – %@").replace("%@", &opened.connection_name);
    let built = WebviewWindowBuilder::new(&app, &opened.browser_id, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_inner_size(820.0, 460.0)
        .build();
    if let Err(error) = built {
        core.files_close(&opened.browser_id);
        return Err(
            CommandError::new("windowFailed", "Couldn't open the window.").with_detail(error.to_string())
        );
    }
    Ok(opened)
}

/// The browser's state (the window asks on load). Errors: `browserNotFound`.
#[tauri::command]
pub fn files_info(core: State<'_, Core>, browser_id: String) -> Result<FilesStatusDto, CommandError> {
    core.files_info(&browser_id)
}

/// Connects a failed browser again. Errors: `browserNotFound`, `locked`, `notFound`.
#[tauri::command]
pub async fn files_reconnect(core: State<'_, Core>, browser_id: String) -> Result<(), CommandError> {
    blocking(&core, move |c| c.files_reconnect(&browser_id)).await
}

/// Lists a remote folder. Errors: `browserNotFound`, `notConnected`, `listFailed`, `unreadable`, `ssh`.
#[tauri::command]
pub async fn files_list_remote(
    core: State<'_, Core>,
    browser_id: String,
    path: String,
    show_hidden: bool,
) -> Result<ListingDto, CommandError> {
    let core = Arc::clone(&core);
    core.files_list_remote(&browser_id, &path, show_hidden).await
}

/// Lists a local folder (empty: Downloads). Errors: `listFailed`.
#[tauri::command]
pub async fn files_list_local(
    core: State<'_, Core>,
    path: Option<String>,
    show_hidden: bool,
) -> Result<ListingDto, CommandError> {
    blocking(&core, move |_| {
        AppCore::files_list_local(path.as_deref(), show_hidden)
    })
    .await
}

/// Starts a transfer; returns its id. Errors: `browserNotFound`, `notConnected`, `transferRunning`,
/// `nothingToCopy`.
#[tauri::command]
pub fn files_transfer(
    core: State<'_, Core>,
    browser_id: String,
    direction: Direction,
    sources: Vec<String>,
    destination: String,
    gzip: bool,
) -> Result<String, CommandError> {
    core.files_transfer(
        &browser_id,
        TransferInput {
            direction,
            sources,
            destination,
            gzip,
        },
    )
}

/// Cancels the browser's transfer.
#[tauri::command]
pub fn files_cancel(core: State<'_, Core>, browser_id: String) {
    core.files_cancel(&browser_id);
}

/// Answers a `files-conflict`.
#[tauri::command]
pub fn files_conflict_answer(
    core: State<'_, Core>,
    request_id: String,
    choice: ConflictChoice,
    apply_to_all: bool,
) {
    core.files_conflict_answer(&request_id, choice, apply_to_all);
}

/// The main window tells whether "Files (SCP)…" applies now (an SSH/`MikroTik` selection or a terminal
/// tab); the menu item follows.
#[tauri::command]
pub fn menu_files_target(app: AppHandle, core: State<'_, Core>, enabled: bool) {
    if core.set_files_target(enabled) {
        crate::menu::refresh(&app);
    }
}
