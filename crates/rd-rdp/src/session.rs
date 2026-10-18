//! One embedded RDP session: a handle to the engine running on its own thread.
//!
//! The engine runs `FreeRDP` on a dedicated thread; commands reach it over a channel and wake its poll.
//! Everything the session reports goes to an [`RdpHandler`]: frames (see [`crate::frame`]),
//! [`RdpEvent`]s, and the two questions it may ask while connecting (the certificate and NLA
//! credentials), which block the connection until answered.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use serde::Serialize;
use zeroize::Zeroizing;

use crate::engine;

/// What to connect to and how.
#[derive(Clone)]
pub struct RdpConfig {
    /// Host name or address.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// User name, already split from its domain (`rd_core::rdp::split_user`).
    pub username: String,
    /// Windows domain, may be empty.
    pub domain: String,
    /// Password, may be empty.
    pub password: Zeroizing<String>,
    /// Connect to the administrative (console) session.
    pub admin_session: bool,
    /// The view size; the initial desktop is at least 800×600 (`rd_core::rdp::initial_size`) and a
    /// Display Control resize follows when they differ.
    pub width: u32,
    /// See `width`.
    pub height: u32,
    /// The computer name the server sees (15 characters at most).
    pub client_name: String,
    /// The `rdp_known_hosts` store; `None` asks about every certificate.
    pub known_hosts: Option<PathBuf>,
    /// Reconnect by itself when the connection drops (not when the server ends the session).
    pub auto_reconnect: bool,
    /// The session's private folder for files copied from the remote clipboard (each download gets a
    /// fresh subfolder; the folder is deleted when the session ends). `None` turns file transfers off.
    pub clipboard_dir: Option<PathBuf>,
}

impl std::fmt::Debug for RdpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("domain", &self.domain)
            .field("admin_session", &self.admin_session)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// What a session reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RdpEvent {
    /// The desktop is up (also after a reconnect).
    Connected {
        /// Desktop width.
        width: u16,
        /// Desktop height.
        height: u16,
    },
    /// The server changed the desktop size (after a Display Control resize).
    Resized {
        /// Desktop width.
        width: u16,
        /// Desktop height.
        height: u16,
    },
    /// The connection dropped; trying again.
    Reconnecting {
        /// 1 for the first retry.
        attempt: u32,
    },
    /// The session ended.
    Disconnected {
        /// The user logged off or disconnected inside the session, or cancelled the credential prompt:
        /// the tab may close (`rd_core::rdp::DisconnectReason`).
        clean: bool,
        /// The server's error info code, 0 when it gave none.
        code: u32,
        /// The reason, empty when there is none ("the server gave no reason").
        message: String,
    },
    /// The remote clipboard has new text (the handler puts it on the local clipboard).
    RemoteClipboard {
        /// The text.
        #[serde(skip)]
        text: String,
        /// Its length in characters.
        chars: usize,
    },
    /// "Type clipboard" finished (`finished`) or was interrupted.
    Typed {
        /// All keystrokes went out.
        finished: bool,
    },
    /// Files from the remote clipboard are downloading (sent at the start and then a few times a
    /// second).
    FileProgress {
        /// Bytes written.
        done: u64,
        /// Bytes expected (grows while sizes are asked).
        total: u64,
    },
    /// The remote clipboard's files are downloaded (the handler puts them on the local clipboard).
    FilesReady {
        /// The top-level files and folders.
        #[serde(skip)]
        paths: Vec<PathBuf>,
        /// How many there are.
        count: usize,
    },
    /// A file transfer failed: a download, a refused server file list, or local files that can't be
    /// offered to the server.
    FileTransferFailed {
        /// Why (English, from the engine).
        message: String,
    },
    /// The download was cancelled (by the user or because the remote clipboard changed).
    FileTransferCancelled,
    /// The remote pointer's shape, for the view's cursor.
    #[serde(rename_all = "camelCase")]
    PointerShape {
        /// RGBA PNG, base64.
        png: String,
        /// The click point, inside the image.
        hotspot_x: u16,
        /// The click point, inside the image.
        hotspot_y: u16,
    },
    /// The remote pointer is hidden (e.g. while typing or in a full-screen video). The system default
    /// pointer comes as the Windows arrow's [`RdpEvent::PointerShape`].
    PointerHidden,
}

