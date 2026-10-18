//! The user's `known_hosts` files for background connections (Swift `SSHHostKeys`, which used
//! `ssh-keygen -F` and `ssh-keyscan`): lookup with plain patterns (wildcards, negation), hashed
//! `|1|salt|hash` entries (HMAC-SHA1) and the `[host]:port` form, `@revoked` keys, OpenSSH-style SHA256
//! fingerprints, appending an accepted key (`~/.ssh` created 0700, the file 0600), and replacing a
//! changed key (the conflicting entries removed, every other line kept byte for byte, the file rewritten
//! atomically with its permissions).

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::config::host_matches;

/// A public host key: its algorithm name and wire-format blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    /// `ssh-ed25519`, `ssh-rsa`, `ecdsa-sha2-nistp256`, …
    pub algorithm: String,
    /// The SSH wire encoding of the key.
    pub blob: Vec<u8>,
}

impl HostKey {
    /// A key from its algorithm and blob.
    #[must_use]
    pub fn new(algorithm: impl Into<String>, blob: Vec<u8>) -> Self {
        Self {
            algorithm: algorithm.into(),
            blob,
        }
    }

    /// Parses `<algorithm> <base64>` (a trailing comment is ignored).
    #[must_use]
    pub fn from_openssh(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace();
        let algorithm = parts.next()?;
        let blob = STANDARD.decode(parts.next()?).ok()?;
        Some(Self::new(algorithm, blob))
    }

    /// `<algorithm> <base64>`.
    #[must_use]
    pub fn to_openssh(&self) -> String {
        format!("{} {}", self.algorithm, STANDARD.encode(&self.blob))
    }

    /// `SHA256:<base64 without padding>`, as `ssh-keygen -l` prints it.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(&self.blob)))
    }
}

/// The name a host key is stored under: `host`, or `[host]:port` when the port isn't 22.
#[must_use]
pub fn lookup_name(host: &str, port: u16) -> String {
    let host = host.to_lowercase();
    if port == 22 {
        host
    } else {
        format!("[{host}]:{port}")
    }
}

/// What the `known_hosts` files say about a server's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A matching entry exists.
    Known,
    /// No entry with this key for the host: ask the user.
    Unknown,
    /// The host has a different key of the same type: refuse (possible attack).
    Changed {
        /// The file holding the old key.
        file: PathBuf,
        /// Its 1-based line.
        line: usize,
    },
    /// The key is marked `@revoked`: refuse.
    Revoked {
        /// The file holding the marker.
        file: PathBuf,
        /// Its 1-based line.
        line: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    None,
    Revoked,
    CertAuthority,
}

#[derive(Debug, Clone)]
struct Entry {
    file: PathBuf,
    line: usize,
    marker: Marker,
    hosts: String,
    key: HostKey,
}

impl Entry {
    fn matches_host(&self, name: &str) -> bool {
        if let Some(hashed) = self.hosts.strip_prefix("|1|") {
            let Some((salt, hash)) = hashed.split_once('|') else {
                return false;
            };
            let (Ok(salt), Ok(hash)) = (STANDARD.decode(salt), STANDARD.decode(hash)) else {
                return false;
            };
            let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
                return false;
            };
            mac.update(name.as_bytes());
            mac.verify_slice(&hash).is_ok()
        } else {
            let patterns: Vec<String> = self.hosts.split(',').map(str::to_owned).collect();
            host_matches(name, &patterns)
        }
    }
}

/// The entries of one or more `known_hosts` files.
#[derive(Debug, Clone, Default)]
pub struct KnownHosts {
    entries: Vec<Entry>,
}

fn parse_line(file: &Path, number: usize, line: &str) -> Option<Entry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let mut first = parts.next()?;
    let marker = match first {
        "@revoked" => Marker::Revoked,
        "@cert-authority" => Marker::CertAuthority,
        m if m.starts_with('@') => return None,
        _ => Marker::None,
    };
    if marker != Marker::None {
        first = parts.next()?;
    }
    let algorithm = parts.next()?;
    let blob = STANDARD.decode(parts.next()?).ok()?;
    Some(Entry {
        file: file.to_path_buf(),
        line: number,
        marker,
        hosts: first.to_owned(),
        key: HostKey::new(algorithm, blob),
    })
}

