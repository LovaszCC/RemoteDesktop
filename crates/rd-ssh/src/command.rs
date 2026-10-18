//! The `ssh` argument list for a connection and the secrets that go with it (a port of the Swift
//! `SSHLaunchScript.sshArguments` and `AppState.sshScript`).
//!
//! There is no launch script any more: the arguments go straight to the PTY or the external terminal, and
//! the secrets are answered over the askpass channel (see [`crate::askpass`]).

use std::path::{Path, PathBuf};

use rd_core::{Connection, ConnectionType, Library, Sealed};
use secrecy::{ExposeSecret, SecretString};

/// Why no argument list could be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgsError {
    /// The connection has no host.
    #[error("The connection has no host.")]
    EmptyHost,
    /// The destination starts with `-`, so `ssh` would read it as an option.
    #[error("The host or user name can't start with a dash.")]
    InvalidHost,
}

/// The decrypted secrets of a resolved connection. Empty values count as absent, and a key passphrase is
/// dropped when an unlocked key replaces the key file (Swift `SSHLaunchScript.init`).
#[derive(Debug, Default)]
pub struct SshSecrets {
    /// The login password.
    pub password: Option<SecretString>,
    /// The passphrase of the key file.
    pub passphrase: Option<SecretString>,
    /// A passphrase-free copy of the credential's key (OpenSSH PEM).
    pub unlocked_key: Option<SecretString>,
}

fn non_empty(secret: Option<SecretString>) -> Option<SecretString> {
    secret.filter(|s| !s.expose_secret().is_empty())
}

impl SshSecrets {
    /// Applies the empty-value and unlocked-key rules.
    #[must_use]
    pub fn new(
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
        unlocked_key: Option<SecretString>,
    ) -> Self {
        let unlocked_key = non_empty(unlocked_key);
        let passphrase = if unlocked_key.is_some() {
            None
        } else {
            non_empty(passphrase)
        };
        Self {
            password: non_empty(password),
            passphrase,
            unlocked_key,
        }
    }

    /// Whether the askpass channel has anything to answer on its own.
    #[must_use]
    pub fn uses_askpass(&self) -> bool {
        self.password.is_some() || self.passphrase.is_some()
    }
}

/// Resolves `connection` against its linked credential and opens the secrets with `open` (the vault):
/// the password and key passphrase of the resolved connection, plus the credential's unlocked key. Called
/// on every (re)connect, so edits to the entry or the credential apply.
///
/// # Errors
/// The first error `open` returns.
pub fn resolve<E>(
    library: &Library,
    connection: &Connection,
    mut open: impl FnMut(&Sealed) -> Result<SecretString, E>,
) -> Result<(Connection, SshSecrets), E> {
    let resolved = library.resolved(connection);
    let mut open_optional = |sealed: Option<&Sealed>| sealed.map(&mut open).transpose();
    let password = open_optional(resolved.password_enc.as_ref())?;
    let passphrase = open_optional(resolved.ssh_key_passphrase_enc.as_ref())?;
    let unlocked = library
        .credential(connection.credential_id.as_deref())
        .and_then(|c| c.ssh_key_unlocked_enc.as_ref());
    let unlocked_key = open_optional(unlocked)?;
    Ok((resolved, SshSecrets::new(password, passphrase, unlocked_key)))
}

/// `~` and `~/…` in the user's home (`home`); anything else unchanged.
#[must_use]
pub fn expand_tilde(path: &str, home: Option<&Path>) -> PathBuf {
    match (path, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (path, Some(home)) if path.starts_with("~/") => home.join(&path[2..]),
        (path, _) => PathBuf::from(path),
    }
}

/// Swift `.whitespaces` trim (spaces and tabs).
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

