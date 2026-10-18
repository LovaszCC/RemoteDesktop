//! One remote directory's listing (Swift `RemoteListing`): GNU `find -printf` where available (exact,
//! any file name), else `LC_ALL=C ls -lnA` (BSD, busybox). The script prints the absolute path, a format
//! marker, then the entries.

use std::cmp::Ordering;

use chrono::{Datelike, Local, TimeZone};
use serde::Serialize;

use super::path::cd_command;

/// What a remote (or local) entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link (not followed).
    Symlink,
    /// Devices, sockets, pipes.
    Other,
}

/// One entry of a listing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The file name (any bytes but `/` and NUL; invalid UTF-8 is replaced).
    pub name: String,
    /// File, directory, symlink or other.
    pub kind: EntryKind,
    /// Size in bytes (the link itself for a symlink).
    pub size: u64,
    /// Modification time in Unix seconds, when known.
    pub modified: Option<i64>,
    /// `drwxr-xr-x` style.
    pub permissions: String,
}

impl Entry {
    /// Whether it is a directory.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}

/// The output wasn't a listing (the folder couldn't be opened or the shell printed something else).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("The folder listing can't be read.")]
pub struct Unreadable;

const FIND_MARKER: &str = "RD-FIND";
const LS_MARKER: &str = "RD-LS";

/// The listing script for `path` (`~` and `~/x` go through `$HOME`).
#[must_use]
pub fn script(path: &str) -> String {
    format!(
        "{} || exit 2\n\
         pwd\n\
         if find . -maxdepth 0 -printf '' >/dev/null 2>&1; then\n  \
         echo {FIND_MARKER}\n  \
         find . -mindepth 1 -maxdepth 1 -printf '%y\\t%s\\t%T@\\t%M\\0%P\\0'\n\
         else\n  \
         echo {LS_MARKER}\n  \
         LC_ALL=C ls -lnA\n\
         fi\n",
        cd_command(path)
    )
}

/// A parsed listing: the absolute path and the entries (directories first, then by name).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listing {
    /// The directory's absolute path (`pwd`).
    pub path: String,
    /// Its entries.
    pub entries: Vec<Entry>,
}

/// Parses the script's output; `now` decides the year of recent `ls` dates.
///
/// # Errors
/// [`Unreadable`] when the output has no absolute path or no known marker.
pub fn parse(data: &[u8], now: i64) -> Result<Listing, Unreadable> {
    let text = String::from_utf8_lossy(data);
    let mut lines = text.splitn(3, '\n');
    let path = lines.next().ok_or(Unreadable)?;
    let marker = lines.next().ok_or(Unreadable)?;
    if !path.starts_with('/') {
        return Err(Unreadable);
    }
    let body = lines.next().unwrap_or("");
    let mut entries = match marker {
        FIND_MARKER => parse_find(body),
        LS_MARKER => parse_ls(body, now),
        _ => return Err(Unreadable),
    };
    sort_entries(&mut entries);
    Ok(Listing {
        path: path.to_owned(),
        entries,
    })
}

/// Directories first, then names in Finder order.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_directory()
            .cmp(&a.is_directory())
            .then_with(|| standard_compare(&a.name, &b.name))
    });
}

/// Finder-like name order (Swift `localizedStandardCompare`): case and accents ignored, digit runs
/// compared as numbers; ties fall back to the raw names.
#[must_use]
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (ka, kb) = (sort_key(a), sort_key(b));
    ka.cmp(&kb).then_with(|| a.cmp(b))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Token {
    /// A digit run: compared by length after trimming leading zeros, then digit by digit.
    Number(usize, String),
    Char(char),
}

fn sort_key(name: &str) -> Vec<Token> {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    let folded: String = name
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect();
    let mut tokens = Vec::new();
    let mut chars = folded.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut digits = String::from(c);
            while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                digits.push(d);
                chars.next();
            }
            let trimmed = digits.trim_start_matches('0').to_owned();
            tokens.push(Token::Number(trimmed.len(), trimmed));
        } else {
            tokens.push(Token::Char(c));
        }
    }
    tokens
}

