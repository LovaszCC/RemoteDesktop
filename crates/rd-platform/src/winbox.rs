//! MikroTik WinBox (Swift `WinboxLauncher`, `ExternalApps.winboxOpenArguments`).
//!
//! WinBox logs in directly with `<host> [<user> [<password>]]`. It only accepts the password positionally
//! on its command line, so other local users can see it in the process list while WinBox runs: a known
//! limitation WinBox imposes, disclosed in Settings.
//!
//! Where WinBox is:
//! - macOS: the app with bundle id `com.mikrotik.winbox` (Launch Services), or an `.app` / binary set in
//!   Settings;
//! - Windows: `winbox64.exe` / `winbox.exe` on `PATH`, then the usual install and download folders;
//! - Linux: the native WinBox 4 binary (`WinBox`, `winbox`) on `PATH` or in the usual folders; a
//!   `winbox64.exe` (set in Settings, or found in the home or Downloads folder when Wine is installed)
//!   runs through Wine.
//!
//! The candidate lists and the command lines are plain functions, tested on every OS.

use std::path::{Path, PathBuf};

use crate::launch::{LaunchError, ProcessSpec};

/// The macOS bundle id.
pub const BUNDLE_ID: &str = "com.mikrotik.winbox";

/// The OS a command line is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// Linux and the other Unix desktops.
    Linux,
}

impl Os {
    /// This build's OS.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Where WinBox is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// macOS: opened through Launch Services by bundle id.
    Bundle,
    /// An `.app` bundle (macOS), an executable, or a `winbox64.exe` for Wine (Linux).
    Path(PathBuf),
}

/// `<host> [<user> [<password>]]`, trimmed; the password only with a user, and only when not empty (Swift
/// `ExternalApps.winboxOpenArguments`). The port is not passed: a MikroTik connection's port is its SSH
/// port, and WinBox uses its own (8291).
#[must_use]
pub fn login_args(host: &str, username: &str, password: Option<&str>) -> Vec<String> {
    let mut args = vec![host.trim().to_owned()];
    let user = username.trim();
    if user.is_empty() {
        return args;
    }
    args.push(user.to_owned());
    if let Some(password) = password.filter(|p| !p.is_empty()) {
        args.push(password.to_owned());
    }
    args
}

