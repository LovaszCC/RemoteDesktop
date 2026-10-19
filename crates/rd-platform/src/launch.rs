//! Handing connections to other apps (Swift `Launchers/*`): the errors, process helpers and the
//! [`Launcher`] seam the app core calls, so its flows are tested without starting anything.

use std::fmt;
use std::io;
use std::process::{Command, Stdio};

use zeroize::Zeroizing;

/// Why a launch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// The app to hand the connection to isn't installed; carries its name.
    AppNotInstalled(String),
    /// Starting it failed; carries the OS message.
    OpenFailed(String),
    /// The clipboard can't be written.
    Clipboard(String),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppNotInstalled(name) => write!(f, "{name} is not installed."),
            Self::OpenFailed(detail) => write!(f, "Couldn't launch: {detail}"),
            Self::Clipboard(detail) => write!(f, "The clipboard is not available. ({detail})"),
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<io::Error> for LaunchError {
    fn from(error: io::Error) -> Self {
        Self::OpenFailed(error.to_string())
    }
}

impl From<crate::clipboard::ClipboardError> for LaunchError {
    fn from(error: crate::clipboard::ClipboardError) -> Self {
        Self::Clipboard(error.0)
    }
}

/// A process to start: the pure builders return these, so tests compare them as values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// The executable.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl ProcessSpec {
    /// `program args…`.
    #[must_use]
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }

    /// Starts it detached and reaps it in the background; the app outlives the call.
    ///
    /// # Errors
    /// When the program can't be started.
    pub fn spawn(&self) -> Result<(), LaunchError> {
        let mut child = self
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// Starts it detached with `input` written to its standard input, which is then closed (`FreeRDP`'s
    /// `/from-stdin`). The input never touches the command line.
    ///
    /// # Errors
    /// When the program can't be started.
    pub fn spawn_with_input(&self, input: Zeroizing<String>) -> Result<(), LaunchError> {
        use std::io::Write;
        let mut child = self
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take();
        std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(input.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
            let _ = child.wait();
        });
        Ok(())
    }

    /// Runs it to the end (`open(1)` hands off and exits); a non-zero exit is `OpenFailed` with its
    /// standard error (Swift `runProcess`).
    ///
    /// # Errors
    /// When it can't be started or fails.
    pub fn run(&self) -> Result<(), LaunchError> {
        let output = self.command().stdin(Stdio::null()).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(LaunchError::OpenFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }
}

/// `program` found on `PATH` (with the Windows executable extensions).
#[must_use]
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    std::env::split_paths(&path).find_map(|dir| {
        extensions.iter().find_map(|ext| {
            let candidate = dir.join(format!("{program}{ext}"));
            candidate.is_file().then_some(candidate)
        })
    })
}

/// The side effects of the launch flows. [`SystemLauncher`] does them for real; tests record them.
pub trait Launcher: Send + Sync {
    /// Opens `url` in the browser with `browser_id`, or the default one when it is `None` or unknown here.
    ///
    /// # Errors
    /// When nothing could open it.
    fn open_web(&self, url: &str, browser_id: Option<&str>) -> Result<(), LaunchError>;

    /// Opens `url` with the OS handler of its scheme (`smb://`, `ssh://`, ...).
    ///
    /// # Errors
    /// When nothing could open it.
    fn open_url(&self, url: &str) -> Result<(), LaunchError>;

    /// Copies `values` one after another, unmarked, one second apart.
    ///
    /// # Errors
    /// When the clipboard is unavailable.
    fn copy_sequence(&self, values: Vec<Zeroizing<String>>) -> Result<(), LaunchError>;

    /// Starts WinBox (the `configured` one, else the detected one) logging in with `login`.
    ///
    /// # Errors
    /// `AppNotInstalled` when there is no WinBox, or the start failed.
    fn launch_winbox(
        &self,
        configured: Option<&std::path::Path>,
        login: Vec<String>,
    ) -> Result<(), LaunchError>;

    /// Opens `connection` (resolved against its credential) in the system RDP client with `password`;
    /// `on_fallback` gets a catalog key when the password ends up on the clipboard later.
    ///
    /// # Errors
    /// `AppNotInstalled` when there is no client, else the start or clipboard failure.
    fn launch_rdp(
        &self,
        connection: &rd_core::Connection,
        password: Option<Zeroizing<String>>,
        on_fallback: crate::rdp::Fallback,
    ) -> Result<crate::rdp::RdpOutcome, LaunchError>;
}

/// The real launcher.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open_web(&self, url: &str, browser_id: Option<&str>) -> Result<(), LaunchError> {
        crate::web::open_url(url, browser_id)
    }

    fn open_url(&self, url: &str) -> Result<(), LaunchError> {
        #[cfg(target_os = "macos")]
        {
            ProcessSpec::new("/usr/bin/open", vec![url.to_owned()]).run()
        }
        #[cfg(not(target_os = "macos"))]
        {
            open::that_detached(url).map_err(LaunchError::from)
        }
    }

    fn copy_sequence(&self, values: Vec<Zeroizing<String>>) -> Result<(), LaunchError> {
        Ok(crate::clipboard::copy_sequence(
            values,
            crate::web::CLIPBOARD_INTERVAL,
        )?)
    }

    fn launch_winbox(
        &self,
        configured: Option<&std::path::Path>,
        login: Vec<String>,
    ) -> Result<(), LaunchError> {
        crate::winbox::launch(&crate::winbox::resolve(configured)?, login)
    }

    fn launch_rdp(
        &self,
        connection: &rd_core::Connection,
        password: Option<Zeroizing<String>>,
        on_fallback: crate::rdp::Fallback,
    ) -> Result<crate::rdp::RdpOutcome, LaunchError> {
        crate::rdp::launch(connection, password, on_fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_texts_match_swift() {
        assert_eq!(
            LaunchError::AppNotInstalled("WinBox".into()).to_string(),
            "WinBox is not installed."
        );
        assert_eq!(
            LaunchError::OpenFailed("boom".into()).to_string(),
            "Couldn't launch: boom"
        );
        let io = io::Error::new(io::ErrorKind::NotFound, "missing");
        assert_eq!(LaunchError::from(io), LaunchError::OpenFailed("missing".into()));
    }

    #[test]
    fn run_reports_the_exit_status() {
        let ok = if cfg!(windows) {
            ProcessSpec::new("cmd", vec!["/C".into(), "exit 0".into()])
        } else {
            ProcessSpec::new("sh", vec!["-c".into(), "exit 0".into()])
        };
        assert_eq!(ok.run(), Ok(()));
        if cfg!(not(windows)) {
            let failing = ProcessSpec::new("sh", vec!["-c".into(), "echo nope >&2; exit 3".into()]);
            assert_eq!(failing.run(), Err(LaunchError::OpenFailed("nope".into())));
        }
        let missing = ProcessSpec::new("rd-platform-no-such-program", Vec::new());
        assert!(matches!(missing.spawn(), Err(LaunchError::OpenFailed(_))));
    }

    #[test]
    fn which_finds_a_shell() {
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(which(shell).is_some());
        assert!(which("rd-platform-no-such-program").is_none());
    }
}
