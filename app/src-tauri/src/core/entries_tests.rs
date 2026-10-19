//! Entries: `save_entry`, `merge_entries`, `duplicate_entry`, the search and the `e:` tags.

use rd_core::ConnectionType;
use rd_core::draft::{ConnectionDraft, EntryDraft};
use secrecy::ExposeSecret;

use super::tests::{Env, entry_draft, entry_with, record};
use super::*;
use crate::dto::{ConnectionDto, EntryDto, LibraryDto};
use rd_core::forward::PortForwardDraft;

fn connection_save(id: Option<&str>, kind: ConnectionType, username: &str) -> ConnectionSave {
    let mut draft = ConnectionDraft::new(kind);
    username.clone_into(&mut draft.username);
    ConnectionSave {
        id: id.map(Into::into),
        draft,
        password: None,
        passphrase: None,
        is_default: false,
    }
}

fn default_save(id: Option<&str>, kind: ConnectionType, username: &str) -> ConnectionSave {
    ConnectionSave {
        is_default: true,
        ..connection_save(id, kind, username)
    }
}

fn entry<'a>(library: &'a LibraryDto, id: &str) -> &'a EntryDto {
    library
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no entry {id}"))
}

fn connection<'a>(library: &'a LibraryDto, id: &str) -> &'a ConnectionDto {
    library
        .connections
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no connection {id}"))
}

#[test]
fn save_entry_creates_and_edits_connections() {
    let env = Env::new();
    let core = env.unlocked();
    let mut ssh = connection_save(None, ConnectionType::Ssh, "root");
    ssh.password = Some("pw".into());
    let id = core
        .save_entry(
            None,
            &entry_draft("Home Server", "home.example.com"),
            &[ssh, connection_save(None, ConnectionType::Rdp, "kiki")],
        )
        .unwrap();
    let library = core.library();
    let created = entry(&library, &id);
    let [ssh_id, rdp_id] = <[String; 2]>::try_from(created.connection_ids.clone()).unwrap();
    assert_eq!(created.default_connection_id.as_deref(), Some(ssh_id.as_str()));
    assert_eq!(created.name, "Home Server");
    let rdp = connection(&library, &rdp_id);
    assert_eq!(
        (rdp.entry_id.as_str(), rdp.sort_order, rdp.display_label.as_str()),
        (id.as_str(), 1, "RDP · kiki")
    );
    assert_eq!(rdp.host, "home.example.com");
    assert!(connection(&library, &ssh_id).has_password);

    // Rename and make RDP the default: both connections take the new name.
    let renamed = entry_draft("Renamed", "home.example.com");
    let same = core
        .save_entry(
            Some(&id),
            &renamed,
            &[
                connection_save(Some(&ssh_id), ConnectionType::Ssh, "root"),
                default_save(Some(&rdp_id), ConnectionType::Rdp, "kiki"),
            ],
        )
        .unwrap();
    assert_eq!(same, id);
    let library = core.library();
    assert_eq!(
        entry(&library, &id).default_connection_id.as_deref(),
        Some(rdp_id.as_str())
    );
    for connection_id in [&ssh_id, &rdp_id] {
        assert_eq!(connection(&library, connection_id).name, "Renamed");
    }
    // The password is kept when not sent.
    assert!(connection(&library, &ssh_id).has_password);

    // Add SSH kiki, drop RDP: the default (RDP) is gone, so the first connection is the default.
    core.save_entry(
        Some(&id),
        &renamed,
        &[
            connection_save(Some(&ssh_id), ConnectionType::Ssh, "root"),
            connection_save(None, ConnectionType::Ssh, "kiki"),
        ],
    )
    .unwrap();
    let library = core.library();
    let edited = entry(&library, &id);
    assert_eq!(edited.connection_ids.len(), 2);
    assert_eq!(edited.connection_ids[0], ssh_id);
    assert_eq!(edited.default_connection_id.as_deref(), Some(ssh_id.as_str()));
    assert!(library.connections.iter().all(|c| c.id != rdp_id));
    let kiki = connection(&library, &edited.connection_ids[1]);
    assert_eq!((kiki.display_label.as_str(), kiki.sort_order), ("SSH · kiki", 1));
    assert_eq!(library.entries.len(), 1);
}

