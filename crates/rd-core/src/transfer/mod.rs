//! The app's own export format: picked entries and credentials for another user of the app, built from the
//! library and read back by the import. SSH key paths, passphrases and unlocked keys never go into it;
//! passwords only when the caller supplies them (an encrypted export).
//!
//! Encryption, files and writing to the database live in the store and the app; this module is pure.

mod build;
mod conflicts;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{ConnectionType, LabelColor, MikrotikMode, RdpClient, SshTerminal};

pub use build::{ExportSelection, OpenSecret, build_payload};
pub use conflicts::{TransferConflicts, conflicts};

/// A password inside an export. Serialized as the plain string; never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExportSecret(pub String);

impl fmt::Debug for ExportSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Everything an export carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPayload {
    /// When the file was written (GRDB text form, UTC).
    pub exported_at: String,
    /// The version of the app that wrote it.
    pub app_version: String,
    /// Exported credentials.
    #[serde(default)]
    pub credentials: Vec<ExportedCredential>,
    /// Exported entries.
    #[serde(default)]
    pub entries: Vec<ExportedEntry>,
}

/// A credential in an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedCredential {
    /// The id connections refer to inside this file.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Display name.
    pub name: String,
    /// Login user.
    #[serde(default)]
    pub username: String,
    /// Windows domain.
    #[serde(default)]
    pub domain: String,
    /// Free-form notes.
    #[serde(default)]
    pub notes: String,
    /// The password; only in encrypted exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<ExportSecret>,
}

/// An entry with its connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEntry {
    /// The id rows refer to inside this file.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Display name.
    pub name: String,
    /// Host name or address.
    #[serde(default)]
    pub host: String,
    /// Free-form notes.
    #[serde(default)]
    pub notes: String,
    /// Tag color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
    /// Shown under Favorites.
    #[serde(default)]
    pub is_favorite: bool,
    /// Folder names from the root down; empty is the root.
    #[serde(default)]
    pub folder_path: Vec<String>,
    /// Index into `connections` of the one double-click opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_connection: Option<usize>,
    /// The entry's connections in their order.
    #[serde(default)]
    pub connections: Vec<ExportedConnection>,
}

/// One connection of an exported entry.
#[allow(clippy::struct_excessive_bools, reason = "mirrors the connection's flags")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedConnection {
    /// Kind of connection.
    pub kind: ConnectionType,
    /// Optional label among the entry's connections.
    #[serde(default)]
    pub label: String,
    /// Port; `None` means the type's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Own login user.
    #[serde(default)]
    pub username: String,
    /// Own Windows domain.
    #[serde(default)]
    pub domain: String,
    /// Own password; only in encrypted exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<ExportSecret>,
    /// [`ExportedCredential::reference`] of the linked credential, when that credential is exported too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    /// RDP only: connect to the administrative session.
    #[serde(default)]
    pub rdp_admin_session: bool,
    /// Web only: browser id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_bundle_id: Option<String>,
    /// MikroTik only: what double-click opens.
    pub mikrotik_mode: MikrotikMode,
    /// Embedded tab or external terminal.
    pub ssh_terminal: SshTerminal,
    /// Embedded tab or the system client.
    pub rdp_client: RdpClient,
    /// Gets its own button on the entry's Details page.
    #[serde(default)]
    pub show_as_button: bool,
    /// Web only: URL scheme.
    #[serde(default)]
    pub web_scheme: String,
    /// Web only: path after host and port.
    #[serde(default)]
    pub web_path: String,
    /// SSH port forwards.
    #[serde(default)]
    pub port_forwards: Vec<ExportedForward>,
}

/// An SSH local port forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedForward {
    /// Optional label.
    #[serde(default)]
    pub name: String,
    /// Port on 127.0.0.1.
    pub local_port: u16,
    /// Destination host as seen from the SSH server.
    pub remote_host: String,
    /// Destination port.
    pub remote_port: u16,
}

impl ExportPayload {
    /// Whether any password is in the payload.
    #[must_use]
    pub fn has_passwords(&self) -> bool {
        self.credentials.iter().any(|c| c.password.is_some())
            || self
                .entries
                .iter()
                .flat_map(|e| &e.connections)
                .any(|c| c.password.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_in_debug_but_serialized() {
        let credential = ExportedCredential {
            reference: "k1".into(),
            name: "Domain admin".into(),
            username: "admin".into(),
            domain: String::new(),
            notes: String::new(),
            password: Some(ExportSecret("hunter2".into())),
        };
        assert!(!format!("{credential:?}").contains("hunter2"));
        let json = serde_json::to_value(&credential).unwrap();
        assert_eq!(json["password"], "hunter2");
        assert_eq!(json["ref"], "k1");
    }

    #[test]
    fn absent_password_is_not_written() {
        let credential = ExportedCredential {
            reference: "k1".into(),
            name: "n".into(),
            username: String::new(),
            domain: String::new(),
            notes: String::new(),
            password: None,
        };
        let json = serde_json::to_string(&credential).unwrap();
        assert!(!json.contains("password"));
    }
}
