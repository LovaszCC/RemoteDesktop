//! Which records of an export already exist in the library, so the import can ask what to do with them.

use super::ExportPayload;
use crate::import::folder_for_path;
use crate::library::Library;

/// The existing record each exported one collides with, in payload order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferConflicts {
    /// One per [`ExportPayload::entries`]: the id of an existing entry.
    pub entries: Vec<Option<String>>,
    /// One per [`ExportPayload::credentials`]: the id of an existing credential.
    pub credentials: Vec<Option<String>>,
}

/// An entry collides with one in the same folder path that has the same host or the same name (both
/// ignoring case); a credential with one of the same name (ignoring case).
#[must_use]
pub fn conflicts(payload: &ExportPayload, library: &Library) -> TransferConflicts {
    TransferConflicts {
        entries: payload
            .entries
            .iter()
            .map(|item| {
                let folder = folder_for_path(library, None, &item.folder_path)?;
                let host = item.host.to_lowercase();
                let name = item.name.to_lowercase();
                library
                    .entries
                    .iter()
                    .find(|e| {
                        e.folder_id == folder
                            && ((!host.is_empty() && e.host.to_lowercase() == host)
                                || e.name.to_lowercase() == name)
                    })
                    .map(|e| e.id.clone())
            })
            .collect(),
        credentials: payload
            .credentials
            .iter()
            .map(|item| {
                let name = item.name.to_lowercase();
                library
                    .credentials
                    .iter()
                    .find(|c| c.name.to_lowercase() == name)
                    .map(|c| c.id.clone())
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Credential, Entry, Folder};
    use crate::transfer::{ExportedCredential, ExportedEntry};

    fn exported(name: &str, host: &str, path: &[&str]) -> ExportedEntry {
        ExportedEntry {
            reference: "e".into(),
            name: name.into(),
            host: host.into(),
            notes: String::new(),
            color: None,
            is_favorite: false,
            folder_path: path.iter().map(|s| (*s).to_owned()).collect(),
            default_connection: None,
            connections: Vec::new(),
        }
    }

    fn library() -> Library {
        let mut folder = Folder::new("Acme");
        folder.id = "F".into();
        let mut inside = Entry::new("dc01", "DC01.acme.local");
        inside.id = "E1".into();
        inside.folder_id = Some("F".into());
        let mut root = Entry::new("Router", "");
        root.id = "E2".into();
        let mut credential = Credential::new("Domain Admin");
        credential.id = "K1".into();
        Library {
            folders: vec![folder],
            entries: vec![inside, root],
            credentials: vec![credential],
            ..Library::default()
        }
    }

    #[test]
    fn finds_entries_by_host_or_name_in_the_same_folder() {
        let payload = ExportPayload {
            exported_at: String::new(),
            app_version: String::new(),
            credentials: Vec::new(),
            entries: vec![
                exported("renamed", "dc01.ACME.local", &["Acme"]),
                exported("DC01", "other", &["Acme"]),
                exported("dc01", "dc01.acme.local", &[]),
                exported("router", "", &[]),
                exported("x", "", &[]),
                exported("dc01", "dc01.acme.local", &["Missing"]),
            ],
        };
        let found = conflicts(&payload, &library());
        assert_eq!(
            found.entries,
            [
                Some("E1".into()),
                Some("E1".into()),
                None,
                Some("E2".into()),
                None,
                None
            ]
        );
    }

    #[test]
    fn finds_credentials_by_name_ignoring_case() {
        let credential = |name: &str| ExportedCredential {
            reference: "k".into(),
            name: name.into(),
            username: String::new(),
            domain: String::new(),
            notes: String::new(),
            password: None,
        };
        let payload = ExportPayload {
            exported_at: String::new(),
            app_version: String::new(),
            credentials: vec![credential("domain admin"), credential("New")],
            entries: Vec::new(),
        };
        assert_eq!(
            conflicts(&payload, &library()).credentials,
            [Some("K1".into()), None]
        );
    }
}
