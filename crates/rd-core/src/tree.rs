//! The sidebar: the folder tree, favorites, search and the row tags used for selection and drag and drop.
//!
//! A port of the Swift `LibraryTree` and `SidebarDrag`.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::library::Library;
use crate::model::{Connection, Credential, Entry, Folder};
use crate::natural::natural_cmp;

/// A sidebar node: a folder with its children, or an entry leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeNode<'a> {
    /// A folder; `children` lists its subfolders first, then its entries.
    Folder {
        /// The folder.
        folder: &'a Folder,
        /// Subfolders, then entries.
        children: Vec<TreeNode<'a>>,
    },
    /// An entry.
    Entry(&'a Entry),
}

impl TreeNode<'_> {
    /// The row's sidebar tag (`f:<id>` or `e:<id>`).
    #[must_use]
    pub fn tag(&self) -> SidebarTag {
        match self {
            Self::Folder { folder, .. } => SidebarTag::Folder(folder.id.clone()),
            Self::Entry(entry) => SidebarTag::Entry(entry.id.clone()),
        }
    }

    /// A folder's children; `None` for an entry.
    #[must_use]
    pub fn children(&self) -> Option<&[Self]> {
        match self {
            Self::Folder { children, .. } => Some(children),
            Self::Entry(_) => None,
        }
    }
}

/// The tree under the root: at each level folders by (sort order, lowercased name), then entries in
/// natural name order. Entries whose folder no longer exists are listed at the root.
#[must_use]
pub fn build(library: &Library) -> Vec<TreeNode<'_>> {
    let mut folders_by_parent: HashMap<Option<&str>, Vec<&Folder>> = HashMap::new();
    for folder in &library.folders {
        folders_by_parent
            .entry(folder.parent_id.as_deref())
            .or_default()
            .push(folder);
    }
    for folders in folders_by_parent.values_mut() {
        folders.sort_by_cached_key(|f| (f.sort_order, f.name.to_lowercase()));
    }
    let known: HashSet<&str> = library.folders.iter().map(|f| f.id.as_str()).collect();
    let mut entries_by_folder: HashMap<Option<&str>, Vec<&Entry>> = HashMap::new();
    for entry in &library.entries {
        let folder = entry.folder_id.as_deref().filter(|id| known.contains(id));
        entries_by_folder.entry(folder).or_default().push(entry);
    }
    for entries in entries_by_folder.values_mut() {
        entries.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    }
    nodes(None, &folders_by_parent, &entries_by_folder, &mut HashSet::new())
}

fn nodes<'a>(
    parent: Option<&str>,
    folders: &HashMap<Option<&str>, Vec<&'a Folder>>,
    entries: &HashMap<Option<&str>, Vec<&'a Entry>>,
    visited: &mut HashSet<&'a str>,
) -> Vec<TreeNode<'a>> {
    let mut result = Vec::new();
    for &folder in folders.get(&parent).into_iter().flatten() {
        // A parent cycle is never reachable from the root, but guard against a self-parented folder.
        if !visited.insert(&folder.id) {
            continue;
        }
        let children = nodes(Some(&folder.id), folders, entries, visited);
        result.push(TreeNode::Folder { folder, children });
    }
    result.extend(
        entries
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|&e| TreeNode::Entry(e)),
    );
    result
}

/// Favorite entries in natural name order, each once however many connections it has.
#[must_use]
pub fn favorites(library: &Library) -> Vec<&Entry> {
    let mut result: Vec<&Entry> = library.entries.iter().filter(|e| e.is_favorite).collect();
    result.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    result
}

fn terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The sidebar search: every whitespace-separated term must occur in the entry's name, host, notes or
/// folder path, or in the user name or label of one of its connections (case-insensitive). An empty query
/// finds nothing. Results are entries in natural name order.
#[must_use]
pub fn search<'a>(library: &'a Library, query: &str) -> Vec<&'a Entry> {
    let terms = terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let paths = FolderPaths::new(library);
    let mut result: Vec<&Entry> = library
        .entries
        .iter()
        .filter(|e| {
            let path = paths.path(e.folder_id.as_deref());
            let mut fields = vec![e.name.as_str(), &e.host, &e.notes, &path];
            for connection in library.connections.iter().filter(|c| c.entry_id == e.id) {
                fields.extend([connection.username.as_str(), &connection.label]);
            }
            let haystack = fields.join(" ").to_lowercase();
            terms.iter().all(|t| haystack.contains(t.as_str()))
        })
        .collect();
    result.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    result
}

