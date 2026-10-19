//! `AppCore` against temp-dir databases with an in-memory key cache.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rd_core::draft::{ConnectionDraft, EntryDraft};
use rd_core::import::ImportKind;
use rd_core::shortcuts::{KeyShortcut, Modifiers, Platform};
use rd_core::{ConnectionType, Timestamp};
use rd_platform::Settings;
use rd_store::{DbLock, KeyCache, KeyCacheError, MemoryKeyCache};
use secrecy::ExposeSecret;
use ssh_key::PrivateKey;
use tempfile::TempDir;
use zeroize::Zeroizing;

use super::ssh_unlock::tests::write_encrypted_key;
use super::*;
use crate::dto::{DeleteCountsDto, MoveResultDto};

/// A key cache the test can look into after handing it to the core.
#[derive(Clone, Default)]
struct SharedKeys(Arc<MemoryKeyCache>);

impl KeyCache for SharedKeys {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, KeyCacheError> {
        self.0.load(account)
    }
    fn save(&self, account: &str, key: &[u8; 32]) -> Result<(), KeyCacheError> {
        self.0.save(account, key)
    }
    fn delete(&self, account: &str) -> Result<(), KeyCacheError> {
        self.0.delete(account)
    }
}

impl SharedKeys {
    fn has(&self, db: &Path) -> bool {
        self.0.load(&account(db)).unwrap().is_some()
    }
}

pub(super) struct Env {
    dir: TempDir,
    keys: SharedKeys,
}

impl Env {
    pub(super) fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            keys: SharedKeys::default(),
        }
    }

    pub(super) fn settings_dir(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    pub(super) fn db(&self, name: &str) -> PathBuf {
        self.dir.path().join("data").join(name)
    }

    /// A core whose settings point at `db`, opened.
    pub(super) fn core_for(&self, db: &str, platform: Platform) -> AppCore {
        let mut settings = Settings::load(&self.settings_dir());
        settings.database_path = Some(self.db(db));
        settings.save(&self.settings_dir()).unwrap();
        let core = AppCore::with_options(self.settings_dir(), Box::new(self.keys.clone()), 1_000, platform);
        core.open_current();
        core
    }

    pub(super) fn core(&self) -> AppCore {
        self.core_for("connections.sqlite", Platform::MacOS)
    }

    /// A core with a fresh vault, unlocked, remembering the key.
    pub(super) fn unlocked(&self) -> AppCore {
        let core = self.core();
        core.create_vault("master", true).unwrap();
        core
    }
}

fn phase(core: &AppCore) -> Phase {
    core.status().phase
}

const LOCKED: Phase = Phase::Locked { needs_setup: false };
const SETUP: Phase = Phase::Locked { needs_setup: true };
const UNLOCKED: Phase = Phase::Unlocked { read_only: false };

pub(super) const HOST: &str = "host.example.com";

/// The shared fields of a root entry.
pub(super) fn entry_draft(name: &str, host: &str) -> EntryDraft {
    EntryDraft {
        name: name.into(),
        host: host.into(),
        ..EntryDraft::default()
    }
}

/// Saves a one-connection entry: a new one, or with `id` the entry of that connection. Returns the
/// connection id.
pub(super) fn save_single(
    core: &AppCore,
    id: Option<&str>,
    entry: &EntryDraft,
    draft: ConnectionDraft,
    password: Option<&str>,
    passphrase: Option<&str>,
) -> Result<String, CommandError> {
    let entry_id = id.map(|id| {
        core.library()
            .connections
            .iter()
            .find(|c| c.id == id)
            .map_or_else(|| id.to_owned(), |c| c.entry_id.clone())
    });
    let save = ConnectionSave {
        id: id.map(Into::into),
        draft,
        password: password.map(Into::into),
        passphrase: passphrase.map(Into::into),
    };
    let entry_id = core.save_entry(entry_id.as_deref(), entry, &[save])?;
    Ok(core
        .library()
        .entries
        .iter()
        .find(|e| e.id == entry_id)
        .map(|e| e.connection_ids[0].clone())
        .unwrap_or_default())
}

/// An entry "Entry" on [`HOST`] with one blank connection per kind; returns its id and the connection
/// ids in order.
pub(super) fn entry_with(core: &AppCore, kinds: &[ConnectionType]) -> (String, Vec<String>) {
    let saves: Vec<ConnectionSave> = kinds
        .iter()
        .map(|&kind| ConnectionSave {
            id: None,
            draft: ConnectionDraft::new(kind),
            password: None,
            passphrase: None,
        })
        .collect();
    let id = core
        .save_entry(None, &entry_draft("Entry", HOST), &saves)
        .unwrap();
    let connections = core
        .library()
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap()
        .connection_ids
        .clone();
    (id, connections)
}

/// The entry a connection belongs to.
fn entry_of(core: &AppCore, connection_id: &str) -> String {
    core.library()
        .connections
        .iter()
        .find(|c| c.id == connection_id)
        .unwrap()
        .entry_id
        .clone()
}

fn folder(core: &AppCore, name: &str, parent: Option<&str>) -> String {
    core.save_folder(&FolderInput {
        name: name.into(),
        parent_id: parent.map(Into::into),
        ..FolderInput::default()
    })
    .unwrap()
}

fn connection(core: &AppCore, name: &str, folder_id: Option<&str>) -> String {
    let mut entry = entry_draft(name, HOST);
    entry.folder_id = folder_id.map(Into::into);
    save_single(
        core,
        None,
        &entry,
        ConnectionDraft::new(ConnectionType::Ssh),
        None,
        None,
    )
    .unwrap()
}

fn credential(core: &AppCore, name: &str, password: Option<&str>) -> String {
    let input = CredentialInput {
        name: name.into(),
        username: "admin".into(),
        ..CredentialInput::default()
    };
    core.save_credential(None, &input, password, None).unwrap()
}

pub(super) fn record<T>(core: &AppCore, read: impl FnOnce(&rd_store::Database) -> T) -> T {
    read(core.inner().db.as_ref().unwrap())
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| (*t).to_owned()).collect()
}

// Opening, vault and keyring policy

