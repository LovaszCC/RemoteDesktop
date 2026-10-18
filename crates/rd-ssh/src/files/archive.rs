//! The local ends of a tar stream: [`unpack`] for downloads, [`pack`] for uploads.
//!
//! Unpacking takes only what was asked for and never writes outside the destination:
//! - every path must be relative, without `..`, and start with the requested item's name;
//! - files are written as new files (an existing file or link is unlinked first, never written through);
//! - parents are checked by canonicalization, so a symlink (in the archive or already on disk) can't
//!   redirect a later entry outside the destination;
//! - devices, pipes and other special entries are skipped, and so are symlinks on Windows;
//! - folder permissions are applied at the end, so a read-only folder can still be filled.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use tar::EntryType;

fn unsafe_entry(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Unsafe path in the archive: {}", path.display()),
    )
}

/// The entry's path relative to the destination: relative, no `..`, first component `top`.
///
/// # Errors
/// `InvalidData` for anything else.
pub fn checked_path(path: &Path, top: &str) -> io::Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(unsafe_entry(path));
            }
        }
    }
    match relative.components().next() {
        Some(Component::Normal(first)) if first == top => Ok(relative),
        _ => Err(unsafe_entry(path)),
    }
}

/// Creates `target` (a folder) after checking that its closest existing ancestor is inside
/// `destination`; an existing non-folder (also a symlink to a folder) is an error.
fn create_folder(destination: &Path, canonical_destination: &Path, target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(m) if m.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a folder", target.display()),
            ));
        }
        Err(_) => {}
    }
    let mut ancestor = target;
    while fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor.parent().unwrap_or(destination);
    }
    if !ancestor.canonicalize()?.starts_with(canonical_destination) {
        return Err(unsafe_entry(target));
    }
    fs::create_dir_all(target)
}

/// Unpacks a tar stream holding `top` (a file or folder) into `destination`.
///
/// # Errors
/// I/O errors, a malformed archive, or an unsafe entry (see the module docs).
pub fn unpack<R: Read>(reader: R, destination: &Path, top: &str) -> io::Result<()> {
    let canonical_destination = destination.canonicalize()?;
    let mut archive = tar::Archive::new(reader);
    archive.set_overwrite(true);
    archive.set_preserve_mtime(true);
    archive.set_preserve_permissions(false);
    archive.set_preserve_ownerships(false);
    archive.set_unpack_xattrs(false);
    let mut folder_modes: Vec<(PathBuf, u32)> = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let relative = checked_path(&path, top)?;
        let kind = entry.header().entry_type();
        match kind {
            EntryType::Directory => {
                let target = destination.join(&relative);
                create_folder(destination, &canonical_destination, &target)?;
                if let Ok(mode) = entry.header().mode() {
                    folder_modes.push((target, mode));
                }
                continue;
            }
            EntryType::Symlink if cfg!(windows) => continue,
            EntryType::Link => {
                let link = entry.link_name()?.ok_or_else(|| unsafe_entry(&path))?;
                checked_path(&link, top)?;
            }
            EntryType::Regular | EntryType::Continuous | EntryType::Symlink => {}
            EntryType::GNULongName
            | EntryType::GNULongLink
            | EntryType::XHeader
            | EntryType::XGlobalHeader => {
                continue;
            }
            _ => continue,
        }
        if !entry.unpack_in(destination)? {
            return Err(unsafe_entry(&path));
        }
    }
    // Deepest first, so a read-only parent doesn't stop its children's changes.
    folder_modes.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in folder_modes {
        set_folder_mode(&path, mode);
    }
    Ok(())
}

#[cfg(unix)]
fn set_folder_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // Owner rwx stays, so the copy can always be removed again.
    let _ = fs::set_permissions(path, fs::Permissions::from_mode((mode & 0o777) | 0o700));
}

#[cfg(not(unix))]
fn set_folder_mode(_path: &Path, _mode: u32) {}

