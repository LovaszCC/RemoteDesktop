//! The whole library as loaded from the store, with the Swift `Library` helpers.

use std::collections::HashSet;

use crate::model::{Connection, Credential, Entry, Folder, PortForward, SavedSplit};

/// Every record, each list in the repository's sort order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    /// Folders by sort order, then name.
    pub folders: Vec<Folder>,
    /// Entries by name.
    pub entries: Vec<Entry>,
    /// Connections by name, each with its entry's shared fields.
    pub connections: Vec<Connection>,
    /// Credentials by name.
    pub credentials: Vec<Credential>,
    /// Port forwards by local port.
    pub port_forwards: Vec<PortForward>,
    /// Saved splits by sort order, then creation time.
    pub saved_splits: Vec<SavedSplit>,
}

impl Library {
    /// The entry with `id`, if any.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The connections of an entry, by sort order, then creation time.
    #[must_use]
    pub fn connections_of(&self, entry_id: &str) -> Vec<&Connection> {
        let mut connections: Vec<&Connection> = self
            .connections
            .iter()
            .filter(|c| c.entry_id == entry_id)
            .collect();
        connections.sort_by_key(|c| (c.sort_order, c.created_at));
        connections
    }

    /// The connection an entry opens by default: its stored default when that is still one of its
    /// connections, otherwise the first of [`connections_of`](Self::connections_of).
    #[must_use]
    pub fn default_connection(&self, entry_id: &str) -> Option<&Connection> {
        let connections = self.connections_of(entry_id);
        let stored = self
            .entry(entry_id)
            .and_then(|e| e.default_connection_id.as_deref());
        stored
            .and_then(|id| connections.iter().find(|c| c.id == id))
            .or_else(|| connections.first())
            .copied()
    }

    /// The credential with `id`, if any.
    #[must_use]
    pub fn credential(&self, id: Option<&str>) -> Option<&Credential> {
        let id = id?;
        self.credentials.iter().find(|c| c.id == id)
    }

    /// The connection with its linked credential applied, ready to launch.
    #[must_use]
    pub fn resolved(&self, connection: &Connection) -> Connection {
        connection.resolved(self.credential(connection.credential_id.as_deref()))
    }

    /// Connections linked to a credential.
    pub fn connections_using<'a>(&'a self, credential_id: &'a str) -> impl Iterator<Item = &'a Connection> {
        self.connections
            .iter()
            .filter(move |c| c.credential_id.as_deref() == Some(credential_id))
    }

    /// Ids behind a sidebar selection: the selected connections, then every connection inside the selected
    /// folders and their subfolders, de-duplicated in that order.
    #[must_use]
    pub fn connection_ids_for(&self, connection_ids: &[String], folder_ids: &[String]) -> Vec<String> {
        let mut folders: HashSet<&str> = folder_ids.iter().map(String::as_str).collect();
        let mut pending: Vec<&str> = folder_ids.iter().map(String::as_str).collect();
        while let Some(parent) = pending.pop() {
            for child in &self.folders {
                if child.parent_id.as_deref() == Some(parent) && folders.insert(&child.id) {
                    pending.push(&child.id);
                }
            }
        }
        let in_folders = self
            .connections
            .iter()
            .filter(|c| c.folder_id.as_deref().is_some_and(|f| folders.contains(f)))
            .map(|c| &c.id);
        let mut seen = HashSet::new();
        connection_ids
            .iter()
            .chain(in_folders)
            .filter(|id| seen.insert(id.as_str()))
            .cloned()
            .collect()
    }

    /// [`connection_ids_for`](Self::connection_ids_for) over sidebar tags (`c:`, `fav:`, `f:`; others
    /// ignored), like Swift `Library.connectionIds(forTags:)`.
    #[must_use]
    pub fn connection_ids_for_tags<S: AsRef<str>>(&self, tags: &[S]) -> Vec<String> {
        let items = crate::tree::sidebar_items(tags);
        self.connection_ids_for(&items.connection_ids, &items.folder_ids)
    }

