//! The file browser's local side (Swift `LocalBrowser`): one folder on this computer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::listing::{Entry, EntryKind, sort_entries};

/// Where the local pane starts: Downloads, else the home folder, else the current directory.
#[must_use]
pub fn default_directory() -> PathBuf {
    dirs::download_dir()
        .filter(|d| d.is_dir())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `drwxr-xr-x` from the permission bits.
#[must_use]
pub fn permission_string(mode: u32, kind: EntryKind) -> String {
    const SYMBOLS: &[u8; 9] = b"rwxrwxrwx";
    let mut text = String::with_capacity(10);
    text.push(match kind {
        EntryKind::Directory => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::File => '-',
        EntryKind::Other => '?',
    });
    for (index, symbol) in SYMBOLS.iter().enumerate() {
        text.push(if mode & (1 << (8 - index)) == 0 {
            '-'
        } else {
            char::from(*symbol)
        });
    }
    text
}

/// Whether a name is hidden: a leading dot everywhere, plus the hidden attribute on Windows.
fn is_hidden(name: &str, metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
            return true;
        }
    }
    #[cfg(not(windows))]
    let _ = metadata;
    name.starts_with('.')
}

fn mode_of(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        if metadata.permissions().readonly() {
            0o555
        } else {
            0o755
        }
    }
}

/// One entry for `path` (the link itself for a symlink, as in the remote listing).
fn entry(name: String, metadata: &fs::Metadata) -> Entry {
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    Entry {
        name,
        kind,
        size: if kind == EntryKind::Directory {
            0
        } else {
            metadata.len()
        },
        modified,
        permissions: permission_string(mode_of(metadata), kind),
    }
}

/// A local folder's entries (directories first, then by name). A symlink to a folder counts as a folder
/// so it can be opened.
///
/// # Errors
/// When the folder can't be read.
pub fn list(directory: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(directory)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = fs::symlink_metadata(item.path()) else {
            continue;
        };
        if !show_hidden && is_hidden(&name, &metadata) {
            continue;
        }
        let mut entry = entry(name, &metadata);
        if entry.kind == EntryKind::Symlink && item.path().is_dir() {
            entry.kind = EntryKind::Directory;
        }
        entries.push(entry);
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// The bytes a local file or folder holds (symlinks are not followed).
#[must_use]
pub fn size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !metadata.is_dir() {
        return metadata.len();
    }
    fs::read_dir(path).map_or(0, |items| items.flatten().map(|item| size(&item.path())).sum())
}

/// Whether something (even a dangling symlink) is at `path`.
#[must_use]
pub fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Removes a file, link or folder tree (best effort).
pub fn remove(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = fs::remove_file(path);
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_strings() {
        assert_eq!(permission_string(0o755, EntryKind::Directory), "drwxr-xr-x");
        assert_eq!(permission_string(0o100_644, EntryKind::File), "-rw-r--r--");
        assert_eq!(permission_string(0o777, EntryKind::Symlink), "lrwxrwxrwx");
        assert_eq!(permission_string(0, EntryKind::Other), "?---------");
    }

    #[test]
    fn lists_sizes_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/inner.txt"), "12345").unwrap();
        fs::write(dir.path().join("b.txt"), "abc").unwrap();
        fs::write(dir.path().join(".hidden"), "x").unwrap();

        let entries = list(dir.path(), false).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", "b.txt"]);
        assert_eq!(entries[1].size, 3);
        assert!(entries[1].modified.is_some());
        assert!(entries[0].is_directory());
        assert_eq!(list(dir.path(), true).unwrap().len(), 3);

        assert_eq!(size(dir.path()), 9);
        assert_eq!(size(&dir.path().join("b.txt")), 3);
        assert!(exists(&dir.path().join("sub")));
        remove(&dir.path().join("sub"));
        remove(&dir.path().join("b.txt"));
        remove(&dir.path().join("none"));
        assert!(!exists(&dir.path().join("sub")));
        assert!(!exists(&dir.path().join("b.txt")));
        assert!(list(&dir.path().join("none"), false).is_err());
        assert!(default_directory().is_absolute() || default_directory() == Path::new("."));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_for_size_but_open_as_folders() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/f"), "123456").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("gone"), dir.path().join("dangling")).unwrap();
        let entries = list(dir.path(), false).unwrap();
        let alias = entries.iter().find(|e| e.name == "alias").unwrap();
        assert_eq!(alias.kind, EntryKind::Directory);
        let dangling = entries.iter().find(|e| e.name == "dangling").unwrap();
        assert_eq!(dangling.kind, EntryKind::Symlink);
        assert!(exists(&dir.path().join("dangling")));
        // The link's own length, not the folder behind it.
        let links: u64 = ["alias", "dangling"]
            .iter()
            .map(|n| fs::symlink_metadata(dir.path().join(n)).unwrap().len())
            .sum();
        assert_eq!(size(dir.path()), 6 + links);
    }
}
