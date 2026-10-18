//! SSH in the user's own terminal app (a port of the Swift `ExternalTerminal` and
//! `ExternalApps.terminalLaunch`), on every OS.
//!
//! There is no launch script any more: the terminal runs `ssh` directly, with the askpass variables put in
//! front by `/usr/bin/env` on macOS and Linux (a terminal that is already running would not see the app's
//! environment). On Windows, Windows Terminal (`wt.exe`) is used when installed, else a console window
//! (`conhost.exe`), which gets the environment from the app.
//!
//! The command-line builders are plain functions, tested on every OS; detection and spawning are per OS.

use std::io;
use std::process::Command;

use serde::Serialize;

/// What the terminal should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommand {
    /// The `ssh` executable (absolute).
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// The askpass variables.
    pub env: Vec<(String, String)>,
    /// The window or tab title (the connection name).
    pub title: String,
}

/// A process to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    /// The executable.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment for the started process itself.
    pub env: Vec<(String, String)>,
}

impl LaunchCommand {
    fn new(program: &str, args: Vec<String>) -> Self {
        Self {
            program: program.to_owned(),
            args,
            env: Vec::new(),
        }
    }

    /// Starts it detached; the terminal outlives the call.
    ///
    /// # Errors
    /// When the program can't be started.
    pub fn spawn(&self) -> io::Result<()> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn()?;
        // Reap it in the background so no zombie is left (`open` and `osascript` return at once).
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

/// A terminal the user can pick in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalChoice {
    /// The id stored in `Settings.external_terminal`.
    pub id: &'static str,
    /// The app name, shown untranslated.
    pub name: &'static str,
}

/// `/usr/bin/env K=V … ssh args…`: the command with its environment, for terminals that run a command
/// line.
#[must_use]
pub fn env_command(command: &SshCommand) -> Vec<String> {
    let mut line = vec!["/usr/bin/env".to_owned()];
    line.extend(command.env.iter().map(|(k, v)| format!("{k}={v}")));
    line.push(command.program.clone());
    line.extend(command.args.iter().cloned());
    line
}

/// POSIX single-quote escaping (Swift `SSHLaunchScript.quote`).
#[must_use]
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// An `AppleScript` string literal body: backslashes and double quotes escaped.
#[must_use]
pub fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// macOS: Ghostty, iTerm2 and Terminal, as in the Swift app.
pub mod macos {
    use super::{LaunchCommand, SshCommand, TerminalChoice, applescript_escape, env_command, shell_quote};

