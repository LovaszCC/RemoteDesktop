//! Strict, server-distrusting parsing of remote file lists (Swift `RDPClipboardFileList`). Names come from
//! the remote machine, so a hostile server must not be able to write outside the download folder
//! (`..\..\x`, `C:\x`, `\\srv\x` …). The whole list fails on one bad entry.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use unicode_normalization::UnicodeNormalization;

/// Size of one FILEDESCRIPTORW ([MS-RDPECLIP] 2.2.5.2.3.1), little endian.
pub const DESCRIPTOR_SIZE: usize = 592;

const OFFSET_FLAGS: usize = 0;
const OFFSET_ATTRIBUTES: usize = 36;
// clsid 4, sizel 20, pointl 28, creation 40, access 48
const OFFSET_WRITE_TIME: usize = 56;
const OFFSET_SIZE_HIGH: usize = 64;
const OFFSET_SIZE_LOW: usize = 68;
const OFFSET_NAME: usize = 72;
const NAME_UNITS: usize = 260;
const FD_ATTRIBUTES: u32 = 0x04;
const FD_WRITESTIME: u32 = 0x20;
const FD_FILESIZE: u32 = 0x40;
const FD_SHOWPROGRESSUI: u32 = 0x4000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
const MAX_COMPONENT_BYTES: usize = 255;
const MAX_DEPTH: usize = 64;
const MAX_PATH_BYTES: usize = 1024;

/// FILETIME (100 ns since 1601-01-01) of the Unix epoch.
pub const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// One entry of a file list the server put on the clipboard ([MS-RDPECLIP] `CLIPRDR_FILELIST`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    /// Safe relative POSIX path (`dir/file.txt`); never absolute, never containing `.` or `..`.
    pub path: String,
    /// A folder.
    pub is_directory: bool,
    /// The size is known (otherwise it is asked with `FILECONTENTS_SIZE`).
    pub has_size: bool,
    /// The size in bytes (0 for folders).
    pub size: u64,
    /// The last write time, when the server sent one.
    pub modified: Option<SystemTime>,
}

/// A `FILECONTENTS_RANGE` request: the 64-bit position split in two, and the byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRange {
    /// Low 32 bits of the position.
    pub position_low: u32,
    /// High 32 bits of the position.
    pub position_high: u32,
    /// Bytes requested; 0 when the file is done.
    pub length: u32,
}

impl FileRange {
    /// The 64-bit position.
    #[must_use]
    pub fn position(&self) -> u64 {
        u64::from(self.position_high) << 32 | u64::from(self.position_low)
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from(read_u32(bytes, offset)) | u64::from(read_u32(bytes, offset + 4)) << 32
}

/// Rejects lone surrogates and C0 / DEL controls.
fn units_are_valid(units: &[u16]) -> bool {
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit < 0x20 || unit == 0x7F {
            return false;
        }
        if (0xD800..0xDC00).contains(&unit) {
            if !units
                .get(i + 1)
                .is_some_and(|next| (0xDC00..0xE000).contains(next))
            {
                return false;
            }
            i += 1;
        } else if (0xDC00..0xE000).contains(&unit) {
            return false;
        }
        i += 1;
    }
    true
}

/// The relative POSIX path for a Windows name given as UTF-16 units, or `None` when it is unsafe or
/// invalid.
#[must_use]
pub fn sanitized_path_units(name: &[u16]) -> Option<String> {
    if name.is_empty() || !units_are_valid(name) {
        return None;
    }
    let name = String::from_utf16(name).ok()?;
    if name.starts_with(['\\', '/']) {
        return None; // absolute or UNC
    }
    if name.contains(':') {
        return None; // drive letter or NTFS alternate data stream
    }
    let parts: Vec<&str> = name.split(['\\', '/']).collect();
    if parts.len() > MAX_DEPTH {
        return None;
    }
    let mut components = Vec::with_capacity(parts.len());
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        // Windows ignores trailing spaces and dots ("x. " is "x"); "..." would collapse to nothing.
        let component = part.trim_end_matches([' ', '.']);
        if component.is_empty() || component.len() > MAX_COMPONENT_BYTES {
            return None;
        }
        components.push(component);
    }
    let path = components.join("/");
    (path.len() <= MAX_PATH_BYTES).then_some(path)
}

/// [`sanitized_path_units`] for a string.
#[must_use]
pub fn sanitized_path(name: &str) -> Option<String> {
    sanitized_path_units(&name.encode_utf16().collect::<Vec<_>>())
}

/// Case- and normalization-folded key: APFS and NTFS are usually case-insensitive and APFS is
/// normalization-insensitive.
fn fold(path: &str) -> String {
    path.to_lowercase().nfc().collect()
}

