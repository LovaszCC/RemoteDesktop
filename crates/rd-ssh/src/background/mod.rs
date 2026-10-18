//! Background SSH connections with russh: no terminal and no system `ssh`, so the
//! config ([`crate::config`]) and the host key ([`crate::known_hosts`]) are handled here, and an unknown
//! or changed host key is asked about through a callback the app turns into a UI prompt (a changed key
//! the user accepts replaces the stored one).
//!
//! Auth order: the vault password, the credential's unlocked key (in memory, never on disk), key files
//! (the connection's key, then the config's `IdentityFile`s, else the default `~/.ssh/id_*`) with the
//! stored passphrase, then the agent (`SSH_AUTH_SOCK` on Unix; on Windows `SSH_AUTH_SOCK` or the
//! OpenSSH agent pipe `\\.\pipe\openssh-ssh-agent`). Keepalive every 30 s, 3 misses.
//!
//! Gaps: `ProxyJump`/`ProxyCommand` are refused, host certificates are not verified (refused), and
//! keyboard-interactive auth is not tried.

pub mod tunnel;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyBase64};
use russh::{Disconnect, MethodKind};
use secrecy::ExposeSecret;
use serde::Serialize;
use tokio::sync::oneshot;

use crate::SshSecrets;
use crate::config::{SshConfig, Target};
use crate::known_hosts::{self, HostKey, KnownHosts, Verdict};

/// How often a keepalive is sent; the connection drops after [`KEEPALIVE_MAX`] unanswered ones.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
/// Unanswered keepalives before the connection is closed.
pub const KEEPALIVE_MAX: usize = 3;

/// The runtime background connections run on (two worker threads, started on first use).
///
/// # Panics
/// If the OS refuses to start the runtime's threads.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rd-ssh-bg")
            .enable_all()
            .build()
            .expect("the background SSH runtime should start")
    })
}

/// Where the old key of a changed host key is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredKey {
    /// The `known_hosts` file.
    pub file: String,
    /// Its 1-based line.
    pub line: usize,
}

/// An unknown (or changed) host key the user has to accept or reject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyQuestion {
    /// The host the key is stored under (`HostKeyAlias` or the host name).
    pub host: String,
    /// The port.
    pub port: u16,
    /// `ssh-ed25519`, `ssh-rsa`, …
    pub key_type: String,
    /// `SHA256:…`.
    pub fingerprint: String,
    /// Set when the host has a different key of this type: accepting replaces that entry.
    pub changed: Option<StoredKey>,
}

/// Asks the user about an unknown or changed host key; the receiver yields `true` to accept. A dropped
/// sender rejects.
pub type HostKeyPrompt = Arc<dyn Fn(HostKeyQuestion) -> oneshot::Receiver<bool> + Send + Sync>;

/// Why a background connection failed. [`code`](Self::code) is a stable id the UI translates.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackgroundError {
    /// `ProxyJump` or `ProxyCommand` applies to the host.
    #[error("{0} is not supported for background connections.")]
    ProxyUnsupported(String),
    /// TCP connect or the SSH handshake failed.
    #[error("Can't connect to {host}:{port}: {reason}")]
    Connect {
        /// Host name.
        host: String,
        /// Port.
        port: u16,
        /// The OS or protocol error.
        reason: String,
    },
    /// The host key differs from the stored one.
    #[error(
        "The host key of {host} has changed ({fingerprint}). Remove the old key from {file}:{line} if the change is expected."
    )]
    HostKeyChanged {
        /// The host the key is stored under.
        host: String,
        /// The new key's fingerprint.
        fingerprint: String,
        /// The `known_hosts` file.
        file: String,
        /// Its line.
        line: usize,
    },
    /// The host key is `@revoked`.
    #[error("The host key of {host} is revoked ({file}:{line}).")]
    HostKeyRevoked {
        /// The host.
        host: String,
        /// The file with the marker.
        file: String,
        /// Its line.
        line: usize,
    },
    /// The user rejected the unknown host key.
    #[error("The host key of {0} was not accepted.")]
    HostKeyRejected(String),
    /// The server presented a certificate.
    #[error("Host certificates are not supported for background connections.")]
    HostCertificate,
    /// The accepted key couldn't be written.
    #[error("Can't save the host key: {0}")]
    KnownHostsWrite(String),
    /// Every auth method failed.
    #[error("Permission denied ({0}).")]
    AuthFailed(String),
    /// The connection closed.
    #[error("The SSH connection was closed.")]
    Disconnected,
}

