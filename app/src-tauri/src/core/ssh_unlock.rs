//! Removing the passphrase from an SSH private key, so SSH can use the stored copy without prompting.
//!
//! Replaces the Swift `SSHKeyUnlocker` (`ssh-keygen -p`) with the `ssh-key` crate. The key file is only
//! read; the unlocked copy exists in memory and is sealed by the caller.

use std::path::{Path, PathBuf};

use ssh_key::{LineEnding, PrivateKey};
use zeroize::Zeroizing;

/// Why a key could not be unlocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlockError {
    /// The file is missing or unreadable.
    FileUnreadable,
    /// The passphrase does not decrypt the key.
    WrongPassphrase,
    /// The file is not an OpenSSH private key, or re-encoding failed.
    Failed(String),
}

/// `~/…` to an absolute path in the user's home.
#[must_use]
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// The inverse of [`expand_tilde`] for the key file picker (Swift `SSHKeyField`): a path inside `home`
/// becomes `~/…` with `/` separators, anything else stays absolute.
#[must_use]
pub fn home_relative(path: &Path, home: Option<&Path>) -> String {
    let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return path.display().to_string();
    };
    let parts: Vec<String> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        "~".to_owned()
    } else {
        format!("~/{}", parts.join("/"))
    }
}

/// Reads the key at `path` (`~` expanded), decrypts it with `passphrase` if it is encrypted, and returns
/// it as an unencrypted OpenSSH PEM.
///
/// # Errors
/// [`UnlockError::FileUnreadable`], [`UnlockError::WrongPassphrase`] or [`UnlockError::Failed`].
pub fn unlock(path: &str, passphrase: &str) -> Result<Zeroizing<String>, UnlockError> {
    let bytes = Zeroizing::new(std::fs::read(expand_tilde(path)).map_err(|_| UnlockError::FileUnreadable)?);
    let key = PrivateKey::from_openssh(&*bytes).map_err(|e| UnlockError::Failed(e.to_string()))?;
    let key = if key.is_encrypted() {
        key.decrypt(passphrase)
            .map_err(|_| UnlockError::WrongPassphrase)?
    } else {
        key
    };
    key.to_openssh(LineEnding::LF)
        .map_err(|e| UnlockError::Failed(e.to_string()))
}

#[cfg(test)]
pub(crate) mod tests {
    use ssh_key::rand_core::OsRng;
    use ssh_key::{Algorithm, PrivateKey};

    use super::*;

    /// Writes a fresh ed25519 key encrypted with `passphrase` to `path`.
    pub(crate) fn write_encrypted_key(path: &std::path::Path, passphrase: &str) -> PrivateKey {
        let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
        let encrypted = key.encrypt(&mut OsRng, passphrase).unwrap();
        std::fs::write(path, encrypted.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
        key
    }

    #[test]
    fn unlocks_with_the_right_passphrase_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        let original = write_encrypted_key(&path, "key-pass");
        let path_text = path.to_str().unwrap();

        let pem = unlock(path_text, "key-pass").unwrap();
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        let parsed = PrivateKey::from_openssh(pem.as_bytes()).unwrap();
        assert!(!parsed.is_encrypted());
        assert_eq!(parsed.public_key(), original.public_key());

        assert_eq!(unlock(path_text, "nope"), Err(UnlockError::WrongPassphrase));
        assert_eq!(
            unlock(&format!("{path_text}.missing"), "key-pass"),
            Err(UnlockError::FileUnreadable)
        );
        std::fs::write(&path, "not a key").unwrap();
        assert!(matches!(
            unlock(path_text, "key-pass"),
            Err(UnlockError::Failed(_))
        ));
    }

    #[test]
    fn expands_the_home_directory() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/.ssh/id"), home.join(".ssh/id"));
            assert_eq!(expand_tilde("~"), home);
        }
        assert_eq!(expand_tilde("/etc/key"), PathBuf::from("/etc/key"));
    }

    #[test]
    fn relativizes_paths_inside_the_home_directory() {
        let home = Path::new("/home/me");
        assert_eq!(
            home_relative(Path::new("/home/me/.ssh/id_ed25519"), Some(home)),
            "~/.ssh/id_ed25519"
        );
        assert_eq!(home_relative(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(
            home_relative(Path::new("/home/meow/key"), Some(home)),
            "/home/meow/key"
        );
        assert_eq!(home_relative(Path::new("/etc/key"), Some(home)), "/etc/key");
        assert_eq!(home_relative(Path::new("/etc/key"), None), "/etc/key");
        if let Some(home) = dirs::home_dir() {
            let picked = home.join(".ssh").join("id_rsa");
            assert_eq!(expand_tilde(&home_relative(&picked, Some(&home))), picked);
        }
    }
}