#[test]
fn create_unlock_lock_follow_the_keyring_policy() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    let core = env.core();
    assert_eq!(phase(&core), SETUP);
    assert_eq!(core.status().database_path, Some(path.display().to_string()));

    core.create_vault("master", true).unwrap();
    assert_eq!(phase(&core), UNLOCKED);
    assert!(env.keys.has(&path));
    assert_eq!(
        core.create_vault("again", true).unwrap_err().code,
        "vaultAlreadySetUp"
    );
    drop(core);

    // A remembered key unlocks on open.
    let core = env.core();
    assert_eq!(phase(&core), UNLOCKED);

    // Lock forgets it.
    assert_eq!(core.lock().phase, LOCKED);
    assert!(!env.keys.has(&path));
    assert_eq!(core.library().connections.len(), 0);

    let err = core.unlock("wrong", true).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("wrongPassword", "Wrong password")
    );
    assert_eq!(phase(&core), LOCKED);

    // Remember off: unlocks, deletes the key and persists the choice.
    core.unlock("master", false).unwrap();
    assert_eq!(phase(&core), UNLOCKED);
    assert!(!env.keys.has(&path));
    assert!(!Settings::load(&env.settings_dir()).remember_master_key);
    assert!(!core.status().remember_master_key);
    drop(core);
    assert_eq!(phase(&env.core()), LOCKED);
}

#[test]
fn empty_password_is_refused() {
    let env = Env::new();
    let core = env.core();
    assert_eq!(core.create_vault("", true).unwrap_err().code, "emptyPassword");
    assert_eq!(phase(&core), SETUP);
}

#[test]
fn switching_databases_keeps_the_old_key_and_persists_the_path() {
    let env = Env::new();
    let first = env.db("connections.sqlite");
    let second = env.db("other/second.sqlite");
    let core = env.unlocked();
    connection(&core, "web01", None);

    let status = core.create_database(&second);
    assert_eq!(status.phase, SETUP);
    assert_eq!(status.database_path, Some(second.display().to_string()));
    assert!(env.keys.has(&first), "switching must not forget the old key");
    assert_eq!(
        Settings::load(&env.settings_dir()).database_path,
        Some(second.clone())
    );
    assert!(core.library().connections.is_empty());
    // The old database's lock file is released, the new one taken.
    assert!(!DbLock::path_for(&first).exists());
    assert!(DbLock::path_for(&second).exists());

    assert_eq!(core.open_database(&first).phase, UNLOCKED);
    assert_eq!(core.library().connections.len(), 1);
    assert_eq!(
        core.settings_get().database_path,
        Some(first.display().to_string())
    );
}

#[test]
fn an_unopenable_database_shows_the_error_screen() {
    let env = Env::new();
    let core = env.unlocked();
    let not_a_db = env.dir.path().join("data/garbage.sqlite");
    std::fs::write(&not_a_db, b"this is not sqlite at all, not even close......").unwrap();
    let status = core.open_database(&not_a_db);
    let Phase::NoDatabase { code, error } = status.phase else {
        panic!("expected the error screen, got {:?}", status.phase)
    };
    assert_eq!(code, "openFailed");
    assert!(error.starts_with("Can't open the database: "));
    assert!(!DbLock::path_for(&not_a_db).exists());
    assert_eq!(
        core.save_folder(&FolderInput::default()).unwrap_err().code,
        "locked"
    );
}

/// A core pretending to be app `version`, on the settings' database, opened.
fn versioned(env: &Env, version: &str) -> AppCore {
    let core = AppCore::with_options(
        env.settings_dir(),
        Box::new(env.keys.clone()),
        1_000,
        Platform::MacOS,
    )
    .with_app_version(version);
    core.open_current();
    core
}

fn stamp(path: &Path) -> Option<String> {
    rd_store::stored_app_version(path).unwrap()
}

fn set_stamp(path: &Path, version: &str) {
    let db = rd_store::Database::open(path).unwrap();
    db.set_meta(rd_store::APP_VERSION_KEY, version.as_bytes())
        .unwrap();
}

#[test]
fn opening_stamps_the_database_with_the_newest_app_version() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    drop(env.unlocked());
    assert_eq!(stamp(&path).as_deref(), Some(env!("CARGO_PKG_VERSION")));

    drop(versioned(&env, "99.0.0"));
    assert_eq!(stamp(&path).as_deref(), Some("99.0.0"));
    set_stamp(&path, "1.0.0");
    let core = versioned(&env, "1.2.0");
    assert_eq!(phase(&core), UNLOCKED);
    assert_eq!(stamp(&path).as_deref(), Some("1.2.0"));
}

#[test]
fn an_older_app_is_refused_without_touching_the_database() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    drop(env.unlocked());
    set_stamp(&path, "9.0.0");
    let before = std::fs::read(&path).unwrap();

    let core = versioned(&env, "1.6.6");
    assert_eq!(
        phase(&core),
        Phase::NeedsUpdate {
            database_version: "9.0.0".into(),
            app_version: "1.6.6".into()
        }
    );
    assert!(!DbLock::path_for(&path).exists(), "no lock is taken");
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file is untouched");
    assert_eq!(
        core.save_folder(&FolderInput::default()).unwrap_err().code,
        "locked"
    );
    assert_eq!(
        serde_json::to_value(core.status()).unwrap()["phase"],
        serde_json::json!({"kind": "needsUpdate", "databaseVersion": "9.0.0", "appVersion": "1.6.6"})
    );

    // Retry after the database was used with this version again (e.g. a restored backup).
    set_stamp(&path, "1.6.6");
    assert_eq!(core.open_current().phase, UNLOCKED);

    // The same build as the stamp opens it and keeps the stamp.
    drop(core);
    set_stamp(&path, "9.0.0");
    assert_eq!(phase(&versioned(&env, "9.0.0")), UNLOCKED);
    assert_eq!(stamp(&path).as_deref(), Some("9.0.0"));
}

#[test]
fn a_read_only_open_does_not_stamp() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    drop(env.unlocked());
    set_stamp(&path, "1.0.0");
    write_foreign_lock(&path);
    let core = versioned(&env, "2.0.0");
    assert!(matches!(phase(&core), Phase::LockConflict { .. }));
    core.resolve_db_lock(LockChoice::ReadOnly).unwrap();
    assert_eq!(stamp(&path).as_deref(), Some("1.0.0"));
}

