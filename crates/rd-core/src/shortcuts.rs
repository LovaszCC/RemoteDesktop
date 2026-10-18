//! Customisable keyboard shortcuts: the commands, per-OS defaults, validation and overrides-only storage.
//!
//! A port of the Swift `ShortcutCommand`, `KeyShortcut` and `ShortcutSettings`. On Windows and Linux the
//! "command" modifier bit means the Super key, which apps may not take, and the defaults use Ctrl+Shift so
//! plain Ctrl+letter stays with the terminal and the RDP session. Ctrl+Alt+character is
//! refused there: on AltGr keyboard layouts it types characters.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::BitOr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::{UnknownValue, raw_enum};

/// The operating system, for shortcut defaults, reserved keys and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// macOS: glyphs, ⌘ shortcuts.
    MacOS,
    /// Windows: `Ctrl+Alt+Shift+Key`.
    Windows,
    /// Linux: like Windows.
    Linux,
}

impl Platform {
    /// Every platform.
    pub const ALL: [Self; 3] = [Self::MacOS, Self::Windows, Self::Linux];

    /// The platform this build runs on (anything that is not macOS or Windows counts as Linux).
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Modifier bits as stored in the JSON: command 1, shift 2, option 4, control 8.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self(0);
    /// ⌘ on macOS; the Super (Windows) key elsewhere.
    pub const COMMAND: Self = Self(1);
    /// ⇧ / Shift.
    pub const SHIFT: Self = Self(2);
    /// ⌥ / Alt.
    pub const OPTION: Self = Self(4);
    /// ⌃ / Ctrl.
    pub const CONTROL: Self = Self(8);

    /// Whether every bit of `other` is set.
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any bit of `other` is set.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Special key names and their macOS glyph and Windows/Linux label.
const SPECIAL_KEYS: &[(&str, &str, &str)] = &[
    ("return", "↩", "Enter"),
    ("tab", "⇥", "Tab"),
    ("delete", "⌫", "Backspace"),
    ("escape", "⎋", "Esc"),
    ("space", "Space", "Space"),
    ("left", "←", "Left"),
    ("right", "→", "Right"),
    ("up", "↑", "Up"),
    ("down", "↓", "Down"),
    ("f1", "F1", "F1"),
    ("f2", "F2", "F2"),
    ("f3", "F3", "F3"),
    ("f4", "F4", "F4"),
    ("f5", "F5", "F5"),
    ("f6", "F6", "F6"),
    ("f7", "F7", "F7"),
    ("f8", "F8", "F8"),
    ("f9", "F9", "F9"),
    ("f10", "F10", "F10"),
    ("f11", "F11", "F11"),
    ("f12", "F12", "F12"),
];

fn special(key: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    SPECIAL_KEYS.iter().find(|(name, _, _)| *name == key)
}

/// A key plus modifiers, stored as `{"key": "n", "modifiers": 3}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KeyShortcut {
    /// A single lowercased character (`n`, `,`) or a special key name (`return`, `tab`, `f5`, …).
    pub key: String,
    /// Modifier bits.
    pub modifiers: Modifiers,
}

impl KeyShortcut {
    /// A shortcut; a character key is lowercased, a special key name is kept.
    #[must_use]
    pub fn new(key: &str, modifiers: Modifiers) -> Self {
        let key = if special(key).is_some() {
            key.to_owned()
        } else {
            key.to_lowercase()
        };
        Self { key, modifiers }
    }

    /// Whether the key is a special key name rather than a character.
    #[must_use]
    pub fn is_special_key(&self) -> bool {
        special(&self.key).is_some()
    }

    /// Whether it has a modifier that keeps it from taking keys away from typing: ⌘, ⌃ or ⌥ on macOS, Ctrl
    /// or Alt on Windows and Linux.
    #[must_use]
    pub fn has_required_modifier(&self, platform: Platform) -> bool {
        let required = match platform {
            Platform::MacOS => Modifiers::COMMAND | Modifiers::CONTROL | Modifiers::OPTION,
            Platform::Windows | Platform::Linux => Modifiers::CONTROL | Modifiers::OPTION,
        };
        self.modifiers.intersects(required)
    }