    /// A macOS terminal app.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MacTerminal {
        /// Ghostty.
        Ghostty,
        /// iTerm2.
        Iterm2,
        /// Terminal.app.
        Terminal,
    }

    impl MacTerminal {
        /// Every choice, in menu order.
        pub const ALL: [Self; 3] = [Self::Ghostty, Self::Iterm2, Self::Terminal];

        /// The stored id.
        #[must_use]
        pub fn id(self) -> &'static str {
            match self {
                Self::Ghostty => "ghostty",
                Self::Iterm2 => "iterm2",
                Self::Terminal => "terminal",
            }
        }

        /// The terminal with stored id `id`.
        #[must_use]
        pub fn from_id(id: &str) -> Option<Self> {
            Self::ALL.into_iter().find(|t| t.id() == id)
        }

        /// The bundle identifier.
        #[must_use]
        pub fn bundle_id(self) -> &'static str {
            match self {
                Self::Ghostty => "com.mitchellh.ghostty",
                Self::Iterm2 => "com.googlecode.iterm2",
                Self::Terminal => "com.apple.Terminal",
            }
        }

        /// The app name, shown untranslated.
        #[must_use]
        pub fn display_name(self) -> &'static str {
            match self {
                Self::Ghostty => "Ghostty",
                Self::Iterm2 => "iTerm2",
                Self::Terminal => "Terminal",
            }
        }

        /// For the Settings list.
        #[must_use]
        pub fn choice(self) -> TerminalChoice {
            TerminalChoice {
                id: self.id(),
                name: self.display_name(),
            }
        }

        /// The stored choice; when unset or unknown, Ghostty if installed (the earlier fixed choice), else
        /// Terminal (Swift `ExternalTerminal.current`).
        #[must_use]
        pub fn current(stored: Option<&str>, mut is_installed: impl FnMut(&str) -> bool) -> Self {
            if let Some(terminal) = stored.and_then(Self::from_id) {
                return terminal;
            }
            if is_installed(Self::Ghostty.bundle_id()) {
                Self::Ghostty
            } else {
                Self::Terminal
            }
        }
    }

    /// The `ssh` command line for a shell or iTerm2: POSIX-quoted.
    fn quoted_line(command: &SshCommand) -> String {
        env_command(command)
            .iter()
            .map(|part| shell_quote(part))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// New iTerm2 window with the default profile running the command (it has no command-line option for
    /// this). iTerm2 splits `command` like a shell, so it is POSIX-quoted, then escaped for `AppleScript`.
    #[must_use]
    pub fn iterm_applescript(command_line: &str) -> String {
        format!(
            "tell application id \"{}\"\n    activate\n    create window with default profile command \"{}\"\nend tell",
            MacTerminal::Iterm2.bundle_id(),
            applescript_escape(command_line)
        )
    }

    /// A new Terminal window whose shell clears the typed line and is replaced by `ssh` (`exec`), so the
    /// window ends with the connection.
    #[must_use]
    pub fn terminal_applescript(command_line: &str) -> String {
        format!(
            "tell application id \"{}\"\n    activate\n    do script \"{}\"\nend tell",
            MacTerminal::Terminal.bundle_id(),
            applescript_escape(&format!("clear; exec {command_line}"))
        )
    }

    /// The process that opens `terminal` running `command`:
    /// - Ghostty: `open -n -b com.mitchellh.ghostty --args -e /usr/bin/env … ssh …`;
    /// - iTerm2 and Terminal: `osascript -e <script>`.
    #[must_use]
    pub fn launch_command(terminal: MacTerminal, command: &SshCommand) -> LaunchCommand {
        match terminal {
            MacTerminal::Ghostty => {
                let mut args: Vec<String> = ["-n", "-b", terminal.bundle_id(), "--args", "-e"]
                    .map(str::to_owned)
                    .to_vec();
                args.extend(env_command(command));
                LaunchCommand::new("/usr/bin/open", args)
            }
            MacTerminal::Iterm2 => LaunchCommand::new(
                "/usr/bin/osascript",
                vec!["-e".into(), iterm_applescript(&quoted_line(command))],
            ),
            MacTerminal::Terminal => LaunchCommand::new(
                "/usr/bin/osascript",
                vec!["-e".into(), terminal_applescript(&quoted_line(command))],
            ),
        }
    }
}

/// Windows: Windows Terminal, else a console window.
pub mod windows {
    use super::{LaunchCommand, SshCommand, TerminalChoice};

    /// A Windows terminal.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WinTerminal {
        /// Windows Terminal (`wt.exe`).
        WindowsTerminal,
        /// A classic console window (`conhost.exe`).
        Conhost,
    }

    impl WinTerminal {
        /// Every choice, in menu order.
        pub const ALL: [Self; 2] = [Self::WindowsTerminal, Self::Conhost];

        /// The stored id.
        #[must_use]
        pub fn id(self) -> &'static str {
            match self {
                Self::WindowsTerminal => "wt",
                Self::Conhost => "conhost",
            }
        }

        /// The terminal with stored id `id`.
        #[must_use]
        pub fn from_id(id: &str) -> Option<Self> {
            Self::ALL.into_iter().find(|t| t.id() == id)
        }

        /// The app name, shown untranslated.
        #[must_use]
        pub fn display_name(self) -> &'static str {
            match self {
                Self::WindowsTerminal => "Windows Terminal",
                Self::Conhost => "Console Host",
            }
        }

        /// For the Settings list.
        #[must_use]
        pub fn choice(self) -> TerminalChoice {
            TerminalChoice {
                id: self.id(),
                name: self.display_name(),
            }
        }

        /// The stored choice if it is installed; else Windows Terminal when installed, else the console.
        #[must_use]
        pub fn current(stored: Option<&str>, has_wt: bool) -> Self {
            if has_wt && stored.and_then(Self::from_id) != Some(Self::Conhost) {
                Self::WindowsTerminal
            } else {
                Self::Conhost
            }
        }
    }

    /// `wt` treats `;` as a command separator anywhere on its command line.
    fn wt_escape(arg: &str) -> String {
        arg.replace(';', "\\;")
    }

    /// The process that opens `terminal` running `command`. The environment goes on the started process:
    /// `wt.exe` passes the caller's environment to the new tab, `conhost.exe` inherits it.
    /// - Windows Terminal: `wt.exe -w new new-tab --title <name> -- <ssh> args…`;
    /// - console: `conhost.exe <ssh> args…`.
    #[must_use]
    pub fn launch_command(
        terminal: WinTerminal,
        wt: &str,
        conhost: &str,
        command: &SshCommand,
    ) -> LaunchCommand {
        let mut launch = match terminal {
            WinTerminal::WindowsTerminal => {
                let mut args: Vec<String> = ["-w", "new", "new-tab", "--title"].map(str::to_owned).to_vec();
                args.push(wt_escape(&command.title));
                args.push("--".to_owned());
                args.push(wt_escape(&command.program));
                args.extend(command.args.iter().map(|a| wt_escape(a)));
                LaunchCommand::new(wt, args)
            }
            WinTerminal::Conhost => {
                let mut args = vec![command.program.clone()];
                args.extend(command.args.iter().cloned());
                LaunchCommand::new(conhost, args)
            }
        };
        launch.env.clone_from(&command.env);
        launch
    }
}

