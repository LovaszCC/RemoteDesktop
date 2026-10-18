//! Local files offered to the server (Swift `RDPClipboard.uriListFor` plus `FreeRDP`'s file helper): the
//! clipboard's files and folders become a `FileGroupDescriptorW` list (folders recursively, in pre-order,
//! names relative to the copied item's parent with `\`), and File Contents Requests are served from the
//! files.
//!
//! A list that can't go to Windows is refused as a whole, naming the culprit: a name Windows can't hold,
//! a link inside a copied folder (it could point anywhere, or loop), a name too long for the descriptor,
//! or more than [`MAX_LOCAL_FILES`](super::MAX_LOCAL_FILES) entries.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rd_freerdp::{ContentsRequest, FILECONTENTS_SIZE};

use super::MAX_LOCAL_FILES;
use super::file_list::{Descriptor, FILETIME_UNIX_EPOCH};

/// Most bytes served for one range request.
pub const MAX_RANGE: u32 = 16 * 1024 * 1024;
/// UTF-16 units a descriptor's name may hold (260 with the terminator).
const MAX_NAME_UNITS: usize = 259;

/// One entry offered to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    descriptor: Descriptor,
}

/// The local clipboard's files, ready to be offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFiles {
    sources: Vec<PathBuf>,
    entries: Vec<Entry>,
}

/// A Windows file name can't hold these; the whole list is refused when one does.
fn is_windows_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c < ' ' || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

fn filetime(time: SystemTime) -> Option<u64> {
    let since = time.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let ticks = since.as_secs().checked_mul(10_000_000)? + u64::from(since.subsec_nanos() / 100);
    ticks.checked_add(FILETIME_UNIX_EPOCH)
}

fn problem(name: &str) -> String {
    format!("{name} cannot be copied to Windows (unsupported name, link or too many files)")
}

struct Scan {
    entries: Vec<Entry>,
}

impl Scan {
    /// Adds `path` named `name` below `folder` (`None` at the top), and a folder's contents.
    fn add(
        &mut self,
        path: &Path,
        name: &str,
        folder: Option<&str>,
        metadata: &std::fs::Metadata,
    ) -> Result<(), String> {
        if !is_windows_safe_name(name) || self.entries.len() >= MAX_LOCAL_FILES {
            return Err(problem(name));
        }
        let wire = folder.map_or_else(|| name.to_owned(), |f| format!("{f}\\{name}"));
        if wire.encode_utf16().count() > MAX_NAME_UNITS {
            return Err(problem(name));
        }
        let is_directory = metadata.is_dir();
        let descriptor = Descriptor {
            name: wire.clone(),
            is_directory,
            size: (!is_directory).then_some(metadata.len()),
            modified: metadata.modified().ok().and_then(filetime),
        };
        self.entries.push(Entry {
            path: path.to_path_buf(),
            descriptor,
        });
        if !is_directory {
            return Ok(());
        }
        let mut children: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("{name}: {e}"))?
            .filter_map(Result::ok)
            .collect();
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let child_name = child.file_name();
            let Some(child_name) = child_name.to_str() else {
                return Err(problem(&child_name.to_string_lossy()));
            };
            let metadata = child.metadata().map_err(|e| format!("{child_name}: {e}"))?;
            if metadata.file_type().is_symlink() {
                return Err(problem(child_name));
            }
            self.add(&child.path(), child_name, Some(&wire), &metadata)?;
        }
        Ok(())
    }
}

impl LocalFiles {
    /// Scans the clipboard's files and folders (`paths`, absolute). Links among `paths` are followed;
    /// links inside folders refuse the list.
    ///
    /// # Errors
    /// A message naming what can't be copied.
    pub fn scan(paths: &[PathBuf]) -> Result<Self, String> {
        let mut scan = Scan { entries: Vec::new() };
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.file_name().and_then(|n| n.to_str()).is_none() {
                return Err(problem(&name));
            }
            let metadata = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?;
            scan.add(path, &name, None, &metadata)?;
        }
        if scan.entries.is_empty() {
            return Err("no files".into());
        }
        Ok(Self {
            sources: paths.to_vec(),
            entries: scan.entries,
        })
    }

    /// The clipboard's paths this list was made of.
    #[must_use]
    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    /// The descriptors, in list order.
    #[must_use]
    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.entries.iter().map(|e| e.descriptor.clone()).collect()
    }

    /// Answers a File Contents Request: the size (8 bytes), or up to [`MAX_RANGE`] bytes from the position
    /// (fewer at the end of the file). Unknown entries, folders and unreadable files get `None` (an error
    /// response).
    #[must_use]
    pub fn serve(&self, request: &ContentsRequest) -> Option<Vec<u8>> {
        let entry = usize::try_from(request.index)
            .ok()
            .and_then(|i| self.entries.get(i))?;
        entry.descriptor.size?;
        if request.flags & FILECONTENTS_SIZE != 0 {
            return match std::fs::metadata(&entry.path) {
                Ok(metadata) if metadata.is_file() => Some(metadata.len().to_le_bytes().to_vec()),
                _ => None,
            };
        }
        read_range(&entry.path, request.position, request.requested.min(MAX_RANGE)).ok()
    }
}