/// The certificate question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateRequest {
    /// The host connected to.
    pub host: String,
    /// The port.
    pub port: u16,
    /// SHA-256 of the certificate, `AB:CD:…`.
    pub fingerprint: String,
    /// A different certificate was accepted for this server before.
    pub changed: bool,
    /// The certificate is not for this host name.
    pub name_mismatch: bool,
    /// The certificate's common name.
    pub common_name: String,
    /// Its subject.
    pub subject: String,
    /// Its issuer.
    pub issuer: String,
}

/// The answer to a [`CertificateRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CertificateChoice {
    /// Connect and remember the certificate in `rdp_known_hosts`.
    AcceptPermanently,
    /// Connect; ask again next time (the session's reconnects don't ask).
    AcceptOnce,
    /// Don't connect.
    Reject,
}

/// The NLA credential question: the server requires NLA and the credentials are missing, or it
/// rejected them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialsRequest {
    /// The user name tried (pre-filled).
    pub username: String,
    /// The domain tried (pre-filled).
    pub domain: String,
    /// The server rejected these credentials (rather than them being missing).
    pub rejected: bool,
}

/// Credentials typed into the prompt.
#[derive(Clone)]
pub struct Credentials {
    /// User name, already split from its domain.
    pub username: String,
    /// Domain.
    pub domain: String,
    /// Password.
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// Receives what a session reports. The two questions block the connection (they are called on the
/// session thread) until answered.
pub trait RdpHandler: Send + Sync + 'static {
    /// A frame message (see [`crate::frame`]); acknowledge it with [`RdpSession::ack_frame`].
    fn frame(&self, frame: Vec<u8>);
    /// An event.
    fn event(&self, event: RdpEvent);
    /// Accept the certificate?
    fn certificate(&self, request: CertificateRequest) -> CertificateChoice;
    /// Credentials for NLA; `None` cancels (the session ends cleanly).
    fn credentials(&self, request: CredentialsRequest) -> Option<Credentials>;
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Middle (wheel click).
    Middle,
    /// Back.
    X1,
    /// Forward.
    X2,
}

/// Raw input for the session. Coordinates are desktop pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// A scancode press or release (`0x100` = extended).
    Scancode {
        /// The scancode.
        code: u16,
        /// Press or release.
        down: bool,
    },
    /// A UTF-16 unit typed as a Unicode key event (press and release).
    Unicode(u16),
    /// The pointer moved.
    MouseMove {
        /// X.
        x: u16,
        /// Y.
        y: u16,
    },
    /// A button went down or up.
    MouseButton {
        /// Which.
        button: MouseButton,
        /// Down or up.
        down: bool,
        /// X.
        x: u16,
        /// Y.
        y: u16,
    },
    /// The wheel turned: `delta` in wheel units (120 per notch), positive away from the user (up), or
    /// to the right when `horizontal`.
    Wheel {
        /// Rotation.
        delta: i16,
        /// Horizontal wheel.
        horizontal: bool,
        /// X.
        x: u16,
        /// Y.
        y: u16,
    },
}

/// What the handle sends to the engine.
#[derive(Debug)]
pub(crate) enum Command {
    Input(Vec<InputEvent>),
    Resize(u32, u32),
    Clipboard(String),
    ClipboardFiles(Option<Arc<crate::clipfiles::LocalFiles>>),
    Type(Vec<rd_core::rdp::TypedKey>),
    CancelFileTransfer,
    Close,
}

/// State shared between the handle and the engine.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub in_flight: AtomicU32,
    pub running: AtomicBool,
    pub connected: AtomicBool,
    pub typing: AtomicBool,
    /// [`RdpSession::close`] was called.
    pub closing: AtomicBool,
    /// Wakes (or aborts) the engine's `FreeRDP` connection.
    pub remote: Mutex<Option<Arc<rd_freerdp::Remote>>>,
}