#[test]
fn change_password_checks_the_current_one_and_rekeys() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    let core = env.unlocked();
    let id = connection(&core, "srv", None);
    save_single(
        &core,
        Some(&id),
        &entry_draft("srv", HOST),
        ConnectionDraft::new(ConnectionType::Ssh),
        Some("secret"),
        None,
    )
    .unwrap();

    let err = core.change_password("nope", "new").unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("wrongPassword", "The current master password is wrong.")
    );
    assert_eq!(
        core.change_password("master", "").unwrap_err().code,
        "emptyPassword"
    );
    core.change_password("master", "new-master").unwrap();
    assert_eq!(core.password_for_copy(&id).unwrap().expose_secret(), "secret");
    drop(core);

    // The keyring follows the new key.
    let core = env.core();
    assert_eq!(phase(&core), UNLOCKED);
    core.lock();
    assert!(!env.keys.has(&path));
    assert_eq!(core.unlock("master", true).unwrap_err().code, "wrongPassword");
    core.unlock("new-master", true).unwrap();
    assert_eq!(core.password_for_copy(&id).unwrap().expose_secret(), "secret");
}

#[test]
fn turning_remember_off_while_unlocked_forgets_the_key() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    let core = env.unlocked();
    assert!(env.keys.has(&path));
    core.settings_set(&SettingsPatch {
        remember_master_key: Some(false),
        ..SettingsPatch::default()
    })
    .unwrap();
    assert!(!env.keys.has(&path));
    core.settings_set(&SettingsPatch {
        remember_master_key: Some(true),
        ..SettingsPatch::default()
    })
    .unwrap();
    assert!(env.keys.has(&path));
}

// Lock file

fn write_foreign_lock(db: &Path) {
    let lock = format!(
        r#"{{"host":"other-host.invalid","pid":4242,"refreshedAt":"{}"}}"#,
        Timestamp::now()
    );
    std::fs::write(DbLock::path_for(db), lock).unwrap();
}

#[test]
fn lock_conflict_offers_read_only_open_anyway_and_cancel() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    let core = env.unlocked();
    let id = connection(&core, "srv", None);
    assert_eq!(
        core.resolve_db_lock(LockChoice::Cancel).unwrap_err().code,
        "noLockConflict"
    );
    drop(core);

    write_foreign_lock(&path);
    let core = env.core();
    let Phase::LockConflict { holder } = phase(&core) else {
        panic!("expected a lock conflict")
    };
    assert_eq!((holder.host.as_str(), holder.pid), ("other-host.invalid", 4242));
    assert_eq!(
        core.save_folder(&FolderInput::default()).unwrap_err().code,
        "locked"
    );

    // Read-only: unlocked from the keyring, reads work, writes are refused.
    let status = core.resolve_db_lock(LockChoice::ReadOnly).unwrap();
    assert_eq!(status.phase, Phase::Unlocked { read_only: true });
    assert!(status.read_only);
    assert_eq!(core.library().connections.len(), 1);
    assert!(core.password_for_copy(&id).is_err_and(|e| e.code == "noPassword"));
    let refused = [
        core.save_folder(&FolderInput {
            name: "x".into(),
            ..FolderInput::default()
        })
        .unwrap_err(),
        core.set_favorite(&format!("c:{id}"), true).unwrap_err(),
        core.delete_items(&tags(&["c:x"])).unwrap_err(),
        core.duplicate_entry(&entry_of(&core, &id), " (copy)")
            .unwrap_err(),
        core.merge_entries("a", &["b".into()]).unwrap_err(),
        core.save_entry(None, &entry_draft("x", HOST), &[]).unwrap_err(),
        core.change_password("master", "x").unwrap_err(),
    ];
    assert!(refused.iter().all(|e| e.code == "readOnly"), "{refused:?}");
    assert!(
        std::fs::read_to_string(DbLock::path_for(&path))
            .unwrap()
            .contains("other-host.invalid")
    );
    drop(core);

    // Cancel: the error screen.
    let core = env.core();
    let status = core.resolve_db_lock(LockChoice::Cancel).unwrap();
    assert!(matches!(status.phase, Phase::NoDatabase { ref code, .. } if code == "lockCancelled"));
    drop(core);

    // Open anyway: the lock becomes ours and writes work.
    let core = env.core();
    assert_eq!(
        core.resolve_db_lock(LockChoice::OpenAnyway).unwrap().phase,
        UNLOCKED
    );
    assert!(
        !std::fs::read_to_string(DbLock::path_for(&path))
            .unwrap()
            .contains("other-host.invalid")
    );
    core.set_favorite(&format!("c:{id}"), true).unwrap();
    core.refresh_lock();
    core.close();
    assert!(!DbLock::path_for(&path).exists());
}

#[test]
fn writes_are_refused_while_locked() {
    let env = Env::new();
    let core = env.unlocked();
    let id = connection(&core, "srv", None);
    core.lock();
    assert_eq!(
        core.set_favorite(&format!("c:{id}"), true).unwrap_err().code,
        "locked"
    );
    assert_eq!(core.password_for_copy(&id).unwrap_err().code, "locked");
    assert!(core.search("srv").entries.is_empty());
    assert!(core.library().connections.is_empty());
}

// Secrets

#[test]
fn connection_secrets_keep_clear_and_set() {
    let env = Env::new();
    let core = env.unlocked();
    let entry = entry_draft("srv", HOST);
    let ssh = || ConnectionDraft::new(ConnectionType::Ssh);
    let save = |id: Option<&str>, password: Option<&str>, passphrase: Option<&str>| {
        save_single(&core, id, &entry, ssh(), password, passphrase).unwrap()
    };
    let id = save(None, Some("pw-1"), Some("phrase-1"));
    let stored = |core: &AppCore| record(core, |db| db.connection(&id).unwrap().unwrap());
    let first = stored(&core);
    assert!(first.password_enc.is_some() && first.ssh_key_passphrase_enc.is_some());
    assert_eq!(core.password_for_copy(&id).unwrap().expose_secret(), "pw-1");

    // null keeps the sealed bytes untouched.
    save(Some(&id), None, None);
    let kept = stored(&core);
    assert_eq!(kept.password_enc, first.password_enc);
    assert_eq!(kept.ssh_key_passphrase_enc, first.ssh_key_passphrase_enc);
    assert_eq!(kept.created_at, first.created_at);

    // A value replaces, "" clears.
    save(Some(&id), Some("pw-2"), Some(""));
    assert_eq!(core.password_for_copy(&id).unwrap().expose_secret(), "pw-2");
    assert_eq!(stored(&core).ssh_key_passphrase_enc, None);
    save(Some(&id), Some(""), None);
    let err = core.password_for_copy(&id).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("noPassword", "No saved password")
    );

    // Non-SSH types drop the passphrase.
    let rdp = save_single(
        &core,
        None,
        &entry_draft("dc", HOST),
        ConnectionDraft::new(ConnectionType::Rdp),
        Some("pw"),
        Some("phrase"),
    )
    .unwrap();
    assert_eq!(
        record(&core, |db| db
            .connection(&rdp)
            .unwrap()
            .unwrap()
            .ssh_key_passphrase_enc),
        None
    );
}

