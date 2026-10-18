//! Web connections (Swift `WebLauncher`, `InstalledBrowsers`, `WebLaunch`): the browsers
//! registered for `https`, opening a URL in one of them, and the clipboard login sequence.
//!
//! A browser's id is what `browserBundleId` stores: a bundle id on macOS, the `StartMenuInternet` key name
//! on Windows, a `.desktop` file id on Linux. An id that is unknown on this OS opens the default browser.
//!
//! The parsers and command-line builders are plain functions, tested on every OS; enumeration and spawning
//! are per OS.

use std::time::Duration;

use serde::Serialize;

use crate::launch::LaunchError;
#[cfg(not(target_os = "macos"))]
use crate::launch::ProcessSpec;

/// Pause between the clipboard writes so clipboard managers (Raycast, Win+V, Klipper) record each value.
pub const CLIPBOARD_INTERVAL: Duration = Duration::from_secs(1);

/// The values copied after opening, in order: password first, username last, so the username is on the
/// clipboard and the password one step back in the history (Swift `WebLaunch.clipboardSequence`).
#[must_use]
pub fn clipboard_sequence(username: &str, password: Option<&str>) -> Vec<String> {
    [password.unwrap_or_default(), username]
        .into_iter()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A browser for the connection editor's picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    /// The stored id.
    pub id: String,
    /// The app name, shown untranslated.
    pub name: String,
}

/// Sorts by name (case-insensitive) and drops repeated ids, keeping the first.
fn sorted_unique(browsers: impl IntoIterator<Item = Browser>) -> Vec<Browser> {
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<Browser> = browsers
        .into_iter()
        .filter(|b| seen.insert(b.id.clone()))
        .collect();
    list.sort_by_key(|b| b.name.to_lowercase());
    list
}

/// macOS: `open [-b <bundle id>] <url>`, the default browser without an id (Swift
/// `ExternalApps.browserOpenArguments`).
#[must_use]
pub fn mac_open_args(url: &str, bundle_id: Option<&str>) -> Vec<String> {
    match bundle_id.filter(|id| !id.is_empty()) {
        Some(id) => vec!["-b".to_owned(), id.to_owned(), url.to_owned()],
        None => vec![url.to_owned()],
    }
}

/// Linux: freedesktop `.desktop` entries.
pub mod desktop {
    use std::path::{Path, PathBuf};

