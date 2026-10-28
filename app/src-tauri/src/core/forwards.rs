//! Port forwards (Swift `AppState` port forward section): save with validation, delete, start and stop
//! tunnels, their states, and the host key prompt of background connections.
//!
//! - Editing a running forward restarts it with the new settings.
//! - Deleting a forward or its connection stops it.
//! - Locking, switching the database and quitting stop every tunnel.
//! - [`running_count`](AppCore::running_count) counts starting and active tunnels.
//!
//! The secrets are opened from the vault on every start and stay in Rust: they go straight into the
//! russh connection.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use rd_core::forward::PortForwardDraft;
use rd_core::{ConnectionType, new_id};
use rd_ssh::background::tunnel::{ForwardState, Opener, Paused, StartClock, Tunnel, TunnelSpec};
use rd_ssh::background::{Client, ConnectSpec, HostKeyPrompt, HostKeyQuestion, StoredKey, runtime};
use serde::Serialize;
use tokio::sync::oneshot;

use super::{AppCore, CommandError};

/// A tunnel's state for the UI (event `port-forward-status`): `{id, connectionId, state, code?, error?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardStatusDto {
    /// The forward.
    pub id: String,
    /// Its SSH connection.
    pub connection_id: String,
    /// `state` (`starting`, `active`, `failed`, `stopped`) and, when failed, `code` and `error`.
    #[serde(flatten)]
    pub state: ForwardState,
}

/// An unknown or changed host key to accept or reject (event `host-key-prompt`); answer with
/// `host_key_answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyPromptDto {
    /// Answer with this id.
    pub request_id: String,
    /// The forward that is starting.
    pub forward_id: String,
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

/// What the forwards tell the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardNotice {
    /// A tunnel's state changed.
    Status(ForwardStatusDto),
    /// Ask about a host key.
    HostKeyPrompt(HostKeyPromptDto),
    /// The prompt is gone (its tunnel stopped): close the dialog.
    HostKeyClosed(String),
}

/// Receives forward notices (emits the events).
pub type ForwardNotifier = Arc<dyn Fn(ForwardNotice) + Send + Sync>;

struct Entry {
    connection_id: String,
    generation: u64,
    state: ForwardState,
    tunnel: Option<Tunnel>,
    /// The VPN the tunnel goes through, kept up until the forward stops.
    _vpn: Option<super::VpnHold>,
}

struct PendingHostKey {
    forward_id: String,
    answer: oneshot::Sender<bool>,
    _paused: Paused,
}

/// Test hooks: where the background connections look for the user's files, and whether the agent is used.
#[derive(Debug, Clone, Default)]
pub(crate) struct Paths {
    pub(crate) home: Option<PathBuf>,
    pub(crate) known_hosts: Option<Vec<PathBuf>>,
    pub(crate) no_agent: bool,
}