#[test]
fn drafts_are_validated_in_rust() {
    let env = Env::new();
    let core = env.unlocked();
    let check = |id: Option<&str>, entry: &EntryDraft, draft: ConnectionDraft| {
        save_single(&core, id, entry, draft, None, None).unwrap_err().code
    };
    let ssh = || ConnectionDraft::new(ConnectionType::Ssh);
    assert_eq!(check(None, &entry_draft("  ", HOST), ssh()), "emptyName");
    let mut bad = ssh();
    bad.port = "70000".into();
    assert_eq!(check(None, &entry_draft("x", HOST), bad), "invalidPort");
    assert_eq!(
        check(
            None,
            &entry_draft("site", "has space"),
            ConnectionDraft::new(ConnectionType::Web)
        ),
        "invalidUrl"
    );
    assert_eq!(check(Some("missing"), &entry_draft("x", HOST), ssh()), "notFound");
    assert_eq!(
        core.save_credential(None, &CredentialInput::default(), None, None)
            .unwrap_err()
            .code,
        "emptyName"
    );
    assert!(core.library().connections.is_empty());
}

#[test]
fn copy_password_resolves_the_linked_credential() {
    let env = Env::new();
    let core = env.unlocked();
    let id = connection(&core, "srv", None);
    let key = credential(&core, "domain admin", Some("cred-pw"));
    assert_eq!(
        core.set_credential(Some(&key), &[], std::slice::from_ref(&id))
            .unwrap(),
        1
    );
    assert_eq!(core.password_for_copy(&id).unwrap().expose_secret(), "cred-pw");
    assert_eq!(core.password_for_copy("missing").unwrap_err().code, "notFound");
}

fn credential_with_key(
    core: &AppCore,
    id: Option<&str>,
    path: &str,
    passphrase: Option<&str>,
) -> Result<String, CommandError> {
    let input = CredentialInput {
        name: "deploy".into(),
        username: "deploy".into(),
        ssh_key_path: path.into(),
        ..CredentialInput::default()
    };
    core.save_credential(id, &input, None, passphrase)
}

#[test]
fn credential_save_generates_the_unlocked_key() {
    let env = Env::new();
    let core = env.unlocked();
    let key_path = env.dir.path().join("id_ed25519");
    let original = write_encrypted_key(&key_path, "key-pass");
    let path = key_path.to_str().unwrap();

    let err = credential_with_key(&core, None, path, Some("wrong")).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("wrongKeyPassphrase", "Wrong key passphrase.")
    );

    let id = credential_with_key(&core, None, path, Some("key-pass")).unwrap();
    let unlocked = |core: &AppCore| {
        record(core, |db| {
            db.credential(&id).unwrap().unwrap().ssh_key_unlocked_enc
        })
    };
    let sealed = unlocked(&core).expect("unlocked key stored");
    let pem = core.inner().vault.as_ref().unwrap().open(&sealed).unwrap();
    let parsed = PrivateKey::from_openssh(pem.expose_secret().as_bytes()).unwrap();
    assert!(!parsed.is_encrypted());
    assert_eq!(parsed.public_key(), original.public_key());
    let dto = core
        .library()
        .credentials
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert!(dto.has_unlocked_key && dto.has_passphrase);

    // The file is gone: same path and passphrase keep the stored copy (the passphrase comes from the vault).
    std::fs::remove_file(&key_path).unwrap();
    credential_with_key(&core, Some(&id), path, None).unwrap();
    assert_eq!(unlocked(&core), Some(sealed.clone()));
    credential_with_key(&core, Some(&id), path, Some("key-pass")).unwrap();
    assert_eq!(unlocked(&core), Some(sealed));

    // A changed passphrase or path can't use the stored copy.
    let err = credential_with_key(&core, Some(&id), path, Some("other")).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.detail.as_deref()),
        ("cannotReadKey", Some(path))
    );
    let moved = format!("{path}.moved");
    let err = credential_with_key(&core, Some(&id), &moved, None).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.detail.as_deref()),
        ("cannotReadKey", Some(moved.as_str()))
    );

    // No passphrase: no unlocked key.
    credential_with_key(&core, Some(&id), path, Some("")).unwrap();
    assert_eq!(unlocked(&core), None);

    // Not a key file.
    std::fs::write(&key_path, "garbage").unwrap();
    let err = credential_with_key(&core, Some(&id), path, Some("x")).unwrap_err();
    assert_eq!(err.code, "cannotUnlockKey");
}

// Library operations

#[test]
fn bulk_delete_counts_each_kind() {
    let env = Env::new();
    let core = env.unlocked();
    let parent = folder(&core, "Parent", None);
    let child = folder(&core, "Child", Some(&parent));
    let inside = connection(&core, "inside", Some(&child));
    let loose = connection(&core, "loose", None);
    let key = credential(&core, "key", None);
    let split = core
        .save_saved_split(None, "pair", &[inside.clone(), loose.clone()])
        .unwrap();

    let counts = core
        .delete_items(&tags(&[
            &format!("c:{loose}"),
            &format!("fav:{}", entry_of(&core, &loose)),
            &format!("f:{parent}"),
            &format!("k:{key}"),
            &format!("s:{split}"),
            "c:missing",
            "bogus",
        ]))
        .unwrap();
    assert_eq!(
        counts,
        DeleteCountsDto {
            entries: 1,
            connections: 1,
            folders: 1,
            credentials: 1,
            splits: 1
        }
    );
    let library = core.library();
    assert!(library.folders.is_empty(), "subfolders cascade");
    assert!(library.credentials.is_empty() && library.saved_splits.is_empty());
    assert_eq!(library.connections.len(), 1);
    assert_eq!(library.connections[0].id, inside);
    assert_eq!(library.connections[0].folder_id, None, "entries move to the root");
}

