//! The connection editor's draft: validation and the normalisation applied on save.
//!
//! A port of the Swift `ConnectionDraft.isValid`, `AppState.save(_:)` and `WebLaunch.url(from:)`. Secrets
//! are not part of the draft: the caller seals a typed password or passphrase separately.

use serde::{Deserialize, Serialize};

use crate::model::{Connection, ConnectionType, LabelColor, MikrotikMode, RdpClient, SshTerminal};

/// The editable, secret-free fields of a connection as the editor sends them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDraft {
    /// Kind of connection.
    #[serde(rename = "type")]
    pub kind: ConnectionType,
    /// Containing folder, `None` at the root.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Linked credential.
    #[serde(default)]
    pub credential_id: Option<String>,
    /// Display name; required.
    #[serde(default)]
    pub name: String,
    /// Host, or the URL for web connections.
    #[serde(default)]
    pub host: String,
    /// Port as typed; empty means the type's default.
    #[serde(default)]
    pub port: String,
    /// Login user.
    #[serde(default)]
    pub username: String,
    /// Windows domain (RDP only).
    #[serde(default)]
    pub domain: String,
    /// SSH key path (SSH types only); empty means none.
    #[serde(default)]
    pub ssh_key_path: String,
    /// Free-form notes.
    #[serde(default)]
    pub notes: String,
    /// Shown under Favorites.
    #[serde(default)]
    pub is_favorite: bool,
    /// RDP only: connect to the administrative session.
    #[serde(default)]
    pub rdp_admin_session: bool,
    /// RDP: embedded tab or the system client.
    #[serde(default = "default_rdp_client")]
    pub rdp_client: RdpClient,
    /// Web only: browser id; `None` means the system default.
    #[serde(default)]
    pub browser_bundle_id: Option<String>,
    /// MikroTik: what double-click opens.
    #[serde(default = "default_mikrotik_mode")]
    pub mikrotik_mode: MikrotikMode,
    /// SSH types: embedded tab or external terminal.
    #[serde(default = "default_ssh_terminal")]
    pub ssh_terminal: SshTerminal,
    /// Tag color.
    #[serde(default)]
    pub color: Option<LabelColor>,
}

fn default_rdp_client() -> RdpClient {
    RdpClient::Embedded
}

fn default_mikrotik_mode() -> MikrotikMode {
    MikrotikMode::Winbox
}

fn default_ssh_terminal() -> SshTerminal {
    SshTerminal::Embedded
}

/// Why a draft can't be saved. [`code`](Self::code) is a stable id the UI translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DraftError {
    /// The name is empty after trimming.
    #[error("The name is required.")]
    EmptyName,
    /// Web: the URL can't be opened.
    #[error("The URL is not valid.")]
    InvalidUrl,
    /// The host is empty after trimming.
    #[error("The host is required.")]
    EmptyHost,
    /// The port is not empty and not a number from 1 to 65535.
    #[error("The port must be a number from 1 to 65535.")]
    InvalidPort,
}

impl DraftError {
    /// The stable error code: `emptyName`, `invalidUrl`, `emptyHost` or `invalidPort`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::EmptyName => "emptyName",
            Self::InvalidUrl => "invalidUrl",
            Self::EmptyHost => "emptyHost",
            Self::InvalidPort => "invalidPort",
        }
    }
}

/// Trims Swift's `.whitespaces` (spaces and tabs, not line breaks).
fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| {
        c.is_whitespace()
            && !matches!(
                c,
                '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
    })
}

fn uses_port(kind: ConnectionType) -> bool {
    kind != ConnectionType::Web
}

fn uses_ssh(kind: ConnectionType) -> bool {
    matches!(kind, ConnectionType::Ssh | ConnectionType::Mikrotik)
}

impl ConnectionDraft {
    /// A blank draft for a new connection.
    #[must_use]
    pub fn new(kind: ConnectionType, folder_id: Option<String>) -> Self {
        Self {
            kind,
            folder_id,
            credential_id: None,
            name: String::new(),
            host: String::new(),
            port: String::new(),
            username: String::new(),
            domain: String::new(),
            ssh_key_path: String::new(),
            notes: String::new(),
            is_favorite: false,
            rdp_admin_session: false,
            rdp_client: default_rdp_client(),
            browser_bundle_id: None,
            mikrotik_mode: default_mikrotik_mode(),
            ssh_terminal: default_ssh_terminal(),
            color: None,
        }
    }

    /// The editor's starting values for an existing connection.
    #[must_use]
    pub fn from_connection(connection: &Connection) -> Self {
        Self {
            kind: connection.kind,
            folder_id: connection.folder_id.clone(),
            credential_id: connection.credential_id.clone(),
            name: connection.name.clone(),
            host: connection.host.clone(),
            port: connection.port.map(|p| p.to_string()).unwrap_or_default(),
            username: connection.username.clone(),
            domain: connection.domain.clone(),
            ssh_key_path: connection.ssh_key_path.clone().unwrap_or_default(),
            notes: connection.notes.clone(),
            is_favorite: connection.is_favorite,
            rdp_admin_session: connection.rdp_admin_session,
            rdp_client: connection.rdp_client,
            browser_bundle_id: connection.browser_bundle_id.clone(),
            mikrotik_mode: connection.mikrotik_mode,
            ssh_terminal: connection.ssh_terminal,
            color: connection.color,
        }
    }

