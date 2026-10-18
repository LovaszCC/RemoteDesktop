//! The app side of the askpass channel: one listener per process, one [`Ticket`] per session.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;

use interprocess::local_socket::Stream;
use interprocess::local_socket::prelude::*;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use super::{PromptKind, WireRequest, classify, new_token, read_request, write_response};

/// A prompt the vault can't answer, for the app to ask the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskpassRequest {
    /// The session label given to [`AskpassServer::register`].
    pub session: String,
    /// What is asked.
    pub kind: PromptKind,
    /// `ssh`'s prompt text.
    pub prompt: String,
}

/// Asks the user; `None` refuses (the helper exits 1). Called on a connection thread, so it may block
/// until the user answers.
pub type AskpassHandler = Arc<dyn Fn(AskpassRequest) -> Option<Zeroizing<String>> + Send + Sync>;

struct Session {
    label: String,
    password: Option<SecretString>,
    passphrase: Option<SecretString>,
}

struct Shared {
    sessions: Mutex<HashMap<String, Session>>,
    handler: AskpassHandler,
}

impl Shared {
    fn sessions(&self) -> MutexGuard<'_, HashMap<String, Session>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The vault answers the first password and the first passphrase prompt; the rest goes to the
    /// handler. Unknown tokens are refused.
    fn answer(&self, request: WireRequest) -> Option<Zeroizing<String>> {
        let kind = classify(&request.prompt, &request.hint);
        let label = {
            let mut sessions = self.sessions();
            let session = sessions.get_mut(&request.token)?;
            let stored = match kind {
                PromptKind::Password => session.password.take(),
                PromptKind::Passphrase => session.passphrase.take(),
                PromptKind::Confirm | PromptKind::Other => None,
            };
            if let Some(secret) = stored {
                return Some(Zeroizing::new(secret.expose_secret().to_owned()));
            }
            session.label.clone()
        };
        (self.handler)(AskpassRequest {
            session: label,
            kind,
            prompt: request.prompt,
        })
    }

    fn serve(&self, mut stream: Stream) -> io::Result<()> {
        let request = read_request(&mut stream)?;
        let answer = self.answer(request);
        write_response(&mut stream, answer.as_ref().map(|a| a.as_str()))
    }
}

/// A registered session. Dropping it unregisters the token, so a late helper call is refused.
pub struct Ticket {
    token: String,
    shared: Weak<Shared>,
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ticket(..)")
    }
}

impl Ticket {
    /// The token for [`TOKEN_ENV`](super::TOKEN_ENV).
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.sessions().remove(&self.token);
        }
    }
}

/// The listener. Dropping it stops accepting and removes the socket directory.
pub struct AskpassServer {
    shared: Arc<Shared>,
    endpoint: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    #[cfg(unix)]
    _dir: tempfile::TempDir,
}

impl std::fmt::Debug for AskpassServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskpassServer")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl AskpassServer {
    /// Binds a fresh private endpoint and starts accepting on a background thread.
    ///
    /// # Errors
    /// When the directory, socket or pipe can't be created.
    pub fn start(handler: AskpassHandler) -> io::Result<Self> {
        let endpoint = platform::bind()?;
        let shared = Arc::new(Shared {
            sessions: Mutex::default(),
            handler,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            let listener = endpoint.listener;
            std::thread::Builder::new()
                .name("askpass".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let Ok(stream) = stream else { continue };
                        let shared = Arc::clone(&shared);
                        // The handler may wait for the user, so each prompt gets its own thread.
                        let _ = std::thread::Builder::new()
                            .name("askpass-prompt".into())
                            .spawn(move || {
                                let _ = shared.serve(stream);
                            });
                    }
                })?
        };
        Ok(Self {
            shared,
            endpoint: endpoint.name,
            stop,
            thread: Some(thread),
            #[cfg(unix)]
            _dir: endpoint.dir,
        })
    }

    /// The value for [`SOCKET_ENV`](super::SOCKET_ENV).
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Registers a session under a fresh token. `label` comes back in every [`AskpassRequest`].
    #[must_use]
    pub fn register(
        &self,
        label: &str,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
    ) -> Ticket {
        let token = new_token();
        self.shared.sessions().insert(
            token.clone(),
            Session {
                label: label.to_owned(),
                password,
                passphrase,
            },
        );
        Ticket {
            token,
            shared: Arc::downgrade(&self.shared),
        }
    }

    /// How many sessions are registered.
    #[must_use]
    pub fn registered(&self) -> usize {
        self.shared.sessions().len()
    }
}

impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept.
        let _ = super::client::connect(&self.endpoint);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Bound {
    listener: LocalSocketListener,
    name: String,
    #[cfg(unix)]
    dir: tempfile::TempDir,
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::{GenericFilePath, ListenerOptions};

    use super::Bound;

    /// macOS allows 104 bytes in a socket path; stay clear of it.
    const MAX_SOCKET_PATH: usize = 100;

    fn private_dir(base: &Path) -> io::Result<tempfile::TempDir> {
        let dir = tempfile::Builder::new().prefix("rd-askpass-").tempdir_in(base)?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        Ok(dir)
    }

    /// A socket in a new `0700` directory under the temp dir (or `/tmp` when that path is too long).
    pub(super) fn bind() -> io::Result<Bound> {
        let mut dir = private_dir(&std::env::temp_dir())?;
        if dir.path().join("askpass.sock").as_os_str().len() > MAX_SOCKET_PATH {
            dir = private_dir(Path::new("/tmp"))?;
        }
        let path = dir.path().join("askpass.sock");
        let name = path.as_os_str().to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_sync()?;
        Ok(Bound {
            listener,
            name: path.to_string_lossy().into_owned(),
            dir,
        })
    }
}

