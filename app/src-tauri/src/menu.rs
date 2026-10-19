//! The native menu bar (Swift `RemoteDesktopApp.commands`): the app menu on macOS, File, Connection,
//! Edit, View and Window.
//!
//! The menu is built in Rust from the core's state, so it is right before the webview loads: labels come
//! from a small en/hu table (the language setting), accelerators from the effective shortcuts, and items
//! are disabled while locked or until their feature exists. [`refresh`] rebuilds it after status, language
//! and shortcut changes.
//!
//! A click on a command item emits `menu-command` `{command}`; the UI runs it through the same dispatcher
//! as its keyboard shortcuts (also Settings… and Check for Updates…, which is in the app menu on macOS and
//! in File elsewhere). Quit goes through the quit confirmation in Rust.

use std::collections::BTreeMap;

use rd_core::shortcuts::{KeyShortcut, Modifiers, Platform, ShortcutCommand};
use rd_platform::APP_IDENTITY;
use serde::Serialize;
use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::commands::Core;
use crate::core::AppCore;
use crate::dto::Phase;

/// Event: a menu command was chosen.
pub const MENU_COMMAND: &str = "menu-command";

/// Menu id of Settings… (handled by the UI like a command).
pub const OPEN_SETTINGS: &str = "openSettings";
/// Menu id of the dev-only test tab.
pub const DEV_OPEN_TEST_TAB: &str = "devOpenTestTab";
/// Menu id of Quit.
pub const QUIT: &str = "quit";
/// Menu id of "Check for Updates…" (handled by the UI like a command).
pub const CHECK_FOR_UPDATES: &str = "checkForUpdates";

/// Payload of [`MENU_COMMAND`].
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MenuCommand {
    /// The command id (`newRDP`, `openSettings`, …).
    pub command: String,
}

/// English label → Hungarian, for the menu and the quit dialog. Matches the UI catalog.
const HU: &[(&str, &str)] = &[
    ("File", "Fájl"),
    ("Connection", "Kapcsolat"),
    ("Edit", "Szerkesztés"),
    ("Window", "Ablak"),
    ("View", "Nézet"),
    ("Toggle sidebar", "Oldalsáv megjelenítése/elrejtése"),
    ("New RDP connection", "Új RDP kapcsolat"),
    ("New SSH connection", "Új SSH kapcsolat"),
    ("New web connection", "Új böngésző kapcsolat"),
    ("New MikroTik connection", "Új MikroTik kapcsolat"),
    ("New folder", "Új mappa"),
    ("New credential", "Új credential"),
    ("Import from RDM…", "Importálás RDM-ből…"),
    ("Import from WinBox…", "Importálás WinBoxból…"),
    ("Connect", "Kapcsolódás"),
    ("New Local Terminal", "Új helyi terminál"),
    (
        "Type clipboard into RDP session",
        "Vágólap begépelése az RDP sessionbe",
    ),
    ("Split terminal", "Terminál felosztása"),
    ("Files (SCP)…", "Fájlok (SCP)…"),
    ("Files – %@", "Fájlok – %@"),
    ("Broadcast input to all panes", "Broadcast minden panelre"),
    ("Clear terminal", "Terminál törlése"),
    (
        "Copy next command output",
        "Következő parancs kimenetének másolása",
    ),
    ("Close session", "Session bezárása"),
    ("Next tab", "Következő fül"),
    ("Previous tab", "Előző fül"),
    ("Reconnect", "Újrakapcsolódás"),
    ("Reconnecting", "Újrakapcsolódás"),
    ("Lock", "Zárolás"),
    ("Settings…", "Beállítások…"),
    ("Check for Updates…", "Frissítések keresése…"),
    ("About %@", "A(z) %@ névjegye"),
    ("Hide %@", "%@ elrejtése"),
    ("Hide Others", "Többi elrejtése"),
    ("Show All", "Összes megjelenítése"),
    ("Quit %@", "Kilépés: %@"),
    ("Quit", "Kilépés"),
    ("Cancel", "Mégse"),
    ("Undo", "Visszavonás"),
    ("Redo", "Újra"),
    ("Cut", "Kivágás"),
    ("Copy", "Másolás"),
    ("Paste", "Beillesztés"),
    ("Select All", "Összes kijelölése"),
    ("Minimize", "Kis méret"),
    ("Zoom", "Nagyítás"),
    ("Sessions are running", "Futó sessionök"),
    (
        "%lld sessions are still running. Quitting closes all of them.",
        "%lld session még fut. Kilépéskor mind lezárul.",
    ),
];