#[test]
fn save_entry_needs_a_connection() {
    let env = Env::new();
    let core = env.unlocked();
    let draft = entry_draft("Box", "box.example.com");
    assert_eq!(
        core.save_entry(None, &draft, &[]).unwrap_err().code,
        "noConnections"
    );
    assert!(core.library().entries.is_empty());

    let (id, connections) = entry_with(&core, &[ConnectionType::Ssh]);
    assert_eq!(
        core.save_entry(Some(&id), &draft, &[]).unwrap_err().code,
        "noConnections"
    );
    // Unknown entries and connections of other entries are refused.
    let one = [connection_save(None, ConnectionType::Ssh, "")];
    assert_eq!(
        core.save_entry(Some("missing"), &draft, &one).unwrap_err().code,
        "notFound"
    );
    let (other, _) = entry_with(&core, &[ConnectionType::Rdp]);
    let foreign = [connection_save(Some(&connections[0]), ConnectionType::Ssh, "")];
    assert_eq!(
        core.save_entry(Some(&other), &draft, &foreign).unwrap_err().code,
        "notFound"
    );
    let twice = [
        connection_save(Some(&connections[0]), ConnectionType::Ssh, ""),
        connection_save(Some(&connections[0]), ConnectionType::Ssh, ""),
    ];
    assert_eq!(
        core.save_entry(Some(&id), &draft, &twice).unwrap_err().code,
        "notFound"
    );
    assert_eq!(
        core.save_entry(None, &entry_draft(" ", "h"), &one)
            .unwrap_err()
            .code,
        "emptyName"
    );
    assert_eq!(
        core.save_entry(None, &entry_draft("n", " "), &one)
            .unwrap_err()
            .code,
        "emptyHost"
    );
    assert_eq!(core.library().entries.len(), 2);
}

#[test]
fn merge_moves_connections_and_appends_notes() {
    let env = Env::new();
    let core = env.unlocked();
    let mut target = entry_draft("T", "A");
    target.notes = "t".into();
    let target_id = core
        .save_entry(
            None,
            &target,
            &[connection_save(None, ConnectionType::Rdp, "admin")],
        )
        .unwrap();
    let mut source = entry_draft("S", "B");
    source.notes = "s".into();
    let mut ssh = connection_save(None, ConnectionType::Ssh, "root");
    ssh.password = Some("pw".into());
    let source_id = core.save_entry(None, &source, &[ssh]).unwrap();
    let source_ssh = entry(&core.library(), &source_id).connection_ids[0].clone();
    let forward = core
        .port_forward_save(&PortForwardDraft {
            id: None,
            connection_id: source_ssh.clone(),
            name: "web".into(),
            local_port: "18080".into(),
            remote_host: "localhost".into(),
            remote_port: "80".into(),
        })
        .unwrap();

    let result = core
        .merge_entries(&target_id, std::slice::from_ref(&source_id))
        .unwrap();
    assert_eq!(result.moved_connections, 1);
    let library = core.library();
    assert_eq!(library.entries.len(), 1);
    let merged = entry(&library, &target_id);
    assert_eq!(merged.notes, "t\n\nMerged from S (B):\ns");
    assert_eq!(merged.host, "A");
    assert_eq!(merged.connection_ids.len(), 2);
    assert_eq!(merged.connection_ids[1], source_ssh);
    let moved = connection(&library, &source_ssh);
    assert_eq!((moved.host.as_str(), moved.name.as_str()), ("A", "T"));
    assert!(moved.has_password);
    assert_eq!(core.password_for_copy(&source_ssh).unwrap().expose_secret(), "pw");
    // The forward still belongs to the moved connection.
    let kept = library.port_forwards.iter().find(|f| f.id == forward).unwrap();
    assert_eq!(kept.connection_id, source_ssh);

    // A source on another host without notes still records the host it replaced.
    let quiet = core
        .save_entry(
            None,
            &entry_draft("U", "C"),
            &[connection_save(None, ConnectionType::Web, "")],
        )
        .unwrap();
    let same_host = core
        .save_entry(
            None,
            &entry_draft("V", "a"),
            &[connection_save(None, ConnectionType::Ssh, "v")],
        )
        .unwrap();
    core.merge_entries(&target_id, &[quiet, same_host]).unwrap();
    let library = core.library();
    assert_eq!(
        entry(&library, &target_id).notes,
        "t\n\nMerged from S (B):\ns\n\nMerged from U (C):"
    );
    assert_eq!(entry(&library, &target_id).connection_ids.len(), 4);
    assert_eq!(library.entries.len(), 1);
}

