//! What the Rust side reads out of the PTY stream: the title (OSC 0/2), the alternate screen, bracketed
//! paste, the `ssh` auth failure text and its changed host key report, plus the Swift rules built on them
//! (`RootDetection`, `TerminalClear`, `SessionTabs.exitCode`).
//!
//! The analyzer runs on the reader thread, not in the webview: `WebKit` throttles hidden views, and the
//! recorder and root warning must keep working for background tabs.

use serde::Serialize;

/// Whether a session runs as root (Swift `RootDetection`): from the login user, or once the remote shell
/// reports it, from the `user@host: dir` title most distributions set (so `sudo -i` and `su -` count).
pub mod root {
    /// The login user is `root` (surrounding spaces ignored, case-sensitive).
    #[must_use]
    pub fn is_root_user(username: &str) -> bool {
        username.trim_matches([' ', '\t']) == "root"
    }

    fn is_name_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
    }

    /// The user of a `user@host…` title, or `None` when the title has another form.
    #[must_use]
    pub fn user_from_title(title: &str) -> Option<&str> {
        let title = title.trim_matches([' ', '\t']);
        let user_len = title.find(|c: char| !is_name_char(c)).unwrap_or(title.len());
        let rest = title[user_len..].strip_prefix('@')?;
        (user_len > 0 && rest.starts_with(is_name_char)).then(|| &title[..user_len])
    }

    /// The title's user when it names one, else the login user.
    #[must_use]
    pub fn is_root(login_user: &str, terminal_title: Option<&str>) -> bool {
        match terminal_title.and_then(user_from_title) {
            Some(user) => user == "root",
            None => is_root_user(login_user),
        }
    }
}

/// What "Clear terminal" does (Swift `TerminalClear`): wipe the screen and scrollback locally, then let the
/// remote shell repaint its prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClearAction {
    /// A full-screen program (vim, htop, mc) owns the alternate screen; clearing would only garble it.
    None,
    /// The process has exited: nothing would answer Ctrl+L.
    LocalOnly,
    /// Clear locally, then send [`REDRAW_PROMPT`].
    LocalAndRedraw,
}

/// Cursor home, erase screen (ED 2), erase scrollback (ED 3).
pub const LOCAL_CLEAR_SEQUENCE: &str = "\u{1b}[H\u{1b}[2J\u{1b}[3J";
/// Ctrl+L: bash, zsh, fish and `RouterOS` repaint the prompt with the current input line.
pub const REDRAW_PROMPT: &[u8] = &[0x0C];

/// The clear decision.
#[must_use]
pub fn clear_action(is_alternate_screen: bool, is_running: bool) -> ClearAction {
    if is_alternate_screen {
        ClearAction::None
    } else if is_running {
        ClearAction::LocalAndRedraw
    } else {
        ClearAction::LocalOnly
    }
}

/// The exit code from a reported value, which may be a raw `waitpid` status with the code in the high
/// byte (Swift `SessionTabs.exitCode(fromReported:)`).
#[must_use]
pub fn normalize_exit_code(reported: Option<i64>) -> Option<i64> {
    let value = reported?;
    if value <= 255 {
        return Some(value);
    }
    Some(if value.trailing_zeros() >= 7 {
        (value >> 8) & 0xff
    } else {
        value
    })
}

/// A clean `exit` closes the tab; anything else keeps it open to show the error.
#[must_use]
pub fn closes_tab(exit_code: Option<i64>) -> bool {
    exit_code == Some(0)
}

/// `ssh`'s auth failure line: `user@host: Permission denied (publickey,password).`
const AUTH_FAILURE: &[u8] = b"Permission denied (";

/// The banner of `ssh`'s changed host key report; what follows it is kept for
/// [`crate::known_hosts::parse_changed_warning`].
const HOST_KEY_CHANGED: &[u8] = b"REMOTE HOST IDENTIFICATION HAS CHANGED";

/// How much output after [`HOST_KEY_CHANGED`] is kept (the report is about 1 KB).
const HOST_KEY_REPORT_MAX: usize = 16 * 1024;

/// A change the analyzer noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The terminal title (trimmed, non-empty).
    Title(String),
    /// The alternate screen was entered or left.
    AltScreen(bool),
    /// Bracketed paste mode was switched.
    BracketedPaste(bool),
}

#[derive(Debug, Default)]
struct State {
    title: Option<String>,
    shell_title: Option<String>,
    /// The shell's directory: from OSC 7 or a `user@host: dir` title, whichever came last.
    directory: Option<String>,
    alt_screen: bool,
    bracketed_paste: bool,
    changes: Vec<Change>,
}

