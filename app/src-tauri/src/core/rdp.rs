//! Embedded RDP sessions (Swift `AppState.openRDPSession` and `RDPSession`): opening a session for a
//! connection with its credential resolved from the vault, input through the keyboard translator,
//! resize, reconnect, type clipboard, clipboard sync, and the certificate and NLA credential questions the
//! engine asks while connecting ("Save to this connection" seals the answer into the connection).
//!
//! The engine is behind [`RdpStarter`] so the flows are tested without a server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use rd_core::rdp::keyboard::{KeyAction, KeyInput, KeyboardPlatform, KeyboardTranslator};
use rd_core::{ConnectionType, new_id};
use rd_rdp::clipfiles::LocalFiles;
use rd_rdp::{
    CertificateChoice, CertificateRequest, Credentials, CredentialsRequest, InputEvent, MouseButton,
    RdpConfig, RdpEvent, RdpHandler, RdpSession,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{AppCore, CommandError, SessionsChanged};

/// Where a session's frames and events go (the Tauri channel and `rdp-event`).
pub trait RdpSink: Send + Sync {
    /// A frame message (`rd_rdp::frame`).
    fn frame(&self, frame: Vec<u8>);
    /// An event.
    fn event(&self, event: RdpEvent);
}

/// What a running engine offers; implemented by [`RdpSession`] (tests use a fake).
pub trait RdpRun: Send + Sync {
    /// Sends input.
    fn input(&self, events: Vec<InputEvent>);
    /// The view size changed.
    fn resize(&self, width: u32, height: u32);
    /// Offers local clipboard text.
    fn set_clipboard(&self, text: String);
    /// Offers local clipboard files (`None`: files that can't be offered).
    fn set_clipboard_files(&self, files: Option<Arc<LocalFiles>>);
    /// Cancels the running download of remote clipboard files.
    fn cancel_file_transfer(&self);
    /// Types text; `false` when not connected or busy.
    fn type_text(&self, text: &str) -> bool;
    /// A frame was painted.
    fn ack_frame(&self);
    /// Ends the session.
    fn close(&self);
    /// The engine still runs.
    fn is_running(&self) -> bool;
    /// The desktop is up.
    fn is_connected(&self) -> bool;
}

impl RdpRun for RdpSession {
    fn input(&self, events: Vec<InputEvent>) {
        RdpSession::input(self, events);
    }
    fn resize(&self, width: u32, height: u32) {
        RdpSession::resize(self, width, height);
    }
    fn set_clipboard(&self, text: String) {
        RdpSession::set_clipboard(self, text);
    }
    fn set_clipboard_files(&self, files: Option<Arc<LocalFiles>>) {
        RdpSession::set_clipboard_files(self, files);
    }
    fn cancel_file_transfer(&self) {
        RdpSession::cancel_file_transfer(self);
    }
    fn type_text(&self, text: &str) -> bool {
        RdpSession::type_text(self, text)
    }
    fn ack_frame(&self) {
        RdpSession::ack_frame(self);
    }
    fn close(&self) {
        RdpSession::close(self);
    }
    fn is_running(&self) -> bool {
        RdpSession::is_running(self)
    }
    fn is_connected(&self) -> bool {
        RdpSession::is_connected(self)
    }
}

/// Starts an engine.
pub type RdpStarter =
    Box<dyn Fn(RdpConfig, Arc<dyn RdpHandler>) -> std::io::Result<Arc<dyn RdpRun>> + Send + Sync>;

fn start_engine(config: RdpConfig, handler: Arc<dyn RdpHandler>) -> std::io::Result<Arc<dyn RdpRun>> {
    Ok(Arc::new(RdpSession::start(config, handler)?))
}

/// The certificate question for the UI (event `rdp-cert-prompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpCertPromptDto {
    /// Answer with `rdp_cert_answer`.
    pub request_id: String,
    /// The session asking.
    pub session_id: String,
    /// The connection's name.
    pub connection_name: String,
    /// The host connected to.
    pub host: String,
    /// The port.
    pub port: u16,
    /// SHA-256, `AB:CD:…`.
    pub fingerprint: String,
    /// A different certificate was accepted before: warn.
    pub changed: bool,
    /// The certificate is not for this host name: warn.
    pub name_mismatch: bool,
    /// The certificate's common name.
    pub common_name: String,
    /// Its subject.
    pub subject: String,
    /// Its issuer.
    pub issuer: String,
}