/// The label of `english` in `language` (`en` or `hu`); unknown labels stay English.
#[must_use]
pub fn tr(language: &str, english: &'static str) -> &'static str {
    if language == "hu" {
        HU.iter()
            .find(|(en, _)| *en == english)
            .map_or(english, |(_, hu)| hu)
    } else {
        english
    }
}

/// The menu title of a command (Swift `ShortcutCommand.title`).
#[must_use]
pub fn command_title(command: ShortcutCommand) -> &'static str {
    use ShortcutCommand as C;
    match command {
        C::NewRdp => "New RDP connection",
        C::NewSsh => "New SSH connection",
        C::NewWeb => "New web connection",
        C::NewMikrotik => "New MikroTik connection",
        C::NewFolder => "New folder",
        C::NewCredential => "New credential",
        C::ImportRdm => "Import from RDM…",
        C::ImportWinbox => "Import from WinBox…",
        C::Connect => "Connect",
        C::NewLocalTerminal => "New Local Terminal",
        C::TypeClipboard => "Type clipboard into RDP session",
        C::SplitTerminal => "Split terminal",
        C::BroadcastAll => "Broadcast input to all panes",
        C::ClearTerminal => "Clear terminal",
        C::RecordOutput => "Copy next command output",
        C::OpenFiles => "Files (SCP)…",
        C::CloseTab => "Close session",
        C::NextTab => "Next tab",
        C::PreviousTab => "Previous tab",
        C::Reconnect => "Reconnect",
        C::Lock => "Lock",
        C::ToggleSidebar => "Toggle sidebar",
    }
}

/// A shortcut as a Tauri accelerator (`Cmd+Shift+N`, `Ctrl+Enter`). `None` for a key the accelerator
/// syntax has no name for. The command bit is ⌘ on macOS and Super elsewhere.
#[must_use]
pub fn accelerator(shortcut: &KeyShortcut, platform: Platform) -> Option<String> {
    let key = match shortcut.key.as_str() {
        "return" => "Enter".to_owned(),
        "tab" => "Tab".to_owned(),
        "delete" => "Backspace".to_owned(),
        "escape" => "Escape".to_owned(),
        "space" => "Space".to_owned(),
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        key if key.len() > 1
            && key.starts_with('f')
            && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            key.to_uppercase()
        }
        key => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            match c {
                'a'..='z' | '0'..='9' => c.to_ascii_uppercase().to_string(),
                '`' | '\\' | '[' | ']' | ',' | '=' | '-' | '.' | '\'' | ';' | '/' => c.to_string(),
                _ => return None,
            }
        }
    };
    let m = shortcut.modifiers;
    let command = if platform == Platform::MacOS {
        "Cmd"
    } else {
        "Super"
    };
    let mut parts: Vec<String> = [
        (Modifiers::COMMAND, command),
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::OPTION, "Alt"),
        (Modifiers::SHIFT, "Shift"),
    ]
    .into_iter()
    .filter(|(bit, _)| m.contains(*bit))
    .map(|(_, name)| name.to_owned())
    .collect();
    parts.push(key);
    Some(parts.join("+"))
}

/// What the menu shows.
#[allow(clippy::struct_excessive_bools)] // independent enable flags
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// `en` or `hu`.
    pub language: String,
    /// Which shortcut notation and app menu layout.
    pub platform: Platform,
    /// Effective shortcuts.
    pub shortcuts: BTreeMap<ShortcutCommand, Option<KeyShortcut>>,
    /// The library is open.
    pub unlocked: bool,
    /// The database accepts writes.
    pub writable: bool,
    /// At least one terminal session tab is open.
    pub sessions: bool,
    /// At least one RDP session tab is open.
    pub rdp_sessions: bool,
    /// The main window has an SSH/`MikroTik` selection or a terminal tab ("Files (SCP)…" applies).
    pub files_target: bool,
    /// Debug build: offer the test tab.
    pub dev: bool,
}

impl MenuModel {
    /// The model of the core's current state.
    #[must_use]
    pub fn of(core: &AppCore) -> Self {
        let status = core.status();
        let unlocked = matches!(status.phase, Phase::Unlocked { .. });
        Self {
            language: core.settings_get().language,
            platform: status.platform,
            shortcuts: core
                .shortcuts_get()
                .into_iter()
                .map(|row| (row.command, row.shortcut))
                .collect(),
            unlocked,
            writable: unlocked && !status.read_only,
            sessions: core.ssh_session_count() > 0,
            rdp_sessions: core.rdp_session_count() > 0,
            files_target: core.files_target(),
            dev: cfg!(debug_assertions),
        }
    }