    /// Checks the draft in the Swift order: name, then the URL (web) or host and port.
    ///
    /// # Errors
    /// The first problem found.
    pub fn validate(&self) -> Result<(), DraftError> {
        if trim(&self.name).is_empty() {
            return Err(DraftError::EmptyName);
        }
        if !uses_port(self.kind) {
            return normalized_web_url(&self.host)
                .map(|_| ())
                .ok_or(DraftError::InvalidUrl);
        }
        if trim(&self.host).is_empty() {
            return Err(DraftError::EmptyHost);
        }
        self.parsed_port().map(|_| ())
    }

    /// The port to store: `None` when empty or for web connections.
    fn parsed_port(&self) -> Result<Option<u16>, DraftError> {
        if !uses_port(self.kind) || self.port.is_empty() {
            return Ok(None);
        }
        self.port
            .parse::<i64>()
            .ok()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|&p| p >= 1)
            .map(Some)
            .ok_or(DraftError::InvalidPort)
    }

    /// Validates, then writes the normalised fields into `connection` (Swift `AppState.save`).
    ///
    /// Name, host, user, domain and key path are trimmed. The port is dropped for web connections, the domain
    /// is kept only for RDP, the key path and passphrase only for SSH types, the admin flag only for RDP and
    /// the browser only for web. The id, dates, password and passphrase are left to the caller, except that
    /// the passphrase is cleared for types that don't use SSH.
    ///
    /// # Errors
    /// The validation error; `connection` is unchanged then.
    pub fn apply_to(&self, connection: &mut Connection) -> Result<(), DraftError> {
        self.validate()?;
        let ssh = uses_ssh(self.kind);
        let key_path = trim(&self.ssh_key_path);
        connection.kind = self.kind;
        connection.folder_id.clone_from(&self.folder_id);
        connection.credential_id.clone_from(&self.credential_id);
        trim(&self.name).clone_into(&mut connection.name);
        trim(&self.host).clone_into(&mut connection.host);
        connection.port = self.parsed_port()?;
        trim(&self.username).clone_into(&mut connection.username);
        connection.domain = if self.kind == ConnectionType::Rdp {
            trim(&self.domain).to_owned()
        } else {
            String::new()
        };
        connection.ssh_key_path = (ssh && !key_path.is_empty()).then(|| key_path.to_owned());
        if !ssh {
            connection.ssh_key_passphrase_enc = None;
        }
        connection.is_favorite = self.is_favorite;
        connection.rdp_admin_session = self.kind == ConnectionType::Rdp && self.rdp_admin_session;
        connection.browser_bundle_id = if self.kind == ConnectionType::Web {
            self.browser_bundle_id.clone()
        } else {
            None
        };
        connection.mikrotik_mode = self.mikrotik_mode;
        connection.ssh_terminal = self.ssh_terminal;
        connection.rdp_client = self.rdp_client;
        connection.color = self.color;
        connection.notes.clone_from(&self.notes);
        Ok(())
    }
}

