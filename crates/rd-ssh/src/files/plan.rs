//! The remote commands of a transfer (Swift `TransferPlan`) and the conflict policy.
//!
//! A file or folder normally travels as one tar stream: the remote `tar` writes it (gzip optional) and
//! this side unpacks it, or this side writes it and the remote `tar` unpacks it. Without `tar` on the
//! server (RouterOS) single files go over SFTP instead.

use serde::{Deserialize, Serialize};

use crate::external::shell_quote;

/// Which way a transfer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// Remote → local.
    Download,
    /// Local → remote.
    Upload,
}

/// `tar` on the server writing `./name` from `source_directory` to stdout.
#[must_use]
pub fn download_command(name: &str, source_directory: &str, compress: bool) -> String {
    format!(
        "tar -C {} -c{}f - {}",
        shell_quote(source_directory),
        if compress { "z" } else { "" },
        shell_quote(&format!("./{name}"))
    )
}

/// `tar` on the server unpacking stdin into `destination_directory`. `-o`: the server doesn't take over
/// this computer's user and group ids.
#[must_use]
pub fn upload_command(destination_directory: &str, compress: bool) -> String {
    format!(
        "tar -C {} -x{}of -",
        shell_quote(destination_directory),
        if compress { "z" } else { "" }
    )
}

/// Prints `yes` when the server has `tar`.
pub const REMOTE_TAR_CHECK: &str = "command -v tar >/dev/null 2>&1 && echo yes";

/// Size of a remote file or folder (`du -sk`, so rounded up to KiB).
#[must_use]
pub fn remote_size_script(path: &str) -> String {
    format!("du -sk -- {} 2>/dev/null", shell_quote(path))
}

/// `du -sk` output in bytes.
#[must_use]
pub fn parse_du_output(output: &str) -> Option<u64> {
    output
        .split([' ', '\t', '\n'])
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|k| k * 1024)
}

/// Succeeds when something (even a dangling link) is at `path`.
#[must_use]
pub fn remote_exists_script(path: &str) -> String {
    let quoted = shell_quote(path);
    format!("test -e {quoted} || test -L {quoted}")
}

/// Succeeds when `path` is a folder.
#[must_use]
pub fn remote_is_directory_script(path: &str) -> String {
    format!("test -d {}", shell_quote(path))
}

/// Removes a partial copy.
#[must_use]
pub fn remote_remove_script(path: &str) -> String {
    format!("rm -rf -- {}", shell_quote(path))
}

/// The answer to "already exists".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictChoice {
    /// Overwrite (a folder is merged: same names are overwritten, other files stay).
    Replace,
    /// Leave the existing item and don't copy this one.
    Skip,
}

/// What to do when the destination already exists; "Apply to all" turns a choice into a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Ask the user.
    #[default]
    Ask,
    /// Replace every conflict.
    ReplaceAll,
    /// Skip every conflict.
    SkipAll,
}

impl ConflictPolicy {
    /// `None`: ask the user.
    #[must_use]
    pub fn resolve(self, exists: bool) -> Option<ConflictChoice> {
        if !exists {
            return Some(ConflictChoice::Replace);
        }
        match self {
            Self::Ask => None,
            Self::ReplaceAll => Some(ConflictChoice::Replace),
            Self::SkipAll => Some(ConflictChoice::Skip),
        }
    }

    /// The policy "Apply to all" makes of `choice`.
    #[must_use]
    pub fn remembering(choice: ConflictChoice) -> Self {
        match choice {
            ConflictChoice::Replace => Self::ReplaceAll,
            ConflictChoice::Skip => Self::SkipAll,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_streams_tar_with_optional_gzip() {
        assert_eq!(
            download_command("it's dir", "/srv/a b", true),
            "tar -C '/srv/a b' -czf - './it'\\''s dir'"
        );
        assert_eq!(download_command("d", "/s", false), "tar -C '/s' -cf - './d'");
    }

    #[test]
    fn upload_drops_the_owner() {
        assert_eq!(upload_command("/var/www", true), "tar -C '/var/www' -xzof -");
        assert_eq!(upload_command("/var/www", false), "tar -C '/var/www' -xof -");
        assert_eq!(REMOTE_TAR_CHECK, "command -v tar >/dev/null 2>&1 && echo yes");
    }

    #[test]
    fn du_output_and_scripts() {
        assert_eq!(parse_du_output("1234\t/data/x\n"), Some(1_263_616));
        assert_eq!(parse_du_output(""), None);
        assert_eq!(parse_du_output("du: cannot access"), None);
        assert_eq!(remote_size_script("/a b"), "du -sk -- '/a b' 2>/dev/null");
        assert_eq!(remote_exists_script("/x"), "test -e '/x' || test -L '/x'");
        assert_eq!(remote_is_directory_script("/x y"), "test -d '/x y'");
        assert_eq!(remote_remove_script("/t/it's"), "rm -rf -- '/t/it'\\''s'");
    }

    #[test]
    fn conflict_policy() {
        assert_eq!(ConflictPolicy::Ask.resolve(false), Some(ConflictChoice::Replace));
        assert_eq!(ConflictPolicy::Ask.resolve(true), None);
        assert_eq!(
            ConflictPolicy::ReplaceAll.resolve(true),
            Some(ConflictChoice::Replace)
        );
        assert_eq!(ConflictPolicy::SkipAll.resolve(true), Some(ConflictChoice::Skip));
        assert_eq!(
            ConflictPolicy::remembering(ConflictChoice::Replace),
            ConflictPolicy::ReplaceAll
        );
        assert_eq!(
            ConflictPolicy::remembering(ConflictChoice::Skip),
            ConflictPolicy::SkipAll
        );
    }
}
