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
    /// Host (web: the host of the URL, without scheme, port or path).
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
    /// Web only: the URL scheme, `https` by default.
    #[serde(default = "default_web_scheme")]
    pub web_scheme: String,
    /// Web only: the path, with any `?query#fragment`; empty means none.
    #[serde(default)]
    pub web_path: String,
}

fn default_web_scheme() -> String {
    "https".to_owned()
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
            web_scheme: default_web_scheme(),
            web_path: String::new(),
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
            web_scheme: connection.web_scheme.clone(),
            web_path: connection.web_path.clone(),
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
        if self.kind == ConnectionType::Web {
            return self.web_parts().map(|_| ());
        }
        if trim(&self.host).is_empty() {
            return Err(DraftError::EmptyHost);
        }
        self.parsed_port().map(|_| ())
    }

    /// The parts of a web draft's URL. A host holding a whole URL (`://`, or a path) is split and replaces the draft's
    /// scheme, port and path; otherwise the host may only be `[userinfo@]host` (a bracketed IPv6 address is
    /// fine) and the scheme must be a valid one.
    fn web_parts(&self) -> Result<WebParts, DraftError> {
        let host = trim(&self.host);
        if host.contains("://") || host.contains(['/', '?', '#']) {
            return split_web_url(host).ok_or(DraftError::InvalidUrl);
        }
        let scheme = trim(&self.web_scheme);
        let plain = split_authority(host, true).is_some_and(|(_, port)| port.is_none());
        if !is_scheme(scheme) || !plain || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(DraftError::InvalidUrl);
        }
        let parts = WebParts {
            scheme: scheme.to_owned(),
            host: host.to_owned(),
            port: self.parsed_port()?,
            path: trim(&self.web_path).to_owned(),
        };
        web_url(&parts.scheme, &parts.host, parts.port, &parts.path)
            .map(|_| parts)
            .ok_or(DraftError::InvalidUrl)
    }

    /// The port to store: `None` when empty.
    fn parsed_port(&self) -> Result<Option<u16>, DraftError> {
        if self.port.is_empty() {
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
    /// Name, host, user, domain and key path are trimmed. The domain
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
        connection.port = if self.kind == ConnectionType::Web {
            None
        } else {
            self.parsed_port()?
        };
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
        if self.kind == ConnectionType::Web {
            let parts = self.web_parts()?;
            connection.host = parts.host;
            connection.port = parts.port;
            connection.web_scheme = parts.scheme;
            connection.web_path = parts.path;
        } else {
            connection.web_scheme = default_web_scheme();
            connection.web_path = String::new();
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

/// The parts of a web URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebParts {
    /// The scheme, as written (`https`, `smb`, ...).
    pub scheme: String,
    /// The host; an IPv6 address stays in brackets.
    pub host: String,
    /// The explicit port, if any.
    pub port: Option<u16>,
    /// The path with any `?query#fragment`; empty when there is none.
    pub path: String,
}

/// Splits a typed web URL (Swift `WebLaunch.url(from:)`, reworked): any `scheme://` is accepted, none means
/// `https`; a `user@` is dropped; the path keeps its `?query#fragment`. `None` for an empty value, one with
/// whitespace, a bad scheme, a missing host or a port that isn't a number from 1 to 65535.
#[must_use]
pub fn split_web_url(text: &str) -> Option<WebParts> {
    split_inner(text, false)
}

/// Like [`split_web_url`], but keeps a `userinfo@` as part of the host, so the rebuilt URL opens the same
/// page. Used when upgrading stored URLs.
#[must_use]
pub fn split_stored_web_url(text: &str) -> Option<WebParts> {
    split_inner(text, true)
}

fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Splits an authority into its host (with `userinfo@` when `keep_userinfo`) and the port text.
fn split_authority(authority: &str, keep_userinfo: bool) -> Option<(String, Option<&str>)> {
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if host_port.starts_with('[') {
        let close = host_port.find(']')?;
        let (host, after) = host_port.split_at(close + 1);
        let port = if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':')?)
        };
        (host, port)
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if host.is_empty() || host == "[]" {
        return None;
    }
    let prefix = if keep_userinfo {
        &authority[..authority.len() - host_port.len()]
    } else {
        ""
    };
    Some((format!("{prefix}{host}"), port))
}

