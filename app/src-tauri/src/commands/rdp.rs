//! Embedded RDP commands.
//!
//! Frames go over the `onFrame` channel as binary messages (layout in `rd_rdp::frame`: a 12-byte
//! header `seq u32, width u16, height u16, count u16, 0 u16`, then per rectangle `x, y, w, h` as u16
//! and its RGBA rows); acknowledge each painted frame with [`rdp_ack_frame`] (at most 2 are in flight).
//! Everything else is an event:
//! - `rdp-event` ([`RdpEventDto`]): `{sessionId, kind, …}` with `kind` one of `connected {width,
//!   height}`, `resized {width, height}`, `reconnecting {attempt}`, `disconnected {clean, code, message}`
//!   (`clean`: the tab may close; an empty `message` means "the server gave no reason"),
//!   `remoteClipboard {chars}` (the text is already on the local clipboard), `typed {finished}`, and the
//!   file clipboard's `fileProgress {done, total}` (bytes), `filesReady {count}` (the files are already
//!   on the local clipboard), `fileTransferFailed {message}` and `fileTransferCancelled`;
//! - `rdp-cert-prompt` ([`RdpCertPromptDto`](crate::core::RdpCertPromptDto)): answer with
//!   [`rdp_cert_answer`];
//! - `rdp-credentials-prompt` ([`RdpCredentialsPromptDto`](crate::core::RdpCredentialsPromptDto)):
//!   answer with [`rdp_credentials_answer`];
//! - `rdp-prompt-closed` (`{requestId}`): the prompt's session ended, close its dialog.
//!
//! While the canvas has focus, [`rdp_capture_keyboard`] takes the key combinations the OS would handle
//! (Windows hook, X11 grab) and sends them to that session.
//!
//! Local clipboard text or files go to the connected sessions while the main window has focus (checked
//! every second). Files on the remote clipboard are downloaded as soon as the server announces them
//! (into the session's private temp folder, see `rd_rdp::clipfiles`) and then put on the local
//! clipboard; [`rdp_cancel_file_transfer`] stops a download.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rd_platform::keyboard_capture::{CaptureMode, CapturedKey, KeySink, KeyboardCapture};
use rd_rdp::clipfiles::LocalFiles;
use rd_rdp::{CertificateChoice, InputEvent, RdpEvent};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{Core, blocking, emit_library};
use crate::core::{
    AppCore, CommandError, RdpCredentialsAnswer, RdpEngineInfoDto, RdpInput, RdpNotice, RdpNotifier, RdpSink,
    SessionsChanged, TypeClipboardDto,
};

/// Event: a session's state changed.
pub const RDP_EVENT: &str = "rdp-event";
/// Event: accept the server certificate?
pub const RDP_CERT_PROMPT: &str = "rdp-cert-prompt";
/// Event: NLA sign-in.
pub const RDP_CREDENTIALS_PROMPT: &str = "rdp-credentials-prompt";
/// Event: a pending prompt is gone.
pub const RDP_PROMPT_CLOSED: &str = "rdp-prompt-closed";

/// Payload of [`RDP_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpEventDto {
    /// The session.
    pub session_id: String,
    /// `kind` and its fields.
    #[serde(flatten)]
    pub event: RdpEvent,
}

/// Payload of [`RDP_PROMPT_CLOSED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpPromptClosedDto {
    /// The closed request.
    pub request_id: String,
}

/// Reply of [`open_rdp_session`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRdpSessionDto {
    /// The new session.
    pub session_id: String,
}

/// What the local clipboard held at the last exchange with the sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Exchanged {
    Text(String),
    Files(Vec<PathBuf>),
}

/// The clipboard last exchanged with the sessions, so it isn't sent back and forth.
static LAST_CLIPBOARD: Mutex<Option<Exchanged>> = Mutex::new(None);

fn last_clipboard() -> std::sync::MutexGuard<'static, Option<Exchanged>> {
    LAST_CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts downloaded files on the local clipboard; what the clipboard then reports is remembered so it
