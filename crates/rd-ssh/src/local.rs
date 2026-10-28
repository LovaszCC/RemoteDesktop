//! The local shell of the "Local Terminal" sidebar entry: which program runs, with which arguments, and
//! which inherited variables it must not see. It runs in the same PTY session as `ssh`
//! ([`crate::SshSession`] with [`crate::session::SessionKind::Local`]).
//!
//! Unix: `$SHELL` as a login shell (`-l`), else `/bin/zsh` on macOS or `/bin/bash`, then `/bin/sh`.
//! Windows: `pwsh.exe` from `PATH`, else Windows `PowerShell`, else `%COMSPEC%` (`cmd.exe`).
//!
//! A new local terminal can start in another one's directory: [`process_directory`] asks the OS where that
//! shell is now (macOS `lsof`, Linux `/proc/<pid>/cwd`); the shell's own reports (OSC 7) aren't relied on,
//! as the default macOS zsh sends none.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::askpass::{PROMPT_HINT_ENV, SOCKET_ENV, TOKEN_ENV};

/// The askpass variables: removed from a local shell's environment, so a shell started from the app
/// never talks to its askpass server (and the app's executable never starts in helper mode in it).
pub const REMOVED_ENV: [&str; 5] = [
    "SSH_ASKPASS",
    "SSH_ASKPASS_REQUIRE",
    PROMPT_HINT_ENV,
    TOKEN_ENV,
    SOCKET_ENV,
];

/// The OS rules the shell choice follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOs {
    /// macOS: zsh is the default shell.
    MacOS,
    /// Windows: `PowerShell` or `cmd.exe`.
    Windows,
    /// Linux and the other Unixes: bash.
    OtherUnix,
}

impl ShellOs {
    /// The OS this build runs on.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::OtherUnix
        }
    }
}

/// The shell to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalShell {
    /// The executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
}

impl LocalShell {
    fn new(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|&a| a.to_owned()).collect(),
        }
    }
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|v| !v.is_empty())
}

