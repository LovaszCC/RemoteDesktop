//! Embedded SSH sessions and external terminals (Swift `AppState` session lifecycle): opening a session
//! for a connection, input, resize, reconnect with re-resolved secrets, close, "copy next command output",
//! the clear decision, and the askpass prompts the vault can't answer. A local terminal (the sidebar's
//! "Local Terminal") is a session of the same kind running the local shell, with no connection behind it.
//!
//! The secrets never leave Rust: the vault password and key passphrase go to the askpass server, the
//! unlocked key to a private temp file that is deleted when the run ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use rd_core::{ConnectionType, new_id};
use rd_ssh::analyzer::ClearAction;
use rd_ssh::askpass::server::{AskpassHandler, AskpassRequest, AskpassServer, Ticket};
use rd_ssh::askpass::{PromptKind, askpass_env};
use rd_ssh::external::{SshCommand, TerminalChoice};
use rd_ssh::known_hosts::{self, ChangedHostKey};
use rd_ssh::local::LocalShell;
use rd_ssh::recorder::Phase;
use rd_ssh::{
    ArgsError, SessionEvent, SessionKind, Sink, SpawnSpec, SshSecrets, SshSession, UnlockedKeyFile,
};
use serde::Serialize;
use zeroize::Zeroizing;

use super::{AppCore, CommandError};
use crate::menu::tr;

/// How long an external terminal's askpass token and unlocked key stay valid.
pub const EXTERNAL_LIFETIME: Duration = Duration::from_secs(180);

/// An askpass prompt for the UI (event `askpass-prompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskpassPromptDto {
    /// Answer with `askpass_answer`.
    pub request_id: String,
    /// The embedded session, or `external-…` for an external terminal.
    pub session_id: String,
    /// `password`, `passphrase`, `confirm` or `other`.
    pub kind: PromptKind,
    /// `ssh`'s prompt text.
    pub prompt: String,
}

/// What the askpass side tells the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskpassNotice {
    /// Show a dialog.
    Prompt(AskpassPromptDto),
    /// The request was answered elsewhere or its session ended: close the dialog.
    Closed(String),
}

/// Receives askpass notices (emits the events).
pub type AskpassNotifier = Arc<dyn Fn(AskpassNotice) + Send + Sync>;
/// Called when sessions start, end or close (refreshes the menu).
pub type SessionsChanged = Arc<dyn Fn() + Send + Sync>;

struct Run {
    session: Arc<SshSession>,
    // Kept until the run ends: the token answers prompts, the key file is read by `ssh`.
    ticket: Option<Ticket>,
    key_file: Option<UnlockedKeyFile>,
}

/// What a session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// `ssh` for a library connection.
    Connection(String),
    /// The local shell.
    Local,
}

struct Entry {
    target: Target,
    sink: Arc<dyn Sink>,
    /// The run whose output and events pass; bumped before each spawn.
    current: u64,
    run: Option<Run>,
    /// The changed host key the last run's `ssh` refused, for `ssh_replace_host_key`.
    host_key_change: Option<ChangedHostKey>,
}

struct PendingPrompt {
    session_id: String,
    answer: mpsc::Sender<Option<Zeroizing<String>>>,
}

struct External {
    label: String,
    _ticket: Ticket,
    _key_file: Option<UnlockedKeyFile>,
}

