//! The import writer (Swift `ImportWriter`): folders by group path, credentials, connections and their
//! sealed passwords, all in one transaction.

use std::collections::HashMap;

use rd_core::import::{ImportedConnection, ImportedCredential};
use rd_core::{Connection, Credential, Folder};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::repository::{upsert_connection, upsert_credential};
use crate::rows::FOLDER_COLUMNS;
use crate::{Database, StoreError, Vault};

/// What to write.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportBatch<'a> {
    /// Connections to create.
    pub connections: &'a [ImportedConnection],
    /// Credentials to create; connections link to them by [`ImportedCredential::reference`].
    pub credentials: &'a [ImportedCredential],
    /// Group paths created even when no connection lands in them.
    pub empty_groups: &'a [Vec<String>],
    /// Where the group paths start; `None` is the root.
    pub parent_folder_id: Option<&'a str>,
    /// Export references resolved to credentials already in the library, for linked entries that are not
    /// imported again.
    pub existing_credentials: Option<&'a HashMap<String, String>>,
}

/// What the writer created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    /// Connections written.
    pub connections: usize,
    /// Credentials written.
    pub credentials: usize,
    /// Folders created (existing ones are reused).
    pub folders_created: usize,
}

/// Group path → folder id, creating the missing folders.
struct Folders<'a, 'tx> {
    tx: &'a Transaction<'tx>,
    parent: Option<&'a str>,
    cache: HashMap<Vec<String>, String>,
    created: usize,
}

impl Folders<'_, '_> {
    fn resolve(&mut self, path: &[String]) -> Result<Option<String>, StoreError> {
        let mut parent = self.parent.map(str::to_owned);
        for depth in 0..path.len() {
            let key = &path[..=depth];
            if let Some(id) = self.cache.get(key) {
                parent = Some(id.clone());
                continue;
            }
            let name = &path[depth];
            let existing: Option<String> = self
                .tx
                .query_row(
                    "SELECT id FROM folder WHERE name = ?1 AND parentId IS ?2 ORDER BY sortOrder, id LIMIT 1",
                    params![name, parent],
                    |r| r.get(0),
                )
                .optional()?;
            let id = if let Some(id) = existing {
                id
            } else {
                let mut folder = Folder::new(name.clone());
                folder.parent_id.clone_from(&parent);
                self.tx.execute(
                    &format!("INSERT INTO folder ({FOLDER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, NULL)"),
                    params![folder.id, folder.parent_id, folder.name, folder.sort_order],
                )?;
                self.created += 1;
                folder.id
            };
            self.cache.insert(key.to_vec(), id.clone());
            parent = Some(id);
        }
        Ok(parent)
    }
}