#[test]
fn merge_refuses_self_and_missing() {
    let env = Env::new();
    let core = env.unlocked();
    let (a, _) = entry_with(&core, &[ConnectionType::Ssh]);
    let (b, _) = entry_with(&core, &[ConnectionType::Rdp]);
    let before = core.library();
    assert_eq!(
        core.merge_entries(&a, &[b.clone(), a.clone()]).unwrap_err().code,
        "mergeIntoSelf"
    );
    assert_eq!(
        core.merge_entries(&a, &[b.clone(), "missing".into()])
            .unwrap_err()
            .code,
        "notFound"
    );
    assert_eq!(core.merge_entries("missing", &[b]).unwrap_err().code, "notFound");
    assert_eq!(core.library(), before);
}

#[test]
fn duplicate_entry_copies_all_connections() {
    let env = Env::new();
    let core = env.unlocked();
    let mut draft = entry_draft("Box", "box.example.com");
    draft.is_favorite = true;
    let mut rdp = default_save(None, ConnectionType::Rdp, "kiki");
    rdp.draft.label = "Console".into();
    rdp.draft.show_as_button = false;
    let id = core
        .save_entry(
            None,
            &draft,
            &[connection_save(None, ConnectionType::Ssh, "root"), rdp],
        )
        .unwrap();
    let original = entry(&core.library(), &id).clone();

    let copy_id = core.duplicate_entry(&id, " (copy)").unwrap();
    assert_ne!(copy_id, id);
    assert_eq!(copy_id, copy_id.to_uppercase());
    let library = core.library();
    let copy = entry(&library, &copy_id);
    assert_eq!(copy.name, "Box (copy)");
    assert_eq!(copy.host, "box.example.com");
    assert!(copy.is_favorite);
    assert_eq!(copy.connection_ids.len(), 2);
    assert!(
        copy.connection_ids
            .iter()
            .all(|c| !original.connection_ids.contains(c))
    );
    let kinds: Vec<_> = copy
        .connection_ids
        .iter()
        .map(|c| connection(&library, c).kind)
        .collect();
    assert_eq!(kinds, [ConnectionType::Ssh, ConnectionType::Rdp]);
    assert_eq!(copy.default_connection_id.as_ref(), Some(&copy.connection_ids[1]));
    let copied_rdp = connection(&library, &copy.connection_ids[1]);
    assert_eq!(copied_rdp.name, "Box (copy)");
    assert_eq!(
        (copied_rdp.display_label.as_str(), copied_rdp.show_as_button),
        ("Console", false)
    );
    assert_eq!(
        connection(&library, &copy.connection_ids[0]).display_label,
        "SSH · root"
    );
    assert_eq!(entry(&library, &id).connection_ids, original.connection_ids);
    assert_eq!(
        core.duplicate_entry("missing", " (copy)").unwrap_err().code,
        "notFound"
    );
}