/// Linux: the configured terminal, then the usual ones.
pub mod linux {
    use super::{LaunchCommand, SshCommand, TerminalChoice, env_command};

    /// How a terminal takes the command to run.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ExecStyle {
        /// `term -e cmd args…`.
        DashE,
        /// `term -- cmd args…`.
        DoubleDash,
        /// `term cmd args…`.
        Bare,
    }

    /// A known Linux terminal: id (also the executable), name and exec style.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LinuxTerminal {
        /// Stored id and executable name.
        pub id: &'static str,
        /// The app name, shown untranslated.
        pub name: &'static str,
        /// How it takes a command.
        pub style: ExecStyle,
    }

    impl LinuxTerminal {
        /// For the Settings list.
        #[must_use]
        pub fn choice(self) -> TerminalChoice {
            TerminalChoice {
                id: self.id,
                name: self.name,
            }
        }
    }

    /// The fallback order after the configured terminal.
    pub const KNOWN: [LinuxTerminal; 6] = [
        LinuxTerminal {
            id: "x-terminal-emulator",
            name: "System default (x-terminal-emulator)",
            style: ExecStyle::DashE,
        },
        LinuxTerminal {
            id: "gnome-terminal",
            name: "GNOME Terminal",
            style: ExecStyle::DoubleDash,
        },
        LinuxTerminal {
            id: "konsole",
            name: "Konsole",
            style: ExecStyle::DashE,
        },
        LinuxTerminal {
            id: "kitty",
            name: "kitty",
            style: ExecStyle::Bare,
        },
        LinuxTerminal {
            id: "ghostty",
            name: "Ghostty",
            style: ExecStyle::DashE,
        },
        LinuxTerminal {
            id: "alacritty",
            name: "Alacritty",
            style: ExecStyle::DashE,
        },
    ];

    /// The known terminal with `id`.
    #[must_use]
    pub fn known(id: &str) -> Option<LinuxTerminal> {
        KNOWN.into_iter().find(|t| t.id == id)
    }

    /// The terminal to use: the configured one if installed, else the first installed in [`KNOWN`] order.
    #[must_use]
    pub fn current(
        stored: Option<&str>,
        mut is_installed: impl FnMut(&str) -> bool,
    ) -> Option<LinuxTerminal> {
        if let Some(terminal) = stored.and_then(known)
            && is_installed(terminal.id)
        {
            return Some(terminal);
        }
        KNOWN.into_iter().find(|t| is_installed(t.id))
    }

    /// `program` (the terminal's path) running `command` in `style`.
    #[must_use]
    pub fn launch_command(program: &str, style: ExecStyle, command: &SshCommand) -> LaunchCommand {
        let mut args = match style {
            ExecStyle::DashE => vec!["-e".to_owned()],
            ExecStyle::DoubleDash => vec!["--".to_owned()],
            ExecStyle::Bare => Vec::new(),
        };
        args.extend(env_command(command));
        LaunchCommand::new(program, args)
    }
}

