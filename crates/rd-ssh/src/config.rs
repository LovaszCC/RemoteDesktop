//! `~/.ssh/config` resolution for the background connections (russh), which don't run the system `ssh`
//! and so can't ask `ssh -G` (what Swift's `SSHHostKeys.target` did).
//!
//! Supported: `Host` patterns (`*`, `?`, `!negation`), `Match all` / `Match host …` (other `Match`
//! criteria never match), `Include` (relative to `~/.ssh`, `*`/`?` in the file name), `key value` and
//! `key=value` lines with quoted arguments, and the first-value-wins rule. Resolved: `HostName` (`%h`),
//! `Port`, `User`, `IdentityFile` (several, `~` and `%d %h %p %r %u %%`), `HostKeyAlias`,
//! `UserKnownHostsFile` and `IdentitiesOnly`. `ProxyJump` and `ProxyCommand` are reported as
//! unsupported for background connections.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How deep `Include` may nest (OpenSSH's limit).
const MAX_INCLUDE_DEPTH: usize = 16;

/// Which hosts a block applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Condition {
    /// Lines before the first `Host`, and `Match all`.
    Always,
    /// `Host` patterns or `Match host` patterns.
    Patterns(Vec<String>),
    /// A `Match` with criteria this parser doesn't evaluate.
    Never,
}

#[derive(Debug, Clone)]
struct Block {
    condition: Condition,
    /// `(lowercased key, arguments)` in file order.
    options: Vec<(String, Vec<String>)>,
}

/// A parsed `ssh_config` file (with its includes inlined).
#[derive(Debug, Clone, Default)]
pub struct SshConfig {
    blocks: Vec<Block>,
}

/// What the config says about one host, before the connection's own values are applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostConfig {
    /// `HostName` with `%h` expanded, if set.
    pub host_name: Option<String>,
    /// `Port`.
    pub port: Option<u16>,
    /// `User`.
    pub user: Option<String>,
    /// Every `IdentityFile`, in order, as written (expanded later by [`Target`]).
    pub identity_files: Vec<String>,
    /// `HostKeyAlias`.
    pub host_key_alias: Option<String>,
    /// `UserKnownHostsFile` arguments, as written.
    pub user_known_hosts_files: Vec<String>,
    /// `IdentitiesOnly yes`.
    pub identities_only: bool,
    /// `ProxyJump` (not `none`).
    pub proxy_jump: Option<String>,
    /// `ProxyCommand` (not `none`).
    pub proxy_command: Option<String>,
}

/// The resolved connection target: where to connect, as whom, which keys to try and where its host key
/// is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The host as the user typed it.
    pub original_host: String,
    /// The address to connect to.
    pub host_name: String,
    /// The TCP port.
    pub port: u16,
    /// The login user; empty means the local user name.
    pub user: String,
    /// Key files from the config, expanded.
    pub identity_files: Vec<PathBuf>,
    /// The name the host key is stored under (`HostKeyAlias`, else the host name).
    pub host_key_name: String,
    /// The user's `known_hosts` files: `UserKnownHostsFile`, else `~/.ssh/known_hosts` and
    /// `~/.ssh/known_hosts2`. The first one is where accepted keys are added.
    pub known_hosts_files: Vec<PathBuf>,
    /// Only the configured identities, no agent keys.
    pub identities_only: bool,
    /// `ProxyJump` or `ProxyCommand` is set: background connections can't honour it.
    pub unsupported_proxy: Option<String>,
}

/// Splits a config line into words: whitespace separated, `"quoted"` words keep their spaces, and the
/// first `=` (with optional spaces around it) may separate the key.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has_word = false;
    let mut separator_used = false;
    for c in line.chars() {
        // `=` separates the key: right after it, or after the key and spaces.
        let key_separator =
            c == '=' && !separator_used && ((out.is_empty() && has_word) || (out.len() == 1 && !has_word));
        match c {
            '"' => {
                quoted = !quoted;
                has_word = true;
            }
            c if !quoted && (c.is_whitespace() || key_separator) => {
                separator_used |= key_separator;
                if has_word {
                    out.push(std::mem::take(&mut current));
                    has_word = false;
                }
            }
            c => {
                current.push(c);
                has_word = true;
            }
        }
    }
    if has_word {
        out.push(current);
    }
    out
}

