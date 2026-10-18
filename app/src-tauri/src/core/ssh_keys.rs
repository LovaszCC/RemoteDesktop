//! "Set up SSH key login": generating ed25519 key pairs in `~/.ssh` and installing a public key in the
//! server's `~/.ssh/authorized_keys` over a background russh connection.
//!
//! - A connection that already has a key (its own or its credential's) gets that key installed; one
//!   without gets a new pair, `~/.ssh/rdnext_<name>_ed25519` (never overwriting: `_2`, `_3`, … follow),
//!   stored on whichever record the key path resolves from (the linked credential, else the connection).
//! - The public key comes from `<key>.pub`, else from the private key file (OpenSSH keeps the public half
//!   unencrypted), else from the credential's unlocked copy.
//! - The remote command is idempotent: it creates `~/.ssh` and `authorized_keys` with private modes and
//!   appends the key only when the exact line is missing.
//! - Afterwards a second connection with only the key (no password, no agent) checks that it works.
//!
//! The upload connection authenticates like the file browser (password, keys, agent) and asks about an
//! unknown (or changed) host key with the `host-key-prompt` event, answered through `host_key_answer`.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use rd_core::{Connection, ConnectionType, Credential, Library, new_id};
use rd_ssh::SshSecrets;
use rd_ssh::background::{BackgroundError, Client, HostKeyPrompt, HostKeyQuestion, runtime};
use secrecy::ExposeSecret;
use serde::Serialize;
use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, LineEnding, PrivateKey, PublicKey};
use tokio::sync::oneshot;

use super::forwards::HostKeyPromptDto;
use super::ssh_unlock::home_relative;
use super::{AppCore, CommandError};

/// The prefix of the `forwardId` a key setup's host key prompt carries (then the connection id).
pub const KEY_SETUP_PROMPT_PREFIX: &str = "ssh-key-";

/// What `ssh_key_setup` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySetupDto {
    /// The host the key was installed on (as entered on the connection).
    pub host: String,
    /// The key file, `~`-relative when it is in the home folder.
    pub key_path: String,
    /// A new key pair was generated (and saved on the connection or its credential).
    pub generated: bool,
    /// A login with only the key worked afterwards.
    pub verified: bool,
}

/// Receives the key setup's host key prompts (emits `host-key-prompt`).
pub type KeySetupNotifier = Arc<dyn Fn(HostKeyPromptDto) + Send + Sync>;

/// The pending host key prompts of running key setups.
pub(crate) struct KeySetupShared {
    prompts: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    notifier: Mutex<Option<KeySetupNotifier>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl KeySetupShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            prompts: Mutex::default(),
            notifier: Mutex::default(),
        })
    }

    fn ask_host_key(&self, connection_id: &str, question: HostKeyQuestion) -> oneshot::Receiver<bool> {
        let (answer, receiver) = oneshot::channel();
        let Some(notifier) = lock(&self.notifier).clone() else {
            return receiver;
        };
        let request_id = new_id();
        lock(&self.prompts).insert(request_id.clone(), answer);
        notifier(HostKeyPromptDto {
            request_id,
            forward_id: format!("{KEY_SETUP_PROMPT_PREFIX}{connection_id}"),
            host: question.host,
            port: question.port,
            key_type: question.key_type,
            fingerprint: question.fingerprint,
            changed: question.changed,
        });
        receiver
    }
}

/// Which record a connection's SSH key path comes from (see [`Connection::resolved`]): a linked credential
/// always supplies it, even when it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyOwner<'a> {
    /// The connection's own field.
    Connection(&'a Connection),
    /// The linked credential's field.
    Credential(&'a Credential),
}

/// The record that owns `connection`'s key path in `library`.
pub(crate) fn key_owner<'a>(library: &'a Library, connection: &'a Connection) -> KeyOwner<'a> {
    match library.credential(connection.credential_id.as_deref()) {
        Some(credential) => KeyOwner::Credential(credential),
        None => KeyOwner::Connection(connection),
    }
}

/// `rdnext_<name>_ed25519`: the name lowercased, anything but ASCII letters, digits and `-` turned into
/// `_` (runs collapsed, ends trimmed, at most 40 characters); `key` when nothing is left.
#[must_use]
pub fn key_file_stem(name: &str) -> String {
    let mut safe = String::new();
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' {
            c.to_ascii_lowercase()
        } else {
            '_'
        };
        if !(c == '_' && safe.ends_with('_')) {
            safe.push(c);
        }
    }
    let safe: String = safe.trim_matches(['_', '-']).chars().take(40).collect();
    let safe = safe.trim_end_matches(['_', '-']);
    format!("rdnext_{}_ed25519", if safe.is_empty() { "key" } else { safe })
}