impl BackgroundError {
    /// `proxyUnsupported`, `connectFailed`, `hostKeyChanged`, `hostKeyRevoked`, `hostKeyRejected`,
    /// `hostCertificate`, `knownHostsWrite`, `authFailed` or `disconnected`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProxyUnsupported(_) => "proxyUnsupported",
            Self::Connect { .. } => "connectFailed",
            Self::HostKeyChanged { .. } => "hostKeyChanged",
            Self::HostKeyRevoked { .. } => "hostKeyRevoked",
            Self::HostKeyRejected(_) => "hostKeyRejected",
            Self::HostCertificate => "hostCertificate",
            Self::KnownHostsWrite(_) => "knownHostsWrite",
            Self::AuthFailed(_) => "authFailed",
            Self::Disconnected => "disconnected",
        }
    }
}

/// What a background connection needs: the resolved connection's host, port and user, its key path and
/// secrets, and where to find the user's files.
pub struct ConnectSpec {
    /// The host as entered.
    pub host: String,
    /// The connection's port; `None` (or 22) lets the config decide.
    pub port: Option<u16>,
    /// The connection's user; empty lets the config decide.
    pub user: String,
    /// The connection's key file (`~` expanded), if any.
    pub key_path: Option<PathBuf>,
    /// The decrypted secrets.
    pub secrets: SshSecrets,
    /// The home folder (`~/.ssh/config`, `known_hosts`, default keys).
    pub home: Option<PathBuf>,
    /// Replaces the `known_hosts` files (tests).
    pub known_hosts: Option<Vec<PathBuf>>,
    /// Whether to try the agent.
    pub use_agent: bool,
}

impl std::fmt::Debug for ConnectSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectSpec")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl ConnectSpec {
    /// The target after `~/.ssh/config`.
    #[must_use]
    pub fn target(&self) -> Target {
        let config = SshConfig::load_user(self.home.as_deref());
        let port = self.port.filter(|p| *p != 22);
        let mut target = config.target(&self.host, port, &self.user, self.home.as_deref());
        if target.user.is_empty() {
            target.user = crate::config::local_user();
        }
        if let Some(files) = &self.known_hosts {
            target.known_hosts_files.clone_from(files);
        }
        target
    }
}

/// The russh handler: checks the host key and remembers why it refused.
struct Checker {
    host: String,
    port: u16,
    files: Vec<PathBuf>,
    prompt: Option<HostKeyPrompt>,
    refusal: Arc<Mutex<Option<BackgroundError>>>,
    /// Probe mode ([`fetch_host_key`]): the key is stored here and the connection refused.
    capture: Option<Arc<Mutex<Option<HostKey>>>>,
}

impl Checker {
    fn refuse(&self, error: BackgroundError) -> bool {
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
        false
    }

    /// Asks the prompt about `key`; no prompt, or a dropped answer, rejects.
    async fn ask(&self, key: &HostKey, changed: Option<StoredKey>) -> bool {
        match &self.prompt {
            Some(prompt) => prompt(HostKeyQuestion {
                host: self.host.clone(),
                port: self.port,
                key_type: key.algorithm.clone(),
                fingerprint: key.fingerprint(),
                changed,
            })
            .await
            .unwrap_or(false),
            None => false,
        }
    }

    /// A changed key: asks, and if accepted replaces the stored entries; the files must then know the key.
    async fn replace_changed(&self, key: &HostKey, file: PathBuf, line: usize) -> bool {
        let changed = BackgroundError::HostKeyChanged {
            host: self.host.clone(),
            fingerprint: key.fingerprint(),
            file: file.display().to_string(),
            line,
        };
        let stored = StoredKey {
            file: file.display().to_string(),
            line,
        };
        if !self.ask(key, Some(stored)).await {
            return self.refuse(changed);
        }
        let name = known_hosts::lookup_name(&self.host, self.port);
        if let Err(e) = known_hosts::replace(&self.files, &file, &name, key) {
            return self.refuse(BackgroundError::KnownHostsWrite(e.to_string()));
        }
        match KnownHosts::load(&self.files).check(&self.host, self.port, key) {
            Verdict::Known => true,
            _ => self.refuse(changed),
        }
    }

