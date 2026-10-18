//! Port forward commands. Events:
//! - `port-forward-status` ([`ForwardStatusDto`]): `{id, connectionId, state, code?, error?}` whenever a
//!   tunnel starts, becomes active, fails or stops;
//! - `host-key-prompt` ([`HostKeyPromptDto`](crate::core::HostKeyPromptDto)): an unknown host key;
//!   answer with [`host_key_answer`];
//! - `host-key-closed` (`{requestId}`): the prompt's tunnel stopped, close its dialog.

use std::sync::Arc;

use rd_core::forward::PortForwardDraft;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::library::write;
use super::{Core, blocking};
use crate::core::{AppCore, CommandError, ForwardNotice, ForwardNotifier, ForwardStatusDto};

/// Event: a tunnel's state changed.
pub const PORT_FORWARD_STATUS: &str = "port-forward-status";
/// Event: an unknown host key needs an answer.
pub const HOST_KEY_PROMPT: &str = "host-key-prompt";
/// Event: a pending host key prompt is gone.
pub const HOST_KEY_CLOSED: &str = "host-key-closed";

/// Payload of [`HOST_KEY_CLOSED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyClosedDto {
    /// The closed request.
    pub request_id: String,
}

/// Connects the forward notices to events and the menu. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: ForwardNotifier = Arc::new(move |notice| {
        let _ = match notice {
            ForwardNotice::Status(status) => handle.emit(PORT_FORWARD_STATUS, status),
            ForwardNotice::HostKeyPrompt(prompt) => handle.emit(HOST_KEY_PROMPT, prompt),
            ForwardNotice::HostKeyClosed(request_id) => {
                handle.emit(HOST_KEY_CLOSED, HostKeyClosedDto { request_id })
            }
        };
    });
    core.set_forward_hooks(notifier);
}

/// Saves the forward editor; a running forward restarts. Returns the id. Errors: `locked`, `readOnly`,
/// `notFound`, `notSsh`, `invalidLocalPort`, `missingRemoteHost`, `invalidRemotePort`,
/// `duplicateLocalPort`.
#[tauri::command]
pub async fn port_forward_save(
    app: AppHandle,
    core: State<'_, Core>,
    forward: PortForwardDraft,
) -> Result<String, CommandError> {
    write(&app, &core, move |c| c.port_forward_save(&forward)).await
}

/// Stops and deletes a forward. Errors: `locked`, `readOnly`.
#[tauri::command]
pub async fn port_forward_delete(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
) -> Result<(), CommandError> {
    write(&app, &core, move |c| c.port_forward_delete(&id)).await
}

/// Starts a forward's tunnel; progress arrives as `port-forward-status`. Errors: `locked`, `notFound`,
/// `notSsh`, `emptyHost`.
#[tauri::command]
pub async fn port_forward_start(core: State<'_, Core>, id: String) -> Result<(), CommandError> {
    blocking(&core, move |c| c.port_forward_start(&id)).await
}

/// Stops a forward's tunnel.
#[tauri::command]
pub fn port_forward_stop(core: State<'_, Core>, id: String) {
    core.port_forward_stop(&id);
}

/// The states of the started forwards (running or failed).
#[tauri::command]
pub fn port_forward_statuses(core: State<'_, Core>) -> Vec<ForwardStatusDto> {
    core.port_forward_statuses()
}

/// Answers a `host-key-prompt`.
#[tauri::command]
pub fn host_key_answer(core: State<'_, Core>, request_id: String, accept: bool) {
    core.host_key_answer(&request_id, accept);
}