/// The NLA sign-in question (event `rdp-credentials-prompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpCredentialsPromptDto {
    /// Answer with `rdp_credentials_answer`.
    pub request_id: String,
    /// The session asking.
    pub session_id: String,
    /// "Sign in to <name>".
    pub connection_name: String,
    /// Pre-filled user name.
    pub username: String,
    /// Pre-filled domain.
    pub domain: String,
    /// Offer "Save to this connection" (no linked credential).
    pub can_save: bool,
    /// The server rejected the previous credentials.
    pub rejected: bool,
}

/// The answer to an NLA prompt.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpCredentialsAnswer {
    /// User name; `DOMAIN\user` is split.
    pub username: String,
    /// Domain.
    #[serde(default)]
    pub domain: String,
    /// Password.
    #[serde(deserialize_with = "zeroizing")]
    pub password: Zeroizing<String>,
    /// "Save to this connection" was ticked.
    #[serde(default)]
    pub save: bool,
}

fn zeroizing<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(deserializer).map(Zeroizing::new)
}

impl std::fmt::Debug for RdpCredentialsAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpCredentialsAnswer")
            .field("username", &self.username)
            .field("domain", &self.domain)
            .field("save", &self.save)
            .finish_non_exhaustive()
    }
}

/// What the RDP side tells the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RdpNotice {
    /// Show the certificate dialog.
    Certificate(RdpCertPromptDto),
    /// Show the sign-in dialog.
    Credentials(RdpCredentialsPromptDto),
    /// The prompt's session closed: close its dialog.
    Closed(String),
}

/// Receives [`RdpNotice`]s.
pub type RdpNotifier = Arc<dyn Fn(RdpNotice) + Send + Sync>;

/// One canvas input event (`rdp_input`). Coordinates are in view (CSS) pixels of the canvas; they are
/// mapped onto the desktop, which may still have another size (smart sizing).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RdpInput {
    /// `keydown` / `keyup`: `KeyboardEvent.code` and `.key`.
    Key {
        /// Physical key.
        code: String,
        /// Character or key name.
        key: String,
        /// `keydown`.
        down: bool,
    },
    /// `pointermove`.
    MouseMove {
        /// X in the view.
        x: f64,
        /// Y in the view.
        y: f64,
    },
    /// `pointerdown` / `pointerup`; `button` as in `MouseEvent.button` (0 left, 1 middle, 2 right,
    /// 3 back, 4 forward).
    MouseButton {
        /// DOM button number.
        button: u8,
        /// Pressed.
        down: bool,
        /// X in the view.
        x: f64,
        /// Y in the view.
        y: f64,
    },
    /// `wheel`: `deltaX`, `deltaY` and `deltaMode` as the DOM reports them.
    Wheel {
        /// Horizontal delta (positive: right).
        delta_x: f64,
        /// Vertical delta (positive: down).
        delta_y: f64,
        /// 0 pixels, 1 lines, 2 pages.
        #[serde(default)]
        delta_mode: u8,
        /// X in the view.
        x: f64,
        /// Y in the view.
        y: f64,
    },
}

/// What "type clipboard" did (`rdp_type_clipboard`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TypeClipboardDto {
    /// Typing started; `typed {finished}` follows as an `rdp-event`.
    Typing {
        /// Characters to type.
        chars: usize,
    },
    /// More than 2000 characters: ask "Type a long text?", then call again with `confirmed`.
    NeedsConfirmation {
        /// Characters.
        chars: usize,
        /// "Typing takes about N s".
        seconds: usize,
    },
    /// The clipboard holds no text.
    Empty,
    /// Not connected, or already typing.
    Busy,
}

