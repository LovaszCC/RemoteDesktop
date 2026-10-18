//! Embedded SSH, local terminal and external terminal commands.
//!
//! Output of a session goes over the `onData` channel as raw bytes (coalesced in Rust). Everything else is
//! an event:
//! - `ssh-event` ([`SshEventDto`]): `{sessionId, kind, …}` with `kind` one of `title {title}`,
//!   `altScreen {active}`, `bracketedPaste {enabled}`, `root {root}`, `recording {phase}`,
//!   `recorded {outcome, text}` (the text is already on the clipboard when `outcome` is `copied`) and
//!   `exit {code, authFailed, hostKeyChanged?}` (`hostKeyChanged {host, keyType, fingerprint, file,
//!   line}` when `ssh` refused a changed host key: offer [`ssh_replace_host_key`]);
//! - `askpass-prompt` ([`AskpassPromptDto`](crate::core::AskpassPromptDto)): answer with [`askpass_answer`];
//! - `askpass-closed` (`{requestId}`): the prompt's session ended, close its dialog.

use std::sync::Arc;

use rd_ssh::analyzer::ClearAction;
use rd_ssh::external::TerminalChoice;
use rd_ssh::recorder::Phase;
use rd_ssh::session::RecordOutcome;
use rd_ssh::{SessionEvent, Sink};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use super::{Core, blocking};
use crate::core::{AppCore, AskpassNotice, AskpassNotifier, CommandError, SessionsChanged};

/// Event: a session's state changed.
pub const SSH_EVENT: &str = "ssh-event";
/// Event: `ssh` asks something the vault can't answer.
pub const ASKPASS_PROMPT: &str = "askpass-prompt";
/// Event: a pending prompt is gone.
pub const ASKPASS_CLOSED: &str = "askpass-closed";

/// Payload of [`SSH_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshEventDto {
    /// The session.
    pub session_id: String,
    /// `kind` and its fields.
    #[serde(flatten)]
    pub event: SessionEvent,
}

/// Payload of [`ASKPASS_CLOSED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskpassClosedDto {
    /// The closed request.
    pub request_id: String,
}

/// Reply of [`open_ssh_session`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSessionDto {
    /// The new session.
    pub session_id: String,
}

/// Input as typed text (`onData`) or raw bytes (`onBinary`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum InputData {
    /// UTF-8 text.
    Text(String),
    /// Bytes.
    Bytes(Vec<u8>),
}

impl InputData {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text.as_bytes(),
            Self::Bytes(bytes) => bytes,
        }
    }
}

/// A session's output channel plus its events.
struct WebviewSink {
    app: AppHandle,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
}

impl Sink for WebviewSink {
    fn output(&self, bytes: Vec<u8>) {
        let _ = self.channel.send(InvokeResponseBody::Raw(bytes));
    }

    fn event(&self, event: SessionEvent) {
        if let SessionEvent::Recorded {
            outcome: RecordOutcome::Copied,
            text,
        } = &event
        {
            let _ = crate::clipboard::copy_text(text);
        }
        let _ = self.app.emit(
            SSH_EVENT,
            SshEventDto {
                session_id: self.session_id.clone(),
                event,
            },
        );
    }
}

/// Connects the core's askpass prompts and session changes to events and the menu. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: AskpassNotifier = Arc::new(move |notice| {
        let _ = match notice {
            AskpassNotice::Prompt(prompt) => handle.emit(ASKPASS_PROMPT, prompt),
            AskpassNotice::Closed(request_id) => handle.emit(ASKPASS_CLOSED, AskpassClosedDto { request_id }),
        };
    });
    let handle = app.clone();
    let changed: SessionsChanged = Arc::new(move || crate::menu::refresh(&handle));
    core.set_ssh_hooks(notifier, changed);
}