/// Parses `cItems` + descriptors. Fails as a whole on any bad or duplicate entry, a truncated buffer,
/// more than `max_entries` entries, or an entry below a file.
///
/// # Errors
/// A message naming the problem (and the entry, if one is at fault).
pub fn parse(data: &[u8], max_entries: usize) -> Result<Vec<RemoteFile>, String> {
    if data.len() < 4 {
        return Err("truncated file list".into());
    }
    let count = read_u32(data, 0) as usize;
    if count == 0 {
        return Err("empty file list".into());
    }
    if count > max_entries {
        return Err("too many files".into());
    }
    if (data.len() - 4) / DESCRIPTOR_SIZE < count {
        return Err("truncated file list".into());
    }

    let mut files = Vec::with_capacity(count);
    // Folded path → is directory.
    let mut seen: HashMap<String, bool> = HashMap::with_capacity(count);
    for index in 0..count {
        let d = &data[4 + index * DESCRIPTOR_SIZE..4 + (index + 1) * DESCRIPTOR_SIZE];
        let flags = read_u32(d, OFFSET_FLAGS);

        // Raw code units (no decoder that might turn a lone surrogate into U+FFFD unnoticed).
        let units: Vec<u16> = (0..NAME_UNITS)
            .map(|i| u16::from_le_bytes([d[OFFSET_NAME + i * 2], d[OFFSET_NAME + i * 2 + 1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        if units.len() == NAME_UNITS {
            return Err("file name not terminated".into());
        }
        let Some(path) = sanitized_path_units(&units) else {
            return Err(format!("unsafe file name: {}", String::from_utf16_lossy(&units)));
        };

        let key = fold(&path);
        if seen.contains_key(&key) {
            return Err(format!("duplicate file name: {path}"));
        }
        let mut parent = key.as_str();
        while let Some((up, _)) = parent.rsplit_once('/') {
            if seen.get(up) == Some(&false) {
                return Err(format!("file inside a file: {path}"));
            }
            parent = up;
        }

        let is_directory =
            flags & FD_ATTRIBUTES != 0 && read_u32(d, OFFSET_ATTRIBUTES) & FILE_ATTRIBUTE_DIRECTORY != 0;
        // Explorer sometimes omits FD_FILESIZE; then the size is asked with FILECONTENTS_SIZE.
        let has_size = is_directory || flags & FD_FILESIZE != 0;
        let size = if is_directory {
            0
        } else {
            u64::from(read_u32(d, OFFSET_SIZE_HIGH)) << 32 | u64::from(read_u32(d, OFFSET_SIZE_LOW))
        };
        let filetime = read_u64(d, OFFSET_WRITE_TIME);
        let modified = (flags & FD_WRITESTIME != 0 && filetime > FILETIME_UNIX_EPOCH).then(|| {
            let ticks = filetime - FILETIME_UNIX_EPOCH;
            SystemTime::UNIX_EPOCH
                + Duration::from_secs(ticks / 10_000_000)
                + Duration::from_nanos(ticks % 10_000_000 * 100)
        });

        seen.insert(key, is_directory);
        files.push(RemoteFile {
            path,
            is_directory,
            has_size,
            size,
            modified,
        });
    }
    Ok(files)
}

/// One entry of a file list offered to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    /// The name on the wire: the path below the copied item's parent, with `\`.
    pub name: String,
    /// A folder.
    pub is_directory: bool,
    /// The size (files).
    pub size: Option<u64>,
    /// The last write time (FILETIME).
    pub modified: Option<u64>,
}

/// A `CLIPRDR_FILELIST` ([MS-RDPECLIP] 2.2.5.2.3): the count, then one FILEDESCRIPTORW each. Names are
/// cut at 259 UTF-16 units (callers keep them shorter).
#[must_use]
pub fn encode(descriptors: &[Descriptor]) -> Vec<u8> {
    let count = u32::try_from(descriptors.len()).unwrap_or(u32::MAX);
    let mut data = Vec::with_capacity(4 + descriptors.len() * DESCRIPTOR_SIZE);
    data.extend_from_slice(&count.to_le_bytes());
    for descriptor in descriptors {
        let mut d = [0u8; DESCRIPTOR_SIZE];
        let mut flags = FD_ATTRIBUTES | FD_SHOWPROGRESSUI;
        if descriptor.size.is_some() {
            flags |= FD_FILESIZE;
        }
        if descriptor.modified.is_some() {
            flags |= FD_WRITESTIME;
        }
        let attributes = if descriptor.is_directory {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_ARCHIVE
        };
        let size = descriptor.size.unwrap_or(0);
        d[OFFSET_FLAGS..OFFSET_FLAGS + 4].copy_from_slice(&flags.to_le_bytes());
        d[OFFSET_ATTRIBUTES..OFFSET_ATTRIBUTES + 4].copy_from_slice(&attributes.to_le_bytes());
        d[OFFSET_WRITE_TIME..OFFSET_WRITE_TIME + 8]
            .copy_from_slice(&descriptor.modified.unwrap_or(0).to_le_bytes());
        d[OFFSET_SIZE_HIGH..OFFSET_SIZE_HIGH + 4].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
        d[OFFSET_SIZE_LOW..OFFSET_SIZE_LOW + 4].copy_from_slice(&((size & 0xFFFF_FFFF) as u32).to_le_bytes());
        for (i, unit) in descriptor.name.encode_utf16().take(NAME_UNITS - 1).enumerate() {
            d[OFFSET_NAME + i * 2..OFFSET_NAME + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&d);
    }
    data
}

/// A Windows device name (`CON`, `NUL`, `COM1` …, any case), which can't be a file name there.
#[must_use]
pub fn is_windows_device_name(name: &str) -> bool {
    let upper = name.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(number) = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")) else {
        return false;
    };
    matches!(
        number,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}

/// Indices of entries without a parent folder: one clipboard item each.
#[must_use]
pub fn top_level_indices(files: &[RemoteFile]) -> Vec<usize> {
    files
        .iter()
        .enumerate()
        .filter(|(_, file)| !file.path.contains('/'))
        .map(|(index, _)| index)
        .collect()
}

/// The next chunk of a file of `size` bytes from `offset`; length 0 when done.
#[must_use]
pub fn range_for(size: u64, offset: u64, chunk: u32) -> FileRange {
    let length = if offset < size {
        u32::try_from((size - offset).min(u64::from(chunk))).unwrap_or(chunk)
    } else {
        0
    };
    FileRange {
        position_low: (offset & 0xFFFF_FFFF) as u32,
        position_high: (offset >> 32) as u32,
        length,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One entry for [`build`]: the way a server sends it.
    pub struct Entry {
        pub units: Vec<u16>,
        pub directory: bool,
        pub size: Option<u64>,
        pub modified: Option<u64>,
    }

    impl Entry {
        pub fn new(name: &str) -> Self {
            Self {
                units: name.encode_utf16().collect(),
                directory: false,
                size: Some(0),
                modified: None,
            }
        }
        pub fn units(units: &[u16]) -> Self {
            Self {
                units: units.to_vec(),
                ..Self::new("")
            }
        }
        pub fn directory(mut self) -> Self {
            self.directory = true;
            self
        }
        pub fn size(mut self, size: Option<u64>) -> Self {
            self.size = size;
            self
        }
        pub fn modified(mut self, filetime: u64) -> Self {
            self.modified = Some(filetime);
            self
        }
    }

    /// `CLIPRDR_FILELIST` bytes (Swift `FileListBuilder`).
    pub fn build(entries: &[Entry], count: Option<u32>) -> Vec<u8> {
        let mut data = count
            .unwrap_or(u32::try_from(entries.len()).unwrap())
            .to_le_bytes()
            .to_vec();
        for entry in entries {
            let mut d = vec![0u8; DESCRIPTOR_SIZE];
            let mut flags: u32 = 0x04; // FD_ATTRIBUTES
            if entry.size.is_some() {
                flags |= 0x40;
            }
            if entry.modified.is_some() {
                flags |= 0x20;
            }
            let size = entry.size.unwrap_or(0);
            d[0..4].copy_from_slice(&flags.to_le_bytes());
            d[36..40].copy_from_slice(&(if entry.directory { 0x10u32 } else { 0x80 }).to_le_bytes());
            d[56..64].copy_from_slice(&entry.modified.unwrap_or(0).to_le_bytes());
            d[64..68].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
            d[68..72].copy_from_slice(&((size & 0xFFFF_FFFF) as u32).to_le_bytes());
            for (i, unit) in entry.units.iter().take(260).enumerate() {
                d[72 + i * 2..74 + i * 2].copy_from_slice(&unit.to_le_bytes());
            }
            data.extend_from_slice(&d);
        }
        data
    }

    #[test]
    fn offered_lists_parse_back() {
        let descriptors = [
            Descriptor {
                name: "top".into(),
                is_directory: true,
                size: None,
                modified: Some(FILETIME_UNIX_EPOCH + 10_000_000),
            },
            Descriptor {
                name: "top\\árvíz.txt".into(),
                is_directory: false,
                size: Some(5_000_000_000),
                modified: None,
            },
        ];
        let data = encode(&descriptors);
        assert_eq!(data.len(), 4 + 2 * DESCRIPTOR_SIZE);
        let files = parse(&data, 10).unwrap();
        assert_eq!(files[0].path, "top");
        assert!(files[0].is_directory);
        assert_eq!(
            files[0].modified,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
        );
        assert_eq!(files[1].path, "top/árvíz.txt");
        assert_eq!((files[1].has_size, files[1].size), (true, 5_000_000_000));
    }

    #[test]
    fn device_names_are_recognised() {
        for name in ["CON", "nul", "Com1", "LPT9", "COM²", "conin$"] {
            assert!(is_windows_device_name(name), "{name}");
        }
        for name in ["COM0", "COM10", "CONSOLE", "file", ""] {
            assert!(!is_windows_device_name(name), "{name}");
        }
    }

    #[test]
    fn unsafe_names_are_rejected() {
        for name in [
            "..\\..\\x",
            "a\\..\\..\\b",
            "\\abs",
            "\\\\srv\\share\\x",
            "/etc/passwd",
            "C:\\x",
            "a:stream",
            ".\\a",
            "a\\.\\b",
            "..",
            ". .",
            "...",
            "a\\\\b",
            "a\\",
            "tab\there",
            "nul\u{7F}",
        ] {
            assert_eq!(sanitized_path(name), None, "{name:?}");
        }
    }

    #[test]
    fn names_become_relative_posix_paths() {
        assert_eq!(
            sanitized_path("dir\\sub\\file.txt").as_deref(),
            Some("dir/sub/file.txt")
        );
        assert_eq!(
            sanitized_path("árvíztűrő tükörfúrógép.txt").as_deref(),
            Some("árvíztűrő tükörfúrógép.txt")
        );
        assert_eq!(sanitized_path("😀 漢字").as_deref(), Some("😀 漢字"));
        // Windows ignores trailing dots and spaces.
        assert_eq!(sanitized_path("name. \\x.").as_deref(), Some("name/x"));
    }

    #[test]
    fn long_components_and_deep_paths_are_rejected() {
        assert_eq!(sanitized_path(&"a".repeat(256)), None);
        assert!(sanitized_path(&"a".repeat(255)).is_some());
        let deep = vec!["d"; 65].join("\\");
        assert_eq!(sanitized_path(&deep), None);
    }

    #[test]
    fn lone_surrogate_is_rejected() {
        let data = build(&[Entry::units(&[0x61, 0xD800, 0x62])], None);
        assert!(parse(&data, 10).is_err());
    }

    #[test]
    #[allow(clippy::duration_suboptimal_units)] // the Swift test's Unix time
    fn parses_directories_sizes_and_times() {
        // 2024-01-01T00:00:00Z as FILETIME.
        let filetime: u64 = FILETIME_UNIX_EPOCH + 1_704_067_200 * 10_000_000;
        let data = build(
            &[
                Entry::new("dir").directory(),
                Entry::new("dir\\big.bin")
                    .size(Some(5 << 32 | 7))
                    .modified(filetime),
                Entry::new("loose.txt").size(None),
            ],
            None,
        );
        let files = parse(&data, 10).unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["dir", "dir/big.bin", "loose.txt"]
        );
        assert!(files[0].is_directory);
        assert_eq!(files[1].size, 5 << 32 | 7);
        assert_eq!(
            files[1].modified,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_704_067_200))
        );
        assert!(!files[2].has_size);
        assert_eq!(top_level_indices(&files), [0, 2]);
    }

    #[test]
    fn whole_list_fails_on_one_bad_entry() {
        let data = build(&[Entry::new("ok.txt"), Entry::new("..\\evil.txt")], None);
        let error = parse(&data, 10).unwrap_err();
        assert!(error.contains("evil"), "{error}");
    }

    #[test]
    fn duplicates_ignoring_case_are_rejected() {
        let data = build(&[Entry::new("Readme.TXT"), Entry::new("readme.txt")], None);
        assert!(parse(&data, 10).is_err());
    }

    #[test]
    fn entries_below_a_file_are_rejected() {
        let data = build(&[Entry::new("file"), Entry::new("file\\x")], None);
        assert!(parse(&data, 10).is_err());
    }

    #[test]
    fn truncated_or_oversized_lists_are_rejected() {
        let full = build(&[Entry::new("a"), Entry::new("b")], None);
        assert!(parse(&full[..full.len() - 1], 10).is_err());
        assert!(parse(&build(&[Entry::new("a")], Some(3)), 10).is_err());
        assert!(parse(&full, 1).is_err());
        assert!(parse(&[0, 0, 0, 0], 10).is_err());
        assert!(parse(&[1, 0], 10).is_err());
    }

    #[test]
    fn unterminated_name_is_rejected() {
        let data = build(&[Entry::new(&"x".repeat(260))], None);
        assert!(parse(&data, 10).is_err());
    }

    #[test]
    fn ranges_split_sixty_four_bit_offsets() {
        let size: u64 = (5 << 32) + 10;
        let range = range_for(size, 5 << 32, 4);
        assert!(range.position_high == 5 && range.position_low == 0 && range.length == 4);
        let last = range_for(size, (5 << 32) + 8, 4);
        assert_eq!(last.length, 2);
        assert_eq!(range_for(size, size, 4).length, 0);
    }
}