/// The SSH state of the app; shared with the askpass handler and the session sinks.
pub(crate) struct SshShared {
    sessions: Mutex<HashMap<String, Entry>>,
    prompts: Mutex<HashMap<String, PendingPrompt>>,
    externals: Mutex<Vec<External>>,
    askpass: Mutex<Option<AskpassServer>>,
    notifier: Mutex<Option<AskpassNotifier>>,
    changed: Mutex<Option<SessionsChanged>>,
    next_run: Mutex<u64>,
    /// Test hooks: the `ssh` to run, the askpass helper and the local shell.
    ssh_program: Mutex<Option<PathBuf>>,
    helper: Mutex<Option<PathBuf>>,
    local_shell: Mutex<Option<LocalShell>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SshShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::default(),
            prompts: Mutex::default(),
            externals: Mutex::default(),
            askpass: Mutex::default(),
            notifier: Mutex::default(),
            changed: Mutex::default(),
            next_run: Mutex::new(0),
            ssh_program: Mutex::default(),
            helper: Mutex::default(),
            local_shell: Mutex::default(),
        })
    }

    fn notify(&self, notice: AskpassNotice) {
        let notifier = lock(&self.notifier).clone();
        if let Some(notifier) = notifier {
            notifier(notice);
        }
    }

    fn sessions_changed(&self) {
        let changed = lock(&self.changed).clone();
        if let Some(changed) = changed {
            changed();
        }
    }

    /// Refuses the open prompts of `session_id` and tells the UI to close them.
    fn cancel_prompts(&self, session_id: &str) {
        let cancelled: Vec<(String, PendingPrompt)> = {
            let mut prompts = lock(&self.prompts);
            let ids: Vec<String> = prompts
                .iter()
                .filter(|(_, p)| p.session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| prompts.remove(&id).map(|p| (id, p)))
                .collect()
        };
        for (id, prompt) in cancelled {
            let _ = prompt.answer.send(None);
            self.notify(AskpassNotice::Closed(id));
        }
    }

    /// The askpass handler: forwards a prompt to the UI and waits for `askpass_answer`.
    fn ask_user(self: &Arc<Self>, request: AskpassRequest) -> Option<Zeroizing<String>> {
        if lock(&self.notifier).is_none() {
            return None;
        }
        let (answer, receiver) = mpsc::channel();
        let request_id = new_id();
        lock(&self.prompts).insert(
            request_id.clone(),
            PendingPrompt {
                session_id: request.session.clone(),
                answer,
            },
        );
        self.notify(AskpassNotice::Prompt(AskpassPromptDto {
            request_id,
            session_id: request.session,
            kind: request.kind,
            prompt: request.prompt,
        }));
        receiver.recv().ok().flatten()
    }

    fn askpass_endpoint(self: &Arc<Self>) -> Result<String, CommandError> {
        let mut server = lock(&self.askpass);
        if server.is_none() {
            let weak = Arc::downgrade(self);
            let handler: AskpassHandler =
                Arc::new(move |request| weak.upgrade().and_then(|shared| shared.ask_user(request)));
            let started = AskpassServer::start(handler).map_err(|e| {
                CommandError::new("askpassFailed", "Couldn't start the password helper.")
                    .with_detail(e.to_string())
            })?;
            *server = Some(started);
        }
        Ok(server
            .as_ref()
            .map(|s| s.endpoint().to_owned())
            .unwrap_or_default())
    }

    fn register(&self, label: &str, secrets: &mut SshSecrets) -> Option<Ticket> {
        let server = lock(&self.askpass);
        server
            .as_ref()
            .map(|s| s.register(label, secrets.password.take(), secrets.passphrase.take()))
    }

    fn next_run(&self) -> u64 {
        let mut next = lock(&self.next_run);
        *next += 1;
        *next
    }

    /// A run ended: forget its token and delete its key file, refuse its open prompts, and remember the
    /// changed host key `ssh` refused, if any.
    fn run_ended(&self, session_id: &str, number: u64, host_key_change: Option<ChangedHostKey>) {
        if let Some(entry) = lock(&self.sessions).get_mut(session_id)
            && entry.current == number
        {
            entry.host_key_change = host_key_change;
            if let Some(run) = entry.run.as_mut() {
                run.ticket = None;
                run.key_file = None;
            }
        }
        self.cancel_prompts(session_id);
        self.sessions_changed();
    }

    fn is_current(&self, session_id: &str, number: u64) -> bool {
        lock(&self.sessions)
            .get(session_id)
            .is_some_and(|e| e.current == number)
    }
}

/// Passes one run's output and events on, dropping those of a replaced or closed run, and cleans up when
/// the run ends.
struct RunSink {
    shared: Weak<SshShared>,
    session_id: String,
    number: u64,
    inner: Arc<dyn Sink>,
}

