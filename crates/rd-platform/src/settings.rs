//! App settings in `settings.json` under the OS config directory, and the default database location.
//!
//! Every path derives from [`APP_IDENTITY`], so no other code names a folder.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rd_core::Timestamp;
use serde::{Deserialize, Deserializer, Serialize};

use crate::APP_IDENTITY;

/// The settings file name inside [`config_dir`].
pub const SETTINGS_FILE: &str = "settings.json";

/// The database file name inside the default data folder.
pub const DATABASE_FILE: &str = "connections.sqlite";

/// User settings. Missing keys take their defaults, so older files keep loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The database file; `None` means [`default_database_path`].
    pub database_path: Option<PathBuf>,
    /// Keep the vault key in the OS keyring after unlocking.
    pub remember_master_key: bool,
    /// The external terminal id for SSH connections set to "external"; `None` means the OS default.
    pub external_terminal: Option<String>,
    /// Shortcut overrides, `{command: {key, modifiers} | null}` (see `rd_core::shortcuts`).
    pub keyboard_shortcuts: serde_json::Value,
    /// Check for updates on launch.
    pub check_for_updates_automatically: bool,
    /// When updates were last checked; an unreadable value counts as never.
    #[serde(deserialize_with = "lenient_timestamp")]
    pub last_update_check: Option<Timestamp>,
    /// UI language code (`en`, `hu`).
    pub language: String,
    /// Folder ids expanded in the sidebar.
    pub expanded_folders: Vec<String>,
    /// WinBox: an `.app`, an executable or (Linux) a `winbox64.exe` for Wine; `None` means auto-detect.
    pub winbox_path: Option<PathBuf>,
    /// What a single click on a sidebar entry with an open connection shows: `details` or `tab` (its only
    /// plain tab).
    pub selection_shows: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            database_path: None,
            remember_master_key: true,
            external_terminal: None,
            keyboard_shortcuts: serde_json::Value::Object(serde_json::Map::new()),
            check_for_updates_automatically: true,
            last_update_check: None,
            language: "en".to_owned(),
            expanded_folders: Vec::new(),
            winbox_path: None,
            selection_shows: "details".to_owned(),
        }
    }
}

fn lenient_timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Timestamp>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_str().and_then(|text| Timestamp::parse(text).ok()))
}

impl Settings {
    /// Reads `settings.json` from `dir`. A missing, unreadable or corrupt file gives the defaults.
    #[must_use]
    pub fn load(dir: &Path) -> Self {
        fs::read(dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes `settings.json` into `dir`, creating it if needed. The file is replaced atomically: written
    /// to a temporary file next to it, then renamed over it.
    ///
    /// # Errors
    /// If the directory or the file can't be written.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let temp = dir.join(format!("{SETTINGS_FILE}.tmp"));
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        fs::rename(&temp, dir.join(SETTINGS_FILE)).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
    }

    /// The database to open: the configured path, else [`default_database_path`].
    #[must_use]
    pub fn database_path(&self) -> Option<PathBuf> {
        self.database_path.clone().or_else(default_database_path)
    }
}

/// This build's config directory: the OS config dir joined with the app identifier. `None` if the OS has
/// no config dir for the user.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(APP_IDENTITY.identifier))
}

/// The default database: `<OS data dir>/<data_dir_name>/connections.sqlite`. `None` if the OS has no data
/// dir for the user.
#[must_use]
pub fn default_database_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_IDENTITY.data_dir_name).join(DATABASE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let settings = Settings::default();
        assert!(settings.remember_master_key);
        assert!(settings.check_for_updates_automatically);
        assert_eq!(settings.language, "en");
        assert_eq!(settings.keyboard_shortcuts, serde_json::json!({}));
        assert_eq!(settings.database_path, None);
        assert!(settings.expanded_folders.is_empty());
        assert_eq!(settings.winbox_path, None);
    }

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        fs::write(dir.path().join(SETTINGS_FILE), "{not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        assert_eq!(Settings::load(&dir.path().join("missing")), Settings::default());
    }

    #[test]
    fn round_trips_through_a_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("config");
        let settings = Settings {
            database_path: Some(PathBuf::from("/tmp/example/connections.sqlite")),
            remember_master_key: false,
            external_terminal: Some("example-terminal".into()),
            keyboard_shortcuts: serde_json::json!({"lock": null}),
            check_for_updates_automatically: false,
            last_update_check: Some(Timestamp::from_unix(1_767_225_600)),
            language: "hu".into(),
            expanded_folders: vec!["A".into(), "B".into()],
            winbox_path: Some(PathBuf::from("/opt/winbox/WinBox")),
            selection_shows: "tab".into(),
        };
        settings.save(&nested).unwrap();
        assert_eq!(Settings::load(&nested), settings);
        assert!(!nested.join(format!("{SETTINGS_FILE}.tmp")).exists());

        let mut changed = settings.clone();
        changed.language = "en".into();
        changed.save(&nested).unwrap();
        assert_eq!(Settings::load(&nested), changed);
    }

    #[test]
    fn uses_camel_case_keys_and_fills_missing_ones() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "checkForUpdatesAutomatically",
                "databasePath",
                "expandedFolders",
                "externalTerminal",
                "keyboardShortcuts",
                "language",
                "lastUpdateCheck",
                "rememberMasterKey",
                "selectionShows",
                "winboxPath"
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"language":"hu","lastUpdateCheck":42,"unknownKey":1}"#,
        )
        .unwrap();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.language, "hu");
        assert_eq!(loaded.last_update_check, None);
        assert!(loaded.remember_master_key);
    }

    #[test]
    fn paths_derive_from_the_identity() {
        if let Some(dir) = config_dir() {
            assert!(dir.ends_with(APP_IDENTITY.identifier));
        }
        if let Some(path) = default_database_path() {
            assert!(path.ends_with(Path::new(APP_IDENTITY.data_dir_name).join(DATABASE_FILE)));
        }
        let custom = Settings {
            database_path: Some(PathBuf::from("/tmp/example/other.sqlite")),
            ..Settings::default()
        };
        assert_eq!(
            custom.database_path(),
            Some(PathBuf::from("/tmp/example/other.sqlite"))
        );
        assert_eq!(Settings::default().database_path(), default_database_path());
    }
}