/// The engine, for Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpEngineInfoDto {
    /// "FreeRDP".
    pub name: &'static str,
    /// Its version.
    pub version: &'static str,
}

enum Pending {
    Certificate {
        session_id: String,
        answer: mpsc::Sender<CertificateChoice>,
    },
    Credentials {
        session_id: String,
        connection_id: String,
        can_save: bool,
        answer: mpsc::Sender<Option<Credentials>>,
    },
}

impl Pending {
    fn session_id(&self) -> &str {
        match self {
            Self::Certificate { session_id, .. } | Self::Credentials { session_id, .. } => session_id,
        }
    }
}

struct Entry {
    connection_id: String,
    connection_name: String,
    can_save: bool,
    sink: Arc<dyn RdpSink>,
    /// The run whose output passes; bumped for each start.
    current: u64,
    run: Option<Arc<dyn RdpRun>>,
    translator: KeyboardTranslator,
    view: (u32, u32),
    desktop: (u32, u32),
}

/// The RDP state of the app; shared with the session handlers.
pub(crate) struct RdpShared {
    sessions: Mutex<HashMap<String, Entry>>,
    prompts: Mutex<HashMap<String, Pending>>,
    notifier: Mutex<Option<RdpNotifier>>,
    changed: Mutex<Option<SessionsChanged>>,
    starter: Mutex<RdpStarter>,
    next_run: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RdpShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::default(),
            prompts: Mutex::default(),
            notifier: Mutex::default(),
            changed: Mutex::default(),
            starter: Mutex::new(Box::new(start_engine)),
            next_run: AtomicU64::new(0),
        })
    }

    fn notify(&self, notice: RdpNotice) {
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

    fn is_current(&self, session_id: &str, number: u64) -> bool {
        lock(&self.sessions)
            .get(session_id)
            .is_some_and(|e| e.current == number)
    }

    /// Refuses the open prompts of `session_id` and tells the UI to close them.
    fn cancel_prompts(&self, session_id: &str) {
        let cancelled: Vec<(String, Pending)> = {
            let mut prompts = lock(&self.prompts);
            let ids: Vec<String> = prompts
                .iter()
                .filter(|(_, p)| p.session_id() == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| prompts.remove(&id).map(|p| (id, p)))
                .collect()
        };
        for (id, pending) in cancelled {
            match pending {
                Pending::Certificate { answer, .. } => {
                    let _ = answer.send(CertificateChoice::Reject);
                }
                Pending::Credentials { answer, .. } => {
                    let _ = answer.send(None);
                }
            }
            self.notify(RdpNotice::Closed(id));
        }
    }
}

/// The engine's handler for one run of a session.
struct Handler {
    shared: Weak<RdpShared>,
    session_id: String,
    number: u64,
    connection_id: String,
    connection_name: String,
    can_save: bool,
    sink: Arc<dyn RdpSink>,
}

impl Handler {
    fn current(&self) -> Option<Arc<RdpShared>> {
        self.shared
            .upgrade()
            .filter(|s| s.is_current(&self.session_id, self.number))
    }
}

impl RdpHandler for Handler {
    fn frame(&self, frame: Vec<u8>) {
        if self.current().is_some() {
            self.sink.frame(frame);
        }
    }

    fn event(&self, event: RdpEvent) {
        let Some(shared) = self.current() else {
            return;
        };
        match &event {
            RdpEvent::Connected { width, height } | RdpEvent::Resized { width, height } => {
                if let Some(entry) = lock(&shared.sessions).get_mut(&self.session_id) {
                    entry.desktop = (u32::from(*width), u32::from(*height));
                }
            }
            _ => {}
        }
        let ended = matches!(event, RdpEvent::Disconnected { .. });
        let connected = matches!(event, RdpEvent::Connected { .. });
        self.sink.event(event);
        if ended {
            shared.cancel_prompts(&self.session_id);
        }
        if ended || connected {
            shared.sessions_changed();
        }
    }

