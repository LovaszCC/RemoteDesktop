//! Ports of the Swift `ConnectionRepository`, `Credential` and `CredentialLink` tests.

use rd_core::{
    Connection, ConnectionType, Credential, Folder, LabelColor, MikrotikMode, PortForward, RdpClient,
    SavedSplit, Sealed, SshTerminal, Timestamp,
};
use rd_store::{Database, StoreError};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn folder(name: &str, parent: Option<&Folder>) -> Folder {
    let mut folder = Folder::new(name);
    folder.parent_id = parent.map(|p| p.id.clone());
    folder
}

fn ssh(name: &str) -> Connection {
    Connection::new(
        ConnectionType::Ssh,
        name,
        format!("{}.example.com", name.to_lowercase()),
    )
}

#[test]
fn saves_and_loads_every_field() {
    let db = db();
    let parent = folder("Clients", None);
    db.save_folder(&parent).unwrap();
    let mut credential = Credential::new("Admin");
    credential.username = "administrator".into();
    credential.ssh_key_unlocked_enc = Some(Sealed::from_bytes(vec![9; 40]));
    let credential = db.save_credential(&credential).unwrap();

    let mut connection = Connection::new(ConnectionType::Mikrotik, "Router", "192.0.2.1");
    connection.folder_id = Some(parent.id.clone());
    connection.credential_id = Some(credential.id.clone());
    connection.port = Some(8291);
    connection.username = "admin".into();
    connection.domain = "D".into();
    connection.password_enc = Some(Sealed::from_bytes(vec![1; 30]));
    connection.ssh_key_path = Some("~/.ssh/id".into());
    connection.ssh_key_passphrase_enc = Some(Sealed::from_bytes(vec![2; 30]));
    connection.is_favorite = true;
    connection.rdp_admin_session = true;
    connection.browser_bundle_id = Some("org.example.browser".into());
    connection.mikrotik_mode = MikrotikMode::Ssh;
    connection.ssh_terminal = SshTerminal::External;
    connection.rdp_client = RdpClient::WindowsApp;
    connection.color = Some(LabelColor::Pink);
    connection.notes = "line 1\nline 2".into();
    let saved = db.save_connection(&connection).unwrap();
    assert!(saved.updated_at >= connection.updated_at);

    let library = db.library().unwrap();
    assert_eq!(library.folders, vec![parent]);
    assert_eq!(library.credentials, vec![credential]);
    assert_eq!(library.connections, vec![saved.clone()]);
    assert_eq!(db.connection(&saved.id).unwrap(), Some(saved));
    assert_eq!(db.connection("missing").unwrap(), None);
}

#[test]
fn updates_existing_connection_and_keeps_its_forwards() {
    let db = db();
    let mut connection = db.save_connection(&ssh("Web")).unwrap();
    let forward = PortForward {
        id: rd_core::new_id(),
        connection_id: connection.id.clone(),
        name: "db".into(),
        local_port: 15432,
        remote_host: "127.0.0.1".into(),
        remote_port: 5432,
        created_at: Timestamp::now(),
    };
    db.save_port_forward(&forward).unwrap();

    connection.name = "Web renamed".into();
    db.save_connection(&connection).unwrap();
    let library = db.library().unwrap();
    assert_eq!(library.connections.len(), 1);
    assert_eq!(library.connections[0].name, "Web renamed");
    assert_eq!(library.port_forwards, vec![forward]);
}

#[test]
fn deleting_folder_cascades_subfolders_and_orphans_connections() {
    let db = db();
    let root = folder("Root", None);
    let child = folder("Child", Some(&root));
    db.save_folder(&root).unwrap();
    db.save_folder(&child).unwrap();
    let mut inside = ssh("Inside");
    inside.folder_id = Some(child.id.clone());
    let inside = db.save_connection(&inside).unwrap();

    db.delete_folder(&root.id).unwrap();
    let library = db.library().unwrap();
    assert!(library.folders.is_empty());
    assert_eq!(library.connections[0].id, inside.id);
    assert_eq!(library.connections[0].folder_id, None);
}