    /// The menu notation: `⌃⌥⇧⌘N` on macOS, `Ctrl+Alt+Shift+N` on Windows and Linux.
    #[must_use]
    pub fn display(&self, platform: Platform) -> String {
        let m = self.modifiers;
        let special = special(&self.key);
        if platform == Platform::MacOS {
            let mut text = String::new();
            for (bit, glyph) in [
                (Modifiers::CONTROL, "⌃"),
                (Modifiers::OPTION, "⌥"),
                (Modifiers::SHIFT, "⇧"),
                (Modifiers::COMMAND, "⌘"),
            ] {
                if m.contains(bit) {
                    text.push_str(glyph);
                }
            }
            text.push_str(&special.map_or_else(|| self.key.to_uppercase(), |s| s.1.to_owned()));
            text
        } else {
            let mut parts: Vec<String> = [
                (Modifiers::CONTROL, "Ctrl"),
                (Modifiers::OPTION, "Alt"),
                (Modifiers::SHIFT, "Shift"),
                (Modifiers::COMMAND, "Super"),
            ]
            .into_iter()
            .filter(|(bit, _)| m.contains(*bit))
            .map(|(_, name)| name.to_owned())
            .collect();
            parts.push(special.map_or_else(|| self.key.to_uppercase(), |s| s.2.to_owned()));
            parts.join("+")
        }
    }
}

raw_enum! {
    /// App commands whose shortcut can be changed in Settings. The raw ids are the Swift ones.
    #[derive(PartialOrd, Ord)]
    ShortcutCommand {
        /// New RDP connection.
        NewRdp = "newRDP",
        /// New SSH connection.
        NewSsh = "newSSH",
        /// New web connection.
        NewWeb = "newWeb",
        /// New MikroTik connection.
        NewMikrotik = "newMikrotik",
        /// New folder.
        NewFolder = "newFolder",
        /// New credential.
        NewCredential = "newCredential",
        /// Import from RDM.
        ImportRdm = "importRDM",
        /// Import from WinBox.
        ImportWinbox = "importWinbox",
        /// Connect the selection.
        Connect = "connect",
        /// Open a local terminal (new in this app).
        NewLocalTerminal = "newLocalTerminal",
        /// Type the clipboard into an RDP session.
        TypeClipboard = "typeClipboard",
        /// Split the terminal.
        SplitTerminal = "splitTerminal",
        /// Broadcast input to all panes.
        BroadcastAll = "broadcastAll",
        /// Clear the terminal.
        ClearTerminal = "clearTerminal",
        /// Copy the next command's output.
        RecordOutput = "recordOutput",
        /// Files (SCP).
        OpenFiles = "openFiles",
        /// Close the session tab.
        CloseTab = "closeTab",
        /// Next tab.
        NextTab = "nextTab",
        /// Previous tab.
        PreviousTab = "previousTab",
        /// Reconnect.
        Reconnect = "reconnect",
        /// Lock the app.
        Lock = "lock",
        /// Show or hide the sidebar (new in this app; macOS's standard ⌃⌘S).
        ToggleSidebar = "toggleSidebar",
    }
}

use ShortcutCommand as Cmd;

const CMD: Modifiers = Modifiers::COMMAND;
const SHIFT: Modifiers = Modifiers::SHIFT;
const OPT: Modifiers = Modifiers::OPTION;
const CTRL: Modifiers = Modifiers::CONTROL;

impl ShortcutCommand {
    /// Commands of the File menu.
    pub const FILE_MENU: [Self; 8] = [
        Cmd::NewRdp,
        Cmd::NewSsh,
        Cmd::NewWeb,
        Cmd::NewMikrotik,
        Cmd::NewFolder,
        Cmd::NewCredential,
        Cmd::ImportRdm,
        Cmd::ImportWinbox,
    ];

    /// Commands of the View menu.
    pub const VIEW_MENU: [Self; 1] = [Cmd::ToggleSidebar];

    /// Commands of the Connection menu and sessions: every command not in [`FILE_MENU`](Self::FILE_MENU)
    /// or [`VIEW_MENU`](Self::VIEW_MENU).
    #[must_use]
    pub fn connection_menu() -> Vec<Self> {
        Self::ALL
            .iter()
            .copied()
            .filter(|c| !Self::FILE_MENU.contains(c) && !Self::VIEW_MENU.contains(c))
            .collect()
    }

    /// The default shortcut on `platform`.
    #[must_use]
    pub fn default_shortcut(self, platform: Platform) -> KeyShortcut {
        let (key, modifiers) = match platform {
            Platform::MacOS => self.mac_default(),
            Platform::Windows | Platform::Linux => self.pc_default(),
        };
        KeyShortcut::new(key, modifiers)
    }