impl Sink for RunSink {
    fn output(&self, bytes: Vec<u8>) {
        if self
            .shared
            .upgrade()
            .is_some_and(|s| s.is_current(&self.session_id, self.number))
        {
            self.inner.output(bytes);
        }
    }

    fn event(&self, event: SessionEvent) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let current = shared.is_current(&self.session_id, self.number);
        // Clean up before the UI hears of the exit, so a reconnect never races the old key file.
        if let SessionEvent::Exit { host_key_changed, .. } = &event {
            shared.run_ended(&self.session_id, self.number, host_key_changed.clone());
        }
        if current {
            self.inner.event(event);
        }
    }
}

/// A resolved launch: the arguments, environment, login user and what must live as long as `ssh`.
struct Prepared {
    kind: SessionKind,
    /// The connection's name (the external terminal's title).
    name: String,
    login_user: String,
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, std::ffi::OsString)>,
    ticket: Option<Ticket>,
    key_file: Option<UnlockedKeyFile>,
}

fn args_error(error: &ArgsError) -> CommandError {
    match error {
        ArgsError::EmptyHost => CommandError::new("emptyHost", "The connection has no host."),
        ArgsError::InvalidHost => CommandError::new("invalidHost", &error.to_string()),
    }
}

fn session_not_found(id: &str) -> CommandError {
    CommandError::new("sessionNotFound", "The session doesn't exist any more.").with_detail(id)
}

impl AppCore {
    /// Sets the askpass notifier and the sessions-changed hook (the Tauri layer emits events from them).
    pub fn set_ssh_hooks(&self, notifier: AskpassNotifier, changed: SessionsChanged) {
        *lock(&self.ssh.notifier) = Some(notifier);
        *lock(&self.ssh.changed) = Some(changed);
    }

    /// Test hook: run `program` instead of the system `ssh`, and name `helper` as the askpass program.
    pub fn set_ssh_program(&self, program: Option<PathBuf>, helper: Option<PathBuf>) {
        *lock(&self.ssh.ssh_program) = program;
        *lock(&self.ssh.helper) = helper;
    }

