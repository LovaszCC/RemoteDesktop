//! Library → export payload.

use std::collections::HashSet;

use super::{
    ExportPayload, ExportSecret, ExportedConnection, ExportedCredential, ExportedEntry, ExportedForward,
    ExportedVpn,
};
use crate::library::Library;
use crate::model::{Sealed, Timestamp};

/// What to export, by record id. Unknown ids are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportSelection {
    /// Entry ids.
    pub entries: Vec<String>,
    /// Credential ids.
    pub credentials: Vec<String>,
}

/// Turns a sealed password into plain text, for an encrypted export.
pub type OpenSecret<'a, E> = &'a mut dyn FnMut(&Sealed) -> Result<String, E>;

/// Folder names from the root down to `folder_id`.
fn folder_path(library: &Library, folder_id: Option<&str>) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = folder_id;
    // Guards against a damaged parent chain.
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            break;
        }
        let Some(folder) = library.folders.iter().find(|f| f.id == id) else {
            break;
        };
        path.push(folder.name.clone());
        current = folder.parent_id.as_deref();
    }
    path.reverse();
    path
}

/// Builds the payload of the selected records, in library order. A connection keeps its credential link
/// only when that credential is selected too. `open` turns sealed passwords into plain text; `None` leaves
/// every password out. SSH key fields are never exported.
///
/// # Errors
/// The first error of `open`.
pub fn build_payload<E>(
    library: &Library,
    selection: &ExportSelection,
    app_version: &str,
    mut open: Option<OpenSecret<'_, E>>,
) -> Result<ExportPayload, E> {
    let mut password = |sealed: Option<&Sealed>| -> Result<Option<ExportSecret>, E> {
        match (sealed, open.as_mut()) {
            (Some(sealed), Some(open)) => {
                let plain = open(sealed)?;
                Ok((!plain.is_empty()).then_some(ExportSecret(plain)))
            }
            _ => Ok(None),
        }
    };

    let credential_ids: HashSet<&str> = selection.credentials.iter().map(String::as_str).collect();
    let mut credentials = Vec::new();
    for (index, credential) in library
        .credentials
        .iter()
        .filter(|c| credential_ids.contains(c.id.as_str()))
        .enumerate()
    {
        credentials.push((
            credential.id.as_str(),
            ExportedCredential {
                reference: format!("k{}", index + 1),
                name: credential.name.clone(),
                username: credential.username.clone(),
                domain: credential.domain.clone(),
                notes: credential.notes.clone(),
                password: password(credential.password_enc.as_ref())?,
            },
        ));
    }

    let entry_ids: HashSet<&str> = selection.entries.iter().map(String::as_str).collect();
    let mut entries = Vec::new();
    for (index, entry) in library
        .entries
        .iter()
        .filter(|e| entry_ids.contains(e.id.as_str()))
        .enumerate()
    {
        let default = library.default_connection(&entry.id).map(|c| c.id.clone());
        let mut connections = Vec::new();
        let mut default_connection = None;
        for (position, connection) in library.connections_of(&entry.id).into_iter().enumerate() {
            if default.as_deref() == Some(connection.id.as_str()) {
                default_connection = Some(position);
            }
            let credential_ref = connection.credential_id.as_deref().and_then(|id| {
                credentials
                    .iter()
                    .find(|(cid, _)| *cid == id)
                    .map(|(_, c)| c.reference.clone())
            });
            connections.push(ExportedConnection {
                kind: connection.kind,
                label: connection.label.clone(),
                port: connection.port,
                username: connection.username.clone(),
                domain: connection.domain.clone(),
                password: password(connection.password_enc.as_ref())?,
                credential_ref,
                rdp_admin_session: connection.rdp_admin_session,
                browser_bundle_id: connection.browser_bundle_id.clone(),
                mikrotik_mode: connection.mikrotik_mode,
                ssh_terminal: connection.ssh_terminal,
                rdp_client: connection.rdp_client,
                show_as_button: connection.show_as_button,
                web_scheme: connection.web_scheme.clone(),
                web_path: connection.web_path.clone(),
                port_forwards: library
                    .port_forwards
                    .iter()
                    .filter(|f| f.connection_id == connection.id)
                    .map(|f| ExportedForward {
                        name: f.name.clone(),
                        local_port: f.local_port,
                        remote_host: f.remote_host.clone(),
                        remote_port: f.remote_port,
                    })
                    .collect(),
            });
        }
        entries.push(ExportedEntry {
            reference: format!("e{}", index + 1),
            name: entry.name.clone(),
            host: entry.host.clone(),
            notes: entry.notes.clone(),
            color: entry.color,
            is_favorite: entry.is_favorite,
            folder_path: folder_path(library, entry.folder_id.as_deref()),
            default_connection,
            connections,
            vpn: entry.vpn.as_ref().map(exported_vpn),
        });
    }

    Ok(ExportPayload {
        exported_at: Timestamp::now().to_string(),
        app_version: app_version.to_owned(),
        credentials: credentials.into_iter().map(|(_, c)| c).collect(),
        entries,
    })
}

