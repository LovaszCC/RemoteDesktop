//! The one-time import of the Swift app's settings (its `UserDefaults`) on the first launch after the
//! handover.
//!
//! The Swift app keeps its settings in `~/Library/Preferences/<bundle id>.plist`, the standard domain of
//! the shared bundle id. The first time this app starts without a `settings.json`, the
//! values it understands are mapped onto [`Settings`] and saved, so a custom database path, the language,
//! the shortcuts and the rest carry over. The plist is only read, never changed, so going back to the Swift
//! app keeps working. Pure mapping and file handling: the plist path is passed in, so tests never touch the
//! real preferences.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use plist::{Dictionary, Value};
use rd_core::Timestamp;
use rd_core::shortcuts::{Platform, ShortcutSettings};

use crate::APP_IDENTITY;
use crate::settings::{SETTINGS_FILE, Settings};

/// The external terminals the Swift app stored (`ExternalTerminal` raw values).
const TERMINALS: [&str; 3] = ["ghostty", "iterm2", "terminal"];

/// The languages the Swift app offered (`AppLanguage` raw values).
const LANGUAGES: [&str; 2] = ["en", "hu"];

/// The Swift app's preferences file: `~/Library/Preferences/<bundle id>.plist`. `None` off macOS or
/// without a home directory.
#[must_use]
pub fn swift_defaults_path() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    dirs::home_dir().map(|home| {
        home.join("Library/Preferences")
            .join(format!("{}.plist", APP_IDENTITY.identifier))
    })
}

/// Imports the Swift settings from `plist_path` into `settings_dir` when this app has no `settings.json`
/// yet. Returns the saved settings, or `None` when nothing was imported: the settings file already exists,
/// the plist is missing or unreadable, or saving failed.
#[must_use = "`Some` means this launch is the first after the handover"]
pub fn import_swift_defaults(settings_dir: &Path, plist_path: &Path) -> Option<Settings> {
    if settings_dir.join(SETTINGS_FILE).exists() {
        return None;
    }
    let defaults = Value::from_file(plist_path).ok()?.into_dictionary()?;
    let settings = settings_from_swift_defaults(&defaults);
    settings.save(settings_dir).ok()?;
    Some(settings)
}

/// Maps the Swift `UserDefaults` keys onto [`Settings`]. A missing key, or one with an unexpected type or
/// value, keeps its default.
#[must_use]
pub fn settings_from_swift_defaults(defaults: &Dictionary) -> Settings {
    let mut settings = Settings::default();
    let string = |key: &str| defaults.get(key).and_then(Value::as_string);
    let boolean = |key: &str| defaults.get(key).and_then(Value::as_boolean);

    if let Some(path) = string("databasePath").filter(|path| !path.is_empty()) {
        settings.database_path = Some(PathBuf::from(path));
    }
    if let Some(remember) = boolean("rememberMasterKey") {
        settings.remember_master_key = remember;
    }
    if let Some(language) = swift_language(defaults) {
        language.clone_into(&mut settings.language);
    }
    if let Some(terminal) = string("externalTerminal").filter(|id| TERMINALS.contains(id)) {
        settings.external_terminal = Some(terminal.to_owned());
    }
    if let Some(shortcuts) = swift_shortcuts(defaults) {
        settings.keyboard_shortcuts = shortcuts;
    }
    if let Some(automatic) = boolean("checkForUpdatesAutomatically") {
        settings.check_for_updates_automatically = automatic;
    }
    settings.last_update_check = defaults.get("lastUpdateCheck").and_then(swift_date);
    if let Some(folders) = string("expandedFolders") {
        settings.expanded_folders = folders
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect();
    }
    settings
}

/// `AppleLanguages` in the app's own domain: `["en"]` or `["hu"]`, region suffixes (`hu-HU`) accepted.
fn swift_language(defaults: &Dictionary) -> Option<&'static str> {
    let first = defaults.get("AppleLanguages")?.as_array()?.first()?.as_string()?;
    let code = first.split(['-', '_']).next()?;
    LANGUAGES.into_iter().find(|language| *language == code)
}