fn is_windows_exe(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// Whether this location runs through Wine on `os`.
#[must_use]
pub fn uses_wine(os: Os, location: &Location) -> bool {
    matches!((os, location), (Os::Linux, Location::Path(p)) if is_windows_exe(p))
}

/// The process that starts WinBox at `location` on `os` with `login` (from [`login_args`]):
/// - macOS bundle: `open -n -b com.mikrotik.winbox --args …`; an `.app`: `open -n -a <app> --args …`;
/// - a `winbox64.exe` on Linux: `<wine> <exe> …`;
/// - otherwise the executable itself.
#[must_use]
pub fn launch_command(os: Os, location: &Location, login: Vec<String>, wine: &str) -> ProcessSpec {
    let path = match location {
        Location::Bundle => {
            let mut args: Vec<String> = ["-n", "-b", BUNDLE_ID, "--args"].map(str::to_owned).to_vec();
            args.extend(login);
            return ProcessSpec::new("/usr/bin/open", args);
        }
        Location::Path(path) => path,
    };
    let path_text = path.display().to_string();
    if os == Os::MacOs && path.extension().is_some_and(|e| e == "app") {
        let mut args = vec!["-n".to_owned(), "-a".to_owned(), path_text, "--args".to_owned()];
        args.extend(login);
        return ProcessSpec::new("/usr/bin/open", args);
    }
    if uses_wine(os, location) {
        let mut args = vec![path_text];
        args.extend(login);
        return ProcessSpec::new(wine, args);
    }
    ProcessSpec::new(path_text, login)
}

/// Windows: where to look, in order. `path` is `PATH` split; the folders come from `%ProgramFiles%`,
/// `%ProgramFiles(x86)%`, `%LOCALAPPDATA%` and `%USERPROFILE%`.
#[must_use]
pub fn windows_candidates(
    path: &[PathBuf],
    program_files: &[PathBuf],
    local_app_data: Option<&Path>,
    user_profile: Option<&Path>,
) -> Vec<PathBuf> {
    const NAMES: [&str; 2] = ["winbox64.exe", "winbox.exe"];
    let mut list: Vec<PathBuf> = path.iter().flat_map(|dir| NAMES.map(|n| dir.join(n))).collect();
    for root in program_files {
        for dir in ["MikroTik\\WinBox", "WinBox", "MikroTik"] {
            list.push(root.join(dir).join("WinBox.exe"));
            list.extend(NAMES.map(|n| root.join(dir).join(n)));
        }
    }
    if let Some(local) = local_app_data {
        list.push(local.join("Programs\\WinBox\\WinBox.exe"));
        list.push(local.join("Programs\\MikroTik\\WinBox\\WinBox.exe"));
    }
    if let Some(profile) = user_profile {
        for dir in ["Downloads", "Desktop"] {
            list.extend(NAMES.map(|n| profile.join(dir).join(n)));
        }
    }
    list
}

/// Linux: where to look, in order: the native binary on `PATH` and in the usual folders, then (only when
/// `has_wine`) a `winbox64.exe` in the home or Downloads folder.
#[must_use]
pub fn linux_candidates(path: &[PathBuf], home: Option<&Path>, has_wine: bool) -> Vec<PathBuf> {
    const NAMES: [&str; 2] = ["WinBox", "winbox"];
    let mut list: Vec<PathBuf> = path.iter().flat_map(|dir| NAMES.map(|n| dir.join(n))).collect();
    for dir in ["/opt/winbox", "/opt/WinBox", "/opt/mikrotik/winbox"] {
        list.extend(NAMES.map(|n| Path::new(dir).join(n)));
    }
    if let Some(home) = home {
        list.push(home.join(".local/share/winbox/WinBox"));
        list.push(home.join("WinBox/WinBox"));
        list.push(home.join("Applications/WinBox/WinBox"));
        if has_wine {
            for dir in [".", "Downloads", ".local/share/winbox"] {
                list.push(home.join(dir).join("winbox64.exe"));
            }
        }
    }
    list
}

#[cfg(not(target_os = "macos"))]
fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// The WinBox found on this computer, without the Settings override.
#[must_use]
pub fn detect() -> Option<Location> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::is_installed(BUNDLE_ID).then_some(Location::Bundle)
    }
    #[cfg(windows)]
    {
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(env)
            .collect();
        windows_candidates(
            &path_dirs(),
            &program_files,
            env("LOCALAPPDATA").as_deref(),
            env("USERPROFILE").as_deref(),
        )
        .into_iter()
        .find(|p| p.is_file())
        .map(Location::Path)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let has_wine = crate::launch::which("wine").is_some();
        linux_candidates(&path_dirs(), dirs::home_dir().as_deref(), has_wine)
            .into_iter()
            .find(|p| p.is_file())
            .map(Location::Path)
    }
}

/// The path to show for a location: the bundle's app path on macOS.
#[must_use]
pub fn display_path(location: &Location) -> Option<String> {
    match location {
        Location::Path(path) => Some(path.display().to_string()),
        #[cfg(target_os = "macos")]
        Location::Bundle => crate::macos::app_path(BUNDLE_ID),
        #[cfg(not(target_os = "macos"))]
        Location::Bundle => None,
    }
}

/// The location to use: the Settings path when it exists, else the detected one.
///
/// # Errors
/// `AppNotInstalled("WinBox")` when neither exists.
pub fn resolve(configured: Option<&Path>) -> Result<Location, LaunchError> {
    if let Some(path) = configured {
        return if path.exists() {
            Ok(Location::Path(path.to_path_buf()))
        } else {
            Err(LaunchError::AppNotInstalled(format!(
                "WinBox ({})",
                path.display()
            )))
        };
    }
    detect().ok_or_else(|| LaunchError::AppNotInstalled("WinBox".to_owned()))
}