fn split_inner(text: &str, keep_userinfo: bool) -> Option<WebParts> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = trimmed.split_once("://").unwrap_or(("https", trimmed));
    if !is_scheme(scheme) {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let (host, port) = split_authority(authority, keep_userinfo)?;
    let port = match port {
        None => None,
        Some(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            Some(p.parse::<u16>().ok().filter(|&p| p >= 1)?)
        }
        Some(_) => return None,
    };
    Some(WebParts {
        scheme: scheme.to_owned(),
        host,
        port,
        path: path.to_owned(),
    })
}

/// Builds the URL from its parts; the port is written as given, even when it is the scheme's default, and the
/// path is joined with a single `/`. `None` for an empty or whitespace scheme or host.
#[must_use]
pub fn web_url(scheme: &str, host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let (scheme, host) = (scheme.trim(), host.trim());
    if scheme.is_empty()
        || host.is_empty()
        || scheme.chars().any(char::is_whitespace)
        || host.chars().any(char::is_whitespace)
    {
        return None;
    }
    let mut url = format!("{scheme}://{host}");
    if let Some(port) = port {
        url.push(':');
        url.push_str(&port.to_string());
    }
    if path.starts_with(['?', '#']) {
        url.push_str(path);
    } else {
        let path = path.trim_start_matches('/');
        if !path.is_empty() {
            url.push('/');
            url.push_str(path);
        }
    }
    Some(url)
}