fn exported_vpn(vpn: &crate::model::EntryVpn) -> ExportedVpn {
    ExportedVpn {
        connection_id: vpn.connection_id.clone(),
        test_mode: vpn.test_mode,
        test_address: vpn.test_address.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;
    use crate::model::{Connection, ConnectionType, Credential, Entry, Folder, PortForward};

    fn sealed(text: &str) -> Sealed {
        Sealed::from_bytes(text.as_bytes().to_vec())
    }

    fn library() -> Library {
        let mut parent = Folder::new("Clients");
        parent.id = "F1".into();
        let mut child = Folder::new("Acme");
        child.id = "F2".into();
        child.parent_id = Some("F1".into());

        let mut credential = Credential::new("Domain admin");
        credential.id = "K1".into();
        credential.username = "admin".into();
        credential.password_enc = Some(sealed("cred-pw"));
        credential.ssh_key_path = Some("~/.ssh/id_ed25519".into());
        credential.ssh_key_passphrase_enc = Some(sealed("phrase"));
        let mut other = Credential::new("Other");
        other.id = "K2".into();

        let mut entry = Entry::new("dc01", "dc01.acme.local");
        entry.id = "E1".into();
        entry.folder_id = Some("F2".into());
        let mut rdp = Connection::new(ConnectionType::Rdp, "dc01", "dc01.acme.local");
        rdp.id = "C1".into();
        rdp.entry_id = "E1".into();
        rdp.folder_id = Some("F2".into());
        rdp.credential_id = Some("K1".into());
        let mut ssh = Connection::new(ConnectionType::Ssh, "dc01", "dc01.acme.local");
        ssh.id = "C2".into();
        ssh.entry_id = "E1".into();
        ssh.folder_id = Some("F2".into());
        ssh.sort_order = 1;
        ssh.username = "root".into();
        ssh.password_enc = Some(sealed("own-pw"));
        ssh.ssh_key_path = Some("~/.ssh/id_rsa".into());
        ssh.credential_id = Some("K2".into());
        entry.default_connection_id = Some("C2".into());

        let mut lone = Entry::new("web", "intranet");
        lone.id = "E2".into();
        let mut web = Connection::new(ConnectionType::Web, "web", "intranet");
        web.id = "C3".into();
        web.entry_id = "E2".into();

        Library {
            folders: vec![parent, child],
            entries: vec![entry, lone],
            connections: vec![rdp, ssh, web],
            credentials: vec![credential, other],
            port_forwards: vec![PortForward {
                id: "P1".into(),
                connection_id: "C2".into(),
                name: "db".into(),
                local_port: 15432,
                remote_host: "127.0.0.1".into(),
                remote_port: 5432,
                created_at: Timestamp::now(),
            }],
            saved_splits: Vec::new(),
        }
    }

    fn selection() -> ExportSelection {
        ExportSelection {
            entries: vec!["E1".into()],
            credentials: vec!["K1".into()],
        }
    }

    #[test]
    fn exports_the_selection_with_folder_path_and_forwards() {
        let payload = build_payload::<Infallible>(&library(), &selection(), "1.2.3", None).unwrap();
        assert_eq!(payload.app_version, "1.2.3");
        assert_eq!(payload.entries.len(), 1);
        let entry = &payload.entries[0];
        assert_eq!(entry.name, "dc01");
        assert_eq!(entry.folder_path, ["Clients", "Acme"]);
        assert_eq!(entry.default_connection, Some(1));
        assert_eq!(entry.connections.len(), 2);
        assert_eq!(entry.connections[0].kind, ConnectionType::Rdp);
        assert_eq!(entry.connections[1].username, "root");
        assert_eq!(entry.connections[1].port_forwards[0].local_port, 15432);
        assert_eq!(payload.credentials.len(), 1);
        assert_eq!(payload.credentials[0].name, "Domain admin");
    }

    #[test]
    fn keeps_links_only_to_exported_credentials() {
        let payload = build_payload::<Infallible>(&library(), &selection(), "1", None).unwrap();
        let reference = payload.credentials[0].reference.clone();
        let connections = &payload.entries[0].connections;
        assert_eq!(connections[0].credential_ref.as_deref(), Some(reference.as_str()));
        assert_eq!(connections[1].credential_ref, None);

        let without = ExportSelection {
            entries: vec!["E1".into()],
            credentials: Vec::new(),
        };
        let payload = build_payload::<Infallible>(&library(), &without, "1", None).unwrap();
        assert!(payload.credentials.is_empty());
        assert!(
            payload.entries[0]
                .connections
                .iter()
                .all(|c| c.credential_ref.is_none())
        );
    }

    #[test]
    fn no_passwords_without_open() {
        let payload = build_payload::<Infallible>(&library(), &selection(), "1", None).unwrap();
        assert!(!payload.has_passwords());
        let json = serde_json::to_string(&payload).unwrap();
        assert!(!json.contains("password"));
    }

    #[test]
    fn passwords_with_open_and_never_ssh_keys() {
        let mut open = |s: &Sealed| -> Result<String, Infallible> {
            Ok(String::from_utf8(s.as_bytes().to_vec()).unwrap())
        };
        let payload = build_payload(&library(), &selection(), "1", Some(&mut open)).unwrap();
        assert!(payload.has_passwords());
        assert_eq!(payload.credentials[0].password.as_ref().unwrap().0, "cred-pw");
        assert_eq!(
            payload.entries[0].connections[1].password.as_ref().unwrap().0,
            "own-pw"
        );
        let json = serde_json::to_string(&payload).unwrap();
        assert!(!json.contains("sshKey"), "{json}");
        assert!(!json.contains("id_ed25519"));
        assert!(!json.contains("id_rsa"));
        assert!(!json.contains("phrase"));
    }

    #[test]
    fn open_errors_are_returned() {
        let mut open = |_: &Sealed| -> Result<String, &'static str> { Err("broken") };
        let result = build_payload(&library(), &selection(), "1", Some(&mut open));
        assert_eq!(result.unwrap_err(), "broken");
    }

    #[test]
    fn root_entries_have_an_empty_path_and_unknown_ids_are_ignored() {
        let pick = ExportSelection {
            entries: vec!["E2".into(), "nope".into()],
            credentials: vec!["nope".into()],
        };
        let payload = build_payload::<Infallible>(&library(), &pick, "1", None).unwrap();
        assert_eq!(payload.entries.len(), 1);
        assert!(payload.entries[0].folder_path.is_empty());
        assert_eq!(payload.entries[0].default_connection, Some(0));
        assert!(payload.credentials.is_empty());
    }

    #[test]
    fn round_trips_through_json() {
        let payload = build_payload::<Infallible>(&library(), &selection(), "1", None).unwrap();
        let json = serde_json::to_string(&payload).unwrap();
        let back: ExportPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
    }
}