/// The URL a web connection opens (Swift `WebLaunch.url(from:)`): trimmed, `https://` added when there is
/// no `://`, and a host required. `None` for an empty value, one with whitespace, or one without a host.
#[must_use]
pub fn normalized_web_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let (scheme, rest) = with_scheme.split_once("://")?;
    let mut scheme_chars = scheme.chars();
    if !scheme_chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        (host, after.strip_prefix(':'))
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if host.is_empty() || port.is_some_and(|p| !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(with_scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sealed;

    fn draft(kind: ConnectionType) -> ConnectionDraft {
        let mut draft = ConnectionDraft::new(kind, None);
        draft.name = "srv".into();
        draft.host = "srv.example.com".into();
        draft
    }

    /// `WebConnectionTests`: urlNormalisation.
    #[test]
    fn url_normalisation() {
        assert_eq!(
            normalized_web_url("router.example.com/admin").as_deref(),
            Some("https://router.example.com/admin")
        );
        assert_eq!(
            normalized_web_url("  http://192.0.2.1:8080/x ").as_deref(),
            Some("http://192.0.2.1:8080/x")
        );
        assert_eq!(
            normalized_web_url("https://example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(normalized_web_url(""), None);
        assert_eq!(normalized_web_url("not a url"), None);
        assert_eq!(normalized_web_url("https://"), None);
    }

    #[test]
    fn url_edge_cases() {
        assert_eq!(
            normalized_web_url("https://user@[2001:db8::1]:8443/").as_deref(),
            Some("https://user@[2001:db8::1]:8443/")
        );
        assert_eq!(normalized_web_url("https://host:port"), None);
        assert_eq!(normalized_web_url("1http://example.com"), None);
        assert_eq!(normalized_web_url("https:///path"), None);
        assert_eq!(normalized_web_url("exa\tmple.com"), None);
    }

    #[test]
    fn validation_order_and_rules() {
        let mut d = draft(ConnectionType::Ssh);
        assert_eq!(d.validate(), Ok(()));
        d.port = "65535".into();
        assert_eq!(d.validate(), Ok(()));
        for bad in ["0", "65536", "-1", "abc", " 22"] {
            d.port = bad.into();
            assert_eq!(d.validate(), Err(DraftError::InvalidPort), "{bad}");
        }
        d.host = "  ".into();
        assert_eq!(d.validate(), Err(DraftError::EmptyHost));
        d.name = " \t".into();
        assert_eq!(d.validate(), Err(DraftError::EmptyName));
        assert_eq!(DraftError::EmptyName.code(), "emptyName");
        assert_eq!(
            serde_json::to_string(&DraftError::InvalidPort).unwrap(),
            "\"invalidPort\""
        );
    }

    #[test]
    fn web_validates_the_url_and_ignores_the_port() {
        let mut d = draft(ConnectionType::Web);
        d.port = "junk".into();
        assert_eq!(d.validate(), Ok(()));
        d.host = "not a url".into();
        assert_eq!(d.validate(), Err(DraftError::InvalidUrl));
    }

    #[test]
    fn apply_trims_and_clears_per_type() {
        let mut d = draft(ConnectionType::Ssh);
        d.name = "  web01 ".into();
        d.host = " 192.0.2.5\t".into();
        d.port = "2222".into();
        d.username = " root ".into();
        d.domain = "CORP".into();
        d.ssh_key_path = " ~/.ssh/id_ed25519 ".into();
        d.rdp_admin_session = true;
        d.browser_bundle_id = Some("com.example.browser".into());
        d.notes = " keep ".into();
        let mut c = Connection::new(ConnectionType::Rdp, "old", "old");
        c.ssh_key_passphrase_enc = Some(Sealed::from_bytes(vec![1]));
        d.apply_to(&mut c).unwrap();
        assert_eq!(c.kind, ConnectionType::Ssh);
        assert_eq!((c.name.as_str(), c.host.as_str()), ("web01", "192.0.2.5"));
        assert_eq!(c.port, Some(2222));
        assert_eq!(c.username, "root");
        assert_eq!(c.domain, "");
        assert_eq!(c.ssh_key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert!(c.ssh_key_passphrase_enc.is_some());
        assert!(!c.rdp_admin_session);
        assert_eq!(c.browser_bundle_id, None);
        assert_eq!(c.notes, " keep ");

        let mut rdp = draft(ConnectionType::Rdp);
        rdp.domain = " CORP ".into();
        rdp.ssh_key_path = "~/.ssh/k".into();
        rdp.rdp_admin_session = true;
        d = rdp;
        d.apply_to(&mut c).unwrap();
        assert_eq!(c.domain, "CORP");
        assert_eq!(c.port, None);
        assert_eq!(c.ssh_key_path, None);
        assert_eq!(c.ssh_key_passphrase_enc, None);
        assert!(c.rdp_admin_session);
    }

    #[test]
    fn apply_web_keeps_browser_and_drops_port() {
        let mut d = draft(ConnectionType::Web);
        d.port = "8080".into();
        d.browser_bundle_id = Some("com.example.browser".into());
        let mut c = Connection::new(ConnectionType::Web, "w", "w");
        c.port = Some(1);
        d.apply_to(&mut c).unwrap();
        assert_eq!(c.port, None);
        assert_eq!(c.browser_bundle_id.as_deref(), Some("com.example.browser"));
    }

    #[test]
    fn apply_refuses_an_invalid_draft_without_changes() {
        let mut d = draft(ConnectionType::Ssh);
        d.port = "99999".into();
        let mut c = Connection::new(ConnectionType::Rdp, "old", "old");
        let before = c.clone();
        assert_eq!(d.apply_to(&mut c), Err(DraftError::InvalidPort));
        assert_eq!(c, before);
    }

    #[test]
    fn round_trips_a_connection_and_deserializes_camel_case() {
        let mut c = Connection::new(ConnectionType::Mikrotik, "r", "192.0.2.1");
        c.port = Some(8291);
        c.ssh_key_path = Some("~/.ssh/k".into());
        let d = ConnectionDraft::from_connection(&c);
        assert_eq!(d.port, "8291");
        let mut copy = c.clone();
        d.apply_to(&mut copy).unwrap();
        assert_eq!(copy, c);

        let parsed: ConnectionDraft = serde_json::from_str(
            r#"{"type":"rdp","name":"n","host":"h","rdpAdminSession":true,"rdpClient":"windowsApp"}"#,
        )
        .unwrap();
        assert_eq!(parsed.kind, ConnectionType::Rdp);
        assert!(parsed.rdp_admin_session);
        assert_eq!(parsed.rdp_client, RdpClient::WindowsApp);
        assert_eq!(parsed.mikrotik_mode, MikrotikMode::Winbox);
        assert!(serde_json::from_str::<ConnectionDraft>(r#"{"name":"n"}"#).is_err());
    }
}