    /// The `[Desktop Entry]` keys a browser needs.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DesktopEntry {
        /// The desktop file id (`firefox.desktop`, `org.gnome.Epiphany.desktop`).
        pub id: String,
        /// `Name` (unlocalized).
        pub name: String,
        /// `Exec`.
        pub exec: String,
        /// `MimeType`, split.
        pub mime_types: Vec<String>,
        /// `NoDisplay` or `Hidden` is true, or it is no application.
        pub hidden: bool,
    }

    impl DesktopEntry {
        /// Whether it is a visible application that opens `https` links.
        #[must_use]
        pub fn is_browser(&self) -> bool {
            !self.hidden && self.mime_types.iter().any(|m| m == "x-scheme-handler/https")
        }
    }

    /// Parses the `[Desktop Entry]` group of a `.desktop` file; `None` without a `Name` or `Exec`.
    #[must_use]
    pub fn parse(id: &str, text: &str) -> Option<DesktopEntry> {
        let mut in_entry = false;
        let (mut name, mut exec, mut mime, mut hidden, mut application) =
            (None, None, Vec::new(), false, true);
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "Name" => name = Some(value.to_owned()),
                "Exec" => exec = Some(value.to_owned()),
                "MimeType" => {
                    mime = value
                        .split(';')
                        .filter(|m| !m.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                "NoDisplay" | "Hidden" if value == "true" => hidden = true,
                "Type" => application = value == "Application",
                _ => {}
            }
        }
        Some(DesktopEntry {
            id: id.to_owned(),
            name: name?,
            exec: exec?,
            mime_types: mime,
            hidden: hidden || !application,
        })
    }

    /// Splits an `Exec` value into words: double quotes group, a backslash escapes inside them.
    fn words(exec: &str) -> Option<Vec<String>> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut started = false;
        let mut chars = exec.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    started = true;
                    loop {
                        match chars.next()? {
                            '"' => break,
                            '\\' => current.push(chars.next()?),
                            other => current.push(other),
                        }
                    }
                }
                ' ' | '\t' => {
                    if started {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                other => {
                    started = true;
                    current.push(other);
                }
            }
        }
        if started {
            words.push(current);
        }
        Some(words)
    }

    /// The command line that opens `url`: `%u`/`%U`/`%f`/`%F` become the URL (appended when there is
    /// none), `%%` a percent sign, and the other field codes are dropped. `None` for a malformed value.
    #[must_use]
    pub fn exec_args(exec: &str, url: &str) -> Option<Vec<String>> {
        let mut args = Vec::new();
        let mut placed = false;
        for word in words(exec)? {
            match word.as_str() {
                "%u" | "%U" | "%f" | "%F" => {
                    if !placed {
                        args.push(url.to_owned());
                        placed = true;
                    }
                }
                "%i" | "%c" | "%k" => {}
                _ => {
                    let mut out = String::new();
                    let mut chars = word.chars();
                    while let Some(c) = chars.next() {
                        if c == '%' {
                            match chars.next() {
                                Some('%') => out.push('%'),
                                Some('u' | 'U' | 'f' | 'F') if !placed => {
                                    out.push_str(url);
                                    placed = true;
                                }
                                _ => {}
                            }
                        } else {
                            out.push(c);
                        }
                    }
                    args.push(out);
                }
            }
        }
        if args.is_empty() {
            return None;
        }
        if !placed {
            args.push(url.to_owned());
        }
        Some(args)
    }

    /// The XDG data directories in precedence order: `$XDG_DATA_HOME` (or `~/.local/share`), then
    /// `$XDG_DATA_DIRS` (or `/usr/local/share:/usr/share`).
    #[must_use]
    pub fn data_dirs(data_home: Option<&str>, data_dirs: Option<&str>, home: Option<&Path>) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        match data_home.filter(|v| !v.is_empty()) {
            Some(dir) => dirs.push(PathBuf::from(dir)),
            None => dirs.extend(home.map(|h| h.join(".local/share"))),
        }
        let system = data_dirs
            .filter(|v| !v.is_empty())
            .unwrap_or("/usr/local/share:/usr/share");
        dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
        dirs
    }

    fn scan_into(root: &Path, dir: &Path, depth: usize, found: &mut Vec<DesktopEntry>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() && depth < 4 {
                scan_into(root, &path, depth + 1, found);
            } else if path.extension().is_some_and(|e| e == "desktop") {
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                let id = relative.to_string_lossy().replace(['/', '\\'], "-");
                if found.iter().any(|e| e.id == id) {
                    continue;
                }
                if let Some(entry) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&id, &t)) {
                    found.push(entry);
                }
            }
        }
    }

    /// Every entry under `<dir>/applications` of `dirs`; an id found earlier hides later ones.
    #[must_use]
    pub fn scan(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
        let mut found = Vec::new();
        for dir in dirs {
            let root = dir.join("applications");
            scan_into(&root, &root, 0, &mut found);
        }
        found
    }

    /// The entries of this session's data directories.
    #[must_use]
    pub fn system_entries() -> Vec<DesktopEntry> {
        let home = dirs::home_dir();
        scan(&data_dirs(
            std::env::var("XDG_DATA_HOME").ok().as_deref(),
            std::env::var("XDG_DATA_DIRS").ok().as_deref(),
            home.as_deref(),
        ))
    }
}

/// Windows: the `SOFTWARE\Clients\StartMenuInternet` registry keys.
pub mod start_menu {
    use super::Browser;