/// Opens an embedded session; output arrives on `on_data` as bytes. Errors: `locked`, `notFound`,
/// `notSsh`, `sshNotFound`, `emptyHost`, `invalidHost`, `keyFile`, `askpassFailed`, `spawnFailed`.
#[tauri::command]
pub async fn open_ssh_session(
    app: AppHandle,
    core: State<'_, Core>,
    connection_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<OpenSessionDto, CommandError> {
    blocking(&core, move |c| {
        let session_id = c.open_ssh_session(&connection_id, cols, rows, |session_id| {
            Arc::new(WebviewSink {
                app,
                session_id: session_id.to_owned(),
                channel: on_data,
            })
        })?;
        Ok(OpenSessionDto { session_id })
    })
    .await
}

/// Opens a local terminal (this machine's shell) as an embedded session; the other `ssh_*` commands
/// apply to it like to an SSH session, and `ssh_reconnect` starts a new shell. Errors: `spawnFailed`.
#[tauri::command]
pub async fn open_local_terminal(
    app: AppHandle,
    core: State<'_, Core>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<OpenSessionDto, CommandError> {
    blocking(&core, move |c| {
        let session_id = c.open_local_terminal(cols, rows, |session_id| {
            Arc::new(WebviewSink {
                app,
                session_id: session_id.to_owned(),
                channel: on_data,
            })
        })?;
        Ok(OpenSessionDto { session_id })
    })
    .await
}

/// Keyboard or paste input (`data`: a string, or an array of bytes). Errors: `sessionNotFound`,
/// `sessionEnded`.
#[tauri::command]
pub fn ssh_input(core: State<'_, Core>, session_id: String, data: InputData) -> Result<(), CommandError> {
    core.ssh_input(&session_id, data.as_bytes())
}

/// Input typed into a broadcasting split pane: written to `source_session_id`, then mirrored to each of
/// `target_session_ids` (bracketed-paste markers dropped for targets not in that mode; ended or unknown
/// targets skipped). Errors: those of [`ssh_input`] for the source.
#[tauri::command]
pub fn ssh_broadcast_input(
    core: State<'_, Core>,
    source_session_id: String,
    target_session_ids: Vec<String>,
    data: InputData,
) -> Result<(), CommandError> {
    core.ssh_broadcast_input(&source_session_id, &target_session_ids, data.as_bytes())
}

/// Resizes the PTY. Errors: `sessionNotFound`.
#[tauri::command]
pub fn ssh_resize(
    core: State<'_, Core>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    core.ssh_resize(&session_id, cols, rows)
}

/// Ends the process and forgets the session.
#[tauri::command]
pub fn ssh_close(core: State<'_, Core>, session_id: String) {
    core.ssh_close(&session_id);
}

/// Starts the session again in the same tab with re-resolved secrets. Errors: `sessionNotFound` and
/// those of [`open_ssh_session`].
#[tauri::command]
pub async fn ssh_reconnect(
    core: State<'_, Core>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    blocking(&core, move |c| c.ssh_reconnect(&session_id, cols, rows)).await
}

/// "Replace key" after an `exit` event with `hostKeyChanged`: replaces (or, when the server's key can't
/// be confirmed, removes) the stored key; the UI reconnects afterwards. Errors: `sessionNotFound`,
/// `noHostKeyChange`, `locked`, `notFound`, `emptyHost`, `hostKeyReplace`.
#[tauri::command]
pub async fn ssh_replace_host_key(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    blocking(&core, move |c| c.ssh_replace_host_key(&session_id)).await
}

/// The record button: arms, cancels or finishes; returns the new phase. Errors: `sessionNotFound`.
#[tauri::command]
pub fn ssh_record_next_output(core: State<'_, Core>, session_id: String) -> Result<Phase, CommandError> {
    core.ssh_record_next_output(&session_id)
}

/// Esc while recording: stops without a result. Errors: `sessionNotFound`.
#[tauri::command]
pub fn ssh_record_cancel(core: State<'_, Core>, session_id: String) -> Result<(), CommandError> {
    core.ssh_record_cancel(&session_id)
}

/// What "Clear terminal" should do: `none`, `localOnly` or `localAndRedraw` (then send Ctrl+L).
/// Errors: `sessionNotFound`.
#[tauri::command]
pub fn ssh_clear_decision(core: State<'_, Core>, session_id: String) -> Result<ClearAction, CommandError> {
    core.ssh_clear_decision(&session_id)
}

/// Answers an `askpass-prompt`; `null` refuses.
#[tauri::command]
pub fn askpass_answer(core: State<'_, Core>, request_id: String, answer: Option<String>) {
    core.askpass_answer(&request_id, answer);
}

/// Opens the connection in the external terminal; returns the terminal id used. Errors: those of
/// [`open_ssh_session`] and `terminalFailed`.
#[tauri::command]
pub async fn open_external_terminal(
    core: State<'_, Core>,
    connection_id: String,
) -> Result<String, CommandError> {
    blocking(&core, move |c| c.open_external_terminal(&connection_id)).await
}

/// The terminals installed here, for Settings (`[{id, name}]`).
#[tauri::command]
pub async fn installed_terminals() -> Vec<TerminalChoice> {
    tauri::async_runtime::spawn_blocking(AppCore::installed_terminals)
        .await
        .unwrap_or_default()
}

/// The clipboard's text for a terminal paste (Ctrl+Shift+V); `null` when it holds no text. Errors:
/// `clipboard`.
#[tauri::command]
pub fn terminal_paste_text() -> Result<Option<String>, CommandError> {
    crate::clipboard::read_text()
}

/// Whether a link clicked in a terminal may be opened: `http` and `https` only.
fn is_web_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")) && !url.chars().any(char::is_control)
}

