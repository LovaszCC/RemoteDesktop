//! Commands the webview can invoke. Each one is a thin wrapper: it runs an [`AppCore`] method (off the
//! main thread when it may block) and emits the events that keep the UI in sync.
//!
//! Events:
//! - `app-status-changed` ([`AppStatusDto`](crate::dto::AppStatusDto)) after open, unlock, lock and
//!   database switches;
//! - `library-changed` ([`LibraryDto`](crate::dto::LibraryDto)) after every change to the library and
//!   after unlocking or locking;
//! - `settings-changed` ([`SettingsDto`](crate::dto::SettingsDto)) and `shortcuts-changed`
//!   (`ShortcutDto[]`) after settings writes;
//! - `menu-command` ([`MenuCommand`](crate::menu::MenuCommand)) when a native menu item is chosen;
//! - `ssh-event`, `askpass-prompt` and `askpass-closed` for SSH sessions (see [`ssh`]);
//! - `port-forward-status`, `host-key-prompt` and `host-key-closed` for port forwards (see [`forwards`]);
//!   `host-key-prompt` also for "Set up SSH key login" (see [`keys`]);
//! - `files-status`, `files-host-key-prompt`, `files-progress`, `files-conflict` and `files-done` for the
//!   file browser windows (see [`files`]);
//! - `launch-notice` when an external launch falls back later (see [`launch`]);
//! - `rdp-event`, `rdp-cert-prompt`, `rdp-credentials-prompt` and `rdp-prompt-closed` for embedded RDP
//!   sessions (see [`rdp`]);
//! - `update-progress` while an update downloads (see [`updates`]).
//!
//! Status, language and shortcut changes also rebuild the native menu.

// Tauri deserializes command arguments into owned values and passes `State`/`AppHandle` by value.
#![allow(clippy::needless_pass_by_value)]

pub mod database;
pub mod files;
pub mod forwards;
pub mod import;
pub mod keys;
pub mod launch;
pub mod library;
pub mod rdp;
pub mod session;
pub mod settings;
pub mod ssh;
pub mod updates;

use std::sync::Arc;

use rd_platform::APP_IDENTITY;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::{AppCore, CommandError};

/// The managed state.
pub type Core = Arc<AppCore>;

/// Event: the root view's state changed.
pub const APP_STATUS_CHANGED: &str = "app-status-changed";
/// Event: the library changed.
pub const LIBRARY_CHANGED: &str = "library-changed";
/// Event: the settings changed.
pub const SETTINGS_CHANGED: &str = "settings-changed";
/// Event: the shortcuts changed.
pub const SHORTCUTS_CHANGED: &str = "shortcuts-changed";

/// Runs `work` on the blocking pool (PBKDF2, key files, SQLite) so the main thread stays responsive.
async fn blocking<T: Send + 'static>(
    core: &Core,
    work: impl FnOnce(&AppCore) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    let core = Arc::clone(core);
    tauri::async_runtime::spawn_blocking(move || work(&core))
        .await
        .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?
}

/// Emits the status and the library (empty while locked), and updates the menu's enabled items.
fn emit_status(app: &AppHandle, core: &AppCore) {
    let _ = app.emit(APP_STATUS_CHANGED, core.status());
    emit_library(app, core);
    crate::menu::refresh(app);
}

/// Emits the library.
fn emit_library(app: &AppHandle, core: &AppCore) {
    let _ = app.emit(LIBRARY_CHANGED, core.library());
}

/// Reply to [`ping`]; proves the command bridge and DTO serialization work end to end.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PingReply {
    /// Product name of this build.
    pub product_name: &'static str,
    /// This build's version (a beta build's includes `-beta.N`).
    pub version: String,
}

fn ping_reply(version: String) -> PingReply {
    PingReply {
        product_name: APP_IDENTITY.product_name,
        version,
    }
}

/// Health check used by the UI on startup.
#[tauri::command]
pub fn ping(app: AppHandle) -> PingReply {
    ping_reply(app.package_info().version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_serializes_camel_case() {
        let json = serde_json::to_value(ping_reply("1.6.7-beta.3".into())).unwrap();
        assert_eq!(json["productName"], "RemoteDesktop");
        assert_eq!(json["version"], "1.6.7-beta.3");
    }
}