/// Records of `type\tsize\tmtime\tmode` NUL `name` NUL.
fn parse_find(body: &str) -> Vec<Entry> {
    let fields: Vec<&str> = body.split('\0').collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index + 1 < fields.len() {
        let meta: Vec<&str> = fields[index].split('\t').filter(|s| !s.is_empty()).collect();
        let name = fields[index + 1];
        index += 2;
        if meta.len() != 4 || name.is_empty() {
            continue;
        }
        entries.push(Entry {
            name: name.to_owned(),
            kind: kind_of(meta[0].chars().next().unwrap_or('?'), 'f'),
            size: meta[1].parse().unwrap_or(0),
            // Whole seconds, as `Date(timeIntervalSince1970:)` truncated for display.
            #[allow(clippy::cast_possible_truncation)]
            modified: meta[2].parse::<f64>().ok().map(|s| s.floor() as i64),
            permissions: meta[3].to_owned(),
        });
    }
    entries
}

/// `d`, `l` and the regular-file letter (`f` for find, `-` for ls) map to kinds; anything else is other.
fn kind_of(letter: char, file: char) -> EntryKind {
    match letter {
        'd' => EntryKind::Directory,
        'l' => EntryKind::Symlink,
        c if c == file => EntryKind::File,
        _ => EntryKind::Other,
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `ls -lnA` lines: mode, links, uid, gid, size, month, day, time-or-year, name (`name -> target` for
/// links). Dates are local time; `HH:MM` dates more than a day in the future belong to last year.
fn parse_ls(body: &str, now: i64) -> Vec<Entry> {
    let current_year = Local.timestamp_opt(now, 0).single().map_or(1970, |d| d.year());
    body.split('\n')
        .filter_map(|line| parse_ls_line(line, now, current_year))
        .collect()
}

fn parse_ls_line(line: &str, now: i64, current_year: i32) -> Option<Entry> {
    let mut rest = line;
    let mut fields: Vec<&str> = Vec::with_capacity(8);
    while fields.len() < 8 {
        rest = rest.trim_start_matches(' ');
        let end = rest.find(' ')?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let mut name = rest.strip_prefix(' ').unwrap_or(rest);
    let mode = fields[0];
    let letter = mode.chars().next()?;
    if name.is_empty() {
        return None;
    }
    let kind = kind_of(letter, '-');
    if kind == EntryKind::Symlink
        && let Some(arrow) = name.find(" -> ")
    {
        name = &name[..arrow];
    }
    let modified = MONTHS
        .iter()
        .position(|m| *m == fields[5])
        .zip(fields[6].parse::<u32>().ok())
        .and_then(|(month, day)| ls_date(u32::try_from(month).ok()? + 1, day, fields[7], now, current_year));
    Some(Entry {
        name: name.to_owned(),
        kind,
        size: fields[4].parse().unwrap_or(0),
        modified,
        permissions: mode.chars().take(10).collect(),
    })
}

fn ls_date(month: u32, day: u32, time_or_year: &str, now: i64, current_year: i32) -> Option<i64> {
    let local = |year: i32, hour: u32, minute: u32| {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .earliest()
            .map(|d| d.timestamp())
    };
    if let Some((hour, minute)) = time_or_year.split_once(':') {
        let (hour, minute) = (hour.parse().ok()?, minute.parse().unwrap_or(0));
        let this_year = local(current_year, hour, minute)?;
        if this_year > now + 86_400 {
            local(current_year - 1, hour, minute)
        } else {
            Some(this_year)
        }
    } else {
        local(time_or_year.parse().ok()?, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn year(timestamp: i64) -> i32 {
        Local.timestamp_opt(timestamp, 0).unwrap().year()
    }

    fn at(iso: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(iso).unwrap().timestamp()
    }

    fn names(listing: &Listing) -> Vec<&str> {
        listing.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn parses_find_records_with_any_name() {
        let data = [
            "/data\n",
            "RD-FIND\n",
            "d\t4096\t1760000000.5\tdrwxr-xr-x\0sub dir\0",
            "f\t6\t1760000100.0\t-rw-r--r--\0a\tfile with tab.txt\0",
            "l\t10\t1760000200.0\tlrwxrwxrwx\0link\0",
            "f\t1\t1760000300.0\t-rw-------\0ékezet\0",
        ]
        .concat();
        let listing = parse(data.as_bytes(), 0).unwrap();
        assert_eq!(listing.path, "/data");
        assert_eq!(
            names(&listing),
            ["sub dir", "a\tfile with tab.txt", "ékezet", "link"]
        );
        let file = listing.entries.iter().find(|e| e.name.starts_with('a')).unwrap();
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(file.size, 6);
        assert_eq!(file.permissions, "-rw-r--r--");
        assert_eq!(file.modified, Some(1_760_000_100));
        let link = listing.entries.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
    }

    #[test]
    fn parses_ls_fallback() {
        let now = at("2026-10-13T12:00:00Z");
        let body = "total 16\n\
            -rw-r--r--@ 1 501  0   3 Oct 13 11:00 a  file.txt\n\
            lrwxr-xr-x  1 0    0  10 Oct 12 09:05 link -> a  file.txt\n\
            drwxr-xr-x  2 0    0  64 Mar  2  2024 sub dir\n\
            crw-rw-rw-  1 0    0 1,3 Oct 13 10:00 null";
        let listing = parse(format!("/x\nRD-LS\n{body}").as_bytes(), now).unwrap();
        assert_eq!(names(&listing), ["sub dir", "a  file.txt", "link", "null"]);
        let file = &listing.entries[1];
        assert_eq!(file.permissions, "-rw-r--r--");
        assert_eq!(file.size, 3);
        assert_eq!(year(file.modified.unwrap()), 2026);
        assert_eq!(year(listing.entries[0].modified.unwrap()), 2024);
        assert_eq!(listing.entries[2].kind, EntryKind::Symlink);
        assert_eq!(listing.entries[3].kind, EntryKind::Other);
    }

    #[test]
    fn ls_date_in_the_future_is_last_year() {
        let now = at("2026-01-05T12:00:00Z");
        let listing = parse(b"/x\nRD-LS\n-rw-r--r-- 1 0 0 1 Dec 30 10:00 old\n", now).unwrap();
        assert_eq!(year(listing.entries[0].modified.unwrap()), 2025);
    }

    #[test]
    fn empty_folder_and_garbage() {
        assert!(parse(b"/empty\nRD-FIND\n", 0).unwrap().entries.is_empty());
        assert_eq!(parse(b"oops", 0), Err(Unreadable));
        assert_eq!(parse(b"/x\nWHAT\n", 0), Err(Unreadable));
        assert_eq!(parse(b"relative\nRD-FIND\n", 0), Err(Unreadable));
    }

    #[test]
    fn script_changes_into_quoted_path() {
        assert!(script("/var/it's").starts_with("cd -- '/var/it'\\''s' || exit 2\n"));
        assert!(script("~").starts_with("cd || exit 2\n"));
        assert!(script("~/src").starts_with("cd -- \"$HOME\"/'src' || exit 2\n"));
        assert!(script("/").contains("-printf '%y\\t%s\\t%T@\\t%M\\0%P\\0'"));
        assert!(script("/").contains("LC_ALL=C ls -lnA"));
    }

    #[test]
    fn finder_order() {
        let mut names = vec!["file10", "File2", "b", "Álom", "a", "file1"];
        names.sort_by(|a, b| standard_compare(a, b));
        assert_eq!(names, ["a", "Álom", "b", "file1", "File2", "file10"]);
        assert_eq!(standard_compare("x", "x"), Ordering::Equal);
        assert_ne!(standard_compare("A", "a"), Ordering::Equal);
    }
}