    /// One subkey: its name, default value and `shell\open\command` default value.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RegistryClient {
        /// The key name: the stored id.
        pub key: String,
        /// The default value: the display name.
        pub name: Option<String>,
        /// `shell\open\command`.
        pub command: Option<String>,
    }

    /// A browser with the program that opens a URL.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RegisteredBrowser {
        /// Id and name.
        pub browser: Browser,
        /// The executable.
        pub program: String,
    }

    /// The executable of a `shell\open\command` value: the quoted part, else up to `.exe`, else the first
    /// word.
    #[must_use]
    pub fn command_program(command: &str) -> Option<String> {
        let command = command.trim();
        let program = if let Some(rest) = command.strip_prefix('"') {
            rest.split_once('"').map_or(rest, |(p, _)| p)
        } else if let Some(end) = command.to_ascii_lowercase().find(".exe") {
            &command[..end + 4]
        } else {
            command.split_whitespace().next().unwrap_or_default()
        };
        (!program.is_empty()).then(|| program.to_owned())
    }

    /// The browsers of the current user's (`HKCU`) and the machine's (`HKLM`) keys; a user key hides the
    /// machine key of the same name. Keys without a usable command are left out.
    #[must_use]
    pub fn browsers(user: Vec<RegistryClient>, machine: Vec<RegistryClient>) -> Vec<RegisteredBrowser> {
        let mut list: Vec<RegisteredBrowser> = Vec::new();
        for client in user.into_iter().chain(machine) {
            if list
                .iter()
                .any(|b| b.browser.id.eq_ignore_ascii_case(&client.key))
            {
                continue;
            }
            let Some(program) = client.command.as_deref().and_then(command_program) else {
                continue;
            };
            let name = client
                .name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| client.key.clone());
            list.push(RegisteredBrowser {
                browser: Browser { id: client.key, name },
                program,
            });
        }
        list
    }

    /// Reads both registry hives.
    #[cfg(windows)]
    #[must_use]
    pub fn system_browsers() -> Vec<RegisteredBrowser> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};

        fn read(hive: winreg::HKEY) -> Vec<RegistryClient> {
            let Ok(root) =
                RegKey::predef(hive).open_subkey_with_flags(r"SOFTWARE\Clients\StartMenuInternet", KEY_READ)
            else {
                return Vec::new();
            };
            root.enum_keys()
                .flatten()
                .filter_map(|key| {
                    let sub = root.open_subkey_with_flags(&key, KEY_READ).ok()?;
                    let name = sub.get_value::<String, _>("").ok();
                    let command = sub
                        .open_subkey_with_flags(r"shell\open\command", KEY_READ)
                        .ok()
                        .and_then(|c| c.get_value::<String, _>("").ok());
                    Some(RegistryClient { key, name, command })
                })
                .collect()
        }

        browsers(read(HKEY_CURRENT_USER), read(HKEY_LOCAL_MACHINE))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSString, NSURL};

    use super::Browser;
    use crate::macos::{app_url, display_name};

    /// Launch Services' handlers for `https` (Swift `InstalledBrowsers.all`).
    pub fn browsers() -> Vec<Browser> {
        let Some(probe) = NSURL::URLWithString(&NSString::from_str("https://example.com")) else {
            return Vec::new();
        };
        NSWorkspace::sharedWorkspace()
            .URLsForApplicationsToOpenURL(&probe)
            .iter()
            .filter_map(|app| {
                let id = NSBundle::bundleWithURL(&app)?.bundleIdentifier()?.to_string();
                Some(Browser {
                    id,
                    name: display_name(&app)?,
                })
            })
            .collect()
    }

    /// The display name of the app with `bundle_id`.
    pub fn name(bundle_id: &str) -> Option<String> {
        display_name(&*app_url(bundle_id)?)
    }
}

/// The browsers registered for `https` here, sorted by name.
#[must_use]
pub fn installed_browsers() -> Vec<Browser> {
    #[cfg(target_os = "macos")]
    {
        sorted_unique(mac::browsers())
    }
    #[cfg(windows)]
    {
        sorted_unique(start_menu::system_browsers().into_iter().map(|b| b.browser))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        sorted_unique(
            desktop::system_entries()
                .into_iter()
                .filter(desktop::DesktopEntry::is_browser)
                .map(|e| Browser {
                    id: e.id,
                    name: e.name,
                }),
        )
    }
}

/// The name of the browser with `id`; `None` when it isn't installed here.
#[must_use]
pub fn browser_name(id: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        mac::name(id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        installed_browsers()
            .into_iter()
            .find(|b| b.id == id)
            .map(|b| b.name)
    }
}

/// The process that opens `url` in the browser with `id` here, `None` when the id is unknown.
#[cfg(not(target_os = "macos"))]
fn browser_command(id: &str, url: &str) -> Option<ProcessSpec> {
    #[cfg(windows)]
    {
        start_menu::system_browsers()
            .into_iter()
            .find(|b| b.browser.id.eq_ignore_ascii_case(id))
            .map(|b| ProcessSpec::new(b.program, vec![url.to_owned()]))
    }
    #[cfg(not(windows))]
    {
        let entry = desktop::system_entries()
            .into_iter()
            .find(|e| e.id == id && !e.hidden)?;
        let mut args = desktop::exec_args(&entry.exec, url)?;
        let program = args.remove(0);
        Some(ProcessSpec::new(program, args))
    }
}