/// Writes `source_directory/name` (a file, folder or symlink; links are stored, not followed) as a tar
/// stream into `writer` and returns the writer.
///
/// # Errors
/// I/O errors reading the source or writing the stream.
pub fn pack<W: Write>(writer: W, source_directory: &Path, name: &str) -> io::Result<W> {
    let source = source_directory.join(name);
    let metadata = fs::symlink_metadata(&source)?;
    let mut builder = tar::Builder::new(writer);
    builder.follow_symlinks(false);
    if metadata.is_dir() {
        builder.append_dir_all(name, &source)?;
    } else {
        builder.append_path_with_name(&source, name)?;
    }
    builder.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tar with one entry whose header path is written raw (the builder refuses unsafe paths).
    fn raw_tar(entries: &[(&str, EntryType, &[u8], Option<&str>)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, kind, data, link) in entries {
            let mut header = tar::Header::new_gnu();
            let name = &mut header.as_old_mut().name;
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_entry_type(*kind);
            header.set_size(data.len() as u64);
            header.set_mode(if *kind == EntryType::Directory {
                0o755
            } else {
                0o644
            });
            if let Some(link) = link {
                let field = &mut header.as_old_mut().linkname;
                field[..link.len()].copy_from_slice(link.as_bytes());
            }
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    #[test]
    fn checked_paths() {
        assert_eq!(
            checked_path(Path::new("./d/x"), "d").unwrap(),
            PathBuf::from("d/x")
        );
        assert_eq!(checked_path(Path::new("d"), "d").unwrap(), PathBuf::from("d"));
        for bad in ["/etc/passwd", "d/../../x", "../d", "other/x", ".", ""] {
            assert!(checked_path(Path::new(bad), "d").is_err(), "{bad}");
        }
    }

    #[test]
    fn round_trip_of_a_folder_and_a_file() {
        let source = tempfile::tempdir().unwrap();
        let tree = source.path().join("site dir");
        fs::create_dir_all(tree.join("inner/deep")).unwrap();
        fs::write(tree.join("inner/it's 1.txt"), "one").unwrap();
        fs::write(tree.join("inner/deep/f.txt"), "deep").unwrap();
        fs::write(source.path().join("single.txt"), "single").unwrap();

        let stream = pack(Vec::new(), source.path(), "site dir").unwrap();
        let destination = tempfile::tempdir().unwrap();
        unpack(stream.as_slice(), destination.path(), "site dir").unwrap();
        let copy = destination.path().join("site dir");
        assert_eq!(fs::read_to_string(copy.join("inner/it's 1.txt")).unwrap(), "one");
        assert_eq!(fs::read_to_string(copy.join("inner/deep/f.txt")).unwrap(), "deep");

        let stream = pack(Vec::new(), source.path(), "single.txt").unwrap();
        unpack(stream.as_slice(), destination.path(), "single.txt").unwrap();
        assert_eq!(
            fs::read_to_string(destination.path().join("single.txt")).unwrap(),
            "single"
        );

        // Replace: an existing file is overwritten, other files in a merged folder stay.
        fs::write(copy.join("inner/extra"), "stays").unwrap();
        fs::write(source.path().join("site dir/inner/it's 1.txt"), "changed").unwrap();
        let stream = pack(Vec::new(), source.path(), "site dir").unwrap();
        unpack(stream.as_slice(), destination.path(), "site dir").unwrap();
        assert_eq!(
            fs::read_to_string(copy.join("inner/it's 1.txt")).unwrap(),
            "changed"
        );
        assert_eq!(fs::read_to_string(copy.join("inner/extra")).unwrap(), "stays");
    }

    #[test]
    fn refuses_traversal_absolute_and_foreign_entries() {
        let destination = tempfile::tempdir().unwrap();
        let inner = destination.path().join("inner");
        fs::create_dir(&inner).unwrap();
        for path in ["d/../../escape", "/abs", "other.txt"] {
            let tar = raw_tar(&[(path, EntryType::Regular, b"x", None)]);
            let error = unpack(tar.as_slice(), &inner, "d").unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{path}");
        }
        assert!(!destination.path().join("escape").exists());
        assert_eq!(fs::read_dir(&inner).unwrap().count(), 0);
        // A hard link to something outside.
        let tar = raw_tar(&[("d", EntryType::Link, b"", Some("../outside"))]);
        assert!(unpack(tar.as_slice(), &inner, "d").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn never_writes_through_symlinks() {
        let destination = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("victim");
        fs::write(&target, "original").unwrap();

        // A symlink in the archive, then a file through it.
        let link_target = outside.path().to_str().unwrap();
        let tar = raw_tar(&[
            ("d", EntryType::Directory, b"", None),
            ("d/link", EntryType::Symlink, b"", Some(link_target)),
            ("d/link/victim", EntryType::Regular, b"evil", None),
        ]);
        assert!(unpack(tar.as_slice(), destination.path(), "d").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");

        // A symlink to a file, then a regular file of the same name: the link is replaced, not followed.
        let tar = raw_tar(&[
            ("e", EntryType::Symlink, b"", Some(target.to_str().unwrap())),
            ("e", EntryType::Regular, b"new", None),
        ]);
        unpack(tar.as_slice(), destination.path(), "e").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(fs::read_to_string(destination.path().join("e")).unwrap(), "new");
        assert!(
            !fs::symlink_metadata(destination.path().join("e"))
                .unwrap()
                .is_symlink()
        );

        // An existing local symlink named like the item doesn't redirect the folder.
        std::os::unix::fs::symlink(outside.path(), destination.path().join("f")).unwrap();
        let tar = raw_tar(&[("f/victim", EntryType::Regular, b"evil", None)]);
        assert!(unpack(tar.as_slice(), destination.path(), "f").is_err());
        let tar = raw_tar(&[("f", EntryType::Directory, b"", None)]);
        assert!(unpack(tar.as_slice(), destination.path(), "f").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn read_only_folders_are_filled_and_specials_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let destination = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut dir = tar::Header::new_gnu();
        dir.set_entry_type(EntryType::Directory);
        dir.set_mode(0o555);
        dir.set_size(0);
        builder.append_data(&mut dir, "ro", io::empty()).unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_mode(0o644);
        file.set_size(2);
        builder.append_data(&mut file, "ro/f", &b"ok"[..]).unwrap();
        let mut fifo = tar::Header::new_gnu();
        fifo.set_entry_type(EntryType::Fifo);
        fifo.set_size(0);
        builder.append_data(&mut fifo, "ro/pipe", io::empty()).unwrap();
        let tar = builder.into_inner().unwrap();
        unpack(tar.as_slice(), destination.path(), "ro").unwrap();
        let ro = destination.path().join("ro");
        assert_eq!(fs::read_to_string(ro.join("f")).unwrap(), "ok");
        assert!(!ro.join("pipe").exists());
        assert_eq!(fs::metadata(&ro).unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn garbage_is_an_error() {
        let destination = tempfile::tempdir().unwrap();
        assert!(unpack(&b"not a tar archive at all"[..], destination.path(), "x").is_err());
        assert!(pack(Vec::new(), destination.path(), "missing").is_err());
    }
}