impl State {
    fn set_title(&mut self, raw: &str) {
        let trimmed = raw.trim_matches([' ', '\t']);
        if trimmed.is_empty() || self.title.as_deref() == Some(trimmed) {
            return;
        }
        self.title = Some(trimmed.to_owned());
        if root::user_from_title(trimmed).is_some() {
            self.shell_title = Some(trimmed.to_owned());
            if let Some(directory) = crate::files::path::directory_from_terminal_title(trimmed) {
                self.directory = Some(directory);
            }
        }
        self.changes.push(Change::Title(trimmed.to_owned()));
    }

    fn set_alt_screen(&mut self, on: bool) {
        if self.alt_screen != on {
            self.alt_screen = on;
            self.changes.push(Change::AltScreen(on));
        }
    }

    fn set_bracketed_paste(&mut self, on: bool) {
        if self.bracketed_paste != on {
            self.bracketed_paste = on;
            self.changes.push(Change::BracketedPaste(on));
        }
    }
}

impl vte::Perform for State {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, title @ ..] = params
            && (*kind == b"0" || *kind == b"2")
            && !title.is_empty()
        {
            // vte splits on `;`, which a title may contain.
            let title = title.join(&b';');
            self.set_title(&String::from_utf8_lossy(&title));
        } else if let [b"7", url @ ..] = params
            && !url.is_empty()
            && let Some(directory) =
                crate::files::path::directory_from_file_url(&String::from_utf8_lossy(&url.join(&b';')))
        {
            // OSC 7 `file://host/path`: the shell's working directory (no change event, read on demand).
            self.directory = Some(directory);
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        if intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        let on = action == 'h';
        for param in params {
            match param.first() {
                Some(1049 | 1047 | 47) => self.set_alt_screen(on),
                Some(2004) => self.set_bracketed_paste(on),
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        // RIS (full reset) leaves the alternate screen and bracketed paste.
        if intermediates.is_empty() && byte == b'c' {
            self.set_alt_screen(false);
            self.set_bracketed_paste(false);
        }
    }
}

/// The stream analyzer of one session.
pub struct Analyzer {
    parser: vte::Parser,
    state: State,
    tail: Vec<u8>,
    auth_failed: bool,
    /// The last bytes, for a [`HOST_KEY_CHANGED`] banner split across chunks.
    banner_tail: Vec<u8>,
    /// The output from the banner on, once it was seen.
    host_key_report: Option<Vec<u8>>,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyzer")
            .field("state", &self.state)
            .field("auth_failed", &self.auth_failed)
            .finish_non_exhaustive()
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self {
            parser: vte::Parser::new(),
            state: State::default(),
            tail: Vec::new(),
            auth_failed: false,
            banner_tail: Vec::new(),
            host_key_report: None,
        }
    }
}

impl Analyzer {
    /// Feeds output and returns what changed, in order.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Change> {
        self.parser.advance(&mut self.state, bytes);
        self.watch_auth_failure(bytes);
        self.watch_host_key_report(bytes);
        std::mem::take(&mut self.state.changes)
    }

    fn watch_host_key_report(&mut self, bytes: &[u8]) {
        if let Some(report) = &mut self.host_key_report {
            let room = HOST_KEY_REPORT_MAX.saturating_sub(report.len());
            report.extend_from_slice(&bytes[..bytes.len().min(room)]);
            return;
        }
        self.banner_tail.extend_from_slice(bytes);
        if let Some(at) = self
            .banner_tail
            .windows(HOST_KEY_CHANGED.len())
            .position(|w| w == HOST_KEY_CHANGED)
        {
            let mut report = self.banner_tail[at..].to_vec();
            report.truncate(HOST_KEY_REPORT_MAX);
            self.host_key_report = Some(report);
            self.banner_tail = Vec::new();
            return;
        }
        let keep = HOST_KEY_CHANGED.len() - 1;
        if self.banner_tail.len() > keep {
            self.banner_tail.drain(..self.banner_tail.len() - keep);
        }
    }

    /// `ssh` refused a changed host key: what its report says (see
    /// [`crate::known_hosts::parse_changed_warning`]).
    #[must_use]
    pub fn host_key_changed(&self) -> Option<crate::known_hosts::ChangedHostKey> {
        let report = self.host_key_report.as_ref()?;
        crate::known_hosts::parse_changed_warning(&String::from_utf8_lossy(report))
    }

    fn watch_auth_failure(&mut self, bytes: &[u8]) {
        if self.auth_failed {
            return;
        }
        self.tail.extend_from_slice(bytes);
        if self.tail.windows(AUTH_FAILURE.len()).any(|w| w == AUTH_FAILURE) {
            self.auth_failed = true;
        }
        let keep = AUTH_FAILURE.len() - 1;
        if self.tail.len() > keep {
            self.tail.drain(..self.tail.len() - keep);
        }
    }

    /// The last non-empty title.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.state.title.as_deref()
    }

    /// The last `user@host…` title (program titles such as `htop` don't replace it).
    #[must_use]
    pub fn shell_title(&self) -> Option<&str> {
        self.state.shell_title.as_deref()
    }

    /// The shell's working directory: the last OSC 7 report or `user@host: dir` title (`~` forms kept).
    #[must_use]
    pub fn directory(&self) -> Option<&str> {
        self.state.directory.as_deref()
    }

    /// A full-screen program owns the alternate screen.
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        self.state.alt_screen
    }

    /// The remote side asked for bracketed paste.
    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        self.state.bracketed_paste
    }

    /// `ssh` printed its "Permission denied (…)" line.
    #[must_use]
    pub fn auth_failed(&self) -> bool {
        self.auth_failed
    }
}