    fn mac_default(self) -> (&'static str, Modifiers) {
        match self {
            Cmd::NewRdp => ("n", CMD),
            Cmd::NewSsh => ("n", CMD | SHIFT),
            Cmd::NewWeb => ("b", CMD | SHIFT),
            Cmd::NewMikrotik => ("m", CMD | SHIFT),
            Cmd::NewFolder => ("n", CMD | OPT),
            Cmd::NewCredential => ("n", CMD | CTRL),
            Cmd::ImportRdm => ("i", CMD),
            Cmd::ImportWinbox => ("i", CMD | SHIFT),
            Cmd::Connect => ("return", CMD),
            Cmd::NewLocalTerminal => ("t", CMD),
            Cmd::TypeClipboard => ("v", CMD | SHIFT),
            Cmd::SplitTerminal => ("d", CMD),
            Cmd::BroadcastAll => ("i", CMD | OPT),
            Cmd::ClearTerminal => ("k", CMD),
            Cmd::RecordOutput => ("c", CMD | OPT),
            Cmd::OpenFiles => ("f", CMD | SHIFT),
            Cmd::CloseTab => ("w", CMD),
            Cmd::NextTab => ("tab", CTRL),
            Cmd::PreviousTab => ("tab", CTRL | SHIFT),
            Cmd::Reconnect => ("r", CMD),
            Cmd::Lock => ("l", CMD | CTRL),
            Cmd::ToggleSidebar => ("s", CMD | CTRL),
        }
    }

    /// Windows and Linux: Ctrl+Shift+letter, or Ctrl with a special key. Never Ctrl+Alt+letter: on AltGr
    /// layouts (Hungarian, German, …) Ctrl+Alt is AltGr and types characters (AltGr+V is `@`).
    fn pc_default(self) -> (&'static str, Modifiers) {
        match self {
            Cmd::NewRdp => ("n", CTRL | SHIFT),
            Cmd::NewSsh => ("s", CTRL | SHIFT),
            Cmd::NewWeb => ("b", CTRL | SHIFT),
            Cmd::NewMikrotik => ("m", CTRL | SHIFT),
            Cmd::NewFolder => ("g", CTRL | SHIFT),
            Cmd::NewCredential => ("p", CTRL | SHIFT),
            Cmd::ImportRdm => ("i", CTRL | SHIFT),
            Cmd::ImportWinbox => ("x", CTRL | SHIFT),
            Cmd::Connect => ("return", CTRL),
            // Ctrl+Shift+T types the clipboard into RDP.
            Cmd::NewLocalTerminal => ("h", CTRL | SHIFT),
            Cmd::TypeClipboard => ("t", CTRL | SHIFT),
            Cmd::SplitTerminal => ("d", CTRL | SHIFT),
            Cmd::BroadcastAll => ("a", CTRL | SHIFT),
            Cmd::ClearTerminal => ("k", CTRL | SHIFT),
            Cmd::RecordOutput => ("o", CTRL | SHIFT),
            Cmd::OpenFiles => ("f", CTRL | SHIFT),
            Cmd::CloseTab => ("w", CTRL | SHIFT),
            Cmd::NextTab => ("tab", CTRL),
            Cmd::PreviousTab => ("tab", CTRL | SHIFT),
            Cmd::Reconnect => ("r", CTRL | SHIFT),
            Cmd::Lock => ("l", CTRL | SHIFT),
            Cmd::ToggleSidebar => ("e", CTRL | SHIFT),
        }
    }
}

/// Shortcuts taken by the OS or the standard menus.
#[must_use]
pub fn reserved(platform: Platform) -> Vec<KeyShortcut> {
    let list: &[(&str, Modifiers)] = match platform {
        Platform::MacOS => &[
            ("q", CMD),
            (",", CMD),
            ("c", CMD),
            ("v", CMD),
            ("x", CMD),
            ("a", CMD),
            ("z", CMD),
            ("z", CMD | SHIFT),
            ("h", CMD),
            ("h", CMD | OPT),
            ("m", CMD),
            ("`", CMD),
            ("f", CMD),
            ("tab", CMD),
            ("space", CMD),
            ("f", CMD | CTRL),
            ("q", CMD | CTRL),
        ],
        Platform::Windows | Platform::Linux => &[
            ("c", CTRL),
            ("v", CTRL),
            ("x", CTRL),
            ("a", CTRL),
            ("z", CTRL),
            ("y", CTRL),
            ("c", CTRL | SHIFT),
            ("v", CTRL | SHIFT),
            ("f4", OPT),
            ("tab", OPT),
            ("f11", Modifiers::NONE),
        ],
    };
    list.iter().map(|&(key, m)| KeyShortcut::new(key, m)).collect()
}