/// The `n`th candidate file for `stem` in `dir`: `stem`, then `stem_2`, `stem_3`, …
fn candidate(dir: &Path, stem: &str, n: u32) -> PathBuf {
    if n <= 1 {
        dir.join(stem)
    } else {
        dir.join(format!("{stem}_{n}"))
    }
}

/// `<key>.pub`.
fn pub_path(key: &Path) -> PathBuf {
    let mut name = key.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

/// The comment of generated keys: `<local user>@<computer> RemoteDesktop`, quotes and control
/// characters left out.
#[must_use]
pub fn key_comment() -> String {
    let user = rd_ssh::config::local_user();
    let host = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let who = match (user.is_empty(), host.is_empty()) {
        (false, false) => format!("{user}@{host} "),
        (false, true) => format!("{user} "),
        (true, false) => format!("{host} "),
        (true, true) => String::new(),
    };
    format!("{who}RemoteDesktop")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\'' | '"' | '\\'))
        .collect()
}

#[cfg(unix)]
fn create_file(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
}

/// Windows: the file inherits the profile folder's ACL (the user, SYSTEM, Administrators), which
/// OpenSSH accepts for private keys.
#[cfg(not(unix))]
fn create_file(path: &Path, _mode: u32) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Creates `dir` when missing (`0700` on Unix).
fn ensure_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Generates an unencrypted ed25519 key pair in `dir` (created if missing) named after `stem` without
/// overwriting anything: the private key in OpenSSH format (`0600` on Unix) and `<key>.pub` (`0644`).
/// Returns the private key's path.
///
/// # Errors
/// When the folder or the files can't be created, or the key can't be encoded.
pub fn generate_key_pair(dir: &Path, stem: &str, comment: &str) -> io::Result<PathBuf> {
    ensure_dir(dir)?;
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(io::Error::other)?;
    key.set_comment(comment);
    let pem = key.to_openssh(LineEnding::LF).map_err(io::Error::other)?;
    let public = key.public_key().to_openssh().map_err(io::Error::other)?;
    let mut n = 1;
    let (path, mut file) = loop {
        let path = candidate(dir, stem, n);
        n += 1;
        if pub_path(&path).exists() {
            continue;
        }
        match create_file(&path, 0o600) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    };
    let written = file
        .write_all(pem.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            let mut file = create_file(&pub_path(&path), 0o644)?;
            file.write_all(format!("{public}\n").as_bytes())
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// Checks one `authorized_keys` style public key line, `<algorithm> <base64> [comment]`, and returns it
/// with single spaces between the parts. Refuses quotes, backslashes and control characters (the line
/// goes into a single-quoted shell word) and anything `ssh-key` can't parse.
///
/// # Errors
/// The reason, in English.
pub fn validate_public_key(line: &str) -> Result<String, String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("empty".to_owned());
    }
    if line
        .chars()
        .any(|c| (c.is_control() && c != '\t') || matches!(c, '\'' | '"' | '\\'))
    {
        return Err("quotes, backslashes or control characters".to_owned());
    }
    let split = |text: &str| -> (String, String) {
        let text = text.trim_start_matches([' ', '\t']);
        match text.split_once([' ', '\t']) {
            Some((head, rest)) => (head.to_owned(), rest.trim_matches([' ', '\t']).to_owned()),
            None => (text.to_owned(), String::new()),
        }
    };
    let (algorithm, rest) = split(line);
    let (data, comment) = split(&rest);
    if data.is_empty() {
        return Err("no key data".to_owned());
    }
    let algorithm_ok = algorithm
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '@' | '.' | '_'));
    let data_ok = data
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
    if !algorithm_ok || !data_ok {
        return Err("not an OpenSSH public key".to_owned());
    }
    let normalized = if comment.is_empty() {
        format!("{algorithm} {data}")
    } else {
        format!("{algorithm} {data} {comment}")
    };
    PublicKey::from_openssh(&normalized).map_err(|e| e.to_string())?;
    Ok(normalized)
}

/// The idempotent remote command installing `key` (a [`validate_public_key`] result): `~/.ssh` (`700`)
/// and `authorized_keys` (`600`) are created if missing, a missing final newline is added, and the key is
/// appended only when the exact line isn't there yet.
#[must_use]
pub fn install_command(key: &str) -> String {
    let file = "~/.ssh/authorized_keys";
    format!(
        "umask 077; mkdir -p ~/.ssh && touch {file} && chmod 700 ~/.ssh && chmod 600 {file} && \
         (grep -qxF -e '{key}' {file} || {{ if [ -s {file} ] && [ -n \"$(tail -c 1 {file})\" ]; then \
         echo >> {file}; fi; printf '%s\\n' '{key}' >> {file}; }})"
    )
}

