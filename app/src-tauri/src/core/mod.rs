//! The app's state and flows in plain Rust, testable without Tauri: opening the database, the lock file,
//! the vault and the keyring policy (a port of the Swift `AppState`), plus the library, settings and
//! shortcut operations in the submodules. The Tauri commands are thin wrappers that call these methods
//! and emit events.

mod error;
mod files;
#[cfg(test)]
mod files_tests;
mod forwards;
#[cfg(test)]
mod forwards_tests;
mod import;
mod launch;
#[cfg(test)]
mod launch_tests;
mod library;
mod rdp;
#[cfg(test)]
mod rdp_tests;
mod settings;
mod ssh;
mod ssh_keys;
#[cfg(test)]
mod ssh_keys_tests;
#[cfg(all(test, unix))]
mod ssh_tests;
pub mod ssh_unlock;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use rd_core::shortcuts::Platform;
use rd_platform::Settings;
use rd_store::{Database, DbLock, KeyCache, LockStatus, Vault};
use serde::Deserialize;

pub use error::CommandError;
pub use files::{
    BrowserState, FilesConflictDto, FilesDoneDto, FilesHostKeyDto, FilesNotice, FilesNotifier, FilesOpenDto,
    FilesProgressDto, FilesStatusDto, ListingDto, TransferInput, browser_id, is_files_label,
};
pub use forwards::{ForwardNotice, ForwardNotifier, ForwardStatusDto, HostKeyPromptDto};
pub use launch::{LaunchNoticeDto, LaunchNotifier, LaunchOutcomeDto, WinboxStatusDto};
pub use library::{CredentialInput, FolderInput};
pub use rdp::{
    RdpCertPromptDto, RdpCredentialsAnswer, RdpCredentialsPromptDto, RdpEngineInfoDto, RdpInput, RdpNotice,
    RdpNotifier, RdpRun, RdpSink, RdpStarter, TypeClipboardDto,
};
pub use settings::SettingsPatch;
pub use ssh::{AskpassNotice, AskpassNotifier, AskpassPromptDto, EXTERNAL_LIFETIME, SessionsChanged};
pub use ssh_keys::{KeySetupDto, KeySetupNotifier};

use crate::dto::{AppStatusDto, LockHolderDto, Phase};

/// The answer to a lock conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LockChoice {
    /// Open without taking the lock; every write is refused.
    ReadOnly,
    /// Take the lock over from the other instance.
    OpenAnyway,
    /// Don't open; show the error screen.
    Cancel,
}

/// Everything behind the mutex.
struct Inner {
    settings: Settings,
    settings_dir: PathBuf,
    db_path: Option<PathBuf>,
    db: Option<Database>,
    vault: Option<Vault>,
    lock: Option<DbLock>,
    read_only: bool,
    phase: Phase,
    /// The Swift settings were imported at this launch and the welcome banner hasn't been dismissed.
    handover: bool,
}

/// The app core. One per process; shared by the Tauri commands behind an `Arc`.
pub struct AppCore {
    inner: Mutex<Inner>,
    keys: Box<dyn KeyCache>,
    iterations: u32,
    platform: Platform,
    ssh: std::sync::Arc<ssh::SshShared>,
    forwards: std::sync::Arc<forwards::ForwardShared>,
    files: std::sync::Arc<files::FilesShared>,
    rdp: std::sync::Arc<rdp::RdpShared>,
    key_setup: std::sync::Arc<ssh_keys::KeySetupShared>,
    launcher: Box<dyn rd_platform::launch::Launcher>,
    launch_notifier: Mutex<Option<LaunchNotifier>>,
    /// This build's version: stamped into the database, and an older build than the stamp is refused.
    app_version: String,
}

impl std::fmt::Debug for AppCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppCore(..)")
    }
}