impl Database {
    /// Writes an import in one transaction: nothing is written when any part fails. Folders of the same
    /// name that already exist under the same parent are reused.
    ///
    /// # Errors
    /// If SQLite fails, for example for an unknown `parent_folder_id`.
    pub fn import(&self, batch: &ImportBatch<'_>, vault: &Vault) -> Result<ImportCounts, StoreError> {
        // Seal first (as Swift does), outside the lock.
        let credentials: Vec<(&str, Credential)> = batch
            .credentials
            .iter()
            .map(|item| {
                let mut credential = Credential::new(item.name.clone());
                credential.username.clone_from(&item.username);
                credential.domain.clone_from(&item.domain);
                credential.password_enc = vault.seal_optional(item.password.as_deref());
                credential.ssh_key_path.clone_from(&item.ssh_key_path);
                (item.reference.as_str(), credential)
            })
            .collect();
        let mut links: HashMap<&str, &str> = batch
            .existing_credentials
            .map(|m| m.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect())
            .unwrap_or_default();
        for (reference, credential) in &credentials {
            links.insert(reference, &credential.id);
        }
        let connections: Vec<(&ImportedConnection, Connection)> = batch
            .connections
            .iter()
            .map(|item| {
                let mut connection = Connection::new(item.kind, item.name.clone(), item.host.clone());
                connection.credential_id = item
                    .credential_ref
                    .as_deref()
                    .and_then(|r| links.get(r))
                    .map(|id| (*id).to_owned());
                connection.port = item.port;
                connection.username.clone_from(&item.username);
                connection.domain.clone_from(&item.domain);
                connection.password_enc = vault.seal_optional(item.password.as_deref());
                connection.ssh_key_path.clone_from(&item.ssh_key_path);
                connection.rdp_admin_session = item.rdp_admin_session;
                connection.notes.clone_from(&item.notes);
                (item, connection)
            })
            .collect();

        self.transaction(|tx| {
            let mut folders = Folders {
                tx,
                parent: batch.parent_folder_id,
                cache: HashMap::new(),
                created: 0,
            };
            for (_, credential) in &credentials {
                upsert_credential(tx, credential)?;
            }
            for group in batch.empty_groups {
                folders.resolve(group)?;
            }
            for (item, connection) in &connections {
                let mut connection = connection.clone();
                connection.folder_id = folders.resolve(&item.group_path)?;
                upsert_connection(tx, &connection)?;
            }
            Ok(ImportCounts {
                connections: connections.len(),
                credentials: credentials.len(),
                folders_created: folders.created,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use rd_core::ConnectionType;
    use rd_core::import::{ImportKind, parse};
    use secrecy::ExposeSecret;

    use super::*;

    const RDM_SAMPLE: &str = include_str!("../../rd-core/tests/fixtures/rdm-sample.rdm");

    fn setup() -> (Database, Vault) {
        let db = Database::open_in_memory().unwrap();
        let vault = Vault::create(&db, "pw", 1_000).unwrap();
        (db, vault)
    }

    fn winbox(name: &str, host: &str, password: Option<&str>) -> ImportedConnection {
        let mut item = ImportedConnection::new(ConnectionType::Mikrotik, name, host);
        item.group_path = vec!["Branch East".into()];
        item.username = "admin".into();
        item.password = password.map(Into::into);
        item
    }

    fn open(vault: &Vault, sealed: Option<&rd_core::Sealed>) -> Option<String> {
        vault
            .open_optional(sealed)
            .unwrap()
            .map(|s| s.expose_secret().to_owned())
    }

    /// WinboxImporterTests.writesUnderTargetWithGroupFoldersAndDetectsDuplicates.
    #[test]
    fn writes_under_target_with_group_folders_and_detects_duplicates() {
        let (db, vault) = setup();
        let target = Folder::new("MikroTik");
        db.save_folder(&target).unwrap();
        let items = [
            winbox("core-rtr", "192.0.2.1", Some("s3cret")),
            winbox("edge", "192.0.2.2", None),
        ];
        let batch = ImportBatch {
            connections: &items,
            parent_folder_id: Some(&target.id),
            ..ImportBatch::default()
        };

        let first = db.import(&batch, &vault).unwrap();
        assert_eq!(first.connections, 2);
        assert_eq!(first.folders_created, 1);
        let library = db.library().unwrap();
        let group = library.folders.iter().find(|f| f.name == "Branch East").unwrap();
        assert_eq!(group.parent_id.as_deref(), Some(target.id.as_str()));
        let imported: Vec<_> = library
            .connections
            .iter()
            .filter(|c| c.folder_id.as_deref() == Some(group.id.as_str()))
            .collect();
        assert_eq!(imported.len(), 2);
        let core = imported.iter().find(|c| c.name == "core-rtr").unwrap();
        assert_eq!(
            open(&vault, core.password_enc.as_ref()).as_deref(),
            Some("s3cret")
        );
        assert_eq!(core.kind, ConnectionType::Mikrotik);
        assert_eq!(
            imported.iter().find(|c| c.name == "edge").unwrap().password_enc,
            None
        );

        let parsed = rd_core::import::ParsedImport {
            connections: items.to_vec(),
            ..Default::default()
        };
        assert_eq!(
            parsed.duplicates(&library, Some(&target.id)).connections,
            [true, true]
        );
        assert_eq!(parsed.duplicates(&library, None).connections, [false, false]);

        // Importing again reuses the group folder.
        let second = db
            .import(
                &ImportBatch {
                    connections: &items[..1],
                    ..batch
                },
                &vault,
            )
            .unwrap();
        assert_eq!(second.folders_created, 0);
        assert_eq!(db.library().unwrap().folders.len(), 2);
    }

    /// RDMImporterTests.importsIntoDatabaseReusingFolders.
    #[test]
    fn imports_rdm_reusing_folders() {
        let (db, vault) = setup();
        let existing = Folder::new("Customers");
        db.save_folder(&existing).unwrap();
        let parsed = parse(ImportKind::Rdm, RDM_SAMPLE.as_bytes()).unwrap();

        let counts = db
            .import(
                &ImportBatch {
                    connections: &parsed.connections,
                    credentials: &parsed.credentials,
                    empty_groups: &parsed.empty_groups,
                    ..ImportBatch::default()
                },
                &vault,
            )
            .unwrap();

        assert_eq!(counts.connections, 3);
        assert_eq!(counts.folders_created, 2, "Empty + Example; Customers reused");
        let library = db.library().unwrap();
        let example = library.folders.iter().find(|f| f.name == "Example").unwrap();
        assert_eq!(example.parent_id.as_deref(), Some(existing.id.as_str()));
        assert!(library.folders.iter().any(|f| f.name == "Empty"));
        let dc = library
            .connections
            .iter()
            .find(|c| c.name == "example-dc01")
            .unwrap();
        assert_eq!(dc.folder_id.as_deref(), Some(example.id.as_str()));
        assert_eq!(dc.port, Some(3390));
        assert_eq!(
            (dc.username.as_str(), dc.domain.as_str()),
            ("administrator", "EXAMPLE")
        );
        assert_eq!(open(&vault, dc.password_enc.as_ref()).as_deref(), Some("Secret1"));
        let web = library
            .connections
            .iter()
            .find(|c| c.name == "example-web")
            .unwrap();
        assert_eq!(web.ssh_key_path.as_deref(), Some("~/.ssh/example"));
        assert_eq!(
            library
                .connections
                .iter()
                .find(|c| c.name == "rootbox")
                .unwrap()
                .folder_id,
            None
        );
    }

    #[test]
    fn credentials_link_to_new_or_existing_entries() {
        let (db, vault) = setup();
        let mut known = Credential::new("known");
        known.username = "ops".into();
        let known = db.save_credential(&known).unwrap();
        let new_credential = ImportedCredential {
            reference: "NEW".into(),
            name: "Shared".into(),
            username: "admin".into(),
            domain: "EXAMPLE".into(),
            password: Some("pw1".into()),
            ssh_key_path: None,
        };
        let mut to_new = ImportedConnection::new(ConnectionType::Rdp, "a", "a.example.com");
        to_new.credential_ref = Some("NEW".into());
        let mut to_known = ImportedConnection::new(ConnectionType::Rdp, "b", "b.example.com");
        to_known.credential_ref = Some("OLD".into());
        let mut dangling = ImportedConnection::new(ConnectionType::Rdp, "c", "c.example.com");
        dangling.credential_ref = Some("GONE".into());
        let existing = HashMap::from([("OLD".to_owned(), known.id.clone())]);

        let counts = db
            .import(
                &ImportBatch {
                    connections: &[to_new, to_known, dangling],
                    credentials: std::slice::from_ref(&new_credential),
                    existing_credentials: Some(&existing),
                    ..ImportBatch::default()
                },
                &vault,
            )
            .unwrap();
        assert_eq!(counts.credentials, 1);
        let library = db.library().unwrap();
        let shared = library.credentials.iter().find(|c| c.name == "Shared").unwrap();
        assert_eq!(open(&vault, shared.password_enc.as_ref()).as_deref(), Some("pw1"));
        let by_name = |n: &str| library.connections.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by_name("a").credential_id.as_deref(), Some(shared.id.as_str()));
        assert_eq!(by_name("b").credential_id.as_deref(), Some(known.id.as_str()));
        assert_eq!(by_name("c").credential_id, None);
    }

    #[test]
    fn a_failure_writes_nothing() {
        let (db, vault) = setup();
        let credential = ImportedCredential {
            reference: "R".into(),
            name: "c".into(),
            username: String::new(),
            domain: String::new(),
            password: None,
            ssh_key_path: None,
        };
        let items = [winbox("x", "192.0.2.3", Some("pw"))];
        let result = db.import(
            &ImportBatch {
                connections: &items,
                credentials: std::slice::from_ref(&credential),
                parent_folder_id: Some("NO-SUCH-FOLDER"),
                ..ImportBatch::default()
            },
            &vault,
        );
        assert!(result.is_err());
        let library = db.library().unwrap();
        assert!(library.folders.is_empty());
        assert!(library.credentials.is_empty());
        assert!(library.connections.is_empty());
    }

    #[test]
    fn same_path_in_one_batch_creates_each_folder_once() {
        let (db, vault) = setup();
        let mut a = winbox("a", "192.0.2.4", None);
        a.group_path = vec!["X".into(), "Y".into()];
        let mut b = a.clone();
        b.name = "b".into();
        let groups = [vec!["X".into()], vec!["X".into(), "Z".into()]];
        let counts = db
            .import(
                &ImportBatch {
                    connections: &[a, b],
                    empty_groups: &groups,
                    ..ImportBatch::default()
                },
                &vault,
            )
            .unwrap();
        assert_eq!(counts.folders_created, 3);
        assert_eq!(db.library().unwrap().folders.len(), 3);
    }
}