/// isn't offered back. A failure is reported as `fileTransferFailed`.
fn files_ready(paths: &[PathBuf], event: RdpEvent) -> RdpEvent {
    match crate::clipboard::copy_file_list(paths) {
        Ok(()) => {
            let listed = crate::clipboard::read_file_list().ok().flatten();
            *last_clipboard() = Some(Exchanged::Files(listed.unwrap_or_else(|| paths.to_vec())));
            event
        }
        Err(e) => RdpEvent::FileTransferFailed {
            message: e.detail.unwrap_or(e.message),
        },
    }
}

/// A session's frame channel plus its events.
struct WebviewSink {
    app: AppHandle,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
}

impl RdpSink for WebviewSink {
    fn frame(&self, frame: Vec<u8>) {
        let _ = self.channel.send(InvokeResponseBody::Raw(frame));
    }

    fn event(&self, event: RdpEvent) {
        if let RdpEvent::RemoteClipboard { text, .. } = &event {
            *last_clipboard() = Some(Exchanged::Text(text.clone()));
            let _ = crate::clipboard::copy_text(text);
        }
        let event = match event {
            RdpEvent::FilesReady { paths, count } => {
                let ready = RdpEvent::FilesReady {
                    paths: Vec::new(),
                    count,
                };
                files_ready(&paths, ready)
            }
            other => other,
        };
        let _ = self.app.emit(
            RDP_EVENT,
            RdpEventDto {
                session_id: self.session_id.clone(),
                event,
            },
        );
    }
}

/// Sends the local clipboard's files or text to the connected sessions when it changed since the last
/// exchange.
pub fn sync_clipboard(core: &AppCore) {
    if !core.rdp_any_connected() {
        return;
    }
    if let Ok(Some(paths)) = crate::clipboard::read_file_list() {
        let current = Some(Exchanged::Files(paths.clone()));
        let mut last = last_clipboard();
        if *last == current {
            return;
        }
        *last = current;
        drop(last);
        core.rdp_set_clipboard_files_all(&LocalFiles::scan(&paths).map(Arc::new));
        return;
    }
    let Ok(Some(text)) = crate::clipboard::read_text() else {
        return;
    };
    let current = Some(Exchanged::Text(text.clone()));
    let mut last = last_clipboard();
    if *last != current {
        core.rdp_set_clipboard_all(&text);
        *last = current;
    }
}

/// Connects the core's prompts and session changes to events and the menu, and starts the clipboard
/// watch. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &Core) {
    let handle = app.clone();
    let notifier: RdpNotifier = Arc::new(move |notice| {
        let _ = match notice {
            RdpNotice::Certificate(prompt) => handle.emit(RDP_CERT_PROMPT, prompt),
            RdpNotice::Credentials(prompt) => handle.emit(RDP_CREDENTIALS_PROMPT, prompt),
            RdpNotice::Closed(request_id) => {
                handle.emit(RDP_PROMPT_CLOSED, RdpPromptClosedDto { request_id })
            }
        };
    });
    let handle = app.clone();
    let changed: SessionsChanged = Arc::new(move || crate::menu::refresh(&handle));
    core.set_rdp_hooks(notifier, changed);

    // Download folders a crashed instance left behind.
    std::thread::spawn(rd_platform::clipboard::purge_stale_rdp_clipboard_folders);

    let weak = Arc::downgrade(core);
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(core) = weak.upgrade() else { break };
            let focused = handle
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if focused {
                sync_clipboard(&core);
            }
        }
    });
}

/// Opens an embedded RDP session for a view of `width`×`height` CSS pixels; frames arrive on
/// `on_frame`. `scale` (the device pixel ratio) is accepted for later `HiDPI` work; the desktop is sized in
/// CSS pixels at 100% scale, like the Swift app. Errors: `locked`, `notFound`, `wrongType`, `emptyHost`,
/// `rdpFailed`.
#[tauri::command]
pub async fn open_rdp_session(
    app: AppHandle,
    core: State<'_, Core>,
    connection_id: String,
    width: u32,
    height: u32,
    scale: Option<f64>,
    on_frame: Channel<InvokeResponseBody>,
) -> Result<OpenRdpSessionDto, CommandError> {
    let _ = scale;
    blocking(&core, move |c| {
        let session_id = c.open_rdp_session(&connection_id, width, height, |session_id| {
            Arc::new(WebviewSink {
                app,
                session_id: session_id.to_owned(),
                channel: on_frame,
            })
        })?;
        Ok(OpenRdpSessionDto { session_id })
    })
    .await
}