#[test]
fn move_refuses_cycles_and_moves_the_rest() {
    let env = Env::new();
    let core = env.unlocked();
    let parent = folder(&core, "Parent", None);
    let child = folder(&core, "Child", Some(&parent));
    let other = folder(&core, "Other", None);
    let loose = connection(&core, "loose", None);

    let result = core
        .move_items(
            &tags(&[
                &format!("f:{parent}"),
                &format!("fav:{}", entry_of(&core, &loose)),
                &format!("f:{other}"),
            ]),
            Some(&child),
        )
        .unwrap();
    assert_eq!(
        result,
        MoveResultDto {
            moved: 2,
            cycle_refused: 1
        }
    );
    let library = core.library();
    let parent_of = |id: &str| {
        library
            .folders
            .iter()
            .find(|f| f.id == id)
            .unwrap()
            .parent_id
            .clone()
    };
    assert_eq!(parent_of(&parent), None);
    assert_eq!(parent_of(&other), Some(child.clone()));
    assert_eq!(library.connections[0].folder_id, Some(child.clone()));
    assert_eq!(library.connections[0].folder_path, "Parent / Child");

    // Onto itself: skipped. To the root: allowed.
    assert_eq!(
        core.move_items(&tags(&[&format!("f:{child}")]), Some(&child))
            .unwrap(),
        MoveResultDto::default()
    );
    assert_eq!(
        core.move_items(&tags(&[&format!("c:{loose}")]), None)
            .unwrap()
            .moved,
        1
    );
}

#[test]
fn set_credential_covers_folder_subtrees() {
    let env = Env::new();
    let core = env.unlocked();
    let parent = folder(&core, "Parent", None);
    let child = folder(&core, "Child", Some(&parent));
    let a = connection(&core, "a", Some(&child));
    let b = connection(&core, "b", Some(&parent));
    let c = connection(&core, "c", None);
    let key = credential(&core, "key", Some("pw"));

    let selection = tags(&[&format!("f:{parent}")]);
    assert_eq!(
        core.set_credential(Some(&key), &selection, &[c.clone(), a.clone()])
            .unwrap(),
        3
    );
    assert_eq!(core.set_credential(Some(&key), &selection, &[]).unwrap(), 0);
    let library = core.library();
    assert_eq!(library.credentials[0].usage_count, 3);
    assert!(library.connections.iter().all(|x| x.resolved.has_password));
    assert_eq!(library.connections[0].resolved.username, "admin");

    assert_eq!(
        core.set_credential(None, &tags(&[&format!("c:{b}")]), &[])
            .unwrap(),
        1
    );
    assert_eq!(core.library().credentials[0].usage_count, 2);
    assert_eq!(
        core.set_credential(Some("missing"), &[], &[c]).unwrap_err().code,
        "database"
    );
}

#[test]
fn duplicate_copies_everything_but_identity() {
    let env = Env::new();
    let core = env.unlocked();
    let mut entry = entry_draft("srv", HOST);
    entry.is_favorite = true;
    let id = save_single(
        &core,
        None,
        &entry,
        ConnectionDraft::new(ConnectionType::Ssh),
        Some("pw"),
        None,
    )
    .unwrap();
    let copy = core.duplicate_entry(&entry_of(&core, &id), " (copy)").unwrap();
    let library = core.library();
    let copy = library.entries.iter().find(|e| e.id == copy).unwrap();
    let dup = library
        .connections
        .iter()
        .find(|c| c.id == copy.connection_ids[0])
        .unwrap();
    assert_ne!(dup.id, id);
    assert_eq!(dup.name, "srv (copy)");
    assert!(dup.is_favorite && dup.has_password);
    assert_eq!(core.password_for_copy(&dup.id).unwrap().expose_secret(), "pw");
}

#[test]
fn favorites_folders_and_search() {
    let env = Env::new();
    let core = env.unlocked();
    let prod = folder(&core, "Prod", None);
    let web2 = connection(&core, "web2", Some(&prod));
    let web10 = connection(&core, "web10", None);
    let db = connection(&core, "db", None);
    let (web2_entry, web10_entry) = (entry_of(&core, &web2), entry_of(&core, &web10));
    core.set_favorite(&format!("e:{web10_entry}"), true).unwrap();
    assert_eq!(core.set_favorite("e:missing", true).unwrap_err().code, "notFound");
    assert_eq!(core.set_favorite("k:x", true).unwrap_err().code, "notFound");
    let key = credential(&core, "Web admin", None);

    let found = core.search("WEB");
    assert_eq!(found.entries, [web2_entry.clone(), web10_entry.clone()]);
    assert_eq!(found.credentials, std::slice::from_ref(&key));
    assert_eq!(core.search("prod").entries, std::slice::from_ref(&web2_entry));
    assert!(core.search("   ").entries.is_empty());
    assert_eq!(core.link_candidates(&key, "").len(), 3);
    assert_eq!(
        core.matching(&[db.clone(), web2.clone()], ""),
        [db.clone(), web2.clone()]
    );
    assert_eq!(core.matching(&[db, web2.clone()], "prod"), [web2]);

    let library = core.library();
    assert_eq!(library.favorites, [web10_entry]);
    assert_eq!(library.flattened_folders.len(), 1);
    assert_eq!(library.tree.len(), 3, "the folder, then two root entries");

    // Rename and recolor keep the parent.
    let renamed = core
        .save_folder(&FolderInput {
            id: Some(prod.clone()),
            name: " Production ".into(),
            color: Some(rd_core::LabelColor::Red),
            ..FolderInput::default()
        })
        .unwrap();
    assert_eq!(renamed, prod);
    let library = core.library();
    assert_eq!(library.folders[0].name, "Production");
    assert_eq!(library.folders[0].color, Some(rd_core::LabelColor::Red));
}