/// The `ssh` arguments (without the program) for a resolved connection:
/// - `-p <port>` unless the port is 22;
/// - `-i <unlocked_key> -o IdentitiesOnly=yes` when an unlocked key file is used, otherwise `-i <key>` for
///   the connection's key path (`~` expanded with `home`);
/// - `user@host`, or just the host without a user.
///
/// The user's `~/.ssh/config` stays in effect (no `-F`).
///
/// # Errors
/// [`ArgsError::EmptyHost`], or [`ArgsError::InvalidHost`] when the destination would read as an option.
pub fn ssh_arguments(
    connection: &Connection,
    unlocked_key: Option<&Path>,
    home: Option<&Path>,
) -> Result<Vec<String>, ArgsError> {
    let host = trim(&connection.host);
    if host.is_empty() {
        return Err(ArgsError::EmptyHost);
    }
    let user = trim(&connection.username);
    let destination = if user.is_empty() {
        host.to_owned()
    } else {
        format!("{user}@{host}")
    };
    if destination.starts_with('-') {
        return Err(ArgsError::InvalidHost);
    }
    let mut args = Vec::new();
    let port = connection.effective_port();
    if port != ConnectionType::Ssh.default_port() {
        args.extend(["-p".to_owned(), port.to_string()]);
    }
    if let Some(key) = unlocked_key {
        args.extend([
            "-i".to_owned(),
            key.display().to_string(),
            "-o".to_owned(),
            "IdentitiesOnly=yes".to_owned(),
        ]);
    } else if let Some(key) = connection.ssh_key_path.as_deref().map(trim)
        && !key.is_empty()
    {
        args.extend(["-i".to_owned(), expand_tilde(key, home).display().to_string()]);
    }
    args.push(destination);
    Ok(args)
}

#[cfg(test)]
mod tests {
    use rd_core::{Credential, Sealed};

    use super::*;

    fn ssh(host: &str, user: &str) -> Connection {
        let mut c = Connection::new(ConnectionType::Ssh, "w", host);
        c.username = user.into();
        c
    }

    const HOME: &str = "/home/me";

    fn args(connection: &Connection, key: Option<&Path>) -> Vec<String> {
        ssh_arguments(connection, key, Some(Path::new(HOME))).unwrap()
    }

    /// `SSHLaunchScriptTests.sshArgumentsDefaultPort`.
    #[test]
    fn default_port() {
        assert_eq!(args(&ssh("web01", "root"), None), ["root@web01"]);
    }

    /// `SSHLaunchScriptTests.sshArgumentsPortKeyNoUser`.
    #[test]
    fn port_key_and_no_user() {
        let mut c = ssh("web01", "");
        c.port = Some(2222);
        c.ssh_key_path = Some("~/.ssh/id_ed25519".into());
        let home_key = Path::new(HOME).join(".ssh/id_ed25519").display().to_string();
        assert_eq!(args(&c, None), ["-p", "2222", "-i", home_key.as_str(), "web01"]);
    }

    #[test]
    fn unlocked_key_replaces_the_key_file() {
        let mut c = ssh(" h ", " u ");
        c.ssh_key_path = Some("/k".into());
        let key = Path::new("/tmp/session/key");
        assert_eq!(
            args(&c, Some(key)),
            [
                "-i",
                key.display().to_string().as_str(),
                "-o",
                "IdentitiesOnly=yes",
                "u@h"
            ]
        );
    }

    #[test]
    fn blank_key_path_and_explicit_port_22_add_nothing() {
        let mut c = ssh("h", "");
        c.port = Some(22);
        c.ssh_key_path = Some("  ".into());
        assert_eq!(args(&c, None), ["h"]);
        let mut mikrotik = Connection::new(ConnectionType::Mikrotik, "m", "router");
        mikrotik.username = "admin".into();
        assert_eq!(args(&mikrotik, None), ["admin@router"]);
    }