impl KnownHosts {
    /// Parses the text of `file`.
    #[must_use]
    pub fn parse(file: &Path, text: &str) -> Self {
        Self {
            entries: text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| parse_line(file, i + 1, line))
                .collect(),
        }
    }

    /// Reads every file that exists (missing ones are skipped).
    #[must_use]
    pub fn load(files: &[PathBuf]) -> Self {
        let mut all = Self::default();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(file) {
                all.entries.extend(Self::parse(file, &text).entries);
            }
        }
        all
    }

    /// Checks `key` for `host` (the `HostKeyAlias` or host name) on `port`. A revoked key always loses;
    /// an exact entry is known; a different key of the same type is a change; anything else (no entry,
    /// or only keys of other types) is unknown. `@cert-authority` lines are ignored.
    #[must_use]
    pub fn check(&self, host: &str, port: u16, key: &HostKey) -> Verdict {
        let name = lookup_name(host, port);
        let candidates: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.marker != Marker::CertAuthority)
            .collect();
        if let Some(revoked) = candidates
            .iter()
            .find(|e| e.marker == Marker::Revoked && e.key == *key)
        {
            return Verdict::Revoked {
                file: revoked.file.clone(),
                line: revoked.line,
            };
        }
        let for_host: Vec<&&Entry> = candidates
            .iter()
            .filter(|e| e.marker == Marker::None && e.matches_host(&name))
            .collect();
        if for_host.iter().any(|e| e.key == *key) {
            return Verdict::Known;
        }
        if let Some(old) = for_host.iter().find(|e| e.key.algorithm == key.algorithm) {
            return Verdict::Changed {
                file: old.file.clone(),
                line: old.line,
            };
        }
        Verdict::Unknown
    }
}

/// Appends `host`'s key to `file` as `<name> <algorithm> <base64>`. A missing parent directory is
/// created 0700 and a new file 0600 (Unix); a missing final newline is added first.
///
/// # Errors
/// When the directory or the file can't be created or written.
pub fn append(file: &Path, host: &str, port: u16, key: &HostKey) -> io::Result<()> {
    append_named(file, &lookup_name(host, port), key)
}

/// [`append`] under a ready lookup name (`host` or `[host]:port`).
fn append_named(file: &Path, name: &str, key: &HostKey) -> io::Result<()> {
    if let Some(dir) = file.parent()
        && !dir.as_os_str().is_empty()
        && !dir.exists()
    {
        create_private_dir(dir)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options.open(file)?;
    let length = handle.metadata()?.len();
    let mut needs_newline = false;
    if length > 0 {
        handle.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        handle.read_exact(&mut last)?;
        needs_newline = last[0] != b'\n';
    }
    let line = format!(
        "{}{name} {}\n",
        if needs_newline { "\n" } else { "" },
        key.to_openssh()
    );
    handle.write_all(line.as_bytes())?;
    handle.sync_all()
}

/// A changed host key as the system `ssh` reported it before refusing to connect ("WARNING: REMOTE HOST
/// IDENTIFICATION HAS CHANGED!" … "Host key verification failed.").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedHostKey {
    /// The name the key is stored under, as `ssh` printed it (`host` or `[host]:port`).
    pub host: String,
    /// `ssh`'s short key type: `ED25519`, `RSA`, `ECDSA`, … (see [`key_type_label`]).
    pub key_type: String,
    /// The new key's fingerprint, `SHA256:…`.
    pub fingerprint: String,
    /// The `known_hosts` file with the old key.
    pub file: String,
    /// Its 1-based line.
    pub line: usize,
}