#[test]
fn saved_splits_save_rename_reorder_delete() {
    let env = Env::new();
    let core = env.unlocked();
    let ids: Vec<String> = (1..=5)
        .map(|i| connection(&core, &format!("s{i}"), None))
        .collect();
    let first = core.save_saved_split(None, " four ", &ids).unwrap();
    let second = core.save_saved_split(None, "two", &ids[..2]).unwrap();
    let library = core.library();
    assert_eq!(library.saved_splits[0].name, "four");
    assert_eq!(library.saved_splits[0].connection_ids.len(), 4);

    core.rename_saved_split(&first, "renamed").unwrap();
    assert_eq!(
        core.rename_saved_split(&first, " ").unwrap_err().code,
        "emptyName"
    );
    assert_eq!(
        core.rename_saved_split("missing", "x").unwrap_err().code,
        "notFound"
    );
    core.reorder_saved_splits(&[second.clone(), first.clone()])
        .unwrap();
    let order: Vec<String> = core.library().saved_splits.into_iter().map(|s| s.id).collect();
    assert_eq!(order, [second.clone(), first.clone()]);

    core.save_saved_split(Some(&second), "two", &ids[2..4]).unwrap();
    assert_eq!(core.library().saved_splits[0].connection_ids, ids[2..4]);
    core.delete_saved_split(&first).unwrap();
    assert_eq!(core.library().saved_splits.len(), 1);
}

#[test]
fn dtos_carry_no_secrets() {
    let env = Env::new();
    let core = env.unlocked();
    let key_path = env.dir.path().join("id_ed25519");
    write_encrypted_key(&key_path, "PASSPHRASE-PLAIN-3");
    let id = save_single(
        &core,
        None,
        &entry_draft("srv", HOST),
        ConnectionDraft::new(ConnectionType::Ssh),
        Some("PASSWORD-PLAIN-1"),
        Some("PASSPHRASE-PLAIN-2"),
    )
    .unwrap();
    let cred = core
        .save_credential(
            None,
            &CredentialInput {
                name: "k".into(),
                ssh_key_path: key_path.to_str().unwrap().into(),
                ..CredentialInput::default()
            },
            Some("PASSWORD-PLAIN-4"),
            Some("PASSPHRASE-PLAIN-3"),
        )
        .unwrap();

    let json = serde_json::to_string(&core.library()).unwrap();
    for plain in ["PLAIN-1", "PLAIN-2", "PLAIN-3", "PLAIN-4", "OPENSSH PRIVATE KEY"] {
        assert!(!json.contains(plain), "{plain} leaked");
    }
    assert!(!json.contains("Enc\""), "no sealed fields");
    let connection = record(&core, |db| db.connection(&id).unwrap().unwrap());
    let credential = record(&core, |db| db.credential(&cred).unwrap().unwrap());
    let sealed = [
        connection.password_enc,
        connection.ssh_key_passphrase_enc,
        credential.password_enc,
        credential.ssh_key_passphrase_enc,
        credential.ssh_key_unlocked_enc,
    ];
    for bytes in sealed.iter().map(|s| s.as_ref().expect("sealed").as_bytes()) {
        let as_array = serde_json::to_string(&bytes[..8]).unwrap();
        assert!(!json.contains(as_array.trim_end_matches(']')));
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["connections"][0]["hasPassword"], true);
    assert_eq!(value["connections"][0]["hasPassphrase"], true);
    assert_eq!(value["credentials"][0]["hasUnlockedKey"], true);
    assert_eq!(value["connections"][0]["type"], "ssh");
    assert_eq!(value["connections"][0]["effectivePort"], 22);
}

// Settings and shortcuts

#[test]
fn update_check_schedule_follows_the_settings() {
    let env = Env::new();
    let core = env.core();
    let checked = Timestamp::from_unix(1_800_000_000);
    assert!(core.update_check_due(checked), "never checked");
    let settings = core.mark_update_checked(checked).unwrap();
    assert_eq!(settings.last_update_check, Some(checked));
    assert_eq!(
        Settings::load(&env.settings_dir()).last_update_check,
        Some(checked)
    );
    assert!(!core.update_check_due(Timestamp::from_unix(1_800_000_000 + 3600)));
    assert!(core.update_check_due(Timestamp::from_unix(1_800_000_000 + 24 * 3600)));
    core.settings_set(&SettingsPatch {
        check_for_updates_automatically: Some(false),
        ..SettingsPatch::default()
    })
    .unwrap();
    assert!(!core.update_check_due(Timestamp::from_unix(1_800_000_000 + 48 * 3600)));
}