/// The terminals installed here, in menu order, for Settings.
#[must_use]
pub fn installed_terminals() -> Vec<TerminalChoice> {
    #[cfg(target_os = "macos")]
    {
        macos::MacTerminal::ALL
            .into_iter()
            .filter(|t| *t == macos::MacTerminal::Terminal || mac_app_installed(t.bundle_id()))
            .map(macos::MacTerminal::choice)
            .collect()
    }
    #[cfg(windows)]
    {
        let mut list = Vec::new();
        if crate::locate::which("wt").is_some() {
            list.push(windows::WinTerminal::WindowsTerminal.choice());
        }
        list.push(windows::WinTerminal::Conhost.choice());
        list
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        linux::KNOWN
            .into_iter()
            .filter(|t| crate::locate::which(t.id).is_some())
            .map(linux::LinuxTerminal::choice)
            .collect()
    }
}

/// Whether an app with `bundle_id` is installed: the usual app folders, then Spotlight.
#[cfg(target_os = "macos")]
fn mac_app_installed(bundle_id: &str) -> bool {
    let bundle_name = match bundle_id {
        "com.mitchellh.ghostty" => "Ghostty.app",
        "com.googlecode.iterm2" => "iTerm.app",
        "com.apple.Terminal" => return true,
        _ => "",
    };
    let home = dirs::home_dir().map(|h| h.join("Applications"));
    let in_folder = [Some(std::path::PathBuf::from("/Applications")), home]
        .into_iter()
        .flatten()
        .any(|dir| !bundle_name.is_empty() && dir.join(bundle_name).exists());
    in_folder
        || Command::new("/usr/bin/mdfind")
            .arg(format!("kMDItemCFBundleIdentifier == '{bundle_id}'"))
            .output()
            .is_ok_and(|out| out.status.success() && !out.stdout.trim_ascii().is_empty())
}

/// Opens `command` in the terminal chosen by `stored` (`Settings.external_terminal`) or the OS default.
/// Returns the id of the terminal used.
///
/// # Errors
/// `NotFound` when no terminal is installed (Linux), or the spawn error.
pub fn launch(stored: Option<&str>, command: &SshCommand) -> io::Result<&'static str> {
    #[cfg(target_os = "macos")]
    {
        let terminal = macos::MacTerminal::current(stored, mac_app_installed);
        macos::launch_command(terminal, command).spawn()?;
        Ok(terminal.id())
    }
    #[cfg(windows)]
    {
        let wt = crate::locate::which("wt");
        let terminal = windows::WinTerminal::current(stored, wt.is_some());
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let conhost = format!("{windir}\\System32\\conhost.exe");
        let wt = wt.map(|p| p.display().to_string()).unwrap_or_default();
        windows::launch_command(terminal, &wt, &conhost, command).spawn()?;
        Ok(terminal.id())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let terminal = linux::current(stored, |id| crate::locate::which(id).is_some())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no terminal app is installed"))?;
        let program = crate::locate::which(terminal.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, terminal.id))?;
        linux::launch_command(&program.display().to_string(), terminal.style, command).spawn()?;
        Ok(terminal.id)
    }
}

#[cfg(test)]
mod tests {
    use super::linux::{ExecStyle, KNOWN};
    use super::macos::{MacTerminal, iterm_applescript, terminal_applescript};
    use super::windows::WinTerminal;
    use super::*;

    fn command() -> SshCommand {
        SshCommand {
            program: "/usr/bin/ssh".into(),
            args: vec!["-p".into(), "2222".into(), "u@h".into()],
            env: vec![("RD_ASKPASS_TOKEN".into(), "tok".into())],
            title: "web 01".into(),
        }
    }

    // macOS (Swift `ExternalTerminalTests` and `TerminalLaunchTests`).

    #[test]
    fn unset_prefers_ghostty_when_installed() {
        assert_eq!(MacTerminal::current(None, |_| true), MacTerminal::Ghostty);
    }

    #[test]
    fn unset_falls_back_to_terminal_without_ghostty() {
        let mut asked = Vec::new();
        let terminal = MacTerminal::current(None, |id| {
            asked.push(id.to_owned());
            false
        });
        assert_eq!(terminal, MacTerminal::Terminal);
        assert_eq!(asked, ["com.mitchellh.ghostty"]);
    }

