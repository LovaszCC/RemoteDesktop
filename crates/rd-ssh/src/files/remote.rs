//! The remote side of a file browser: one authenticated russh connection, listings over an exec channel
//! (or SFTP on servers without a shell userland such as RouterOS), and the small probes a transfer needs.
//!
//! On open the server is asked whether it has `tar`. Without it the SFTP subsystem is tried; when that
//! works, listings and single-file transfers use SFTP.

use russh_sftp::client::SftpSession;
use serde::Serialize;

use super::listing::{self, Entry, EntryKind, Listing, sort_entries};
use super::local::permission_string;
use super::path as rpath;
use super::plan;
use crate::background::Client;

/// Why a remote operation failed. [`code`](Self::code) is a stable id the UI translates.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesError {
    /// The folder couldn't be opened (the shell's or the server's message).
    #[error("{0}")]
    Listing(String),
    /// The listing output couldn't be parsed.
    #[error("Can't read the folder listing of {0}.")]
    Unreadable(String),
    /// The SSH connection failed.
    #[error("The SSH connection failed: {0}")]
    Ssh(String),
}

impl FilesError {
    /// `listFailed`, `unreadable` or `ssh`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Listing(_) => "listFailed",
            Self::Unreadable(_) => "unreadable",
            Self::Ssh(_) => "ssh",
        }
    }
}

/// How the server is reached for listings and transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteMode {
    /// Shell commands, tar streams.
    Tar,
    /// Shell commands but no `tar` and no SFTP: listings only.
    ShellOnly,
    /// No `tar`: SFTP for listings and single files (RouterOS).
    Sftp,
}

/// A connected file browser's remote side.
pub struct RemoteFiles {
    client: Client,
    sftp: Option<SftpSession>,
    mode: RemoteMode,
}

impl std::fmt::Debug for RemoteFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteFiles")
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

fn ssh_error(error: impl std::fmt::Display) -> FilesError {
    FilesError::Ssh(error.to_string())
}

fn sftp_entry(name: String, metadata: &russh_sftp::client::fs::Metadata) -> Entry {
    let kind = if metadata.is_symlink() {
        EntryKind::Symlink
    } else if metadata.is_dir() {
        EntryKind::Directory
    } else if metadata.is_regular() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Entry {
        name,
        kind,
        size: metadata.size.unwrap_or(0),
        modified: metadata.mtime.map(i64::from),
        permissions: permission_string(metadata.permissions.unwrap_or(0), kind),
    }
}

