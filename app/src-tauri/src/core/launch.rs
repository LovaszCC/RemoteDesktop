//! Connections handed to other apps (Swift `AppState.connect` with `WebLauncher`, `WinboxLauncher` and
//! `RDPLauncher`). The side effects go through [`Launcher`], so the flows are tested with a recorder.

use rd_core::{Connection, ConnectionType};
use rd_platform::launch::{LaunchError, Launcher};
use rd_platform::rdp::RdpOutcome;
use rd_platform::{web, winbox};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use zeroize::Zeroizing;

use super::{AppCore, CommandError};

/// What the UI tells the user after a launch (Swift `LaunchOutcome`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LaunchOutcomeDto {
    /// Started: "Connecting: name".
    Launched,
    /// Browser opened; the password, then the username are on the clipboard.
    LoginCopied,
    /// The password is on the clipboard for 30 s; `reason` is a catalog key.
    PasswordCopied {
        /// Why it wasn't typed in, e.g. "Accessibility permission missing".
        reason: String,
    },
}

impl From<LaunchError> for CommandError {
    fn from(error: LaunchError) -> Self {
        match error {
            LaunchError::AppNotInstalled(name) => {
                Self::new("appNotInstalled", "%@ is not installed.").with_detail(name)
            }
            LaunchError::OpenFailed(detail) => {
                Self::new("launchFailed", "Couldn't launch: %@").with_detail(detail)
            }
            LaunchError::Clipboard(detail) => {
                Self::new("clipboard", "The clipboard is not available.").with_detail(detail)
            }
        }
    }
}

/// Where WinBox is, for Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinboxStatusDto {
    /// The detected WinBox (an app or executable path); `None` when not found.
    pub detected_path: Option<String>,
    /// Whether the path set in Settings exists; `None` when none is set.
    pub configured_exists: Option<bool>,
    /// The WinBox in use is a `winbox64.exe` started through Wine (Linux).
    pub uses_wine: bool,
}

/// A launch problem found after the command returned (the macOS autofill found no password field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchNoticeDto {
    /// The connection launched.
    pub connection_name: String,
    /// A catalog key, e.g. "Couldn't find the password field — password on the clipboard (30 s)".
    pub message: String,
}

/// Delivers [`LaunchNoticeDto`]s.
pub type LaunchNotifier = std::sync::Arc<dyn Fn(LaunchNoticeDto) + Send + Sync>;

/// A resolved connection and its password, ready to launch.
pub(super) struct Target {
    pub connection: Connection,
    pub password: Option<SecretString>,
}

impl Target {
    /// The password, empty when none is saved.
    pub fn password(&self) -> Option<&str> {
        self.password
            .as_ref()
            .map(ExposeSecret::expose_secret)
            .filter(|p| !p.is_empty())
    }
}

impl AppCore {
    /// Replaces the launcher (tests record the launches instead of starting apps).
    #[must_use]
    pub fn with_launcher(mut self, launcher: Box<dyn Launcher>) -> Self {
        self.launcher = launcher;
        self
    }