/// Starts WinBox at `location` logging in with `login`.
///
/// # Errors
/// When it can't be started (or Wine is missing for a `.exe` on Linux).
pub fn launch(location: &Location, login: Vec<String>) -> Result<(), LaunchError> {
    let os = Os::current();
    let wine = if uses_wine(os, location) {
        crate::launch::which("wine")
            .ok_or_else(|| LaunchError::AppNotInstalled("Wine".to_owned()))?
            .display()
            .to_string()
    } else {
        String::new()
    };
    let command = launch_command(os, location, login, &wine);
    if os == Os::MacOs {
        command.run()
    } else {
        command.spawn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn login_arguments_match_swift() {
        assert_eq!(
            login_args(" router.example ", "admin", Some("pw")),
            ["router.example", "admin", "pw"]
        );
        assert_eq!(
            login_args("router.example", "admin", None),
            ["router.example", "admin"]
        );
        assert_eq!(
            login_args("router.example", "admin", Some("")),
            ["router.example", "admin"]
        );
        assert_eq!(login_args("router.example", "", Some("pw")), ["router.example"]);
        assert_eq!(login_args("router.example", " ", Some("pw")), ["router.example"]);
    }

    #[test]
    fn mac_bundle_command_matches_swift() {
        let login = login_args("router.example", "admin", Some("pw"));
        let command = launch_command(Os::MacOs, &Location::Bundle, login, "");
        assert_eq!(command.program, "/usr/bin/open");
        assert_eq!(
            command.args,
            strings(&[
                "-n",
                "-b",
                "com.mikrotik.winbox",
                "--args",
                "router.example",
                "admin",
                "pw"
            ])
        );
    }

    #[test]
    fn mac_custom_app_or_binary() {
        let app = Location::Path("/Applications/Tools/WinBox.app".into());
        let command = launch_command(Os::MacOs, &app, strings(&["h"]), "");
        assert_eq!(
            (command.program.as_str(), command.args),
            (
                "/usr/bin/open",
                strings(&["-n", "-a", "/Applications/Tools/WinBox.app", "--args", "h"])
            )
        );
        let binary = Location::Path("/opt/WinBox".into());
        let command = launch_command(Os::MacOs, &binary, strings(&["h"]), "");
        assert_eq!(
            (command.program.as_str(), command.args),
            ("/opt/WinBox", strings(&["h"]))
        );
    }

    #[test]
    fn windows_and_linux_commands() {
        let exe = Location::Path(r"C:\Tools\winbox64.exe".into());
        let command = launch_command(Os::Windows, &exe, strings(&["h", "u"]), "");
        assert_eq!(
            (command.program.as_str(), command.args),
            (r"C:\Tools\winbox64.exe", strings(&["h", "u"]))
        );
        assert!(!uses_wine(Os::Windows, &exe));

        let native = Location::Path("/opt/winbox/WinBox".into());
        let command = launch_command(Os::Linux, &native, strings(&["h"]), "/usr/bin/wine");
        assert_eq!(
            (command.program.as_str(), command.args),
            ("/opt/winbox/WinBox", strings(&["h"]))
        );

        let wine = Location::Path("/home/u/Downloads/WINBOX64.EXE".into());
        assert!(uses_wine(Os::Linux, &wine));
        let command = launch_command(Os::Linux, &wine, strings(&["h", "u", "p"]), "/usr/bin/wine");
        assert_eq!(
            (command.program.as_str(), command.args),
            (
                "/usr/bin/wine",
                strings(&["/home/u/Downloads/WINBOX64.EXE", "h", "u", "p"])
            )
        );
        assert!(!uses_wine(Os::Linux, &Location::Bundle));
    }

    #[test]
    fn windows_candidate_order() {
        let list = windows_candidates(
            &[PathBuf::from(r"C:\bin")],
            &[PathBuf::from(r"C:\Program Files")],
            Some(Path::new(r"C:\Users\u\AppData\Local")),
            Some(Path::new(r"C:\Users\u")),
        );
        assert_eq!(list[0], Path::new(r"C:\bin").join("winbox64.exe"));
        assert_eq!(list[1], Path::new(r"C:\bin").join("winbox.exe"));
        assert_eq!(
            list[2],
            Path::new(r"C:\Program Files")
                .join(r"MikroTik\WinBox")
                .join("WinBox.exe")
        );
        assert!(list.contains(&Path::new(r"C:\Users\u\AppData\Local").join(r"Programs\WinBox\WinBox.exe")));
        assert_eq!(
            list.last().unwrap(),
            &Path::new(r"C:\Users\u").join("Desktop").join("winbox.exe")
        );
    }

    #[test]
    fn linux_candidates_add_wine_only_when_installed() {
        let path = [PathBuf::from("/usr/bin")];
        let home = Path::new("/home/u");
        let native = linux_candidates(&path, Some(home), false);
        assert_eq!(native[0], Path::new("/usr/bin/WinBox"));
        assert_eq!(native[1], Path::new("/usr/bin/winbox"));
        assert!(native.iter().all(|p| !is_windows_exe(p)));
        let with_wine = linux_candidates(&path, Some(home), true);
        assert_eq!(with_wine.len(), native.len() + 3);
        assert!(with_wine.contains(&home.join("Downloads").join("winbox64.exe")));
    }

    #[test]
    fn resolve_prefers_an_existing_configured_path() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("WinBox");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(resolve(Some(&exe)), Ok(Location::Path(exe.clone())));
        let missing = temp.path().join("missing");
        assert!(
            matches!(resolve(Some(&missing)), Err(LaunchError::AppNotInstalled(name)) if name.starts_with("WinBox ("))
        );
        assert_eq!(
            display_path(&Location::Path(exe.clone())),
            Some(exe.display().to_string())
        );
    }
}