/// `connections` in natural name order, filtered so that every term of a non-empty query occurs in the
/// name, host, user or folder path. An empty query keeps every connection.
#[must_use]
pub fn matching<'a>(
    connections: impl IntoIterator<Item = &'a Connection>,
    query: &str,
    library: &Library,
) -> Vec<&'a Connection> {
    let terms = terms(query);
    let paths = FolderPaths::new(library);
    let mut result: Vec<&Connection> = connections
        .into_iter()
        .filter(|c| {
            let haystack = [
                c.name.as_str(),
                &c.host,
                &c.username,
                &paths.path(c.folder_id.as_deref()),
            ]
            .join(" ")
            .to_lowercase();
            terms.iter().all(|t| haystack.contains(t.as_str()))
        })
        .collect();
    result.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    result
}

/// Connections that can be linked to a credential: those not linked to it yet (including ones linked to
/// another credential), filtered like [`matching`].
#[must_use]
pub fn link_candidates<'a>(library: &'a Library, credential_id: &str, query: &str) -> Vec<&'a Connection> {
    matching(
        library
            .connections
            .iter()
            .filter(|c| c.credential_id.as_deref() != Some(credential_id)),
        query,
        library,
    )
}

/// Credentials whose name, `DOMAIN\user` or notes contain every term, in library order. An empty query
/// finds nothing.
#[must_use]
pub fn search_credentials<'a>(library: &'a Library, query: &str) -> Vec<&'a Credential> {
    let terms = terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    library
        .credentials
        .iter()
        .filter(|c| {
            let haystack = format!("{} {} {}", c.name, c.display_user(), c.notes).to_lowercase();
            terms.iter().all(|t| haystack.contains(t.as_str()))
        })
        .collect()
}

/// `Parent / Child` for a folder id; empty for the root or an unknown folder. Safe against parent cycles.
#[must_use]
pub fn folder_path(library: &Library, folder_id: Option<&str>) -> String {
    FolderPaths::new(library).path(folder_id)
}

/// Every folder depth first for pickers, siblings by lowercased name, with the depth (root = 0). The
/// `excluding` folder and its whole subtree are left out (a folder can't move into itself).
#[must_use]
pub fn flattened_folders<'a>(library: &'a Library, excluding: Option<&str>) -> Vec<(&'a Folder, usize)> {
    let mut by_parent: HashMap<Option<&str>, Vec<&Folder>> = HashMap::new();
    for folder in &library.folders {
        by_parent
            .entry(folder.parent_id.as_deref())
            .or_default()
            .push(folder);
    }
    for folders in by_parent.values_mut() {
        folders.sort_by_cached_key(|f| f.name.to_lowercase());
    }
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    walk(None, 0, &by_parent, excluding, &mut visited, &mut result);
    result
}

fn walk<'a>(
    parent: Option<&str>,
    depth: usize,
    by_parent: &HashMap<Option<&str>, Vec<&'a Folder>>,
    excluding: Option<&str>,
    visited: &mut HashSet<&'a str>,
    result: &mut Vec<(&'a Folder, usize)>,
) {
    for &folder in by_parent.get(&parent).into_iter().flatten() {
        if Some(folder.id.as_str()) == excluding || !visited.insert(&folder.id) {
            continue;
        }
        result.push((folder, depth));
        walk(Some(&folder.id), depth + 1, by_parent, excluding, visited, result);
    }
}

/// Folder paths with the folder index built once.
struct FolderPaths<'a> {
    by_id: HashMap<&'a str, &'a Folder>,
}

impl<'a> FolderPaths<'a> {
    fn new(library: &'a Library) -> Self {
        Self {
            by_id: library.folders.iter().map(|f| (f.id.as_str(), f)).collect(),
        }
    }