    async fn check(&self, key: &PublicKey) -> bool {
        let key = HostKey::new(key.algorithm().as_str(), key.public_key_bytes());
        if let Some(capture) = &self.capture {
            *capture.lock().unwrap_or_else(PoisonError::into_inner) = Some(key);
            return false;
        }
        let known = KnownHosts::load(&self.files);
        match known.check(&self.host, self.port, &key) {
            Verdict::Known => true,
            Verdict::Changed { file, line } => self.replace_changed(&key, file, line).await,
            Verdict::Revoked { file, line } => self.refuse(BackgroundError::HostKeyRevoked {
                host: self.host.clone(),
                file: file.display().to_string(),
                line,
            }),
            Verdict::Unknown => {
                if !self.ask(&key, None).await {
                    return self.refuse(BackgroundError::HostKeyRejected(self.host.clone()));
                }
                let Some(file) = self.files.first() else {
                    return self.refuse(BackgroundError::KnownHostsWrite("no known_hosts file".to_owned()));
                };
                match known_hosts::append(file, &self.host, self.port, &key) {
                    Ok(()) => true,
                    Err(e) => self.refuse(BackgroundError::KnownHostsWrite(e.to_string())),
                }
            }
        }
    }
}

impl client::Handler for Checker {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(match server_public_key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => self.check(key).await,
            russh::keys::PublicKeyOrCertificate::Certificate(_) => {
                self.refuse(BackgroundError::HostCertificate)
            }
        })
    }
}

/// What a remote command printed, and its exit status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOutput {
    /// The exit status, when the server reported one.
    pub status: Option<u32>,
    /// Standard output.
    pub stdout: Vec<u8>,
    /// Standard error.
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    /// Exit status 0.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }

    /// The last non-empty stderr line, e.g. `tar: x: Cannot open: Permission denied`.
    #[must_use]
    pub fn error_line(&self) -> String {
        last_line(&self.stderr)
    }
}

/// The last non-empty line of `bytes`, trimmed.
#[must_use]
pub fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// How long [`fetch_host_key`] waits for the server's key.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// A server's host key, read without checking it or logging in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedKey {
    /// The name the key is stored under (`host` or `[host]:port`, from `HostKeyAlias` or the host name).
    pub name: String,
    /// The key.
    pub key: HostKey,
    /// The user's `known_hosts` files for this host.
    pub files: Vec<PathBuf>,
}

/// Reads the host key `spec`'s server presents and disconnects: nothing is checked, written or sent
/// beyond the key exchange. The app compares it with the fingerprint the system `ssh` reported before it
/// replaces a changed key.
///
/// # Errors
/// [`BackgroundError::ProxyUnsupported`], [`BackgroundError::HostCertificate`], or
/// [`BackgroundError::Connect`] (also after [`PROBE_TIMEOUT`]).
pub async fn fetch_host_key(spec: &ConnectSpec) -> Result<ProbedKey, BackgroundError> {
    let target = spec.target();
    if let Some(proxy) = &target.unsupported_proxy {
        return Err(BackgroundError::ProxyUnsupported(proxy.clone()));
    }
    let refusal = Arc::new(Mutex::new(None));
    let capture = Arc::new(Mutex::new(None));
    let checker = Checker {
        host: target.host_key_name.clone(),
        port: target.port,
        files: target.known_hosts_files.clone(),
        prompt: None,
        refusal: Arc::clone(&refusal),
        capture: Some(Arc::clone(&capture)),
    };
    let address = (target.host_name.as_str(), target.port);
    let connect = client::connect(Arc::new(client::Config::default()), address, checker);
    let reason = match tokio::time::timeout(PROBE_TIMEOUT, connect).await {
        Ok(Ok(handle)) => {
            // Not reached: the checker refuses every key in probe mode.
            let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            "the server key was not refused".to_owned()
        }
        Ok(Err(e)) => e.to_string(),
        Err(_) => "timed out".to_owned(),
    };
    if let Some(key) = capture.lock().unwrap_or_else(PoisonError::into_inner).take() {
        return Ok(ProbedKey {
            name: known_hosts::lookup_name(&target.host_key_name, target.port),
            key,
            files: target.known_hosts_files,
        });
    }
    Err(refusal
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .unwrap_or(BackgroundError::Connect {
            host: target.host_name.clone(),
            port: target.port,
            reason,
        }))
}

