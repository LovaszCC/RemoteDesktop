//! The session engine on FreeRDP: connect (with the certificate and credential questions), run the
//! session, reconnect after a dropped connection, and report how it ended. Runs on the session thread
//! (see [`crate::session`]); every FreeRDP call happens there.

pub mod certificate;
pub mod clipboard;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use rd_core::rdp::keyboard::{SC_RETURN, SC_TAB};
use rd_core::rdp::{CANCELLED_LAST_ERROR, DisconnectReason, ResizeDebouncer, TypedKey};
use rd_freerdp::{ClipEvent, Connection, ContentsRequest, Handler};
use zeroize::Zeroizing;

use self::certificate::ServerCertificate;
use self::clipboard::{Action, CB_HUGE_FILE_SUPPORT_ENABLED, SessionClipboard};
use crate::clipfiles::download::{CANCELLED, ContentsKind, Downloader, FileRequest, Step};
use crate::clipfiles::{self, LocalFiles, RemoteFile, file_list};
use crate::frame::{self, Dirty, MAX_IN_FLIGHT, Pixels, Rect};
use crate::known_hosts::{self, KnownHosts, Verdict};
use crate::session::{
    CertificateChoice, CertificateRequest, Command, CredentialsRequest, InputEvent, MouseButton, RdpConfig,
    RdpEvent, RdpHandler, Shared,
};

/// How often dirty rectangles go out.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// Reconnect attempts after a dropped connection, and the pause before each.
const RECONNECT_ATTEMPTS: u32 = 10;
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// How long a resize waits when Display Control isn't open yet.
const RESIZE_RETRY: Duration = Duration::from_millis(250);
/// How often a running download's watchdog looks.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);
/// Least time between two download progress events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// The longest a poll waits when nothing is due.
const IDLE_POLL: Duration = Duration::from_millis(500);
/// `FreeRDP` last errors that mean the credentials were wrong (`ERRCONNECT_*`).
const AUTH_FAILURES: [u32; 5] = [
    0x0002_0009, // authentication failed
    0x0002_0014, // logon failure
    0x0002_0015, // wrong password
    0x0002_0016, // access denied
    0x0002_001B, // no or missing credentials
];

/// How the session ended.
enum End {
    /// The user closed the session.
    Closed,
    /// The server ended it.
    Terminated { clean: bool, code: u32, message: String },
    /// The connection dropped without a reason; reconnects when allowed.
    Dropped,
}

/// What `FreeRDP` reported during a connect or a poll, handled afterwards by the session loop.
#[derive(Debug)]
enum Inbound {
    Desktop(u32, u32),
    Painted(Vec<Rect>),
    Pointer(RdpEvent),
    DisplayControlReady,
    Clip(ClipEvent),
}

/// The connection's handler: answers the two questions while connecting and queues everything else.
struct Events {
    handler: Arc<dyn RdpHandler>,
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
    /// The certificate accepted during this session (reconnects don't ask again).
    accepted: Option<String>,
    username: String,
    domain: String,
    password: Zeroizing<String>,
    /// The next credential prompt says the last ones were rejected.
    rejected: bool,
    /// The user cancelled the credential prompt.
    cancelled: bool,
    /// The user rejected the certificate.
    certificate_rejected: bool,
    inbox: Vec<Inbound>,
}

impl Events {
    /// The certificate check: the session's accepted one, then the store, then the user.
    fn verify(&mut self, certificate: &ServerCertificate) -> bool {
        let fingerprint = known_hosts::fingerprint(&certificate.der);
        if self.accepted.as_ref() == Some(&fingerprint) {
            return true;
        }
        let store = self.known_hosts.as_ref().map(KnownHosts::new);
        let verdict = store
            .as_ref()
            .map_or(Verdict::New, |s| s.check(&self.host, self.port, &fingerprint));
        if verdict == Verdict::Same {
            self.accepted = Some(fingerprint);
            return true;
        }
        let request = CertificateRequest {
            host: self.host.clone(),
            port: self.port,
            fingerprint: fingerprint.clone(),
            changed: matches!(verdict, Verdict::Changed { .. }),
            name_mismatch: !known_hosts::host_matches(&self.host, &certificate.names),
            common_name: certificate.common_name.clone(),
            subject: certificate.subject.clone(),
            issuer: certificate.issuer.clone(),
        };
        match self.handler.certificate(request) {
            CertificateChoice::AcceptPermanently => {
                if let Some(store) = store {
                    // A store that can't be written only means the question comes again next time.
                    let _ = store.accept(&self.host, self.port, &fingerprint);
                }
            }
            CertificateChoice::AcceptOnce => {}
            CertificateChoice::Reject => {
                self.certificate_rejected = true;
                return false;
            }
        }
        self.accepted = Some(fingerprint);
        true
    }

