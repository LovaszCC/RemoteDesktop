//! The export file (a JSON envelope, optionally encrypted with its own password) and the writer that imports
//! one into the library.
//!
//! Encrypted envelopes use the vault's primitives with their own salt: PBKDF2-HMAC-SHA256 → AES-256-GCM,
//! `nonce(12) ‖ ciphertext ‖ tag(16)`, with [`EXPORT_AAD`] as associated data so a vault seal can't stand in
//! for an export.

use std::collections::{HashMap, HashSet};

use aes_gcm::aead::{Aead, Generate, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rd_core::transfer::{ExportPayload, ExportedConnection, ExportedEntry, TransferConflicts, conflicts};
use rd_core::{Connection, Credential, Entry, Library, PortForward, Timestamp, new_id};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::import::Folders;
use crate::repository::{upsert_connection, upsert_credential, upsert_entry};
use crate::rows::PORT_FORWARD_COLUMNS;
use crate::vault::{derive_key, random_salt};
use crate::{Database, StoreError, Vault};

/// The envelope's `format`.
pub const EXPORT_FORMAT: &str = "remotedesktop-export";
/// The envelope version this build writes and reads.
pub const EXPORT_VERSION: u32 = 1;
/// Associated data of the encrypted payload.
pub const EXPORT_AAD: &[u8] = b"remotedesktop-export-v1";
/// File extension of exports.
pub const EXPORT_EXTENSION: &str = "rdexport";

const KDF_NAME: &str = "pbkdf2-sha256";
const CIPHER_NAME: &str = "aes-256-gcm";
const NONCE_LEN: usize = 12;
/// Refuses files that would make the key derivation hang.
const MAX_ITERATIONS: u32 = 10_000_000;

/// Why an export can't be written or read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransferError {
    /// Not an export of this app, or a damaged one.
    #[error("this isn't a RemoteDesktop export or it is damaged")]
    Unreadable,
    /// Written by a newer version of the app.
    #[error("this export was made by a newer version of the app (format {0})")]
    UnsupportedVersion(u32),
    /// The file is encrypted and no password was given.
    #[error("the export is encrypted")]
    NeedsPassword,
    /// The password doesn't open the file.
    #[error("wrong password")]
    WrongPassword,
    /// An empty password was given for encryption.
    #[error("the password is empty")]
    EmptyPassword,
    /// A payload with passwords can only be written encrypted.
    #[error("passwords can only be exported with a password")]
    PlainWithPasswords,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    format: String,
    version: u32,
    encrypted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload: Option<ExportPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kdf: Option<Kdf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cipher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Kdf {
    name: String,
    iterations: u32,
    salt: String,
}

fn cipher(password: &str, salt: &[u8], iterations: u32) -> Aes256Gcm {
    let key = derive_key(password, salt, iterations);
    Aes256Gcm::new((&*key).into())
}

/// The export file's text. With a password the payload is encrypted; without one it is written as is and
/// must not hold passwords.
///
/// # Errors
/// [`TransferError::EmptyPassword`] or [`TransferError::PlainWithPasswords`].
///
/// # Panics
/// Never: AES-GCM sealing can't fail.
pub fn write_export(
    payload: &ExportPayload,
    password: Option<&str>,
    iterations: u32,
) -> Result<String, TransferError> {
    let envelope = match password {
        None if payload.has_passwords() => return Err(TransferError::PlainWithPasswords),
        None => Envelope {
            format: EXPORT_FORMAT.to_owned(),
            version: EXPORT_VERSION,
            encrypted: false,
            payload: Some(payload.clone()),
            kdf: None,
            cipher: None,
            data: None,
        },
        Some("") => return Err(TransferError::EmptyPassword),
        Some(password) => {
            let plain = Zeroizing::new(serde_json::to_vec(payload).map_err(|_| TransferError::Unreadable)?);
            let salt = random_salt();
            let nonce = Nonce::generate();
            let sealed = cipher(password, &salt, iterations)
                .encrypt(
                    &nonce,
                    Payload {
                        msg: &plain,
                        aad: EXPORT_AAD,
                    },
                )
                .expect("AES-GCM sealing cannot fail");
            let mut data = Vec::with_capacity(NONCE_LEN + sealed.len());
            data.extend_from_slice(&nonce);
            data.extend_from_slice(&sealed);
            Envelope {
                format: EXPORT_FORMAT.to_owned(),
                version: EXPORT_VERSION,
                encrypted: true,
                payload: None,
                kdf: Some(Kdf {
                    name: KDF_NAME.to_owned(),
                    iterations,
                    salt: B64.encode(salt),
                }),
                cipher: Some(CIPHER_NAME.to_owned()),
                data: Some(B64.encode(data)),
            }
        }
    };
    serde_json::to_string_pretty(&envelope).map_err(|_| TransferError::Unreadable)
}

fn envelope(data: &[u8]) -> Result<Envelope, TransferError> {
    let envelope: Envelope = serde_json::from_slice(data).map_err(|_| TransferError::Unreadable)?;
    if envelope.format != EXPORT_FORMAT {
        return Err(TransferError::Unreadable);
    }
    if envelope.version > EXPORT_VERSION {
        return Err(TransferError::UnsupportedVersion(envelope.version));
    }
    Ok(envelope)
}

/// Whether an export file is encrypted.
///
/// # Errors
/// [`TransferError::Unreadable`] or [`TransferError::UnsupportedVersion`].
pub fn export_is_encrypted(data: &[u8]) -> Result<bool, TransferError> {
    Ok(envelope(data)?.encrypted)
}

/// Reads an export file; `password` is needed (and only used) when it is encrypted.
///
/// # Errors
/// [`TransferError::NeedsPassword`], [`TransferError::WrongPassword`], [`TransferError::Unreadable`] or
/// [`TransferError::UnsupportedVersion`].
pub fn read_export(data: &[u8], password: Option<&str>) -> Result<ExportPayload, TransferError> {
    let envelope = envelope(data)?;
    if !envelope.encrypted {
        return envelope.payload.ok_or(TransferError::Unreadable);
    }
    let password = password
        .filter(|p| !p.is_empty())
        .ok_or(TransferError::NeedsPassword)?;
    let (Some(kdf), Some(cipher_name), Some(sealed)) = (envelope.kdf, envelope.cipher, envelope.data) else {
        return Err(TransferError::Unreadable);
    };
    if kdf.name != KDF_NAME || cipher_name != CIPHER_NAME || !(1..=MAX_ITERATIONS).contains(&kdf.iterations) {
        return Err(TransferError::Unreadable);
    }
    let salt = B64.decode(kdf.salt).map_err(|_| TransferError::Unreadable)?;
    let sealed = B64.decode(sealed).map_err(|_| TransferError::Unreadable)?;
    if sealed.len() < NONCE_LEN + 16 {
        return Err(TransferError::Unreadable);
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::try_from(nonce).map_err(|_| TransferError::Unreadable)?;
    let plain = Zeroizing::new(
        cipher(password, &salt, kdf.iterations)
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad: EXPORT_AAD,
                },
            )
            .map_err(|_| TransferError::WrongPassword)?,
    );
    serde_json::from_slice(&plain).map_err(|_| TransferError::Unreadable)
}