/// Why a shortcut can't be given to a command. [`code`](Self::code) is a stable id the UI translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "command", rename_all = "camelCase")]
pub enum ShortcutError {
    /// Windows and Linux: the Super key is the OS's.
    #[error("The Windows/Super key can't be used.")]
    SuperKey,
    /// No ⌘, ⌃ or ⌥ (Ctrl or Alt on Windows and Linux).
    #[error("A required modifier is missing.")]
    MissingModifier,
    /// Windows and Linux: Ctrl+Alt with a character key is AltGr on some layouts and types a character.
    #[error("Ctrl+Alt acts as AltGr on some keyboard layouts.")]
    AltGr,
    /// Taken by the OS or the standard menus.
    #[error("This shortcut is reserved by the system.")]
    Reserved,
    /// Another command has it.
    #[error("Already used by: {0}")]
    InUse(ShortcutCommand),
}

impl ShortcutError {
    /// The stable error code: `superKey`, `missingModifier`, `altGr`, `reserved` or `inUse`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::SuperKey => "superKey",
            Self::MissingModifier => "missingModifier",
            Self::AltGr => "altGr",
            Self::Reserved => "reserved",
            Self::InUse(_) => "inUse",
        }
    }

    /// The English message, matching the Swift catalog keys where Swift has one (`%@` is the command).
    #[must_use]
    pub fn message(self, platform: Platform) -> &'static str {
        match (self, platform) {
            (Self::SuperKey, _) => "The Windows/Super key can't be used in app shortcuts.",
            (Self::MissingModifier, Platform::MacOS) => "Use at least one of ⌘, ⌃ or ⌥.",
            (Self::MissingModifier, _) => "Use at least one of Ctrl or Alt.",
            (Self::AltGr, _) => "Ctrl+Alt acts as AltGr on some keyboard layouts.",
            (Self::Reserved, Platform::MacOS) => "This shortcut is reserved by macOS.",
            (Self::Reserved, _) => "This shortcut is reserved by the system.",
            (Self::InUse(_), _) => "Already used by: %@",
        }
    }
}

/// The shortcuts the user changed; every other command keeps its default for the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSettings {
    platform: Platform,
    /// `None` value: the user removed the command's shortcut.
    overrides: BTreeMap<ShortcutCommand, Option<KeyShortcut>>,
}