    /// Asks for NLA credentials; `false` when cancelled.
    fn ask_credentials(&mut self) -> bool {
        let request = CredentialsRequest {
            username: self.username.clone(),
            domain: self.domain.clone(),
            rejected: std::mem::take(&mut self.rejected),
        };
        let Some(credentials) = self.handler.credentials(request) else {
            self.cancelled = true;
            return false;
        };
        self.username = credentials.username;
        self.domain = credentials.domain;
        self.password = credentials.password;
        true
    }
}

impl Handler for Events {
    fn verify_certificate(&mut self, pem: &[u8], _host: &str, _port: u16, _flags: u32) -> bool {
        ServerCertificate::from_pem(pem).is_ok_and(|certificate| self.verify(&certificate))
    }

    fn authenticate(&mut self) -> Option<rd_freerdp::Credentials> {
        self.ask_credentials().then(|| rd_freerdp::Credentials {
            username: self.username.clone(),
            domain: self.domain.clone(),
            password: self.password.to_string(),
        })
    }

    fn desktop(&mut self, width: u32, height: u32) {
        self.inbox.push(Inbound::Desktop(width, height));
    }

    fn painted(&mut self, rects: &[rd_freerdp::Rect]) {
        let rects = rects.iter().filter_map(|r| frame_rect(*r)).collect();
        self.inbox.push(Inbound::Painted(rects));
    }

    fn pointer_shape(&mut self, rgba: &[u8], width: u32, height: u32, hotspot_x: u32, hotspot_y: u32) {
        let (Ok(width), Ok(height)) = (u16::try_from(width), u16::try_from(height)) else {
            return;
        };
        let pointer = crate::pointer::Pointer {
            width,
            height,
            hotspot_x: u16::try_from(hotspot_x).unwrap_or(0),
            hotspot_y: u16::try_from(hotspot_y).unwrap_or(0),
            rgba,
        };
        if let Some(event) = crate::pointer::shape_event(&pointer) {
            self.inbox.push(Inbound::Pointer(event));
        }
    }

    fn pointer_hidden(&mut self) {
        self.inbox.push(Inbound::Pointer(RdpEvent::PointerHidden));
    }

    fn pointer_default(&mut self) {
        if let Some(event) = crate::pointer::default_arrow_event() {
            self.inbox.push(Inbound::Pointer(event));
        }
    }

    fn display_control_ready(&mut self) {
        self.inbox.push(Inbound::DisplayControlReady);
    }

    fn clip(&mut self, event: ClipEvent) {
        self.inbox.push(Inbound::Clip(event));
    }
}

/// A `FreeRDP` rectangle (exclusive edges) as an inclusive frame rectangle; `None` when empty.
fn frame_rect(r: rd_freerdp::Rect) -> Option<Rect> {
    if r.width <= 0 || r.height <= 0 {
        return None;
    }
    let edge = |v: i32| u16::try_from(v.max(0)).unwrap_or(u16::MAX);
    Some(Rect {
        left: edge(r.x),
        top: edge(r.y),
        right: edge(r.x + r.width - 1),
        bottom: edge(r.y + r.height - 1),
    })
}

/// The session's state across connections (besides what [`Events`] keeps).
struct Ctx {
    config: RdpConfig,
    handler: Arc<dyn RdpHandler>,
    shared: Arc<Shared>,
    /// The view size the desktop should follow.
    view: (u32, u32),
    /// The last text that came from the server (not offered back).
    remote_text: Option<String>,
    clipboard: SessionClipboard,
    /// Downloads the remote clipboard's files; `None` when file transfers are off.
    downloader: Option<Downloader>,
    /// The list being downloaded.
    download_files: Vec<RemoteFile>,
    /// When the last progress event went out (`None`: none yet for this download).
    progress_sent: Option<Instant>,
    /// File Contents Responses served off the session thread.
    served_tx: Sender<(u32, Option<Vec<u8>>)>,
    served_rx: Receiver<(u32, Option<Vec<u8>>)>,
}

impl Ctx {
    fn event(&self, event: RdpEvent) {
        self.handler.event(event);
    }

    fn disconnected(&self, clean: bool, code: u32, message: impl Into<String>) {
        self.event(RdpEvent::Disconnected {
            clean,
            code,
            message: message.into(),
        });
    }

    fn typing_interrupted(&self) {
        if self.shared.typing.swap(false, Ordering::SeqCst) {
            self.event(RdpEvent::Typed { finished: false });
        }
    }