#[test]
fn search_finds_entries_by_connection_username() {
    let env = Env::new();
    let core = env.unlocked();
    let mut labelled = connection_save(None, ConnectionType::Rdp, "kiki");
    labelled.draft.label = "Console".into();
    let home = core
        .save_entry(
            None,
            &entry_draft("Home", "home.example.com"),
            &[connection_save(None, ConnectionType::Ssh, "root"), labelled],
        )
        .unwrap();
    let other = core
        .save_entry(
            None,
            &entry_draft("Other", "other.example.com"),
            &[connection_save(None, ConnectionType::Ssh, "admin")],
        )
        .unwrap();
    assert_eq!(core.search("kiki").entries, std::slice::from_ref(&home));
    assert_eq!(core.search("home root").entries, std::slice::from_ref(&home));
    assert_eq!(core.search("console").entries, std::slice::from_ref(&home));
    assert_eq!(core.search("admin").entries, std::slice::from_ref(&other));
    assert_eq!(core.search("example").entries, [home, other]);
    assert!(core.search("nobody").entries.is_empty());
}

#[test]
fn split_tag_picks_the_default_ssh_connection() {
    let env = Env::new();
    let core = env.unlocked();
    let (id, connections) = entry_with(
        &core,
        &[ConnectionType::Rdp, ConnectionType::Ssh, ConnectionType::Mikrotik],
    );
    let (rdp_only, _) = entry_with(&core, &[ConnectionType::Rdp]);
    let split = |tags: &[String]| {
        record(&core, |db| {
            db.library().unwrap().ssh_connection_ids_for_tags(tags)
        })
    };
    let tag = |prefix: &str, id: &str| vec![format!("{prefix}:{id}")];
    // The default is RDP: the first SSH connection stands in.
    assert_eq!(split(&tag("e", &id)), [connections[1].clone()]);
    assert!(split(&tag("e", &rdp_only)).is_empty());

    // An SSH-capable default is the one.
    let router = core
        .save_entry(
            None,
            &entry_draft("Router", "router.example.com"),
            &[
                connection_save(None, ConnectionType::Ssh, ""),
                default_save(None, ConnectionType::Mikrotik, ""),
            ],
        )
        .unwrap();
    let mikrotik = entry(&core.library(), &router).connection_ids[1].clone();
    assert_eq!(split(&tag("e", &router)), std::slice::from_ref(&mikrotik));
    assert_eq!(split(&tag("fav", &router)), std::slice::from_ref(&mikrotik));
    // A connection row is itself; an RDP one has no place in a split.
    assert_eq!(split(&tag("c", &connections[1])), [connections[1].clone()]);
    assert!(split(&tag("c", &connections[0])).is_empty());
}