#[test]
fn move_folder_rejects_cycles_and_missing_folders() {
    let db = db();
    let a = folder("A", None);
    let b = folder("B", Some(&a));
    let c = folder("C", Some(&b));
    for f in [&a, &b, &c] {
        db.save_folder(f).unwrap();
    }
    assert!(matches!(
        db.move_folder(&a.id, Some(&c.id)),
        Err(StoreError::FolderCycle)
    ));
    assert!(matches!(
        db.move_folder(&a.id, Some(&a.id)),
        Err(StoreError::FolderCycle)
    ));
    assert!(matches!(
        db.move_folder("missing", None),
        Err(StoreError::NotFound)
    ));

    db.move_folder(&c.id, None).unwrap();
    let library = db.library().unwrap();
    assert_eq!(
        library.folders.iter().find(|f| f.id == c.id).unwrap().parent_id,
        None
    );
}

#[test]
fn moves_connection_and_toggles_favorite() {
    let db = db();
    let target = folder("Target", None);
    db.save_folder(&target).unwrap();
    let connection = db.save_connection(&ssh("X")).unwrap();

    db.move_connection(&connection.id, Some(&target.id)).unwrap();
    let moved = db.connection(&connection.id).unwrap().unwrap();
    assert_eq!(moved.folder_id.as_deref(), Some(target.id.as_str()));

    db.set_favorite(&connection.id, true).unwrap();
    let starred = db.connection(&connection.id).unwrap().unwrap();
    assert!(starred.is_favorite);
    assert_eq!(starred.updated_at, moved.updated_at);

    assert!(matches!(
        db.move_connection("missing", None),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        db.set_favorite("missing", true),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn deletes_connection() {
    let db = db();
    let connection = db.save_connection(&ssh("Gone")).unwrap();
    db.delete_connection(&connection.id).unwrap();
    assert!(db.library().unwrap().connections.is_empty());
}

#[test]
fn search_matches_name_host_user_and_notes() {
    let db = db();
    let mut by_user = ssh("Alpha");
    by_user.username = "deployer".into();
    let mut by_notes = ssh("Beta");
    by_notes.notes = "runs the Billing stack".into();
    for c in [&by_user, &by_notes, &ssh("Gamma")] {
        db.save_connection(c).unwrap();
    }
    let names = |q: &str| {
        db.search(q)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("alp"), ["Alpha"]);
    assert_eq!(names("GAMMA.example"), ["Gamma"]);
    assert_eq!(names("deploy"), ["Alpha"]);
    assert_eq!(names("billing"), ["Beta"]);
    assert_eq!(names("  "), ["Alpha", "Beta", "Gamma"]);
    assert!(names("nothing").is_empty());
}

#[test]
fn library_uses_swift_sort_orders() {
    let db = db();
    let mut second = folder("a-second", None);
    second.sort_order = 1;
    let mut first_b = folder("B", None);
    first_b.sort_order = 0;
    let mut first_a = folder("a", None);
    first_a.sort_order = 0;
    for f in [&second, &first_b, &first_a] {
        db.save_folder(f).unwrap();
    }
    for name in ["beta", "Alpha", "gamma"] {
        db.save_connection(&ssh(name)).unwrap();
    }
    let library = db.library().unwrap();
    assert_eq!(
        library
            .folders
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "B", "a-second"]
    );
    assert_eq!(
        library
            .connections
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "beta", "gamma"]
    );
}

#[test]
fn credential_crud_and_unlink_on_delete() {
    let db = db();
    let mut credential = Credential::new("Ops");
    credential.username = "ops".into();
    let credential = db.save_credential(&credential).unwrap();
    let mut linked = ssh("Linked");
    linked.credential_id = Some(credential.id.clone());
    let linked = db.save_connection(&linked).unwrap();

    let mut renamed = credential.clone();
    renamed.name = "Ops team".into();
    db.save_credential(&renamed).unwrap();
    assert_eq!(db.credential(&credential.id).unwrap().unwrap().name, "Ops team");

    db.delete_credential(&credential.id).unwrap();
    assert_eq!(db.credential(&credential.id).unwrap(), None);
    assert_eq!(db.connection(&linked.id).unwrap().unwrap().credential_id, None);
}

#[test]
fn links_and_unlinks_connections() {
    let db = db();
    let credential = db.save_credential(&Credential::new("Shared")).unwrap();
    let a = db.save_connection(&ssh("A")).unwrap();
    let b = db.save_connection(&ssh("B")).unwrap();
    let ids = vec![a.id.clone(), b.id.clone(), "missing".to_owned()];

    assert_eq!(db.set_credential(Some(&credential.id), &ids).unwrap(), 2);
    assert_eq!(db.set_credential(Some(&credential.id), &ids).unwrap(), 0);
    let library = db.library().unwrap();
    assert_eq!(library.connections_using(&credential.id).count(), 2);

    assert_eq!(db.set_credential(None, std::slice::from_ref(&a.id)).unwrap(), 1);
    assert_eq!(db.connection(&a.id).unwrap().unwrap().credential_id, None);
}

#[test]
fn relinking_moves_connection_to_other_credential() {
    let db = db();
    let first = db.save_credential(&Credential::new("First")).unwrap();
    let second = db.save_credential(&Credential::new("Second")).unwrap();
    let connection = db.save_connection(&ssh("C")).unwrap();
    db.set_credential(Some(&first.id), std::slice::from_ref(&connection.id))
        .unwrap();
    assert_eq!(
        db.set_credential(Some(&second.id), std::slice::from_ref(&connection.id))
            .unwrap(),
        1
    );
    let library = db.library().unwrap();
    assert_eq!(library.connections_using(&first.id).count(), 0);
    assert_eq!(library.connections_using(&second.id).count(), 1);
}

#[test]
fn linking_an_unknown_credential_changes_nothing() {
    let db = db();
    let a = db.save_connection(&ssh("A")).unwrap();
    assert!(
        db.set_credential(Some("missing"), std::slice::from_ref(&a.id))
            .is_err()
    );
    assert_eq!(db.connection(&a.id).unwrap().unwrap().credential_id, None);
}

#[test]
fn saved_splits_append_reorder_and_keep_duplicates() {
    let db = db();
    let a = db.save_connection(&ssh("A")).unwrap();
    let split = |name: &str| SavedSplit {
        id: rd_core::new_id(),
        name: name.into(),
        connection_ids: vec![a.id.clone(), a.id.clone()],
        sort_order: 99,
        created_at: Timestamp::now(),
    };
    let first = db.save_saved_split(&split("First")).unwrap();
    let second = db.save_saved_split(&split("Second")).unwrap();
    assert_eq!((first.sort_order, second.sort_order), (0, 1));

    let mut renamed = first.clone();
    renamed.name = "First renamed".into();
    assert_eq!(db.save_saved_split(&renamed).unwrap().sort_order, 0);

    db.reorder_saved_splits(&[second.id.clone(), "missing".into(), first.id.clone()])
        .unwrap();
    let library = db.library().unwrap();
    let names: Vec<_> = library.saved_splits.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Second", "First renamed"]);
    assert_eq!(
        library.saved_splits[0].connection_ids,
        [a.id.clone(), a.id.clone()]
    );

    db.delete_saved_split(&second.id).unwrap();
    assert_eq!(db.library().unwrap().saved_splits.len(), 1);
}

#[test]
fn port_forwards_sort_by_local_port_and_cascade() {
    let db = db();
    let connection = db.save_connection(&ssh("Tunnel")).unwrap();
    let forward = |port: u16| PortForward {
        id: rd_core::new_id(),
        connection_id: connection.id.clone(),
        name: String::new(),
        local_port: port,
        remote_host: "localhost".into(),
        remote_port: 80,
        created_at: Timestamp::now(),
    };
    let high = forward(9000);
    let low = forward(8000);
    db.save_port_forward(&high).unwrap();
    db.save_port_forward(&low).unwrap();
    assert_eq!(db.library().unwrap().port_forwards, [low.clone(), high]);

    db.delete_port_forward(&low.id).unwrap();
    assert_eq!(db.library().unwrap().port_forwards.len(), 1);
    db.delete_connection(&connection.id).unwrap();
    assert!(db.library().unwrap().port_forwards.is_empty());
}

#[test]
fn transaction_rolls_back_on_error() {
    let db = db();
    let result: Result<(), StoreError> = db.transaction(|tx| {
        tx.execute("INSERT INTO meta(key, value) VALUES ('k', x'01')", [])?;
        Err(StoreError::NotFound)
    });
    assert!(result.is_err());
    assert_eq!(db.meta("k").unwrap(), None);
}

#[test]
fn unreadable_values_are_reported() {
    let db = db();
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO connection (id, type, name, host, createdAt, updatedAt) VALUES ('X', 'telnet', 'x', 'h', 't', 't')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(db.library().is_err());
}
