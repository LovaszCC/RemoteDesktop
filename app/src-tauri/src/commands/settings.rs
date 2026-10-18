//! Settings and shortcut commands.

use rd_core::shortcuts::KeyShortcut;
use tauri::{AppHandle, Emitter, State};

use super::{Core, SETTINGS_CHANGED, SHORTCUTS_CHANGED};
use crate::core::{CommandError, SettingsPatch};
use crate::dto::{SettingsDto, ShortcutDto};

/// The current settings.
#[tauri::command]
pub fn settings_get(core: State<'_, Core>) -> SettingsDto {
    core.settings_get()
}

/// Applies a partial settings change. Errors: `invalidLanguage`, `settingsWrite`.
#[tauri::command]
pub fn settings_set(
    app: AppHandle,
    core: State<'_, Core>,
    patch: SettingsPatch,
) -> Result<SettingsDto, CommandError> {
    let settings = core.settings_set(&patch)?;
    let _ = app.emit(SETTINGS_CHANGED, &settings);
    if patch.language.is_some() {
        crate::menu::refresh(&app);
    }
    Ok(settings)
}

fn shortcuts_changed(
    app: &AppHandle,
    rows: Result<Vec<ShortcutDto>, CommandError>,
) -> Result<Vec<ShortcutDto>, CommandError> {
    let rows = rows?;
    let _ = app.emit(SHORTCUTS_CHANGED, &rows);
    crate::menu::refresh(app);
    Ok(rows)
}

/// Every command's effective shortcut with display strings for this OS.
#[tauri::command]
pub fn shortcuts_get(core: State<'_, Core>) -> Vec<ShortcutDto> {
    core.shortcuts_get()
}

/// Sets (or with `null` removes) a shortcut. Errors: `unknownCommand`, `superKey`, `missingModifier`,
/// `altGr`, `reserved`, `inUse` (detail: the command that has it).
#[tauri::command]
pub fn shortcut_set(
    app: AppHandle,
    core: State<'_, Core>,
    command: String,
    shortcut: Option<KeyShortcut>,
) -> Result<Vec<ShortcutDto>, CommandError> {
    shortcuts_changed(&app, core.shortcut_set(&command, shortcut))
}

/// Restores one default. Errors: `unknownCommand`, `inUse`.
#[tauri::command]
pub fn shortcut_reset(
    app: AppHandle,
    core: State<'_, Core>,
    command: String,
) -> Result<Vec<ShortcutDto>, CommandError> {
    shortcuts_changed(&app, core.shortcut_reset(&command))
}

/// Restores every default.
#[tauri::command]
pub fn shortcuts_reset_all(app: AppHandle, core: State<'_, Core>) -> Result<Vec<ShortcutDto>, CommandError> {
    shortcuts_changed(&app, core.shortcuts_reset_all())
}