    /// Resolves `connection_id` against the vault and prepares a run labelled `label`.
    fn prepare(&self, connection_id: &str, label: &str) -> Result<Prepared, CommandError> {
        let (connection, mut secrets) = {
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
        let program = lock(&self.ssh.ssh_program)
            .clone()
            .or_else(rd_ssh::find_ssh)
            .ok_or_else(|| CommandError::new("sshNotFound", "The ssh program is not installed."))?;
        let key_file = secrets
            .unlocked_key
            .take()
            .map(|key| UnlockedKeyFile::write(&key))
            .transpose()
            .map_err(|e| {
                CommandError::new("keyFile", "Couldn't prepare the SSH key.").with_detail(e.to_string())
            })?;
        let home = dirs::home_dir();
        let args = rd_ssh::ssh_arguments(
            &connection,
            key_file.as_ref().map(UnlockedKeyFile::path),
            home.as_deref(),
        )
        .map_err(|e| args_error(&e))?;
        let endpoint = self.ssh.askpass_endpoint()?;
        let ticket = self
            .ssh
            .register(label, &mut secrets)
            .ok_or_else(|| CommandError::new("askpassFailed", "Couldn't start the password helper."))?;
        let helper = lock(&self.ssh.helper)
            .clone()
            .or_else(|| std::env::current_exe().ok())
            .ok_or_else(|| CommandError::new("askpassFailed", "Couldn't find the app executable."))?;
        let env = askpass_env(&helper, &endpoint, ticket.token());
        Ok(Prepared {
            kind: SessionKind::Ssh,
            login_user: connection.username.trim_matches([' ', '\t']).to_owned(),
            name: connection.name,
            program,
            args,
            env,
            ticket: Some(ticket),
            key_file,
        })
    }

    /// Test hook: run `shell` instead of this machine's shell in local terminals.
    pub fn set_local_shell(&self, shell: Option<LocalShell>) {
        *lock(&self.ssh.local_shell) = shell;
    }

    /// The local shell launch (no secrets, no askpass).
    fn prepare_local(&self) -> Prepared {
        let shell = lock(&self.ssh.local_shell)
            .clone()
            .unwrap_or_else(rd_ssh::local::local_shell);
        Prepared {
            kind: SessionKind::Local,
            name: String::new(),
            login_user: rd_ssh::local::login_user(|name| std::env::var_os(name)),
            program: shell.program,
            args: shell.args,
            env: Vec::new(),
            ticket: None,
            key_file: None,
        }
    }

    fn prepare_target(&self, target: &Target, label: &str) -> Result<Prepared, CommandError> {
        match target {
            Target::Connection(id) => self.prepare(id, label),
            Target::Local => Ok(self.prepare_local()),
        }
    }

    /// Spawns a run for the session entry `session_id` (which must exist) and makes it current. The
    /// sessions lock is not held while spawning: the run's first events come back through it.
    fn spawn_run(
        &self,
        session_id: &str,
        prepared: Prepared,
        cols: u16,
        rows: u16,
    ) -> Result<Run, CommandError> {
        let number = self.ssh.next_run();
        let sink = {
            let mut sessions = lock(&self.ssh.sessions);
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            entry.current = number;
            entry.host_key_change = None;
            Arc::clone(&entry.sink)
        };
        let run_sink: Arc<dyn Sink> = Arc::new(RunSink {
            shared: Arc::downgrade(&self.ssh),
            session_id: session_id.to_owned(),
            number,
            inner: sink,
        });
        let spec = SpawnSpec {
            kind: prepared.kind,
            program: prepared.program,
            args: prepared.args,
            env: prepared.env,
            login_user: prepared.login_user,
            cols,
            rows,
        };
        let message = match spec.kind {
            SessionKind::Ssh => "Couldn't start ssh.",
            SessionKind::Local => "Couldn't start the shell.",
        };
        let session = SshSession::spawn(spec, run_sink)
            .map_err(|e| CommandError::new("spawnFailed", message).with_detail(e.to_string()))?;
        Ok(Run {
            session: Arc::new(session),
            ticket: prepared.ticket,
            key_file: prepared.key_file,
        })
    }

    /// Opens an embedded session for an SSH or `MikroTik` connection; output goes to the sink `make_sink`
    /// builds for the new session id. Returns the session id.
    ///
    /// # Errors
    /// `locked`, `noDatabase`, `notFound`, `notSsh`, `sshNotFound`, `emptyHost`, `invalidHost`,
    /// `keyFile`, `askpassFailed`, `spawnFailed` or `corruptData`.
    pub fn open_ssh_session(
        &self,
        connection_id: &str,
        cols: u16,
        rows: u16,
        make_sink: impl FnOnce(&str) -> Arc<dyn Sink>,
    ) -> Result<String, CommandError> {
        self.open_session(
            Target::Connection(connection_id.to_owned()),
            cols,
            rows,
            make_sink,
        )
    }

    /// Opens a local terminal: this machine's shell in the home directory, as a session like the SSH ones
    /// (input, resize, reconnect, close and split panes work the same). It needs no unlocked vault: no
    /// secret is involved. Returns the session id.
    ///
    /// # Errors
    /// `spawnFailed`.
    pub fn open_local_terminal(
        &self,
        cols: u16,
        rows: u16,
        make_sink: impl FnOnce(&str) -> Arc<dyn Sink>,
    ) -> Result<String, CommandError> {
        self.open_session(Target::Local, cols, rows, make_sink)
    }

    fn open_session(
        &self,
        target: Target,
        cols: u16,
        rows: u16,
        make_sink: impl FnOnce(&str) -> Arc<dyn Sink>,
    ) -> Result<String, CommandError> {
        let session_id = new_id();
        let sink = make_sink(&session_id);
        let prepared = self.prepare_target(&target, &session_id)?;
        lock(&self.ssh.sessions).insert(
            session_id.clone(),
            Entry {
                target,
                sink,
                current: 0,
                run: None,
                host_key_change: None,
            },
        );
        match self.spawn_run(&session_id, prepared, cols, rows) {
            Ok(run) => {
                if let Some(entry) = lock(&self.ssh.sessions).get_mut(&session_id) {
                    entry.run = Some(run);
                }
                self.ssh.sessions_changed();
                Ok(session_id)
            }
            Err(e) => {
                lock(&self.ssh.sessions).remove(&session_id);
                Err(e)
            }
        }
    }

    /// Starts the session again with the connection and credential resolved anew (edits apply), in the
    /// same tab; a local terminal gets a new shell. A running process is ended first; its exit is not
    /// reported. After an error the tab has no running process.
    ///
    /// # Errors
    /// `sessionNotFound`, or the errors of [`open_ssh_session`](Self::open_ssh_session).
    pub fn ssh_reconnect(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), CommandError> {
        let (target, sink, old) = {
            let mut sessions = lock(&self.ssh.sessions);
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            // The old run's exit must not reach the tab.
            entry.current = 0;
            let old = entry.run.as_ref().map(|r| Arc::clone(&r.session));
            (entry.target.clone(), Arc::clone(&entry.sink), old)
        };
        if let Some(old) = old {
            old.kill();
        }
        self.ssh.cancel_prompts(session_id);
        let language = self.inner().settings.language.clone();
        sink.output(format!("\r\n\u{1b}[2m— {} —\u{1b}[0m\r\n", tr(&language, "Reconnecting")).into_bytes());
        let prepared = self.prepare_target(&target, session_id)?;
        let run = self.spawn_run(session_id, prepared, cols, rows)?;
        let old = lock(&self.ssh.sessions)
            .get_mut(session_id)
            .and_then(|entry| entry.run.replace(run));
        drop(old);
        self.ssh.sessions_changed();
        Ok(())
    }

    /// "Replace key" after the session's `ssh` refused a changed host key (the `hostKeyChanged` of its
    /// exit event). The server's key is read again (russh, no login); when its fingerprint is the one
    /// `ssh` reported and it is stored under the same name, the old entries are replaced with it, so the
    /// reconnect that follows needs no prompt. Otherwise (a proxy, an unreachable server, another
    /// fingerprint) the old entries are only removed and `ssh` asks about the key as unknown on
    /// reconnect. The caller reconnects. The report must name the connection's own host key name and one
    /// of the user's `known_hosts` files (a remote command could print a fake one and exit 255).
    ///
    /// # Errors
    /// `sessionNotFound`, `noHostKeyChange`, `locked`, `notFound`, `emptyHost`, or `hostKeyReplace` when
    /// the report names another host or file, or a `known_hosts` file can't be written.
    pub fn ssh_replace_host_key(&self, session_id: &str) -> Result<(), CommandError> {
        let (connection_id, change) = {
            let sessions = lock(&self.ssh.sessions);
            let entry = sessions
                .get(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            let no_change = || {
                CommandError::new(
                    "noHostKeyChange",
                    "The session has no changed host key to replace.",
                )
            };
            let change = entry.host_key_change.clone().ok_or_else(no_change)?;
            let Target::Connection(connection_id) = &entry.target else {
                return Err(no_change());
            };
            (connection_id.clone(), change)
        };
        let connection = {
            let inner = self.inner();
            let (db, _) = Self::unlocked(&inner)?;
            db.library()?
                .connections
                .into_iter()
                .find(|c| c.id == connection_id)
                .ok_or_else(|| CommandError::not_found(&connection_id))?
        };
        let spec = self.background_spec(&connection, SshSecrets::default())?;
        let target = spec.target();
        let files = target.known_hosts_files;
        let old_file = PathBuf::from(&change.file);
        // The report is PTY text, and a remote command can exit 255 too: only the connection's own host
        // name and the user's known_hosts files are touched.
        let expected = known_hosts::lookup_name(&target.host_key_name, target.port);
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if !change.host.eq_ignore_ascii_case(&expected)
            || !files.iter().any(|f| canonical(f) == canonical(&old_file))
        {
            return Err(
                CommandError::new("hostKeyReplace", "Couldn't replace the host key.").with_detail(format!(
                    "{} in {} is not this connection's known_hosts entry",
                    change.host, change.file
                )),
            );
        }
        let (sender, receiver) = mpsc::channel();
        rd_ssh::background::runtime().spawn(async move {
            let _ = sender.send(rd_ssh::background::fetch_host_key(&spec).await);
        });
        let written = match receiver.recv() {
            Ok(Ok(probed))
                if probed.key.fingerprint() == change.fingerprint
                    && probed.name.eq_ignore_ascii_case(&change.host) =>
            {
                known_hosts::replace(&probed.files, &old_file, &change.host, &probed.key)
            }
            _ => files
                .iter()
                .chain(std::iter::once(&old_file))
                .try_fold(0, |n, file| {
                    Ok(n + known_hosts::remove(file, &change.host, &change.key_type, None)?)
                }),
        };
        written.map_err(|e| {
            CommandError::new("hostKeyReplace", "Couldn't replace the host key.")
                .with_detail(format!("{}: {e}", change.file))
        })?;
        if let Some(entry) = lock(&self.ssh.sessions).get_mut(session_id) {
            entry.host_key_change = None;
        }
        Ok(())
    }

    /// Runs `work` on the session's current process, outside the sessions lock (the session emits events
    /// that take it).
    fn with_session<T>(
        &self,
        session_id: &str,
        work: impl FnOnce(&SshSession) -> T,
    ) -> Result<T, CommandError> {
        let session = lock(&self.ssh.sessions)
            .get(session_id)
            .and_then(|e| e.run.as_ref().map(|r| Arc::clone(&r.session)))
            .ok_or_else(|| session_not_found(session_id))?;
        Ok(work(&session))
    }

    /// Sends keyboard or paste input.
    ///
    /// # Errors
    /// `sessionNotFound`, or `sessionEnded` after the process exited.
    pub fn ssh_input(&self, session_id: &str, data: &[u8]) -> Result<(), CommandError> {
        self.with_session(session_id, |s| s.write(data))?.map_err(|e| {
            CommandError::new("sessionEnded", "The session has ended.").with_detail(e.to_string())
        })
    }

    /// Input typed into a broadcasting split pane: `data` goes to `source` as typed, then to every running
    /// target, without bracketed-paste markers for targets whose remote side didn't ask for them (each
    /// target's live state from its analyzer). Targets that are gone or ended are skipped.
    ///
    /// # Errors
    /// Those of [`Self::ssh_input`] for the source.
    pub fn ssh_broadcast_input(
        &self,
        source: &str,
        targets: &[String],
        data: &[u8],
    ) -> Result<(), CommandError> {
        self.ssh_input(source, data)?;
        for target in targets.iter().filter(|t| t.as_str() != source) {
            let _ = self.with_session(target, |s| {
                if s.is_running() {
                    let bytes = rd_ssh::broadcast::filter_for_target(data, s.bracketed_paste());
                    if !bytes.is_empty() {
                        let _ = s.write(&bytes);
                    }
                }
            });
        }
        Ok(())
    }

    /// Resizes the PTY. Errors after exit are ignored.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn ssh_resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), CommandError> {
        self.with_session(session_id, |s| {
            let _ = s.resize(cols, rows);
        })
    }

    /// Ends the process and forgets the session (its key file is deleted, its prompts refused).
    pub fn ssh_close(&self, session_id: &str) {
        let entry = lock(&self.ssh.sessions).remove(session_id);
        if let Some(run) = entry.and_then(|e| e.run) {
            run.session.kill();
        }
        self.ssh.cancel_prompts(session_id);
        self.ssh.sessions_changed();
    }

    /// The record button: arm, cancel or finish. Returns the new phase; the result comes as a
    /// `recorded` event.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn ssh_record_next_output(&self, session_id: &str) -> Result<Phase, CommandError> {
        self.with_session(session_id, SshSession::toggle_recording)
    }

