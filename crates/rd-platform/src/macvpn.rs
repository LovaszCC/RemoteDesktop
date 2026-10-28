// Copied from SepsiG/MacVPN `examples/macvpn-control-client/src/lib.rs` at commit
// bec152b0763a2fbac2fb53eea945dc6040493b40. Changes: the Windows named-pipe transport and `default_endpoint` are
// left out (WinVPN is not supported yet), so non-Unix builds use the "no transport" stub, and the doc example
// names `rd_platform::macvpn`. Keep the rest as upstream ships it (not rustfmt'ed), so updates stay a plain copy.
//
// MIT License
//
// Copyright (c) 2026 SepsiG
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
#![allow(
    clippy::pedantic,
    missing_docs,
    rustdoc::broken_intra_doc_links,
    reason = "copied verbatim from MacVPN's example client"
)]

//! A minimal client for MacVPN's local control API (see `docs/control-api.md`).
//!
//! This file is meant to be copied into your own app. It needs only `std` and `serde_json` (on Windows also
//! `macvpn-winsys`, for the named pipe: see the README), and it has no global state: everything lives in a
//! [`Client`], which is one socket connection to MacVPN.
//!
//! On Windows the endpoint is WinVPN's named pipe ([`default_endpoint`]), passed wherever a socket path goes.
//!
//! # The lifetime rule
//!
//! A VPN that you bring up with [`Client::up`] stays up **only while the socket of that `Client` is open**.
//! When the `Client` is dropped (or your process exits or crashes), MacVPN ends the lease, and it disconnects
//! the VPN if no one else needs it. So the pattern is: one `Client` per remote session, created when the
//! session starts and dropped when it ends. [`Lease`] wraps exactly that.
//!
//! ```no_run
//! use rd_platform::macvpn::{Lease, default_socket_path};
//!
//! # fn run_remote_session() {}
//! # fn main() -> Result<(), rd_platform::macvpn::Error> {
//! let token = "…from your Keychain item…";
//! let vpn = Lease::acquire(&default_socket_path()?, token, "6F1C…-UUID", |event| {
//!     println!("{}: {}", event.connection, event.state.as_str());
//! })?;
//! run_remote_session();
//! drop(vpn); // the socket closes, MacVPN brings the VPN down (unless someone else holds it)
//! # Ok(())
//! # }
//! ```
//!
//! # Forward compatibility
//!
//! Newer MacVPN versions may send states and error codes that this file does not know. The rule of the API is:
//! - an unknown **state** means "in progress" ([`State::Other`], [`State::is_in_progress`]);
//! - an unknown **error code** means `failed` ([`ErrorCode::from_wire`]; the original text stays in
//!   the `wire` field of [`Error::Api`] for your log);
//! - unknown **fields** and unknown **event** types are ignored.
//!
//! The `closing` event (`{"event":"closing","reason":"disabled"}`, or `"app_quitting"`) comes last before
//! MacVPN closes the socket on its side: [`Client::closing_reason`] then tells why the connection ended.

use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::time::Duration;
use transport::Stream;

/// The protocol version this client was written for (`hello` replies with `{"ok":true,"api":1}`).
pub const API_VERSION: u64 = 1;

/// How long [`Client::pair`] waits for the user to answer the pairing window (MacVPN itself gives up after
/// 2 minutes and answers `denied`).
pub const PAIRING_WAIT: Duration = Duration::from_secs(150);

/// `~/Library/Application Support/MacVPN/control/macvpn.sock`, where MacVPN listens while
/// "Allow other apps to control VPNs" is on. The file does not exist while the setting is off.
pub fn default_socket_path() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "HOME is not set"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/MacVPN/control/macvpn.sock"))
}

// MARK: - Values on the wire

/// A connection's state as MacVPN reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Disconnected,
    Connecting,
    /// MacVPN shows a one-time code or sign-in prompt in its own window. The client never sees the code.
    WaitingForMfa,
    /// MacVPN asks the user for a missing user name, password, shared secret or key password (from 1.1.0). The
    /// client never sees what is typed.
    WaitingForCredentials,
    Connected,
    Disconnecting,
    Failed,
    /// A state this client does not know yet (a newer MacVPN may add some).
    /// Treat it as "in progress".
    Other(String),
}

