//! The file browser windows (Swift `FileBrowserSession` + `AppState` file browser section): one russh
//! connection per window, listings, one transfer at a time, conflict and host key questions.
//!
//! - A browser is keyed by its window label, `files-<connection id>`, so a connection has one window.
//! - Opened from a terminal tab, it starts in that shell's directory (OSC 7 or the `user@host: dir`
//!   title), else in the remote home folder.
//! - Lock, database switch and quit close every browser (its window too): the connection was opened with
//!   secrets from the vault.
//! - [`running_count`](AppCore::running_count) counts running transfers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use rd_core::{ConnectionType, new_id};
use rd_ssh::background::{Client, HostKeyPrompt, HostKeyQuestion, StoredKey, runtime};
use rd_ssh::files::transfer::{self, ConflictAnswer, ConflictQuestion, Progress, TransferObserver};
use rd_ssh::files::{
    ConflictChoice, Direction, Entry, ItemReport, RemoteFiles, RemoteMode, TransferRequest, local,
    path as rpath,
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::{AppCore, CommandError};

/// The window label (and browser id) prefix.
pub const FILES_LABEL_PREFIX: &str = "files-";

/// The browser id (window label) of a connection: `files-` and the id with anything but letters, digits,
/// `-` and `_` replaced.
#[must_use]
pub fn browser_id(connection_id: &str) -> String {
    let safe: String = connection_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{FILES_LABEL_PREFIX}{safe}")
}

/// A browser's connection state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum BrowserState {
    /// Connecting (or waiting for the host key answer).
    Connecting,
    /// Connected.
    Ready {
        /// Tar streams, SFTP or listings only.
        mode: RemoteMode,
    },
    /// The connection failed or closed.
    Failed {
        /// A [`BackgroundError`](rd_ssh::background::BackgroundError) code or `disconnected`.
        code: String,
        /// The English message.
        error: String,
    },
}

/// What `files_open` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesOpenDto {
    /// The browser id, also the window label.
    pub browser_id: String,
    /// "Files – <connection name>" (the UI translates the prefix; this is the English title).
    pub connection_name: String,
    /// A new browser was created (else the existing window is focused).
    pub created: bool,
}

/// A browser's state (command `files_info`, event `files-status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesStatusDto {
    /// The browser.
    pub browser_id: String,
    /// Its connection.
    pub connection_id: String,
    /// The connection's name.
    pub connection_name: String,
    /// Where the remote pane starts (`~` for the home folder).
    pub start_path: String,
    /// The connection state.
    #[serde(flatten)]
    pub state: BrowserState,
    /// A transfer is running.
    pub transferring: bool,
}

/// A folder listing for a pane.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingDto {
    /// The folder's absolute path.
    pub path: String,
    /// Its parent, `None` at the root.
    pub parent: Option<String>,
    /// The entries, directories first.
    pub entries: Vec<Entry>,
}

/// An unknown or changed host key of a file browser connection (event `files-host-key-prompt`); answer
/// with `host_key_answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesHostKeyDto {
    /// Answer with this id.
    pub request_id: String,
    /// The browser that is connecting.
    pub browser_id: String,
    /// The host the key is stored under.
    pub host: String,
    /// The port.
    pub port: u16,
    /// `ssh-ed25519`, `ssh-rsa`, …
    pub key_type: String,
    /// `SHA256:…`.
    pub fingerprint: String,
    /// `{file, line}` of the old key when it changed (accepting replaces it), else `null`.
    pub changed: Option<StoredKey>,
}

/// Progress of the running transfer (event `files-progress`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesProgressDto {
    /// The browser.
    pub browser_id: String,
    /// The transfer.
    pub transfer_id: String,
    /// Which way.
    pub direction: Direction,
    /// Item, counts and bytes.
    #[serde(flatten)]
    pub progress: Progress,
}

/// "Already exists" (event `files-conflict`); answer with `files_conflict_answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesConflictDto {
    /// The browser.
    pub browser_id: String,
    /// Answer with this id.
    pub request_id: String,
    /// Name, folder flag, direction.
    #[serde(flatten)]
    pub question: ConflictQuestion,
}

