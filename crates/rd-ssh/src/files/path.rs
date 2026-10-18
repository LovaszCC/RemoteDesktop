//! POSIX path helpers for the remote side (Swift `RemotePath`): never `std::path`, which would follow the
//! local OS's rules.

use crate::analyzer::root::user_from_title;
use crate::external::shell_quote;

/// `directory/name`.
#[must_use]
pub fn join(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

/// The parent folder: `/a/b/` → `/a`, `/a` → `/`, `/` → `/`; a relative name → `~`.
#[must_use]
pub fn parent(path: &str) -> String {
    let mut trimmed = path;
    while trimmed.len() > 1 && trimmed.ends_with('/') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    match trimmed.rfind('/') {
        None => "~".to_owned(),
        Some(0) => "/".to_owned(),
        Some(slash) => trimmed[..slash].to_owned(),
    }
}

/// The last component (`/var/log` → `log`).
#[must_use]
pub fn name(path: &str) -> String {
    path.split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// `cd` to a path: `~` and `~/x` go through `$HOME`, everything else is quoted.
#[must_use]
pub fn cd_command(path: &str) -> String {
    if path.is_empty() || path == "~" {
        return "cd".to_owned();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("cd -- \"$HOME\"/{}", shell_quote(rest));
    }
    format!("cd -- {}", shell_quote(path))
}

/// The working directory in a `user@host: dir` terminal title (`/var/log`, `~`, `~/src`), else `None`.
#[must_use]
pub fn directory_from_terminal_title(title: &str) -> Option<String> {
    user_from_title(title)?;
    let colon = title.find(':')?;
    let path = title[colon + 1..].trim_matches([' ', '\t']);
    (path.starts_with('/') || path.starts_with('~')).then(|| path.to_owned())
}

/// A `file://host/path` URL (OSC 7) as a path, percent-decoded; `None` for another scheme.
#[must_use]
pub fn directory_from_file_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = bytes
                .get(index + 1..index + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let path = String::from_utf8_lossy(&decoded).into_owned();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_parent_name() {
        assert_eq!(join("/a", "b c"), "/a/b c");
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(parent("/a/b"), "/a");
        assert_eq!(parent("/a/b/"), "/a");
        assert_eq!(parent("/a"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(parent("x"), "~");
        assert_eq!(name("/var/log"), "log");
        assert_eq!(name("/var/log/"), "log");
    }

    #[test]
    fn directory_from_titles() {
        assert_eq!(
            directory_from_terminal_title("root@web: /var/log").as_deref(),
            Some("/var/log")
        );
        assert_eq!(
            directory_from_terminal_title("admin@web:~/src").as_deref(),
            Some("~/src")
        );
        assert_eq!(
            directory_from_terminal_title("root@pbx-middle: ~").as_deref(),
            Some("~")
        );
        assert_eq!(directory_from_terminal_title("htop"), None);
        assert_eq!(directory_from_terminal_title("root@web: vim x"), None);
    }

    #[test]
    fn directory_from_osc7_urls() {
        assert_eq!(
            directory_from_file_url("file://web01/var/log%20files").as_deref(),
            Some("/var/log files")
        );
        assert_eq!(directory_from_file_url("file:///tmp").as_deref(), Some("/tmp"));
        assert_eq!(
            directory_from_file_url("file://h/%C3%A9%zz").as_deref(),
            Some("/é%zz")
        );
        assert_eq!(directory_from_file_url("http://h/x"), None);
        assert_eq!(directory_from_file_url("file://host-only"), None);
    }
}