fn read_range(path: &Path, position: u64, length: u32) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(position))?;
    let mut data = Vec::with_capacity(length as usize);
    file.take(u64::from(length)).read_to_end(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    use rd_freerdp::FILECONTENTS_RANGE;

    fn request(index: u32, flags: u32, position: u64, size: u32) -> ContentsRequest {
        ContentsRequest {
            stream_id: 5,
            index,
            flags,
            position,
            requested: size,
            data_id: None,
        }
    }

    #[test]
    fn folders_are_listed_recursively_relative_to_the_copied_item() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("top");
        std::fs::create_dir_all(top.join("sub")).unwrap();
        std::fs::write(top.join("sub/b.txt"), b"bb").unwrap();
        std::fs::write(top.join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("loose.bin"), b"hello").unwrap();
        let files = LocalFiles::scan(&[top.clone(), dir.path().join("loose.bin")]).unwrap();
        let names: Vec<_> = files
            .descriptors()
            .into_iter()
            .map(|d| (d.name, d.size))
            .collect();
        assert_eq!(
            names,
            [
                ("top".to_owned(), None),
                ("top\\a.txt".to_owned(), Some(1)),
                ("top\\sub".to_owned(), None),
                ("top\\sub\\b.txt".to_owned(), Some(2)),
                ("loose.bin".to_owned(), Some(5)),
            ]
        );
        let descriptors = files.descriptors();
        assert!(descriptors[0].is_directory);
        assert!(descriptors[4].modified.is_some_and(|t| t > FILETIME_UNIX_EPOCH));
        assert_eq!(files.sources().len(), 2);
    }

    #[test]
    fn contents_are_served_by_size_and_range() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        std::fs::write(dir.path().join("f.txt"), b"hello world").unwrap();
        let files = LocalFiles::scan(&[dir.path().join("d"), dir.path().join("f.txt")]).unwrap();
        let size = files.serve(&request(1, FILECONTENTS_SIZE, 0, 8));
        assert_eq!(size, Some(11u64.to_le_bytes().to_vec()));
        let range = files.serve(&request(1, FILECONTENTS_RANGE, 6, 100));
        assert_eq!(range.as_deref(), Some(&b"world"[..]));
        assert!(
            files.serve(&request(0, FILECONTENTS_SIZE, 0, 8)).is_none(),
            "a folder"
        );
        assert!(
            files.serve(&request(2, FILECONTENTS_SIZE, 0, 8)).is_none(),
            "no such entry"
        );
        assert!(
            files
                .serve(&request(u32::MAX, FILECONTENTS_RANGE, 0, 1))
                .is_none()
        );
        std::fs::remove_file(dir.path().join("f.txt")).unwrap();
        assert!(
            files.serve(&request(1, FILECONTENTS_RANGE, 0, 1)).is_none(),
            "gone"
        );
    }

    #[test]
    fn missing_or_no_files_refuse_the_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(LocalFiles::scan(&[dir.path().join("missing")]).is_err());
        assert!(LocalFiles::scan(&[]).is_err());
        assert!(!is_windows_safe_name("a?b") && !is_windows_safe_name("tab\t") && !is_windows_safe_name(""));
        assert!(is_windows_safe_name("árvíztűrő tükörfúrógép.txt"));
    }

    // Names Windows can't hold, and paths beyond MAX_PATH, can only be made on Unix.
    #[cfg(unix)]
    #[test]
    fn names_windows_cant_hold_refuse_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("ok");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a?b"), b"").unwrap();
        let error = LocalFiles::scan(&[folder]).unwrap_err();
        assert!(error.starts_with("a?b cannot be copied"), "{error}");

        let long = dir.path().join("x".repeat(200));
        std::fs::create_dir(&long).unwrap();
        std::fs::write(long.join("y".repeat(100)), b"").unwrap();
        assert!(
            LocalFiles::scan(&[long]).is_err(),
            "the descriptor holds 259 units"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_inside_folders_refuse_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("f");
        std::fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink("/", folder.join("root")).unwrap();
        let error = LocalFiles::scan(&[folder]).unwrap_err();
        assert!(error.starts_with("root cannot be copied"), "{error}");
        // A link copied itself is followed.
        std::fs::write(dir.path().join("target.txt"), b"t").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).unwrap();
        let files = LocalFiles::scan(&[dir.path().join("link.txt")]).unwrap();
        assert_eq!(files.descriptors()[0].size, Some(1));
    }
}
