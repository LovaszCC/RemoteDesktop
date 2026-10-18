//! Vault commands: status, create, unlock, lock, change password, and the quit hook.

use tauri::{AppHandle, State};

use super::{Core, blocking, emit_status};
use crate::core::CommandError;
use crate::dto::AppStatusDto;

/// The root view's state.
#[tauri::command]
pub fn app_status(core: State<'_, Core>) -> AppStatusDto {
    core.status()
}

/// Sets up the vault of a new database. Errors: `noDatabase`, `readOnly`, `emptyPassword`.
#[tauri::command]
pub async fn create_vault(
    app: AppHandle,
    core: State<'_, Core>,
    password: String,
    remember: bool,
) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, move |c| c.create_vault(&password, remember)).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Unlocks with the master password. Errors: `wrongPassword`, `noDatabase`.
#[tauri::command]
pub async fn unlock(
    app: AppHandle,
    core: State<'_, Core>,
    password: String,
    remember: bool,
) -> Result<AppStatusDto, CommandError> {
    let status = blocking(&core, move |c| c.unlock(&password, remember)).await?;
    emit_status(&app, &core);
    Ok(status)
}

/// Locks and forgets the remembered key.
#[tauri::command]
pub fn lock(app: AppHandle, core: State<'_, Core>) -> AppStatusDto {
    let status = core.lock();
    emit_status(&app, &core);
    status
}

/// Changes the master password. Errors: `wrongPassword`, `emptyPassword`, `locked`, `readOnly`.
#[tauri::command]
pub async fn change_password(
    core: State<'_, Core>,
    current: String,
    new: String,
) -> Result<(), CommandError> {
    blocking(&core, move |c| c.change_password(&current, &new)).await
}

/// How many sessions, forwards and transfers would be closed by quitting.
#[tauri::command]
pub fn running_count(core: State<'_, Core>) -> usize {
    core.running_count()
}