/// The public key line for the private key at `path`: `<path>.pub`, else the public half of the key file,
/// else of the unlocked copy.
///
/// # Errors
/// `cannotReadKey` (detail: the path) or `invalidPublicKey`.
fn public_key_of(path: &Path, unlocked: Option<&str>) -> Result<String, CommandError> {
    let shown = path.display().to_string();
    let invalid = |reason: String| {
        CommandError::new("invalidPublicKey", "The public key isn't valid: %@").with_detail(reason)
    };
    let pub_file = pub_path(path);
    if pub_file.is_file() {
        let text = std::fs::read_to_string(&pub_file).map_err(|_| {
            CommandError::new("cannotReadKey", "Can't read the SSH key: %@")
                .with_detail(pub_file.display().to_string())
        })?;
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or_default();
        return validate_public_key(line).map_err(|e| invalid(format!("{}: {e}", pub_file.display())));
    }
    let from_file = std::fs::read(path)
        .ok()
        .and_then(|bytes| PrivateKey::from_openssh(bytes).ok());
    let key = from_file
        .or_else(|| unlocked.and_then(|pem| PrivateKey::from_openssh(pem).ok()))
        .ok_or_else(|| {
            CommandError::new("cannotReadKey", "Can't read the SSH key: %@").with_detail(shown.clone())
        })?;
    let line = key
        .public_key()
        .to_openssh()
        .map_err(|e| invalid(e.to_string()))?;
    validate_public_key(&line).map_err(|e| invalid(format!("{shown}: {e}")))
}

fn connect_error(error: &BackgroundError) -> CommandError {
    CommandError::new(error.code(), &error.to_string())
}

fn install_failed(reason: impl Into<String>) -> CommandError {
    CommandError::new("keyInstallFailed", "Couldn't install the SSH key: %@").with_detail(reason)
}

/// Everything the upload needs, read from the library under the lock.
struct Prepared {
    host: String,
    key_path: String,
    generated: bool,
    public_key: String,
    upload: rd_ssh::background::ConnectSpec,
    verify: rd_ssh::background::ConnectSpec,
}

impl AppCore {
    /// Sets the key setup's host key prompt notifier (the Tauri layer emits `host-key-prompt`).
    pub fn set_key_setup_hooks(&self, notifier: KeySetupNotifier) {
        *lock(&self.key_setup.notifier) = Some(notifier);
    }

    /// Answers a key setup host key prompt; `false` when `request_id` isn't one.
    pub(crate) fn key_setup_host_key_answer(&self, request_id: &str, accept: bool) -> bool {
        match lock(&self.key_setup.prompts).remove(request_id) {
            Some(answer) => {
                let _ = answer.send(accept);
                true
            }
            None => false,
        }
    }

    /// Generates a key pair in `~/.ssh` for the editors' "Generate…" (nothing is saved); `name` (the
    /// connection or credential name) names the file. Returns the `~`-relative path.
    ///
    /// # Errors
    /// `noHome` or `keyGenerateFailed`.
    pub fn ssh_key_generate(name: &str) -> Result<String, CommandError> {
        let home = dirs::home_dir().ok_or_else(no_home)?;
        generate_in(&home, name)
    }