/// Reads `ssh`'s changed host key report out of its output. `None` unless it refused the connection
/// ("Host key verification failed.") and named the host, the new fingerprint and the offending entry.
#[must_use]
pub fn parse_changed_warning(output: &str) -> Option<ChangedHostKey> {
    let lines: Vec<&str> = output.lines().map(str::trim).collect();
    if !lines.contains(&"Host key verification failed.") {
        return None;
    }
    let mut host = None;
    let mut fingerprint = None;
    let mut offending = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("Host key for ")
            && let Some((name, _)) = rest.split_once(" has changed")
        {
            host = Some(name.to_owned());
        } else if line.starts_with("The fingerprint for the ") && line.ends_with("sent by the remote host is")
        {
            fingerprint = lines[i + 1..]
                .iter()
                .find(|l| !l.is_empty())
                .map(|l| l.trim_end_matches('.').to_owned());
        } else if let Some(rest) = line.strip_prefix("Offending ")
            && offending.is_none()
            && let Some((key_type, location)) = rest.split_once(" key in ")
            && !key_type.contains(' ')
            && let Some((file, number)) = location.rsplit_once(':')
            && let Ok(number) = number.parse::<usize>()
        {
            offending = Some((key_type.to_owned(), file.to_owned(), number));
        }
    }
    let (key_type, file, line) = offending?;
    Some(ChangedHostKey {
        host: host?,
        key_type,
        fingerprint: fingerprint.filter(|f| f.contains(':'))?,
        file,
        line,
    })
}

/// `ssh`'s short name of a key type, as in its "Offending ED25519 key in …" line: `ED25519`, `RSA`,
/// `ECDSA`, `DSA`, `ED25519-SK`, `ECDSA-SK`, else the algorithm name itself.
#[must_use]
pub fn key_type_label(algorithm: &str) -> &str {
    match algorithm {
        "ssh-ed25519" => "ED25519",
        "ssh-rsa" | "rsa-sha2-256" | "rsa-sha2-512" => "RSA",
        "ssh-dss" => "DSA",
        "sk-ssh-ed25519@openssh.com" => "ED25519-SK",
        "sk-ecdsa-sha2-nistp256@openssh.com" => "ECDSA-SK",
        a if a.starts_with("ecdsa-sha2-") => "ECDSA",
        a => a,
    }
}

/// What one line becomes when the entries of `name` with a key of type `label` (other than `keep`) are
/// removed: `None` keeps it as it is, `Some("")` drops it, anything else is the rewritten line. Comments,
/// `@revoked` and `@cert-authority` lines are never touched. A plain line listing other hosts too keeps
/// them and loses only the literal `name`; a line matching through a wildcard goes as a whole.
fn without_entry(path: &Path, raw: &str, name: &str, label: &str, keep: Option<&HostKey>) -> Option<String> {
    let entry = parse_line(path, 0, raw)?;
    if entry.marker != Marker::None
        || key_type_label(&entry.key.algorithm) != label
        || keep.is_some_and(|k| *k == entry.key)
        || !entry.matches_host(name)
    {
        return None;
    }
    if entry.hosts.starts_with("|1|") {
        return Some(String::new());
    }
    let rest: Vec<String> = entry
        .hosts
        .split(',')
        .filter(|p| !p.eq_ignore_ascii_case(name))
        .map(str::to_owned)
        .collect();
    if rest.iter().all(|p| p.starts_with('!')) || host_matches(name, &rest) {
        return Some(String::new());
    }
    // `parse_line` took the hosts from the first word, so this finds them.
    let start = raw.find(&entry.hosts)?;
    Some(format!(
        "{}{}{}",
        &raw[..start],
        rest.join(","),
        &raw[start + entry.hosts.len()..]
    ))
}