impl ShortcutSettings {
    /// No overrides on `platform`.
    #[must_use]
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            overrides: BTreeMap::new(),
        }
    }

    /// The platform the defaults and reserved keys come from.
    #[must_use]
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// The overrides: a command with `None` has had its shortcut removed.
    #[must_use]
    pub fn overrides(&self) -> &BTreeMap<ShortcutCommand, Option<KeyShortcut>> {
        &self.overrides
    }

    /// The command's shortcut: the override, else the default. `None` when removed.
    #[must_use]
    pub fn shortcut(&self, command: ShortcutCommand) -> Option<KeyShortcut> {
        match self.overrides.get(&command) {
            Some(value) => value.clone(),
            None => Some(command.default_shortcut(self.platform)),
        }
    }

    /// Whether the command differs from its default.
    #[must_use]
    pub fn is_customized(&self, command: ShortcutCommand) -> bool {
        self.overrides.contains_key(&command)
    }

    /// The effective shortcut of every command.
    #[must_use]
    pub fn effective(&self) -> BTreeMap<ShortcutCommand, Option<KeyShortcut>> {
        ShortcutCommand::ALL
            .iter()
            .map(|&c| (c, self.shortcut(c)))
            .collect()
    }

    /// Why `shortcut` can't go to `command`: Super key (Windows/Linux), missing modifier, Ctrl+Alt with a
    /// character (Windows/Linux, AltGr), reserved, then already used.
    ///
    /// # Errors
    /// The first rule the shortcut breaks.
    pub fn validate(&self, shortcut: &KeyShortcut, command: ShortcutCommand) -> Result<(), ShortcutError> {
        if self.platform != Platform::MacOS && shortcut.modifiers.contains(Modifiers::COMMAND) {
            return Err(ShortcutError::SuperKey);
        }
        if !shortcut.has_required_modifier(self.platform) {
            return Err(ShortcutError::MissingModifier);
        }
        if self.platform != Platform::MacOS
            && shortcut
                .modifiers
                .contains(Modifiers::CONTROL | Modifiers::OPTION)
            && !shortcut.is_special_key()
        {
            return Err(ShortcutError::AltGr);
        }
        if reserved(self.platform).contains(shortcut) {
            return Err(ShortcutError::Reserved);
        }
        if let Some(&owner) = ShortcutCommand::ALL
            .iter()
            .find(|&&c| c != command && self.shortcut(c).as_ref() == Some(shortcut))
        {
            return Err(ShortcutError::InUse(owner));
        }
        Ok(())
    }

    /// Sets, or with `None` removes, a command's shortcut. Setting the default drops the override.
    ///
    /// # Errors
    /// The validation error; nothing changes then.
    pub fn set(
        &mut self,
        shortcut: Option<KeyShortcut>,
        command: ShortcutCommand,
    ) -> Result<(), ShortcutError> {
        if let Some(shortcut) = &shortcut {
            self.validate(shortcut, command)?;
        }
        if shortcut.as_ref() == Some(&command.default_shortcut(self.platform)) {
            self.overrides.remove(&command);
        } else {
            self.overrides.insert(command, shortcut);
        }
        Ok(())
    }

    /// Back to the default, unless another command took it meanwhile.
    ///
    /// # Errors
    /// [`ShortcutError::InUse`] naming the command that has the default now.
    pub fn reset(&mut self, command: ShortcutCommand) -> Result<(), ShortcutError> {
        self.set(Some(command.default_shortcut(self.platform)), command)
    }

    /// Drops every override.
    pub fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// Reads the stored overrides (`{command: {key, modifiers} | null}`). Invalid JSON gives no overrides;
    /// unknown commands are skipped.
    #[must_use]
    pub fn from_json(platform: Platform, value: &serde_json::Value) -> Self {
        let mut settings = Self::new(platform);
        if let Ok(stored) = BTreeMap::<String, Option<KeyShortcut>>::deserialize(value) {
            for (name, shortcut) in stored {
                if let Ok(command) = name.parse::<ShortcutCommand>() {
                    settings.overrides.insert(command, shortcut);
                }
            }
        }
        settings
    }

    /// The overrides for storage.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let stored: BTreeMap<&str, &Option<KeyShortcut>> =
            self.overrides.iter().map(|(c, s)| (c.as_str(), s)).collect();
        serde_json::to_value(stored).unwrap_or_default()
    }
}

impl fmt::Display for KeyShortcut {
    /// The current platform's notation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(Platform::current()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;

    const MAC: Platform = Platform::MacOS;

    fn k(key: &str, modifiers: Modifiers) -> KeyShortcut {
        KeyShortcut::new(key, modifiers)
    }

    fn mac() -> ShortcutSettings {
        ShortcutSettings::new(MAC)
    }

    #[test]
    fn display_uses_menu_order_and_glyphs() {
        assert_eq!(k("n", CMD | SHIFT).display(MAC), "⇧⌘N");
        assert_eq!(k("l", CMD | CTRL).display(MAC), "⌃⌘L");
        assert_eq!(k("i", CMD | OPT | SHIFT | CTRL).display(MAC), "⌃⌥⇧⌘I");
        assert_eq!(k("return", CMD).display(MAC), "⌘↩");
        assert_eq!(k("tab", CTRL | SHIFT).display(MAC), "⌃⇧⇥");
        assert_eq!(k("f5", OPT).display(MAC), "⌥F5");
    }

    #[test]
    fn display_on_windows_and_linux() {
        for p in [Platform::Windows, Platform::Linux] {
            assert_eq!(k("n", CTRL | OPT).display(p), "Ctrl+Alt+N");
            assert_eq!(k("tab", CTRL | SHIFT).display(p), "Ctrl+Shift+Tab");
            assert_eq!(k("return", CTRL).display(p), "Ctrl+Enter");
            assert_eq!(
                k("k", CTRL | OPT | SHIFT | CMD).display(p),
                "Ctrl+Alt+Shift+Super+K"
            );
            assert_eq!(k("f11", Modifiers::NONE).display(p), "F11");
        }
    }

    #[test]
    fn characters_are_lowercased_specials_kept() {
        assert_eq!(k("N", CMD).key, "n");
        assert!(k("return", CMD).is_special_key());
        assert!(!k("n", CMD).is_special_key());
    }