    fn certificate(&self, request: CertificateRequest) -> CertificateChoice {
        let Some(shared) = self.current() else {
            return CertificateChoice::Reject;
        };
        if lock(&shared.notifier).is_none() {
            return CertificateChoice::Reject;
        }
        let (answer, receiver) = mpsc::channel();
        let request_id = new_id();
        lock(&shared.prompts).insert(
            request_id.clone(),
            Pending::Certificate {
                session_id: self.session_id.clone(),
                answer,
            },
        );
        shared.notify(RdpNotice::Certificate(RdpCertPromptDto {
            request_id,
            session_id: self.session_id.clone(),
            connection_name: self.connection_name.clone(),
            host: request.host,
            port: request.port,
            fingerprint: request.fingerprint,
            changed: request.changed,
            name_mismatch: request.name_mismatch,
            common_name: request.common_name,
            subject: request.subject,
            issuer: request.issuer,
        }));
        drop(shared);
        receiver.recv().unwrap_or(CertificateChoice::Reject)
    }

    fn credentials(&self, request: CredentialsRequest) -> Option<Credentials> {
        let shared = self.current()?;
        lock(&shared.notifier).as_ref()?;
        let (answer, receiver) = mpsc::channel();
        let request_id = new_id();
        lock(&shared.prompts).insert(
            request_id.clone(),
            Pending::Credentials {
                session_id: self.session_id.clone(),
                connection_id: self.connection_id.clone(),
                can_save: self.can_save,
                answer,
            },
        );
        shared.notify(RdpNotice::Credentials(RdpCredentialsPromptDto {
            request_id,
            session_id: self.session_id.clone(),
            connection_name: self.connection_name.clone(),
            username: request.username,
            domain: request.domain,
            can_save: self.can_save,
            rejected: request.rejected,
        }));
        drop(shared);
        receiver.recv().ok().flatten()
    }
}

fn session_not_found(id: &str) -> CommandError {
    CommandError::new("sessionNotFound", "The session doesn't exist any more.").with_detail(id)
}

/// RDP wheel units (120 per notch, positive away from the user / to the right) for a DOM wheel delta.
#[must_use]
pub fn wheel_units(delta: f64, mode: u8) -> i16 {
    let notches = match mode {
        // Lines: three per notch in most browsers.
        1 => delta / 3.0,
        // Pages.
        2 => delta * 3.0,
        // Pixels: ~100 per notch.
        _ => delta / 100.0,
    };
    let units = (notches * 120.0).round().clamp(-30_000.0, 30_000.0);
    // In range after the clamp.
    #[allow(clippy::cast_possible_truncation)]
    let units = units as i16;
    units
}

fn dom_button(button: u8) -> Option<MouseButton> {
    match button {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Middle),
        2 => Some(MouseButton::Right),
        3 => Some(MouseButton::X1),
        4 => Some(MouseButton::X2),
        _ => None,
    }
}

fn key_events(actions: Vec<KeyAction>) -> impl Iterator<Item = InputEvent> {
    actions.into_iter().map(|action| match action {
        KeyAction::Scancode { code, down } => InputEvent::Scancode { code, down },
        KeyAction::Unicode(unit) => InputEvent::Unicode(unit),
    })
}