#[cfg(test)]
mod tests {
    use super::root::*;
    use super::*;

    /// `RootDetectionTests.rootUser`.
    #[test]
    fn root_user() {
        assert!(is_root_user("root"));
        assert!(is_root_user(" root "));
        assert!(!is_root_user("Root"));
        assert!(!is_root_user("rootless"));
        assert!(!is_root_user(""));
    }

    /// `RootDetectionTests.userFromTitle`.
    #[test]
    fn user_from_titles() {
        assert_eq!(user_from_title("root@pbx-middle: ~"), Some("root"));
        assert_eq!(user_from_title("admin@web01.example.com:/var/log"), Some("admin"));
        assert_eq!(user_from_title("rootless@h: ~"), Some("rootless"));
        assert_eq!(user_from_title("vim /etc/hosts"), None);
        assert_eq!(user_from_title("[admin@MikroTik] >"), None);
        assert_eq!(user_from_title("@host"), None);
        assert_eq!(user_from_title("user@"), None);
    }

    /// `RootDetectionTests.titleWinsOverLoginUser`.
    #[test]
    fn title_wins_over_login_user() {
        assert!(is_root("admin", Some("root@web: ~")));
        assert!(!is_root("admin", Some("admin@web: ~")));
        assert!(!is_root("root", Some("deploy@web: ~")));
    }

    /// `RootDetectionTests.loginUserWhenTitleSaysNothing`.
    #[test]
    fn login_user_when_title_says_nothing() {
        assert!(is_root("root", None));
        assert!(is_root("root", Some("htop")));
        assert!(!is_root("admin", None));
    }

    /// `TerminalClearTests` (5).
    #[test]
    fn running_shell_is_cleared_and_redrawn() {
        assert_eq!(clear_action(false, true), ClearAction::LocalAndRedraw);
    }

    #[test]
    fn exited_session_is_only_cleared_locally() {
        assert_eq!(clear_action(false, false), ClearAction::LocalOnly);
    }

    #[test]
    fn full_screen_program_is_left_alone() {
        for running in [true, false] {
            assert_eq!(clear_action(true, running), ClearAction::None);
        }
    }

    #[test]
    fn local_sequence_erases_screen_and_scrollback() {
        assert!(LOCAL_CLEAR_SEQUENCE.starts_with("\u{1b}[H"));
        assert!(LOCAL_CLEAR_SEQUENCE.contains("\u{1b}[2J"));
        assert!(LOCAL_CLEAR_SEQUENCE.contains("\u{1b}[3J"));
    }

    #[test]
    fn redraw_is_control_l() {
        assert_eq!(REDRAW_PROMPT, [0x0C]);
    }

    /// `SessionTabsTests.exitCodeNormalisation` and `exitPolicy`.
    #[test]
    fn exit_code_normalisation_and_policy() {
        assert_eq!(normalize_exit_code(Some(0)), Some(0));
        assert_eq!(normalize_exit_code(Some(255)), Some(255));
        assert_eq!(normalize_exit_code(Some(128)), Some(128));
        assert_eq!(normalize_exit_code(Some(255 << 8)), Some(255));
        assert_eq!(normalize_exit_code(Some(1 << 8)), Some(1));
        assert_eq!(normalize_exit_code(Some((1 << 8) | 9)), Some((1 << 8) | 9));
        assert_eq!(normalize_exit_code(None), None);
        assert!(closes_tab(Some(0)));
        assert!(!closes_tab(Some(255)));
        assert!(!closes_tab(Some(1)));
        assert!(!closes_tab(None));
    }