    /// The connection with its linked credential applied and its password, when it has `kind`.
    pub(super) fn launch_target(
        &self,
        connection_id: &str,
        kind: ConnectionType,
    ) -> Result<Target, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::unlocked(&inner)?;
        let library = db.library()?;
        let connection = library
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| CommandError::not_found(connection_id))?;
        if connection.kind != kind {
            return Err(
                CommandError::new("wrongType", "This connection can't be opened this way.")
                    .with_detail(connection.kind.as_str()),
            );
        }
        let connection = library.resolved(connection);
        let password = vault.open_optional(connection.password_enc.as_ref())?;
        Ok(Target { connection, password })
    }

    /// Opens a web connection in its browser (the default one when unset or unknown here), then copies the
    /// password and, a second later, the username, unmarked (Swift `WebLauncher.launch`).
    ///
    /// # Errors
    /// `locked`, `notFound`, `wrongType`, `invalidUrl` (detail: the stored value), `launchFailed`,
    /// `clipboard`.
    pub fn connect_web(&self, connection_id: &str) -> Result<LaunchOutcomeDto, CommandError> {
        let target = self.launch_target(connection_id, ConnectionType::Web)?;
        let connection = &target.connection;
        let url = rd_core::draft::normalized_web_url(&connection.host).ok_or_else(|| {
            CommandError::new("invalidUrl", "Invalid URL: %@").with_detail(&connection.host)
        })?;
        self.launcher
            .open_web(&url, connection.browser_bundle_id.as_deref())?;
        let sequence: Vec<Zeroizing<String>> =
            web::clipboard_sequence(&connection.username, target.password())
                .into_iter()
                .map(Zeroizing::new)
                .collect();
        if sequence.is_empty() {
            return Ok(LaunchOutcomeDto::Launched);
        }
        self.launcher.copy_sequence(sequence)?;
        Ok(LaunchOutcomeDto::LoginCopied)
    }

    /// Starts WinBox for a MikroTik connection, logging in with `<host> [<user> [<password>]]` (Swift
    /// `WinboxLauncher.launch`). The WinBox set in Settings wins over the detected one.
    ///
    /// # Errors
    /// `locked`, `notFound`, `wrongType`, `appNotInstalled` (detail: "WinBox", or "WinBox (<path>)" for a
    /// missing configured path, or "Wine"), `launchFailed`.
    pub fn connect_winbox(&self, connection_id: &str) -> Result<LaunchOutcomeDto, CommandError> {
        let target = self.launch_target(connection_id, ConnectionType::Mikrotik)?;
        let configured = self.inner().settings.winbox_path.clone();
        let login = winbox::login_args(
            &target.connection.host,
            &target.connection.username,
            target.password(),
        );
        self.launcher.launch_winbox(configured.as_deref(), login)?;
        Ok(LaunchOutcomeDto::Launched)
    }

    /// Where WinBox is, for Settings.
    #[must_use]
    pub fn winbox_status(&self) -> WinboxStatusDto {
        let configured = self.inner().settings.winbox_path.clone();
        let detected = winbox::detect();
        let os = winbox::Os::current();
        let effective = configured
            .clone()
            .map(winbox::Location::Path)
            .or_else(|| detected.clone());
        WinboxStatusDto {
            detected_path: detected.as_ref().and_then(winbox::display_path),
            configured_exists: configured.as_ref().map(|p| p.exists()),
            uses_wine: effective.is_some_and(|l| winbox::uses_wine(os, &l)),
        }
    }

    /// Sets where late launch notices go (the Tauri `launch-notice` event).
    pub fn set_launch_notifier(&self, notifier: LaunchNotifier) {
        *self
            .launch_notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(notifier);
    }

    /// Opens an RDP connection in the system RDP client (`rdpClient = windowsApp` on every OS): Windows
    /// App with the Accessibility autofill on macOS, `mstsc` with a session credential on Windows,
    /// Remmina or `xfreerdp` on Linux (Swift `RDPLauncher.launch`). When the macOS autofill gives up later,
    /// a `launch-notice` says the password is on the clipboard.
    ///
    /// # Errors
    /// `locked`, `notFound`, `wrongType`, `appNotInstalled`, `launchFailed`, `clipboard`.
    pub fn connect_rdp_external(&self, connection_id: &str) -> Result<LaunchOutcomeDto, CommandError> {
        let target = self.launch_target(connection_id, ConnectionType::Rdp)?;
        let notifier = self
            .launch_notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let name = target.connection.name.clone();
        let on_fallback: rd_platform::rdp::Fallback = Box::new(move |message| {
            if let Some(notify) = notifier {
                notify(LaunchNoticeDto {
                    connection_name: name,
                    message: message.to_owned(),
                });
            }
        });
        let password = target.password().map(|p| Zeroizing::new(p.to_owned()));
        Ok(
            match self
                .launcher
                .launch_rdp(&target.connection, password, on_fallback)?
            {
                RdpOutcome::Launched => LaunchOutcomeDto::Launched,
                RdpOutcome::PasswordCopied(reason) => LaunchOutcomeDto::PasswordCopied {
                    reason: reason.to_owned(),
                },
            },
        )
    }

    /// The macOS Accessibility permission for the RDP autofill; `None` on the other OSes.
    #[must_use]
    pub fn accessibility_status() -> Option<bool> {
        rd_platform::rdp::accessibility_trusted(false)
    }

    /// Asks for the Accessibility permission and opens System Settings at Accessibility (macOS).
    ///
    /// # Errors
    /// `launchFailed` when System Settings can't be opened.
    pub fn open_accessibility_settings() -> Result<(), CommandError> {
        Ok(rd_platform::rdp::open_accessibility_settings()?)
    }

    /// The browsers registered for `https` here, sorted by name.
    #[must_use]
    pub fn installed_browsers() -> Vec<web::Browser> {
        web::installed_browsers()
    }
}