/// `*` and `?` wildcard match, case-insensitive (OpenSSH `match_pattern` on a lowercased host).
#[must_use]
pub fn wildcard_match(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let (mut t, mut p) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            t += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// OpenSSH `match_hostname`: a negated pattern that matches rejects the host outright; otherwise any
/// positive match accepts it.
#[must_use]
pub fn host_matches(host: &str, patterns: &[String]) -> bool {
    let mut matched = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard_match(host, negated) {
                return false;
            }
        } else if wildcard_match(host, pattern) {
            matched = true;
        }
    }
    matched
}

/// The `Match` line's condition. Only `all` and `host`/`originalhost` lists are evaluated.
fn match_condition(args: &[String]) -> Condition {
    match args {
        [all] if all.eq_ignore_ascii_case("all") => Condition::Always,
        [criterion, list] if ["host", "originalhost"].contains(&criterion.to_lowercase().as_str()) => {
            Condition::Patterns(list.split(',').map(str::to_owned).collect())
        }
        _ => Condition::Never,
    }
}

/// Files named by an `Include` argument: `~` expanded, relative to `ssh_dir`, `*`/`?` in the file name.
fn include_paths(arg: &str, ssh_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let expanded = crate::command::expand_tilde(arg, home);
    let path = if expanded.is_absolute() {
        expanded
    } else {
        ssh_dir.join(expanded)
    };
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| wildcard_match(&e.file_name().to_string_lossy(), &name))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

impl SshConfig {
    /// Parses config text. `ssh_dir` is where relative `Include`s are looked up; `home` expands `~`.
    #[must_use]
    pub fn parse(text: &str, ssh_dir: &Path, home: Option<&Path>) -> Self {
        let mut config = Self {
            blocks: vec![Block {
                condition: Condition::Always,
                options: Vec::new(),
            }],
        };
        config.parse_into(text, ssh_dir, home, 0);
        config
    }

    /// Reads `path` (a missing or unreadable file is an empty config).
    #[must_use]
    pub fn load(path: &Path, home: Option<&Path>) -> Self {
        let ssh_dir = path.parent().unwrap_or(Path::new("."));
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self::parse(&text, ssh_dir, home)
    }

    /// The user's `~/.ssh/config`.
    #[must_use]
    pub fn load_user(home: Option<&Path>) -> Self {
        match home {
            Some(home) => Self::load(&home.join(".ssh").join("config"), Some(home)),
            None => Self::default(),
        }
    }

