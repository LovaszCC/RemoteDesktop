//! Update commands (Swift `UpdateModel`): the launch check and "Check now" ask the Tauri updater for
//! `latest.json`; Install downloads and verifies the signed artifact with `update-progress` events; Relaunch
//! restarts through the quit confirmation.
//!
//! The pending update stays in Rust: the webview has no updater plugin permissions, only these commands.

use std::sync::Mutex;

use rd_core::Timestamp;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use super::{Core, SETTINGS_CHANGED};
use crate::core::CommandError;
use crate::updates::{ProgressThrottle, UpdateCheckDto, configured_pubkey, pubkey_configured};

/// Event: download progress of `update_install` ([`UpdateProgressDto`](crate::updates::UpdateProgressDto)).
pub const UPDATE_PROGRESS: &str = "update-progress";

/// The update found by the last check, waiting for Install.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

impl PendingUpdate {
    fn set(&self, update: Option<Update>) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = update;
    }

    fn get(&self) -> Option<Update> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Whether this build carries a real updater public key.
fn configured(app: &AppHandle) -> bool {
    configured_pubkey(&serde_json::to_value(&app.config().plugins).unwrap_or_default())
        .is_some_and(pubkey_configured)
}

fn update_error(code: &str, message: &str, error: &impl ToString) -> CommandError {
    CommandError::new(code, message).with_detail(error.to_string())
}

/// Checks for a newer release. `automatic` (the launch check) only runs when enabled and due, and is
/// `skipped` otherwise. A completed check updates `lastUpdateCheck` and emits `settings-changed`.
/// Errors: `updateCheckFailed` (offline, no release yet, bad `latest.json`, …).
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    core: State<'_, Core>,
    pending: State<'_, PendingUpdate>,
    automatic: bool,
) -> Result<UpdateCheckDto, CommandError> {
    let current_version = current_version(&app);
    if !configured(&app) {
        return Ok(UpdateCheckDto::NotConfigured { current_version });
    }
    if automatic && !core.update_check_due(Timestamp::now()) {
        return Ok(UpdateCheckDto::Skipped);
    }
    let core_for_exit = std::sync::Arc::downgrade(&core);
    let updater = app
        .updater_builder()
        // Windows runs the installer and exits the process right away: release the database lock first.
        .on_before_exit(move || {
            if let Some(core) = core_for_exit.upgrade() {
                core.close();
            }
        })
        .build()
        .map_err(|e| update_error("updateCheckFailed", "Couldn't check for updates.", &e))?;
    let found = updater
        .check()
        .await
        .map_err(|e| update_error("updateCheckFailed", "Couldn't check for updates.", &e))?;
    if let Ok(settings) = core.mark_update_checked(Timestamp::now()) {
        let _ = app.emit(SETTINGS_CHANGED, &settings);
    }
    let result = match &found {
        Some(update) => UpdateCheckDto::Available {
            current_version,
            version: update.version.clone(),
            notes: update.body.clone().filter(|body| !body.trim().is_empty()),
            date: update
                .date
                .and_then(|date| date.format(&time::format_description::well_known::Rfc3339).ok()),
        },
        None => UpdateCheckDto::UpToDate { current_version },
    };
    pending.set(found);
    Ok(result)
}

/// Downloads, verifies (minisign) and installs the update found by the last check, emitting
/// `update-progress`. On Windows the installer takes over and the app exits; elsewhere `update_relaunch`
/// starts the new version. Errors: `noUpdate`, `updateInstallFailed` (download, signature, install).
#[tauri::command]
pub async fn update_install(app: AppHandle, pending: State<'_, PendingUpdate>) -> Result<(), CommandError> {
    let update = pending
        .get()
        .ok_or_else(|| CommandError::new("noUpdate", "There is no update to install."))?;
    let mut throttle = ProgressThrottle::default();
    let progress = app.clone();
    update
        .download_and_install(
            move |length, total| {
                if let Some(event) = throttle.chunk(length, total) {
                    let _ = progress.emit(UPDATE_PROGRESS, event);
                }
            },
            || {},
        )
        .await
        .map_err(|e| update_error("updateInstallFailed", "The update couldn't be installed.", &e))?;
    pending.set(None);
    Ok(())
}

/// Restarts into the installed version; while sessions run, the quit confirmation asks first.
#[tauri::command]
pub fn update_relaunch(app: AppHandle) {
    crate::quit::request_relaunch(&app);
}

/// Registers the pending-update state.
pub fn install_state(app: &AppHandle) {
    app.manage(PendingUpdate::default());
}