/// What to do with one exported record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferAction {
    /// Leave it out.
    Skip,
    /// Create it, even when a matching record exists ("keep both").
    New,
    /// Replace the matching record; without one, create it.
    Overwrite,
}

/// One action per exported record, in payload order; missing ones mean [`TransferAction::Skip`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferDecisions {
    /// Per [`ExportPayload::entries`].
    pub entries: Vec<TransferAction>,
    /// Per [`ExportPayload::credentials`].
    pub credentials: Vec<TransferAction>,
}

/// What [`Database::import_transfer`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransferCounts {
    /// Entries created.
    pub entries_created: usize,
    /// Entries replaced.
    pub entries_updated: usize,
    /// Credentials created.
    pub credentials_created: usize,
    /// Credentials replaced.
    pub credentials_updated: usize,
    /// Folders created (existing ones are reused).
    pub folders_created: usize,
}

fn action(list: &[TransferAction], index: usize) -> TransferAction {
    list.get(index).copied().unwrap_or(TransferAction::Skip)
}

/// The library connection an imported one replaces: same kind and label, not taken yet.
fn matching<'a>(
    old: &[&'a Connection],
    item: &ExportedConnection,
    taken: &mut HashSet<String>,
) -> Option<&'a Connection> {
    let found = old
        .iter()
        .find(|c| c.kind == item.kind && c.label == item.label && !taken.contains(&c.id))
        .copied()?;
    taken.insert(found.id.clone());
    Some(found)
}