/// The keyring account for a database: its absolute path.
fn account(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn failed(code: &str, error: impl Into<String>) -> Phase {
    Phase::NoDatabase {
        code: code.to_owned(),
        error: error.into(),
    }
}

impl AppCore {
    /// A core reading `settings.json` from `settings_dir`, remembering keys in `keys`. Nothing is opened
    /// until [`open_current`](Self::open_current).
    #[must_use]
    pub fn new(settings_dir: PathBuf, keys: Box<dyn KeyCache>) -> Self {
        Self::with_options(settings_dir, keys, Vault::DEFAULT_ITERATIONS, Platform::current())
    }

    /// Like [`new`](Self::new) with the PBKDF2 rounds for new vaults and the shortcut platform (tests).
    #[must_use]
    pub fn with_options(
        settings_dir: PathBuf,
        keys: Box<dyn KeyCache>,
        iterations: u32,
        platform: Platform,
    ) -> Self {
        Self {
            inner: Mutex::new(Inner {
                settings: Settings::load(&settings_dir),
                settings_dir,
                db_path: None,
                db: None,
                vault: None,
                lock: None,
                read_only: false,
                phase: failed("noPath", "No database is open."),
                handover: false,
            }),
            keys,
            iterations,
            platform,
            ssh: ssh::SshShared::new(),
            forwards: forwards::ForwardShared::new(),
            files: files::FilesShared::new(),
            rdp: rdp::RdpShared::new(),
            key_setup: ssh_keys::KeySetupShared::new(),
            launcher: Box::new(rd_platform::launch::SystemLauncher),
            launch_notifier: Mutex::new(None),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    /// Pretends to be another app version (tests of the database version guard).
    #[must_use]
    pub fn with_app_version(mut self, version: &str) -> Self {
        version.clone_into(&mut self.app_version);
        self
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The state for the root view.
    #[must_use]
    pub fn status(&self) -> AppStatusDto {
        let inner = self.inner();
        self.status_of(&inner)
    }

    fn status_of(&self, inner: &Inner) -> AppStatusDto {
        AppStatusDto {
            phase: inner.phase.clone(),
            database_path: inner.db_path.as_ref().map(|p| p.display().to_string()),
            read_only: inner.read_only,
            remember_master_key: inner.settings.remember_master_key,
            platform: self.platform,
            handover_notice: inner.handover,
        }
    }

    /// This launch is the first after the Swift app: the Swift settings were just imported, so the main
    /// window shows the welcome banner.
    pub fn mark_handover(&self) {
        self.inner().handover = true;
    }

    /// Hides the welcome banner for the rest of this launch.
    #[must_use]
    pub fn dismiss_handover(&self) -> AppStatusDto {
        let mut inner = self.inner();
        inner.handover = false;
        self.status_of(&inner)
    }

    /// The database file in use, if any.
    #[must_use]
    pub fn database_path(&self) -> Option<PathBuf> {
        self.inner().db_path.clone()
    }

    // Opening

    /// Opens the configured database (or the default one): on startup and for "Retry".
    pub fn open_current(&self) -> AppStatusDto {
        let mut inner = self.inner();
        if let Some(path) = inner.settings.database_path() {
            self.open_at(&mut inner, &path);
        } else {
            self.close_inner(&mut inner, false);
            inner.phase = failed("noPath", "There is no default database location on this system.");
        }
        self.status_of(&inner)
    }

    /// Switches to an existing database file and remembers it in the settings.
    pub fn open_database(&self, path: &Path) -> AppStatusDto {
        let mut inner = self.inner();
        inner.settings.database_path = Some(path.to_path_buf());
        // Like Swift's UserDefaults write, a failed settings save doesn't stop the switch.
        let _ = inner.settings.save(&inner.settings_dir);
        self.open_at(&mut inner, path);
        self.status_of(&inner)
    }

    /// Switches to a new database file (created if missing) and remembers it in the settings. An existing
    /// file at `path` is opened, never truncated, as in the Swift app.
    pub fn create_database(&self, path: &Path) -> AppStatusDto {
        self.open_database(path)
    }

    /// Answers a lock conflict.
    ///
    /// # Errors
    /// `noLockConflict` when the app isn't waiting for that answer.
    pub fn resolve_db_lock(&self, choice: LockChoice) -> Result<AppStatusDto, CommandError> {
        let mut inner = self.inner();
        let Phase::LockConflict { .. } = inner.phase else {
            return Err(CommandError::new(
                "noLockConflict",
                "No lock conflict is pending.",
            ));
        };
        let Some(path) = inner.db_path.clone() else {
            return Err(CommandError::no_database());
        };
        match choice {
            LockChoice::ReadOnly => {
                inner.read_only = true;
                self.finish_open(&mut inner, &path);
            }
            LockChoice::OpenAnyway => match DbLock::force_acquire(&path) {
                Ok(lock) => {
                    inner.lock = Some(lock);
                    self.finish_open(&mut inner, &path);
                }
                Err(e) => inner.phase = failed("openFailed", format!("Can't open the database: {e}")),
            },
            LockChoice::Cancel => {
                inner.phase = failed("lockCancelled", "The database is in use on another computer.");
            }
        }
        Ok(self.status_of(&inner))
    }

    /// Swift `openDatabase`: lock without forgetting the cached key, take the lock file, open, then
    /// unlock from the keyring when possible.
    fn open_at(&self, inner: &mut Inner, path: &Path) {
        self.close_inner(inner, false);
        inner.db_path = Some(path.to_path_buf());
        // A newer app has used this file: refuse before the lock and before migrating, so it stays untouched.
        // A file that can't be read here gets the usual error from the real open below.
        if let Ok(Some(stored)) = rd_store::stored_app_version(path)
            && rd_store::is_newer(&stored, &self.app_version)
        {
            inner.phase = Phase::NeedsUpdate {
                database_version: stored,
                app_version: self.app_version.clone(),
            };
            return;
        }
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            inner.phase = failed("openFailed", format!("Can't open the database: {e}"));
            return;
        }
        match DbLock::acquire(path) {
            Ok(LockStatus::Acquired(lock)) => {
                inner.lock = Some(lock);
                self.finish_open(inner, path);
            }
            Ok(LockStatus::HeldElsewhere(holder)) => {
                inner.phase = Phase::LockConflict {
                    holder: LockHolderDto::from(&holder),
                };
            }
            Err(e) => inner.phase = failed("openFailed", format!("Can't open the database: {e}")),
        }
    }

    fn finish_open(&self, inner: &mut Inner, path: &Path) {
        let opened = Database::open(path).map_err(|e| e.to_string()).and_then(|db| {
            let initialized = Vault::is_initialized(&db).map_err(|e| e.to_string())?;
            Ok((db, initialized))
        });
        let (db, initialized) = match opened {
            Ok(value) => value,
            Err(e) => {
                inner.lock = None;
                inner.phase = failed("openFailed", format!("Can't open the database: {e}"));
                return;
            }
        };
        if !inner.read_only {
            // Like the keyring writes, a failed stamp doesn't stop the open.
            let _ = db.record_app_version(&self.app_version);
        }
        if !initialized {
            inner.db = Some(db);
            inner.phase = Phase::Locked { needs_setup: true };
            return;
        }
        let cached = self
            .keys
            .load(&account(path))
            .ok()
            .flatten()
            .and_then(|key| Vault::unlock_with_key(&db, *key).ok());
        inner.db = Some(db);
        match cached {
            Some(vault) => self.did_unlock(inner, vault),
            None => inner.phase = Phase::Locked { needs_setup: false },
        }
    }

    /// Stops the tunnels and drops the vault, the database and the lock file; `clear_key` also forgets the
    /// cached key.
    fn close_inner(&self, inner: &mut Inner, clear_key: bool) {
        self.stop_all_forwards();
        self.close_all_files();
        inner.vault = None;
        if clear_key && let Some(path) = &inner.db_path {
            let _ = self.keys.delete(&account(path));
        }
        inner.db = None;
        inner.lock = None;
        inner.read_only = false;
    }

    /// On exit: ends the SSH and RDP sessions (deleting the SSH key files) and the tunnels, and releases
    /// the lock file.
    pub fn close(&self) {
        self.shutdown_ssh();
        self.shutdown_rdp();
        let mut inner = self.inner();
        self.close_inner(&mut inner, false);
    }

    /// Refreshes the lock file; called every [`rd_store::REFRESH_EVERY`].
    pub fn refresh_lock(&self) {
        if let Some(lock) = self.inner().lock.as_mut() {
            let _ = lock.refresh();
        }
    }

    // Vault

    /// Swift `didUnlock`: remember or forget the key per the setting, then show the main window.
    fn did_unlock(&self, inner: &mut Inner, vault: Vault) {
        if let Some(path) = &inner.db_path {
            let account = account(path);
            // Keyring failures are ignored, as with Swift's `try?`: the user just types the password again.
            let _ = if inner.settings.remember_master_key {
                self.keys.save(&account, &vault.export_key())
            } else {
                self.keys.delete(&account)
            };
        }
        inner.vault = Some(vault);
        inner.phase = Phase::Unlocked {
            read_only: inner.read_only,
        };
    }

    fn set_remember(inner: &mut Inner, remember: bool) {
        if inner.settings.remember_master_key != remember {
            inner.settings.remember_master_key = remember;
            let _ = inner.settings.save(&inner.settings_dir);
        }
    }

    /// Sets up the vault of a new database.
    ///
    /// # Errors
    /// `noDatabase`, `readOnly`, `emptyPassword`, `vaultAlreadySetUp` or `database`.
    pub fn create_vault(&self, password: &str, remember: bool) -> Result<AppStatusDto, CommandError> {
        let mut inner = self.inner();
        let db = inner.db.as_ref().ok_or_else(CommandError::no_database)?;
        if inner.read_only {
            return Err(CommandError::read_only());
        }
        let vault = Vault::create(db, password, self.iterations)?;
        Self::set_remember(&mut inner, remember);
        self.did_unlock(&mut inner, vault);
        Ok(self.status_of(&inner))
    }

    /// Unlocks with the master password.
    ///
    /// # Errors
    /// `wrongPassword`, `noDatabase`, `vaultNotSetUp` or `database`.
    pub fn unlock(&self, password: &str, remember: bool) -> Result<AppStatusDto, CommandError> {
        let mut inner = self.inner();
        let db = inner.db.as_ref().ok_or_else(CommandError::no_database)?;
        let vault = Vault::unlock(db, password)?;
        Self::set_remember(&mut inner, remember);
        self.did_unlock(&mut inner, vault);
        Ok(self.status_of(&inner))
    }

    /// Locks and forgets the cached key. Sessions are not closed (they are hidden behind the unlock
    /// screen); port forward tunnels and file browsers are stopped.
    pub fn lock(&self) -> AppStatusDto {
        self.stop_all_forwards();
        self.close_all_files();
        let mut inner = self.inner();
        inner.vault = None;
        if let Some(path) = &inner.db_path {
            let _ = self.keys.delete(&account(path));
        }
        if inner.db.is_some() {
            inner.phase = Phase::Locked { needs_setup: false };
        }
        self.status_of(&inner)
    }

    /// Re-keys the vault after checking the current password; the keyring entry follows the new key.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `wrongPassword` ("The current master password is wrong."), `emptyPassword`
    /// or `database`.
    pub fn change_password(&self, current: &str, new: &str) -> Result<(), CommandError> {
        let mut inner = self.inner();
        if inner.vault.is_none() {
            return Err(CommandError::locked());
        }
        if inner.read_only {
            return Err(CommandError::read_only());
        }
        let db = inner.db.as_ref().ok_or_else(CommandError::no_database)?;
        let mut vault = Vault::unlock(db, current).map_err(|e| match e {
            rd_store::VaultError::WrongPassword => {
                CommandError::new("wrongPassword", "The current master password is wrong.")
            }
            other => other.into(),
        })?;
        vault.change_password(db, new)?;
        self.did_unlock(&mut inner, vault);
        Ok(())
    }

    /// The open database and vault, or `noDatabase` / `locked`.
    fn unlocked(inner: &Inner) -> Result<(&Database, &Vault), CommandError> {
        let vault = inner.vault.as_ref().ok_or_else(CommandError::locked)?;
        let db = inner.db.as_ref().ok_or_else(CommandError::no_database)?;
        Ok((db, vault))
    }

    /// Like [`unlocked`](Self::unlocked), and `readOnly` when writes are refused.
    fn writable(inner: &Inner) -> Result<(&Database, &Vault), CommandError> {
        let pair = Self::unlocked(inner)?;
        if inner.read_only {
            return Err(CommandError::read_only());
        }
        Ok(pair)
    }

    /// How many sessions, forwards and transfers are running (for the quit confirmation): the embedded
    /// SSH sessions whose process still runs, the RDP sessions whose engine runs, the starting or active
    /// tunnels and the running file transfers.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.running_ssh_sessions()
            + self.running_rdp_sessions()
            + self.running_forwards()
            + self.running_transfers()
    }
}