    #[test]
    fn required_modifier() {
        assert!(!k("n", Modifiers::NONE).has_required_modifier(MAC));
        assert!(!k("n", SHIFT).has_required_modifier(MAC));
        assert!(k("n", OPT).has_required_modifier(MAC));
        assert!(k("n", CTRL).has_required_modifier(MAC));
        assert!(k("n", CMD).has_required_modifier(MAC));
        assert!(!k("n", CMD).has_required_modifier(Platform::Windows));
        assert!(k("n", OPT).has_required_modifier(Platform::Linux));
    }

    #[test]
    fn raw_ids_match_swift() {
        assert_eq!(
            ShortcutCommand::ALL
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>(),
            [
                "newRDP",
                "newSSH",
                "newWeb",
                "newMikrotik",
                "newFolder",
                "newCredential",
                "importRDM",
                "importWinbox",
                "connect",
                "newLocalTerminal",
                "typeClipboard",
                "splitTerminal",
                "broadcastAll",
                "clearTerminal",
                "recordOutput",
                "openFiles",
                "closeTab",
                "nextTab",
                "previousTab",
                "reconnect",
                "lock",
                "toggleSidebar"
            ]
        );
        assert_eq!(serde_json::to_string(&Cmd::ImportRdm).unwrap(), "\"importRDM\"");
    }

    #[test]
    fn defaults_are_unique_valid_and_not_reserved_on_every_platform() {
        for platform in Platform::ALL {
            let settings = ShortcutSettings::new(platform);
            let defaults: Vec<KeyShortcut> = ShortcutCommand::ALL
                .iter()
                .map(|c| c.default_shortcut(platform))
                .collect();
            assert_eq!(
                defaults.iter().collect::<HashSet<_>>().len(),
                defaults.len(),
                "{platform:?}"
            );
            let reserved = reserved(platform);
            for &command in ShortcutCommand::ALL {
                let shortcut = command.default_shortcut(platform);
                assert!(shortcut.has_required_modifier(platform), "{command}");
                assert!(!reserved.contains(&shortcut), "{command}");
                assert_eq!(
                    settings.validate(&shortcut, command),
                    Ok(()),
                    "{platform:?} {command}"
                );
            }
        }
    }

    #[test]
    fn pc_defaults_leave_terminal_keys_alone() {
        for platform in [Platform::Windows, Platform::Linux] {
            for &command in ShortcutCommand::ALL {
                let s = command.default_shortcut(platform);
                assert!(!s.modifiers.contains(CMD), "{command} uses Super");
                let plain_ctrl_letter = s.modifiers == CTRL && !s.is_special_key();
                assert!(!plain_ctrl_letter, "{command} is plain {}", s.display(platform));
                assert!(s.modifiers.contains(CTRL), "{command} lacks Ctrl");
                assert!(!s.modifiers.contains(OPT), "{command} uses Alt (AltGr)");
            }
        }
        let p = Platform::Windows;
        let shown = |c: Cmd| c.default_shortcut(p).display(p);
        assert_eq!(shown(Cmd::NewRdp), "Ctrl+Shift+N");
        assert_eq!(shown(Cmd::NewSsh), "Ctrl+Shift+S");
        assert_eq!(shown(Cmd::NewFolder), "Ctrl+Shift+G");
        assert_eq!(shown(Cmd::TypeClipboard), "Ctrl+Shift+T");
        assert_eq!(shown(Cmd::CloseTab), "Ctrl+Shift+W");
        assert_eq!(shown(Cmd::NextTab), "Ctrl+Tab");
        assert_eq!(shown(Cmd::PreviousTab), "Ctrl+Shift+Tab");
        assert_eq!(shown(Cmd::Connect), "Ctrl+Enter");
        assert_eq!(shown(Cmd::NewLocalTerminal), "Ctrl+Shift+H");
    }

    #[test]
    fn defaults_match_previous_keys() {
        let shown = |c: Cmd| c.default_shortcut(MAC).display(MAC);
        assert_eq!(shown(Cmd::NewSsh), "⇧⌘N");
        assert_eq!(shown(Cmd::Connect), "⌘↩");
        assert_eq!(shown(Cmd::NewLocalTerminal), "⌘T");
        assert_eq!(shown(Cmd::SplitTerminal), "⌘D");
        assert_eq!(shown(Cmd::BroadcastAll), "⌥⌘I");
        assert_eq!(shown(Cmd::ClearTerminal), "⌘K");
        assert_eq!(shown(Cmd::RecordOutput), "⌥⌘C");
        assert_eq!(shown(Cmd::CloseTab), "⌘W");
        assert_eq!(shown(Cmd::NextTab), "⌃⇥");
        assert_eq!(shown(Cmd::PreviousTab), "⌃⇧⇥");
        assert_eq!(shown(Cmd::Lock), "⌃⌘L");
        assert_eq!(shown(Cmd::ToggleSidebar), "⌃⌘S");
    }