/// The canvas input of a session turned into engine input: keys through the translator, points mapped
/// from the view onto the desktop.
fn translate(entry: &mut Entry, inputs: Vec<RdpInput>) -> Vec<InputEvent> {
    let mut events = Vec::with_capacity(inputs.len());
    let point =
        |entry: &Entry, x: f64, y: f64| rd_core::rdp::view_to_desktop((x, y), entry.view, entry.desktop);
    for input in inputs {
        match input {
            RdpInput::Key { code, key, down } => {
                let actions = entry.translator.key(&KeyInput { code, key, down });
                events.extend(key_events(actions));
            }
            RdpInput::MouseMove { x, y } => {
                let (x, y) = point(entry, x, y);
                events.push(InputEvent::MouseMove { x, y });
            }
            RdpInput::MouseButton { button, down, x, y } => {
                if let Some(button) = dom_button(button) {
                    let (x, y) = point(entry, x, y);
                    events.push(InputEvent::MouseButton { button, down, x, y });
                }
            }
            RdpInput::Wheel {
                delta_x,
                delta_y,
                delta_mode,
                x,
                y,
            } => {
                let (x, y) = point(entry, x, y);
                // DOM down is RDP negative; DOM right is RDP positive.
                let vertical = wheel_units(-delta_y, delta_mode);
                let horizontal = wheel_units(delta_x, delta_mode);
                if vertical != 0 {
                    events.push(InputEvent::Wheel {
                        delta: vertical,
                        horizontal: false,
                        x,
                        y,
                    });
                }
                if horizontal != 0 {
                    events.push(InputEvent::Wheel {
                        delta: horizontal,
                        horizontal: true,
                        x,
                        y,
                    });
                }
            }
        }
    }
    events
}

/// The computer name the server sees.
fn client_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|h| h.split('.').next().unwrap_or_default().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "RemoteDesktop".to_owned())
}

impl AppCore {
    /// Sets the prompt notifier and the sessions-changed hook (the Tauri layer emits events from them).
    pub fn set_rdp_hooks(&self, notifier: RdpNotifier, changed: SessionsChanged) {
        *lock(&self.rdp.notifier) = Some(notifier);
        *lock(&self.rdp.changed) = Some(changed);
    }

    /// Test hook: start sessions with `starter` instead of the FreeRDP engine.
    pub fn set_rdp_starter(&self, starter: RdpStarter) {
        *lock(&self.rdp.starter) = starter;
    }

    /// The engine configuration for an RDP connection with its credential resolved, plus its name and
    /// whether credentials may be saved into it (no linked credential).
    fn rdp_config(
        &self,
        connection_id: &str,
        width: u32,
        height: u32,
    ) -> Result<(RdpConfig, String, bool), CommandError> {
        let target = self.launch_target(connection_id, ConnectionType::Rdp)?;
        let connection = &target.connection;
        let host = connection.host.trim().to_owned();
        if host.is_empty() {
            return Err(CommandError::new("emptyHost", "The connection has no host."));
        }
        let (username, domain) = rd_core::rdp::split_user(&connection.username, &connection.domain);
        let known_hosts: PathBuf = self.inner().settings_dir.join(rd_rdp::known_hosts::FILE_NAME);
        let config = RdpConfig {
            host,
            port: connection.effective_port(),
            username,
            domain,
            password: Zeroizing::new(target.password().unwrap_or_default().to_owned()),
            admin_session: connection.rdp_admin_session,
            width,
            height,
            client_name: client_name(),
            known_hosts: Some(known_hosts),
            auto_reconnect: true,
            clipboard_dir: Some(rd_platform::clipboard::rdp_clipboard_session_dir()),
        };
        Ok((
            config,
            connection.name.clone(),
            connection.credential_id.is_none(),
        ))
    }

