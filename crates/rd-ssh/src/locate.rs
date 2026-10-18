//! Finding programs: the system `ssh` and the terminal apps.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The executable file name of `name` on this OS (`ssh.exe` on Windows).
#[must_use]
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) && Path::new(name).extension().is_none() {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[cfg(unix)]
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub(crate) fn is_executable(path: &Path) -> bool {
    // App execution aliases (`wt.exe`) are reparse points that `is_file` follows poorly; existence is enough.
    path.exists() && !path.is_dir()
}

/// The first `file` (an exact file name) in the directories of `path_var` that is an executable file.
#[must_use]
pub fn search_path(file: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(file))
        .find(|candidate| is_executable(candidate))
}

/// `name` from `PATH` (with `.exe` on Windows).
#[must_use]
pub fn which(name: &str) -> Option<PathBuf> {
    search_path(&exe_name(name), std::env::var_os("PATH").as_deref())
}

/// The Windows OpenSSH client that ships as an optional feature: `<windir>\System32\OpenSSH\ssh.exe`.
#[must_use]
pub fn windows_openssh(windir: &Path) -> PathBuf {
    windir.join("System32").join("OpenSSH").join("ssh.exe")
}

/// The system `ssh`: from `PATH`, else on Windows the OpenSSH feature under `%WINDIR%`. `None` when it
/// isn't installed (the UI explains how to add it).
#[must_use]
pub fn find_ssh() -> Option<PathBuf> {
    which("ssh").or_else(|| {
        if !cfg!(windows) {
            return None;
        }
        let windir = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"))?;
        Some(windows_openssh(Path::new(&windir))).filter(|p| is_executable(p))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exe(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn searches_the_path_in_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        make_exe(&b.path().join("tool"));
        let var = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(search_path("tool", Some(&var)), Some(b.path().join("tool")));
        make_exe(&a.path().join("tool"));
        assert_eq!(search_path("tool", Some(&var)), Some(a.path().join("tool")));
        assert_eq!(search_path("missing", Some(&var)), None);
        assert_eq!(search_path("tool", None), None);
    }

    #[cfg(unix)]
    #[test]
    fn skips_files_without_the_executable_bit_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plain"), "x").unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        let var = dir.path().as_os_str();
        assert_eq!(search_path("plain", Some(var)), None);
        assert_eq!(search_path("folder", Some(var)), None);
    }

    #[test]
    fn windows_feature_location_and_exe_names() {
        assert_eq!(
            windows_openssh(Path::new("C:\\Windows")),
            Path::new("C:\\Windows")
                .join("System32")
                .join("OpenSSH")
                .join("ssh.exe")
        );
        if cfg!(windows) {
            assert_eq!(exe_name("ssh"), "ssh.exe");
            assert_eq!(exe_name("wt.exe"), "wt.exe");
        } else {
            assert_eq!(exe_name("ssh"), "ssh");
        }
    }

    #[test]
    fn finds_the_system_ssh_when_installed() {
        // CI runners and developer machines all ship an OpenSSH client.
        if let Some(path) = find_ssh() {
            assert!(path.is_absolute());
            assert!(path.file_stem().is_some_and(|s| s == "ssh"));
        }
    }
}