/// An authenticated background connection.
pub struct Client {
    handle: Handle<Checker>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Client(..)")
    }
}

/// Keys to try from files: the connection's key, the config's, else the default ones.
fn key_files(spec: &ConnectSpec, target: &Target) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = spec.key_path.iter().cloned().collect();
    files.extend(target.identity_files.iter().cloned());
    if files.is_empty()
        && let Some(home) = &spec.home
    {
        let dir = home.join(".ssh");
        files.extend(
            ["id_ed25519", "id_ecdsa", "id_rsa"]
                .iter()
                .map(|name| dir.join(name)),
        );
    }
    files.dedup();
    files
}

fn load_key(path: &Path, passphrase: Option<&str>) -> Option<PrivateKey> {
    if !path.is_file() {
        return None;
    }
    russh::keys::load_secret_key(path, passphrase)
        .or_else(|_| russh::keys::load_secret_key(path, None))
        .ok()
}

fn method_names(methods: &russh::MethodSet) -> String {
    let names: Vec<&str> = methods
        .iter()
        .map(|m| match m {
            MethodKind::None => "none",
            MethodKind::Password => "password",
            MethodKind::PublicKey => "publickey",
            MethodKind::HostBased => "hostbased",
            MethodKind::KeyboardInteractive => "keyboard-interactive",
            MethodKind::GssapiWithMic => "gssapi-with-mic",
        })
        .collect();
    names.join(",")
}

impl Client {
    /// Connects, checks the host key (asking `prompt` about an unknown one) and authenticates.
    ///
    /// # Errors
    /// See [`BackgroundError`].
    pub async fn connect(spec: ConnectSpec, prompt: Option<HostKeyPrompt>) -> Result<Self, BackgroundError> {
        let target = spec.target();
        if let Some(proxy) = &target.unsupported_proxy {
            return Err(BackgroundError::ProxyUnsupported(proxy.clone()));
        }
        let refusal = Arc::new(Mutex::new(None));
        let checker = Checker {
            host: target.host_key_name.clone(),
            port: target.port,
            files: target.known_hosts_files.clone(),
            prompt,
            refusal: Arc::clone(&refusal),
            capture: None,
        };
        let config = Arc::new(client::Config {
            keepalive_interval: Some(KEEPALIVE_INTERVAL),
            keepalive_max: KEEPALIVE_MAX,
            nodelay: true,
            ..client::Config::default()
        });
        let address = (target.host_name.as_str(), target.port);
        let handle = client::connect(config, address, checker).await.map_err(|e| {
            refusal
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .unwrap_or_else(|| BackgroundError::Connect {
                    host: target.host_name.clone(),
                    port: target.port,
                    reason: e.to_string(),
                })
        })?;
        let mut client = Self { handle };
        client.authenticate(&spec, &target).await?;
        Ok(client)
    }

    async fn authenticate(&mut self, spec: &ConnectSpec, target: &Target) -> Result<(), BackgroundError> {
        let user = target.user.clone();
        let lost = |e: russh::Error| BackgroundError::Connect {
            host: target.host_name.clone(),
            port: target.port,
            reason: e.to_string(),
        };
        let mut remaining = String::from("none");
        if let Some(password) = &spec.secrets.password {
            let result = self
                .handle
                .authenticate_password(&user, password.expose_secret())
                .await
                .map_err(lost)?;
            match result {
                client::AuthResult::Success => return Ok(()),
                client::AuthResult::Failure {
                    remaining_methods, ..
                } => remaining = method_names(&remaining_methods),
            }
        }
        let hash = self
            .handle
            .best_supported_rsa_hash()
            .await
            .ok()
            .flatten()
            .flatten();
        let mut keys: Vec<PrivateKey> = Vec::new();
        if let Some(unlocked) = &spec.secrets.unlocked_key
            && let Ok(key) = russh::keys::decode_secret_key(unlocked.expose_secret(), None)
        {
            keys.push(key);
        }
        let passphrase = spec.secrets.passphrase.as_ref().map(ExposeSecret::expose_secret);
        keys.extend(
            key_files(spec, target)
                .iter()
                .filter_map(|path| load_key(path, passphrase)),
        );
        for key in keys {
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
            match self
                .handle
                .authenticate_publickey(&user, key)
                .await
                .map_err(lost)?
            {
                client::AuthResult::Success => return Ok(()),
                client::AuthResult::Failure {
                    remaining_methods, ..
                } => remaining = method_names(&remaining_methods),
            }
        }
        if spec.use_agent && !target.identities_only && self.agent_auth(&user, hash).await {
            return Ok(());
        }
        Err(BackgroundError::AuthFailed(remaining))
    }