    #[test]
    fn stored_choice_wins_even_when_ghostty_is_installed() {
        assert_eq!(
            MacTerminal::current(Some("iterm2"), |_| true),
            MacTerminal::Iterm2
        );
    }

    #[test]
    fn ids_round_trip_for_every_terminal() {
        for terminal in MacTerminal::ALL {
            assert_eq!(MacTerminal::from_id(terminal.id()), Some(terminal));
            assert_eq!(MacTerminal::current(Some(terminal.id()), |_| false), terminal);
        }
    }

    #[test]
    fn unknown_stored_value_falls_back_to_default() {
        assert_eq!(
            MacTerminal::current(Some("warp"), |_| false),
            MacTerminal::Terminal
        );
    }

    #[test]
    fn bundle_ids_and_names() {
        assert_eq!(MacTerminal::Ghostty.bundle_id(), "com.mitchellh.ghostty");
        assert_eq!(MacTerminal::Iterm2.bundle_id(), "com.googlecode.iterm2");
        assert_eq!(MacTerminal::Terminal.bundle_id(), "com.apple.Terminal");
        let names: Vec<_> = MacTerminal::ALL.iter().map(|t| t.display_name()).collect();
        assert_eq!(names, ["Ghostty", "iTerm2", "Terminal"]);
    }

    #[test]
    fn ghostty_runs_ssh_with_dash_e_through_env() {
        let launch = macos::launch_command(MacTerminal::Ghostty, &command());
        assert_eq!(launch.program, "/usr/bin/open");
        assert_eq!(
            launch.args,
            [
                "-n",
                "-b",
                "com.mitchellh.ghostty",
                "--args",
                "-e",
                "/usr/bin/env",
                "RD_ASKPASS_TOKEN=tok",
                "/usr/bin/ssh",
                "-p",
                "2222",
                "u@h"
            ]
        );
    }

    #[test]
    fn terminal_app_runs_ssh_through_applescript() {
        let launch = macos::launch_command(MacTerminal::Terminal, &command());
        assert_eq!(launch.program, "/usr/bin/osascript");
        assert_eq!(
            launch.args,
            [
                "-e",
                "tell application id \"com.apple.Terminal\"\n    activate\n    do script \"clear; exec \
                 '/usr/bin/env' 'RD_ASKPASS_TOKEN=tok' '/usr/bin/ssh' '-p' '2222' 'u@h'\"\nend tell"
            ]
        );
    }

    #[test]
    fn iterm_uses_applescript() {
        let launch = macos::launch_command(MacTerminal::Iterm2, &command());
        assert_eq!(launch.program, "/usr/bin/osascript");
        assert_eq!(
            launch.args,
            [
                "-e",
                "tell application id \"com.googlecode.iterm2\"\n    activate\n    create window with default \
                 profile command \"'/usr/bin/env' 'RD_ASKPASS_TOKEN=tok' '/usr/bin/ssh' '-p' '2222' 'u@h'\"\nend \
                 tell"
            ]
        );
    }