    /// Starts a run for the existing entry `session_id` and makes it current.
    fn start_rdp_run(&self, session_id: &str, config: RdpConfig) -> Result<(), CommandError> {
        let number = self.rdp.next_run.fetch_add(1, Ordering::SeqCst) + 1;
        let handler = {
            let mut sessions = lock(&self.rdp.sessions);
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            entry.current = number;
            entry.view = (config.width, config.height);
            let (w, h) = rd_core::rdp::initial_size(config.width, config.height);
            entry.desktop = (w, h);
            entry.translator = KeyboardTranslator::new(KeyboardPlatform::current());
            Arc::new(Handler {
                shared: Arc::downgrade(&self.rdp),
                session_id: session_id.to_owned(),
                number,
                connection_id: entry.connection_id.clone(),
                connection_name: entry.connection_name.clone(),
                can_save: entry.can_save,
                sink: Arc::clone(&entry.sink),
            })
        };
        let run = (lock(&self.rdp.starter))(config, handler).map_err(|e| {
            CommandError::new("rdpFailed", "Couldn't start the RDP session.").with_detail(e.to_string())
        })?;
        if let Some(entry) = lock(&self.rdp.sessions).get_mut(session_id) {
            entry.run = Some(run);
        }
        Ok(())
    }

    /// Opens an embedded RDP session for a connection in a view of `width`×`height`; frames and events
    /// go to the sink `make_sink` builds for the new session id. Returns the session id.
    ///
    /// # Errors
    /// `locked`, `noDatabase`, `notFound`, `wrongType`, `emptyHost`, `rdpFailed` or `corruptData`.
    pub fn open_rdp_session(
        &self,
        connection_id: &str,
        width: u32,
        height: u32,
        make_sink: impl FnOnce(&str) -> Arc<dyn RdpSink>,
    ) -> Result<String, CommandError> {
        let (config, connection_name, can_save) = self.rdp_config(connection_id, width, height)?;
        let session_id = new_id();
        lock(&self.rdp.sessions).insert(
            session_id.clone(),
            Entry {
                connection_id: connection_id.to_owned(),
                connection_name,
                can_save,
                sink: make_sink(&session_id),
                current: 0,
                run: None,
                translator: KeyboardTranslator::new(KeyboardPlatform::current()),
                view: (width, height),
                desktop: (width, height),
            },
        );
        if let Err(e) = self.start_rdp_run(&session_id, config) {
            lock(&self.rdp.sessions).remove(&session_id);
            return Err(e);
        }
        self.rdp.sessions_changed();
        Ok(session_id)
    }

    /// Connects the session again in the same tab, with the connection and credential resolved anew. The
    /// old run is closed first and reports nothing more.
    ///
    /// # Errors
    /// `sessionNotFound`, or the errors of [`open_rdp_session`](Self::open_rdp_session).
    pub fn rdp_reconnect(&self, session_id: &str, width: u32, height: u32) -> Result<(), CommandError> {
        let (connection_id, old) = {
            let mut sessions = lock(&self.rdp.sessions);
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            entry.current = 0;
            (entry.connection_id.clone(), entry.run.take())
        };
        if let Some(old) = old {
            old.close();
        }
        self.rdp.cancel_prompts(session_id);
        let (config, connection_name, can_save) = self.rdp_config(&connection_id, width, height)?;
        if let Some(entry) = lock(&self.rdp.sessions).get_mut(session_id) {
            entry.connection_name = connection_name;
            entry.can_save = can_save;
        }
        self.start_rdp_run(session_id, config)?;
        self.rdp.sessions_changed();
        Ok(())
    }

    fn with_rdp<T>(&self, session_id: &str, work: impl FnOnce(&mut Entry) -> T) -> Result<T, CommandError> {
        let mut sessions = lock(&self.rdp.sessions);
        let entry = sessions
            .get_mut(session_id)
            .ok_or_else(|| session_not_found(session_id))?;
        Ok(work(entry))
    }

    fn rdp_run(&self, session_id: &str) -> Result<Arc<dyn RdpRun>, CommandError> {
        self.with_rdp(session_id, |e| e.run.clone())?
            .ok_or_else(|| session_not_found(session_id))
    }