/// Chooses the shell for `os` from the environment (`var`), with `exists` telling whether an absolute
/// path is an executable file and `on_path` finding an executable name on `PATH`. A relative or missing
/// `$SHELL` is ignored. There is always an answer: the last fallbacks (`/bin/sh`, `cmd.exe`) are not
/// checked.
pub fn choose_shell(
    os: ShellOs,
    var: impl Fn(&str) -> Option<OsString>,
    exists: impl Fn(&Path) -> bool,
    on_path: impl Fn(&str) -> Option<PathBuf>,
) -> LocalShell {
    if os == ShellOs::Windows {
        if let Some(pwsh) = on_path("pwsh.exe") {
            return LocalShell::new(pwsh, &["-NoLogo"]);
        }
        let bundled = non_empty(var("SystemRoot"))
            .or_else(|| non_empty(var("WINDIR")))
            .map(|root| {
                PathBuf::from(root)
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            })
            .filter(|p| exists(p));
        if let Some(powershell) = on_path("powershell.exe").or(bundled) {
            return LocalShell::new(powershell, &["-NoLogo"]);
        }
        return LocalShell::new(non_empty(var("COMSPEC")).unwrap_or_else(|| "cmd.exe".into()), &[]);
    }
    let from_env = non_empty(var("SHELL"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && exists(p));
    let fallbacks: &[&str] = if os == ShellOs::MacOS {
        &["/bin/zsh", "/bin/bash"]
    } else {
        &["/bin/bash"]
    };
    let program = from_env
        .or_else(|| fallbacks.iter().map(PathBuf::from).find(|p| exists(p)))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    LocalShell::new(program, &["-l"])
}

/// The shell for this machine.
#[must_use]
pub fn local_shell() -> LocalShell {
    choose_shell(
        ShellOs::current(),
        |name| std::env::var_os(name),
        crate::locate::is_executable,
        |file| crate::locate::search_path(file, std::env::var_os("PATH").as_deref()),
    )
}

/// The user the shell runs as, for the root warning: `USER`, `LOGNAME` or `USERNAME`; empty when unset.
pub fn login_user(var: impl Fn(&str) -> Option<OsString>) -> String {
    ["USER", "LOGNAME", "USERNAME"]
        .into_iter()
        .find_map(|name| non_empty(var(name)))
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The current working directory of process `pid`, from the OS: `lsof` on macOS, `/proc/<pid>/cwd` on
/// Linux. `None` elsewhere (Windows), or when the process is gone or the answer isn't a directory.
#[must_use]
pub fn process_directory(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let directory = {
        let output = std::process::Command::new("/usr/sbin/lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        parse_lsof_cwd(&String::from_utf8_lossy(&output.stdout))
    };
    #[cfg(target_os = "linux")]
    let directory = std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let directory: Option<PathBuf> = {
        let _ = pid;
        None
    };
    directory.filter(|d| d.is_dir())
}

/// The path of `lsof -Fn` output: the first `n` field line (`p<pid>`, `f<fd>` and the others are skipped).
#[must_use]
pub fn parse_lsof_cwd(output: &str) -> Option<PathBuf> {
    output
        .lines()
        .find_map(|line| line.strip_prefix('n'))
        .filter(|path| path.starts_with('/'))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn files(present: &[&str]) -> impl Fn(&Path) -> bool {
        let present: Vec<PathBuf> = present.iter().map(PathBuf::from).collect();
        move |path| present.iter().any(|p| p == path)
    }

    fn path_of(present: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
        let map: HashMap<String, PathBuf> = present
            .iter()
            .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    // Unix paths aren't absolute on a Windows host (no drive), so these run on Unix only.
    #[cfg(unix)]
    #[test]
    fn unix_uses_shell_as_a_login_shell() {
        let shell = choose_shell(
            ShellOs::OtherUnix,
            env(&[("SHELL", "/usr/bin/fish")]),
            files(&["/usr/bin/fish", "/bin/bash"]),
            path_of(&[]),
        );
        assert_eq!(shell, LocalShell::new("/usr/bin/fish", &["-l"]));
    }

    #[cfg(unix)]
    #[test]
    fn unix_ignores_an_empty_relative_or_missing_shell() {
        for value in ["", "zsh", "/opt/gone/zsh"] {
            let shell = choose_shell(
                ShellOs::MacOS,
                env(&[("SHELL", value)]),
                files(&["/bin/zsh", "/bin/bash"]),
                path_of(&[]),
            );
            assert_eq!(shell, LocalShell::new("/bin/zsh", &["-l"]), "{value:?}");
        }
        let shell = choose_shell(
            ShellOs::OtherUnix,
            env(&[]),
            files(&["/bin/zsh", "/bin/bash"]),
            path_of(&[]),
        );
        assert_eq!(
            shell,
            LocalShell::new("/bin/bash", &["-l"]),
            "no zsh fallback off macOS"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_falls_back_to_bin_sh() {
        let shell = choose_shell(ShellOs::MacOS, env(&[]), files(&[]), path_of(&[]));
        assert_eq!(shell, LocalShell::new("/bin/sh", &["-l"]));
        let shell = choose_shell(ShellOs::MacOS, env(&[]), files(&["/bin/bash"]), path_of(&[]));
        assert_eq!(shell, LocalShell::new("/bin/bash", &["-l"]));
    }

    #[test]
    fn windows_prefers_pwsh_then_windows_powershell_then_comspec() {
        let shell = choose_shell(
            ShellOs::Windows,
            env(&[
                ("COMSPEC", "C:\\Windows\\system32\\cmd.exe"),
                ("SHELL", "/bin/zsh"),
            ]),
            files(&[]),
            path_of(&[
                ("pwsh.exe", "C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
                ("powershell.exe", "C:\\ps.exe"),
            ]),
        );
        assert_eq!(
            shell,
            LocalShell::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe", &["-NoLogo"])
        );

        let shell = choose_shell(
            ShellOs::Windows,
            env(&[]),
            files(&[]),
            path_of(&[("powershell.exe", "C:\\ps.exe")]),
        );
        assert_eq!(shell, LocalShell::new("C:\\ps.exe", &["-NoLogo"]));

        let bundled = PathBuf::from("C:\\Windows")
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let shell = choose_shell(
            ShellOs::Windows,
            env(&[("SystemRoot", "C:\\Windows")]),
            |p: &Path| p == bundled,
            path_of(&[]),
        );
        assert_eq!(shell, LocalShell::new(bundled.clone(), &["-NoLogo"]));

        let shell = choose_shell(
            ShellOs::Windows,
            env(&[("SystemRoot", "C:\\Windows"), ("COMSPEC", "C:\\cmd.exe")]),
            files(&[]),
            path_of(&[]),
        );
        assert_eq!(shell, LocalShell::new("C:\\cmd.exe", &[]));
        let shell = choose_shell(ShellOs::Windows, env(&[]), files(&[]), path_of(&[]));
        assert_eq!(shell, LocalShell::new("cmd.exe", &[]));
    }

    #[test]
    fn this_machine_has_a_shell() {
        let shell = local_shell();
        assert!(!shell.program.as_os_str().is_empty());
        if cfg!(unix) {
            assert!(shell.program.is_absolute());
            assert_eq!(shell.args, ["-l"]);
        }
    }

    #[test]
    fn login_user_from_the_environment() {
        assert_eq!(login_user(env(&[("USER", "root"), ("USERNAME", "x")])), "root");
        assert_eq!(login_user(env(&[("USER", ""), ("LOGNAME", "admin")])), "admin");
        assert_eq!(login_user(env(&[("USERNAME", "Admin")])), "Admin");
        assert_eq!(login_user(env(&[])), "");
    }

    #[test]
    fn removes_every_askpass_variable() {
        let set = crate::askpass::askpass_env(Path::new("/app"), "/sock", "tok");
        for (name, _) in set {
            assert!(REMOVED_ENV.contains(&name.as_str()), "{name}");
        }
        assert!(REMOVED_ENV.contains(&PROMPT_HINT_ENV));
    }
    #[test]
    fn reads_the_path_of_lsof_field_output() {
        assert_eq!(
            parse_lsof_cwd("p4242\nfcwd\nn/Users/me/Documents\n"),
            Some(PathBuf::from("/Users/me/Documents"))
        );
        assert_eq!(
            parse_lsof_cwd("p1\nfcwd\nn/Users/me/with space\n"),
            Some(PathBuf::from("/Users/me/with space"))
        );
        assert_eq!(parse_lsof_cwd("p4242\nfcwd\n"), None);
        assert_eq!(parse_lsof_cwd(""), None);
        assert_eq!(parse_lsof_cwd("nrelative\n"), None);
    }

    #[test]
    fn finds_the_directory_of_a_running_process() {
        let directory = process_directory(std::process::id());
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
            assert_eq!(directory.map(|d| d.canonicalize().unwrap()), Some(expected));
        } else {
            assert_eq!(directory, None);
        }
        assert_eq!(process_directory(u32::MAX), None);
    }
}