    fn parse_into(&mut self, text: &str, ssh_dir: &Path, home: Option<&Path>, depth: usize) {
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = words(line);
            if words.is_empty() {
                continue;
            }
            let key = words.remove(0).to_lowercase();
            match key.as_str() {
                "host" => self.blocks.push(Block {
                    condition: Condition::Patterns(words),
                    options: Vec::new(),
                }),
                "match" => self.blocks.push(Block {
                    condition: match_condition(&words),
                    options: Vec::new(),
                }),
                "include" => {
                    if depth >= MAX_INCLUDE_DEPTH {
                        continue;
                    }
                    for arg in &words {
                        for path in include_paths(arg, ssh_dir, home) {
                            if let Ok(text) = std::fs::read_to_string(&path) {
                                self.parse_into(&text, ssh_dir, home, depth + 1);
                            }
                        }
                    }
                }
                _ => {
                    if let Some(block) = self.blocks.last_mut() {
                        block.options.push((key, words));
                    }
                }
            }
        }
    }

    /// The settings for `host` (as typed): first value wins, `IdentityFile` accumulates.
    #[must_use]
    pub fn host(&self, host: &str) -> HostConfig {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = HostConfig::default();
        for block in &self.blocks {
            let applies = match &block.condition {
                Condition::Always => true,
                Condition::Patterns(patterns) => host_matches(host, patterns),
                Condition::Never => false,
            };
            if !applies {
                continue;
            }
            for (key, args) in &block.options {
                let Some(first) = args.first() else { continue };
                if key == "identityfile" {
                    out.identity_files.push(first.clone());
                    continue;
                }
                if !seen.insert(key.as_str()) {
                    continue;
                }
                match key.as_str() {
                    "hostname" => out.host_name = Some(expand_host_name(first, host)),
                    "port" => out.port = first.parse().ok(),
                    "user" => out.user = Some(first.clone()),
                    "hostkeyalias" => out.host_key_alias = Some(first.clone()),
                    "userknownhostsfile" => {
                        out.user_known_hosts_files = args
                            .iter()
                            .filter(|a| !a.eq_ignore_ascii_case("none"))
                            .cloned()
                            .collect();
                    }
                    "identitiesonly" => out.identities_only = first.eq_ignore_ascii_case("yes"),
                    "proxyjump" if !first.eq_ignore_ascii_case("none") => {
                        out.proxy_jump = Some(args.join(" "));
                    }
                    "proxycommand" if !first.eq_ignore_ascii_case("none") => {
                        out.proxy_command = Some(args.join(" "));
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// The full target for a connection to `host`. The connection's own non-default `port` and non-empty
    /// `user` win over the config, as `ssh -p … user@host` would.
    #[must_use]
    pub fn target(&self, host: &str, port: Option<u16>, user: &str, home: Option<&Path>) -> Target {
        let config = self.host(host);
        let host_name = config.host_name.clone().unwrap_or_else(|| host.to_owned());
        let port = port.or(config.port).unwrap_or(22);
        let user = if user.is_empty() {
            config.user.clone().unwrap_or_default()
        } else {
            user.to_owned()
        };
        let tokens = Tokens {
            home,
            host: &host_name,
            original_host: host,
            port,
            user: &user,
        };
        let identity_files = config
            .identity_files
            .iter()
            .filter(|f| !f.eq_ignore_ascii_case("none"))
            .map(|f| tokens.expand_path(f))
            .collect();
        let known_hosts_files = if config.user_known_hosts_files.is_empty() {
            home.map(|h| {
                let dir = h.join(".ssh");
                vec![dir.join("known_hosts"), dir.join("known_hosts2")]
            })
            .unwrap_or_default()
        } else {
            config
                .user_known_hosts_files
                .iter()
                .map(|f| tokens.expand_path(f))
                .collect()
        };
        let unsupported_proxy = config
            .proxy_jump
            .map(|j| format!("ProxyJump {j}"))
            .or(config.proxy_command.map(|c| format!("ProxyCommand {c}")));
        Target {
            original_host: host.to_owned(),
            host_key_name: config.host_key_alias.unwrap_or_else(|| host_name.clone()),
            host_name,
            port,
            user,
            identity_files,
            known_hosts_files,
            identities_only: config.identities_only,
            unsupported_proxy,
        }
    }
}

/// `HostName`'s `%h` (the host as typed) and `%%`.
fn expand_host_name(value: &str, host: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(host),
            Some('%') | None => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

struct Tokens<'a> {
    home: Option<&'a Path>,
    host: &'a str,
    original_host: &'a str,
    port: u16,
    user: &'a str,
}

impl Tokens<'_> {
    /// `~` and the `%d %h %n %p %r %u %%` tokens of `IdentityFile` / `UserKnownHostsFile`.
    fn expand_path(&self, value: &str) -> PathBuf {
        let home = self.home.map(|h| h.display().to_string()).unwrap_or_default();
        let local_user = local_user();
        let mut out = String::new();
        let mut chars = value.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('d') => out.push_str(&home),
                Some('h') => out.push_str(self.host),
                Some('n') => out.push_str(self.original_host),
                Some('p') => out.push_str(&self.port.to_string()),
                Some('r') => out.push_str(if self.user.is_empty() {
                    &local_user
                } else {
                    self.user
                }),
                Some('u') => out.push_str(&local_user),
                Some('%') | None => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
            }
        }
        crate::command::expand_tilde(&out, self.home)
    }
}

/// The local user name (`USER`, `USERNAME` on Windows).
#[must_use]
pub fn local_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> SshConfig {
        SshConfig::parse(text, Path::new("/nonexistent/.ssh"), Some(Path::new("/home/me")))
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("web1.example.com", "*.example.com"));
        assert!(wildcard_match("WEB1.Example.com", "web?.example.com"));
        assert!(!wildcard_match("web10.example.com", "web?.example.com"));
        assert!(wildcard_match("anything", "*"));
        assert!(wildcard_match("a", "a*"));
        assert!(!wildcard_match("example.com", "*.example.com"));
        assert!(wildcard_match("192.0.2.7", "192.0.2.*"));
        assert!(wildcard_match("abcabd", "*abd"));
    }

    #[test]
    fn negation_rejects_even_with_a_positive_match() {
        let patterns = vec!["*.example.com".to_owned(), "!bastion.example.com".to_owned()];
        assert!(host_matches("web.example.com", &patterns));
        assert!(!host_matches("bastion.example.com", &patterns));
        assert!(!host_matches("other.test", &patterns));
        // Only negations never match.
        assert!(!host_matches("x", &["!y".to_owned()]));
    }

    #[test]
    fn line_syntax() {
        assert_eq!(words("Host a b"), ["Host", "a", "b"]);
        assert_eq!(words("Port=2222"), ["Port", "2222"]);
        assert_eq!(words("  Port = 2222"), ["Port", "2222"]);
        assert_eq!(
            words(r#"IdentityFile "~/my keys/id""#),
            ["IdentityFile", "~/my keys/id"]
        );
        assert_eq!(words("LocalCommand a=b"), ["LocalCommand", "a=b"]);
    }

    #[test]
    fn first_value_wins_and_identity_files_accumulate() {
        let config = config(
            "# comment\n\
             Host web.example.com\n\
             \tHostName 192.0.2.10\n\
             \tUser deploy\n\
             \tIdentityFile ~/.ssh/web\n\
             Host *.example.com\n\
             \tHostName ignored.example.com\n\
             \tPort 2200\n\
             \tUser other\n\
             \tIdentityFile %d/.ssh/id_%h\n\
             Host *\n\
             \tIdentityFile ~/.ssh/id_ed25519\n",
        );
        let host = config.host("web.example.com");
        assert_eq!(host.host_name.as_deref(), Some("192.0.2.10"));
        assert_eq!(host.port, Some(2200));
        assert_eq!(host.user.as_deref(), Some("deploy"));
        assert_eq!(
            host.identity_files,
            ["~/.ssh/web", "%d/.ssh/id_%h", "~/.ssh/id_ed25519"]
        );

        let target = config.target("web.example.com", None, "", Some(Path::new("/home/me")));
        assert_eq!(target.host_name, "192.0.2.10");
        assert_eq!(target.port, 2200);
        assert_eq!(target.user, "deploy");
        assert_eq!(
            target.identity_files,
            [
                PathBuf::from("/home/me/.ssh/web"),
                PathBuf::from("/home/me/.ssh/id_192.0.2.10"),
                PathBuf::from("/home/me/.ssh/id_ed25519"),
            ]
        );
        assert_eq!(target.host_key_name, "192.0.2.10");
        assert_eq!(
            target.known_hosts_files,
            [
                PathBuf::from("/home/me/.ssh/known_hosts"),
                PathBuf::from("/home/me/.ssh/known_hosts2")
            ]
        );
        assert_eq!(target.unsupported_proxy, None);
    }

    #[test]
    fn connection_values_win_over_the_config() {
        let config = config("Host h\n Port 2200\n User cfg\n");
        let target = config.target("h", Some(2222), "me", None);
        assert_eq!((target.port, target.user.as_str()), (2222, "me"));
        let target = config.target("h", None, "", None);
        assert_eq!((target.port, target.user.as_str()), (2200, "cfg"));
        let target = config.target("other", None, "", None);
        assert_eq!((target.port, target.host_name.as_str()), (22, "other"));
    }

    #[test]
    fn global_lines_alias_known_hosts_and_hostname_token() {
        let config = config(
            "Port 2022\n\
             Host *.internal.example\n\
             HostName %h.example.org\n\
             HostKeyAlias shared-alias\n\
             UserKnownHostsFile ~/.ssh/kh_%h /etc/ssh/extra none\n\
             IdentitiesOnly yes\n",
        );
        let target = config.target("db.internal.example", None, "u", Some(Path::new("/home/me")));
        assert_eq!(target.port, 2022);
        assert_eq!(target.host_name, "db.internal.example.example.org");
        assert_eq!(target.host_key_name, "shared-alias");
        assert!(target.identities_only);
        assert_eq!(
            target.known_hosts_files,
            [
                PathBuf::from("/home/me/.ssh/kh_db.internal.example.example.org"),
                PathBuf::from("/etc/ssh/extra")
            ]
        );
    }

    #[test]
    fn match_blocks() {
        let config = config(
            "Match host *.example.com\n User matched\n\
             Match exec \"true\"\n Port 1\n\
             Match all\n Port 2\n",
        );
        let host = config.host("a.example.com");
        assert_eq!(host.user.as_deref(), Some("matched"));
        assert_eq!(host.port, Some(2));
        assert_eq!(config.host("b.test").user, None);
    }

    #[test]
    fn proxy_jump_is_reported() {
        let config = config(
            "Host inner\n ProxyJump bastion.example.com\nHost other\n ProxyJump none\nHost cmd\n ProxyCommand nc %h %p\n",
        );
        assert_eq!(
            config
                .target("inner", None, "", None)
                .unsupported_proxy
                .as_deref(),
            Some("ProxyJump bastion.example.com")
        );
        assert_eq!(config.target("other", None, "", None).unsupported_proxy, None);
        assert_eq!(
            config.target("cmd", None, "", None).unsupported_proxy.as_deref(),
            Some("ProxyCommand nc %h %p")
        );
    }

    #[test]
    fn includes_with_wildcards_and_nesting() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("conf.d")).unwrap();
        std::fs::write(ssh.join("conf.d/b.conf"), "Host b\n Port 2002\n").unwrap();
        std::fs::write(ssh.join("conf.d/a.conf"), "Host a\n Port 2001\n Include nested\n").unwrap();
        std::fs::write(ssh.join("nested"), "User nested\n").unwrap();
        std::fs::write(ssh.join("conf.d/ignored.txt"), "Host b\n Port 9\n").unwrap();
        std::fs::write(
            ssh.join("config"),
            "Include conf.d/*.conf\nInclude ~/.ssh/missing\nHost *\n Port 22\n",
        )
        .unwrap();
        let config = SshConfig::load_user(Some(dir.path()));
        assert_eq!(config.host("a").port, Some(2001));
        assert_eq!(config.host("a").user.as_deref(), Some("nested"));
        assert_eq!(config.host("b").port, Some(2002));
        assert_eq!(config.host("c").port, Some(22));
    }

    #[test]
    fn recursive_include_stops() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("config"), "Include config\nPort 7\n").unwrap();
        let config = SshConfig::load_user(Some(dir.path()));
        assert_eq!(config.host("x").port, Some(7));
    }

    #[test]
    fn missing_file_is_empty() {
        let config = SshConfig::load(Path::new("/nonexistent/config"), None);
        assert_eq!(config.host("x"), HostConfig::default());
        assert!(SshConfig::load_user(None).host("x").identity_files.is_empty());
    }

    #[test]
    fn tokens_in_paths() {
        let config = config("IdentityFile /keys/%r@%n:%p%%\n");
        let target = config.target("h", Some(2222), "bob", None);
        assert_eq!(target.identity_files, [PathBuf::from("/keys/bob@h:2222%")]);
        assert_eq!(expand_host_name("%h.%x%", "a"), "a.%x%");
    }
}