#[test]
fn tags_act_on_entries() {
    let env = Env::new();
    let core = env.unlocked();
    let folder = core
        .save_folder(&FolderInput {
            name: "Prod".into(),
            ..FolderInput::default()
        })
        .unwrap();
    let (id, connections) = entry_with(&core, &[ConnectionType::Rdp, ConnectionType::Ssh]);
    let (other, other_connections) = entry_with(&core, &[ConnectionType::Ssh]);
    let tag = format!("e:{id}");

    // Move and favorite the whole entry.
    let moved = core
        .move_items(std::slice::from_ref(&tag), Some(&folder))
        .unwrap();
    assert_eq!(moved.moved, 1);
    core.set_favorite(&tag, true).unwrap();
    let library = core.library();
    assert_eq!(entry(&library, &id).folder_id.as_deref(), Some(folder.as_str()));
    assert_eq!(library.favorites, std::slice::from_ref(&id));
    for c in &connections {
        let c = connection(&library, c);
        assert!(c.is_favorite);
        assert_eq!(c.folder_id.as_deref(), Some(folder.as_str()));
    }
    // A connection's tag stars its entry.
    core.set_favorite(&format!("c:{}", other_connections[0]), true)
        .unwrap();
    assert!(entry(&core.library(), &other).is_favorite);
    core.set_favorite(&format!("fav:{other}"), false).unwrap();
    assert!(!entry(&core.library(), &other).is_favorite);
    assert_eq!(core.set_favorite("e:missing", true).unwrap_err().code, "notFound");

    // A credential links the entry's default connection; a folder links every connection inside.
    let credential = core
        .save_credential(
            None,
            &CredentialInput {
                name: "Admin".into(),
                ..CredentialInput::default()
            },
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        core.set_credential(Some(&credential), std::slice::from_ref(&tag), &[])
            .unwrap(),
        1
    );
    let library = core.library();
    assert_eq!(
        connection(&library, &connections[0]).credential_id.as_deref(),
        Some(credential.as_str())
    );
    assert_eq!(connection(&library, &connections[1]).credential_id, None);
    assert_eq!(
        core.set_credential(Some(&credential), &[format!("f:{folder}")], &[])
            .unwrap(),
        1
    );

    // Deleting the entry deletes its connections.
    let counts = core.delete_items(&[tag, format!("e:{id}")]).unwrap();
    assert_eq!((counts.entries, counts.connections), (1, 2));
    let library = core.library();
    assert_eq!(
        library.entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        std::slice::from_ref(&other)
    );
    assert!(library.connections.iter().all(|c| c.entry_id == other));
    // Deleting the last connection of an entry deletes the entry.
    let counts = core
        .delete_items(&[format!("c:{}", other_connections[0])])
        .unwrap();
    assert_eq!((counts.entries, counts.connections), (0, 1));
    assert!(core.library().entries.is_empty());
}