/// The tunnels and pending host key prompts.
pub(crate) struct ForwardShared {
    entries: Mutex<HashMap<String, Entry>>,
    prompts: Mutex<HashMap<String, PendingHostKey>>,
    notifier: Mutex<Option<ForwardNotifier>>,
    next_generation: Mutex<u64>,
    paths: Mutex<Option<Paths>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ForwardShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::default(),
            prompts: Mutex::default(),
            notifier: Mutex::default(),
            next_generation: Mutex::new(0),
            paths: Mutex::default(),
        })
    }

    fn notify(&self, notice: ForwardNotice) {
        let notifier = lock(&self.notifier).clone();
        if let Some(notifier) = notifier {
            notifier(notice);
        }
    }

    fn status(id: &str, connection_id: &str, state: ForwardState) -> ForwardNotice {
        ForwardNotice::Status(ForwardStatusDto {
            id: id.to_owned(),
            connection_id: connection_id.to_owned(),
            state,
        })
    }

    /// A state from the tunnel of `generation`; dropped when the tunnel was replaced or stopped.
    fn update(&self, id: &str, generation: u64, state: ForwardState) {
        let connection_id = {
            let mut entries = lock(&self.entries);
            let Some(entry) = entries.get_mut(id).filter(|e| e.generation == generation) else {
                return;
            };
            entry.state = state.clone();
            entry.connection_id.clone()
        };
        if !state.is_running() {
            self.cancel_prompts(id);
        }
        self.notify(Self::status(id, &connection_id, state));
    }

    /// Asks the UI about a host key; the start clock stays paused until the answer.
    fn ask_host_key(
        &self,
        forward_id: &str,
        question: HostKeyQuestion,
        clock: &StartClock,
    ) -> oneshot::Receiver<bool> {
        let (answer, receiver) = oneshot::channel();
        if lock(&self.notifier).is_none() {
            return receiver;
        }
        let request_id = new_id();
        lock(&self.prompts).insert(
            request_id.clone(),
            PendingHostKey {
                forward_id: forward_id.to_owned(),
                answer,
                _paused: clock.pause(),
            },
        );
        self.notify(ForwardNotice::HostKeyPrompt(HostKeyPromptDto {
            request_id,
            forward_id: forward_id.to_owned(),
            host: question.host,
            port: question.port,
            key_type: question.key_type,
            fingerprint: question.fingerprint,
            changed: question.changed,
        }));
        receiver
    }

    /// Rejects the open prompts of `forward_id` and tells the UI to close them.
    fn cancel_prompts(&self, forward_id: &str) {
        let cancelled: Vec<(String, PendingHostKey)> = {
            let mut prompts = lock(&self.prompts);
            let ids: Vec<String> = prompts
                .iter()
                .filter(|(_, p)| p.forward_id == forward_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| prompts.remove(&id).map(|p| (id, p)))
                .collect()
        };
        for (id, prompt) in cancelled {
            let _ = prompt.answer.send(false);
            self.notify(ForwardNotice::HostKeyClosed(id));
        }
    }

    /// Stops `id`'s tunnel and forgets it; reports `stopped` when it was running.
    fn stop(&self, id: &str) {
        let removed = lock(&self.entries).remove(id);
        self.cancel_prompts(id);
        if let Some(entry) = removed {
            if let Some(tunnel) = &entry.tunnel {
                tunnel.stop();
            }
            self.notify(Self::status(id, &entry.connection_id, ForwardState::Stopped));
        }
    }

    fn ids(&self, filter: impl Fn(&Entry) -> bool) -> Vec<String> {
        lock(&self.entries)
            .iter()
            .filter(|(_, e)| filter(e))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl AppCore {
    /// Sets the forward notifier (the Tauri layer emits events from it).
    pub fn set_forward_hooks(&self, notifier: ForwardNotifier) {
        *lock(&self.forwards.notifier) = Some(notifier);
    }

    /// Test hook: the home folder, `known_hosts` files and agent use of background connections.
    #[cfg(test)]
    pub(crate) fn set_forward_paths(&self, paths: Option<Paths>) {
        *lock(&self.forwards.paths) = paths;
    }

    /// Saves the forward editor's draft; a running forward restarts with the new settings. Returns the
    /// id.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `notFound` (connection), `notSsh`, `invalidLocalPort`,
    /// `missingRemoteHost`, `invalidRemotePort`, `duplicateLocalPort` or `database`.
    pub fn port_forward_save(&self, draft: &PortForwardDraft) -> Result<String, CommandError> {
        let id = {
            let inner = self.inner();
            let (db, _) = Self::writable(&inner)?;
            let library = db.library()?;
            let connection = library
                .connections
                .iter()
                .find(|c| c.id == draft.connection_id)
                .ok_or_else(|| CommandError::not_found(&draft.connection_id))?;
            if connection.kind != ConnectionType::Ssh {
                return Err(CommandError::new(
                    "notSsh",
                    "Port forwards need an SSH connection.",
                ));
            }
            let existing = draft
                .id
                .as_deref()
                .and_then(|id| library.port_forwards.iter().find(|f| f.id == id));
            let forward = draft.to_forward(existing, &library.port_forwards)?;
            db.save_port_forward(&forward)?;
            forward.id
        };
        let running = lock(&self.forwards.entries)
            .get(&id)
            .is_some_and(|e| e.state.is_running());
        if running {
            self.port_forward_start(&id)?;
        }
        Ok(id)
    }

    /// Stops and deletes a forward.
    ///
    /// # Errors
    /// `locked`, `readOnly` or `database`.
    pub fn port_forward_delete(&self, id: &str) -> Result<(), CommandError> {
        let inner = self.inner();
        let (db, _) = Self::writable(&inner)?;
        self.forwards.stop(id);
        db.delete_port_forward(id)?;
        Ok(())
    }

    /// Starts (or restarts) a forward's tunnel. Progress and failures arrive as `port-forward-status`.
    ///
    /// # Errors
    /// `locked`, `notFound`, `notSsh`, `emptyHost` or `corruptData` (secrets).
    pub fn port_forward_start(&self, id: &str) -> Result<(), CommandError> {
        let (forward, connection, secrets) = {
            let inner = self.inner();
            let (db, vault) = Self::unlocked(&inner)?;
            let library = db.library()?;
            let forward = library
                .port_forwards
                .iter()
                .find(|f| f.id == id)
                .cloned()
                .ok_or_else(|| CommandError::not_found(id))?;
            let connection = library
                .connections
                .iter()
                .find(|c| c.id == forward.connection_id)
                .ok_or_else(|| CommandError::not_found(&forward.connection_id))?;
            if connection.kind != ConnectionType::Ssh {
                return Err(CommandError::new(
                    "notSsh",
                    "Port forwards need an SSH connection.",
                ));
            }
            let (resolved, secrets) = rd_ssh::resolve(&library, connection, |s| vault.open(s))?;
            (forward, resolved, secrets)
        };
        let spec = self.background_spec(&connection, secrets)?;
        let vpn = self.vpn_gate(&forward.connection_id)?;

        self.forwards.stop(id);
        let generation = {
            let mut next = lock(&self.forwards.next_generation);
            *next += 1;
            *next
        };
        lock(&self.forwards.entries).insert(
            id.to_owned(),
            Entry {
                connection_id: forward.connection_id.clone(),
                generation,
                state: ForwardState::Starting,
                tunnel: None,
                _vpn: vpn,
            },
        );
        let weak: Weak<ForwardShared> = Arc::downgrade(&self.forwards);
        let forward_id = id.to_owned();
        let on_state = Arc::new(move |state: ForwardState| {
            if let Some(shared) = weak.upgrade() {
                shared.update(&forward_id, generation, state);
            }
        });
        let weak = Arc::downgrade(&self.forwards);
        let forward_id = id.to_owned();
        let tunnel = Tunnel::start(
            runtime().handle(),
            TunnelSpec {
                local_port: forward.local_port,
                remote_host: forward.remote_host.clone(),
                remote_port: forward.remote_port,
            },
            move |clock| {
                let prompt: HostKeyPrompt = Arc::new(move |question| match weak.upgrade() {
                    Some(shared) => shared.ask_host_key(&forward_id, question, &clock),
                    None => oneshot::channel().1,
                });
                async move {
                    let client = Client::connect(spec, Some(prompt)).await?;
                    Ok(Arc::new(client) as Arc<dyn Opener>)
                }
            },
            on_state,
        );
        if let Some(entry) = lock(&self.forwards.entries)
            .get_mut(id)
            .filter(|e| e.generation == generation)
        {
            entry.tunnel = Some(tunnel);
        }
        Ok(())
    }

    /// The russh connection settings of a resolved connection (shared with the file browser).
    ///
    /// # Errors
    /// `emptyHost`.
    pub(crate) fn background_spec(
        &self,
        connection: &rd_core::Connection,
        secrets: rd_ssh::SshSecrets,
    ) -> Result<ConnectSpec, CommandError> {
        let host = connection.host.trim_matches([' ', '\t']).to_owned();
        if host.is_empty() {
            return Err(CommandError::new("emptyHost", "The connection has no host."));
        }
        let paths = lock(&self.forwards.paths).clone();
        let home = match &paths {
            Some(paths) => paths.home.clone(),
            None => dirs::home_dir(),
        };
        let key_path = connection
            .ssh_key_path
            .as_deref()
            .map(|p| p.trim_matches([' ', '\t']))
            .filter(|p| !p.is_empty())
            .map(|p| rd_ssh::command::expand_tilde(p, home.as_deref()));
        Ok(ConnectSpec {
            host,
            port: Some(connection.effective_port()),
            user: connection.username.trim_matches([' ', '\t']).to_owned(),
            key_path,
            secrets,
            home,
            known_hosts: paths.as_ref().and_then(|p| p.known_hosts.clone()),
            use_agent: !paths.as_ref().is_some_and(|p| p.no_agent),
        })
    }

    /// Stops a forward's tunnel (nothing happens when it isn't running).
    pub fn port_forward_stop(&self, id: &str) {
        self.forwards.stop(id);
    }

    /// The states of the forwards that were started (running or failed).
    #[must_use]
    pub fn port_forward_statuses(&self) -> Vec<ForwardStatusDto> {
        let mut list: Vec<ForwardStatusDto> = lock(&self.forwards.entries)
            .iter()
            .map(|(id, e)| ForwardStatusDto {
                id: id.clone(),
                connection_id: e.connection_id.clone(),
                state: e.state.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// The user's answer to a host key prompt of a tunnel, a file browser or a key setup (an unknown or answered
    /// request is ignored).
    pub fn host_key_answer(&self, request_id: &str, accept: bool) {
        if self.files_host_key_answer(request_id, accept)
            || self.key_setup_host_key_answer(request_id, accept)
        {
            return;
        }
        if let Some(prompt) = lock(&self.forwards.prompts).remove(request_id) {
            let _ = prompt.answer.send(accept);
        }
    }

    /// Stops every tunnel: on lock, database switch and quit.
    pub fn stop_all_forwards(&self) {
        for id in self.forwards.ids(|_| true) {
            self.forwards.stop(&id);
        }
    }

    /// Stops the tunnels of connections that no longer exist (their forwards were cascade-deleted).
    pub(crate) fn stop_forwards_of(&self, connection_ids: &[String]) {
        for id in self.forwards.ids(|e| connection_ids.contains(&e.connection_id)) {
            self.forwards.stop(&id);
        }
    }

    /// Starting and active tunnels.
    #[must_use]
    pub fn running_forwards(&self) -> usize {
        lock(&self.forwards.entries)
            .values()
            .filter(|e| e.state.is_running())
            .count()
    }
}