/// The sealed records of one imported entry, ready for the transaction.
struct EntryPlan {
    entry: Entry,
    folder_path: Vec<String>,
    connections: Vec<Connection>,
    forwards: Vec<PortForward>,
    delete: Vec<String>,
}

/// The credentials to write and where each exported reference links to.
fn plan_credentials<'p>(
    payload: &'p ExportPayload,
    found: &TransferConflicts,
    library: &Library,
    decisions: &TransferDecisions,
    vault: &Vault,
    now: Timestamp,
    counts: &mut TransferCounts,
) -> (Vec<Credential>, HashMap<&'p str, String>) {
    let mut credentials = Vec::new();
    let mut links = HashMap::new();
    for (index, item) in payload.credentials.iter().enumerate() {
        let existing = found.credentials[index]
            .as_deref()
            .and_then(|id| library.credential(Some(id)));
        let mut credential = match (action(&decisions.credentials, index), existing) {
            (TransferAction::Skip, Some(existing)) => {
                links.insert(item.reference.as_str(), existing.id.clone());
                continue;
            }
            (TransferAction::Skip, None) => continue,
            (TransferAction::Overwrite, Some(existing)) => {
                counts.credentials_updated += 1;
                let mut credential = existing.clone();
                credential.updated_at = now;
                credential
            }
            (TransferAction::New | TransferAction::Overwrite, _) => {
                counts.credentials_created += 1;
                Credential::new(item.name.clone())
            }
        };
        credential.username.clone_from(&item.username);
        credential.domain.clone_from(&item.domain);
        credential.notes.clone_from(&item.notes);
        if let Some(password) = &item.password {
            credential.password_enc = vault.seal_optional(Some(&password.0));
        }
        links.insert(item.reference.as_str(), credential.id.clone());
        credentials.push(credential);
    }
    (credentials, links)
}

/// `entry` (a new one, or the one being replaced with its connections `old`) with the file's fields and
/// connections.
fn plan_entry(
    item: &ExportedEntry,
    mut entry: Entry,
    old: &[&Connection],
    links: &HashMap<&str, String>,
    vault: &Vault,
    now: Timestamp,
) -> EntryPlan {
    entry.name.clone_from(&item.name);
    entry.host.clone_from(&item.host);
    entry.notes.clone_from(&item.notes);
    entry.color = item.color;
    entry.is_favorite = item.is_favorite;
    entry.updated_at = now;

    let mut taken = HashSet::new();
    let mut connections = Vec::new();
    let mut forwards = Vec::new();
    for (position, source) in item.connections.iter().enumerate() {
        let previous = matching(old, source, &mut taken);
        let mut connection = previous.map_or_else(
            || Connection::new(source.kind, item.name.clone(), item.host.clone()),
            |c| (*c).clone(),
        );
        connection.entry_id.clone_from(&entry.id);
        connection.kind = source.kind;
        connection.label.clone_from(&source.label);
        connection.port = source.port;
        connection.username.clone_from(&source.username);
        connection.domain.clone_from(&source.domain);
        if let Some(password) = &source.password {
            connection.password_enc = vault.seal_optional(Some(&password.0));
        }
        if let Some(id) = source.credential_ref.as_deref().and_then(|r| links.get(r)) {
            connection.credential_id = Some(id.clone());
        }
        connection.rdp_admin_session = source.rdp_admin_session;
        connection.browser_bundle_id.clone_from(&source.browser_bundle_id);
        connection.mikrotik_mode = source.mikrotik_mode;
        connection.ssh_terminal = source.ssh_terminal;
        connection.rdp_client = source.rdp_client;
        connection.show_as_button = source.show_as_button;
        connection.sort_order = i64::try_from(position).unwrap_or(i64::MAX);
        connection.web_scheme.clone_from(&source.web_scheme);
        connection.web_path.clone_from(&source.web_path);
        connection.updated_at = now;
        forwards.extend(source.port_forwards.iter().map(|forward| PortForward {
            id: new_id(),
            connection_id: connection.id.clone(),
            name: forward.name.clone(),
            local_port: forward.local_port,
            remote_host: forward.remote_host.clone(),
            remote_port: forward.remote_port,
            created_at: now,
        }));
        connections.push(connection);
    }
    entry.default_connection_id = item
        .default_connection
        .and_then(|i| connections.get(i))
        .map(|c| c.id.clone());
    EntryPlan {
        entry,
        folder_path: item.folder_path.clone(),
        connections,
        forwards,
        delete: old
            .iter()
            .filter(|c| !taken.contains(&c.id))
            .map(|c| c.id.clone())
            .collect(),
    }
}