/// Opens `url` in the browser with `id`; the default browser when `id` is `None` or unknown here.
///
/// # Errors
/// When the browser (or the default handler) can't be started.
pub fn open_url(url: &str, id: Option<&str>) -> Result<(), LaunchError> {
    #[cfg(target_os = "macos")]
    {
        let id = id.filter(|id| crate::macos::is_installed(id));
        crate::launch::ProcessSpec::new("/usr/bin/open", mac_open_args(url, id)).run()
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(command) = id.and_then(|id| browser_command(id, url)) {
            return command.spawn();
        }
        open::that_detached(url).map_err(LaunchError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::desktop::{self, DesktopEntry};
    use super::start_menu::{self, RegistryClient};
    use super::*;

    #[test]
    fn clipboard_sequence_password_then_username() {
        assert_eq!(clipboard_sequence("bob", Some("pw")), ["pw", "bob"]);
        assert_eq!(clipboard_sequence("bob", None), ["bob"]);
        assert_eq!(clipboard_sequence("", Some("pw")), ["pw"]);
        assert!(clipboard_sequence("", Some("")).is_empty());
    }

    #[test]
    fn mac_open_arguments() {
        let url = "https://example.com/login";
        assert_eq!(mac_open_args(url, None), [url]);
        assert_eq!(mac_open_args(url, Some("")), [url]);
        assert_eq!(
            mac_open_args(url, Some("com.brave.Browser")),
            ["-b", "com.brave.Browser", url]
        );
    }

    #[test]
    fn sorts_by_name_and_drops_repeats() {
        let b = |id: &str, name: &str| Browser {
            id: id.into(),
            name: name.into(),
        };
        let list = sorted_unique([
            b("z", "zen"),
            b("a", "Arc"),
            b("z", "Zen again"),
            b("f", "Firefox"),
        ]);
        let names: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Arc", "Firefox", "zen"]);
    }

    const FIREFOX: &str = "[Desktop Entry]\nVersion=1.0\nName=Firefox Web Browser\nName[hu]=Firefox böngésző\n\
        Exec=firefox %u\nType=Application\n\
        MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;\n\n\
        [Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n";

    #[test]
    fn parses_a_browser_entry() {
        let entry = desktop::parse("firefox.desktop", FIREFOX).unwrap();
        assert_eq!(entry.name, "Firefox Web Browser");
        assert_eq!(entry.exec, "firefox %u");
        assert!(entry.is_browser());
        assert!(entry.mime_types.contains(&"x-scheme-handler/http".to_owned()));
    }

    #[test]
    fn hidden_and_non_browser_entries_are_not_browsers() {
        let hidden = FIREFOX.replace("Type=Application", "Type=Application\nNoDisplay=true");
        assert!(!desktop::parse("a.desktop", &hidden).unwrap().is_browser());
        let link = FIREFOX.replace("Type=Application", "Type=Link");
        assert!(!desktop::parse("a.desktop", &link).unwrap().is_browser());
        let editor = "[Desktop Entry]\nName=Editor\nExec=editor %F\nMimeType=text/plain;\n";
        assert!(!desktop::parse("e.desktop", editor).unwrap().is_browser());
        assert_eq!(
            desktop::parse("x.desktop", "[Desktop Entry]\nName=No exec\n"),
            None
        );
    }

    #[test]
    fn exec_field_codes() {
        let url = "https://example.com/a?b=1&c=%20";
        assert_eq!(desktop::exec_args("firefox %u", url).unwrap(), ["firefox", url]);
        assert_eq!(
            desktop::exec_args("/usr/bin/google-chrome-stable %U --incognito", url).unwrap(),
            ["/usr/bin/google-chrome-stable", url, "--incognito"]
        );
        assert_eq!(
            desktop::exec_args("\"/opt/My Browser/browser\" --name=%c %i %u", url).unwrap(),
            ["/opt/My Browser/browser", "--name=", url]
        );
        assert_eq!(desktop::exec_args("epiphany", url).unwrap(), ["epiphany", url]);
        assert_eq!(
            desktop::exec_args("sh -c \"echo 100%%\" --url=%u", url).unwrap(),
            ["sh", "-c", "echo 100%", &format!("--url={url}")]
        );
        assert_eq!(desktop::exec_args("\"unterminated", url), None);
        assert_eq!(desktop::exec_args("   ", url), None);
    }

    #[test]
    fn xdg_data_dirs() {
        let home = Path::new("/home/user");
        assert_eq!(
            desktop::data_dirs(None, None, Some(home)),
            [
                PathBuf::from("/home/user/.local/share"),
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share")
            ]
        );
        assert_eq!(
            desktop::data_dirs(Some("/x/data"), Some("/a::/b"), Some(home)),
            [PathBuf::from("/x/data"), PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn scans_applications_with_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user");
        let system = temp.path().join("system");
        std::fs::create_dir_all(user.join("applications")).unwrap();
        std::fs::create_dir_all(system.join("applications/org.example")).unwrap();
        std::fs::write(
            user.join("applications/firefox.desktop"),
            FIREFOX.replace("Firefox Web Browser", "My Firefox"),
        )
        .unwrap();
        std::fs::write(system.join("applications/firefox.desktop"), FIREFOX).unwrap();
        std::fs::write(
            system.join("applications/org.example/Browser.desktop"),
            FIREFOX.replace("Firefox Web Browser", "Example"),
        )
        .unwrap();
        std::fs::write(system.join("applications/readme.txt"), "x").unwrap();
        let entries: Vec<DesktopEntry> = desktop::scan(&[user, system, temp.path().join("missing")]);
        let ids: Vec<(&str, &str)> = entries.iter().map(|e| (e.id.as_str(), e.name.as_str())).collect();
        assert_eq!(
            ids,
            [
                ("firefox.desktop", "My Firefox"),
                ("org.example-Browser.desktop", "Example")
            ]
        );
    }

    #[test]
    fn start_menu_command_programs() {
        assert_eq!(
            start_menu::command_program(r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#)
                .as_deref(),
            Some(r"C:\Program Files\Mozilla Firefox\firefox.exe")
        );
        assert_eq!(
            start_menu::command_program(r"C:\Program Files\Browser\browser.EXE --flag").as_deref(),
            Some(r"C:\Program Files\Browser\browser.EXE")
        );
        assert_eq!(
            start_menu::command_program("iexplore"),
            Some("iexplore".to_owned())
        );
        assert_eq!(start_menu::command_program("  "), None);
    }

    #[test]
    fn start_menu_user_keys_hide_machine_keys() {
        let client = |key: &str, name: Option<&str>, command: Option<&str>| RegistryClient {
            key: key.into(),
            name: name.map(str::to_owned),
            command: command.map(str::to_owned),
        };
        let user = vec![client(
            "Google Chrome",
            Some("Google Chrome"),
            Some(r#""C:\Users\u\AppData\Local\Google\Chrome\Application\chrome.exe""#),
        )];
        let machine = vec![
            client(
                "GOOGLE CHROME",
                Some("Chrome (machine)"),
                Some(r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#),
            ),
            client(
                "Microsoft Edge",
                Some(""),
                Some(r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe""#),
            ),
            client("Broken", Some("Broken"), None),
        ];
        let list = start_menu::browsers(user, machine);
        let rows: Vec<(&str, &str, &str)> = list
            .iter()
            .map(|b| (b.browser.id.as_str(), b.browser.name.as_str(), b.program.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "Google Chrome",
                    "Google Chrome",
                    r"C:\Users\u\AppData\Local\Google\Chrome\Application\chrome.exe"
                ),
                (
                    "Microsoft Edge",
                    "Microsoft Edge",
                    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
                ),
            ]
        );
    }

    #[test]
    fn browser_list_serializes_camel_case() {
        let json = serde_json::to_value(Browser {
            id: "firefox.desktop".into(),
            name: "Firefox".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"id": "firefox.desktop", "name": "Firefox"})
        );
    }

    /// Lists the real browsers (no launch); run by hand with `cargo test -p rd-platform -- --ignored`.
    #[test]
    #[ignore = "reads the OS browser registry"]
    fn lists_installed_browsers() {
        let list = installed_browsers();
        assert!(!list.is_empty());
        for browser in &list {
            assert_eq!(browser_name(&browser.id).as_deref(), Some(browser.name.as_str()));
        }
    }
}