    #[test]
    fn refuses_empty_and_option_like_destinations() {
        assert_eq!(
            ssh_arguments(&ssh(" ", "u"), None, None),
            Err(ArgsError::EmptyHost)
        );
        assert_eq!(
            ssh_arguments(&ssh("-oProxyCommand=x", ""), None, None),
            Err(ArgsError::InvalidHost)
        );
        assert_eq!(
            ssh_arguments(&ssh("h", "-oProxyCommand=x"), None, None),
            Err(ArgsError::InvalidHost)
        );
    }

    #[test]
    fn tilde_expansion() {
        let home = Path::new(HOME);
        assert_eq!(expand_tilde("~", Some(home)), home);
        assert_eq!(expand_tilde("~/a", Some(home)), home.join("a"));
        assert_eq!(expand_tilde("~other/a", Some(home)), PathBuf::from("~other/a"));
        assert_eq!(expand_tilde("~/a", None), PathBuf::from("~/a"));
    }

    #[allow(clippy::unnecessary_wraps)]
    fn secret(text: &str) -> Option<SecretString> {
        Some(SecretString::from(text))
    }

    /// `SSHLaunchScriptTests.emptySecretsAreIgnored`, plus the unlocked-key rule.
    #[test]
    fn empty_secrets_are_ignored() {
        let s = SshSecrets::new(secret(""), secret(""), secret(""));
        assert!(!s.uses_askpass());
        assert!(s.unlocked_key.is_none());

        let s = SshSecrets::new(secret("pw"), secret("pp"), None);
        assert!(s.uses_askpass());
        assert_eq!(s.passphrase.unwrap().expose_secret(), "pp");

        let s = SshSecrets::new(None, secret("pp"), secret("KEY"));
        assert!(s.passphrase.is_none());
        assert!(!s.uses_askpass());
        assert_eq!(s.unlocked_key.unwrap().expose_secret(), "KEY");
    }

    /// Fake sealing for the tests: the "sealed" bytes are the plain text.
    fn seal(text: &str) -> Sealed {
        Sealed::from_bytes(text.as_bytes().to_vec())
    }

    fn open(sealed: &Sealed) -> Result<SecretString, String> {
        if sealed.as_bytes() == b"bad" {
            return Err("corrupt".into());
        }
        Ok(SecretString::from(
            String::from_utf8(sealed.as_bytes().to_vec()).unwrap(),
        ))
    }

    #[test]
    fn resolves_the_credential_and_its_unlocked_key() {
        let mut credential = Credential::new("cred");
        credential.username = "admin".into();
        credential.password_enc = Some(seal("cred-pw"));
        credential.ssh_key_path = Some("~/.ssh/id".into());
        credential.ssh_key_passphrase_enc = Some(seal("pp"));
        credential.ssh_key_unlocked_enc = Some(seal("PEM"));
        let mut connection = ssh("h", "local");
        connection.password_enc = Some(seal("own-pw"));
        connection.credential_id = Some(credential.id.clone());
        let library = Library {
            connections: vec![connection.clone()],
            credentials: vec![credential],
            ..Library::default()
        };

        let (resolved, secrets) = resolve(&library, &connection, open).unwrap();
        assert_eq!(resolved.username, "admin");
        assert_eq!(resolved.ssh_key_path.as_deref(), Some("~/.ssh/id"));
        assert_eq!(secrets.password.unwrap().expose_secret(), "cred-pw");
        assert!(
            secrets.passphrase.is_none(),
            "the unlocked key replaces the passphrase"
        );
        assert_eq!(secrets.unlocked_key.unwrap().expose_secret(), "PEM");

        let mut plain = ssh("h", "u");
        plain.password_enc = Some(seal("own"));
        let (_, secrets) = resolve(&Library::default(), &plain, open).unwrap();
        assert_eq!(secrets.password.unwrap().expose_secret(), "own");
        assert!(secrets.unlocked_key.is_none());

        plain.password_enc = Some(seal("bad"));
        assert_eq!(resolve(&Library::default(), &plain, open).unwrap_err(), "corrupt");
    }
}