impl Database {
    /// Imports an export in one transaction, following `decisions`. Conflicts are looked up again here.
    ///
    /// - A replaced credential gets the file's user, domain and notes, and its password when the file has
    ///   one; its SSH key settings stay.
    /// - A replaced entry gets the file's fields and connections. A connection of the same kind and label
    ///   keeps its id (so saved splits stay), its SSH key settings, and its password and credential link
    ///   when the file has none. Other old connections are deleted; port forwards come from the file.
    /// - A connection links to its credential when that was imported or replaced; when it was skipped, to
    ///   the existing credential of the same name, if any.
    ///
    /// # Errors
    /// If SQLite fails; nothing changes then.
    pub fn import_transfer(
        &self,
        payload: &ExportPayload,
        decisions: &TransferDecisions,
        vault: &Vault,
    ) -> Result<TransferCounts, StoreError> {
        let library = self.library()?;
        let found = conflicts(payload, &library);
        let now = Timestamp::now();
        let mut counts = TransferCounts::default();

        let (credentials, links) =
            plan_credentials(payload, &found, &library, decisions, vault, now, &mut counts);
        let mut writes = Vec::new();
        for (index, item) in payload.entries.iter().enumerate() {
            let existing = found.entries[index].as_deref().and_then(|id| library.entry(id));
            let (entry, old) = match (action(&decisions.entries, index), existing) {
                (TransferAction::Skip, _) => continue,
                (TransferAction::Overwrite, Some(existing)) => {
                    counts.entries_updated += 1;
                    (existing.clone(), library.connections_of(&existing.id))
                }
                (TransferAction::New | TransferAction::Overwrite, _) => {
                    counts.entries_created += 1;
                    (Entry::new(item.name.clone(), item.host.clone()), Vec::new())
                }
            };
            writes.push(plan_entry(item, entry, &old, &links, vault, now));
        }

        self.transaction(|tx| {
            let mut folders = Folders::new(tx, None);
            for credential in &credentials {
                upsert_credential(tx, credential)?;
            }
            for write in &writes {
                let mut entry = write.entry.clone();
                // A replaced entry stays where it is (it was found through that very path).
                entry.folder_id = folders.resolve(&write.folder_path)?;
                upsert_entry(tx, &entry)?;
                for id in &write.delete {
                    tx.execute("DELETE FROM connection WHERE id = ?1", [id])?;
                }
                for connection in &write.connections {
                    tx.execute("DELETE FROM portForward WHERE connectionId = ?1", [&connection.id])?;
                    upsert_connection(tx, connection)?;
                }
                for forward in &write.forwards {
                    tx.execute(
                        &format!(
                            "INSERT INTO portForward ({PORT_FORWARD_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                        ),
                        params![
                            forward.id,
                            forward.connection_id,
                            forward.name,
                            forward.local_port,
                            forward.remote_host,
                            forward.remote_port,
                            forward.created_at.to_string()
                        ],
                    )?;
                }
            }
            counts.folders_created = folders.created;
            Ok(counts)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rd_core::transfer::{ExportSelection, build_payload};
    use rd_core::{ConnectionType, Folder};
    use secrecy::ExposeSecret;

    use super::*;

    const ITERATIONS: u32 = 1_000;

    fn setup() -> (Database, Vault) {
        let db = Database::open_in_memory().unwrap();
        let vault = Vault::create(&db, "master", ITERATIONS).unwrap();
        (db, vault)
    }

    fn open(vault: &Vault, sealed: Option<&rd_core::Sealed>) -> Option<String> {
        vault
            .open_optional(sealed)
            .unwrap()
            .map(|s| s.expose_secret().to_owned())
    }

    /// A library with `Clients/Acme/dc01` (RDP linked to "Domain admin" + SSH with its own password and a
    /// forward) and the credential.
    fn seed(db: &Database, vault: &Vault) -> (Entry, Credential) {
        let parent = Folder::new("Clients");
        db.save_folder(&parent).unwrap();
        let mut child = Folder::new("Acme");
        child.parent_id = Some(parent.id.clone());
        db.save_folder(&child).unwrap();

        let mut credential = Credential::new("Domain admin");
        credential.username = "admin".into();
        credential.password_enc = Some(vault.seal("cred-pw"));
        credential.ssh_key_path = Some("~/.ssh/id_ed25519".into());
        let credential = db.save_credential(&credential).unwrap();

        let mut entry = Entry::new("dc01", "dc01.acme.local");
        entry.folder_id = Some(child.id.clone());
        let mut rdp = Connection::new(ConnectionType::Rdp, "dc01", "dc01.acme.local");
        rdp.credential_id = Some(credential.id.clone());
        let mut ssh = Connection::new(ConnectionType::Ssh, "dc01", "dc01.acme.local");
        ssh.username = "root".into();
        ssh.password_enc = Some(vault.seal("own-pw"));
        ssh.ssh_key_path = Some("~/.ssh/id_rsa".into());
        ssh.sort_order = 1;
        let write = db.write_entry(&entry, &[rdp, ssh.clone()], &[]).unwrap();
        db.save_port_forward(&PortForward {
            id: new_id(),
            connection_id: ssh.id.clone(),
            name: "db".into(),
            local_port: 15432,
            remote_host: "127.0.0.1".into(),
            remote_port: 5432,
            created_at: Timestamp::now(),
        })
        .unwrap();
        (write.entry, credential)
    }

    fn export(db: &Database, vault: &Vault, entry: &Entry, credential: Option<&Credential>) -> ExportPayload {
        let selection = ExportSelection {
            entries: vec![entry.id.clone()],
            credentials: credential.map(|c| vec![c.id.clone()]).unwrap_or_default(),
        };
        let mut opener = |s: &rd_core::Sealed| -> Result<String, Infallible> {
            Ok(vault.open(s).unwrap().expose_secret().to_owned())
        };
        build_payload(&db.library().unwrap(), &selection, "1.0.0", Some(&mut opener)).unwrap()
    }

    fn all(n: usize, action: TransferAction) -> Vec<TransferAction> {
        vec![action; n]
    }

    // Envelope

    #[test]
    fn encrypted_round_trip_hides_the_payload() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let payload = export(&db, &vault, &entry, Some(&credential));
        let text = write_export(&payload, Some("share-pw"), ITERATIONS).unwrap();
        assert!(!text.contains("cred-pw"));
        assert!(!text.contains("dc01"));
        assert!(export_is_encrypted(text.as_bytes()).unwrap());
        assert_eq!(read_export(text.as_bytes(), Some("share-pw")).unwrap(), payload);
    }

    #[test]
    fn encrypted_needs_the_right_password() {
        let (db, vault) = setup();
        let (entry, _) = seed(&db, &vault);
        let text = write_export(&export(&db, &vault, &entry, None), Some("pw"), ITERATIONS).unwrap();
        assert_eq!(
            read_export(text.as_bytes(), None),
            Err(TransferError::NeedsPassword)
        );
        assert_eq!(
            read_export(text.as_bytes(), Some("")),
            Err(TransferError::NeedsPassword)
        );
        assert_eq!(
            read_export(text.as_bytes(), Some("nope")),
            Err(TransferError::WrongPassword)
        );
    }

    #[test]
    fn tampered_data_is_refused() {
        let (db, vault) = setup();
        let (entry, _) = seed(&db, &vault);
        let text = write_export(&export(&db, &vault, &entry, None), Some("pw"), ITERATIONS).unwrap();
        let mut envelope: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut data = B64.decode(envelope["data"].as_str().unwrap()).unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;
        envelope["data"] = B64.encode(data).into();
        let tampered = envelope.to_string();
        assert_eq!(
            read_export(tampered.as_bytes(), Some("pw")),
            Err(TransferError::WrongPassword)
        );

        envelope["kdf"]["iterations"] = (MAX_ITERATIONS + 1).into();
        assert_eq!(
            read_export(envelope.to_string().as_bytes(), Some("pw")),
            Err(TransferError::Unreadable)
        );
    }

    #[test]
    fn plain_files_have_no_passwords() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let with_passwords = export(&db, &vault, &entry, Some(&credential));
        assert_eq!(
            write_export(&with_passwords, None, ITERATIONS),
            Err(TransferError::PlainWithPasswords)
        );
        assert_eq!(
            write_export(&with_passwords, Some(""), ITERATIONS),
            Err(TransferError::EmptyPassword)
        );
        let library = db.library().unwrap();
        let selection = ExportSelection {
            entries: vec![entry.id.clone()],
            credentials: vec![credential.id.clone()],
        };
        let plain = build_payload::<Infallible>(&library, &selection, "1", None).unwrap();
        let text = write_export(&plain, None, ITERATIONS).unwrap();
        assert!(!text.contains("password"));
        assert!(!export_is_encrypted(text.as_bytes()).unwrap());
        assert_eq!(read_export(text.as_bytes(), Some("ignored")).unwrap(), plain);
    }

    #[test]
    fn foreign_and_newer_files_are_refused() {
        assert_eq!(read_export(b"not json", None), Err(TransferError::Unreadable));
        assert_eq!(
            read_export(br#"{"format":"other","version":1,"encrypted":false}"#, None),
            Err(TransferError::Unreadable)
        );
        assert_eq!(
            read_export(
                br#"{"format":"remotedesktop-export","version":2,"encrypted":false}"#,
                None
            ),
            Err(TransferError::UnsupportedVersion(2))
        );
        assert_eq!(
            read_export(
                br#"{"format":"remotedesktop-export","version":1,"encrypted":false}"#,
                None
            ),
            Err(TransferError::Unreadable)
        );
    }

    // Writer

    #[test]
    fn imports_into_an_empty_library() {
        let (source, source_vault) = setup();
        let (entry, credential) = seed(&source, &source_vault);
        let payload = export(&source, &source_vault, &entry, Some(&credential));

        let (db, vault) = setup();
        let counts = db
            .import_transfer(
                &payload,
                &TransferDecisions {
                    entries: all(1, TransferAction::New),
                    credentials: all(1, TransferAction::New),
                },
                &vault,
            )
            .unwrap();
        assert_eq!(counts.entries_created, 1);
        assert_eq!(counts.credentials_created, 1);
        assert_eq!(counts.folders_created, 2);

        let library = db.library().unwrap();
        let imported = &library.entries[0];
        let acme = library.folders.iter().find(|f| f.name == "Acme").unwrap();
        assert_eq!(imported.folder_id.as_deref(), Some(acme.id.as_str()));
        let connections = library.connections_of(&imported.id);
        assert_eq!(connections.len(), 2);
        let credential = &library.credentials[0];
        assert_eq!(
            open(&vault, credential.password_enc.as_ref()).as_deref(),
            Some("cred-pw")
        );
        assert_eq!(credential.ssh_key_path, None);
        assert_eq!(
            connections[0].credential_id.as_deref(),
            Some(credential.id.as_str())
        );
        assert_eq!(
            open(&vault, connections[1].password_enc.as_ref()).as_deref(),
            Some("own-pw")
        );
        assert_eq!(connections[1].ssh_key_path, None);
        assert_eq!(library.port_forwards.len(), 1);
        assert_eq!(library.port_forwards[0].connection_id, connections[1].id);
        assert_eq!(
            imported.default_connection_id.as_deref(),
            Some(connections[0].id.as_str())
        );
    }

    #[test]
    fn skipped_records_are_not_written() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let payload = export(&db, &vault, &entry, Some(&credential));
        let counts = db
            .import_transfer(&payload, &TransferDecisions::default(), &vault)
            .unwrap();
        assert_eq!(counts, TransferCounts::default());
        let library = db.library().unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.credentials.len(), 1);
    }

    #[test]
    fn keep_both_creates_copies() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let payload = export(&db, &vault, &entry, Some(&credential));
        db.import_transfer(
            &payload,
            &TransferDecisions {
                entries: all(1, TransferAction::New),
                credentials: all(1, TransferAction::New),
            },
            &vault,
        )
        .unwrap();
        let library = db.library().unwrap();
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.credentials.len(), 2);
        assert_eq!(library.port_forwards.len(), 2);
        // The copy links to the new credential.
        let copy = library.entries.iter().find(|e| e.id != entry.id).unwrap();
        let rdp = library.connections_of(&copy.id)[0];
        assert_ne!(rdp.credential_id.as_deref(), Some(credential.id.as_str()));
    }

    #[test]
    fn overwrite_replaces_but_keeps_local_settings() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let library = db.library().unwrap();
        let old_ssh = library.connections_of(&entry.id)[1].clone();

        // The colleague's file: new notes and a new credential password, the SSH connection without a
        // password (plain export), an extra web connection.
        let mut payload = export(&db, &vault, &entry, Some(&credential));
        payload.entries[0].notes = "updated".into();
        payload.credentials[0].password = Some(rd_core::transfer::ExportSecret("new-cred-pw".into()));
        payload.credentials[0].username = "administrator".into();
        payload.entries[0].connections[1].password = None;
        payload.entries[0].connections[1].port_forwards.clear();
        let mut web = payload.entries[0].connections[0].clone();
        web.kind = ConnectionType::Web;
        web.credential_ref = None;
        payload.entries[0].connections.push(web);

        let counts = db
            .import_transfer(
                &payload,
                &TransferDecisions {
                    entries: all(1, TransferAction::Overwrite),
                    credentials: all(1, TransferAction::Overwrite),
                },
                &vault,
            )
            .unwrap();
        assert_eq!(counts.entries_updated, 1);
        assert_eq!(counts.credentials_updated, 1);
        assert_eq!(counts.entries_created, 0);

        let library = db.library().unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].id, entry.id);
        assert_eq!(library.entries[0].notes, "updated");
        assert_eq!(library.credentials.len(), 1);
        let updated = &library.credentials[0];
        assert_eq!(updated.username, "administrator");
        assert_eq!(
            open(&vault, updated.password_enc.as_ref()).as_deref(),
            Some("new-cred-pw")
        );
        assert_eq!(updated.ssh_key_path.as_deref(), Some("~/.ssh/id_ed25519"));

        let connections = library.connections_of(&entry.id);
        assert_eq!(connections.len(), 3);
        let ssh = connections[1];
        assert_eq!(ssh.id, old_ssh.id);
        assert_eq!(ssh.ssh_key_path.as_deref(), Some("~/.ssh/id_rsa"));
        assert_eq!(open(&vault, ssh.password_enc.as_ref()).as_deref(), Some("own-pw"));
        assert!(library.port_forwards.is_empty());
        assert_eq!(connections[2].kind, ConnectionType::Web);
        assert_eq!(connections[2].notes, "updated");
    }

    #[test]
    fn overwrite_drops_connections_missing_from_the_file() {
        let (db, vault) = setup();
        let (entry, _) = seed(&db, &vault);
        let mut payload = export(&db, &vault, &entry, None);
        payload.entries[0].connections.truncate(1);
        db.import_transfer(
            &payload,
            &TransferDecisions {
                entries: all(1, TransferAction::Overwrite),
                credentials: Vec::new(),
            },
            &vault,
        )
        .unwrap();
        let library = db.library().unwrap();
        assert_eq!(library.connections_of(&entry.id).len(), 1);
        assert!(library.port_forwards.is_empty());
    }

    #[test]
    fn skipped_credential_links_to_the_existing_one() {
        let (db, vault) = setup();
        let (entry, credential) = seed(&db, &vault);
        let payload = export(&db, &vault, &entry, Some(&credential));
        db.import_transfer(
            &payload,
            &TransferDecisions {
                entries: all(1, TransferAction::New),
                credentials: all(1, TransferAction::Skip),
            },
            &vault,
        )
        .unwrap();
        let library = db.library().unwrap();
        assert_eq!(library.credentials.len(), 1);
        let copy = library.entries.iter().find(|e| e.id != entry.id).unwrap();
        assert_eq!(
            library.connections_of(&copy.id)[0].credential_id.as_deref(),
            Some(credential.id.as_str())
        );
    }

    #[test]
    fn unknown_credential_ref_leaves_no_link() {
        let (source, source_vault) = setup();
        let (entry, credential) = seed(&source, &source_vault);
        let payload = export(&source, &source_vault, &entry, Some(&credential));
        let (db, vault) = setup();
        db.import_transfer(
            &payload,
            &TransferDecisions {
                entries: all(1, TransferAction::New),
                credentials: all(1, TransferAction::Skip),
            },
            &vault,
        )
        .unwrap();
        let library = db.library().unwrap();
        assert!(library.credentials.is_empty());
        assert!(library.connections.iter().all(|c| c.credential_id.is_none()));
    }

    #[test]
    fn overwrite_without_a_match_creates() {
        let (source, source_vault) = setup();
        let (entry, credential) = seed(&source, &source_vault);
        let payload = export(&source, &source_vault, &entry, Some(&credential));
        let (db, vault) = setup();
        let counts = db
            .import_transfer(
                &payload,
                &TransferDecisions {
                    entries: all(1, TransferAction::Overwrite),
                    credentials: all(1, TransferAction::Overwrite),
                },
                &vault,
            )
            .unwrap();
        assert_eq!(counts.entries_created, 1);
        assert_eq!(counts.credentials_created, 1);
    }
}
