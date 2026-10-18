//! Settings and keyboard shortcuts.

use rd_core::Timestamp;
use rd_core::shortcuts::{KeyShortcut, ShortcutCommand, ShortcutSettings};
use serde::Deserialize;

use super::{AppCore, CommandError, Inner, account};
use crate::dto::{SettingsDto, ShortcutDto, shortcut_rows};

/// The languages the UI ships.
pub const LANGUAGES: [&str; 2] = ["en", "hu"];

/// The settings the UI may change; absent fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    /// `en` or `hu`.
    pub language: Option<String>,
    /// Keep the vault key in the OS keyring.
    pub remember_master_key: Option<bool>,
    /// External terminal id; `""` means the OS default.
    pub external_terminal: Option<String>,
    /// Check for updates on launch.
    pub check_for_updates_automatically: Option<bool>,
    /// Expanded sidebar folders.
    pub expanded_folders: Option<Vec<String>>,
    /// WinBox path; `""` means auto-detect.
    pub winbox_path: Option<String>,
}

impl AppCore {
    fn settings_dto(inner: &Inner) -> SettingsDto {
        let settings = &inner.settings;
        SettingsDto {
            database_path: inner
                .db_path
                .clone()
                .or_else(|| settings.database_path())
                .map(|p| p.display().to_string()),
            language: settings.language.clone(),
            remember_master_key: settings.remember_master_key,
            external_terminal: settings.external_terminal.clone(),
            check_for_updates_automatically: settings.check_for_updates_automatically,
            last_update_check: settings.last_update_check,
            expanded_folders: settings.expanded_folders.clone(),
            winbox_path: settings.winbox_path.as_ref().map(|p| p.display().to_string()),
        }
    }

    fn save_settings(inner: &Inner) -> Result<(), CommandError> {
        inner
            .settings
            .save(&inner.settings_dir)
            .map_err(|e| CommandError::settings_write(&e))
    }

    /// The current settings.
    #[must_use]
    pub fn settings_get(&self) -> SettingsDto {
        Self::settings_dto(&self.inner())
    }

    /// Applies and saves a patch. Turning "remember" on or off while unlocked updates the keyring at once.
    ///
    /// # Errors
    /// `invalidLanguage` or `settingsWrite`; nothing changes on `invalidLanguage`.
    pub fn settings_set(&self, patch: &SettingsPatch) -> Result<SettingsDto, CommandError> {
        let mut inner = self.inner();
        if let Some(language) = &patch.language
            && !LANGUAGES.contains(&language.as_str())
        {
            return Err(CommandError::new("invalidLanguage", "Unknown language.").with_detail(language));
        }
        if let Some(language) = &patch.language {
            inner.settings.language.clone_from(language);
        }
        if let Some(remember) = patch.remember_master_key {
            inner.settings.remember_master_key = remember;
            if let (Some(vault), Some(path)) = (&inner.vault, &inner.db_path) {
                let account = account(path);
                let _ = if remember {
                    self.keys.save(&account, &vault.export_key())
                } else {
                    self.keys.delete(&account)
                };
            }
        }
        if let Some(terminal) = &patch.external_terminal {
            inner.settings.external_terminal = (!terminal.is_empty()).then(|| terminal.clone());
        }
        if let Some(check) = patch.check_for_updates_automatically {
            inner.settings.check_for_updates_automatically = check;
        }
        if let Some(expanded) = &patch.expanded_folders {
            let mut expanded = expanded.clone();
            expanded.sort();
            expanded.dedup();
            inner.settings.expanded_folders = expanded;
        }
        if let Some(winbox) = &patch.winbox_path {
            let winbox = winbox.trim();
            inner.settings.winbox_path = (!winbox.is_empty()).then(|| std::path::PathBuf::from(winbox));
        }
        Self::save_settings(&inner)?;
        Ok(Self::settings_dto(&inner))
    }

    /// Whether the launch update check should run at `now`: automatic checks on and the last check at
    /// least 24 hours ago (Swift `UpdateSchedule.isDue`).
    #[must_use]
    pub fn update_check_due(&self, now: Timestamp) -> bool {
        rd_update::schedule::is_due(&self.inner().settings, now)
    }

    /// Records a completed update check (`lastUpdateCheck`) and returns the settings.
    ///
    /// # Errors
    /// `settingsWrite`.
    pub fn mark_update_checked(&self, now: Timestamp) -> Result<SettingsDto, CommandError> {
        let mut inner = self.inner();
        rd_update::schedule::mark_checked(&mut inner.settings, now);
        Self::save_settings(&inner)?;
        Ok(Self::settings_dto(&inner))
    }

    fn shortcut_settings(&self, inner: &Inner) -> ShortcutSettings {
        ShortcutSettings::from_json(self.platform, &inner.settings.keyboard_shortcuts)
    }

    fn store_shortcuts(
        inner: &mut Inner,
        shortcuts: &ShortcutSettings,
    ) -> Result<Vec<ShortcutDto>, CommandError> {
        inner.settings.keyboard_shortcuts = shortcuts.to_json();
        Self::save_settings(inner)?;
        Ok(shortcut_rows(shortcuts))
    }

    /// Every command's effective shortcut for this platform.
    #[must_use]
    pub fn shortcuts_get(&self) -> Vec<ShortcutDto> {
        shortcut_rows(&self.shortcut_settings(&self.inner()))
    }

    /// Sets, or with `None` removes, a command's shortcut.
    ///
    /// # Errors
    /// `unknownCommand`, a validation code (`superKey`, `missingModifier`, `altGr`, `reserved`, `inUse`
    /// with the owning command as detail) or `settingsWrite`.
    pub fn shortcut_set(
        &self,
        command: &str,
        shortcut: Option<KeyShortcut>,
    ) -> Result<Vec<ShortcutDto>, CommandError> {
        let command = parse_command(command)?;
        let mut inner = self.inner();
        let mut shortcuts = self.shortcut_settings(&inner);
        let shortcut = shortcut.map(|s| KeyShortcut::new(&s.key, s.modifiers));
        shortcuts
            .set(shortcut, command)
            .map_err(|e| CommandError::shortcut(e, self.platform))?;
        Self::store_shortcuts(&mut inner, &shortcuts)
    }

    /// Restores a command's default shortcut.
    ///
    /// # Errors
    /// `unknownCommand`, `inUse` when another command has the default now, or `settingsWrite`.
    pub fn shortcut_reset(&self, command: &str) -> Result<Vec<ShortcutDto>, CommandError> {
        let command = parse_command(command)?;
        let mut inner = self.inner();
        let mut shortcuts = self.shortcut_settings(&inner);
        shortcuts
            .reset(command)
            .map_err(|e| CommandError::shortcut(e, self.platform))?;
        Self::store_shortcuts(&mut inner, &shortcuts)
    }

    /// Restores every default.
    ///
    /// # Errors
    /// `settingsWrite`.
    pub fn shortcuts_reset_all(&self) -> Result<Vec<ShortcutDto>, CommandError> {
        let mut inner = self.inner();
        let mut shortcuts = self.shortcut_settings(&inner);
        shortcuts.reset_all();
        Self::store_shortcuts(&mut inner, &shortcuts)
    }
}

fn parse_command(command: &str) -> Result<ShortcutCommand, CommandError> {
    command
        .parse()
        .map_err(|_| CommandError::new("unknownCommand", "Unknown command.").with_detail(command))
}