impl RemoteFiles {
    /// Probes the server (tar, else SFTP) over an authenticated connection.
    pub async fn open(client: Client) -> Self {
        let has_tar = client
            .exec(plan::REMOTE_TAR_CHECK)
            .await
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("yes"));
        if has_tar {
            return Self {
                client,
                sftp: None,
                mode: RemoteMode::Tar,
            };
        }
        match client.sftp().await {
            Ok(sftp) => Self {
                client,
                sftp: Some(sftp),
                mode: RemoteMode::Sftp,
            },
            Err(_) => Self {
                client,
                sftp: None,
                mode: RemoteMode::ShellOnly,
            },
        }
    }

    /// SFTP for everything, even when the server has `tar` (the RouterOS path, for tests).
    ///
    /// # Errors
    /// [`FilesError::Ssh`] when the SFTP subsystem can't be opened.
    pub async fn open_sftp(client: Client) -> Result<Self, FilesError> {
        let sftp = client.sftp().await.map_err(FilesError::Ssh)?;
        Ok(Self {
            client,
            sftp: Some(sftp),
            mode: RemoteMode::Sftp,
        })
    }

    /// How the server is used.
    #[must_use]
    pub fn mode(&self) -> RemoteMode {
        self.mode
    }

    /// Whether folders (tar streams) can be copied.
    #[must_use]
    pub fn has_tar(&self) -> bool {
        self.mode == RemoteMode::Tar
    }

    /// The connection.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The SFTP session in [`RemoteMode::Sftp`].
    #[must_use]
    pub fn sftp(&self) -> Option<&SftpSession> {
        self.sftp.as_ref()
    }

    /// Whether the connection is gone.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    /// Closes the connection.
    pub async fn disconnect(&self) {
        if let Some(sftp) = &self.sftp {
            let _ = sftp.close().await;
        }
        self.client.disconnect().await;
    }

    /// Lists a folder (`~` and `~/x` are the home folder); the answer carries the absolute path.
    ///
    /// # Errors
    /// See [`FilesError`].
    pub async fn list(&self, path: &str) -> Result<Listing, FilesError> {
        if let Some(sftp) = &self.sftp {
            return self.list_sftp(sftp, path).await;
        }
        let output = self
            .client
            .exec(&listing::script(path))
            .await
            .map_err(ssh_error)?;
        if !output.succeeded() {
            let line = output.error_line();
            return Err(FilesError::Listing(if line.is_empty() {
                format!("Can't open {path}.")
            } else {
                line
            }));
        }
        listing::parse(&output.stdout, now()).map_err(|_| FilesError::Unreadable(path.to_owned()))
    }

    async fn resolve_sftp(sftp: &SftpSession, path: &str) -> Result<String, FilesError> {
        let listing_error = |e: russh_sftp::client::error::Error| FilesError::Listing(e.to_string());
        let home = || async { sftp.canonicalize(".").await.map_err(listing_error) };
        let target = if path.is_empty() || path == "~" {
            home().await?
        } else if let Some(rest) = path.strip_prefix("~/") {
            rpath::join(&home().await?, rest)
        } else {
            path.to_owned()
        };
        sftp.canonicalize(target).await.map_err(listing_error)
    }

    async fn list_sftp(&self, sftp: &SftpSession, path: &str) -> Result<Listing, FilesError> {
        let absolute = Self::resolve_sftp(sftp, path).await?;
        let items = sftp
            .read_dir(absolute.clone())
            .await
            .map_err(|e| FilesError::Listing(e.to_string()))?;
        let mut entries: Vec<Entry> = items
            .filter(|item| {
                let name = item.file_name();
                name != "." && name != ".."
            })
            .map(|item| sftp_entry(item.file_name(), &item.metadata()))
            .collect();
        sort_entries(&mut entries);
        Ok(Listing {
            path: absolute,
            entries,
        })
    }

    /// Whether something (even a dangling link) is at `path`.
    pub async fn exists(&self, path: &str) -> bool {
        if let Some(sftp) = &self.sftp {
            return sftp.symlink_metadata(path).await.is_ok();
        }
        self.succeeds(&plan::remote_exists_script(path)).await
    }

    /// Whether `path` is a folder.
    pub async fn is_directory(&self, path: &str) -> bool {
        if let Some(sftp) = &self.sftp {
            return sftp.metadata(path).await.is_ok_and(|m| m.is_dir());
        }
        self.succeeds(&plan::remote_is_directory_script(path)).await
    }

    /// The bytes `path` holds (`du`, so rounded up to KiB), when known.
    pub async fn size(&self, path: &str) -> Option<u64> {
        if let Some(sftp) = &self.sftp {
            return sftp.metadata(path).await.ok().and_then(|m| m.size);
        }
        let output = self.client.exec(&plan::remote_size_script(path)).await.ok()?;
        plan::parse_du_output(&String::from_utf8_lossy(&output.stdout))
    }

    /// Removes a partial copy (best effort).
    pub async fn remove(&self, path: &str) {
        if let Some(sftp) = &self.sftp {
            let _ = sftp.remove_file(path).await;
            return;
        }
        let _ = self.client.exec(&plan::remote_remove_script(path)).await;
    }

    async fn succeeds(&self, command: &str) -> bool {
        self.client.exec(command).await.is_ok_and(|o| o.succeeded())
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes() {
        assert_eq!(FilesError::Listing("x".into()).code(), "listFailed");
        assert_eq!(FilesError::Unreadable("/x".into()).code(), "unreadable");
        assert_eq!(FilesError::Ssh("gone".into()).code(), "ssh");
        assert_eq!(
            FilesError::Unreadable("/x".into()).to_string(),
            "Can't read the folder listing of /x."
        );
        assert!(now() > 1_700_000_000);
    }

    #[test]
    fn sftp_metadata_becomes_an_entry() {
        let mut metadata = russh_sftp::client::fs::Metadata {
            permissions: Some(0o040_755),
            size: Some(4096),
            mtime: Some(1_760_000_000),
            ..Default::default()
        };
        let entry = sftp_entry("d".into(), &metadata);
        assert_eq!(entry.kind, EntryKind::Directory);
        assert_eq!(entry.permissions, "drwxr-xr-x");
        assert_eq!(entry.modified, Some(1_760_000_000));
        metadata.permissions = Some(0o100_644);
        assert_eq!(sftp_entry("f".into(), &metadata).kind, EntryKind::File);
        metadata.permissions = Some(0o120_777);
        assert_eq!(sftp_entry("l".into(), &metadata).kind, EntryKind::Symlink);
        metadata.permissions = None;
        assert_eq!(sftp_entry("o".into(), &metadata).kind, EntryKind::Other);
    }
}