/// Opens a link clicked in a terminal in the default browser. Errors: `invalidUrl` for anything but
/// `http(s)`, `openFailed`.
#[tauri::command]
pub fn terminal_open_url(app: AppHandle, url: String) -> Result<(), CommandError> {
    if !is_web_url(&url) {
        return Err(CommandError::new("invalidUrl", "Only web links can be opened.").with_detail(url));
    }
    app.opener()
        .open_url(url.trim(), None::<&str>)
        .map_err(|e| CommandError::new("openFailed", "Couldn't open the link.").with_detail(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AskpassPromptDto;

    #[test]
    fn events_flatten_with_the_session_id() {
        let json = serde_json::to_value(SshEventDto {
            session_id: "S".into(),
            event: SessionEvent::Exit {
                code: Some(0),
                auth_failed: false,
                host_key_changed: None,
            },
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"sessionId": "S", "kind": "exit", "code": 0, "authFailed": false})
        );
        let json = serde_json::to_value(SshEventDto {
            session_id: "S".into(),
            event: SessionEvent::Title { title: "t".into() },
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"sessionId": "S", "kind": "title", "title": "t"})
        );
    }

    #[test]
    fn only_web_links_open() {
        assert!(is_web_url("https://example.com/a?b=c"));
        assert!(is_web_url("HTTP://example.com"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("ssh://host"));
        assert!(!is_web_url("https://exa\nmple.com"));
    }

    #[test]
    fn input_accepts_text_or_bytes() {
        let text: InputData = serde_json::from_value(serde_json::json!("ls\r")).unwrap();
        assert_eq!(text.as_bytes(), b"ls\r");
        let bytes: InputData = serde_json::from_value(serde_json::json!([27, 91, 77])).unwrap();
        assert_eq!(bytes.as_bytes(), [27, 91, 77]);
    }

    #[test]
    fn prompt_and_reply_shapes() {
        let json = serde_json::to_value(AskpassPromptDto {
            request_id: "R".into(),
            session_id: "S".into(),
            kind: rd_ssh::askpass::PromptKind::Confirm,
            prompt: "yes/no?".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"requestId": "R", "sessionId": "S", "kind": "confirm", "prompt": "yes/no?"})
        );
        let json = serde_json::to_value(OpenSessionDto {
            session_id: "S".into(),
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"sessionId": "S"}));
        assert_eq!(
            serde_json::to_value(ClearAction::LocalAndRedraw).unwrap(),
            serde_json::json!("localAndRedraw")
        );
        assert_eq!(
            serde_json::to_value(Phase::Armed).unwrap(),
            serde_json::json!("armed")
        );
    }
}