#[cfg(windows)]
mod platform {
    use std::io;

    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions};
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    use super::{Bound, owner_only_sddl, parse_whoami_sid};

    /// The current user's SID, from `whoami` (no Win32 calls: the workspace denies `unsafe`).
    fn current_user_sid() -> Option<String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        parse_whoami_sid(&String::from_utf8_lossy(&output.stdout))
    }

    /// A named pipe with a random name that only the current user (and SYSTEM) may open.
    pub(super) fn bind() -> io::Result<Bound> {
        let pipe = format!("rd-askpass-{}", &crate::askpass::new_token()[..32]);
        let sddl = owner_only_sddl(current_user_sid().as_deref());
        let sddl = U16CString::from_str(&sddl).map_err(io::Error::other)?;
        let descriptor = SecurityDescriptor::deserialize(&sddl)?;
        let name = pipe.as_str().to_ns_name::<GenericNamespaced>()?;
        let listener = ListenerOptions::new()
            .name(name)
            .security_descriptor(descriptor)
            .create_sync()?;
        Ok(Bound { listener, name: pipe })
    }
}

/// The pipe's DACL: full access for `sid` (else the owner) and SYSTEM, nobody else.
#[cfg_attr(not(windows), allow(dead_code))]
fn owner_only_sddl(sid: Option<&str>) -> String {
    format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", sid.unwrap_or("OW"))
}

/// The SID column of `whoami /user /fo csv /nh` (`"host\user","S-1-5-21-…"`).
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_whoami_sid(output: &str) -> Option<String> {
    output
        .trim()
        .rsplit(',')
        .next()
        .map(|field| field.trim().trim_matches('"').to_owned())
        .filter(|sid| sid.starts_with("S-1-") && sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::super::client::ask;
    use super::*;

    fn request(token: &str, prompt: &str) -> WireRequest {
        WireRequest {
            token: token.into(),
            hint: String::new(),
            prompt: prompt.into(),
        }
    }

    fn server_with(answer: Option<&'static str>) -> (AskpassServer, mpsc::Receiver<AskpassRequest>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let handler: AskpassHandler = Arc::new(move |req| {
            tx.lock().unwrap().send(req).unwrap();
            answer.map(|a| Zeroizing::new(a.to_owned()))
        });
        (AskpassServer::start(handler).unwrap(), rx)
    }

    fn text(answer: Option<Zeroizing<String>>) -> Option<String> {
        answer.map(|a| a.as_str().to_owned())
    }

    #[test]
    fn vault_answers_first_password_and_passphrase_then_the_user() {
        let (server, rx) = server_with(Some("typed"));
        let ticket = server.register(
            "S1",
            Some(SecretString::from("pw")),
            Some(SecretString::from("pp")),
        );
        let ask = |prompt: &str| text(ask(server.endpoint(), &request(ticket.token(), prompt)).unwrap());

        assert_eq!(ask("u@h's password: ").as_deref(), Some("pw"));
        assert!(rx.try_recv().is_err(), "the vault answered without the user");
        assert_eq!(ask("Enter passphrase for key '/k': ").as_deref(), Some("pp"));
        assert_eq!(ask("u@h's password: ").as_deref(), Some("typed"));
        let forwarded = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            forwarded,
            AskpassRequest {
                session: "S1".into(),
                kind: PromptKind::Password,
                prompt: "u@h's password: ".into()
            }
        );
        assert_eq!(
            ask("Are you sure you want to continue connecting (yes/no)? ").as_deref(),
            Some("typed")
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().kind,
            PromptKind::Confirm
        );
    }

    #[test]
    fn sessions_without_secrets_go_to_the_user_and_refusals_pass_through() {
        let (server, rx) = server_with(None);
        let ticket = server.register("S2", None, None);
        let answer = ask(server.endpoint(), &request(ticket.token(), "password: ")).unwrap();
        assert!(answer.is_none());
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().session, "S2");
    }

    #[test]
    fn unknown_and_dropped_tokens_are_refused() {
        let (server, rx) = server_with(Some("typed"));
        assert!(
            ask(server.endpoint(), &request("nope", "password: "))
                .unwrap()
                .is_none()
        );
        let ticket = server.register("S3", Some(SecretString::from("pw")), None);
        let token = ticket.token().to_owned();
        assert_eq!(server.registered(), 1);
        drop(ticket);
        assert_eq!(server.registered(), 0);
        assert!(
            ask(server.endpoint(), &request(&token, "password: "))
                .unwrap()
                .is_none()
        );
        assert!(rx.try_recv().is_err(), "unknown tokens never reach the user");
    }

    #[cfg(unix)]
    #[test]
    fn socket_lives_in_a_private_directory_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let (server, _rx) = server_with(None);
        let socket = std::path::PathBuf::from(server.endpoint());
        let dir = socket.parent().unwrap().to_path_buf();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        drop(server);
        assert!(!dir.exists());
    }

    #[test]
    fn sddl_and_whoami_parsing() {
        assert_eq!(
            owner_only_sddl(Some("S-1-5-21-1-2-3-1001")),
            "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)(A;;GA;;;SY)"
        );
        assert_eq!(owner_only_sddl(None), "D:P(A;;GA;;;OW)(A;;GA;;;SY)");
        assert_eq!(
            parse_whoami_sid("\"desktop-x\\me\",\"S-1-5-21-111-222-333-1001\"\r\n").as_deref(),
            Some("S-1-5-21-111-222-333-1001")
        );
        assert_eq!(parse_whoami_sid(""), None);
        assert_eq!(parse_whoami_sid("\"x\",\"S-1-5;(A;;GA;;;WD)\""), None);
    }
}