    /// Tries every agent identity; `false` when there is no agent or none was accepted.
    async fn agent_auth(&mut self, user: &str, hash: Option<russh::keys::HashAlg>) -> bool {
        #[cfg(unix)]
        let agent = russh::keys::agent::client::AgentClient::connect_env().await;
        #[cfg(windows)]
        let agent = {
            let pipe =
                std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| r"\\.\pipe\openssh-ssh-agent".to_owned());
            russh::keys::agent::client::AgentClient::connect_named_pipe(pipe).await
        };
        let Ok(mut agent) = agent else {
            return false;
        };
        let Ok(identities) = agent.request_identities().await else {
            return false;
        };
        for identity in identities {
            let russh::keys::agent::AgentIdentity::PublicKey { key, .. } = identity else {
                continue;
            };
            let hash = if key.algorithm().is_rsa() { hash } else { None };
            if let Ok(client::AuthResult::Success) = self
                .handle
                .authenticate_publickey_with(user, key, hash, &mut agent)
                .await
            {
                return true;
            }
        }
        false
    }

    /// Opens a `direct-tcpip` channel to `host:port` as seen from the server.
    ///
    /// # Errors
    /// When the server refuses the channel or the connection is gone.
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        originator_port: u16,
    ) -> Result<russh::ChannelStream<client::Msg>, russh::Error> {
        let channel = self
            .handle
            .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", u32::from(originator_port))
            .await?;
        Ok(channel.into_stream())
    }

    /// Opens a session channel running `command` (in the user's shell on the server).
    ///
    /// # Errors
    /// When the server refuses the channel or the connection is gone.
    pub async fn exec_channel(&self, command: &str) -> Result<russh::Channel<client::Msg>, russh::Error> {
        let channel = self.handle.channel_open_session().await?;
        channel.exec(true, command).await?;
        Ok(channel)
    }

    /// Runs `command` to completion and collects its output.
    ///
    /// # Errors
    /// When the channel can't be opened.
    pub async fn exec(&self, command: &str) -> Result<ExecOutput, russh::Error> {
        let mut channel = self.exec_channel(command).await?;
        let mut output = ExecOutput::default();
        while let Some(message) = channel.wait().await {
            match message {
                russh::ChannelMsg::Data { data } => output.stdout.extend_from_slice(&data),
                russh::ChannelMsg::ExtendedData { data, ext: 1 } => output.stderr.extend_from_slice(&data),
                russh::ChannelMsg::ExitStatus { exit_status } => output.status = Some(exit_status),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok(output)
    }

    /// Opens the server's `sftp` subsystem.
    ///
    /// # Errors
    /// When the server has no SFTP subsystem (e.g. Dropbear without `sftp-server`) or the connection is
    /// gone.
    pub async fn sftp(&self) -> Result<russh_sftp::client::SftpSession, String> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| e.to_string())?;
        let session = tokio::time::timeout(
            Duration::from_secs(10),
            russh_sftp::client::SftpSession::new(channel.into_stream()),
        )
        .await
        .map_err(|_| "The SFTP subsystem didn't answer.".to_owned())?;
        session.map_err(|e| e.to_string())
    }

    /// Whether the connection is gone (server closed it, keepalive failed).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Says goodbye to the server.
    pub async fn disconnect(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_and_messages() {
        let errors = [
            BackgroundError::ProxyUnsupported("ProxyJump b".to_owned()),
            BackgroundError::Connect {
                host: "h".to_owned(),
                port: 22,
                reason: "refused".to_owned(),
            },
            BackgroundError::HostKeyChanged {
                host: "h".to_owned(),
                fingerprint: "SHA256:x".to_owned(),
                file: "/kh".to_owned(),
                line: 3,
            },
            BackgroundError::HostKeyRevoked {
                host: "h".to_owned(),
                file: "/kh".to_owned(),
                line: 1,
            },
            BackgroundError::HostKeyRejected("h".to_owned()),
            BackgroundError::HostCertificate,
            BackgroundError::KnownHostsWrite("denied".to_owned()),
            BackgroundError::AuthFailed("publickey".to_owned()),
            BackgroundError::Disconnected,
        ];
        let codes: Vec<&str> = errors.iter().map(BackgroundError::code).collect();
        assert_eq!(
            codes,
            [
                "proxyUnsupported",
                "connectFailed",
                "hostKeyChanged",
                "hostKeyRevoked",
                "hostKeyRejected",
                "hostCertificate",
                "knownHostsWrite",
                "authFailed",
                "disconnected"
            ]
        );
        assert!(errors[2].to_string().contains("/kh:3"));
        assert_eq!(errors[7].to_string(), "Permission denied (publickey).");
    }

    #[test]
    fn exec_output_lines() {
        let output = ExecOutput {
            status: Some(2),
            stdout: Vec::new(),
            stderr: b"warning\ntar: x: Cannot open: Permission denied  \n\n".to_vec(),
        };
        assert!(!output.succeeded());
        assert_eq!(output.error_line(), "tar: x: Cannot open: Permission denied");
        assert_eq!(last_line(b""), "");
        assert!(
            ExecOutput {
                status: Some(0),
                ..ExecOutput::default()
            }
            .succeeded()
        );
    }

    fn spec(home: &Path) -> ConnectSpec {
        ConnectSpec {
            host: "gw.example.com".to_owned(),
            port: Some(22),
            user: String::new(),
            key_path: None,
            secrets: SshSecrets::default(),
            home: Some(home.to_path_buf()),
            known_hosts: None,
            use_agent: false,
        }
    }

    #[test]
    fn target_and_key_files() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(
            ssh.join("config"),
            "Host gw.example.com\n HostName 192.0.2.30\n Port 2200\n User admin\n",
        )
        .unwrap();
        let spec = spec(home.path());
        let target = spec.target();
        assert_eq!(
            (target.host_name.as_str(), target.port, target.user.as_str()),
            ("192.0.2.30", 2200, "admin")
        );
        // No configured identity: the defaults.
        assert_eq!(
            key_files(&spec, &target),
            [ssh.join("id_ed25519"), ssh.join("id_ecdsa"), ssh.join("id_rsa")]
        );
        let mut with_key = spec;
        with_key.key_path = Some(PathBuf::from("/keys/gw"));
        with_key.port = Some(2222);
        with_key.known_hosts = Some(vec![PathBuf::from("/tmp/kh")]);
        let target = with_key.target();
        assert_eq!(target.port, 2222);
        assert_eq!(target.known_hosts_files, [PathBuf::from("/tmp/kh")]);
        assert_eq!(key_files(&with_key, &target), [PathBuf::from("/keys/gw")]);
        assert!(format!("{with_key:?}").contains("gw.example.com"));
    }

    #[test]
    fn missing_and_unreadable_keys_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_key(&dir.path().join("none"), None).is_none());
        std::fs::write(dir.path().join("bad"), "not a key").unwrap();
        assert!(load_key(&dir.path().join("bad"), Some("x")).is_none());
    }

    #[test]
    fn proxy_jump_is_refused_before_connecting() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("config"), "Host *\n ProxyJump bastion\n").unwrap();
        let result = runtime().block_on(Client::connect(spec(home.path()), None));
        assert_eq!(
            result.unwrap_err(),
            BackgroundError::ProxyUnsupported("ProxyJump bastion".to_owned())
        );
    }

    #[test]
    fn connection_refused() {
        let home = tempfile::tempdir().unwrap();
        // A port nobody listens on: bind, note it, close.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut spec = spec(home.path());
        spec.host = "127.0.0.1".to_owned();
        spec.port = Some(port);
        let error = runtime().block_on(Client::connect(spec, None)).unwrap_err();
        assert_eq!(error.code(), "connectFailed");
    }
}