/// A finished transfer (event `files-done`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesDoneDto {
    /// The browser.
    pub browser_id: String,
    /// The transfer.
    pub transfer_id: String,
    /// Which way (the destination pane refreshes).
    pub direction: Direction,
    /// Per item results.
    pub items: Vec<ItemReport>,
    /// It was cancelled.
    pub cancelled: bool,
}

/// What a `files_transfer` call asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInput {
    /// `download` (remote → local) or `upload`.
    pub direction: Direction,
    /// Full paths on the source side.
    pub sources: Vec<String>,
    /// The folder on the other side.
    pub destination: String,
    /// gzip the tar stream.
    pub gzip: bool,
}

/// What the file browsers tell the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesNotice {
    /// A browser's state changed.
    Status(FilesStatusDto),
    /// Ask about a host key.
    HostKeyPrompt(FilesHostKeyDto),
    /// Ask about an existing destination.
    Conflict(FilesConflictDto),
    /// Transfer progress.
    Progress(FilesProgressDto),
    /// A transfer finished.
    Done(FilesDoneDto),
    /// The browser was closed by the app (lock, database switch): close its window.
    Closed(String),
}

/// Receives file browser notices (emits the events, closes windows).
pub type FilesNotifier = Arc<dyn Fn(FilesNotice) + Send + Sync>;

struct Running {
    id: String,
    cancel: CancellationToken,
}

struct Browser {
    connection_id: String,
    connection_name: String,
    start_path: String,
    generation: u64,
    state: BrowserState,
    remote: Option<Arc<RemoteFiles>>,
    transfer: Option<Running>,
    /// The VPN the browser goes through, kept up while its window is open.
    vpn: Option<super::VpnHold>,
}

struct Pending<T> {
    browser_id: String,
    answer: oneshot::Sender<T>,
}

/// The browsers and their pending questions.
pub(crate) struct FilesShared {
    browsers: Mutex<HashMap<String, Browser>>,
    host_keys: Mutex<HashMap<String, Pending<bool>>>,
    conflicts: Mutex<HashMap<String, Pending<ConflictAnswer>>>,
    notifier: Mutex<Option<FilesNotifier>>,
    next_generation: Mutex<u64>,
    /// The main window has something "Files (SCP)…" applies to (for the menu).
    target: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_open(browser_id: &str) -> CommandError {
    CommandError::new("browserNotFound", "The file browser is closed.").with_detail(browser_id)
}

fn not_connected() -> CommandError {
    CommandError::new("notConnected", "The file browser isn't connected.")
}

fn listing(path: String, entries: Vec<Entry>, show_hidden: bool) -> ListingDto {
    let parent = rpath::parent(&path);
    ListingDto {
        parent: (parent != path).then_some(parent),
        entries: entries
            .into_iter()
            .filter(|e| show_hidden || !e.name.starts_with('.'))
            .collect(),
        path,
    }
}

impl FilesShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            browsers: Mutex::default(),
            host_keys: Mutex::default(),
            conflicts: Mutex::default(),
            notifier: Mutex::default(),
            next_generation: Mutex::new(0),
            target: AtomicBool::new(false),
        })
    }

    fn notify(&self, notice: FilesNotice) {
        let notifier = lock(&self.notifier).clone();
        if let Some(notifier) = notifier {
            notifier(notice);
        }
    }

    fn status_of(id: &str, browser: &Browser) -> FilesStatusDto {
        FilesStatusDto {
            browser_id: id.to_owned(),
            connection_id: browser.connection_id.clone(),
            connection_name: browser.connection_name.clone(),
            start_path: browser.start_path.clone(),
            state: browser.state.clone(),
            transferring: browser.transfer.is_some(),
        }
    }

    fn notify_status(&self, id: &str) {
        let status = lock(&self.browsers).get(id).map(|b| Self::status_of(id, b));
        if let Some(status) = status {
            self.notify(FilesNotice::Status(status));
        }
    }

    /// The connect result of `generation`; dropped when the browser was reconnected or closed.
    fn connected(&self, id: &str, generation: u64, result: Result<RemoteFiles, BrowserState>) {
        let stale = {
            let mut browsers = lock(&self.browsers);
            match browsers.get_mut(id).filter(|b| b.generation == generation) {
                Some(browser) => {
                    match result {
                        Ok(remote) => {
                            browser.state = BrowserState::Ready { mode: remote.mode() };
                            browser.remote = Some(Arc::new(remote));
                        }
                        Err(state) => browser.state = state,
                    }
                    None
                }
                None => result.ok(),
            }
        };
        if let Some(remote) = stale {
            runtime().spawn(async move { remote.disconnect().await });
            return;
        }
        self.notify_status(id);
    }

    fn ask_host_key(&self, browser_id: &str, question: HostKeyQuestion) -> oneshot::Receiver<bool> {
        let (answer, receiver) = oneshot::channel();
        if lock(&self.notifier).is_none() {
            return receiver;
        }
        let request_id = new_id();
        lock(&self.host_keys).insert(
            request_id.clone(),
            Pending {
                browser_id: browser_id.to_owned(),
                answer,
            },
        );
        self.notify(FilesNotice::HostKeyPrompt(FilesHostKeyDto {
            request_id,
            browser_id: browser_id.to_owned(),
            host: question.host,
            port: question.port,
            key_type: question.key_type,
            fingerprint: question.fingerprint,
            changed: question.changed,
        }));
        receiver
    }

    /// Drops the browser's pending questions (their senders reject / skip).
    fn cancel_questions(&self, browser_id: &str) {
        lock(&self.host_keys).retain(|_, p| p.browser_id != browser_id);
        lock(&self.conflicts).retain(|_, p| p.browser_id != browser_id);
    }

    fn close(&self, browser_id: &str) -> bool {
        let removed = lock(&self.browsers).remove(browser_id);
        self.cancel_questions(browser_id);
        let Some(browser) = removed else {
            return false;
        };
        if let Some(running) = &browser.transfer {
            running.cancel.cancel();
        }
        if let Some(remote) = browser.remote {
            runtime().spawn(async move { remote.disconnect().await });
        }
        true
    }
}