/// Whether `scheme` opens in a browser (`http` or `https`); other schemes go to the OS handler.
#[must_use]
pub fn is_http(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
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

    fn parts(scheme: &str, host: &str, port: Option<u16>, path: &str) -> WebParts {
        WebParts {
            scheme: scheme.into(),
            host: host.into(),
            port,
            path: path.into(),
        }
    }

    #[test]
    fn split_and_build_round_trip() {
        let cases = [
            (
                "https://192.168.1.10:8443/admin",
                parts("https", "192.168.1.10", Some(8443), "/admin"),
            ),
            (
                "smb://homeserver.local/share",
                parts("smb", "homeserver.local", None, "/share"),
            ),
            (
                "http://user@host/x?y=1#z",
                parts("http", "host", None, "/x?y=1#z"),
            ),
            ("https://[::1]:8443", parts("https", "[::1]", Some(8443), "")),
            (
                "https://user@[2001:db8::1]/x",
                parts("https", "[2001:db8::1]", None, "/x"),
            ),
            ("example.com", parts("https", "example.com", None, "")),
            (
                "  router.example.com/admin ",
                parts("https", "router.example.com", None, "/admin"),
            ),
        ];
        for (text, expected) in cases {
            let got = split_web_url(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(got, expected, "{text}");
            let url = web_url(&got.scheme, &got.host, got.port, &got.path).unwrap();
            assert_eq!(split_web_url(&url), Some(got), "{url}");
        }
        assert_eq!(
            web_url("https", "example.com", Some(443), "").as_deref(),
            Some("https://example.com:443")
        );
        assert_eq!(web_url("http", "h", None, "a/b").as_deref(), Some("http://h/a/b"));
        assert_eq!(web_url("http", "h", None, "//a").as_deref(), Some("http://h/a"));
        assert_eq!(
            web_url("http", "h", None, "?q=1").as_deref(),
            Some("http://h?q=1")
        );
    }

    #[test]
    fn split_and_build_reject_bad_input() {
        for bad in [
            "",
            "not a url",
            "https://",
            "https://host:port",
            "https://host:",
            "https://host:99999",
            "1http://example.com",
            "https:///path",
            "exa\tmple.com",
            "https://[::1",
            "https://[]",
        ] {
            assert_eq!(split_web_url(bad), None, "{bad:?}");
        }
        assert_eq!(web_url("", "h", None, ""), None);
        assert_eq!(web_url("https", "  ", None, ""), None);
        assert_eq!(web_url(" ", "h", None, ""), None);
        assert_eq!(web_url("https", "a b", None, ""), None);
    }

    #[test]
    fn split_rejects_port_zero_and_can_keep_userinfo() {
        assert_eq!(split_web_url("https://h:0"), None);
        assert_eq!(
            split_stored_web_url("https://user:pw@h:81/x"),
            Some(parts("https", "user:pw@h", Some(81), "/x"))
        );
        assert_eq!(
            split_stored_web_url("u@[::1]"),
            Some(parts("https", "u@[::1]", None, ""))
        );
    }

    #[test]
    fn web_draft_host_is_split_or_checked() {
        let mut d = draft(ConnectionType::Web);
        d.host = " smb://user@nas.local:445/share?x#y ".into();
        d.web_scheme = "https".into();
        d.port = "junk".into();
        assert_eq!(d.validate(), Ok(()));
        let mut c = Connection::new(ConnectionType::Web, "w", "w");
        d.apply_to(&mut c).unwrap();
        assert_eq!(
            (
                c.host.as_str(),
                c.port,
                c.web_scheme.as_str(),
                c.web_path.as_str()
            ),
            ("nas.local", Some(445), "smb", "/share?x#y")
        );

        for bad in ["host:8080", "[::1", "a b", "https://", "http://:80"] {
            d.host = bad.into();
            assert_eq!(d.validate(), Err(DraftError::InvalidUrl), "{bad}");
        }
        d.host = "a/b".into();
        d.port = String::new();
        d.apply_to(&mut c).unwrap();
        assert_eq!((c.host.as_str(), c.web_path.as_str()), ("a", "/b"));
        d.host = "u:p@h".into();
        assert_eq!(d.validate(), Ok(()));
        d.host = "[::1]".into();
        assert_eq!(d.validate(), Ok(()));
        d.host = "h".into();
        for bad in ["1ht!", "https://", "", "a b"] {
            d.web_scheme = bad.into();
            assert_eq!(d.validate(), Err(DraftError::InvalidUrl), "{bad}");
        }
    }

    #[test]
    fn http_schemes() {
        assert!(is_http("http") && is_http("HTTPS"));
        assert!(!is_http("smb") && !is_http(""));
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
    fn web_validates_the_url_parts() {
        let mut d = draft(ConnectionType::Web);
        assert_eq!(d.validate(), Ok(()));
        d.port = "junk".into();
        assert_eq!(d.validate(), Err(DraftError::InvalidPort));
        d.port = "8443".into();
        assert_eq!(d.validate(), Ok(()));
        d.host = "not a url".into();
        assert_eq!(d.validate(), Err(DraftError::InvalidUrl));
        d.host = "  ".into();
        assert_eq!(d.validate(), Err(DraftError::InvalidUrl));
        d.host = "h".into();
        d.web_scheme = " ".into();
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
    fn apply_web_keeps_browser_port_scheme_and_path() {
        let mut d = draft(ConnectionType::Web);
        d.port = "8080".into();
        d.web_scheme = " smb ".into();
        d.web_path = " /share ".into();
        d.browser_bundle_id = Some("com.example.browser".into());
        let mut c = Connection::new(ConnectionType::Web, "w", "w");
        c.port = Some(1);
        d.apply_to(&mut c).unwrap();
        assert_eq!(c.port, Some(8080));
        assert_eq!((c.web_scheme.as_str(), c.web_path.as_str()), ("smb", "/share"));
        assert_eq!(c.browser_bundle_id.as_deref(), Some("com.example.browser"));
        let round = ConnectionDraft::from_connection(&c);
        assert_eq!(
            (round.web_scheme.as_str(), round.web_path.as_str()),
            ("smb", "/share")
        );

        let mut ssh = draft(ConnectionType::Ssh);
        ssh.web_scheme = "smb".into();
        ssh.apply_to(&mut c).unwrap();
        assert_eq!((c.web_scheme.as_str(), c.web_path.as_str()), ("https", ""));
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