/// Removes from `file` the entries of `name` (a lookup name: `host` or `[host]:port`) whose key has the
/// type `label` (see [`key_type_label`]) and isn't `keep`. Every other line stays byte for byte; the file
/// is rewritten atomically (a temp file renamed over it, following a symlink, with the old permissions),
/// and only when something changed. A missing file removes nothing. Returns the number of lines changed.
///
/// # Errors
/// When the file can't be read or written.
pub fn remove(file: &Path, name: &str, label: &str, keep: Option<&HostKey>) -> io::Result<usize> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let name = name.to_lowercase();
    let mut changed = 0;
    let mut out = String::with_capacity(text.len());
    for raw in text.split_inclusive('\n') {
        let (body, ending) = match raw.strip_suffix("\r\n") {
            Some(body) => (body, "\r\n"),
            None => raw.strip_suffix('\n').map_or((raw, ""), |body| (body, "\n")),
        };
        match without_entry(file, body, &name, label, keep) {
            None => out.push_str(raw),
            Some(line) => {
                changed += 1;
                if !line.is_empty() {
                    out.push_str(&line);
                    out.push_str(ending);
                }
            }
        }
    }
    if changed > 0 {
        rewrite(file, out.as_bytes())?;
    }
    Ok(changed)
}

/// Replaces a changed host key: the entries of `name` (a lookup name) with another key of the same type
/// are removed from each of `files` and `target` (see [`remove`]), then `key` is appended to `target` (the
/// file that held the old key) unless it is there already. Returns the number of lines removed or
/// rewritten.
///
/// # Errors
/// When a file can't be read or written; files before it may already be updated.
pub fn replace(files: &[PathBuf], target: &Path, name: &str, key: &HostKey) -> io::Result<usize> {
    let label = key_type_label(&key.algorithm);
    let mut all: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    if !all.contains(&target) {
        all.push(target);
    }
    let mut changed = 0;
    for file in all {
        changed += remove(file, name, label, Some(key))?;
    }
    let name = name.to_lowercase();
    let present = KnownHosts::load(&[target.to_path_buf()])
        .entries
        .iter()
        .any(|e| e.marker == Marker::None && e.key == *key && e.matches_host(&name));
    if !present {
        append_named(target, &name, key)?;
    }
    Ok(changed)
}