/// Forwards a transfer's progress and conflicts as notices.
struct Observer {
    shared: Weak<FilesShared>,
    browser_id: String,
    transfer_id: String,
    direction: Direction,
}

impl TransferObserver for Observer {
    fn progress(&self, progress: &Progress) {
        if let Some(shared) = self.shared.upgrade() {
            shared.notify(FilesNotice::Progress(FilesProgressDto {
                browser_id: self.browser_id.clone(),
                transfer_id: self.transfer_id.clone(),
                direction: self.direction,
                progress: progress.clone(),
            }));
        }
    }

    fn conflict(&self, question: ConflictQuestion) -> oneshot::Receiver<ConflictAnswer> {
        let (answer, receiver) = oneshot::channel();
        let Some(shared) = self.shared.upgrade() else {
            return receiver;
        };
        let request_id = new_id();
        lock(&shared.conflicts).insert(
            request_id.clone(),
            Pending {
                browser_id: self.browser_id.clone(),
                answer,
            },
        );
        shared.notify(FilesNotice::Conflict(FilesConflictDto {
            browser_id: self.browser_id.clone(),
            request_id,
            question,
        }));
        receiver
    }
}

impl AppCore {
    /// Sets the file browser notifier (the Tauri layer emits events and closes windows from it).
    pub fn set_files_hooks(&self, notifier: FilesNotifier) {
        *lock(&self.files.notifier) = Some(notifier);
    }