impl State {
    pub fn from_wire(text: &str) -> State {
        match text {
            "disconnected" => State::Disconnected,
            "connecting" => State::Connecting,
            "waiting_for_mfa" => State::WaitingForMfa,
            "waiting_for_credentials" => State::WaitingForCredentials,
            "connected" => State::Connected,
            "disconnecting" => State::Disconnecting,
            "failed" => State::Failed,
            other => State::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            State::Disconnected => "disconnected",
            State::Connecting => "connecting",
            State::WaitingForMfa => "waiting_for_mfa",
            State::WaitingForCredentials => "waiting_for_credentials",
            State::Connected => "connected",
            State::Disconnecting => "disconnecting",
            State::Failed => "failed",
            State::Other(text) => text,
        }
    }

    /// No tunnel, and nothing is on its way: `disconnected` or `failed`.
    pub fn is_down(&self) -> bool {
        matches!(self, State::Disconnected | State::Failed)
    }

    /// Neither up nor down: the connection is changing. Every unknown state counts as in progress.
    pub fn is_in_progress(&self) -> bool {
        !self.is_down() && *self != State::Connected
    }
}

/// The error codes of API version 1. Anything else is mapped to [`ErrorCode::Failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Wrong or revoked token, or the app runs from another path than the one it was paired from.
    Unauthorized,
    /// The connection is not in this app's allowlist (or the id is not exactly as `list` returned it).
    NotAllowed,
    /// The connection was deleted in MacVPN.
    UnknownConnection,
    /// The connection is changing state, or this socket already waits for an `up` of it, or another pairing
    /// window is open. Try again later.
    Busy,
    /// Homebrew, a VPN tool or MacVPN's root helper is missing: the user has to finish the System Status window.
    SetupRequired,
    MfaCancelled,
    MfaTimeout,
    /// The user cancelled MacVPN's credential prompt, or it expired (from 1.1.0).
    CredentialsCancelled,
    /// Anything else that went wrong. The reply's `message` (if any) is a short localized reason for the user.
    Failed,
    /// MacVPN is quitting. The socket closes next.
    AppQuitting,
    /// The user turned "Allow other apps to control VPNs" off. The socket closes next.
    Disabled,
    Unsupported,
    BadRequest,
    /// The user declined the pairing, the window timed out, or pairing is blocked after repeated rejections.
    Denied,
}

impl ErrorCode {
    /// Unknown codes (for example `locked` from a newer MacVPN) are `Failed`.
    pub fn from_wire(text: &str) -> ErrorCode {
        match text {
            "unauthorized" => ErrorCode::Unauthorized,
            "not_allowed" => ErrorCode::NotAllowed,
            "unknown_connection" => ErrorCode::UnknownConnection,
            "busy" => ErrorCode::Busy,
            "setup_required" => ErrorCode::SetupRequired,
            "mfa_cancelled" => ErrorCode::MfaCancelled,
            "mfa_timeout" => ErrorCode::MfaTimeout,
            "credentials_cancelled" => ErrorCode::CredentialsCancelled,
            "app_quitting" => ErrorCode::AppQuitting,
            "disabled" => ErrorCode::Disabled,
            "unsupported" => ErrorCode::Unsupported,
            "bad_request" => ErrorCode::BadRequest,
            "denied" => ErrorCode::Denied,
            _ => ErrorCode::Failed,
        }
    }
}

/// One entry of `list`: a connection this app may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The key to pass to `up` and `down`, **exactly** as received (keys are compared case-sensitively).
    pub id: String,
    pub name: String,
    /// `openvpn`, `wireguard`, `strongswan`, `sstp`, `l2tp`, `cisco` or `system` (more may come).
    pub kind: String,
    pub state: State,
}

/// The unsolicited `{"event":"state",…}` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateEvent {
    pub connection: String,
    pub state: State,
    /// With `failed`: a short localized reason, for showing to the user.
    pub reason: Option<String>,
}

/// The result of a successful pairing. `Debug` does not show the token.
#[derive(Clone, PartialEq, Eq)]
pub struct Pairing {
    /// The secret for `hello`. MacVPN sends it exactly once: store it in your Keychain item right away, never
    /// in a plain file or a log.
    pub token: String,
    /// The id MacVPN gave this pairing (shown nowhere secret; useful in your own log).
    pub client_id: String,
}

impl fmt::Debug for Pairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pairing").field("token", &"<redacted>").field("client_id", &self.client_id).finish()
    }
}