    fn path(&self, folder_id: Option<&str>) -> String {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut cursor = folder_id;
        while let Some(id) = cursor
            && let Some(folder) = self.by_id.get(id)
            && seen.insert(id)
        {
            names.push(folder.name.as_str());
            cursor = folder.parent_id.as_deref();
        }
        names.reverse();
        names.join(" / ")
    }
}

/// The identity of a sidebar row, used for selection and drag and drop.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SidebarTag {
    /// `f:<id>`, a folder.
    Folder(String),
    /// `e:<id>`, an entry in the tree or in the search results.
    Entry(String),
    /// `c:<id>`, one connection of an entry.
    Connection(String),
    /// `fav:<id>`, the Favorites row of an entry.
    Favorite(String),
    /// `k:<id>`, a credential.
    Credential(String),
    /// `s:<id>`, a saved split.
    Split(String),
}

impl SidebarTag {
    /// Parses `f:`, `e:`, `c:`, `fav:`, `k:` or `s:` followed by an id; anything else is `None`.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        // `fav:` before `f:` is not needed (`fav:` doesn't start with `f:`), but keep the Swift order.
        if let Some(id) = tag.strip_prefix("f:") {
            Some(Self::Folder(id.to_owned()))
        } else if let Some(id) = tag.strip_prefix("e:") {
            Some(Self::Entry(id.to_owned()))
        } else if let Some(id) = tag.strip_prefix("c:") {
            Some(Self::Connection(id.to_owned()))
        } else if let Some(id) = tag.strip_prefix("fav:") {
            Some(Self::Favorite(id.to_owned()))
        } else if let Some(id) = tag.strip_prefix("k:") {
            Some(Self::Credential(id.to_owned()))
        } else {
            tag.strip_prefix("s:").map(|id| Self::Split(id.to_owned()))
        }
    }

    /// The record id behind the tag.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Folder(id)
            | Self::Entry(id)
            | Self::Connection(id)
            | Self::Favorite(id)
            | Self::Credential(id)
            | Self::Split(id) => id,
        }
    }
}

impl fmt::Display for SidebarTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self {
            Self::Folder(_) => "f:",
            Self::Entry(_) => "e:",
            Self::Connection(_) => "c:",
            Self::Favorite(_) => "fav:",
            Self::Credential(_) => "k:",
            Self::Split(_) => "s:",
        };
        write!(f, "{prefix}{}", self.id())
    }
}

/// What a dragged row carries: the whole selection (sorted, one tag per line) when the row is part of a
/// selection of more than one row, otherwise just the row's tag.
#[must_use]
pub fn drag_payload(tag: &str, selection: &[String]) -> String {
    let mut sorted: Vec<&str> = selection.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() > 1 && sorted.contains(&tag) {
        sorted.join("\n")
    } else {
        tag.to_owned()
    }
}

/// The tags in dropped payloads, in order.
#[must_use]
pub fn tags_from_payloads(payloads: &[String]) -> Vec<String> {
    payloads
        .iter()
        .flat_map(|p| p.split('\n'))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The ids behind a set of sidebar tags, each list de-duplicated in tag order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarItems {
    /// From `e:` and `fav:` tags (a favorite row is its entry).
    pub entry_ids: Vec<String>,
    /// From `c:` tags.
    pub connection_ids: Vec<String>,
    /// From `f:` tags.
    pub folder_ids: Vec<String>,
    /// From `k:` tags.
    pub credential_ids: Vec<String>,
}

/// Sorts tags into entry, connection, folder and credential ids. Split and unknown tags are ignored.
#[must_use]
pub fn sidebar_items<S: AsRef<str>>(tags: &[S]) -> SidebarItems {
    let mut items = SidebarItems::default();
    for tag in tags {
        match SidebarTag::parse(tag.as_ref()) {
            Some(SidebarTag::Folder(id)) => items.folder_ids.push(id),
            Some(SidebarTag::Entry(id) | SidebarTag::Favorite(id)) => items.entry_ids.push(id),
            Some(SidebarTag::Connection(id)) => items.connection_ids.push(id),
            Some(SidebarTag::Credential(id)) => items.credential_ids.push(id),
            Some(SidebarTag::Split(_)) | None => {}
        }
    }
    unique(&mut items.entry_ids);
    unique(&mut items.connection_ids);
    unique(&mut items.folder_ids);
    unique(&mut items.credential_ids);
    items
}

/// What a drop moves: entries (a connection row stands for its entry) and folders. Credentials have no
/// place in the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Movable {
    /// Entries to move.
    pub entry_ids: Vec<String>,
    /// Connections whose entries move.
    pub connection_ids: Vec<String>,
    /// Folders to move.
    pub folder_ids: Vec<String>,
}