    /// Commands that arrive while not connected. Returns `false` on close.
    fn idle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Close => return false,
            Command::Resize(w, h) => self.view = (w, h),
            Command::Clipboard(text) => {
                if self.remote_text.as_ref() != Some(&text) {
                    self.clipboard.local_text = Some(text);
                    self.clipboard.local_files = None;
                }
            }
            Command::ClipboardFiles(files) => {
                self.clipboard.local_files = files;
                self.clipboard.local_text = None;
            }
            Command::Type(_) => self.typing_interrupted(),
            Command::Input(_) | Command::CancelFileTransfer => {}
        }
        true
    }

    fn downloading(&self) -> bool {
        self.downloader.as_ref().is_some_and(Downloader::is_running)
    }

    /// A progress event, at most every [`PROGRESS_INTERVAL`] unless `force`.
    fn report_progress(&mut self, force: bool) {
        let Some((done, total)) = self
            .downloader
            .as_ref()
            .filter(|d| d.is_running())
            .map(Downloader::progress)
        else {
            return;
        };
        let now = Instant::now();
        if force
            || self
                .progress_sent
                .is_none_or(|sent| now - sent >= PROGRESS_INTERVAL)
        {
            self.progress_sent = Some(now);
            self.event(RdpEvent::FileProgress { done, total });
        }
    }

    /// Reports how a download ended.
    fn download_finished(&mut self, result: Result<PathBuf, String>) {
        let files = std::mem::take(&mut self.download_files);
        self.progress_sent = None;
        self.event(match result {
            Ok(folder) => {
                let paths: Vec<_> = file_list::top_level_indices(&files)
                    .into_iter()
                    .map(|i| folder.join(&files[i].path))
                    .collect();
                RdpEvent::FilesReady {
                    count: paths.len(),
                    paths,
                }
            }
            Err(message) if message == CANCELLED => RdpEvent::FileTransferCancelled,
            Err(message) => RdpEvent::FileTransferFailed { message },
        });
    }

    /// Cancels the running download (reported); nothing when idle.
    fn cancel_download(&mut self) {
        if let Some(Step::Finished(result)) = self.downloader.as_mut().and_then(Downloader::cancel) {
            self.download_finished(result);
        }
    }
}

/// Runs a session until it ends or is closed, then deletes its clipboard folder.
pub(crate) fn run(
    config: RdpConfig,
    handler: Arc<dyn RdpHandler>,
    commands: &Receiver<Command>,
    shared: &Arc<Shared>,
) {
    let folder = config.clipboard_dir.clone();
    run_session(config, handler, commands, shared);
    if let Some(folder) = folder {
        let _ = std::fs::remove_dir_all(folder);
    }
}