/// Writes `contents` to `file` through a temp file in the same folder renamed over it, keeping the old
/// file's permissions. A symlinked `file` is resolved first, so the link stays a link.
fn rewrite(file: &Path, contents: &[u8]) -> io::Result<()> {
    let real = std::fs::canonicalize(file)?;
    let dir = real.parent().unwrap_or_else(|| Path::new("."));
    let permissions = std::fs::metadata(&real)?.permissions();
    let mut temp = tempfile::Builder::new()
        .prefix(".known_hosts.")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    temp.write_all(contents)?;
    temp.as_file().sync_all()?;
    std::fs::set_permissions(temp.path(), permissions)?;
    temp.persist(&real).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up keys: only the bytes matter here.
    fn key(algorithm: &str, seed: u8) -> HostKey {
        let mut blob = Vec::new();
        blob.extend_from_slice(&u32::try_from(algorithm.len()).unwrap().to_be_bytes());
        blob.extend_from_slice(algorithm.as_bytes());
        blob.extend_from_slice(&[0, 0, 0, 32]);
        blob.extend(std::iter::repeat_n(seed, 32));
        HostKey::new(algorithm, blob)
    }

    fn hashed(name: &str, salt: &[u8]) -> String {
        let mut mac = Hmac::<Sha1>::new_from_slice(salt).unwrap();
        mac.update(name.as_bytes());
        format!(
            "|1|{}|{}",
            STANDARD.encode(salt),
            STANDARD.encode(mac.finalize().into_bytes())
        )
    }

    fn sample() -> (KnownHosts, HostKey, HostKey, HostKey) {
        let ed = key("ssh-ed25519", 1);
        let ed_other = key("ssh-ed25519", 2);
        let rsa = key("ssh-rsa", 3);
        let text = format!(
            "# comment\n\
             \n\
             example.com,192.0.2.5 {ed}\n\
             [example.com]:2222 {rsa}\n\
             {hashed} {ed}\n\
             *.example.org,!bad.example.org {ed}\n\
             @cert-authority *.example.net {ed_other}\n\
             @revoked * {revoked}\n\
             @unknown-marker example.com {ed_other}\n\
             garbage line\n\
             broken.example.com ssh-ed25519 !!!notbase64\n",
            ed = ed.to_openssh(),
            rsa = rsa.to_openssh(),
            ed_other = ed_other.to_openssh(),
            hashed = hashed("[192.0.2.9]:2200", b"salt-bytes-12345678"),
            revoked = key("ssh-ed25519", 9).to_openssh(),
        );
        (KnownHosts::parse(Path::new("/kh"), &text), ed, ed_other, rsa)
    }

    #[test]
    fn plain_entries_and_port_form() {
        let (hosts, ed, ed_other, rsa) = sample();
        assert_eq!(hosts.check("example.com", 22, &ed), Verdict::Known);
        assert_eq!(hosts.check("EXAMPLE.com", 22, &ed), Verdict::Known);
        assert_eq!(hosts.check("192.0.2.5", 22, &ed), Verdict::Known);
        assert_eq!(hosts.check("example.com", 2222, &rsa), Verdict::Known);
        // Port 2222 has no ed25519 entry: a new key type is unknown, not changed.
        assert_eq!(hosts.check("example.com", 2222, &ed), Verdict::Unknown);
        assert_eq!(
            hosts.check("example.com", 22, &ed_other),
            Verdict::Changed {
                file: PathBuf::from("/kh"),
                line: 3
            }
        );
        assert_eq!(hosts.check("example.com", 22, &rsa), Verdict::Unknown);
        assert_eq!(hosts.check("other.test", 22, &ed), Verdict::Unknown);
    }

    #[test]
    fn hashed_entries() {
        let (hosts, ed, ed_other, _) = sample();
        assert_eq!(hosts.check("192.0.2.9", 2200, &ed), Verdict::Known);
        assert_eq!(
            hosts.check("192.0.2.9", 2200, &ed_other),
            Verdict::Changed {
                file: PathBuf::from("/kh"),
                line: 5
            }
        );
        assert_eq!(hosts.check("192.0.2.9", 22, &ed), Verdict::Unknown);
    }

    #[test]
    fn hashed_line_from_ssh_keygen() {
        // `ssh-keygen -H` output for `[192.0.2.9]:2200` with a throwaway key.
        let line = "|1|1WvYzwdFiLegzcNnlhlOdTehd9A=|7iKqYxzt/n26cuMMSjEZC6gfTN8= ssh-ed25519 \
                    AAAAC3NzaC1lZDI1NTE5AAAAIDJil+C6OKJkr9cMsZdSuOx85x1QVJ3qbT8HcpDPmmMU";
        let hosts = KnownHosts::parse(Path::new("/kh"), line);
        let key = HostKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDJil+C6OKJkr9cMsZdSuOx85x1QVJ3qbT8HcpDPmmMU",
        )
        .unwrap();
        assert_eq!(hosts.check("192.0.2.9", 2200, &key), Verdict::Known);
        assert_eq!(hosts.check("192.0.2.9", 22, &key), Verdict::Unknown);
    }

    #[test]
    fn wildcards_and_negation() {
        let (hosts, ed, ..) = sample();
        assert_eq!(hosts.check("web.example.org", 22, &ed), Verdict::Known);
        assert_eq!(hosts.check("bad.example.org", 22, &ed), Verdict::Unknown);
    }

    #[test]
    fn markers() {
        let (hosts, _, ed_other, _) = sample();
        // @cert-authority is ignored.
        assert_eq!(hosts.check("a.example.net", 22, &ed_other), Verdict::Unknown);
        assert_eq!(
            hosts.check("anything.test", 22, &key("ssh-ed25519", 9)),
            Verdict::Revoked {
                file: PathBuf::from("/kh"),
                line: 8
            }
        );
    }

    #[test]
    fn several_keys_per_host() {
        let ed = key("ssh-ed25519", 1);
        let ecdsa = key("ecdsa-sha2-nistp256", 4);
        let text = format!(
            "host.example.com {}\nhost.example.com {}\n",
            ed.to_openssh(),
            ecdsa.to_openssh()
        );
        let hosts = KnownHosts::parse(Path::new("/kh"), &text);
        assert_eq!(hosts.check("host.example.com", 22, &ed), Verdict::Known);
        assert_eq!(hosts.check("host.example.com", 22, &ecdsa), Verdict::Known);
    }

    #[test]
    fn fingerprint_like_openssh() {
        // A throwaway key; `ssh-keygen -lf` prints this fingerprint for it.
        let key = HostKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDJil+C6OKJkr9cMsZdSuOx85x1QVJ3qbT8HcpDPmmMU test",
        )
        .unwrap();
        assert_eq!(key.algorithm, "ssh-ed25519");
        assert_eq!(
            key.fingerprint(),
            "SHA256:UrfsIL8MHH+8HuZ73p6ZKMoNYLt5jyAz6OjfkLXRibA"
        );
        assert!(!key.fingerprint().ends_with('='));
        assert!(HostKey::from_openssh("ssh-ed25519").is_none());
        assert!(HostKey::from_openssh("ssh-ed25519 ***").is_none());
    }

    #[test]
    fn load_skips_missing_files_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("new-ssh");
        let first = ssh.join("known_hosts");
        let second = dir.path().join("known_hosts2");
        let ed = key("ssh-ed25519", 1);
        std::fs::write(&second, format!("example.com {}", ed.to_openssh())).unwrap();

        let files = [first.clone(), second.clone()];
        assert_eq!(
            KnownHosts::load(&files).check("example.com", 22, &ed),
            Verdict::Known
        );
        assert_eq!(
            KnownHosts::load(&files).check("192.0.2.1", 2221, &ed),
            Verdict::Unknown
        );

        append(&first, "192.0.2.1", 2221, &ed).unwrap();
        append(&first, "Example.com", 22, &key("ssh-rsa", 3)).unwrap();
        let text = std::fs::read_to_string(&first).unwrap();
        assert_eq!(
            text,
            format!(
                "[192.0.2.1]:2221 {}\nexample.com {}\n",
                ed.to_openssh(),
                key("ssh-rsa", 3).to_openssh()
            )
        );
        assert_eq!(
            KnownHosts::load(&files).check("192.0.2.1", 2221, &ed),
            Verdict::Known
        );

        // A file without a final newline gets one first.
        append(&second, "b.example.com", 22, &ed).unwrap();
        let text = std::fs::read_to_string(&second).unwrap();
        assert!(text.contains(&format!("\nb.example.com {}\n", ed.to_openssh())));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&ssh), 0o700);
            assert_eq!(mode(&first), 0o600);
        }
    }

    // OpenSSH 10's report (the "Add correct host key" path is cut at 100 characters by `ssh` itself).
    const CHANGED_REPORT: &str = "\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\n\
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\r\n\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\n\
IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\r\n\
Someone could be eavesdropping on you right now (man-in-the-middle attack)!\r\n\
It is also possible that a host key has just been changed.\r\n\
The fingerprint for the ED25519 key sent by the remote host is\r\n\
SHA256:5RYZHykU3uEe2QseILbfEyxq3AkxvpPbe5yXRxWpPgo.\r\n\
Please contact your system administrator.\r\n\
Add correct host key in /home/user name/.ssh/kno to get rid of this message.\r\n\
Offending key for IP in /home/user name/.ssh/known_hosts:9\r\n\
Offending ED25519 key in /home/user name/.ssh/known_hosts:2\r\n\
  remove with:\r\n\
  ssh-keygen -f '/home/user name/.ssh/known_hosts' -R '[192.0.2.7]:2221'\r\n\
Host key for [192.0.2.7]:2221 has changed and you have requested strict checking.\r\n\
Host key verification failed.\r\n";

    #[test]
    fn parses_ssh_changed_key_report() {
        assert_eq!(
            parse_changed_warning(CHANGED_REPORT),
            Some(ChangedHostKey {
                host: "[192.0.2.7]:2221".to_owned(),
                key_type: "ED25519".to_owned(),
                fingerprint: "SHA256:5RYZHykU3uEe2QseILbfEyxq3AkxvpPbe5yXRxWpPgo".to_owned(),
                file: "/home/user name/.ssh/known_hosts".to_owned(),
                line: 2,
            })
        );
        let windows = CHANGED_REPORT.replace("/home/user name/.ssh/", "C:\\Users\\u/.ssh/");
        let parsed = parse_changed_warning(&windows).unwrap();
        assert_eq!(
            (parsed.file.as_str(), parsed.line),
            ("C:\\Users\\u/.ssh/known_hosts", 2)
        );
        // ssh didn't refuse (StrictHostKeyChecking=no goes on without passwords): nothing to offer.
        let went_on = CHANGED_REPORT.replace(
            "Host key verification failed.",
            "Password authentication is disabled.",
        );
        assert_eq!(parse_changed_warning(&went_on), None);
        // Without the offending entry, the host or the fingerprint there is nothing to replace.
        for missing in ["Offending ED25519", "Host key for", "SHA256:5RY"] {
            let text = CHANGED_REPORT
                .lines()
                .filter(|l| !l.starts_with(missing))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(parse_changed_warning(&text), None, "{missing}");
        }
        assert_eq!(parse_changed_warning("Host key verification failed.\n"), None);
    }

    #[test]
    fn key_type_labels_like_ssh() {
        assert_eq!(key_type_label("ssh-ed25519"), "ED25519");
        assert_eq!(key_type_label("rsa-sha2-512"), "RSA");
        assert_eq!(key_type_label("ssh-rsa"), "RSA");
        assert_eq!(key_type_label("ecdsa-sha2-nistp384"), "ECDSA");
        assert_eq!(key_type_label("ssh-dss"), "DSA");
        assert_eq!(key_type_label("sk-ssh-ed25519@openssh.com"), "ED25519-SK");
        assert_eq!(key_type_label("sk-ecdsa-sha2-nistp256@openssh.com"), "ECDSA-SK");
        assert_eq!(key_type_label("x-unknown"), "x-unknown");
    }

    #[test]
    fn replace_rewrites_only_the_conflicting_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        let old = key("ssh-ed25519", 1);
        let new = key("ssh-ed25519", 2);
        let rsa = key("ssh-rsa", 3);
        let other = key("ssh-ed25519", 4);
        let text = format!(
            "# keep me\r\n\
             \n\
             [192.0.2.7]:2221 {old}\r\n\
             [192.0.2.7]:2221 {rsa}\n\
             web.example.com,[192.0.2.7]:2221,db.example.com {old} trailing comment\n\
             {hashed} {old}\n\
             {hashed_other} {other}\n\
             @cert-authority [192.0.2.7]:2221 {old}\n\
             @revoked [192.0.2.7]:2221 {old}\n\
             192.0.2.7 {other}\n\
             garbage line\n\
             last.example.com {other}",
            old = old.to_openssh(),
            rsa = rsa.to_openssh(),
            other = other.to_openssh(),
            hashed = hashed("[192.0.2.7]:2221", b"salt-one-12345678901"),
            hashed_other = hashed("[192.0.2.8]:2221", b"salt-two-12345678901"),
        );
        std::fs::write(&file, &text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let before = KnownHosts::load(std::slice::from_ref(&file));
        assert!(matches!(
            before.check("192.0.2.7", 2221, &new),
            Verdict::Changed { line: 3, .. }
        ));

        let changed = replace(std::slice::from_ref(&file), &file, "[192.0.2.7]:2221", &new).unwrap();
        assert_eq!(changed, 3, "the plain line, the shared line and the hashed line");
        let after = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            after,
            format!(
                "# keep me\r\n\
                 \n\
                 [192.0.2.7]:2221 {rsa}\n\
                 web.example.com,db.example.com {old} trailing comment\n\
                 {hashed_other} {other}\n\
                 @cert-authority [192.0.2.7]:2221 {old}\n\
                 @revoked [192.0.2.7]:2221 {old}\n\
                 192.0.2.7 {other}\n\
                 garbage line\n\
                 last.example.com {other}\n\
                 [192.0.2.7]:2221 {new}\n",
                old = old.to_openssh(),
                rsa = rsa.to_openssh(),
                other = other.to_openssh(),
                new = new.to_openssh(),
                hashed_other = hashed("[192.0.2.8]:2221", b"salt-two-12345678901"),
            )
        );
        let hosts = KnownHosts::load(std::slice::from_ref(&file));
        assert_eq!(hosts.check("192.0.2.7", 2221, &new), Verdict::Known);
        assert_eq!(hosts.check("192.0.2.7", 2221, &rsa), Verdict::Known);
        // The revoked marker stays: the old key is still refused.
        assert!(matches!(
            hosts.check("192.0.2.7", 2221, &old),
            Verdict::Revoked { .. }
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o644, "permissions kept");
        }

        // Again: nothing to remove, the key is not appended twice, the file isn't touched.
        assert_eq!(
            replace(std::slice::from_ref(&file), &file, "[192.0.2.7]:2221", &new).unwrap(),
            0
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), after);
    }

    #[test]
    fn replace_across_files_and_wildcards() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("known_hosts");
        let second = dir.path().join("known_hosts2");
        let missing = dir.path().join("missing");
        let old = key("ecdsa-sha2-nistp256", 1);
        let new = key("ecdsa-sha2-nistp256", 2);
        std::fs::write(&first, "# only a comment\n").unwrap();
        std::fs::write(
            &second,
            format!(
                "*.example.com {old}\nHOST.example.com {old}\nother.example.com {old}\n",
                old = old.to_openssh()
            ),
        )
        .unwrap();

        let files = [first.clone(), second.clone(), missing.clone()];
        assert_eq!(replace(&files, &second, "host.example.com", &new).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "# only a comment\n");
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            format!(
                "other.example.com {old}\nhost.example.com {new}\n",
                old = old.to_openssh(),
                new = new.to_openssh()
            )
        );
        assert!(!missing.exists());
        assert_eq!(
            KnownHosts::load(&files).check("host.example.com", 22, &new),
            Verdict::Known
        );
        assert_eq!(remove(&missing, "host.example.com", "ECDSA", None).unwrap(), 0);
    }

    #[test]
    fn remove_by_type_without_a_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        let ed = key("ssh-ed25519", 1);
        let rsa = key("ssh-rsa", 3);
        std::fs::write(
            &file,
            format!(
                "h.example.com {}\nh.example.com {}\n",
                ed.to_openssh(),
                rsa.to_openssh()
            ),
        )
        .unwrap();
        assert_eq!(remove(&file, "H.example.com", "ED25519", None).unwrap(), 1);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("h.example.com {}\n", rsa.to_openssh())
        );
    }

    #[cfg(unix)]
    #[test]
    fn replace_follows_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles-known_hosts");
        let link = dir.path().join("known_hosts");
        let old = key("ssh-ed25519", 1);
        let new = key("ssh-ed25519", 2);
        std::fs::write(&real, format!("h.example.com {}\n", old.to_openssh())).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        replace(std::slice::from_ref(&link), &link, "h.example.com", &new).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            format!("h.example.com {}\n", new.to_openssh())
        );
        // No temp file is left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn lookup_names() {
        assert_eq!(lookup_name("Example.COM", 22), "example.com");
        assert_eq!(lookup_name("192.0.2.1", 2222), "[192.0.2.1]:2222");
    }
}