/// The entries, connections and folders behind dropped tags (see [`sidebar_items`]).
#[must_use]
pub fn movable<S: AsRef<str>>(tags: &[S]) -> Movable {
    let items = sidebar_items(tags);
    Movable {
        entry_ids: items.entry_ids,
        connection_ids: items.connection_ids,
        folder_ids: items.folder_ids,
    }
}

fn unique(ids: &mut Vec<String>) {
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConnectionType;

    fn folder(id: &str, parent: Option<&str>, name: &str, sort_order: i64) -> Folder {
        Folder {
            id: id.into(),
            parent_id: parent.map(Into::into),
            name: name.into(),
            sort_order,
            color: None,
        }
    }

    fn connection(
        id: &str,
        folder: Option<&str>,
        kind: ConnectionType,
        name: &str,
        host: &str,
    ) -> Connection {
        let mut c = Connection::new(kind, name, host);
        c.id = id.into();
        c.entry_id = id.into();
        c.folder_id = folder.map(Into::into);
        c
    }

    /// Gives every connection without an entry its own one.
    fn with_entries(mut library: Library) -> Library {
        for connection in &library.connections {
            if library.entry(&connection.entry_id).is_none() {
                library.entries.push(Entry::for_connection(connection));
            }
        }
        library
    }

    /// `LibraryTreeTests` fixture.
    fn library() -> Library {
        let mut c1 = connection("1", Some("web"), ConnectionType::Ssh, "web02", "192.0.2.2");
        c1.is_favorite = true;
        let c2 = connection("2", Some("web"), ConnectionType::Ssh, "web01", "192.0.2.1");
        let mut c3 = connection("3", None, ConnectionType::Rdp, "laptop", "laptop.example.com");
        c3.username = "bob".into();
        let mut c4 = connection("4", Some("gone"), ConnectionType::Rdp, "orphan", "x");
        c4.is_favorite = true;
        let mut c5 = connection("5", Some("prod"), ConnectionType::Rdp, "dc01", "dc");
        c5.notes = "domain controller".into();
        with_entries(Library {
            folders: vec![
                folder("arch", None, "Archive", 1),
                folder("web", Some("prod"), "Web", 0),
                folder("prod", None, "Prod", 0),
            ],
            connections: vec![c1, c2, c3, c4, c5],
            ..Library::default()
        })
    }

    fn tags(nodes: &[TreeNode<'_>]) -> Vec<String> {
        nodes.iter().map(|n| n.tag().to_string()).collect()
    }

    fn ids(connections: &[&Connection]) -> Vec<String> {
        connections.iter().map(|c| c.id.clone()).collect()
    }

    fn entry_ids(entries: &[&Entry]) -> Vec<String> {
        entries.iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn builds_nested_tree_folders_first() {
        let library = library();
        let tree = build(&library);
        assert_eq!(tags(&tree), ["f:prod", "f:arch", "e:3", "e:4"]);

        let prod = tree[0].children().unwrap();
        assert_eq!(tags(prod), ["f:web", "e:5"]);
        assert_eq!(tags(prod[0].children().unwrap()), ["e:2", "e:1"]);
        assert_eq!(tree[1].children(), Some(&[][..]));
        assert_eq!(tree[2].children(), None);
    }

    #[test]
    fn tree_connections_use_natural_order() {
        let library = with_entries(Library {
            connections: vec![
                connection("a", None, ConnectionType::Ssh, "srv10", "h"),
                connection("b", None, ConnectionType::Ssh, "srv9", "h"),
                connection("c", None, ConnectionType::Ssh, "Srv1", "h"),
            ],
            ..Library::default()
        });
        assert_eq!(tags(&build(&library)), ["e:c", "e:b", "e:a"]);
    }

    #[test]
    fn favorites_list_entries_once() {
        let mut library = library();
        let mut entry = Entry::new("box", "box.example.com");
        entry.id = "E".into();
        entry.is_favorite = true;
        for id in ["E1", "E2"] {
            let mut c = connection(id, None, ConnectionType::Ssh, "box", "box.example.com");
            c.entry_id = "E".into();
            entry.overlay(&mut c);
            library.connections.push(c);
        }
        library.entries.push(entry);
        let ids: Vec<_> = favorites(&library).iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, ["E", "4", "1"]);
    }

    #[test]
    fn favorites_sorted_by_name() {
        assert_eq!(entry_ids(&favorites(&library())), ["4", "1"]);
    }

    #[test]
    fn search_all_terms_across_fields() {
        let library = library();
        assert!(search(&library, "").is_empty());
        assert!(search(&library, "   ").is_empty());
        assert_eq!(entry_ids(&search(&library, "WEB")), ["2", "1"]);
        assert_eq!(entry_ids(&search(&library, "prod web01")), ["2"]);
        assert_eq!(entry_ids(&search(&library, "bob")), ["3"]);
        assert_eq!(entry_ids(&search(&library, "controller")), ["5"]);
        let mut labelled = library.clone();
        labelled.connections[2].label = "Console".into();
        assert_eq!(entry_ids(&search(&labelled, "laptop console")), ["3"]);
        assert!(search(&library, "nomatch").is_empty());
    }

    #[test]
    fn folder_path_and_cycle_safety() {
        let library = library();
        assert_eq!(folder_path(&library, Some("web")), "Prod / Web");
        assert_eq!(folder_path(&library, None), "");
        assert_eq!(folder_path(&library, Some("missing")), "");
        let cyclic = Library {
            folders: vec![folder("a", Some("b"), "A", 0), folder("b", Some("a"), "B", 0)],
            ..Library::default()
        };
        assert_eq!(folder_path(&cyclic, Some("a")), "B / A");
    }

    #[test]
    fn flattened_folders_with_exclusion() {
        let library = library();
        let all = flattened_folders(&library, None);
        assert_eq!(
            all.iter().map(|(f, _)| f.id.as_str()).collect::<Vec<_>>(),
            ["arch", "prod", "web"]
        );
        assert_eq!(all.iter().map(|(_, d)| *d).collect::<Vec<_>>(), [0, 0, 1]);
        let excluded = flattened_folders(&library, Some("prod"));
        assert_eq!(
            excluded.iter().map(|(f, _)| f.id.as_str()).collect::<Vec<_>>(),
            ["arch"]
        );
    }

    #[test]
    fn search_credentials_keeps_library_order() {
        let mut a = Credential::new("Domain Admin");
        a.id = "a".into();
        a.username = "admin".into();
        a.domain = "CORP".into();
        let mut b = Credential::new("Linux root");
        b.id = "b".into();
        b.username = "root".into();
        b.notes = "shared admin".into();
        let library = Library {
            credentials: vec![a, b],
            ..Library::default()
        };
        let found = |q: &str| {
            search_credentials(&library, q)
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>()
        };
        assert!(found("").is_empty());
        assert_eq!(found("corp"), ["a"]);
        assert_eq!(found("root linux"), ["b"]);
        assert_eq!(found("admin"), ["a", "b"]);
    }

    /// `CredentialLinkTests`: linkCandidatesSkipLinkedAndMatchQuery.
    #[test]
    fn link_candidates_skip_linked_and_match_query() {
        let library = Library {
            folders: vec![folder("bp", None, "Budapest", 0)],
            connections: {
                let mut linked = connection("linked", None, ConnectionType::Ssh, "linked", "h");
                linked.credential_id = Some("admin".into());
                let mut router = connection(
                    "router",
                    Some("bp"),
                    ConnectionType::Mikrotik,
                    "router",
                    "192.0.2.1",
                );
                router.credential_id = Some("other".into());
                let server = connection("server", None, ConnectionType::Rdp, "Server", "srv");
                vec![linked, router, server]
            },
            ..Library::default()
        };
        assert_eq!(ids(&link_candidates(&library, "admin", "")), ["router", "server"]);
        assert_eq!(
            ids(&link_candidates(&library, "admin", "budapest 192.0")),
            ["router"]
        );
        assert_eq!(ids(&link_candidates(&library, "admin", "SRV")), ["server"]);
        assert!(link_candidates(&library, "admin", "linked").is_empty());
    }

    /// `CredentialLinkTests`: matchingFiltersAndSortsByName.
    #[test]
    fn matching_filters_and_sorts_by_name() {
        let b = connection("b", Some("sz"), ConnectionType::Ssh, "b-host", "192.0.2.11");
        let mut a = connection("a", None, ConnectionType::Ssh, "A-host", "192.0.2.22");
        a.username = "root".into();
        a.notes = "notes are not searched".into();
        let library = Library {
            folders: vec![folder("sz", None, "Szeged", 0)],
            connections: vec![b.clone(), a.clone()],
            ..Library::default()
        };
        let both = [&b, &a];
        assert_eq!(ids(&matching(both, "", &library)), ["a", "b"]);
        assert_eq!(ids(&matching(both, "szeged", &library)), ["b"]);
        assert_eq!(ids(&matching(both, "ROOT 192.0.2.2", &library)), ["a"]);
        assert!(matching(both, "nope", &library).is_empty());
        assert!(matching(both, "searched", &library).is_empty());
    }

    #[test]
    fn tags_parse_and_format() {
        for raw in ["f:1", "e:6", "c:2", "fav:3", "k:4", "s:5"] {
            assert_eq!(SidebarTag::parse(raw).unwrap().to_string(), raw);
        }
        assert_eq!(SidebarTag::parse("fav:9"), Some(SidebarTag::Favorite("9".into())));
        assert_eq!(SidebarTag::parse("e:9"), Some(SidebarTag::Entry("9".into())));
        assert_eq!(SidebarTag::parse("f:9").unwrap().id(), "9");
        assert_eq!(SidebarTag::parse("junk"), None);
        assert_eq!(SidebarTag::parse("x:3"), None);
    }

    #[test]
    fn payload_carries_selection_only_for_selected_row() {
        let selection: Vec<String> = ["c:2", "f:9", "c:1"].map(String::from).to_vec();
        assert_eq!(drag_payload("c:1", &selection), "c:1\nc:2\nf:9");
        assert_eq!(drag_payload("c:7", &selection), "c:7");
        assert_eq!(drag_payload("c:1", &["c:1".to_owned()]), "c:1");
    }

    #[test]
    fn tags_from_payloads_split_lines() {
        assert_eq!(
            tags_from_payloads(&["c:1\nf:2".to_owned(), "c:3".to_owned()]),
            ["c:1", "f:2", "c:3"]
        );
        assert!(tags_from_payloads(&[]).is_empty());
    }

    #[test]
    fn movable_maps_favorites_and_ignores_credentials() {
        let items = movable(&["e:1", "fav:1", "fav:2", "c:7", "f:9", "k:5", "f:9", "junk"]);
        assert_eq!(items.entry_ids, ["1", "2"]);
        assert_eq!(items.connection_ids, ["7"]);
        assert_eq!(items.folder_ids, ["9"]);
    }

    #[test]
    fn items_include_credentials() {
        let items = sidebar_items(&["k:5", "c:1", "fav:1", "e:2", "f:9", "k:5", "x:3", "s:4"]);
        assert_eq!(items.connection_ids, ["1"]);
        assert_eq!(items.entry_ids, ["1", "2"]);
        assert_eq!(items.folder_ids, ["9"]);
        assert_eq!(items.credential_ids, ["5"]);
    }
}