impl Shared {
    fn remote(&self) -> Option<Arc<rd_freerdp::Remote>> {
        self.remote.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// A running session. Dropping it closes the session.
#[derive(Debug)]
pub struct RdpSession {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl RdpSession {
    /// Starts connecting on a new thread.
    ///
    /// # Errors
    /// When the thread can't be created.
    pub fn start(config: RdpConfig, handler: Arc<dyn RdpHandler>) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        shared.running.store(true, Ordering::SeqCst);
        let engine_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("rdp-session".into())
            .spawn(move || {
                engine::run(config, handler, &receiver, &engine_shared);
                engine_shared.running.store(false, Ordering::SeqCst);
                engine_shared.connected.store(false, Ordering::SeqCst);
            })?;
        Ok(Self {
            commands,
            shared,
            thread: Mutex::new(Some(thread)),
        })
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        if let Some(remote) = self.shared.remote() {
            remote.wake();
        }
    }

    /// Sends input events in order.
    pub fn input(&self, events: Vec<InputEvent>) {
        if !events.is_empty() {
            self.send(Command::Input(events));
        }
    }

    /// The view now measures `width`×`height`; the desktop follows 300 ms after the last change.
    pub fn resize(&self, width: u32, height: u32) {
        self.send(Command::Resize(width, height));
    }

    /// Offers local clipboard text to the remote side (cliprdr). The same text twice in a row, or the
    /// text that just came from the remote side, is not offered again.
    pub fn set_clipboard(&self, text: String) {
        self.send(Command::Clipboard(text));
    }

    /// Offers local clipboard files to the remote side (`None`: the clipboard holds files that can't be
    /// offered; the server is told the clipboard is empty). The same list twice in a row is not offered
    /// again.
    pub fn set_clipboard_files(&self, files: Option<Arc<crate::clipfiles::LocalFiles>>) {
        self.send(Command::ClipboardFiles(files));
    }

    /// Types `text` as keystrokes, 20 ms apart (`rd_core::rdp::typing_keystrokes`). Returns `false`
    /// (and does nothing) while not connected or while already typing; the end is reported as
    /// [`RdpEvent::Typed`].
    pub fn type_text(&self, text: &str) -> bool {
        if !self.is_connected() || self.shared.typing.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.send(Command::Type(rd_core::rdp::typing_keystrokes(text)));
        true
    }

    /// Cancels the running download of remote files (reported as [`RdpEvent::FileTransferCancelled`]).
    pub fn cancel_file_transfer(&self) {
        self.send(Command::CancelFileTransfer);
    }

    /// The page painted a frame.
    pub fn ack_frame(&self) {
        let _ = self
            .shared
            .in_flight
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    }

    /// Ends the session (gracefully when connected; a running connect is aborted). No `disconnected`
    /// event follows.
    pub fn close(&self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        self.send(Command::Close);
        // A connected session disconnects gracefully on its own; a blocking connect needs the abort.
        if !self.is_connected()
            && let Some(remote) = self.shared.remote()
        {
            remote.abort();
        }
    }

    /// Ends the session and waits for its thread.
    pub fn close_and_wait(&self) {
        self.close();
        let thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }

    /// Whether the engine still runs (connecting, connected or reconnecting).
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
    }

    /// Whether the desktop is up.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }

    /// Whether "type clipboard" is in progress.
    #[must_use]
    pub fn is_typing(&self) -> bool {
        self.shared.typing.load(Ordering::SeqCst)
    }
}

impl Drop for RdpSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_a_kind() {
        let json = |event: RdpEvent| serde_json::to_value(event).unwrap();
        assert_eq!(
            json(RdpEvent::Connected {
                width: 1280,
                height: 800
            }),
            serde_json::json!({"kind": "connected", "width": 1280, "height": 800})
        );
        assert_eq!(
            json(RdpEvent::Disconnected {
                clean: false,
                code: 3,
                message: "idle".into()
            }),
            serde_json::json!({"kind": "disconnected", "clean": false, "code": 3, "message": "idle"})
        );
        assert_eq!(
            json(RdpEvent::RemoteClipboard {
                text: "secret".into(),
                chars: 6
            }),
            serde_json::json!({"kind": "remoteClipboard", "chars": 6})
        );
        assert_eq!(
            json(RdpEvent::FileProgress { done: 1, total: 2 }),
            serde_json::json!({"kind": "fileProgress", "done": 1, "total": 2})
        );
        assert_eq!(
            json(RdpEvent::FilesReady {
                paths: vec![PathBuf::from("/private/secret")],
                count: 1
            }),
            serde_json::json!({"kind": "filesReady", "count": 1})
        );
        assert_eq!(
            json(RdpEvent::FileTransferFailed { message: "x".into() }),
            serde_json::json!({"kind": "fileTransferFailed", "message": "x"})
        );
        assert_eq!(
            json(RdpEvent::FileTransferCancelled),
            serde_json::json!({"kind": "fileTransferCancelled"})
        );
        assert_eq!(
            serde_json::from_value::<CertificateChoice>(serde_json::json!("acceptOnce")).unwrap(),
            CertificateChoice::AcceptOnce
        );
    }

    #[test]
    fn secrets_stay_out_of_debug_output() {
        let credentials = Credentials {
            username: "u".into(),
            domain: "d".into(),
            password: Zeroizing::new("hunter2".into()),
        };
        assert!(!format!("{credentials:?}").contains("hunter2"));
    }
}