// MARK: - Errors

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// MacVPN answered `"ok":false`.
    Api {
        /// What to act on. An unknown code is already mapped to `Failed`.
        code: ErrorCode,
        /// The code as sent, for your log.
        wire: String,
        /// A short localized reason, when MacVPN gives one (for `failed`, `mfa_*`).
        message: Option<String>,
    },
    /// MacVPN closed the connection without a reply. It does that after the auth deadline (no `hello` within
    /// 5 s), on more than 20 requests per second, when the 8-client limit is reached, on a revoke, and when the
    /// setting is turned off or MacVPN quits (these two send a `closing` event first: see
    /// [`Client::closing_reason`]).
    Closed,
    /// A line that is not the JSON this client expects.
    Protocol(String),
}

impl Error {
    /// The API error code, if MacVPN sent one.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Error::Api { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Api { wire, message: Some(message), .. } => write!(f, "{wire}: {message}"),
            Error::Api { wire, message: None, .. } => f.write_str(wire),
            Error::Closed => f.write_str("MacVPN closed the connection"),
            Error::Protocol(what) => write!(f, "unexpected reply: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        // A peer that is gone shows up as one of these; it means the same as an end of file.
        match e.kind() {
            ErrorKind::ConnectionReset | ErrorKind::BrokenPipe | ErrorKind::NotConnected => Error::Closed,
            _ => Error::Io(e),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// MARK: - Client

/// One socket connection to MacVPN.
///
/// Requests are sent one at a time: each method writes one line and reads until the reply with its `id`.
/// State events that arrive meanwhile are kept for [`Client::next_event`] (or, during [`Client::up`], handed to
/// its callback). The type is not `Sync`; to end a session from another thread, use [`Client::closer`].
pub struct Client {
    stream: Stream,
    reader: BufReader<Stream>,
    next_id: u64,
    events: VecDeque<StateEvent>,
    /// The `reason` of the `closing` event, once MacVPN sent it.
    closing: Option<String>,
}

impl Client {
    /// Connects to the socket. Send `pair` or `hello` right away: MacVPN closes a connection that has not
    /// authenticated within 5 seconds. Prefer [`Client::open`], which does both.
    pub fn connect(path: &Path) -> Result<Client> {
        let stream = transport::connect(path)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Client { stream, reader, next_id: 1, events: VecDeque::new(), closing: None })
    }

    /// Connects and authenticates with the token from pairing.
    pub fn open(path: &Path, token: &str) -> Result<Client> {
        let mut client = Client::connect(path)?;
        client.hello(token)?;
        Ok(client)
    }

    /// Asks the user to pair this app: MacVPN shows a window with the given name and **this process's
    /// executable path**. Only that executable can use the token later, so call this from your real app, not
    /// from a helper tool. Blocks until the user answers (at most about 2 minutes). MacVPN closes the
    /// connection after the reply; use [`Client::open`] with the token afterwards.
    pub fn pair(path: &Path, name: &str) -> Result<Pairing> {
        let mut client = Client::connect(path)?;
        client.stream.set_read_timeout(Some(PAIRING_WAIT))?;
        let reply = client.call(json!({"op": "pair", "name": name}), None)?;
        let token = string_field(&reply, "token")?;
        let client_id = string_field(&reply, "client_id")?;
        Ok(Pairing { token, client_id })
    }

    /// Authenticates this connection. Returns the server's API version. A wrong or revoked token, or an app
    /// started from another path than the paired one, gets `unauthorized` after a 1 s delay, and the connection
    /// is closed.
    pub fn hello(&mut self, token: &str) -> Result<u64> {
        let reply = self.call(json!({"op": "hello", "token": token}), None)?;
        reply.get("api").and_then(Value::as_u64).ok_or_else(|| Error::Protocol("hello without api".into()))
    }

    /// The connections this app may use, in MacVPN's order.
    pub fn list(&mut self) -> Result<Vec<Connection>> {
        let reply = self.call(json!({"op": "list"}), None)?;
        let Some(Value::Array(items)) = reply.get("connections") else {
            return Err(Error::Protocol("list without connections".into()));
        };
        items
            .iter()
            .map(|item| {
                let Value::Object(item) = item else { return Err(Error::Protocol("a connection is no object".into())) };
                Ok(Connection {
                    id: string_field(item, "id")?,
                    name: string_field(item, "name")?,
                    kind: string_field(item, "kind")?,
                    state: State::from_wire(&string_field(item, "state")?),
                })
            })
            .collect()
    }

    /// Takes a lease on `connection` and waits until it is connected. `on_event` gets every state event that
    /// arrives meanwhile (`connecting`, `waiting_for_mfa`, …; events of other connections too, if you
    /// subscribed with [`Client::status`]).
    ///
    /// `Ok` means connected. The lease lasts until [`Client::down`] or until this socket closes. An already
    /// connected VPN answers at once; if it was up before any app asked for it, a lease end never takes it down.
    /// This may take minutes when the user has to answer an MFA prompt in MacVPN: do not set a short read
    /// timeout around it.
    pub fn up(&mut self, connection: &str, mut on_event: impl FnMut(&StateEvent)) -> Result<()> {
        let reply = self.call(json!({"op": "up", "connection": connection}), Some(&mut on_event))?;
        match reply.get("state").and_then(Value::as_str) {
            Some("connected") => Ok(()),
            other => Err(Error::Protocol(format!("up replied with state {other:?}"))),
        }
    }

    /// Ends this socket's lease on `connection`. MacVPN disconnects it if this was the last lease and the VPN
    /// was started for the leases; the reply comes after that disconnect. A pending `up` of the same connection
    /// is answered `failed` first.
    pub fn down(&mut self, connection: &str) -> Result<()> {
        self.call(json!({"op": "down", "connection": connection}), None).map(drop)
    }

    /// Subscribes to the state events of every allowed connection (without it, you only get the events of the
    /// connections you hold or wait for). Read them with [`Client::next_event`].
    pub fn status(&mut self) -> Result<()> {
        self.call(json!({"op": "status"}), None).map(drop)
    }

    /// The next state event; blocks until one arrives. `Ok(None)`: MacVPN closed the connection (the setting
    /// was turned off, MacVPN quit, the app was revoked: see [`Client::closing_reason`]) or [`Closer::close`]
    /// was called.
    pub fn next_event(&mut self) -> Result<Option<StateEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        loop {
            match self.read_message() {
                Ok(Some(Message::Event(event))) => return Ok(Some(event)),
                // A late reply to an earlier request (e.g. after a read timeout): nothing waits for it.
                Ok(Some(Message::Reply(_) | Message::Ignored)) => continue,
                Ok(None) | Err(Error::Closed) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    /// Why MacVPN is closing this connection, once it said so with the `closing` event (the last line before
    /// its close): `"disabled"` (the user turned "Allow other apps to control VPNs" off) or `"app_quitting"`.
    /// Newer MacVPN versions may send other reasons. `None` while the connection is open, and after a close
    /// without the event (a revoke, a limit, or a MacVPN version without it). Check it after
    /// [`Client::next_event`] returned `Ok(None)` or a request returned [`Error::Closed`].
    pub fn closing_reason(&self) -> Option<&str> {
        self.closing.as_deref()
    }

    /// A handle that closes this connection from another thread, for example when the remote session ends
    /// while this thread is blocked in [`Client::next_event`].
    pub fn closer(&self) -> io::Result<Closer> {
        Ok(Closer(self.stream.try_clone()?))
    }

    /// A timeout for every read (`None`: wait forever, the default). A read that times out returns
    /// `Error::Io` with `WouldBlock` / `TimedOut`; the connection stays usable.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    // MARK: internals

    /// Sends one request with a fresh `id` and reads until its reply. `Ok` is the reply object of an
    /// `"ok":true` reply.
    fn call(
        &mut self,
        mut request: Value,
        mut on_event: Option<&mut dyn FnMut(&StateEvent)>,
    ) -> Result<Map<String, Value>> {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = Value::from(id);
        let mut line = request.to_string();
        line.push('\n');
        self.stream.write_all(line.as_bytes())?;
        loop {
            match self.read_message()? {
                None => return Err(Error::Closed),
                Some(Message::Event(event)) => match on_event.as_mut() {
                    Some(callback) => callback(&event),
                    None => self.events.push_back(event),
                },
                Some(Message::Ignored) => {}
                // A reply without an `id` is one whose request MacVPN could not read (e.g. an over-long line): it
                // can only be ours, since requests go one at a time.
                Some(Message::Reply(reply)) => match reply.get("id").map(Value::as_u64) {
                    Some(Some(reply_id)) if reply_id != id => continue,
                    _ => return into_result(reply),
                },
            }
        }
    }

    /// One line from the socket; `None` at the end of the stream.
    fn read_message(&mut self) -> Result<Option<Message>> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let value: Value = serde_json::from_str(&line).map_err(|e| Error::Protocol(e.to_string()))?;
        let Value::Object(obj) = value else { return Err(Error::Protocol("a line is no JSON object".into())) };
        Ok(Some(match obj.get("event") {
            Some(Value::String(kind)) if kind == "state" => match parse_event(&obj) {
                Some(event) => Message::Event(event),
                None => Message::Ignored,
            },
            // MacVPN closes the connection next; keep the reason for `closing_reason`.
            Some(Value::String(kind)) if kind == "closing" => {
                self.closing = Some(obj.get("reason").and_then(Value::as_str).unwrap_or("").to_string());
                Message::Ignored
            }
            // An event type this client does not know: ignored.
            Some(_) => Message::Ignored,
            None => Message::Reply(obj),
        }))
    }
}

/// Closes a [`Client`]'s socket from another thread (see [`Client::closer`]).
pub struct Closer(Stream);

impl Closer {
    /// The `Client`'s pending and future reads end (`Ok(None)` / `Error::Closed`), and MacVPN ends the leases.
    pub fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

// MARK: - Transport

/// The socket under a [`Client`]: a Unix domain socket on macOS (and other Unix systems).
#[cfg(unix)]
mod transport {
    use std::io;
    use std::path::Path;

    pub(crate) type Stream = std::os::unix::net::UnixStream;

    pub(crate) fn connect(path: &Path) -> io::Result<Stream> {
        Stream::connect(path)
    }
}

/// Not Unix (Windows here: WinVPN's pipe is not used yet): no transport; [`Client::connect`] returns
/// `ErrorKind::Unsupported`.
#[cfg(not(unix))]
mod transport {
    use std::io::{self, ErrorKind, Read, Write};
    use std::net::Shutdown;
    use std::path::Path;
    use std::time::Duration;

    /// No value exists: [`connect`] always fails, so no `Client` is ever built on this platform.
    pub(crate) enum Stream {}

    pub(crate) fn connect(_path: &Path) -> io::Result<Stream> {
        Err(io::Error::new(ErrorKind::Unsupported, "no control API transport on this system"))
    }

    impl Stream {
        pub(crate) fn try_clone(&self) -> io::Result<Stream> {
            match *self {}
        }

        pub(crate) fn set_read_timeout(&self, _timeout: Option<Duration>) -> io::Result<()> {
            match *self {}
        }

        pub(crate) fn set_write_timeout(&self, _timeout: Option<Duration>) -> io::Result<()> {
            match *self {}
        }

        pub(crate) fn shutdown(&self, _how: Shutdown) -> io::Result<()> {
            match *self {}
        }
    }

    impl Read for Stream {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            match *self {}
        }
    }

    impl Write for Stream {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            match *self {}
        }

        fn flush(&mut self) -> io::Result<()> {
            match *self {}
        }
    }
}

enum Message {
    Reply(Map<String, Value>),
    Event(StateEvent),
    Ignored,
}

fn parse_event(obj: &Map<String, Value>) -> Option<StateEvent> {
    Some(StateEvent {
        connection: obj.get("connection")?.as_str()?.to_string(),
        state: State::from_wire(obj.get("state")?.as_str()?),
        reason: obj.get("reason").and_then(Value::as_str).map(str::to_string),
    })
}

fn into_result(reply: Map<String, Value>) -> Result<Map<String, Value>> {
    if reply.get("ok") == Some(&Value::Bool(true)) {
        return Ok(reply);
    }
    // A reply without a usable `error` is still a failure; it counts as `failed` like any unknown code.
    let wire = reply.get("error").and_then(Value::as_str).unwrap_or("failed").to_string();
    let message = reply.get("message").and_then(Value::as_str).map(str::to_string);
    Err(Error::Api { code: ErrorCode::from_wire(&wire), wire, message })
}

fn string_field(obj: &Map<String, Value>, key: &str) -> Result<String> {
    obj.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| Error::Protocol(format!("no {key}")))
}

// MARK: - Lease

/// One remote session's hold on one VPN: its own socket, kept open for as long as this value lives.
///
/// Create it when the session starts and drop it when the session ends. Dropping closes the socket, and MacVPN
/// ends the lease even if your process crashes. Use one `Lease` (one socket) per session: two sessions that
/// need the same VPN each hold their own lease, and the VPN stays up until both have ended.
pub struct Lease {
    client: Client,
    connection: String,
}

impl Lease {
    /// Connects, authenticates and brings `connection` up (see [`Client::up`]). On an error, the socket is
    /// closed again, so nothing is left held.
    pub fn acquire(path: &Path, token: &str, connection: &str, on_event: impl FnMut(&StateEvent)) -> Result<Lease> {
        let mut client = Client::open(path, token)?;
        client.up(connection, on_event)?;
        Ok(Lease { client, connection: connection.to_string() })
    }

