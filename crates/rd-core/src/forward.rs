//! Port forward rules (Swift `PortForward.validationError(among:)`, `displayName`, `destination`) and the
//! editor's draft.

use serde::{Deserialize, Serialize};

use crate::model::{PortForward, Timestamp, new_id};

/// Why a forward can't be saved. [`code`](Self::code) is a stable id the UI translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PortForwardError {
    /// The local port is not 1–65535.
    #[error("The local port must be between 1 and 65535.")]
    InvalidLocalPort,
    /// The remote host is empty after trimming.
    #[error("Enter the remote host.")]
    MissingRemoteHost,
    /// The remote port is not 1–65535.
    #[error("The remote port must be between 1 and 65535.")]
    InvalidRemotePort,
    /// Another forward of the same connection uses the local port.
    #[error("Another forward of this connection uses this local port.")]
    DuplicateLocalPort,
}

impl PortForwardError {
    /// `invalidLocalPort`, `missingRemoteHost`, `invalidRemotePort` or `duplicateLocalPort`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidLocalPort => "invalidLocalPort",
            Self::MissingRemoteHost => "missingRemoteHost",
            Self::InvalidRemotePort => "invalidRemotePort",
            Self::DuplicateLocalPort => "duplicateLocalPort",
        }
    }
}

/// Swift `.whitespaces` trim (spaces and tabs).
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

fn valid_port(port: i64) -> Option<u16> {
    u16::try_from(port).ok().filter(|p| *p >= 1)
}

/// The fields Swift validates, before they are narrowed to `u16`.
fn validate(
    id: &str,
    connection_id: &str,
    local_port: i64,
    remote_host: &str,
    remote_port: i64,
    others: &[PortForward],
) -> Result<(u16, u16), PortForwardError> {
    let local = valid_port(local_port).ok_or(PortForwardError::InvalidLocalPort)?;
    if trim(remote_host).is_empty() {
        return Err(PortForwardError::MissingRemoteHost);
    }
    let remote = valid_port(remote_port).ok_or(PortForwardError::InvalidRemotePort)?;
    let clash = others
        .iter()
        .any(|o| o.id != id && o.connection_id == connection_id && o.local_port == local);
    if clash {
        return Err(PortForwardError::DuplicateLocalPort);
    }
    Ok((local, remote))
}

impl PortForward {
    /// Why this forward can't be saved next to `others` (the saved forwards), or `None`.
    #[must_use]
    pub fn validation_error(&self, others: &[PortForward]) -> Option<PortForwardError> {
        validate(
            &self.id,
            &self.connection_id,
            i64::from(self.local_port),
            &self.remote_host,
            i64::from(self.remote_port),
            others,
        )
        .err()
    }

    /// `host:port`, IPv6 hosts bracketed.
    #[must_use]
    pub fn destination(&self) -> String {
        let host = trim(&self.remote_host);
        if host.contains(':') {
            format!("[{host}]:{}", self.remote_port)
        } else {
            format!("{host}:{}", self.remote_port)
        }
    }

    /// The name, or the destination when unnamed.
    #[must_use]
    pub fn display_name(&self) -> String {
        let name = trim(&self.name);
        if name.is_empty() {
            self.destination()
        } else {
            name.to_owned()
        }
    }
}

/// A forward as the editor sends it. Ports arrive as typed; anything that isn't a whole number counts
/// as 0, as Swift's `Int(text) ?? 0`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForwardDraft {
    /// The forward being edited; `None` for a new one.
    #[serde(default)]
    pub id: Option<String>,
    /// The SSH connection it belongs to.
    pub connection_id: String,
    /// Optional label.
    #[serde(default)]
    pub name: String,
    /// The local port as typed.
    #[serde(default)]
    pub local_port: String,
    /// The destination host.
    #[serde(default)]
    pub remote_host: String,
    /// The destination port as typed.
    #[serde(default)]
    pub remote_port: String,
}

fn parse_port(text: &str) -> i64 {
    trim(text).parse().unwrap_or(0)
}