#[test]
fn entry_draft_round_trips_camel_case() {
    let parsed: EntryDraft =
        serde_json::from_str(r#"{"name":"n","host":"h","folderId":"F","isFavorite":true}"#).unwrap();
    assert_eq!(parsed.folder_id.as_deref(), Some("F"));
    assert!(parsed.is_favorite);
    let save: ConnectionSave = serde_json::from_str(
        r#"{"id":null,"draft":{"type":"ssh","label":"L","showAsButton":false},"password":"pw"}"#,
    )
    .unwrap();
    assert_eq!(save.draft.label, "L");
    assert!(!save.draft.show_as_button);
    assert_eq!(save.password.as_deref(), Some("pw"));
    assert!(!save.is_default);
    let flagged: ConnectionSave =
        serde_json::from_str(r#"{"draft":{"type":"rdp"},"isDefault":true}"#).unwrap();
    assert!(flagged.is_default);
}

#[test]
fn new_connection_can_be_the_default() {
    let env = Env::new();
    let core = env.unlocked();
    let id = core
        .save_entry(
            None,
            &entry_draft("Box", "box.example.com"),
            &[
                connection_save(None, ConnectionType::Ssh, "root"),
                default_save(None, ConnectionType::Rdp, "kiki"),
            ],
        )
        .unwrap();
    let created = entry(&core.library(), &id).clone();
    assert_eq!(
        created.default_connection_id.as_ref(),
        Some(&created.connection_ids[1])
    );

    // Added in an edit and flagged: the new one wins over the stored default.
    let ssh = created.connection_ids[0].clone();
    let rdp = created.connection_ids[1].clone();
    core.save_entry(
        Some(&id),
        &entry_draft("Box", "box.example.com"),
        &[
            connection_save(Some(&ssh), ConnectionType::Ssh, "root"),
            connection_save(Some(&rdp), ConnectionType::Rdp, "kiki"),
            default_save(None, ConnectionType::Web, ""),
        ],
    )
    .unwrap();
    let edited = entry(&core.library(), &id).clone();
    assert_eq!(edited.connection_ids.len(), 3);
    assert_eq!(
        edited.default_connection_id.as_ref(),
        Some(&edited.connection_ids[2])
    );

    // Nothing flagged: the stored default stays.
    core.save_entry(
        Some(&id),
        &entry_draft("Box", "box.example.com"),
        &[
            connection_save(Some(&edited.connection_ids[2]), ConnectionType::Web, ""),
            connection_save(Some(&ssh), ConnectionType::Ssh, "root"),
        ],
    )
    .unwrap();
    let kept = entry(&core.library(), &id).clone();
    assert_eq!(kept.connection_ids[0], edited.connection_ids[2]);
    assert_eq!(
        kept.default_connection_id.as_ref(),
        Some(&edited.connection_ids[2])
    );
    let ordered = kept.connection_ids.clone();
    core.save_entry(
        Some(&id),
        &entry_draft("Box", "box.example.com"),
        &[
            connection_save(Some(&ordered[1]), ConnectionType::Ssh, "root"),
            connection_save(Some(&ordered[0]), ConnectionType::Web, ""),
        ],
    )
    .unwrap();
    assert_eq!(
        entry(&core.library(), &id).default_connection_id.as_ref(),
        Some(&ordered[0])
    );
}

#[test]
fn multiple_defaults_are_refused() {
    let env = Env::new();
    let core = env.unlocked();
    let (id, connections) = entry_with(&core, &[ConnectionType::Ssh, ConnectionType::Rdp]);
    let before = core.library();
    let both = [
        default_save(Some(&connections[0]), ConnectionType::Ssh, "changed"),
        default_save(Some(&connections[1]), ConnectionType::Rdp, ""),
    ];
    assert_eq!(
        core.save_entry(Some(&id), &entry_draft("Entry", "h"), &both)
            .unwrap_err()
            .code,
        "multipleDefaults"
    );
    let new = [
        default_save(None, ConnectionType::Ssh, ""),
        default_save(None, ConnectionType::Rdp, ""),
    ];
    assert_eq!(
        core.save_entry(None, &entry_draft("New", "h"), &new)
            .unwrap_err()
            .code,
        "multipleDefaults"
    );
    assert_eq!(core.library(), before);
}

#[test]
fn failed_save_writes_nothing() {
    let env = Env::new();
    let core = env.unlocked();
    let (id, connections) = entry_with(&core, &[ConnectionType::Ssh, ConnectionType::Rdp]);
    let before = core.library();
    // The credential is unknown: the foreign key fails inside the transaction, after the entry was
    // written and the RDP connection deleted.
    let mut linked = connection_save(None, ConnectionType::Web, "");
    linked.draft.credential_id = Some("missing-credential".into());
    let saves = [
        connection_save(Some(&connections[0]), ConnectionType::Ssh, "changed"),
        linked,
    ];
    assert_eq!(
        core.save_entry(Some(&id), &entry_draft("Renamed", "h"), &saves)
            .unwrap_err()
            .code,
        "database"
    );
    // An unknown folder fails the same way.
    let mut moved = entry_draft("Renamed", "h");
    moved.folder_id = Some("missing-folder".into());
    assert_eq!(
        core.save_entry(
            Some(&id),
            &moved,
            &[connection_save(Some(&connections[0]), ConnectionType::Ssh, "")]
        )
        .unwrap_err()
        .code,
        "database"
    );
    assert_eq!(core.library(), before);
}

#[test]
fn merge_with_no_sources_changes_nothing() {
    let env = Env::new();
    let core = env.unlocked();
    let (id, _) = entry_with(&core, &[ConnectionType::Ssh]);
    let before = core.library();
    let result = core.merge_entries(&id, &[]).unwrap();
    assert_eq!(result.moved_connections, 0);
    assert_eq!(core.library(), before);
    assert_eq!(core.merge_entries("missing", &[]).unwrap_err().code, "notFound");
}