    pub fn connection(&self) -> &str {
        &self.connection
    }

    /// Waits for the next change of the held connection. A `disconnected` or `failed` state means the lease has
    /// ended (the user disconnected in MacVPN, or the VPN dropped). MacVPN never reconnects a connection it started
    /// for leases (its "Reconnect Automatically" may bring back only a connection the user started, and even then
    /// the lease stays ended), so decide whether to call [`Client::up`] again. `Ok(None)`: the socket closed ([`Lease::closing_reason`] tells
    /// whether MacVPN said why).
    pub fn next_change(&mut self) -> Result<Option<StateEvent>> {
        while let Some(event) = self.client.next_event()? {
            if event.connection == self.connection {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    /// Why MacVPN closed the socket (see [`Client::closing_reason`]), after [`Lease::next_change`] returned
    /// `Ok(None)`.
    pub fn closing_reason(&self) -> Option<&str> {
        self.client.closing_reason()
    }

    /// Ends the session from another thread (see [`Client::closer`]).
    pub fn closer(&self) -> io::Result<Closer> {
        self.client.closer()
    }

    /// The underlying connection, for other requests on the same socket.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    /// Releases the lease explicitly and waits for MacVPN's reply (after the disconnect, if there is one).
    /// Dropping the `Lease` has the same effect without waiting.
    pub fn release(self) -> Result<()> {
        let Lease { mut client, connection } = self;
        client.down(&connection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credential_prompt_state_and_error_are_known() {
        let state = State::from_wire("waiting_for_credentials");
        assert_eq!(state, State::WaitingForCredentials);
        assert!(state.is_in_progress());
        assert_eq!(state.as_str(), "waiting_for_credentials");
        assert_eq!(ErrorCode::from_wire("credentials_cancelled"), ErrorCode::CredentialsCancelled);
        let reply = json!({"id": 3, "ok": false, "error": "credentials_cancelled"});
        let Value::Object(reply) = reply else { unreachable!() };
        let Err(Error::Api { code, wire, .. }) = into_result(reply) else { panic!() };
        assert_eq!((code, wire.as_str()), (ErrorCode::CredentialsCancelled, "credentials_cancelled"));
    }

    #[test]
    fn unknown_states_are_in_progress() {
        let state = State::from_wire("waiting_for_unlock");
        assert_eq!(state, State::Other("waiting_for_unlock".into()));
        assert!(state.is_in_progress());
        assert!(!state.is_down());
        assert_eq!(state.as_str(), "waiting_for_unlock");
        assert!(State::from_wire("disconnecting").is_in_progress());
        assert!(State::from_wire("failed").is_down());
        assert!(!State::from_wire("connected").is_in_progress());
    }

    #[test]
    fn unknown_error_codes_are_failed() {
        for code in ["locked", "vault_locked", ""] {
            assert_eq!(ErrorCode::from_wire(code), ErrorCode::Failed);
        }
        assert_eq!(ErrorCode::from_wire("busy"), ErrorCode::Busy);
        let reply = json!({"id": 3, "ok": false, "error": "locked"});
        let Value::Object(reply) = reply else { unreachable!() };
        let Err(Error::Api { code, wire, message }) = into_result(reply) else { panic!() };
        assert_eq!((code, wire.as_str(), message), (ErrorCode::Failed, "locked", None));
    }

    #[cfg(unix)]
    #[test]
    fn the_closing_event_is_kept_and_unknown_events_are_ignored() {
        use std::os::unix::net::UnixStream;
        let (ours, mut server) = UnixStream::pair().unwrap();
        let reader = BufReader::new(ours.try_clone().unwrap());
        let mut client = Client { stream: ours, reader, next_id: 1, events: VecDeque::new(), closing: None };
        server.write_all(b"{\"event\":\"news\",\"x\":1}\n{\"event\":\"closing\",\"reason\":\"disabled\"}\n").unwrap();
        drop(server);
        assert_eq!(client.closing_reason(), None);
        assert_eq!(client.next_event().unwrap(), None);
        assert_eq!(client.closing_reason(), Some("disabled"));
    }

    #[test]
    fn pairing_debug_hides_the_token() {
        let pairing = Pairing { token: "secret-token".into(), client_id: "id".into() };
        assert!(!format!("{pairing:?}").contains("secret-token"));
    }
}
