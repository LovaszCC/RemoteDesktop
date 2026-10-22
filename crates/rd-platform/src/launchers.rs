//! System search launchers: one file per sidebar entry in a folder the app owns, so Spotlight, the
//! Start menu search or the desktop search finds the entry by name. Opening one starts the app with the
//! entry's id (`--connect <id>`, or a `.rdentry` document on macOS).
//!
//! The folder is reconciled as a whole against the wanted set: files with the launcher extension that
//! aren't wanted byte for byte are removed, missing ones written. Other files in the folder stay.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rd_core::Timestamp;

use crate::APP_IDENTITY;

/// The launch argument that opens an entry.
pub const CONNECT_FLAG: &str = "--connect";

/// The macOS document extension.
pub const RDENTRY_EXTENSION: &str = "rdentry";

/// An entry that gets a launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherTarget {
    /// Entry id, the only thing the launcher carries.
    pub entry_id: String,
    /// Display name; becomes the file name.
    pub name: String,
    /// Orders same-named entries: the oldest keeps the plain name.
    pub created_at: Timestamp,
}

/// The launcher file kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherFormat {
    /// macOS: a `.rdentry` document the app opens.
    RdEntry,
    /// Windows: a `.lnk` shortcut to the app with `--connect <id>`.
    Lnk,
    /// Linux: a `.desktop` file running the app with `--connect <id>`.
    Desktop,
}

impl LauncherFormat {
    /// This OS's format.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::RdEntry
        } else if cfg!(windows) {
            Self::Lnk
        } else {
            Self::Desktop
        }
    }

    /// File extension without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::RdEntry => RDENTRY_EXTENSION,
            Self::Lnk => "lnk",
            Self::Desktop => "desktop",
        }
    }
}

/// This OS's launcher folder: `~/Applications/RemoteDesktop Entries` (Spotlight hides `~/Library`),
/// the Start menu's `Programs\RemoteDesktop Entries`, or `$XDG_DATA_HOME/applications/remotedesktop-entries`.
#[must_use]
pub fn launcher_dir() -> Option<PathBuf> {
    let name = format!("{} Entries", APP_IDENTITY.product_name);
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join("Applications").join(name))
    } else if cfg!(windows) {
        dirs::data_dir().map(|data| data.join(r"Microsoft\Windows\Start Menu\Programs").join(name))
    } else {
        let folder = format!("{}-entries", APP_IDENTITY.product_name.to_lowercase());
        dirs::data_dir().map(|data| data.join("applications").join(folder))
    }
}

/// The executable launchers start: the `AppImage` when running from one (its mount path changes on every
/// run), otherwise this binary.
///
/// # Errors
/// When the running binary's path is unknown.
pub fn launcher_exe() -> io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) if !image.is_empty() => Ok(PathBuf::from(image)),
        _ => std::env::current_exe(),
    }
}

/// A name usable as a file name on every OS: path separators and reserved characters become `_`,
/// leading dots and surrounding spaces go, a blank name becomes `Untitled`.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('.')
        .trim();
    if trimmed.is_empty() {
        "Untitled".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `(entry id, file stem)` per target in input order. Same names (case-insensitive) get ` (2)`, ` (3)`…
/// by creation time, then id.
#[must_use]
pub fn file_names(targets: &[LauncherTarget]) -> Vec<(String, String)> {
    let mut order: Vec<&LauncherTarget> = targets.iter().collect();
    order.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.entry_id.cmp(&b.entry_id))
    });
    let mut taken: HashSet<String> = HashSet::new();
    let mut stems: HashMap<&str, String> = HashMap::new();
    for target in order {
        let base = sanitize(&target.name);
        let mut stem = base.clone();
        let mut counter = 2;
        while taken.contains(&stem.to_lowercase()) {
            stem = format!("{base} ({counter})");
            counter += 1;
        }
        taken.insert(stem.to_lowercase());
        stems.insert(&target.entry_id, stem);
    }
    targets
        .iter()
        .map(|t| (t.entry_id.clone(), stems[t.entry_id.as_str()].clone()))
        .collect()
}