    #[test]
    fn menu_groups_cover_every_command_once() {
        let mut grouped: Vec<Cmd> = ShortcutCommand::FILE_MENU.to_vec();
        grouped.extend(ShortcutCommand::connection_menu());
        grouped.extend(ShortcutCommand::VIEW_MENU);
        assert_eq!(grouped.len(), ShortcutCommand::ALL.len());
        assert_eq!(
            grouped.iter().collect::<HashSet<_>>(),
            ShortcutCommand::ALL.iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn validate_refuses_with_reason() {
        let settings = mac();
        assert_eq!(
            settings.validate(&k("k", Modifiers::NONE), Cmd::Lock),
            Err(ShortcutError::MissingModifier)
        );
        assert_eq!(
            settings.validate(&k("k", SHIFT), Cmd::Lock),
            Err(ShortcutError::MissingModifier)
        );
        assert_eq!(
            settings.validate(&k("q", CMD), Cmd::Lock),
            Err(ShortcutError::Reserved)
        );
        assert_eq!(
            settings.validate(&k("n", CMD | SHIFT), Cmd::Lock),
            Err(ShortcutError::InUse(Cmd::NewSsh))
        );
        assert_eq!(settings.validate(&k("l", CMD | CTRL), Cmd::Lock), Ok(()));
        assert_eq!(settings.validate(&k("k", CMD | OPT), Cmd::Lock), Ok(()));
        assert_eq!(
            ShortcutError::InUse(Cmd::NewSsh).message(MAC),
            "Already used by: %@"
        );
        assert_eq!(
            ShortcutError::Reserved.message(MAC),
            "This shortcut is reserved by macOS."
        );
    }

    #[test]
    fn validate_on_windows_and_linux() {
        for p in [Platform::Windows, Platform::Linux] {
            let settings = ShortcutSettings::new(p);
            assert_eq!(
                settings.validate(&k("j", CMD | CTRL), Cmd::Lock),
                Err(ShortcutError::SuperKey)
            );
            assert_eq!(
                settings.validate(&k("j", CMD), Cmd::Lock),
                Err(ShortcutError::SuperKey)
            );
            assert_eq!(
                settings.validate(&k("j", SHIFT), Cmd::Lock),
                Err(ShortcutError::MissingModifier)
            );
            for reserved in [k("c", CTRL), k("y", CTRL), k("v", CTRL | SHIFT), k("f4", OPT)] {
                assert_eq!(
                    settings.validate(&reserved, Cmd::Lock),
                    Err(ShortcutError::Reserved)
                );
            }
            assert_eq!(
                settings.validate(&k("f11", Modifiers::NONE), Cmd::Lock),
                Err(ShortcutError::MissingModifier)
            );
            assert_eq!(
                settings.validate(&k("w", CTRL | SHIFT), Cmd::Lock),
                Err(ShortcutError::InUse(Cmd::CloseTab))
            );
            assert_eq!(settings.validate(&k("j", CTRL), Cmd::Lock), Ok(()));
            assert_eq!(
                ShortcutError::Reserved.message(p),
                "This shortcut is reserved by the system."
            );
        }
        assert_eq!(
            serde_json::to_value(ShortcutError::InUse(Cmd::NewSsh)).unwrap(),
            json!({"code": "inUse", "command": "newSSH"})
        );
        assert_eq!(
            serde_json::to_value(ShortcutError::Reserved).unwrap(),
            json!({"code": "reserved"})
        );
        assert_eq!(ShortcutError::SuperKey.code(), "superKey");
    }

    #[test]
    fn ctrl_alt_character_is_altgr_on_windows_and_linux() {
        for p in [Platform::Windows, Platform::Linux] {
            let settings = ShortcutSettings::new(p);
            for shortcut in [
                k("n", CTRL | OPT),
                k("v", CTRL | OPT | SHIFT),
                k("@", CTRL | OPT),
                k("7", CTRL | OPT),
            ] {
                assert_eq!(
                    settings.validate(&shortcut, Cmd::Lock),
                    Err(ShortcutError::AltGr),
                    "{p:?} {}",
                    shortcut.display(p)
                );
            }
            // Special keys and Alt without Ctrl are fine.
            assert_eq!(settings.validate(&k("f5", CTRL | OPT), Cmd::Lock), Ok(()));
            assert_eq!(settings.validate(&k("left", CTRL | OPT), Cmd::Lock), Ok(()));
            assert_eq!(settings.validate(&k("j", OPT | SHIFT), Cmd::Lock), Ok(()));
            assert_eq!(
                ShortcutError::AltGr.message(p),
                "Ctrl+Alt acts as AltGr on some keyboard layouts."
            );
        }
        assert_eq!(ShortcutError::AltGr.code(), "altGr");
        assert_eq!(
            serde_json::to_value(ShortcutError::AltGr).unwrap(),
            json!({"code": "altGr"})
        );
        // macOS has no AltGr: ⌃⌥ with a letter is allowed.
        assert_eq!(mac().validate(&k("j", CTRL | OPT), Cmd::Lock), Ok(()));
    }

    #[test]
    fn set_clear_and_reset() {
        let mut settings = mac();
        settings.set(Some(k("j", CMD | CTRL)), Cmd::NewSsh).unwrap();
        assert_eq!(settings.shortcut(Cmd::NewSsh), Some(k("j", CMD | CTRL)));
        assert!(settings.is_customized(Cmd::NewSsh));
        // The freed default can go to another command.
        settings.set(Some(k("n", CMD | SHIFT)), Cmd::NewWeb).unwrap();
        assert_eq!(
            settings.validate(&k("j", CMD | CTRL), Cmd::Lock),
            Err(ShortcutError::InUse(Cmd::NewSsh))
        );

        settings.set(None, Cmd::Lock).unwrap();
        assert_eq!(settings.shortcut(Cmd::Lock), None);
        assert!(settings.is_customized(Cmd::Lock));
        assert_eq!(settings.effective()[&Cmd::Lock], None);

        // newSSH's default is now taken by newWeb.
        assert_eq!(
            settings.reset(Cmd::NewSsh),
            Err(ShortcutError::InUse(Cmd::NewWeb))
        );
        settings.reset(Cmd::NewWeb).unwrap();
        settings.reset(Cmd::NewSsh).unwrap();
        assert!(!settings.is_customized(Cmd::NewSsh));
        assert_eq!(
            settings.shortcut(Cmd::NewSsh),
            Some(Cmd::NewSsh.default_shortcut(MAC))
        );

        settings.reset_all();
        assert!(settings.overrides().is_empty());
        assert_eq!(
            settings.shortcut(Cmd::Lock),
            Some(Cmd::Lock.default_shortcut(MAC))
        );
        assert_eq!(settings.effective().len(), ShortcutCommand::ALL.len());
    }

    #[test]
    fn setting_the_default_is_not_a_customization() {
        let mut settings = mac();
        settings
            .set(Some(Cmd::Lock.default_shortcut(MAC)), Cmd::Lock)
            .unwrap();
        assert!(settings.overrides().is_empty());
    }

    #[test]
    fn refused_set_leaves_settings_unchanged() {
        let mut settings = mac();
        assert_eq!(
            settings.set(Some(k("c", CMD)), Cmd::Lock),
            Err(ShortcutError::Reserved)
        );
        assert_eq!(settings, mac());
    }

    #[test]
    fn round_trips_overrides_including_cleared() {
        let mut settings = mac();
        settings.set(Some(k("j", CMD | CTRL)), Cmd::NewSsh).unwrap();
        settings.set(None, Cmd::Lock).unwrap();
        let stored = settings.to_json();
        assert_eq!(
            stored,
            json!({"newSSH": {"key": "j", "modifiers": 9}, "lock": null})
        );

        let loaded = ShortcutSettings::from_json(MAC, &stored);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.shortcut(Cmd::Lock), None);
        assert_eq!(
            loaded.shortcut(Cmd::Connect),
            Some(Cmd::Connect.default_shortcut(MAC))
        );
    }

    #[test]
    fn missing_corrupt_or_unknown_falls_back_to_defaults() {
        assert_eq!(ShortcutSettings::from_json(MAC, &serde_json::Value::Null), mac());
        assert_eq!(ShortcutSettings::from_json(MAC, &json!("not json")), mac());
        assert_eq!(
            ShortcutSettings::from_json(MAC, &json!({"gone": {"key": "k", "modifiers": 1}})),
            mac()
        );
        assert_eq!(
            ShortcutSettings::from_json(MAC, &json!({"lock": {"key": 5}})),
            mac()
        );
    }
}