    /// Whether a command's item is enabled: library commands need the unlocked (and for changes, writable)
    /// library; terminal commands an open session (the UI applies them to the active tab); the rest wait
    /// for their phase.
    #[must_use]
    pub fn enabled(&self, command: ShortcutCommand) -> bool {
        use ShortcutCommand as C;
        match command {
            C::NewRdp
            | C::NewSsh
            | C::NewWeb
            | C::NewMikrotik
            | C::NewFolder
            | C::NewCredential
            | C::ImportRdm
            | C::ImportWinbox => self.writable,
            C::Connect
            | C::NewLocalTerminal
            | C::CloseTab
            | C::NextTab
            | C::PreviousTab
            | C::Lock
            | C::ToggleSidebar => self.unlocked,
            C::ClearTerminal | C::RecordOutput | C::SplitTerminal | C::BroadcastAll => {
                self.unlocked && self.sessions
            }
            C::Reconnect => self.unlocked && (self.sessions || self.rdp_sessions),
            C::OpenFiles => self.unlocked && self.files_target,
            C::TypeClipboard => self.unlocked && self.rdp_sessions,
        }
    }

    fn label(&self, english: &'static str) -> &'static str {
        tr(&self.language, english)
    }

    fn label_with_product(&self, english: &'static str) -> String {
        self.label(english).replace("%@", APP_IDENTITY.product_name)
    }

    /// The accelerator of `command`, if it has a shortcut the syntax can express.
    #[must_use]
    pub fn accelerator(&self, command: ShortcutCommand) -> Option<String> {
        self.shortcuts
            .get(&command)
            .and_then(Option::as_ref)
            .and_then(|s| accelerator(s, self.platform))
    }

    /// Settings: ⌘, on macOS, Ctrl+, elsewhere.
    #[must_use]
    pub fn settings_accelerator(&self) -> &'static str {
        if self.platform == Platform::MacOS {
            "Cmd+,"
        } else {
            "Ctrl+,"
        }
    }
}

type Items = Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>>;

fn command_item(
    app: &AppHandle,
    model: &MenuModel,
    command: ShortcutCommand,
) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(
        app,
        command.as_str(),
        model.label(command_title(command)),
        model.enabled(command),
        model.accelerator(command),
    )
}

fn separator(app: &AppHandle) -> tauri::Result<Box<dyn tauri::menu::IsMenuItem<Wry>>> {
    Ok(Box::new(PredefinedMenuItem::separator(app)?))
}

fn submenu(app: &AppHandle, title: &str, items: &Items) -> tauri::Result<Submenu<Wry>> {
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = items.iter().map(AsRef::as_ref).collect();
    Submenu::with_items(app, title, true, &refs)
}

fn settings_item(app: &AppHandle, model: &MenuModel) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(
        app,
        OPEN_SETTINGS,
        model.label("Settings…"),
        true,
        Some(model.settings_accelerator()),
    )
}

fn updates_item(app: &AppHandle, model: &MenuModel) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(
        app,
        CHECK_FOR_UPDATES,
        model.label("Check for Updates…"),
        true,
        None::<&str>,
    )
}

fn app_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Submenu<Wry>> {
    let about = AboutMetadata {
        name: Some(APP_IDENTITY.product_name.to_owned()),
        version: Some(app.package_info().version.to_string()),
        ..AboutMetadata::default()
    };
    let items: Items = vec![
        Box::new(PredefinedMenuItem::about(
            app,
            Some(&model.label_with_product("About %@")),
            Some(about),
        )?),
        Box::new(updates_item(app, model)?),
        separator(app)?,
        Box::new(settings_item(app, model)?),
        separator(app)?,
        Box::new(PredefinedMenuItem::hide(
            app,
            Some(&model.label_with_product("Hide %@")),
        )?),
        Box::new(PredefinedMenuItem::hide_others(
            app,
            Some(model.label("Hide Others")),
        )?),
        Box::new(PredefinedMenuItem::show_all(app, Some(model.label("Show All")))?),
        separator(app)?,
        Box::new(MenuItem::with_id(
            app,
            QUIT,
            model.label_with_product("Quit %@"),
            true,
            Some("Cmd+Q"),
        )?),
    ];
    submenu(app, APP_IDENTITY.product_name, &items)
}