    /// The page painted a frame.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_ack_frame(&self, session_id: &str) -> Result<(), CommandError> {
        self.rdp_run(session_id)?.ack_frame();
        Ok(())
    }

    /// Canvas input, in order.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_input(&self, session_id: &str, inputs: Vec<RdpInput>) -> Result<(), CommandError> {
        let (run, events) =
            self.with_rdp(session_id, |entry| (entry.run.clone(), translate(entry, inputs)))?;
        if let Some(run) = run {
            run.input(events);
        }
        Ok(())
    }

    /// Engine input as it is (the Windows/Linux keyboard capture forwards scancodes this way).
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_raw_input(&self, session_id: &str, events: Vec<InputEvent>) -> Result<(), CommandError> {
        self.rdp_run(session_id)?.input(events);
        Ok(())
    }

    /// Releases every key the session holds down (the canvas lost focus).
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_release_all_keys(&self, session_id: &str) -> Result<(), CommandError> {
        let (run, events) = self.with_rdp(session_id, |entry| {
            let actions = entry.translator.release_all();
            (entry.run.clone(), key_events(actions).collect::<Vec<_>>())
        })?;
        if let Some(run) = run {
            run.input(events);
        }
        Ok(())
    }

    /// The view now measures `width`×`height`; the desktop follows (debounced in the engine).
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_resize(&self, session_id: &str, width: u32, height: u32) -> Result<(), CommandError> {
        let run = self.with_rdp(session_id, |entry| {
            entry.view = (width, height);
            entry.run.clone()
        })?;
        if let Some(run) = run {
            run.resize(width, height);
        }
        Ok(())
    }

    /// Ends the session and forgets it (its prompts are refused).
    pub fn rdp_close(&self, session_id: &str) {
        let entry = lock(&self.rdp.sessions).remove(session_id);
        if let Some(run) = entry.and_then(|e| e.run) {
            run.close();
        }
        self.rdp.cancel_prompts(session_id);
        self.rdp.sessions_changed();
    }

    /// "Type clipboard": types `text` (the clipboard's text, `None` when it has none) into the session.
    /// Above 2000 characters it only reports `needsConfirmation` unless `confirmed`.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_type_text(
        &self,
        session_id: &str,
        text: Option<&str>,
        confirmed: bool,
    ) -> Result<TypeClipboardDto, CommandError> {
        let run = self.rdp_run(session_id)?;
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return Ok(TypeClipboardDto::Empty);
        };
        let chars = text.chars().count();
        if rd_core::rdp::typing_needs_confirmation(text) && !confirmed {
            return Ok(TypeClipboardDto::NeedsConfirmation {
                chars,
                seconds: rd_core::rdp::typing_seconds(chars),
            });
        }
        Ok(if run.type_text(text) {
            TypeClipboardDto::Typing { chars }
        } else {
            TypeClipboardDto::Busy
        })
    }

    /// Offers local clipboard text to every connected session.
    pub fn rdp_set_clipboard_all(&self, text: &str) {
        let runs: Vec<Arc<dyn RdpRun>> = lock(&self.rdp.sessions)
            .values()
            .filter_map(|e| e.run.clone())
            .filter(|r| r.is_connected())
            .collect();
        for run in runs {
            run.set_clipboard(text.to_owned());
        }
    }

    /// Offers the local clipboard's files to every connected session. A list that can't be offered
    /// (`Err`, the reason) clears what the sessions offer and is reported to each as
    /// `fileTransferFailed`.
    pub fn rdp_set_clipboard_files_all(&self, files: &Result<Arc<LocalFiles>, String>) {
        let targets: Vec<(Arc<dyn RdpRun>, Arc<dyn RdpSink>)> = lock(&self.rdp.sessions)
            .values()
            .filter_map(|e| e.run.clone().map(|run| (run, Arc::clone(&e.sink))))
            .filter(|(run, _)| run.is_connected())
            .collect();
        for (run, sink) in targets {
            match files {
                Ok(files) => run.set_clipboard_files(Some(Arc::clone(files))),
                Err(message) => {
                    run.set_clipboard_files(None);
                    sink.event(RdpEvent::FileTransferFailed {
                        message: message.clone(),
                    });
                }
            }
        }
    }

    /// Cancels the session's download of remote clipboard files.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn rdp_cancel_file_transfer(&self, session_id: &str) -> Result<(), CommandError> {
        self.rdp_run(session_id)?.cancel_file_transfer();
        Ok(())
    }

    /// Whether any session is connected (the clipboard sync runs only then).
    #[must_use]
    pub fn rdp_any_connected(&self) -> bool {
        lock(&self.rdp.sessions)
            .values()
            .any(|e| e.run.as_ref().is_some_and(|r| r.is_connected()))
    }

    /// The answer to a certificate prompt. Unknown ids (answered or cancelled) are ignored.
    pub fn rdp_cert_answer(&self, request_id: &str, choice: CertificateChoice) {
        let pending = lock(&self.rdp.prompts).remove(request_id);
        if let Some(Pending::Certificate { answer, .. }) = pending {
            let _ = answer.send(choice);
        }
    }

    /// The answer to a sign-in prompt; `None` cancels (the session ends cleanly). With `save` (and no
    /// linked credential) the user name, domain and password are stored in the connection first. Returns
    /// whether the connection was changed.
    ///
    /// # Errors
    /// `readOnly`, `locked`, `notFound` or `database` when saving fails; the session gets the answer
    /// anyway.
    pub fn rdp_credentials_answer(
        &self,
        request_id: &str,
        answer: Option<RdpCredentialsAnswer>,
    ) -> Result<bool, CommandError> {
        let Some(Pending::Credentials {
            connection_id,
            can_save,
            answer: reply,
            ..
        }) = lock(&self.rdp.prompts).remove(request_id)
        else {
            return Ok(false);
        };
        let Some(answer) = answer else {
            let _ = reply.send(None);
            return Ok(false);
        };
        let (username, domain) = rd_core::rdp::split_user(&answer.username, &answer.domain);
        let saved = if answer.save && can_save {
            self.save_rdp_credentials(&connection_id, &username, &domain, &answer.password)
        } else {
            Ok(false)
        };
        let _ = reply.send(Some(Credentials {
            username,
            domain,
            password: answer.password,
        }));
        saved
    }

    /// Stores what was typed into the NLA prompt on the connection itself (Swift `saveRDPCredentials`).
    fn save_rdp_credentials(
        &self,
        connection_id: &str,
        username: &str,
        domain: &str,
        password: &str,
    ) -> Result<bool, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::writable(&inner)?;
        let mut connection = db
            .connection(connection_id)?
            .ok_or_else(|| CommandError::not_found(connection_id))?;
        username.clone_into(&mut connection.username);
        domain.clone_into(&mut connection.domain);
        connection.password_enc = vault.seal_optional(Some(password));
        db.save_connection(&connection)?;
        Ok(true)
    }

    /// Sessions whose engine still runs (connecting, connected or reconnecting).
    #[must_use]
    pub fn running_rdp_sessions(&self) -> usize {
        lock(&self.rdp.sessions)
            .values()
            .filter(|e| e.run.as_ref().is_some_and(|r| r.is_running()))
            .count()
    }

    /// Open RDP tabs, running or ended.
    #[must_use]
    pub fn rdp_session_count(&self) -> usize {
        lock(&self.rdp.sessions).len()
    }

    /// The engine's name and version, for Settings.
    #[must_use]
    pub fn rdp_engine_info() -> RdpEngineInfoDto {
        RdpEngineInfoDto {
            name: rd_rdp::ENGINE_NAME,
            version: rd_rdp::ENGINE_VERSION,
        }
    }

    /// On quit: ends every session and refuses every prompt.
    pub fn shutdown_rdp(&self) {
        let sessions: Vec<(String, Entry)> = lock(&self.rdp.sessions).drain().collect();
        for (id, entry) in sessions {
            if let Some(run) = entry.run {
                run.close();
            }
            self.rdp.cancel_prompts(&id);
        }
    }
}