#[test]
fn settings_patch_is_validated_and_persisted() {
    let env = Env::new();
    let core = env.core();
    let settings = core
        .settings_set(&SettingsPatch {
            language: Some("hu".into()),
            external_terminal: Some("example-term".into()),
            check_for_updates_automatically: Some(false),
            expanded_folders: Some(vec!["B".into(), "A".into(), "B".into()]),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(settings.language, "hu");
    assert_eq!(settings.expanded_folders, ["A", "B"]);
    let stored = Settings::load(&env.settings_dir());
    assert_eq!(stored.language, "hu");
    assert_eq!(stored.external_terminal.as_deref(), Some("example-term"));
    assert!(!stored.check_for_updates_automatically);

    let err = core
        .settings_set(&SettingsPatch {
            language: Some("de".into()),
            check_for_updates_automatically: Some(true),
            ..SettingsPatch::default()
        })
        .unwrap_err();
    assert_eq!(err.code, "invalidLanguage");
    assert!(
        !core.settings_get().check_for_updates_automatically,
        "nothing changes on error"
    );
    let cleared = core
        .settings_set(&SettingsPatch {
            external_terminal: Some(String::new()),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(cleared.external_terminal, None);

    assert_eq!(core.settings_get().selection_shows, "details");
    let tab = core
        .settings_set(&SettingsPatch {
            selection_shows: Some("tab".into()),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(tab.selection_shows, "tab");
    assert_eq!(Settings::load(&env.settings_dir()).selection_shows, "tab");
    let err = core
        .settings_set(&SettingsPatch {
            selection_shows: Some("bogus".into()),
            check_for_updates_automatically: Some(true),
            ..SettingsPatch::default()
        })
        .unwrap_err();
    assert_eq!(err.code, "invalidSetting");
    assert_eq!(
        core.settings_get().selection_shows,
        "tab",
        "nothing changes on error"
    );
    assert!(!core.settings_get().check_for_updates_automatically);
}

#[test]
fn shortcuts_set_reset_and_persist() {
    let env = Env::new();
    let core = env.core();
    let rows = core.shortcuts_get();
    assert_eq!(rows.len(), 21);
    assert_eq!(rows[0].group, "file");
    assert_eq!(rows.last().unwrap().group, "connection");
    let lock = rows.iter().find(|r| r.command.as_str() == "lock").unwrap();
    assert_eq!(lock.display.as_deref(), Some("⌃⌘L"));
    assert!(!lock.customized);

    let cmd_ctrl = Modifiers::COMMAND | Modifiers::CONTROL;
    let rows = core
        .shortcut_set("lock", Some(KeyShortcut::new("J", cmd_ctrl)))
        .unwrap();
    let lock = rows.iter().find(|r| r.command.as_str() == "lock").unwrap();
    assert_eq!((lock.display.as_deref(), lock.customized), (Some("⌃⌘J"), true));
    assert_eq!(lock.default_display, "⌃⌘L");
    assert_eq!(
        Settings::load(&env.settings_dir()).keyboard_shortcuts,
        serde_json::json!({"lock": {"key": "j", "modifiers": 9}})
    );

    let err = core
        .shortcut_set("newSSH", Some(KeyShortcut::new("j", cmd_ctrl)))
        .unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.detail.as_deref()),
        ("inUse", Some("lock"))
    );
    assert_eq!(
        core.shortcut_set("bogus", None).unwrap_err().code,
        "unknownCommand"
    );
    assert_eq!(
        core.shortcut_set("lock", Some(KeyShortcut::new("q", Modifiers::COMMAND)))
            .unwrap_err()
            .code,
        "reserved"
    );

    let rows = core.shortcut_set("connect", None).unwrap();
    assert!(
        rows.iter()
            .any(|r| r.command.as_str() == "connect" && r.shortcut.is_none())
    );
    core.shortcut_reset("lock").unwrap();
    assert!(core.shortcuts_reset_all().unwrap().iter().all(|r| !r.customized));
    assert_eq!(
        Settings::load(&env.settings_dir()).keyboard_shortcuts,
        serde_json::json!({})
    );
}

#[test]
fn windows_shortcuts_refuse_ctrl_alt_letters() {
    let env = Env::new();
    let core = env.core_for("connections.sqlite", Platform::Windows);
    let rows = core.shortcuts_get();
    assert_eq!(rows[0].display.as_deref(), Some("Ctrl+Shift+N"));
    let err = core
        .shortcut_set(
            "lock",
            Some(KeyShortcut::new("v", Modifiers::CONTROL | Modifiers::OPTION)),
        )
        .unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("altGr", "Ctrl+Alt acts as AltGr on some keyboard layouts.")
    );
    assert_eq!(core.status().platform, Platform::Windows);
}

#[test]
fn status_serializes_for_the_ui() {
    let env = Env::new();
    let core = env.core();
    let json = serde_json::to_value(core.status()).unwrap();
    assert_eq!(
        json["phase"],
        serde_json::json!({"kind": "locked", "needsSetup": true})
    );
    assert_eq!(json["platform"], "macos");
    assert_eq!(json["rememberMasterKey"], true);
    assert_eq!(json["handoverNotice"], false);
    assert_eq!(core.running_count(), 0);
}

#[test]
fn the_first_launch_after_swift_opens_its_database_and_shows_the_banner_until_dismissed() {
    // The Swift app's database, with a vault, somewhere other than the default path.
    let swift = Env::new();
    drop(swift.unlocked());
    let plist = swift.dir.path().join("swift.plist");
    let mut defaults = plist::Dictionary::new();
    defaults.insert(
        "databasePath".into(),
        plist::Value::String(swift.db("connections.sqlite").display().to_string()),
    );
    defaults.insert(
        "AppleLanguages".into(),
        plist::Value::Array(vec![plist::Value::String("hu".into())]),
    );
    plist::Value::Dictionary(defaults).to_file_binary(&plist).unwrap();

    let env = Env::new();
    assert!(rd_platform::swift_defaults::import_swift_defaults(&env.settings_dir(), &plist).is_some());
    let core = AppCore::with_options(
        env.settings_dir(),
        Box::new(env.keys.clone()),
        1_000,
        Platform::MacOS,
    );
    core.mark_handover();
    core.open_current();

    let status = core.status();
    assert_eq!(status.phase, LOCKED, "the existing vault is found");
    assert_eq!(
        status.database_path,
        Some(swift.db("connections.sqlite").display().to_string())
    );
    assert!(status.handover_notice);
    assert_eq!(core.settings_get().language, "hu");

    assert!(!core.dismiss_handover().handover_notice);
    assert!(!core.status().handover_notice);
}

// Import

/// A WinBox address book with made-up entries: `(name, group, password, user, address)`.
fn cdb(entries: &[(&str, &str, &str, &str, &str)]) -> Vec<u8> {
    let field = |id: u32, value: &str| {
        let mut out = id.to_le_bytes()[..3].to_vec();
        out.push(0x21);
        out.push(u8::try_from(value.len()).unwrap());
        out.extend_from_slice(value.as_bytes());
        out
    };
    let mut file = vec![0x0D, 0xF0, 0x1D, 0xC0];
    for (name, group, password, user, address) in entries {
        let mut body = b"M2".to_vec();
        for (id, value) in [
            (8, group),
            (0x00FE_0009, name),
            (3, password),
            (2, user),
            (1, address),
        ] {
            body.extend(field(id, value));
        }
        file.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
        file.extend(body);
    }
    file
}

fn write_file(env: &Env, name: &str, data: &[u8]) -> PathBuf {
    let path = env.dir.path().join(name);
    std::fs::write(&path, data).unwrap();
    path
}

const RDM_SAMPLE: &str = include_str!("../../../../crates/rd-core/tests/fixtures/rdm-sample.rdm");

#[test]
fn winbox_preview_then_commit_the_selection() {
    let env = Env::new();
    let core = env.unlocked();
    let path = write_file(
        &env,
        "book.bin",
        &cdb(&[
            ("core-rtr", "Branch East", "s3cret-pw", "admin", "192.0.2.1"),
            ("edge", "", "", "admin", "192.0.2.2:8292"),
        ]),
    );

    let preview = core.import_preview(ImportKind::Winbox, &path).unwrap();
    assert_eq!(preview.file_name, "book.bin");
    assert_eq!(preview.rows.len(), 2);
    let first = &preview.rows[0];
    assert_eq!(
        (
            first.id.as_str(),
            first.kind.as_str(),
            first.name.as_str(),
            first.host.as_str()
        ),
        ("c0", "mikrotik", "core-rtr", "192.0.2.1")
    );
    assert_eq!(first.group_path, ["Branch East"]);
    assert!(first.has_password && !first.duplicate);
    assert!(!preview.rows[1].has_password);
    let json = serde_json::to_string(&preview).unwrap();
    assert!(!json.contains("s3cret-pw"), "the preview carries no password");
    assert!(json.contains("\"groupPath\"") && json.contains("\"hasPassword\""));

    let result = core
        .import_commit(ImportKind::Winbox, &path, &tags(&["c0"]))
        .unwrap();
    assert_eq!((result.connections, result.folders_created), (1, 1));
    let library = record(&core, |db| db.library().unwrap());
    let imported = library.connections.iter().find(|c| c.name == "core-rtr").unwrap();
    assert_eq!(imported.kind, ConnectionType::Mikrotik);
    // Each imported item is an entry of its own with that one connection.
    let entry = library.entry(&imported.entry_id).unwrap();
    assert_eq!(entry.name, "core-rtr");
    assert_eq!(library.connections_of(&entry.id).len(), 1);
    assert_eq!(
        library.default_connection(&entry.id).map(|c| c.id.as_str()),
        Some(imported.id.as_str())
    );
    assert_eq!(
        core.password_for_copy(&imported.id).unwrap().expose_secret(),
        "s3cret-pw"
    );

    // The imported entry is now a duplicate; the other one is not.
    let again = core.import_preview(ImportKind::Winbox, &path).unwrap();
    assert_eq!(
        again.rows.iter().map(|r| r.duplicate).collect::<Vec<_>>(),
        [true, false]
    );
    // Unknown ids are ignored; the group folder is reused.
    let second = core
        .import_commit(ImportKind::Winbox, &path, &tags(&["c1", "c9", "k0"]))
        .unwrap();
    assert_eq!((second.connections, second.folders_created), (1, 0));
}

#[test]
fn rdm_preview_lists_credentials_and_skipped_entries() {
    let env = Env::new();
    let core = env.unlocked();
    let existing_credential = credential(&core, "Shared", None);
    let xml = RDM_SAMPLE.replace(
        "</ArrayOfConnection>",
        "<Connection><ConnectionType>Credential</ConnectionType><ID>K1</ID><Name>Shared</Name>\
         <Credentials><UserName>admin</UserName><Password>pw</Password></Credentials></Connection>\
         <Connection><ConnectionType>RDP</ConnectionType><Name>linked</Name><Url>linked.example.com</Url>\
         <CredentialConnectionID>K1</CredentialConnectionID></Connection></ArrayOfConnection>",
    );
    let path = write_file(&env, "export.rdm", xml.as_bytes());

    let preview = core.import_preview(ImportKind::Rdm, &path).unwrap();
    let ids: Vec<_> = preview.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["c0", "c1", "c2", "c3", "k0"]);
    let credential_row = &preview.rows[4];
    assert_eq!(credential_row.kind, "credential");
    assert!(credential_row.duplicate, "same name, user and domain");
    assert_eq!(preview.skipped.len(), 2);
    assert_eq!((preview.without_password, preview.empty_groups), (1, 1));
    let dc = &preview.rows[0];
    assert_eq!((dc.kind.as_str(), dc.port), ("rdp", Some(3390)));

    // The duplicate credential is not imported again; the linked connection uses the existing one.
    let result = core
        .import_commit(ImportKind::Rdm, &path, &tags(&["c0", "c3"]))
        .unwrap();
    assert_eq!(
        (result.connections, result.credentials, result.folders_created),
        (2, 0, 3)
    );
    assert_eq!((result.skipped, result.without_password), (2, 1));
    let library = record(&core, |db| db.library().unwrap());
    let linked = library.connections.iter().find(|c| c.name == "linked").unwrap();
    assert_eq!(
        linked.credential_id.as_deref(),
        Some(existing_credential.as_str())
    );
    assert_eq!(library.credentials.len(), 1);
}

#[test]
fn import_errors() {
    let env = Env::new();
    let core = env.unlocked();
    let garbage = write_file(&env, "garbage.bin", b"hello");
    let missing = env.dir.path().join("missing.cdb");
    let empty_book = write_file(&env, "empty.bin", &cdb(&[]));
    let empty_rdm = write_file(&env, "empty.rdm", b"<ArrayOfConnection/>");
    let refusal = |kind, path: &Path| core.import_preview(kind, path).unwrap_err().code;
    assert_eq!(refusal(ImportKind::Winbox, &garbage), "importUnreadable");
    assert_eq!(refusal(ImportKind::Rdm, &garbage), "importUnreadable");
    assert_eq!(refusal(ImportKind::Winbox, &missing), "importRead");
    assert_eq!(refusal(ImportKind::Winbox, &empty_book), "importEmpty");
    assert_eq!(refusal(ImportKind::Rdm, &empty_rdm), "importEmpty");
    assert_eq!(
        core.import_commit(ImportKind::Winbox, &garbage, &[])
            .unwrap_err()
            .code,
        "importUnreadable"
    );

    core.lock();
    assert_eq!(refusal(ImportKind::Winbox, &garbage), "locked");
    assert_eq!(
        core.import_commit(ImportKind::Winbox, &garbage, &[])
            .unwrap_err()
            .code,
        "locked"
    );
}

#[test]
fn import_is_refused_read_only() {
    let env = Env::new();
    let path = env.db("connections.sqlite");
    drop(env.unlocked());
    write_foreign_lock(&path);
    let core = env.core();
    core.resolve_db_lock(LockChoice::ReadOnly).unwrap();
    let book = write_file(&env, "book.bin", &cdb(&[("r", "", "", "", "192.0.2.3")]));
    assert_eq!(
        core.import_preview(ImportKind::Winbox, &book).unwrap_err().code,
        "readOnly"
    );
    assert_eq!(
        core.import_commit(ImportKind::Winbox, &book, &tags(&["c0"]))
            .unwrap_err()
            .code,
        "readOnly"
    );
}