fn run_session(
    config: RdpConfig,
    handler: Arc<dyn RdpHandler>,
    commands: &Receiver<Command>,
    shared: &Arc<Shared>,
) {
    let events = Events {
        handler: Arc::clone(&handler),
        host: config.host.clone(),
        port: config.port,
        known_hosts: config.known_hosts.clone(),
        accepted: None,
        username: config.username.clone(),
        domain: config.domain.clone(),
        password: config.password.clone(),
        rejected: false,
        cancelled: false,
        certificate_rejected: false,
        inbox: Vec::new(),
    };
    let Some(mut conn) = Connection::new(events) else {
        handler.event(RdpEvent::Disconnected {
            clean: false,
            code: 0,
            message: "FreeRDP couldn't be set up.".into(),
        });
        return;
    };
    *shared
        .remote
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(conn.remote());
    let downloader = config
        .clipboard_dir
        .clone()
        .map(|folder| Downloader::new(folder, clipfiles::CHUNK_SIZE, false, clipfiles::STALL_TIMEOUT));
    let (served_tx, served_rx) = std::sync::mpsc::channel();
    let mut ctx = Ctx {
        view: (config.width, config.height),
        clipboard: SessionClipboard::new(downloader.is_some()),
        config,
        handler,
        shared: Arc::clone(shared),
        remote_text: None,
        downloader,
        download_files: Vec::new(),
        progress_sent: None,
        served_tx,
        served_rx,
    };
    session_loop(&mut ctx, &mut conn, commands);
    *shared
        .remote
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

fn session_loop(ctx: &mut Ctx, conn: &mut Connection<Events>, commands: &Receiver<Command>) {
    // 0 while connecting the first time; the reconnect attempt otherwise.
    let mut attempt = 0u32;
    loop {
        let connected = connect_once(ctx, conn);
        if ctx.shared.closing.load(Ordering::SeqCst) {
            conn.disconnect();
            return;
        }
        if connected {
            attempt = 0;
            match active(ctx, conn, commands) {
                End::Closed => return,
                End::Terminated { clean, code, message } => {
                    ctx.disconnected(clean, code, message);
                    return;
                }
                End::Dropped if !ctx.config.auto_reconnect => {
                    ctx.disconnected(false, 0, "");
                    return;
                }
                End::Dropped => {}
            }
        } else {
            let error = conn.last_error();
            let (rejected, user_cancelled) = conn.with_handler(|e| (e.certificate_rejected, e.cancelled));
            if rejected {
                conn.disconnect();
                ctx.disconnected(false, 0, "The certificate was rejected.");
                return;
            }
            if user_cancelled || error == CANCELLED_LAST_ERROR {
                conn.disconnect();
                return cancelled(ctx);
            }
            if AUTH_FAILURES.contains(&error) {
                conn.disconnect();
                let answered = conn.with_handler(|e| {
                    e.rejected = true;
                    e.ask_credentials()
                });
                if !answered {
                    return cancelled(ctx);
                }
                continue;
            }
            conn.disconnect();
            if attempt == 0 || attempt >= RECONNECT_ATTEMPTS {
                ctx.disconnected(false, 0, connect_message(error));
                return;
            }
        }
        // Reconnect after a pause, unless closed meanwhile.
        attempt += 1;
        ctx.event(RdpEvent::Reconnecting { attempt });
        let wake = Instant::now() + RECONNECT_DELAY;
        loop {
            let left = wake.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match commands.recv_timeout(left) {
                Ok(command) => {
                    if !ctx.idle_command(command) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// The message of a failed connect.
fn connect_message(error: u32) -> String {
    let text = rd_freerdp::last_error_string(error);
    if text.is_empty() {
        "Couldn't connect.".to_owned()
    } else {
        format!("Couldn't connect: {text}")
    }
}

/// Connects once (blocking; [`crate::RdpSession::close`] aborts it).
fn connect_once(ctx: &Ctx, conn: &mut Connection<Events>) -> bool {
    let (width, height) = rd_core::rdp::initial_size(ctx.view.0, ctx.view.1);
    let (username, domain, password) = conn.with_handler(|e| {
        e.cancelled = false;
        e.certificate_rejected = false;
        e.inbox.clear();
        (e.username.clone(), e.domain.clone(), e.password.clone())
    });
    let config_dir = freerdp_dir(&ctx.config);
    let config = rd_freerdp::Config {
        host: &ctx.config.host,
        port: ctx.config.port,
        username: &username,
        domain: &domain,
        password: &password,
        client_name: &ctx.config.client_name,
        config_dir: &config_dir,
        width,
        height,
        console_session: ctx.config.admin_session,
        clipboard: true,
    };
    conn.connect(&config)
}

/// `FreeRDP`'s folder: `freerdp` next to the app's `rdp_known_hosts`, or a temporary one.
fn freerdp_dir(config: &RdpConfig) -> String {
    let dir = config
        .known_hosts
        .as_ref()
        .and_then(|store| store.parent())
        .map_or_else(
            || std::env::temp_dir().join("rd-rdp-freerdp"),
            |settings| settings.join("freerdp"),
        );
    dir.to_string_lossy().into_owned()
}

/// The credential prompt was cancelled: a clean end, like `FreeRDP`'s "connect cancelled".
fn cancelled(ctx: &Ctx) {
    let clean = DisconnectReason::classify(CANCELLED_LAST_ERROR, 0).is_clean();
    ctx.disconnected(clean, 0, "");
}

/// How a session that stopped polling ended, from `FreeRDP`'s codes.
fn ended(last_error: u32, error_info: u32) -> End {
    if error_info != 0 {
        return End::Terminated {
            clean: DisconnectReason::classify(last_error, error_info).is_clean(),
            code: error_info,
            message: rd_freerdp::error_info_string(error_info),
        };
    }
    if DisconnectReason::classify(last_error, 0).is_clean() {
        return End::Terminated {
            clean: true,
            code: 0,
            message: String::new(),
        };
    }
    End::Dropped
}

/// One input event on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    Key { flags: u16, code: u8 },
    Unicode { flags: u16, code: u16 },
    Mouse { flags: u16, x: u16, y: u16 },
    MouseEx { flags: u16, x: u16, y: u16 },
}

/// The wire events for input.
fn wire_events(events: &[InputEvent]) -> Vec<Wire> {
    use rd_freerdp::{
        KBD_FLAGS_DOWN, KBD_FLAGS_EXTENDED, KBD_FLAGS_RELEASE, PTR_FLAGS_BUTTON1, PTR_FLAGS_BUTTON2,
        PTR_FLAGS_BUTTON3, PTR_FLAGS_DOWN, PTR_FLAGS_HWHEEL, PTR_FLAGS_MOVE, PTR_FLAGS_WHEEL,
        PTR_XFLAGS_BUTTON1, PTR_XFLAGS_BUTTON2, PTR_XFLAGS_DOWN,
    };
    let mut out = Vec::with_capacity(events.len() * 2);
    for event in events {
        match *event {
            InputEvent::Scancode { code, down } => {
                let mut flags = if down { KBD_FLAGS_DOWN } else { KBD_FLAGS_RELEASE };
                if code & 0x100 != 0 {
                    flags |= KBD_FLAGS_EXTENDED;
                }
                out.push(Wire::Key {
                    flags,
                    code: (code & 0xFF) as u8,
                });
            }
            InputEvent::Unicode(code) => {
                out.push(Wire::Unicode { flags: 0, code });
                out.push(Wire::Unicode {
                    flags: KBD_FLAGS_RELEASE,
                    code,
                });
            }
            InputEvent::MouseMove { x, y } => out.push(Wire::Mouse {
                flags: PTR_FLAGS_MOVE,
                x,
                y,
            }),
            InputEvent::MouseButton { button, down, x, y } => {
                let pressed = |flag: u16, down_flag: u16| if down { flag | down_flag } else { flag };
                out.push(match button {
                    MouseButton::Left => Wire::Mouse {
                        flags: pressed(PTR_FLAGS_BUTTON1, PTR_FLAGS_DOWN),
                        x,
                        y,
                    },
                    MouseButton::Right => Wire::Mouse {
                        flags: pressed(PTR_FLAGS_BUTTON2, PTR_FLAGS_DOWN),
                        x,
                        y,
                    },
                    MouseButton::Middle => Wire::Mouse {
                        flags: pressed(PTR_FLAGS_BUTTON3, PTR_FLAGS_DOWN),
                        x,
                        y,
                    },
                    MouseButton::X1 => Wire::MouseEx {
                        flags: pressed(PTR_XFLAGS_BUTTON1, PTR_XFLAGS_DOWN),
                        x,
                        y,
                    },
                    MouseButton::X2 => Wire::MouseEx {
                        flags: pressed(PTR_XFLAGS_BUTTON2, PTR_XFLAGS_DOWN),
                        x,
                        y,
                    },
                });
            }
            InputEvent::Wheel {
                delta,
                horizontal,
                x,
                y,
            } => {
                let flag = if horizontal {
                    PTR_FLAGS_HWHEEL
                } else {
                    PTR_FLAGS_WHEEL
                };
                // The wire carries 9-bit two's complement steps (the sign bit is PTR_FLAGS_WHEEL_NEGATIVE).
                let mut rest = i32::from(delta);
                while rest != 0 {
                    let step = rest.clamp(-255, 255);
                    rest -= step;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let rotation = (step as u16) & 0x1FF;
                    out.push(Wire::Mouse {
                        flags: flag | rotation,
                        x,
                        y,
                    });
                }
            }
        }
    }
    out
}

/// The press and release of one typed keystroke.
fn typed_wire(key: TypedKey) -> Vec<Wire> {
    let scancode = |code: u16| {
        vec![
            Wire::Key {
                flags: rd_freerdp::KBD_FLAGS_DOWN,
                code: (code & 0xFF) as u8,
            },
            Wire::Key {
                flags: rd_freerdp::KBD_FLAGS_RELEASE,
                code: (code & 0xFF) as u8,
            },
        ]
    };
    match key {
        TypedKey::Unit(unit) => wire_events(&[InputEvent::Unicode(unit)]),
        TypedKey::Enter => scancode(SC_RETURN),
        TypedKey::Tab => scancode(SC_TAB),
    }
}

fn send_wire(conn: &mut Connection<Events>, events: &[Wire]) {
    for event in events {
        match *event {
            Wire::Key { flags, code } => conn.send_key(flags, code),
            Wire::Unicode { flags, code } => conn.send_unicode(flags, code),
            Wire::Mouse { flags, x, y } => conn.send_mouse(flags, x, y),
            Wire::MouseEx { flags, x, y } => conn.send_mouse_ex(flags, x, y),
        };
    }
}

/// The running session's own state.
struct Active {
    dirty: Dirty,
    seq: u32,
    desktop: (u32, u32),
    resize: ResizeDebouncer,
    display_control: bool,
    typing: VecDeque<TypedKey>,
    next_key: Instant,
    next_frame: Instant,
    next_watchdog: Instant,
}

/// Runs a connected session until it ends.
fn active(ctx: &mut Ctx, conn: &mut Connection<Events>, commands: &Receiver<Command>) -> End {
    let now = Instant::now();
    let mut state = Active {
        dirty: Dirty::default(),
        seq: 0,
        desktop: (0, 0),
        resize: ResizeDebouncer::new(),
        display_control: false,
        typing: VecDeque::new(),
        next_key: now,
        next_frame: now,
        next_watchdog: now + WATCHDOG_INTERVAL,
    };
    state.resize.retry(ctx.view, now + rd_core::rdp::RESIZE_DEBOUNCE);
    ctx.shared.in_flight.store(0, Ordering::SeqCst);
    ctx.shared.connected.store(true, Ordering::SeqCst);
    let mut announced = false;

    let end = loop {
        // Everything FreeRDP reported (the first round holds what came while connecting).
        let inbox = conn.with_handler(|e| std::mem::take(&mut e.inbox));
        for inbound in inbox {
            handle_inbound(ctx, conn, &mut state, inbound, &mut announced);
        }
        while let Ok((stream_id, data)) = ctx.served_rx.try_recv() {
            conn.clip_contents_response(stream_id, data.as_deref());
        }
        // Commands.
        let mut closed = false;
        loop {
            match commands.try_recv() {
                Ok(Command::Close) | Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
                Ok(command) => handle_command(ctx, conn, &mut state, command),
                Err(TryRecvError::Empty) => break,
            }
        }
        if closed || ctx.shared.closing.load(Ordering::SeqCst) {
            conn.disconnect();
            break End::Closed;
        }
        let now = Instant::now();
        run_timers(ctx, conn, &mut state, now);

        if !conn.poll(next_wait(ctx, &state, Instant::now())) {
            if ctx.shared.closing.load(Ordering::SeqCst) {
                conn.disconnect();
                break End::Closed;
            }
            let end = ended(conn.last_error(), conn.error_info());
            conn.disconnect();
            break end;
        }
    };
    ctx.shared.connected.store(false, Ordering::SeqCst);
    ctx.typing_interrupted();
    // The file streams belong to this connection.
    ctx.cancel_download();
    end
}

fn handle_inbound(
    ctx: &mut Ctx,
    conn: &mut Connection<Events>,
    state: &mut Active,
    inbound: Inbound,
    announced: &mut bool,
) {
    match inbound {
        Inbound::Desktop(width, height) => {
            let (w, h) = (
                u16::try_from(width).unwrap_or(u16::MAX),
                u16::try_from(height).unwrap_or(u16::MAX),
            );
            let changed = state.desktop != (width, height);
            state.desktop = (width, height);
            state.dirty.clear();
            state.dirty.add(Rect::full(w, h));
            if !*announced {
                *announced = true;
                ctx.event(RdpEvent::Connected { width: w, height: h });
            } else if changed {
                ctx.event(RdpEvent::Resized { width: w, height: h });
            }
            // A resize sent before the graphics pipeline was up is dropped by Windows, and a reactivation
            // may come back at the old size: the desktop keeps following the view.
            if state.display_control && !state.resize.is_pending() {
                state.resize.retry(ctx.view, Instant::now() + RESIZE_RETRY);
            }
        }
        Inbound::Painted(rects) => {
            for rect in rects {
                state.dirty.add(rect);
            }
        }
        Inbound::Pointer(event) => ctx.event(event),
        Inbound::DisplayControlReady => {
            state.display_control = true;
            state.resize.retry(ctx.view, Instant::now());
        }
        Inbound::Clip(event) => {
            let actions = ctx.clipboard.handle(event);
            run_actions(ctx, conn, actions);
        }
    }
}

fn handle_command(ctx: &mut Ctx, conn: &mut Connection<Events>, state: &mut Active, command: Command) {
    match command {
        Command::Close => {}
        Command::Input(events) => send_wire(conn, &wire_events(&events)),
        Command::Resize(w, h) => {
            ctx.view = (w, h);
            state.resize.request(w, h, Instant::now());
        }
        Command::Clipboard(text) => {
            if ctx.remote_text.as_ref() != Some(&text) && ctx.clipboard.local_text.as_ref() != Some(&text) {
                ctx.clipboard.local_text = Some(text);
                ctx.clipboard.local_files = None;
                let actions = ctx.clipboard.offer();
                run_actions(ctx, conn, actions);
            }
        }
        Command::ClipboardFiles(files) => {
            let same = match (&ctx.clipboard.local_files, &files) {
                (Some(old), Some(new)) => old.sources() == new.sources(),
                _ => false,
            };
            if !same {
                ctx.clipboard.local_files = files;
                ctx.clipboard.local_text = None;
                let actions = ctx.clipboard.offer();
                run_actions(ctx, conn, actions);
            }
        }
        Command::CancelFileTransfer => ctx.cancel_download(),
        Command::Type(keys) => {
            state.typing = keys.into();
            state.next_key = Instant::now();
            if state.typing.is_empty() {
                ctx.shared.typing.store(false, Ordering::SeqCst);
                ctx.event(RdpEvent::Typed { finished: true });
            }
        }
    }
}

fn run_timers(ctx: &mut Ctx, conn: &mut Connection<Events>, state: &mut Active, now: Instant) {
    // Typing: one keystroke per interval.
    if !state.typing.is_empty() && now >= state.next_key {
        if let Some(key) = state.typing.pop_front() {
            send_wire(conn, &typed_wire(key));
        }
        state.next_key = now + rd_core::rdp::TYPE_INTERVAL;
        if state.typing.is_empty() {
            ctx.shared.typing.store(false, Ordering::SeqCst);
            ctx.event(RdpEvent::Typed { finished: true });
        }
    }
    // The desktop follows the view.
    if state.resize.is_pending()
        && let Some((w, h)) = state.resize.due(now, state.desktop)
        && !(state.display_control && conn.resize(w, h))
    {
        state.resize.retry((w, h), now + RESIZE_RETRY);
    }
    // Frames.
    if now >= state.next_frame
        && !state.dirty.is_empty()
        && ctx.shared.in_flight.load(Ordering::SeqCst) < MAX_IN_FLIGHT
    {
        // Packed under FreeRDP's update lock (the guard), sent after it is released.
        let seq = state.seq.wrapping_add(1);
        let message = conn.framebuffer().map(|fb| {
            let pixels = Pixels {
                data: fb.data,
                stride: fb.stride,
                width: u16::try_from(fb.width).unwrap_or(u16::MAX),
                height: u16::try_from(fb.height).unwrap_or(u16::MAX),
            };
            frame::pack(pixels, state.dirty.rects(), seq)
        });
        if let Some(message) = message {
            state.seq = seq;
            state.dirty.clear();
            ctx.shared.in_flight.fetch_add(1, Ordering::SeqCst);
            ctx.handler.frame(message);
        }
        state.next_frame = now + FRAME_INTERVAL;
    }
    // The download's watchdog.
    if now >= state.next_watchdog {
        state.next_watchdog = now + WATCHDOG_INTERVAL;
        if let Some(step) = ctx.downloader.as_mut().and_then(|d| d.check_stall(now)) {
            file_step(ctx, conn, step);
        }
    }
}

/// How long the next poll may wait.
fn next_wait(ctx: &Ctx, state: &Active, now: Instant) -> Duration {
    let mut until = now + IDLE_POLL;
    if !state.typing.is_empty() {
        until = until.min(state.next_key);
    }
    if !state.dirty.is_empty() && ctx.shared.in_flight.load(Ordering::SeqCst) < MAX_IN_FLIGHT {
        until = until.min(state.next_frame);
    } else if !state.dirty.is_empty() {
        // Waiting for the page's acknowledgement.
        until = until.min(now + FRAME_INTERVAL);
    }
    if let Some(deadline) = state.resize.deadline() {
        until = until.min(deadline);
    }
    if ctx.downloading() {
        until = until.min(state.next_watchdog);
    }
    until.saturating_duration_since(now)
}

/// Carries out the clipboard's actions.
fn run_actions(ctx: &mut Ctx, conn: &mut Connection<Events>, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Capabilities(flags) => {
                conn.clip_capabilities(flags);
            }
            Action::FormatList(formats) => {
                conn.clip_format_list(&formats);
            }
            Action::FormatListResponse(ok) => {
                conn.clip_format_list_response(ok);
            }
            Action::DataRequest(format) => {
                conn.clip_data_request(format);
            }
            Action::DataResponse(data) => {
                conn.clip_data_response(data.as_deref());
            }
            Action::Serve(request, files) => serve(ctx, request, files),
            Action::RemoteText(text) => {
                ctx.remote_text = Some(text.clone());
                let chars = text.chars().count();
                ctx.event(RdpEvent::RemoteClipboard { text, chars });
            }
            Action::RemoteChanged => ctx.cancel_download(),
            Action::RemoteFileList(data) => start_download(ctx, conn, &data),
            Action::FileContents { stream_id, ok, data } => {
                if let Some(step) = ctx
                    .downloader
                    .as_mut()
                    .and_then(|d| d.receive(stream_id, ok, &data))
                {
                    file_step(ctx, conn, step);
                }
            }
            Action::Negotiated(flags) => {
                if let Some(downloader) = ctx.downloader.as_mut() {
                    downloader.set_allow_huge_files(flags & CB_HUGE_FILE_SUPPORT_ENABLED != 0);
                }
            }
        }
    }
}

/// Serves a File Contents Request off the session thread (it may read 16 MiB); the response goes out
/// on the next loop round.
fn serve(ctx: &Ctx, request: ContentsRequest, files: Option<Arc<LocalFiles>>) {
    let served = ctx.served_tx.clone();
    let remote = ctx
        .shared
        .remote
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    std::thread::spawn(move || {
        let data = files.and_then(|files| files.serve(&request));
        let _ = served.send((request.stream_id, data));
        if let Some(remote) = remote {
            remote.wake();
        }
    });
}

/// The server's file list arrived: parse it strictly and download it (eagerly).
fn start_download(ctx: &mut Ctx, conn: &mut Connection<Events>, data: &[u8]) {
    if ctx.downloader.is_none() {
        return;
    }
    let files = match file_list::parse(data, clipfiles::MAX_REMOTE_FILES) {
        Ok(files) => files,
        Err(message) => {
            ctx.event(RdpEvent::FileTransferFailed { message });
            return;
        }
    };
    ctx.cancel_download();
    ctx.download_files.clone_from(&files);
    ctx.progress_sent = None;
    if let Some(step) = ctx.downloader.as_mut().map(|d| d.start(files)) {
        file_step(ctx, conn, step);
    }
}

/// Carries out the downloader's next step: sends its request (a request that can't be sent counts as
/// refused) or reports the end.
fn file_step(ctx: &mut Ctx, conn: &mut Connection<Events>, mut step: Step) {
    loop {
        match step {
            Step::Finished(result) => {
                ctx.download_finished(result);
                return;
            }
            Step::Request(request) => {
                if conn.clip_contents_request(&contents_request(request)) {
                    ctx.report_progress(false);
                    return;
                }
                match ctx
                    .downloader
                    .as_mut()
                    .and_then(|d| d.receive(request.stream_id, false, &[]))
                {
                    Some(next) => step = next,
                    None => return,
                }
            }
        }
    }
}

/// A downloader request as a File Contents Request.
fn contents_request(request: FileRequest) -> ContentsRequest {
    ContentsRequest {
        stream_id: request.stream_id,
        index: request.index,
        flags: match request.kind {
            ContentsKind::Size => rd_freerdp::FILECONTENTS_SIZE,
            ContentsKind::Range => rd_freerdp::FILECONTENTS_RANGE,
        },
        position: request.range.position(),
        requested: request.range.length,
        data_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rd_freerdp::{KBD_FLAGS_DOWN, KBD_FLAGS_EXTENDED, KBD_FLAGS_RELEASE};

    #[test]
    fn keys_and_unicode_become_wire_events() {
        let events = wire_events(&[
            InputEvent::Scancode {
                code: 0x14B,
                down: true,
            },
            InputEvent::Scancode {
                code: 0x1E,
                down: false,
            },
            InputEvent::Unicode(0x151),
        ]);
        assert_eq!(
            events,
            [
                Wire::Key {
                    flags: KBD_FLAGS_DOWN | KBD_FLAGS_EXTENDED,
                    code: 0x4B
                },
                Wire::Key {
                    flags: KBD_FLAGS_RELEASE,
                    code: 0x1E
                },
                Wire::Unicode {
                    flags: 0,
                    code: 0x151
                },
                Wire::Unicode {
                    flags: KBD_FLAGS_RELEASE,
                    code: 0x151
                },
            ]
        );
    }

    #[test]
    fn mouse_buttons_and_wheel() {
        let events = wire_events(&[
            InputEvent::MouseButton {
                button: MouseButton::Left,
                down: true,
                x: 1,
                y: 2,
            },
            InputEvent::MouseButton {
                button: MouseButton::X2,
                down: false,
                x: 1,
                y: 2,
            },
            InputEvent::Wheel {
                delta: -600,
                horizontal: false,
                x: 3,
                y: 4,
            },
            InputEvent::Wheel {
                delta: 120,
                horizontal: true,
                x: 3,
                y: 4,
            },
        ]);
        assert_eq!(
            events[0],
            Wire::Mouse {
                flags: 0x1000 | 0x8000,
                x: 1,
                y: 2
            }
        );
        assert_eq!(
            events[1],
            Wire::MouseEx {
                flags: 0x0002,
                x: 1,
                y: 2
            }
        );
        let flags: Vec<u16> = events[2..]
            .iter()
            .map(|e| match e {
                Wire::Mouse { flags, .. } => *flags,
                _ => 0,
            })
            .collect();
        // -255, -255, -90 as 9-bit two's complement, then +120 horizontally.
        assert_eq!(
            flags,
            [0x0200 | 0x101, 0x0200 | 0x101, 0x0200 | 0x1A6, 0x0400 | 0x78]
        );
    }

    #[test]
    fn typed_enter_and_tab_are_real_keys() {
        assert_eq!(
            typed_wire(TypedKey::Enter),
            [
                Wire::Key {
                    flags: KBD_FLAGS_DOWN,
                    code: 0x1C
                },
                Wire::Key {
                    flags: KBD_FLAGS_RELEASE,
                    code: 0x1C
                },
            ]
        );
        assert_eq!(typed_wire(TypedKey::Tab).len(), 2);
        assert_eq!(
            typed_wire(TypedKey::Unit(0x41))[0],
            Wire::Unicode { flags: 0, code: 0x41 }
        );
    }

    #[test]
    fn endings_are_classified() {
        assert!(matches!(
            ended(0, 0xC),
            End::Terminated {
                clean: true,
                code: 0xC,
                ..
            }
        ));
        assert!(matches!(
            ended(0, 0x3),
            End::Terminated {
                clean: false,
                code: 0x3,
                ..
            }
        ));
        assert!(matches!(ended(0x0002_000D, 0), End::Dropped));
        assert!(matches!(
            ended(CANCELLED_LAST_ERROR, 0),
            End::Terminated { clean: true, .. }
        ));
    }

    #[test]
    fn freerdp_rectangles_become_inclusive() {
        let rect = |x, y, width, height| rd_freerdp::Rect { x, y, width, height };
        assert_eq!(
            frame_rect(rect(10, 20, 30, 40)),
            Some(Rect {
                left: 10,
                top: 20,
                right: 39,
                bottom: 59
            })
        );
        assert_eq!(frame_rect(rect(0, 0, 0, 5)), None);
    }
}