    /// Esc while recording: stop without a result.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn ssh_record_cancel(&self, session_id: &str) -> Result<(), CommandError> {
        self.with_session(session_id, SshSession::cancel_recording)
    }

    /// What "Clear terminal" should do now.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn ssh_clear_decision(&self, session_id: &str) -> Result<ClearAction, CommandError> {
        self.with_session(session_id, SshSession::clear_action)
    }

    /// The answer to an askpass prompt; `None` refuses. Unknown ids (already answered or cancelled) are
    /// ignored.
    pub fn askpass_answer(&self, request_id: &str, answer: Option<String>) {
        let pending = lock(&self.ssh.prompts).remove(request_id);
        if let Some(pending) = pending {
            let _ = pending.answer.send(answer.map(Zeroizing::new));
        }
    }

    /// Opens the connection in the external terminal from the settings (or the OS default). The token
    /// and unlocked key stay valid for [`EXTERNAL_LIFETIME`]. Returns the terminal id used.
    ///
    /// # Errors
    /// Those of [`open_ssh_session`](Self::open_ssh_session), and `terminalFailed`.
    pub fn open_external_terminal(&self, connection_id: &str) -> Result<String, CommandError> {
        let label = format!("external-{}", new_id());
        let prepared = self.prepare(connection_id, &label)?;
        let lossy = |s: &std::ffi::OsStr| s.to_string_lossy().into_owned();
        let command = SshCommand {
            program: lossy(prepared.program.as_os_str()),
            args: prepared.args,
            env: prepared.env.iter().map(|(k, v)| (k.clone(), lossy(v))).collect(),
            title: prepared.name.clone(),
        };
        let stored = self.inner().settings.external_terminal.clone();
        let used = rd_ssh::external::launch(stored.as_deref(), &command).map_err(|e| {
            CommandError::new("terminalFailed", "Couldn't open the terminal.").with_detail(e.to_string())
        })?;
        if let Some(ticket) = prepared.ticket {
            lock(&self.ssh.externals).push(External {
                label: label.clone(),
                _ticket: ticket,
                _key_file: prepared.key_file,
            });
            let weak = Arc::downgrade(&self.ssh);
            std::thread::spawn(move || {
                std::thread::sleep(EXTERNAL_LIFETIME);
                if let Some(shared) = weak.upgrade() {
                    lock(&shared.externals).retain(|e| e.label != label);
                    shared.cancel_prompts(&label);
                }
            });
        }
        Ok(used.to_owned())
    }

    /// The terminals installed here, for Settings.
    #[must_use]
    pub fn installed_terminals() -> Vec<TerminalChoice> {
        rd_ssh::external::installed_terminals()
    }

    /// Embedded sessions whose process still runs.
    #[must_use]
    pub fn running_ssh_sessions(&self) -> usize {
        lock(&self.ssh.sessions)
            .values()
            .filter(|e| e.run.as_ref().is_some_and(|r| r.session.is_running()))
            .count()
    }

    /// The shell's working directory of a session (OSC 7 or a `user@host: dir` title) and its connection,
    /// when the session exists and is not a local terminal.
    #[must_use]
    pub fn ssh_session_directory(&self, session_id: &str) -> Option<(String, Option<String>)> {
        let sessions = lock(&self.ssh.sessions);
        let entry = sessions.get(session_id)?;
        let Target::Connection(connection_id) = &entry.target else {
            return None;
        };
        let directory = entry.run.as_ref().and_then(|r| r.session.directory());
        Some((connection_id.clone(), directory))
    }

    /// Open session tabs, running or exited.
    #[must_use]
    pub fn ssh_session_count(&self) -> usize {
        lock(&self.ssh.sessions).len()
    }

    /// On quit: ends every session, refuses every prompt, deletes the key files.
    pub fn shutdown_ssh(&self) {
        let sessions: Vec<(String, Entry)> = lock(&self.ssh.sessions).drain().collect();
        for (id, entry) in sessions {
            if let Some(run) = entry.run {
                run.session.kill();
            }
            self.ssh.cancel_prompts(&id);
        }
        let externals: Vec<External> = lock(&self.ssh.externals).drain(..).collect();
        for external in externals {
            self.ssh.cancel_prompts(&external.label);
        }
    }
}