impl PortForwardDraft {
    /// The forward to save: trimmed values, the id and creation time kept from `existing` (the saved
    /// forward with the draft's id), validated against `others`.
    ///
    /// # Errors
    /// The first [`PortForwardError`].
    pub fn to_forward(
        &self,
        existing: Option<&PortForward>,
        others: &[PortForward],
    ) -> Result<PortForward, PortForwardError> {
        let id = existing.map_or_else(|| self.id.clone().unwrap_or_else(new_id), |e| e.id.clone());
        let remote_host = trim(&self.remote_host).to_owned();
        let (local_port, remote_port) = validate(
            &id,
            &self.connection_id,
            parse_port(&self.local_port),
            &remote_host,
            parse_port(&self.remote_port),
            others,
        )?;
        Ok(PortForward {
            id,
            connection_id: self.connection_id.clone(),
            name: trim(&self.name).to_owned(),
            local_port,
            remote_host,
            remote_port,
            created_at: existing.map_or_else(Timestamp::now, |e| e.created_at),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward(local: u16, host: &str, remote: u16, connection: &str, id: &str) -> PortForward {
        PortForward {
            id: id.to_owned(),
            connection_id: connection.to_owned(),
            name: String::new(),
            local_port: local,
            remote_host: host.to_owned(),
            remote_port: remote,
            created_at: Timestamp::now(),
        }
    }

    fn default_forward() -> PortForward {
        forward(13389, "192.0.2.20", 3389, "c1", "x")
    }

    fn draft(local: &str, host: &str, remote: &str) -> PortForwardDraft {
        PortForwardDraft {
            id: None,
            connection_id: "c1".to_owned(),
            name: String::new(),
            local_port: local.to_owned(),
            remote_host: host.to_owned(),
            remote_port: remote.to_owned(),
        }
    }

    #[test]
    fn valid_forward_has_no_error() {
        assert_eq!(default_forward().validation_error(&[]), None);
        assert_eq!(forward(1, "h", 65535, "c1", "x").validation_error(&[]), None);
    }

    #[test]
    fn rejects_invalid_values() {
        // Swift's table, with the out-of-range ports going through the draft (u16 can't hold them).
        let cases = [
            ("0", "h", "22", PortForwardError::InvalidLocalPort),
            ("65536", "h", "22", PortForwardError::InvalidLocalPort),
            ("-1", "h", "22", PortForwardError::InvalidLocalPort),
            ("8080", "  ", "22", PortForwardError::MissingRemoteHost),
            ("8080", "h", "0", PortForwardError::InvalidRemotePort),
            ("8080", "h", "70000", PortForwardError::InvalidRemotePort),
            ("abc", "h", "22", PortForwardError::InvalidLocalPort),
            ("", "h", "22", PortForwardError::InvalidLocalPort),
        ];
        for (local, host, remote, error) in cases {
            assert_eq!(
                draft(local, host, remote).to_forward(None, &[]),
                Err(error),
                "{local} {host:?} {remote}"
            );
            assert!(!error.to_string().is_empty());
            assert!(!error.code().is_empty());
        }
        assert_eq!(
            forward(0, "h", 22, "c1", "x").validation_error(&[]),
            Some(PortForwardError::InvalidLocalPort)
        );
        assert_eq!(
            forward(1, " ", 22, "c1", "x").validation_error(&[]),
            Some(PortForwardError::MissingRemoteHost)
        );
        assert_eq!(
            forward(1, "h", 0, "c1", "x").validation_error(&[]),
            Some(PortForwardError::InvalidRemotePort)
        );
    }

    #[test]
    fn local_port_is_unique_per_connection() {
        let existing = forward(8080, "h", 22, "c1", "a");
        assert_eq!(
            forward(8080, "h", 22, "c1", "b").validation_error(std::slice::from_ref(&existing)),
            Some(PortForwardError::DuplicateLocalPort)
        );
        assert_eq!(PortForwardError::DuplicateLocalPort.code(), "duplicateLocalPort");
        // The forward itself (editing) and other connections don't clash.
        assert_eq!(
            forward(8080, "h", 22, "c1", "a").validation_error(std::slice::from_ref(&existing)),
            None
        );
        assert_eq!(
            forward(8080, "h", 22, "c2", "b").validation_error(std::slice::from_ref(&existing)),
            None
        );
    }

    #[test]
    fn display_name_and_destination() {
        let mut named = forward(13389, "fs.local", 445, "c1", "x");
        assert_eq!(named.display_name(), "fs.local:445");
        named.name = "  File server ".to_owned();
        assert_eq!(named.display_name(), "File server");
        assert_eq!(forward(1, "fe80::1", 22, "c1", "x").destination(), "[fe80::1]:22");
    }

    #[test]
    fn draft_trims_and_keeps_identity_when_editing() {
        let mut new = draft(" 13389 ", " 192.0.2.20 ", "3389");
        new.name = " RDP ".to_owned();
        let saved = new.to_forward(None, &[]).unwrap();
        assert_eq!(saved.local_port, 13389);
        assert_eq!(saved.remote_host, "192.0.2.20");
        assert_eq!(saved.name, "RDP");
        assert_eq!(saved.id.len(), 36);

        let mut edit = draft("23389", "192.0.2.21", "3390");
        edit.id = Some(saved.id.clone());
        let edited = edit
            .to_forward(Some(&saved), std::slice::from_ref(&saved))
            .unwrap();
        assert_eq!(edited.id, saved.id);
        assert_eq!(edited.created_at, saved.created_at);
        assert_eq!(edited.local_port, 23389);
    }

    #[test]
    fn draft_deserializes_from_the_editor() {
        let draft: PortForwardDraft = serde_json::from_str(
            r#"{"connectionId":"c1","localPort":"8080","remoteHost":"h","remotePort":"80"}"#,
        )
        .unwrap();
        assert_eq!(draft.id, None);
        assert_eq!(draft.name, "");
        assert_eq!(draft.to_forward(None, &[]).unwrap().remote_port, 80);
    }
}