fn file_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Submenu<Wry>> {
    use ShortcutCommand as C;
    let mac = model.platform == Platform::MacOS;
    let mut file: Items = Vec::new();
    for command in [
        C::NewRdp,
        C::NewSsh,
        C::NewWeb,
        C::NewMikrotik,
        C::NewFolder,
        C::NewCredential,
    ] {
        file.push(Box::new(command_item(app, model, command)?));
    }
    file.push(separator(app)?);
    for command in [C::ImportRdm, C::ImportWinbox] {
        file.push(Box::new(command_item(app, model, command)?));
    }
    if !mac {
        file.push(separator(app)?);
        file.push(Box::new(settings_item(app, model)?));
        file.push(Box::new(updates_item(app, model)?));
        file.push(separator(app)?);
        file.push(Box::new(MenuItem::with_id(
            app,
            QUIT,
            model.label("Quit"),
            true,
            None::<&str>,
        )?));
    }
    submenu(app, model.label("File"), &file)
}

fn connection_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Submenu<Wry>> {
    use ShortcutCommand as C;
    let mut connection: Items = vec![
        Box::new(command_item(app, model, C::Connect)?),
        Box::new(command_item(app, model, C::NewLocalTerminal)?),
        separator(app)?,
    ];
    for command in [
        C::TypeClipboard,
        C::SplitTerminal,
        C::OpenFiles,
        C::BroadcastAll,
        C::ClearTerminal,
        C::RecordOutput,
        C::CloseTab,
        C::NextTab,
        C::PreviousTab,
    ] {
        connection.push(Box::new(command_item(app, model, command)?));
    }
    connection.push(separator(app)?);
    connection.push(Box::new(command_item(app, model, C::Lock)?));
    if model.dev {
        connection.push(separator(app)?);
        connection.push(Box::new(MenuItem::with_id(
            app,
            DEV_OPEN_TEST_TAB,
            "Open test tab (dev)",
            model.unlocked,
            None::<&str>,
        )?));
    }
    submenu(app, model.label("Connection"), &connection)
}

fn edit_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Submenu<Wry>> {
    let edit: Items = vec![
        Box::new(PredefinedMenuItem::undo(app, Some(model.label("Undo")))?),
        Box::new(PredefinedMenuItem::redo(app, Some(model.label("Redo")))?),
        separator(app)?,
        Box::new(PredefinedMenuItem::cut(app, Some(model.label("Cut")))?),
        Box::new(PredefinedMenuItem::copy(app, Some(model.label("Copy")))?),
        Box::new(PredefinedMenuItem::paste(app, Some(model.label("Paste")))?),
        Box::new(PredefinedMenuItem::select_all(
            app,
            Some(model.label("Select All")),
        )?),
    ];
    submenu(app, model.label("Edit"), &edit)
}

