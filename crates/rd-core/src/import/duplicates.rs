//! "Already exists" for the import preview (Swift `WinboxImporter.isDuplicate` and
//! `AppState.markDuplicates`), applied to every importer.

use super::{ImportedConnection, ImportedCredential, ParsedImport};
use crate::library::Library;
use crate::model::{Connection, Credential};

/// The existing folder an import group path lands in under `parent` (`None`: the root), following the
/// folders by name. `Some(None)` is the root itself; `None` means part of the path doesn't exist yet.
#[must_use]
pub fn folder_for_path(library: &Library, parent: Option<&str>, path: &[String]) -> Option<Option<String>> {
    let mut current = parent.map(str::to_owned);
    for name in path {
        let folder = library
            .folders
            .iter()
            .find(|f| f.parent_id == current && f.name == *name)?;
        current = Some(folder.id.clone());
    }
    Some(current)
}

/// Already in the library: a connection of the same type in that folder with the same host (ignoring case)
/// and user.
#[must_use]
pub fn is_duplicate(item: &ImportedConnection, connections: &[Connection], folder_id: Option<&str>) -> bool {
    let host = item.host.to_lowercase();
    connections.iter().any(|c| {
        c.kind == item.kind
            && c.folder_id.as_deref() == folder_id
            && c.host.to_lowercase() == host
            && c.username == item.username
    })
}

/// A credential with the same name, user and domain exists.
#[must_use]
pub fn credential_is_duplicate(item: &ImportedCredential, credentials: &[Credential]) -> bool {
    credentials
        .iter()
        .any(|c| c.name == item.name && c.username == item.username && c.domain == item.domain)
}

/// Duplicate flags of a parsed file, in the order of its connections and credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateFlags {
    /// One per [`ParsedImport::connections`].
    pub connections: Vec<bool>,
    /// One per [`ParsedImport::credentials`].
    pub credentials: Vec<bool>,
}

impl ParsedImport {
    /// Which entries already exist when importing under `parent` (`None`: the root).
    #[must_use]
    pub fn duplicates(&self, library: &Library, parent: Option<&str>) -> DuplicateFlags {
        DuplicateFlags {
            connections: self
                .connections
                .iter()
                .map(|item| {
                    folder_for_path(library, parent, &item.group_path)
                        .is_some_and(|folder| is_duplicate(item, &library.connections, folder.as_deref()))
                })
                .collect(),
            credentials: self
                .credentials
                .iter()
                .map(|item| credential_is_duplicate(item, &library.credentials))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnectionType, Folder};

    fn folder(id: &str, name: &str, parent: Option<&str>) -> Folder {
        Folder {
            id: id.into(),
            parent_id: parent.map(Into::into),
            name: name.into(),
            sort_order: 0,
            color: None,
        }
    }

    fn existing(folder_id: Option<&str>, host: &str, user: &str) -> Connection {
        let mut c = Connection::new(ConnectionType::Mikrotik, "x", host);
        c.folder_id = folder_id.map(Into::into);
        c.username = user.into();
        c
    }

    fn item(group: &[&str], host: &str, user: &str) -> ImportedConnection {
        let mut i = ImportedConnection::new(ConnectionType::Mikrotik, "n", host);
        i.group_path = group.iter().map(|s| (*s).to_owned()).collect();
        i.username = user.into();
        i
    }

    fn library() -> Library {
        Library {
            folders: vec![folder("T", "Target", None), folder("G", "Group", Some("T"))],
            connections: vec![
                existing(Some("G"), "Router.Example.com", "admin"),
                existing(None, "192.0.2.1", ""),
            ],
            ..Library::default()
        }
    }

    #[test]
    fn resolves_folders_by_path() {
        let lib = library();
        assert_eq!(folder_for_path(&lib, None, &[]), Some(None));
        assert_eq!(
            folder_for_path(&lib, None, &["Target".into()]),
            Some(Some("T".into()))
        );
        assert_eq!(
            folder_for_path(&lib, Some("T"), &["Group".into()]),
            Some(Some("G".into()))
        );
        assert_eq!(folder_for_path(&lib, None, &["Group".into()]), None);
        assert_eq!(
            folder_for_path(&lib, None, &["Target".into(), "Nope".into()]),
            None
        );
    }

    #[test]
    fn same_type_folder_host_and_user() {
        let lib = library();
        let dup = item(&["Group"], "router.example.com", "admin");
        assert!(is_duplicate(&dup, &lib.connections, Some("G")));
        assert!(!is_duplicate(&dup, &lib.connections, Some("T")));
        assert!(!is_duplicate(
            &item(&["Group"], "router.example.com", "someone"),
            &lib.connections,
            Some("G")
        ));
        let mut rdp = dup.clone();
        rdp.kind = ConnectionType::Rdp;
        assert!(!is_duplicate(&rdp, &lib.connections, Some("G")));
    }

    #[test]
    fn flags_follow_the_import_root() {
        let lib = library();
        let parsed = ParsedImport {
            connections: vec![
                item(&["Group"], "ROUTER.example.com", "admin"),
                item(&[], "192.0.2.1", ""),
                item(&["Missing"], "192.0.2.1", ""),
            ],
            credentials: vec![ImportedCredential {
                reference: "R".into(),
                name: "admin".into(),
                username: "root".into(),
                domain: String::new(),
                password: None,
                ssh_key_path: None,
            }],
            ..ParsedImport::default()
        };
        assert_eq!(
            parsed.duplicates(&lib, Some("T")).connections,
            [true, false, false]
        );
        assert_eq!(parsed.duplicates(&lib, None).connections, [false, true, false]);
        assert_eq!(parsed.duplicates(&lib, None).credentials, [false]);

        let mut with_credential = lib.clone();
        let mut credential = Credential::new("admin");
        credential.username = "root".into();
        with_credential.credentials.push(credential);
        assert_eq!(parsed.duplicates(&with_credential, None).credentials, [true]);
    }
}
