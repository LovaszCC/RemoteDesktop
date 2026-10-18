//! File lists on the OS clipboard, and the folders that hold files copied from RDP sessions.
//!
//! - macOS: `NSPasteboard` file URLs (`arboard`).
//! - Windows: `CF_HDROP` (`arboard`).
//! - Linux (X11): `text/uri-list` plus `x-special/gnome-copied-files` (GNOME Files and other GTK file
//!   managers paste only the latter) and the paths as text, served by a small X11 selection owner
//!   ([`super::x11_files`]); reading uses `arboard` (`text/uri-list`). Without an X display (Wayland
//!   only, no `XWayland`) writing falls back to `arboard`'s `text/uri-list`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{ClipboardError, with_clipboard};

/// The files and folders on the clipboard; `None` when it holds none.
///
/// # Errors
/// When the OS clipboard is unavailable.
pub fn read_file_list() -> Result<Option<Vec<PathBuf>>, ClipboardError> {
    with_clipboard(|c| match c.get().file_list() {
        Ok(paths) if !paths.is_empty() => Ok(Some(paths)),
        Ok(_) | Err(arboard::Error::ContentNotAvailable | arboard::Error::ConversionFailure) => Ok(None),
        Err(e) => Err(e),
    })
}

/// Puts files and folders (absolute paths) on the clipboard, for pasting in the file manager.
///
/// # Errors
/// When the OS clipboard is unavailable or none of the paths exists.
pub fn copy_file_list(paths: &[PathBuf]) -> Result<(), ClipboardError> {
    if paths.is_empty() {
        return Err(ClipboardError("no files".into()));
    }
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    if super::x11_files::serve(paths).is_ok() {
        return Ok(());
    }
    with_clipboard(|c| c.set().file_list(paths))
}

/// Percent-encodes a path for a `file://` URI (RFC 3986: unreserved characters and `/` stay).
#[must_use]
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

/// `text/uri-list`: one URI per line, CRLF-separated (RFC 2483).
#[must_use]
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| file_uri(p)).collect::<Vec<_>>().join("\r\n")
}

/// `x-special/gnome-copied-files`: the operation, then one URI per line.
#[must_use]
pub fn gnome_copied_files(paths: &[PathBuf]) -> String {
    let mut text = String::from("copy");
    for path in paths {
        text.push('\n');
        text.push_str(&file_uri(path));
    }
    text
}

/// The folder under the system temp folder that holds every session's downloads.
#[must_use]
pub fn rdp_clipboard_root() -> PathBuf {
    std::env::temp_dir().join(format!("{}.rdp-clipboard", crate::APP_IDENTITY.identifier))
}

/// A new session's private download folder, `<root>/<pid>-<id>` (not created yet).
#[must_use]
pub fn rdp_clipboard_session_dir() -> PathBuf {
    rdp_clipboard_root().join(format!("{}-{}", std::process::id(), rd_core::new_id()))
}

/// Removes the session folders of app instances that are no longer running (a crash leaves them
/// behind). Returns how many were removed.
pub fn purge_stale_rdp_clipboard_folders() -> usize {
    purge_stale_in(&rdp_clipboard_root(), process_alive)
}

fn process_alive(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

fn purge_stale_in(root: &Path, alive: impl Fn(u32) -> bool) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.split('-').next())
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != std::process::id() && !alive(pid) && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_are_percent_encoded() {
        let paths = [PathBuf::from("/tmp/a b/ű#.txt"), PathBuf::from("/x")];
        assert_eq!(file_uri(&paths[0]), "file:///tmp/a%20b/%C5%B1%23.txt");
        assert_eq!(uri_list(&paths), "file:///tmp/a%20b/%C5%B1%23.txt\r\nfile:///x");
        assert_eq!(
            gnome_copied_files(&paths),
            "copy\nfile:///tmp/a%20b/%C5%B1%23.txt\nfile:///x"
        );
    }

    #[test]
    fn session_folders_live_under_the_root_and_carry_the_pid() {
        let dir = rdp_clipboard_session_dir();
        assert_eq!(dir.parent(), Some(rdp_clipboard_root().as_path()));
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(&format!("{}-", std::process::id())), "{name}");
        assert!(rdp_clipboard_root().ends_with(format!("{}.rdp-clipboard", crate::APP_IDENTITY.identifier)));
    }

    #[test]
    fn only_folders_of_dead_processes_are_purged() {
        let root = tempfile::tempdir().unwrap();
        let mine = format!("{}-a", std::process::id());
        for name in ["1-dead", "2-alive", "notapid", mine.as_str()] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        assert_eq!(purge_stale_in(root.path(), |pid| pid == 2), 1);
        let mut left: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut expected = vec!["2-alive".to_owned(), "notapid".to_owned(), mine];
        expected.sort();
        assert_eq!(left, expected);
        assert_eq!(purge_stale_in(&root.path().join("missing"), |_| false), 0);
        assert!(process_alive(std::process::id()));
    }

    #[test]
    fn an_empty_list_is_refused() {
        assert!(copy_file_list(&[]).is_err());
    }

    /// Touches the real clipboard; run by hand with
    /// `cargo test -p rd-platform -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "needs a desktop clipboard"]
    fn file_lists_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b.txt");
        std::fs::write(&file, b"x").unwrap();
        let file = file.canonicalize().unwrap();
        copy_file_list(std::slice::from_ref(&file)).unwrap();
        assert_eq!(read_file_list().unwrap(), Some(vec![file]));
        super::super::copy_text("text").unwrap();
        assert_eq!(read_file_list().unwrap(), None);
    }
}