/// Builds the whole menu bar for `model`.
///
/// # Errors
/// When the platform menu can't be created.
pub fn build(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<Wry>> {
    let mac = model.platform == Platform::MacOS;
    let mut bar: Vec<Submenu<Wry>> = Vec::new();
    if mac {
        bar.push(app_menu(app, model)?);
    }
    bar.push(file_menu(app, model)?);
    bar.push(connection_menu(app, model)?);
    bar.push(edit_menu(app, model)?);
    let view: Items = vec![Box::new(command_item(
        app,
        model,
        ShortcutCommand::ToggleSidebar,
    )?)];
    bar.push(submenu(app, model.label("View"), &view)?);
    if mac {
        // No "Close Window" (⌘W): it is the default Close session shortcut.
        let window: Items = vec![
            Box::new(PredefinedMenuItem::minimize(app, Some(model.label("Minimize")))?),
            Box::new(PredefinedMenuItem::maximize(app, Some(model.label("Zoom")))?),
        ];
        bar.push(submenu(app, model.label("Window"), &window)?);
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = bar
        .iter()
        .map(|s| s as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    Menu::with_items(app, &refs)
}

/// Rebuilds the menu bar from the core's current state. Failures leave the old menu.
pub fn refresh(app: &AppHandle) {
    let Some(core) = app.try_state::<Core>() else {
        return;
    };
    let model = MenuModel::of(&core);
    if let Ok(menu) = build(app, &model) {
        let _ = app.set_menu(menu);
    }
}

/// Routes a menu click: Quit to the quit confirmation, everything else to the UI as `menu-command`.
pub fn on_menu_event(app: &AppHandle, event: &MenuEvent) {
    let id = event.id().as_ref();
    if id == QUIT {
        crate::quit::request_quit(app);
        return;
    }
    // Close (⌘W) in a file browser window closes that window, not the main window's tab.
    if id == ShortcutCommand::CloseTab.as_str()
        && let Some(window) = app
            .webview_windows()
            .into_values()
            .find(|w| crate::core::is_files_label(w.label()) && w.is_focused().unwrap_or(false))
    {
        let _ = window.close();
        return;
    }
    let known = [OPEN_SETTINGS, CHECK_FOR_UPDATES, DEV_OPEN_TEST_TAB].contains(&id)
        || id.parse::<ShortcutCommand>().is_ok();
    if known {
        let _ = app.emit(
            MENU_COMMAND,
            MenuCommand {
                command: id.to_owned(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(key: &str, modifiers: u8) -> KeyShortcut {
        KeyShortcut::new(key, Modifiers(modifiers))
    }

    #[test]
    fn accelerator_strings() {
        let mac = Platform::MacOS;
        assert_eq!(accelerator(&shortcut("n", 1), mac).as_deref(), Some("Cmd+N"));
        assert_eq!(
            accelerator(&shortcut("n", 3), mac).as_deref(),
            Some("Cmd+Shift+N")
        );
        assert_eq!(
            accelerator(&shortcut("n", 1 | 8), mac).as_deref(),
            Some("Cmd+Ctrl+N")
        );
        assert_eq!(
            accelerator(&shortcut("i", 1 | 4), mac).as_deref(),
            Some("Cmd+Alt+I")
        );
        assert_eq!(
            accelerator(&shortcut("tab", 8 | 2), mac).as_deref(),
            Some("Ctrl+Shift+Tab")
        );
        assert_eq!(
            accelerator(&shortcut("return", 1), mac).as_deref(),
            Some("Cmd+Enter")
        );
        assert_eq!(
            accelerator(&shortcut("delete", 1), mac).as_deref(),
            Some("Cmd+Backspace")
        );
        assert_eq!(accelerator(&shortcut("f5", 8), mac).as_deref(), Some("Ctrl+F5"));
        assert_eq!(accelerator(&shortcut("f12", 8), mac).as_deref(), Some("Ctrl+F12"));
        assert_eq!(
            accelerator(&shortcut("left", 4), mac).as_deref(),
            Some("Alt+ArrowLeft")
        );
        assert_eq!(accelerator(&shortcut(",", 1), mac).as_deref(), Some("Cmd+,"));
        assert_eq!(accelerator(&shortcut("7", 8), mac).as_deref(), Some("Ctrl+7"));
        assert_eq!(
            accelerator(&shortcut("space", 8), mac).as_deref(),
            Some("Ctrl+Space")
        );
        assert_eq!(
            accelerator(&shortcut("escape", 8), mac).as_deref(),
            Some("Ctrl+Escape")
        );
    }

    #[test]
    fn accelerator_on_windows_and_linux() {
        for platform in [Platform::Windows, Platform::Linux] {
            assert_eq!(
                accelerator(&shortcut("n", 8 | 2), platform).as_deref(),
                Some("Ctrl+Shift+N")
            );
            assert_eq!(
                accelerator(&shortcut("n", 1), platform).as_deref(),
                Some("Super+N")
            );
        }
    }

    #[test]
    fn keys_without_an_accelerator_name() {
        assert_eq!(accelerator(&shortcut("é", 1), Platform::MacOS), None);
        assert_eq!(accelerator(&shortcut("f13", 1), Platform::MacOS), None);
        assert_eq!(accelerator(&shortcut("fx", 1), Platform::MacOS), None);
    }

    #[test]
    fn every_default_has_an_accelerator() {
        for platform in Platform::ALL {
            for command in ShortcutCommand::ALL {
                let default = command.default_shortcut(platform);
                assert!(
                    accelerator(&default, platform).is_some(),
                    "{command:?} on {platform:?}"
                );
            }
        }
    }

    #[test]
    fn labels_translate() {
        assert_eq!(tr("hu", "File"), "Fájl");
        assert_eq!(tr("en", "File"), "File");
        assert_eq!(tr("hu", "Not in the table"), "Not in the table");
        for &command in ShortcutCommand::ALL {
            assert_ne!(
                tr("hu", command_title(command)),
                command_title(command),
                "{command:?}"
            );
        }
    }

    fn model(unlocked: bool, writable: bool) -> MenuModel {
        MenuModel {
            language: "en".into(),
            platform: Platform::MacOS,
            shortcuts: ShortcutCommand::ALL
                .iter()
                .map(|c| (*c, Some(c.default_shortcut(Platform::MacOS))))
                .collect(),
            unlocked,
            writable,
            sessions: false,
            rdp_sessions: false,
            files_target: false,
            dev: false,
        }
    }

    #[test]
    fn items_follow_the_lock_state() {
        let locked = model(false, false);
        assert!(!locked.enabled(ShortcutCommand::NewRdp));
        assert!(!locked.enabled(ShortcutCommand::Lock));
        assert!(!locked.enabled(ShortcutCommand::Connect));
        assert!(!locked.enabled(ShortcutCommand::NewLocalTerminal));

        let read_only = model(true, false);
        assert!(!read_only.enabled(ShortcutCommand::NewFolder));
        assert!(!read_only.enabled(ShortcutCommand::ImportRdm));
        assert!(read_only.enabled(ShortcutCommand::Connect));
        assert!(read_only.enabled(ShortcutCommand::NewLocalTerminal));

        let open = model(true, true);
        assert!(open.enabled(ShortcutCommand::NewCredential));
        assert!(open.enabled(ShortcutCommand::ImportRdm));
        assert!(open.enabled(ShortcutCommand::ImportWinbox));
        assert!(!locked.enabled(ShortcutCommand::ImportWinbox));
        assert!(open.enabled(ShortcutCommand::NextTab));
        assert!(!open.enabled(ShortcutCommand::SplitTerminal));
        assert!(!open.enabled(ShortcutCommand::BroadcastAll));
        assert!(!open.enabled(ShortcutCommand::TypeClipboard));
    }

    #[test]
    fn terminal_items_need_an_open_session() {
        let mut m = model(true, true);
        for command in [
            ShortcutCommand::ClearTerminal,
            ShortcutCommand::RecordOutput,
            ShortcutCommand::Reconnect,
            ShortcutCommand::SplitTerminal,
            ShortcutCommand::BroadcastAll,
        ] {
            assert!(!m.enabled(command), "{command:?}");
        }
        m.sessions = true;
        assert!(m.enabled(ShortcutCommand::SplitTerminal));
        assert!(m.enabled(ShortcutCommand::BroadcastAll));
        assert!(m.enabled(ShortcutCommand::ClearTerminal));
        assert!(m.enabled(ShortcutCommand::RecordOutput));
        assert!(m.enabled(ShortcutCommand::Reconnect));
        assert!(!m.enabled(ShortcutCommand::OpenFiles));
        m.files_target = true;
        assert!(m.enabled(ShortcutCommand::OpenFiles));
        m.unlocked = false;
        assert!(!m.enabled(ShortcutCommand::ClearTerminal));
        assert!(!m.enabled(ShortcutCommand::OpenFiles));
    }

    #[test]
    fn rdp_items_need_an_open_rdp_session() {
        let mut m = model(true, true);
        assert!(!m.enabled(ShortcutCommand::TypeClipboard));
        assert!(!m.enabled(ShortcutCommand::Reconnect));
        m.rdp_sessions = true;
        assert!(m.enabled(ShortcutCommand::TypeClipboard));
        assert!(m.enabled(ShortcutCommand::Reconnect));
        assert!(!m.enabled(ShortcutCommand::ClearTerminal));
        m.unlocked = false;
        assert!(!m.enabled(ShortcutCommand::TypeClipboard));
    }

    #[test]
    fn removed_shortcut_has_no_accelerator() {
        let mut m = model(true, true);
        assert_eq!(m.accelerator(ShortcutCommand::NewRdp).as_deref(), Some("Cmd+N"));
        m.shortcuts.insert(ShortcutCommand::NewRdp, None);
        assert_eq!(m.accelerator(ShortcutCommand::NewRdp), None);
        assert_eq!(m.settings_accelerator(), "Cmd+,");
        m.platform = Platform::Windows;
        assert_eq!(m.settings_accelerator(), "Ctrl+,");
    }

    #[test]
    fn product_name_fills_the_app_menu_labels() {
        let mut m = model(true, true);
        assert_eq!(
            m.label_with_product("Quit %@"),
            format!("Quit {}", APP_IDENTITY.product_name)
        );
        m.language = "hu".into();
        assert!(
            m.label_with_product("About %@")
                .contains(APP_IDENTITY.product_name)
        );
    }
}