    #[test]
    fn reads_titles_including_semicolons_and_st_terminators() {
        let mut a = Analyzer::default();
        assert_eq!(
            a.feed(b"\x1b]0;root@web: ~\x07"),
            [Change::Title("root@web: ~".into())]
        );
        assert_eq!(a.feed(b"\x1b]2;a;b\x1b\\"), [Change::Title("a;b".into())]);
        assert_eq!(a.title(), Some("a;b"));
        assert_eq!(
            a.shell_title(),
            Some("root@web: ~"),
            "program titles keep the shell title"
        );
        assert!(
            a.feed(b"\x1b]2;a;b\x07").is_empty(),
            "unchanged titles are not reported"
        );
        assert!(a.feed(b"\x1b]2;   \x07").is_empty(), "blank titles are ignored");
        assert!(
            a.feed(b"\x1b]7;file://h/dir\x07").is_empty(),
            "OSC 7 is not a title"
        );
    }

    #[test]
    fn tracks_the_directory_from_osc7_and_shell_titles() {
        let mut a = Analyzer::default();
        assert_eq!(a.directory(), None);
        a.feed(b"\x1b]0;root@web: ~/src\x07");
        assert_eq!(a.directory(), Some("~/src"));
        a.feed(b"\x1b]7;file://web/var/log%20x\x07");
        assert_eq!(a.directory(), Some("/var/log x"));
        a.feed(b"\x1b]2;htop\x07");
        assert_eq!(a.directory(), Some("/var/log x"), "program titles keep it");
        a.feed(b"\x1b]2;root@web: /etc\x07");
        assert_eq!(a.directory(), Some("/etc"));
        a.feed(b"\x1b]7;http://x/y\x07");
        assert_eq!(a.directory(), Some("/etc"), "other schemes are ignored");
    }

    #[test]
    fn tracks_alt_screen_and_bracketed_paste_across_chunks() {
        let mut a = Analyzer::default();
        assert_eq!(a.feed(b"\x1b[?10"), []);
        assert_eq!(a.feed(b"49h"), [Change::AltScreen(true)]);
        assert!(a.alt_screen());
        assert_eq!(a.feed(b"\x1b[?1049h"), [], "no change, no event");
        assert_eq!(
            a.feed(b"\x1b[?1049l\x1b[?2004h"),
            [Change::AltScreen(false), Change::BracketedPaste(true)]
        );
        assert_eq!(a.feed(b"\x1b[?47h"), [Change::AltScreen(true)]);
        assert_eq!(a.feed(b"\x1b[?1047l"), [Change::AltScreen(false)]);
        assert_eq!(a.feed(b"\x1b[?1;2004l"), [Change::BracketedPaste(false)]);
        assert!(!a.bracketed_paste());
        assert_eq!(a.feed(b"\x1b[1049h"), [], "not a private mode");
        a.feed(b"\x1b[?1049h\x1b[?2004h");
        assert_eq!(
            a.feed(b"\x1bc"),
            [Change::AltScreen(false), Change::BracketedPaste(false)]
        );
    }

    #[test]
    fn notices_the_auth_failure_line_split_across_chunks() {
        let mut a = Analyzer::default();
        a.feed(b"cat: /root/x: Permission denied\r\n");
        assert!(!a.auth_failed());
        a.feed(b"tester@h: Permission de");
        a.feed(b"nied (publickey,password).\r\n");
        assert!(a.auth_failed());
    }

    #[test]
    fn keeps_the_changed_host_key_report_split_across_chunks() {
        let mut a = Analyzer::default();
        a.feed(b"Host key verification failed.\r\n");
        assert_eq!(a.host_key_changed(), None, "no banner, no report");
        a.feed(b"@@@@\r\n@    WARNING: REMOTE HOST IDENTIF");
        a.feed(b"ICATION HAS CHANGED!     @\r\n");
        a.feed(
            b"The fingerprint for the ED25519 key sent by the remote host is\r\nSHA256:abc.\r\n\
              Offending ED25519 key in /kh:4\r\n",
        );
        assert_eq!(a.host_key_changed(), None, "not refused yet");
        a.feed(b"Host key for h.example.com has changed and you have requested strict checking.\r\n");
        a.feed(b"Host key verification failed.\r\n");
        let changed = a.host_key_changed().unwrap();
        assert_eq!(
            (
                changed.host.as_str(),
                changed.key_type.as_str(),
                changed.fingerprint.as_str(),
                changed.file.as_str(),
                changed.line
            ),
            ("h.example.com", "ED25519", "SHA256:abc", "/kh", 4)
        );
        // The kept report is bounded.
        a.feed(&vec![b'x'; HOST_KEY_REPORT_MAX * 2]);
        assert_eq!(a.host_key_report.as_ref().unwrap().len(), HOST_KEY_REPORT_MAX);
    }
}