    #[test]
    fn iterm_script_escapes_quotes_and_backslashes() {
        let path = "/tmp/a \"b\" \\c'd/ssh";
        let script = iterm_applescript(&shell_quote(path));
        let line = script.lines().find(|l| l.contains("command")).unwrap();
        assert!(
            line.ends_with(r#"command "'/tmp/a \"b\" \\c'\\''d/ssh'""#),
            "{line}"
        );
        assert!(terminal_applescript("x\"y").contains(r#"do script "clear; exec x\"y""#));
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("a b"), "'a b'");
    }

    /// `AppleScript` decodes the literal back to the POSIX-quoted path.
    #[cfg(target_os = "macos")]
    #[test]
    fn iterm_literal_round_trips_through_osascript() {
        let path = "/tmp/a \"b\" \\c'd/ssh";
        let script = iterm_applescript(&shell_quote(path));
        let line = script.lines().find(|l| l.contains("command")).unwrap();
        let literal = &line[line.find('"').unwrap()..];
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", &format!("return {literal}")])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim_end_matches('\n'),
            shell_quote(path)
        );
    }

    // Windows.

    #[test]
    fn windows_prefers_windows_terminal_when_installed() {
        assert_eq!(WinTerminal::current(None, true), WinTerminal::WindowsTerminal);
        assert_eq!(WinTerminal::current(None, false), WinTerminal::Conhost);
        assert_eq!(WinTerminal::current(Some("conhost"), true), WinTerminal::Conhost);
        assert_eq!(WinTerminal::current(Some("wt"), false), WinTerminal::Conhost);
        assert_eq!(
            WinTerminal::current(Some("ghostty"), true),
            WinTerminal::WindowsTerminal
        );
        for terminal in WinTerminal::ALL {
            assert_eq!(WinTerminal::from_id(terminal.id()), Some(terminal));
        }
    }

    #[test]
    fn windows_terminal_opens_a_new_window_tab_with_the_environment() {
        let mut cmd = command();
        cmd.title = "a;b".into();
        cmd.program = "C:\\Windows\\System32\\OpenSSH\\ssh.exe".into();
        let launch = windows::launch_command(WinTerminal::WindowsTerminal, "C:\\wt.exe", "conhost.exe", &cmd);
        assert_eq!(launch.program, "C:\\wt.exe");
        assert_eq!(
            launch.args,
            [
                "-w",
                "new",
                "new-tab",
                "--title",
                "a\\;b",
                "--",
                "C:\\Windows\\System32\\OpenSSH\\ssh.exe",
                "-p",
                "2222",
                "u@h"
            ]
        );
        assert_eq!(launch.env, cmd.env);
    }

    #[test]
    fn console_window_runs_ssh_directly() {
        let launch = windows::launch_command(
            WinTerminal::Conhost,
            "",
            "C:\\Windows\\System32\\conhost.exe",
            &command(),
        );
        assert_eq!(launch.program, "C:\\Windows\\System32\\conhost.exe");
        assert_eq!(launch.args, ["/usr/bin/ssh", "-p", "2222", "u@h"]);
        assert_eq!(launch.env, [("RD_ASKPASS_TOKEN".to_owned(), "tok".to_owned())]);
    }

    // Linux.

    #[test]
    fn linux_prefers_the_configured_terminal_then_the_known_order() {
        let installed = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        let pick = |stored, ids| linux::current(stored, installed(ids)).map(|t| t.id);
        assert_eq!(pick(Some("kitty"), &["kitty", "konsole"]), Some("kitty"));
        assert_eq!(pick(Some("kitty"), &["alacritty", "konsole"]), Some("konsole"));
        assert_eq!(
            pick(None, &["alacritty", "x-terminal-emulator"]),
            Some("x-terminal-emulator")
        );
        assert_eq!(pick(Some("unknown"), &["ghostty"]), Some("ghostty"));
        assert_eq!(pick(None, &[]), None);
        let ids: Vec<_> = KNOWN.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                "x-terminal-emulator",
                "gnome-terminal",
                "konsole",
                "kitty",
                "ghostty",
                "alacritty"
            ]
        );
    }

    #[test]
    fn linux_exec_styles() {
        let tail = [
            "/usr/bin/env",
            "RD_ASKPASS_TOKEN=tok",
            "/usr/bin/ssh",
            "-p",
            "2222",
            "u@h",
        ];
        let args = |id: &str| {
            let terminal = linux::known(id).unwrap();
            linux::launch_command(&format!("/usr/bin/{id}"), terminal.style, &command()).args
        };
        for id in ["x-terminal-emulator", "konsole", "ghostty", "alacritty"] {
            assert_eq!(linux::known(id).unwrap().style, ExecStyle::DashE);
            assert_eq!(args(id)[0], "-e");
            assert_eq!(args(id)[1..], tail);
        }
        assert_eq!(args("gnome-terminal")[0], "--");
        assert_eq!(args("gnome-terminal")[1..], tail);
        assert_eq!(args("kitty"), tail);
    }

    #[test]
    fn installed_list_is_not_empty_on_desktops() {
        let list = installed_terminals();
        if cfg!(any(target_os = "macos", windows)) {
            assert!(!list.is_empty());
        }
        for choice in list {
            assert!(!choice.id.is_empty() && !choice.name.is_empty());
        }
    }
}