/// `keyboardShortcuts`: JSON `Data` of `{command: {key, modifiers} | null}`, the same shape this app stores.
/// It goes through [`ShortcutSettings`] so unknown commands and invalid entries are dropped.
fn swift_shortcuts(defaults: &Dictionary) -> Option<serde_json::Value> {
    let data = defaults.get("keyboardShortcuts")?.as_data()?;
    let stored: serde_json::Value = serde_json::from_slice(data).ok()?;
    Some(ShortcutSettings::from_json(Platform::MacOS, &stored).to_json())
}

/// A plist `<date>` (Swift stores a `Date`); dates before 1970 count as never.
fn swift_date(value: &Value) -> Option<Timestamp> {
    let seconds = std::time::SystemTime::from(value.as_date()?)
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(seconds).ok().map(Timestamp::from_unix)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn dict(entries: Vec<(&str, Value)>) -> Dictionary {
        entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn string(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn date(seconds: u64) -> Value {
        Value::Date((SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)).into())
    }

    fn swift_dictionary() -> Dictionary {
        dict(vec![
            ("databasePath", string("/Users/me/Dropbox/connections.sqlite")),
            ("rememberMasterKey", Value::Boolean(false)),
            ("AppleLanguages", Value::Array(vec![string("hu")])),
            ("externalTerminal", string("iterm2")),
            (
                "keyboardShortcuts",
                Value::Data(br#"{"newSSH":{"key":"n","modifiers":3},"lock":null}"#.to_vec()),
            ),
            ("checkForUpdatesAutomatically", Value::Boolean(false)),
            ("lastUpdateCheck", date(1_760_000_000)),
            ("expandedFolders", string("A1,B2")),
            // Keys the Swift app or AppKit keep that have no counterpart.
            ("NSWindow Frame main", string("0 0 800 600")),
        ])
    }

    #[test]
    fn every_known_key_is_mapped() {
        let settings = settings_from_swift_defaults(&swift_dictionary());
        assert_eq!(
            settings,
            Settings {
                database_path: Some(PathBuf::from("/Users/me/Dropbox/connections.sqlite")),
                remember_master_key: false,
                external_terminal: Some("iterm2".into()),
                keyboard_shortcuts: serde_json::json!({"lock": null, "newSSH": {"key": "n", "modifiers": 3}}),
                check_for_updates_automatically: false,
                last_update_check: Some(Timestamp::from_unix(1_760_000_000)),
                language: "hu".into(),
                expanded_folders: vec!["A1".into(), "B2".into()],
                winbox_path: None,
            }
        );
    }

    #[test]
    fn missing_keys_keep_the_defaults() {
        assert_eq!(
            settings_from_swift_defaults(&Dictionary::new()),
            Settings::default()
        );
    }

    #[test]
    fn values_of_the_wrong_type_are_skipped() {
        let defaults = dict(vec![
            ("databasePath", Value::Boolean(true)),
            ("rememberMasterKey", string("no")),
            ("AppleLanguages", string("hu")),
            ("externalTerminal", Value::Integer(1.into())),
            ("keyboardShortcuts", string(r#"{"lock":null}"#)),
            ("checkForUpdatesAutomatically", Value::Integer(0.into())),
            ("lastUpdateCheck", Value::Real(1_760_000_000.0)),
            ("expandedFolders", Value::Array(vec![string("A1")])),
        ]);
        assert_eq!(settings_from_swift_defaults(&defaults), Settings::default());
    }

    #[test]
    fn languages_accept_region_suffixes_and_skip_unknown_ones() {
        let language = |code: &str| {
            settings_from_swift_defaults(&dict(vec![("AppleLanguages", Value::Array(vec![string(code)]))]))
                .language
        };
        assert_eq!(language("hu-HU"), "hu");
        assert_eq!(language("en_US"), "en");
        assert_eq!(language("de"), "en");
        assert_eq!(
            settings_from_swift_defaults(&dict(vec![("AppleLanguages", Value::Array(vec![]))])).language,
            "en"
        );
    }

    #[test]
    fn unknown_terminals_and_an_empty_database_path_are_dropped() {
        let settings = settings_from_swift_defaults(&dict(vec![
            ("externalTerminal", string("warp")),
            ("databasePath", string("")),
        ]));
        assert_eq!(settings.external_terminal, None);
        assert_eq!(settings.database_path, None);
    }

    #[test]
    fn shortcuts_drop_unknown_commands_and_bad_json() {
        let shortcuts = |data: &[u8]| {
            settings_from_swift_defaults(&dict(vec![("keyboardShortcuts", Value::Data(data.to_vec()))]))
                .keyboard_shortcuts
        };
        assert_eq!(
            shortcuts(br#"{"lock":null,"teleport":{"key":"t","modifiers":1}}"#),
            serde_json::json!({"lock": null})
        );
        assert_eq!(shortcuts(b"not json"), serde_json::json!({}));
    }

    #[test]
    fn expanded_folders_skip_empty_ids() {
        let folders = |text: &str| {
            settings_from_swift_defaults(&dict(vec![("expandedFolders", string(text))])).expanded_folders
        };
        assert!(folders("").is_empty());
        assert_eq!(folders("A1,,B2"), vec!["A1".to_owned(), "B2".to_owned()]);
    }

    #[test]
    fn dates_before_1970_count_as_never() {
        let before = Value::Date((SystemTime::UNIX_EPOCH - Duration::from_secs(60)).into());
        assert_eq!(swift_date(&before), None);
        assert_eq!(swift_date(&date(5)), Some(Timestamp::from_unix(5)));
    }

    fn write_plist(path: &Path, defaults: Dictionary) {
        Value::Dictionary(defaults).to_file_binary(path).unwrap();
    }

    #[test]
    fn imports_once_into_a_fresh_settings_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let plist = tmp.path().join("swift.plist");
        let config = tmp.path().join("config");
        write_plist(&plist, swift_dictionary());

        let imported = import_swift_defaults(&config, &plist).expect("imported");
        assert_eq!(imported.language, "hu");
        assert_eq!(Settings::load(&config), imported);

        // settings.json exists now: a second launch imports nothing.
        assert_eq!(import_swift_defaults(&config, &plist), None);
        assert!(plist.exists(), "the Swift plist is left in place");
    }

    #[test]
    fn existing_settings_are_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let plist = tmp.path().join("swift.plist");
        write_plist(&plist, swift_dictionary());
        let own = Settings {
            winbox_path: Some(PathBuf::from("/Applications/WinBox.app")),
            ..Settings::default()
        };
        own.save(tmp.path()).unwrap();

        assert_eq!(import_swift_defaults(tmp.path(), &plist), None);
        assert_eq!(Settings::load(tmp.path()), own);
    }

    #[test]
    fn a_missing_or_corrupt_plist_imports_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            import_swift_defaults(tmp.path(), &tmp.path().join("missing.plist")),
            None
        );

        let corrupt = tmp.path().join("corrupt.plist");
        std::fs::write(&corrupt, b"bplist00 garbage").unwrap();
        assert_eq!(import_swift_defaults(tmp.path(), &corrupt), None);

        let not_a_dictionary = tmp.path().join("array.plist");
        Value::Array(vec![]).to_file_binary(&not_a_dictionary).unwrap();
        assert_eq!(import_swift_defaults(tmp.path(), &not_a_dictionary), None);

        assert!(!tmp.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn the_swift_path_is_the_bundle_id_plist_on_macos_only() {
        let path = swift_defaults_path();
        if cfg!(target_os = "macos") {
            let expected = format!("Library/Preferences/{}.plist", APP_IDENTITY.identifier);
            assert!(path.unwrap().ends_with(expected));
        } else {
            assert_eq!(path, None);
        }
    }
}
