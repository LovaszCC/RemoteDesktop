//! The unlocked key of a credential as a private temp file for `ssh -i` (Swift `SSHLaunchScript.write`):
//! `0600` in a `0700` per-session directory (Windows: an owner-only ACL, best effort), deleted with the
//! value.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

/// The key file; dropping it deletes the file and its directory.
#[derive(Debug)]
pub struct UnlockedKeyFile {
    dir: tempfile::TempDir,
    path: PathBuf,
}

impl UnlockedKeyFile {
    /// Writes `key` (an OpenSSH PEM) to `<new private dir>/key`, adding the trailing newline `ssh` needs.
    ///
    /// # Errors
    /// When the directory or the file can't be created.
    pub fn write(key: &SecretString) -> io::Result<Self> {
        let dir = tempfile::Builder::new().prefix("rd-key-").tempdir()?;
        restrict_dir(dir.path())?;
        let path = dir.path().join("key");
        let mut file = create_private(&path)?;
        let key = key.expose_secret();
        file.write_all(key.as_bytes())?;
        if !key.ends_with('\n') {
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
        Ok(Self { dir, path })
    }

    /// The key file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The private directory holding it.
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Removes inherited entries and grants only the current user (`icacls`; no Win32 calls since the
/// workspace denies `unsafe`). The user's temp folder is already private, so a failure is ignored.
#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
fn restrict_dir(dir: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Ok(user) = std::env::var("USERNAME") {
        let account = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
            _ => user,
        };
        let _ = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("{account}:(OI)(CI)F"))
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    Ok(())
}

#[cfg(windows)]
fn create_private(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_private_key_file_and_deletes_it_on_drop() {
        let file = UnlockedKeyFile::write(&SecretString::from("-----BEGIN KEY-----\nabc")).unwrap();
        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            "-----BEGIN KEY-----\nabc\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(file.dir()), 0o700);
            assert_eq!(mode(file.path()), 0o600);
        }
        let (dir, path) = (file.dir().to_path_buf(), file.path().to_path_buf());
        drop(file);
        assert!(!path.exists());
        assert!(!dir.exists());
    }

    #[test]
    fn keeps_an_existing_trailing_newline() {
        let file = UnlockedKeyFile::write(&SecretString::from("k\n")).unwrap();
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "k\n");
    }
}
