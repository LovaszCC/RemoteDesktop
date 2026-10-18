//! Importers for other connection managers: WinBox address books (`.cdb`) and Devolutions Remote Desktop
//! Manager XML exports (`.rdm`), plus the duplicate check of the import preview.
//!
//! A port of the Swift `WinboxImporter`, `RDMImporter` and `ImportedConnection`. Parsers read byte slices
//! only; the store's import writer seals the passwords and creates the folders.

mod duplicates;
mod rdm;
mod winbox;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::ConnectionType;

pub use duplicates::{DuplicateFlags, credential_is_duplicate, folder_for_path, is_duplicate};
pub use rdm::{parse as parse_rdm, split_group, split_host_port};
pub use winbox::{parse as parse_winbox, split_address};

/// Which importer reads a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportKind {
    /// Devolutions Remote Desktop Manager XML export (`.rdm`, `.xml`).
    Rdm,
    /// WinBox address book (`.cdb`).
    Winbox,
}

impl ImportKind {
    /// File extensions the open panel offers.
    #[must_use]
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Rdm => &["rdm", "xml"],
            Self::Winbox => &["cdb"],
        }
    }
}

/// Why a file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// Not a WinBox address book, or a damaged one.
    #[error("this isn't a WinBox address book (.cdb) or it is damaged")]
    WinboxUnreadable,
    /// Not well-formed XML.
    #[error("this isn't a Devolutions RDM export or it is damaged: {0}")]
    RdmUnreadable(String),
}

/// One connection parsed from an export, before sealing and folder resolution.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportedConnection {
    /// Kind of connection.
    pub kind: ConnectionType,
    /// Display name.
    pub name: String,
    /// Folder names from the import root down; empty means the import root itself.
    pub group_path: Vec<String>,
    /// Host name or address.
    pub host: String,
    /// Port; `None` means the type's default.
    pub port: Option<u16>,
    /// Login user.
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// The plain-text password from the export. Never leaves Rust; sealed by the writer.
    pub password: Option<String>,
    /// Path to an SSH private key.
    pub ssh_key_path: Option<String>,
    /// Free-form notes.
    pub notes: String,
    /// RDP only: connect to the administrative session.
    pub rdp_admin_session: bool,
    /// RDM `CredentialConnectionID`: the export id of a linked credential entry.
    pub credential_ref: Option<String>,
}

impl ImportedConnection {
    /// An entry with the given kind, name and host and everything else empty.
    #[must_use]
    pub fn new(kind: ConnectionType, name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            group_path: Vec::new(),
            host: host.into(),
            port: None,
            username: String::new(),
            domain: String::new(),
            password: None,
            ssh_key_path: None,
            notes: String::new(),
            rdp_admin_session: false,
            credential_ref: None,
        }
    }
}

impl fmt::Debug for ImportedConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedConnection")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("group_path", &self.group_path)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("domain", &self.domain)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("ssh_key_path", &self.ssh_key_path)
            .field("notes", &self.notes)
            .field("rdp_admin_session", &self.rdp_admin_session)
            .field("credential_ref", &self.credential_ref)
            .finish()
    }
}

/// One RDM credential entry.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportedCredential {
    /// The export id that connections refer to.
    pub reference: String,
    /// Display name.
    pub name: String,
    /// Login user.
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// The plain-text password from the export. Never leaves Rust; sealed by the writer.
    pub password: Option<String>,
    /// Path to an SSH private key.
    pub ssh_key_path: Option<String>,
}

impl fmt::Debug for ImportedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedCredential")
            .field("reference", &self.reference)
            .field("name", &self.name)
            .field("username", &self.username)
            .field("domain", &self.domain)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("ssh_key_path", &self.ssh_key_path)
            .finish()
    }
}

/// Why an entry was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum SkipReason {
    /// A connection type the app doesn't have (web pages, VNC, …); carries the RDM type.
    Unsupported {
        /// The RDM `ConnectionType`.
        kind: String,
    },
    /// No host in any of the host fields.
    NoHost,
}