    /// "Set up SSH key login": installs the connection's key on its server, generating (and saving) a new
    /// one first when it has none, then checks a login with only the key.
    ///
    /// # Errors
    /// `locked`, `notFound`, `notSsh`, `emptyHost`, `readOnly` (when a key has to be saved), `noHome`,
    /// `keyGenerateFailed`, `cannotReadKey`, `invalidPublicKey`, a connection code (`connectFailed`,
    /// `authFailed`, `hostKeyRejected`, …) or `keyInstallFailed`.
    pub async fn ssh_key_setup(&self, connection_id: &str) -> Result<KeySetupDto, CommandError> {
        let prepared = self.key_setup_prepare(connection_id)?;
        let weak: Weak<KeySetupShared> = Arc::downgrade(&self.key_setup);
        let id = connection_id.to_owned();
        let prompt: HostKeyPrompt = Arc::new(move |question| match weak.upgrade() {
            Some(shared) => shared.ask_host_key(&id, question),
            None => oneshot::channel().1,
        });
        let Prepared {
            host,
            key_path,
            generated,
            public_key,
            upload,
            verify,
        } = prepared;
        let verified = runtime()
            .spawn(async move {
                let client = Client::connect(upload, Some(prompt))
                    .await
                    .map_err(|e| connect_error(&e))?;
                let output = client.exec(&install_command(&public_key)).await;
                client.disconnect().await;
                let output = output.map_err(|e| install_failed(e.to_string()))?;
                if !output.succeeded() {
                    let line = output.error_line();
                    return Err(install_failed(if line.is_empty() {
                        output
                            .status
                            .map_or_else(|| "no exit status".to_owned(), |s| format!("exit status {s}"))
                    } else {
                        line
                    }));
                }
                Ok(match Client::connect(verify, None).await {
                    Ok(client) => {
                        client.disconnect().await;
                        true
                    }
                    Err(_) => false,
                })
            })
            .await
            .map_err(|e| CommandError::new("internal", "The command failed.").with_detail(e.to_string()))??;
        Ok(KeySetupDto {
            host,
            key_path,
            generated,
            verified,
        })
    }

    /// Finds the connection, generates and saves a key when it has none, and builds both connect specs.
    fn key_setup_prepare(&self, connection_id: &str) -> Result<Prepared, CommandError> {
        let inner = self.inner();
        let (db, vault) = Self::unlocked(&inner)?;
        let find = |library: &Library| {
            library
                .connections
                .iter()
                .find(|c| c.id == connection_id)
                .cloned()
                .ok_or_else(|| CommandError::not_found(connection_id))
        };
        let mut library = db.library()?;
        let mut connection = find(&library)?;
        if connection.kind != ConnectionType::Ssh {
            return Err(CommandError::new(
                "notSsh",
                "SSH key login can only be set up for SSH connections.",
            ));
        }
        let resolved = library.resolved(&connection);
        // Checks the host and finds the home folder (the test hook's, else the user's).
        let home = self.background_spec(&resolved, SshSecrets::default())?.home;
        let has_key = resolved
            .ssh_key_path
            .as_deref()
            .is_some_and(|p| !p.trim_matches([' ', '\t']).is_empty());
        let generated = !has_key;
        if generated {
            let (db, _) = Self::writable(&inner)?;
            let home = home.ok_or_else(no_home)?;
            let path = generate_in(&home, &connection.name)?;
            match key_owner(&library, &connection) {
                KeyOwner::Credential(credential) => {
                    let mut credential = credential.clone();
                    credential.ssh_key_path = Some(path);
                    // The new key has no passphrase, so no unlocked copy.
                    credential.ssh_key_unlocked_enc = None;
                    db.save_credential(&credential)?;
                }
                KeyOwner::Connection(own) => {
                    let mut own = own.clone();
                    own.ssh_key_path = Some(path);
                    db.save_connection(&own)?;
                }
            }
            library = db.library()?;
            connection = find(&library)?;
        }
        let (resolved, secrets) = rd_ssh::resolve(&library, &connection, |s| vault.open(s))?;
        drop(inner);

        let verify_secrets = SshSecrets::new(None, secrets.passphrase.clone(), secrets.unlocked_key.clone());
        let unlocked = secrets.unlocked_key.clone();
        let upload = self.background_spec(&resolved, secrets)?;
        let mut verify = self.background_spec(&resolved, verify_secrets)?;
        verify.use_agent = false;
        let key_file = upload.key_path.clone().ok_or_else(|| {
            CommandError::new("cannotReadKey", "Can't read the SSH key: %@").with_detail(String::new())
        })?;
        let public_key = public_key_of(&key_file, unlocked.as_ref().map(ExposeSecret::expose_secret))?;
        Ok(Prepared {
            host: resolved.host.trim_matches([' ', '\t']).to_owned(),
            key_path: resolved.ssh_key_path.unwrap_or_default(),
            generated,
            public_key,
            upload,
            verify,
        })
    }
}

fn no_home() -> CommandError {
    CommandError::new("noHome", "There is no home folder to put the SSH key in.")
}

/// Generates a pair in `<home>/.ssh` named after `name`; the `~`-relative path.
fn generate_in(home: &Path, name: &str) -> Result<String, CommandError> {
    let path = generate_key_pair(&home.join(".ssh"), &key_file_stem(name), &key_comment()).map_err(|e| {
        CommandError::new("keyGenerateFailed", "Couldn't create the SSH key: %@").with_detail(e.to_string())
    })?;
    Ok(home_relative(&path, Some(home)))
}