/// The launcher file's bytes.
///
/// # Errors
/// Writing the shortcut failed (Windows only).
pub fn render(format: LauncherFormat, entry_id: &str, name: &str, exe: &Path) -> io::Result<Vec<u8>> {
    match format {
        LauncherFormat::RdEntry => Ok(format!("{}\n", serde_json::json!({ "entry": entry_id })).into_bytes()),
        LauncherFormat::Desktop => Ok(desktop_file(entry_id, name, exe).into_bytes()),
        LauncherFormat::Lnk => lnk(entry_id, exe),
    }
}

/// A `.desktop` value: newlines would end the line, so they become spaces.
fn desktop_value(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// A quoted `Exec` argument per the Desktop Entry spec (`"`, `` ` ``, `$`, `\` escaped; `%` doubled).
fn desktop_quote(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn desktop_file(entry_id: &str, name: &str, exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={} {CONNECT_FLAG} {}\nIcon=rd-app\nTerminal=false\n\
         Categories=Network;RemoteAccess;\nX-RemoteDesktop-Entry={}\n",
        desktop_value(name),
        desktop_quote(&exe.to_string_lossy()),
        desktop_quote(entry_id),
        desktop_value(entry_id),
    )
}

#[cfg(windows)]
fn lnk(entry_id: &str, exe: &Path) -> io::Result<Vec<u8>> {
    let mut link = mslnk::ShellLink::new(exe).map_err(|e| io::Error::other(e.to_string()))?;
    link.set_arguments(Some(format!("{CONNECT_FLAG} {entry_id}")));
    link.set_icon_location(Some(exe.to_string_lossy().into_owned()));
    let file = tempfile::NamedTempFile::new()?;
    link.create_lnk(file.path())
        .map_err(|e| io::Error::other(e.to_string()))?;
    fs::read(file.path())
}

#[cfg(not(windows))]
fn lnk(_entry_id: &str, _exe: &Path) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "shortcuts are written on Windows only",
    ))
}

/// Makes `dir` hold exactly one launcher per target (see the module docs). Creates `dir` if needed.
///
/// # Errors
/// The folder can't be read or a file can't be written or removed.
pub fn reconcile(
    dir: &Path,
    format: LauncherFormat,
    targets: &[LauncherTarget],
    exe: &Path,
) -> io::Result<()> {
    let names = file_names(targets);
    let mut wanted: HashMap<String, Vec<u8>> = HashMap::new();
    for (target, (_, stem)) in targets.iter().zip(&names) {
        let bytes = render(format, &target.entry_id, &target.name, exe)?;
        wanted.insert(format!("{stem}.{}", format.extension()), bytes);
    }
    fs::create_dir_all(dir)?;
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if !path.is_file() || path.extension().is_none_or(|e| e != format.extension()) {
            continue;
        }
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match wanted.get(&file_name) {
            Some(bytes) if fs::read(&path).is_ok_and(|current| &current == bytes) => {
                wanted.remove(&file_name);
            }
            _ => fs::remove_file(&path)?,
        }
    }
    for (file_name, bytes) in wanted {
        let partial = dir.join(format!("{file_name}.partial"));
        fs::write(&partial, bytes)?;
        fs::rename(&partial, dir.join(file_name))?;
    }
    Ok(())
}