/// An entry the parser left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// The entry's name (may be empty).
    pub name: String,
    /// Why.
    #[serde(flatten)]
    pub reason: SkipReason,
}

impl fmt::Display for Skipped {
    /// The Swift text: `name (Type)` / `Type`, or `name (no host)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            SkipReason::Unsupported { kind } if self.name.is_empty() => f.write_str(kind),
            SkipReason::Unsupported { kind } => write!(f, "{} ({kind})", self.name),
            SkipReason::NoHost => write!(f, "{} (no host)", self.name),
        }
    }
}

/// Everything parsed from one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedImport {
    /// Connections in file order.
    pub connections: Vec<ImportedConnection>,
    /// RDM credential entries in file order.
    pub credentials: Vec<ImportedCredential>,
    /// RDM folder entries, created even when empty.
    pub empty_groups: Vec<Vec<String>>,
    /// Entries left out.
    pub skipped: Vec<Skipped>,
    /// Entries whose password was present only in encrypted (`SafePassword`) form.
    pub encrypted_passwords: usize,
}

/// Parses a file with the importer of `kind`.
///
/// # Errors
/// [`ImportError`] when the file isn't that kind of export or is damaged.
pub fn parse(kind: ImportKind, data: &[u8]) -> Result<ParsedImport, ImportError> {
    match kind {
        ImportKind::Rdm => parse_rdm(data),
        ImportKind::Winbox => Ok(ParsedImport {
            connections: parse_winbox(data)?,
            ..ParsedImport::default()
        }),
    }
}

/// Swift `.whitespaces`: horizontal white space only.
pub(crate) fn trim_spaces(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() && !is_newline(c))
}

/// Swift `.whitespacesAndNewlines`.
pub(crate) fn trim_all(text: &str) -> &str {
    text.trim()
}

fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_passwords() {
        let mut item = ImportedConnection::new(ConnectionType::Ssh, "n", "h");
        item.password = Some("hunter2".into());
        assert!(!format!("{item:?}").contains("hunter2"));
        let credential = ImportedCredential {
            reference: "R".into(),
            name: "n".into(),
            username: String::new(),
            domain: String::new(),
            password: Some("hunter2".into()),
            ssh_key_path: None,
        };
        assert!(!format!("{credential:?}").contains("hunter2"));
    }

    #[test]
    fn skipped_text_and_json() {
        let unsupported = Skipped {
            name: "portal".into(),
            reason: SkipReason::Unsupported {
                kind: "WebBrowser".into(),
            },
        };
        assert_eq!(unsupported.to_string(), "portal (WebBrowser)");
        assert_eq!(
            serde_json::to_value(&unsupported).unwrap(),
            serde_json::json!({"name": "portal", "reason": "unsupported", "kind": "WebBrowser"})
        );
        let unnamed = Skipped {
            name: String::new(),
            reason: SkipReason::Unsupported { kind: "VNC".into() },
        };
        assert_eq!(unnamed.to_string(), "VNC");
        let no_host = Skipped {
            name: "x".into(),
            reason: SkipReason::NoHost,
        };
        assert_eq!(no_host.to_string(), "x (no host)");
        assert_eq!(
            serde_json::to_value(&no_host).unwrap(),
            serde_json::json!({"name": "x", "reason": "noHost"})
        );
    }

    #[test]
    fn kinds() {
        assert_eq!(ImportKind::Winbox.extensions(), ["cdb"]);
        assert_eq!(ImportKind::Rdm.extensions(), ["rdm", "xml"]);
        assert_eq!(serde_json::to_string(&ImportKind::Winbox).unwrap(), "\"winbox\"");
        assert!(matches!(
            parse(ImportKind::Winbox, b"nope"),
            Err(ImportError::WinboxUnreadable)
        ));
    }

    #[test]
    fn trims_like_swift() {
        assert_eq!(trim_spaces(" \ta b\t "), "a b");
        assert_eq!(trim_spaces("\n a \n"), "\n a \n");
        assert_eq!(trim_all("\n a \r\n"), "a");
    }
}