/// The page painted a frame. Errors: `sessionNotFound`.
#[tauri::command]
pub fn rdp_ack_frame(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    core.rdp_ack_frame(&session_id)
}

/// Canvas input, batched: `[{type: "key", code, key, down} | {type: "mouseMove", x, y} |
/// {type: "mouseButton", button, down, x, y} | {type: "wheel", deltaX, deltaY, deltaMode, x, y}]`, with
/// `x`/`y` in view pixels. Errors: `sessionNotFound`.
#[tauri::command]
pub fn rdp_input(
    core: State<'_, Core>,
    session_id: String,
    events: Vec<RdpInput>,
) -> Result<(), CommandError> {
    core.rdp_input(&session_id, events)
}

/// The canvas lost focus: release every held key. Errors: `sessionNotFound`.
#[tauri::command]
pub fn rdp_release_all_keys(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    core.rdp_release_all_keys(&session_id)
}

/// The view now measures `width`×`height`; the desktop follows 300 ms after the last change (even width,
/// 200…8192). Errors: `sessionNotFound`.
#[tauri::command]
pub fn rdp_resize(
    core: State<'_, Core>,
    session_id: String,
    width: u32,
    height: u32,
) -> Result<(), CommandError> {
    core.rdp_resize(&session_id, width, height)
}

/// Ends the session and forgets it.
#[tauri::command]
pub fn rdp_close(core: State<'_, Core>, session_id: String) {
    stop_capture(Some(&session_id));
    core.rdp_close(&session_id);
}

/// Connects again in the same tab with re-resolved credentials. Errors: `sessionNotFound` and those of
/// [`open_rdp_session`].
#[tauri::command]
pub async fn rdp_reconnect(
    core: State<'_, Core>,
    session_id: String,
    width: u32,
    height: u32,
) -> Result<(), CommandError> {
    blocking(&core, move |c| c.rdp_reconnect(&session_id, width, height)).await
}

/// Types the local clipboard's text into the session (also on the logon screen): `{status: "typing",
/// chars}`, `{status: "needsConfirmation", chars, seconds}` above 2000 characters unless `confirmed`,
/// `{status: "empty"}` or `{status: "busy"}`. Errors: `sessionNotFound`, `clipboard`.
#[tauri::command]
pub fn rdp_type_clipboard(
    core: State<'_, Core>,
    session_id: String,
    confirmed: bool,
) -> Result<TypeClipboardDto, CommandError> {
    let text = crate::clipboard::read_text()?;
    core.rdp_type_text(&session_id, text.as_deref(), confirmed)
}

/// Cancels the session's download of remote clipboard files (`fileTransferCancelled` follows).
/// Errors: `sessionNotFound`.
#[tauri::command]
pub fn rdp_cancel_file_transfer(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    core.rdp_cancel_file_transfer(&session_id)
}

/// Answers an `rdp-cert-prompt`: `acceptPermanently`, `acceptOnce` or `reject`.
#[tauri::command]
pub fn rdp_cert_answer(core: State<'_, Core>, request_id: String, choice: CertificateChoice) {
    core.rdp_cert_answer(&request_id, choice);
}

/// Answers an `rdp-credentials-prompt`: `{username, domain, password, save}`, or `null` to cancel (the
/// session ends cleanly). With `save` the connection is updated and `library-changed` follows. Errors
/// (saving only; the session still gets the answer): `readOnly`, `locked`, `notFound`, `database`.
#[tauri::command]
pub async fn rdp_credentials_answer(
    app: AppHandle,
    core: State<'_, Core>,
    request_id: String,
    answer: Option<RdpCredentialsAnswer>,
) -> Result<(), CommandError> {
    let saved = blocking(&core, move |c| c.rdp_credentials_answer(&request_id, answer)).await?;
    if saved {
        emit_library(&app, &core);
    }
    Ok(())
}