/// Deletes the launcher folder. A missing folder is fine.
///
/// # Errors
/// The folder exists but can't be removed.
pub fn remove_all(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The entry id in a `.rdentry` document.
#[must_use]
pub fn read_rdentry(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let id = value.get("entry")?.as_str()?.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The entry a launch asks for: `--connect <id>`, `--connect=<id>`, or a `.rdentry` path (read from
/// disk). The first argument (the program) is skipped.
#[must_use]
pub fn entry_request(args: &[String]) -> Option<String> {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == CONNECT_FLAG {
            return rest
                .next()
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty());
        }
        if let Some(id) = arg.strip_prefix(&format!("{CONNECT_FLAG}=")) {
            let id = id.trim();
            return (!id.is_empty()).then(|| id.to_owned());
        }
        let path = Path::new(arg);
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(RDENTRY_EXTENSION))
        {
            return fs::read(path).ok().and_then(|bytes| read_rdentry(&bytes));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, name: &str, created: &str) -> LauncherTarget {
        LauncherTarget {
            entry_id: id.to_owned(),
            name: name.to_owned(),
            created_at: Timestamp::parse(created).unwrap(),
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("rd-app")
            .chain(list.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    const EXE: &str = "/opt/rd/rd-app";

    #[test]
    fn file_names_append_counters_by_creation_order() {
        let targets = [
            target("B", "Home Server", "2026-01-02 00:00:00.000"),
            target("A", "home server", "2026-01-01 00:00:00.000"),
            target("C", "Home Server", "2026-01-02 00:00:00.000"),
            target("D", "Router", "2026-01-03 00:00:00.000"),
        ];
        let names = file_names(&targets);
        assert_eq!(
            names,
            vec![
                ("B".to_owned(), "Home Server (2)".to_owned()),
                ("A".to_owned(), "home server".to_owned()),
                ("C".to_owned(), "Home Server (3)".to_owned()),
                ("D".to_owned(), "Router".to_owned()),
            ]
        );
    }

    #[test]
    fn file_names_sanitize_invalid_and_blank_names() {
        let targets = [
            target("A", "a/b:c*?\"<>|\\d", "2026-01-01 00:00:00.000"),
            target("B", "   ", "2026-01-01 00:00:00.000"),
            target("C", "..hidden.", "2026-01-01 00:00:00.000"),
            target("D", "tab\there", "2026-01-01 00:00:00.000"),
        ];
        let stems: Vec<String> = file_names(&targets).into_iter().map(|(_, s)| s).collect();
        assert_eq!(stems, ["a_b_c_______d", "Untitled", "hidden", "tab_here"]);
    }

    #[test]
    fn rdentry_round_trips() {
        let bytes = render(LauncherFormat::RdEntry, "ABC-1", "Home", Path::new(EXE)).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            "{\"entry\":\"ABC-1\"}\n"
        );
        assert_eq!(read_rdentry(&bytes).as_deref(), Some("ABC-1"));
        assert_eq!(read_rdentry(b"{\"entry\":\"\"}"), None);
        assert_eq!(read_rdentry(b"garbage"), None);
    }

    #[test]
    fn desktop_file_has_exec_and_id() {
        let bytes = render(
            LauncherFormat::Desktop,
            "ABC-1",
            "Home\nServer",
            Path::new("/opt/my $app/rd-app"),
        )
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(text.contains("\nName=Home Server\n"));
        assert!(text.contains("\nExec=\"/opt/my \\$app/rd-app\" --connect \"ABC-1\"\n"));
        assert!(text.contains("\nX-RemoteDesktop-Entry=ABC-1\n"));
        assert!(!text.contains("NoDisplay"));
    }

    #[test]
    fn reconcile_writes_missing_and_removes_stale() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("launchers");
        let exe = Path::new(EXE);
        let mut targets = vec![
            target("A", "Home Server", "2026-01-01 00:00:00.000"),
            target("B", "Router", "2026-01-02 00:00:00.000"),
        ];
        reconcile(&dir, LauncherFormat::RdEntry, &targets, exe).unwrap();
        assert_eq!(files(&dir), ["Home Server.rdentry", "Router.rdentry"]);

        // Renamed, deleted, added.
        targets[0].name = "NAS".to_owned();
        targets.remove(1);
        targets.push(target("C", "Printer", "2026-01-03 00:00:00.000"));
        reconcile(&dir, LauncherFormat::RdEntry, &targets, exe).unwrap();
        assert_eq!(files(&dir), ["NAS.rdentry", "Printer.rdentry"]);
        assert_eq!(
            read_rdentry(&fs::read(dir.join("NAS.rdentry")).unwrap()).as_deref(),
            Some("A")
        );

        // A file whose content was swapped is rewritten.
        fs::write(dir.join("Printer.rdentry"), b"{\"entry\":\"A\"}").unwrap();
        reconcile(&dir, LauncherFormat::RdEntry, &targets, exe).unwrap();
        assert_eq!(
            read_rdentry(&fs::read(dir.join("Printer.rdentry")).unwrap()).as_deref(),
            Some("C")
        );
    }

    #[test]
    fn reconcile_renamed_onto_an_existing_name_keeps_both() {
        let temp = tempfile::tempdir().unwrap();
        let exe = Path::new(EXE);
        let mut targets = vec![
            target("A", "Home Server", "2026-01-01 00:00:00.000"),
            target("B", "Router", "2026-01-02 00:00:00.000"),
        ];
        reconcile(temp.path(), LauncherFormat::Desktop, &targets, exe).unwrap();
        targets[1].name = "Home Server".to_owned();
        reconcile(temp.path(), LauncherFormat::Desktop, &targets, exe).unwrap();
        assert_eq!(
            files(temp.path()),
            ["Home Server (2).desktop", "Home Server.desktop"]
        );
        let first = fs::read_to_string(temp.path().join("Home Server.desktop")).unwrap();
        assert!(first.contains("X-RemoteDesktop-Entry=A\n"));
    }

    #[test]
    fn reconcile_keeps_foreign_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".DS_Store"), b"x").unwrap();
        fs::write(temp.path().join("notes.txt"), b"x").unwrap();
        fs::create_dir(temp.path().join("sub.rdentry")).unwrap();
        reconcile(temp.path(), LauncherFormat::RdEntry, &[], Path::new(EXE)).unwrap();
        assert_eq!(files(temp.path()), [".DS_Store", "notes.txt", "sub.rdentry"]);
    }

    #[test]
    fn remove_all_deletes_the_folder_and_tolerates_a_missing_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("launchers");
        remove_all(&dir).unwrap();
        reconcile(
            &dir,
            LauncherFormat::RdEntry,
            &[target("A", "X", "2026-01-01 00:00:00.000")],
            Path::new(EXE),
        )
        .unwrap();
        remove_all(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn entry_request_parses_flag_forms_and_rdentry_path() {
        assert_eq!(
            entry_request(&args(&["--connect", "ABC"])).as_deref(),
            Some("ABC")
        );
        assert_eq!(entry_request(&args(&["--connect=ABC"])).as_deref(), Some("ABC"));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Home.rdentry");
        fs::write(&path, b"{\"entry\":\"XYZ\"}").unwrap();
        assert_eq!(
            entry_request(&args(&[path.to_str().unwrap()])).as_deref(),
            Some("XYZ")
        );
    }

    #[test]
    fn entry_request_ignores_unrelated_args() {
        assert_eq!(entry_request(&args(&[])), None);
        assert_eq!(entry_request(&args(&["--verbose", "file.txt"])), None);
        assert_eq!(entry_request(&args(&["--connect"])), None);
        assert_eq!(entry_request(&args(&["--connect="])), None);
        assert_eq!(entry_request(&["--connect".to_owned(), "X".to_owned()]), None);
    }

    #[test]
    fn launcher_dir_is_named_after_the_app() {
        let dir = launcher_dir().unwrap();
        let name = dir.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(name.starts_with("remotedesktop"), "{name}");
    }

    #[cfg(windows)]
    #[test]
    fn lnk_is_written_for_the_running_exe() {
        let exe = std::env::current_exe().unwrap();
        let bytes = render(LauncherFormat::Lnk, "ABC", "Home", &exe).unwrap();
        assert_eq!(&bytes[..4], &[0x4c, 0, 0, 0]);
        let temp = tempfile::tempdir().unwrap();
        reconcile(
            temp.path(),
            LauncherFormat::Lnk,
            &[target("A", "Home", "2026-01-01 00:00:00.000")],
            &exe,
        )
        .unwrap();
        assert_eq!(files(temp.path()), ["Home.lnk"]);
    }
}
