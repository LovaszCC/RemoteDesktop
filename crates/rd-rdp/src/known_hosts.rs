//! The app's own store of accepted RDP server certificates, `rdp_known_hosts` in the config directory
//! (`FreeRDP` kept its own `known_hosts2` for the Swift app). One line per server: `host port fingerprint`,
//! the fingerprint being the SHA-256 of the certificate's DER encoding as colon-separated hex. Also the
//! host-name check behind the "not for this host name" warning.

use std::fmt::Write as _;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The file name inside the config directory.
pub const FILE_NAME: &str = "rdp_known_hosts";

/// The SHA-256 fingerprint of a DER certificate: `AB:CD:…` (32 bytes, uppercase hex).
#[must_use]
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// What the store says about a server's certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Never accepted permanently: ask.
    New,
    /// The accepted certificate: connect without asking.
    Same,
    /// A different certificate was accepted before: ask with a warning.
    Changed {
        /// The fingerprint accepted before.
        previous: String,
    },
}

/// The `rdp_known_hosts` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHosts {
    path: PathBuf,
}

fn normalized(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

impl KnownHosts {
    /// The store at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store in the config directory `dir`.
    #[must_use]
    pub fn in_dir(dir: &Path) -> Self {
        Self::new(dir.join(FILE_NAME))
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn entries(&self) -> io::Result<Vec<(String, u16, String)>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(text
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let host = parts.next()?;
                let port = parts.next()?.parse().ok()?;
                let fingerprint = parts.next()?;
                (!host.starts_with('#')).then(|| (host.to_owned(), port, fingerprint.to_ascii_uppercase()))
            })
            .collect())
    }

    /// Checks `fingerprint` for `host`:`port`. An unreadable file counts as empty, so the user is asked.
    #[must_use]
    pub fn check(&self, host: &str, port: u16, fingerprint: &str) -> Verdict {
        let host = normalized(host);
        let stored = self
            .entries()
            .unwrap_or_default()
            .into_iter()
            .find(|(h, p, _)| *h == host && *p == port);
        match stored {
            None => Verdict::New,
            Some((_, _, previous)) if previous.eq_ignore_ascii_case(fingerprint) => Verdict::Same,
            Some((_, _, previous)) => Verdict::Changed { previous },
        }
    }

    /// Remembers `fingerprint` for `host`:`port` ("Accept permanently"), replacing an older one. The file
    /// (and its directory) is created when missing and replaced in one rename.
    ///
    /// # Errors
    /// When the file can't be read or written.
    pub fn accept(&self, host: &str, port: u16, fingerprint: &str) -> io::Result<()> {
        let host = normalized(host);
        let mut entries = self.entries()?;
        entries.retain(|(h, p, _)| !(*h == host && *p == port));
        entries.push((host, port, fingerprint.to_ascii_uppercase()));
        let mut text = String::new();
        for (h, p, f) in &entries {
            let _ = writeln!(text, "{h} {p} {f}");
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &self.path)
    }
}

/// Whether the certificate names (`names`: the DNS and IP subject alternative names, then the common
/// name) cover `host`. A `*.` wildcard matches one left-most label; IP addresses must match exactly.
/// Certificates without names never match.
#[must_use]
pub fn host_matches(host: &str, names: &[String]) -> bool {
    let host = normalized(host.trim_start_matches('[').trim_end_matches(']'));
    if let Ok(ip) = host.parse::<IpAddr>() {
        return names
            .iter()
            .any(|n| normalized(n).parse::<IpAddr>().is_ok_and(|other| other == ip));
    }
    names.iter().map(|n| normalized(n)).any(|name| {
        if let Some(suffix) = name.strip_prefix("*.") {
            host.split_once('.')
                .is_some_and(|(label, rest)| !label.is_empty() && rest == suffix)
        } else {
            name == host
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_colon_separated_sha256() {
        let fp = fingerprint(b"abc");
        assert_eq!(fp.len(), 32 * 3 - 1);
        assert!(fp.starts_with("BA:78:16:BF:"));
    }

    #[test]
    fn verdicts_new_same_changed() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnownHosts::in_dir(&dir.path().join("config"));
        assert_eq!(store.path().file_name().unwrap(), FILE_NAME);
        assert_eq!(store.check("srv", 3389, "AA:BB"), Verdict::New);
        store.accept("SRV", 3389, "aa:bb").unwrap();
        assert_eq!(store.check("srv", 3389, "AA:BB"), Verdict::Same);
        assert_eq!(store.check("srv.", 3389, "aa:bb"), Verdict::Same);
        assert_eq!(store.check("srv", 3390, "AA:BB"), Verdict::New);
        assert_eq!(
            store.check("srv", 3389, "CC:DD"),
            Verdict::Changed {
                previous: "AA:BB".into()
            }
        );
    }

    #[test]
    fn accept_replaces_the_old_entry_and_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnownHosts::in_dir(dir.path());
        store.accept("a", 3389, "11").unwrap();
        store.accept("b", 3389, "22").unwrap();
        store.accept("a", 3389, "33").unwrap();
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert_eq!(text, "b 3389 22\na 3389 33\n");
        assert_eq!(store.check("a", 3389, "33"), Verdict::Same);
    }

    #[test]
    fn junk_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnownHosts::in_dir(dir.path());
        std::fs::write(store.path(), "# comment 1 2\nbroken\nx notaport FF\nh 3389 ff\n").unwrap();
        assert_eq!(store.check("h", 3389, "FF"), Verdict::Same);
        assert_eq!(store.check("x", 3389, "FF"), Verdict::New);
    }

    #[test]
    fn host_names_and_wildcards() {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(host_matches("WIN-01", &names(&["win-01"])));
        assert!(host_matches("win-01.corp.local", &names(&["*.corp.local"])));
        assert!(!host_matches("a.b.corp.local", &names(&["*.corp.local"])));
        assert!(!host_matches("corp.local", &names(&["*.corp.local"])));
        assert!(!host_matches("win-02", &names(&["win-01"])));
        assert!(!host_matches("127.0.0.1", &names(&["localhost"])));
        assert!(host_matches("127.0.0.1", &names(&["127.0.0.1"])));
        assert!(host_matches("[::1]", &names(&["::1"])));
        assert!(!host_matches("win-01", &[]));
    }
}