    /// The forwards of one connection.
    pub fn port_forwards_for<'a>(&'a self, connection_id: &'a str) -> impl Iterator<Item = &'a PortForward> {
        self.port_forwards
            .iter()
            .filter(move |f| f.connection_id == connection_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnectionType, Sealed, Timestamp};

    fn folder(id: &str, parent: Option<&str>) -> Folder {
        Folder {
            id: id.into(),
            parent_id: parent.map(Into::into),
            name: id.into(),
            sort_order: 0,
            color: None,
        }
    }

    fn connection(id: &str, folder: Option<&str>) -> Connection {
        let mut c = Connection::new(ConnectionType::Ssh, id, "h");
        c.id = id.into();
        c.entry_id = id.into();
        c.folder_id = folder.map(Into::into);
        c
    }

    fn library() -> Library {
        Library {
            folders: vec![
                folder("F1", None),
                folder("F2", Some("F1")),
                folder("F3", Some("F2")),
                folder("G", None),
            ],
            connections: vec![
                connection("A", Some("F1")),
                connection("B", Some("F3")),
                connection("C", Some("G")),
                connection("D", None),
            ],
            ..Library::default()
        }
    }

    #[test]
    fn folder_selection_includes_nested_subfolders() {
        let ids = library().connection_ids_for(&[], &["F1".into()]);
        assert_eq!(ids, ["A", "B"]);
    }

    #[test]
    fn selected_connections_come_first_without_duplicates() {
        let ids = library().connection_ids_for(&["D".into(), "B".into()], &["F2".into()]);
        assert_eq!(ids, ["D", "B"]);
        assert_eq!(library().connection_ids_for(&[], &[]), Vec::<String>::new());
    }

    /// `CredentialLinkTests`: tagsResolveToConnectionsIncludingFolderTrees.
    #[test]
    fn tags_resolve_to_connections_including_folder_trees() {
        let lib = library();
        let ids = lib.connection_ids_for_tags(&["c:D", "fav:D", "f:F1", "c:B", "k:x"]);
        assert_eq!(ids, ["D", "B", "A"]);
        assert!(lib.connection_ids_for_tags(&["k:x"]).is_empty());
    }

    #[test]
    fn resolves_linked_credential() {
        let mut lib = library();
        let mut credential = Credential::new("admin");
        credential.username = "root".into();
        credential.password_enc = Some(Sealed::from_bytes(vec![7]));
        lib.connections[0].credential_id = Some(credential.id.clone());
        lib.credentials.push(credential.clone());

        let resolved = lib.resolved(&lib.connections[0]);
        assert_eq!(resolved.username, "root");
        assert_eq!(lib.resolved(&lib.connections[1]), lib.connections[1]);
        assert_eq!(lib.connections_using(&credential.id).count(), 1);
        assert!(lib.credential(None).is_none());
    }

    #[test]
    fn entry_connections_sort_and_default() {
        let mut lib = library();
        let mut entry = Entry::new("E", "h");
        entry.id = "E".into();
        let member = |id: &str, sort_order: i64, created: i64| {
            let mut c = connection(id, None);
            c.entry_id = "E".into();
            c.sort_order = sort_order;
            c.created_at = Timestamp::from_unix(created);
            c
        };
        lib.connections
            .extend([member("X", 1, 0), member("Y", 0, 5), member("Z", 0, 1)]);
        lib.entries.push(entry);
        let ids = |list: Vec<&Connection>| list.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(lib.connections_of("E")), ["Z", "Y", "X"]);
        assert_eq!(lib.default_connection("E").unwrap().id, "Z");
        lib.entries[0].default_connection_id = Some("X".into());
        assert_eq!(lib.default_connection("E").unwrap().id, "X");
        lib.entries[0].default_connection_id = Some("A".into());
        assert_eq!(lib.default_connection("E").unwrap().id, "Z");
        assert_eq!(lib.entry("E").unwrap().name, "E");
        assert!(lib.entry("nope").is_none());
        assert!(lib.default_connection("nope").is_none());
        assert_eq!(ids(lib.connections_of("A")), ["A"]);
    }

    #[test]
    fn port_forwards_for_connection() {
        let mut lib = library();
        let forward = |id: &str, conn: &str| PortForward {
            id: id.into(),
            connection_id: conn.into(),
            name: String::new(),
            local_port: 1,
            remote_host: "h".into(),
            remote_port: 2,
            created_at: Timestamp::from_unix(0),
        };
        lib.port_forwards = vec![forward("P1", "A"), forward("P2", "B"), forward("P3", "A")];
        assert_eq!(
            lib.port_forwards_for("A")
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>(),
            ["P1", "P3"]
        );
    }
}