    /// Opens (or finds) the file browser of an SSH or `MikroTik` connection. `start_path` wins; else a
    /// terminal session's current directory (`session_id`); else the remote home folder.
    ///
    /// # Errors
    /// `locked`, `notFound`, `notSsh`, `emptyHost` or `corruptData` (secrets).
    pub fn files_open(
        &self,
        connection_id: &str,
        start_path: Option<String>,
        session_id: Option<&str>,
    ) -> Result<FilesOpenDto, CommandError> {
        let id = browser_id(connection_id);
        let existing = {
            let browsers = lock(&self.files.browsers);
            browsers.get(&id).map(|b| b.connection_name.clone())
        };
        if let Some(connection_name) = existing {
            return Ok(FilesOpenDto {
                browser_id: id,
                connection_name,
                created: false,
            });
        }
        let (connection, spec) = self.files_connection(connection_id)?;
        let vpn = self.vpn_gate(connection_id)?;
        let start_path = start_path
            .filter(|p| !p.trim().is_empty())
            .or_else(|| {
                session_id
                    .and_then(|s| self.ssh_session_directory(s))
                    .filter(|(owner, _)| owner == connection_id)
                    .and_then(|(_, directory)| directory)
            })
            .unwrap_or_else(|| "~".to_owned());
        lock(&self.files.browsers).insert(
            id.clone(),
            Browser {
                connection_id: connection_id.to_owned(),
                connection_name: connection.name.clone(),
                start_path,
                generation: 0,
                state: BrowserState::Connecting,
                remote: None,
                transfer: None,
                vpn,
            },
        );
        self.files_connect(&id, spec);
        Ok(FilesOpenDto {
            browser_id: id,
            connection_name: connection.name,
            created: true,
        })
    }

    fn files_connection(
        &self,
        connection_id: &str,
    ) -> Result<(rd_core::Connection, rd_ssh::background::ConnectSpec), CommandError> {
        let (connection, secrets) = {
            let inner = self.inner();
            let (db, vault) = Self::unlocked(&inner)?;
            let library = db.library()?;
            let connection = library
                .connections
                .iter()
                .find(|c| c.id == connection_id)
                .ok_or_else(|| CommandError::not_found(connection_id))?;
            if !matches!(connection.kind, ConnectionType::Ssh | ConnectionType::Mikrotik) {
                return Err(CommandError::new("notSsh", "This connection doesn't use SSH."));
            }
            rd_ssh::resolve(&library, connection, |sealed| vault.open(sealed))?
        };
        let spec = self.background_spec(&connection, secrets)?;
        Ok((connection, spec))
    }

    /// Starts connecting `id` (a new generation).
    fn files_connect(&self, id: &str, spec: rd_ssh::background::ConnectSpec) {
        let generation = {
            let mut next = lock(&self.files.next_generation);
            *next += 1;
            *next
        };
        {
            let mut browsers = lock(&self.files.browsers);
            let Some(browser) = browsers.get_mut(id) else {
                return;
            };
            browser.generation = generation;
            browser.state = BrowserState::Connecting;
            browser.remote = None;
        }
        self.files.notify_status(id);
        let weak = Arc::downgrade(&self.files);
        let browser_id = id.to_owned();
        let prompt_weak = Weak::clone(&weak);
        let prompt_id = browser_id.clone();
        let prompt: HostKeyPrompt = Arc::new(move |question| match prompt_weak.upgrade() {
            Some(shared) => shared.ask_host_key(&prompt_id, question),
            None => oneshot::channel().1,
        });
        runtime().spawn(async move {
            let result = match Client::connect(spec, Some(prompt)).await {
                Ok(client) => Ok(RemoteFiles::open(client).await),
                Err(error) => Err(BrowserState::Failed {
                    code: error.code().to_owned(),
                    error: error.to_string(),
                }),
            };
            if let Some(shared) = weak.upgrade() {
                shared.connected(&browser_id, generation, result);
            }
        });
    }

    /// Connects a failed browser again (secrets are read from the vault anew).
    ///
    /// # Errors
    /// `browserNotFound`, `locked`, `notFound`, `notSsh`, `emptyHost`.
    pub fn files_reconnect(&self, browser_id: &str) -> Result<(), CommandError> {
        let (connection_id, vpn_alive) = lock(&self.files.browsers)
            .get(browser_id)
            .map(|b| {
                let alive = b.vpn.as_ref().is_some_and(super::VpnHold::is_alive);
                (b.connection_id.clone(), alive)
            })
            .ok_or_else(|| not_open(browser_id))?;
        let (_, spec) = self.files_connection(&connection_id)?;
        if !vpn_alive {
            let vpn = self.vpn_gate(&connection_id)?;
            if let Some(browser) = lock(&self.files.browsers).get_mut(browser_id) {
                browser.vpn = vpn;
            }
        }
        self.files_connect(browser_id, spec);
        Ok(())
    }