/// The running system keyboard capture and the session it feeds.
static CAPTURE: Mutex<Option<(String, KeyboardCapture)>> = Mutex::new(None);

fn stop_capture(session_id: Option<&str>) {
    let mut capture = CAPTURE.lock().unwrap_or_else(PoisonError::into_inner);
    if session_id.is_none_or(|id| capture.as_ref().is_some_and(|(owner, _)| owner == id)) {
        *capture = None;
    }
}

/// Reply of [`rdp_capture_keyboard`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDto {
    /// `hook` (Windows: only system combinations are taken, the rest reaches the canvas as usual),
    /// `grab` (X11: every key goes to the session directly; the canvas gets no key events) or `none`
    /// (macOS, Wayland: nothing is captured).
    pub mode: CaptureMode,
    /// The capture is running now.
    pub active: bool,
}

/// Starts (`active`, when the canvas gains focus) or stops (on blur) taking system key combinations
/// for the session: the Windows low-level hook, the X11 keyboard grab; nothing on macOS and Wayland.
/// Errors: `sessionNotFound`, `captureFailed`.
#[tauri::command]
pub fn rdp_capture_keyboard(
    core: State<'_, Core>,
    session_id: String,
    active: bool,
) -> Result<CaptureDto, CommandError> {
    let mode = rd_platform::keyboard_capture::support();
    if !active {
        stop_capture(Some(&session_id));
        return Ok(CaptureDto { mode, active: false });
    }
    // Validates the session.
    core.rdp_raw_input(&session_id, Vec::new())?;
    stop_capture(None);
    let weak = Arc::downgrade(&*core);
    let target = session_id.clone();
    let sink: KeySink = Arc::new(move |key: CapturedKey| {
        if let Some(core) = weak.upgrade() {
            let _ = core.rdp_raw_input(
                &target,
                vec![InputEvent::Scancode {
                    code: key.scancode,
                    down: key.down,
                }],
            );
        }
    });
    let capture = KeyboardCapture::start(sink).map_err(|e| {
        CommandError::new("captureFailed", "Couldn't capture the keyboard.").with_detail(e.to_string())
    })?;
    let running = capture.is_some();
    if let Some(capture) = capture {
        *CAPTURE.lock().unwrap_or_else(PoisonError::into_inner) = Some((session_id, capture));
    }
    Ok(CaptureDto {
        mode,
        active: running,
    })
}

/// The embedded RDP engine, for Settings: `{name: "FreeRDP", version}`.
#[tauri::command]
pub fn rdp_engine_info() -> RdpEngineInfoDto {
    AppCore::rdp_engine_info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_flatten_with_the_session_id() {
        let json = |event| {
            serde_json::to_value(RdpEventDto {
                session_id: "S".into(),
                event,
            })
            .unwrap()
        };
        assert_eq!(
            json(RdpEvent::Disconnected {
                clean: true,
                code: 12,
                message: "logged off".into()
            }),
            serde_json::json!({"sessionId": "S", "kind": "disconnected", "clean": true, "code": 12, "message": "logged off"})
        );
        assert_eq!(
            json(RdpEvent::RemoteClipboard {
                text: "secret".into(),
                chars: 6
            }),
            serde_json::json!({"sessionId": "S", "kind": "remoteClipboard", "chars": 6})
        );
        assert_eq!(
            json(RdpEvent::FilesReady {
                paths: vec![PathBuf::from("/tmp/x")],
                count: 1
            }),
            serde_json::json!({"sessionId": "S", "kind": "filesReady", "count": 1})
        );
        assert_eq!(
            serde_json::to_value(RdpPromptClosedDto {
                request_id: "R".into()
            })
            .unwrap(),
            serde_json::json!({"requestId": "R"})
        );
        assert_eq!(
            serde_json::to_value(OpenRdpSessionDto {
                session_id: "S".into()
            })
            .unwrap(),
            serde_json::json!({"sessionId": "S"})
        );
        assert_eq!(
            serde_json::to_value(rdp_engine_info()).unwrap(),
            serde_json::json!({"name": "FreeRDP", "version": rd_rdp::ENGINE_VERSION})
        );
    }
}