    /// A browser's state.
    ///
    /// # Errors
    /// `browserNotFound`.
    pub fn files_info(&self, browser_id: &str) -> Result<FilesStatusDto, CommandError> {
        lock(&self.files.browsers)
            .get(browser_id)
            .map(|b| FilesShared::status_of(browser_id, b))
            .ok_or_else(|| not_open(browser_id))
    }

    fn files_remote(&self, browser_id: &str) -> Result<Arc<RemoteFiles>, CommandError> {
        let browsers = lock(&self.files.browsers);
        let browser = browsers.get(browser_id).ok_or_else(|| not_open(browser_id))?;
        browser.remote.clone().ok_or_else(not_connected)
    }

    /// Lists a remote folder (`~` for the home folder). A dropped connection marks the browser failed.
    ///
    /// # Errors
    /// `browserNotFound`, `notConnected`, `listFailed`, `unreadable` or `ssh`.
    pub async fn files_list_remote(
        &self,
        browser_id: &str,
        path: &str,
        show_hidden: bool,
    ) -> Result<ListingDto, CommandError> {
        let remote = self.files_remote(browser_id)?;
        let path = path.to_owned();
        let task = Arc::clone(&remote);
        let result = runtime()
            .spawn(async move { task.list(&path).await })
            .await
            .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))?;
        match result {
            Ok(found) => Ok(listing(found.path, found.entries, show_hidden)),
            Err(error) => {
                if remote.is_closed() {
                    self.files_lost(browser_id);
                }
                Err(CommandError::new(error.code(), &error.to_string()))
            }
        }
    }

    /// The connection dropped: the browser shows the failure (and Retry).
    fn files_lost(&self, browser_id: &str) {
        {
            let mut browsers = lock(&self.files.browsers);
            let Some(browser) = browsers.get_mut(browser_id) else {
                return;
            };
            browser.remote = None;
            browser.state = BrowserState::Failed {
                code: "disconnected".to_owned(),
                error: "The SSH connection was closed.".to_owned(),
            };
        }
        self.files.notify_status(browser_id);
    }

    /// Lists a local folder; an empty path is the default folder (Downloads), `~` the home folder.
    ///
    /// # Errors
    /// `listFailed`.
    pub fn files_list_local(path: Option<&str>, show_hidden: bool) -> Result<ListingDto, CommandError> {
        let home = dirs::home_dir();
        let directory = match path.map(str::trim).filter(|p| !p.is_empty()) {
            None => local::default_directory(),
            Some(p) => rd_ssh::command::expand_tilde(p, home.as_deref()),
        };
        let directory = std::path::absolute(&directory).unwrap_or(directory);
        let entries = local::list(&directory, show_hidden).map_err(|e| {
            CommandError::new("listFailed", &e.to_string()).with_detail(directory.display().to_string())
        })?;
        Ok(ListingDto {
            path: directory.display().to_string(),
            parent: directory.parent().map(|p| p.display().to_string()),
            entries,
        })
    }

    /// Starts a transfer; progress, conflicts and the end arrive as events. Returns the transfer id.
    ///
    /// # Errors
    /// `browserNotFound`, `notConnected`, `transferRunning` (one at a time) or `nothingToCopy`.
    pub fn files_transfer(&self, browser_id: &str, input: TransferInput) -> Result<String, CommandError> {
        if input.sources.is_empty() {
            return Err(CommandError::new("nothingToCopy", "Nothing is selected."));
        }
        let transfer_id = new_id();
        let cancel = CancellationToken::new();
        let remote = {
            let mut browsers = lock(&self.files.browsers);
            let browser = browsers.get_mut(browser_id).ok_or_else(|| not_open(browser_id))?;
            let remote = browser.remote.clone().ok_or_else(not_connected)?;
            if browser.transfer.is_some() {
                return Err(CommandError::new(
                    "transferRunning",
                    "Wait for the running transfer to finish.",
                ));
            }
            browser.transfer = Some(Running {
                id: transfer_id.clone(),
                cancel: cancel.clone(),
            });
            remote
        };
        self.files.notify_status(browser_id);
        let request = TransferRequest {
            direction: input.direction,
            sources: input.sources,
            destination: input.destination,
            compress: input.gzip,
        };
        let observer = Observer {
            shared: Arc::downgrade(&self.files),
            browser_id: browser_id.to_owned(),
            transfer_id: transfer_id.clone(),
            direction: request.direction,
        };
        let weak = Arc::downgrade(&self.files);
        let browser = browser_id.to_owned();
        let id = transfer_id.clone();
        runtime().spawn(async move {
            let report = transfer::run(&remote, &request, &observer, &cancel).await;
            let Some(shared) = weak.upgrade() else {
                return;
            };
            {
                let mut browsers = lock(&shared.browsers);
                if let Some(entry) = browsers.get_mut(&browser)
                    && entry.transfer.as_ref().is_some_and(|t| t.id == id)
                {
                    entry.transfer = None;
                }
            }
            lock(&shared.conflicts).retain(|_, p| p.browser_id != browser);
            shared.notify(FilesNotice::Done(FilesDoneDto {
                browser_id: browser.clone(),
                transfer_id: id,
                direction: request.direction,
                cancelled: report.cancelled(),
                items: report.items,
            }));
            shared.notify_status(&browser);
        });
        Ok(transfer_id)
    }

    /// Cancels the browser's running transfer (an open conflict question is skipped).
    pub fn files_cancel(&self, browser_id: &str) {
        if let Some(running) = lock(&self.files.browsers)
            .get(browser_id)
            .and_then(|b| b.transfer.as_ref())
        {
            running.cancel.cancel();
        }
        lock(&self.files.conflicts).retain(|_, p| p.browser_id != browser_id);
    }

    /// The answer to a `files-conflict` (an unknown or answered request is ignored).
    pub fn files_conflict_answer(&self, request_id: &str, choice: ConflictChoice, apply_to_all: bool) {
        if let Some(pending) = lock(&self.files.conflicts).remove(request_id) {
            let _ = pending.answer.send((choice, apply_to_all));
        }
    }

    /// Answers a file browser host key prompt; `false` when `request_id` isn't one.
    pub(crate) fn files_host_key_answer(&self, request_id: &str, accept: bool) -> bool {
        match lock(&self.files.host_keys).remove(request_id) {
            Some(pending) => {
                let _ = pending.answer.send(accept);
                true
            }
            None => false,
        }
    }

    /// The window closed: stops the transfer and the connection.
    pub fn files_close(&self, browser_id: &str) {
        self.files.close(browser_id);
    }

    /// Closes every browser and tells the UI to close their windows: lock, database switch, quit.
    pub fn close_all_files(&self) {
        let ids: Vec<String> = lock(&self.files.browsers).keys().cloned().collect();
        for id in ids {
            if self.files.close(&id) {
                self.files.notify(FilesNotice::Closed(id));
            }
        }
    }

    /// Whether "Files (SCP)…" applies in the main window now; returns whether that changed.
    pub fn set_files_target(&self, enabled: bool) -> bool {
        self.files.target.swap(enabled, Ordering::SeqCst) != enabled
    }

    /// See [`set_files_target`](Self::set_files_target).
    #[must_use]
    pub fn files_target(&self) -> bool {
        self.files.target.load(Ordering::SeqCst)
    }

    /// Running transfers.
    #[must_use]
    pub fn running_transfers(&self) -> usize {
        lock(&self.files.browsers)
            .values()
            .filter(|b| b.transfer.is_some())
            .count()
    }
}

/// `true` for a file browser window's label.
#[must_use]
pub fn is_files_label(label: &str) -> bool {
    label.starts_with(FILES_LABEL_PREFIX)
}
