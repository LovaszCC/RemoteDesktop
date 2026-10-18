//! "Copy next command output" (a port of the Swift `OutputRecorder`).
//!
//! Once armed, it captures what the remote side prints after the user's next Enter, as plain text (escape
//! sequences stripped, `\r\n` → `\n`), until the prompt seen at arming time comes back. The output is
//! followed all the time (only its last line is kept while idle), so the prompt is the line the stream
//! ended with when the recorder was armed, parsed the same way as the closing one. Fed with the raw PTY
//! stream, so it works in whatever state the shell is in (`sudo -i`, `su`, nested `ssh`, `RouterOS`), and
//! neither line wrapping nor the scrollback limit affect the result.

use serde::Serialize;

/// Where the recorder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Not recording.
    Idle,
    /// Waiting for the user to press Enter.
    Armed,
    /// Capturing the command's output.
    Capturing,
}

/// What a chunk of output meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Nothing special.
    None,
    /// The stream ends with the prompt again: the command has most likely finished.
    AtPrompt,
    /// The output passed `max_bytes`; the recorder has been reset.
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    Esc,
    EscIntermediate,
    Csi,
    Osc,
    OscEsc,
}

/// The recorder state machine.
#[derive(Debug, Clone)]
pub struct OutputRecorder {
    phase: Phase,
    prompt: String,
    max_bytes: usize,
    escape: Escape,
    /// Parameter bytes of the CSI sequence being parsed (e.g. `2` in `ESC [ 2 K`).
    csi_parameters: Vec<u8>,
    pending_carriage_return: bool,
    /// The rest of the echoed command line, up to the first newline after Enter.
    skipping_echo: bool,
    finished: Vec<u8>,
    current: Vec<u8>,
}

impl Default for OutputRecorder {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_BYTES)
    }
}

const LF: u8 = 0x0A;
const CR: u8 = 0x0D;

impl OutputRecorder {
    /// 10 MiB.
    pub const DEFAULT_MAX_BYTES: usize = 10 * 1024 * 1024;

    /// A recorder that gives up after `max_bytes` of output.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            phase: Phase::Idle,
            prompt: String::new(),
            max_bytes,
            escape: Escape::None,
            csi_parameters: Vec::new(),
            pending_carriage_return: false,
            skipping_echo: false,
            finished: Vec::new(),
            current: Vec::new(),
        }
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The prompt taken by [`arm`](Self::arm).
    #[must_use]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Takes the line the output currently ends with as the prompt (an empty one never ends the capture by
    /// itself, only a manual stop does).
    pub fn arm(&mut self) {
        self.prompt = String::from_utf8_lossy(&self.current).into_owned();
        self.finished.clear();
        self.skipping_echo = false;
        self.phase = Phase::Armed;
    }

    /// Keyboard or paste input. Returns true when it started the capture (Enter while armed).
    pub fn user_input(&mut self, data: &[u8]) -> bool {
        if self.phase != Phase::Armed || !data.iter().any(|&b| b == CR || b == LF) {
            return false;
        }
        self.phase = Phase::Capturing;
        self.skipping_echo = true;
        true
    }

    /// Output from the remote side.
    pub fn output(&mut self, data: &[u8]) -> Event {
        if self.phase != Phase::Capturing {
            self.track(data);
            return Event::None;
        }
        for &byte in data {
            self.consume(byte);
        }
        if self.finished.len() + self.current.len() > self.max_bytes {
            self.reset();
            return Event::TooLarge;
        }
        if self.is_at_prompt() {
            Event::AtPrompt
        } else {
            Event::None
        }
    }

    /// The captured text without the closing prompt and the newline before it. A last line that doesn't
    /// end with the saved prompt is dropped too: after a manual stop it is usually a changed prompt (`cd`).
    pub fn finish(&mut self) -> String {
        let mut bytes = std::mem::take(&mut self.finished);
        if let Some(before_prompt) = self.text_before_prompt() {
            bytes.extend_from_slice(&before_prompt);
        }
        if bytes.last() == Some(&LF) {
            bytes.pop();
        }
        self.reset();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Back to idle without a result.
    pub fn cancel(&mut self) {
        self.reset();
    }

    /// Outside a capture only the last line matters (it becomes the prompt): skip to the last newline.
    fn track(&mut self, data: &[u8]) {
        let mut rest = data;
        if let Some(newline) = data.iter().rposition(|&b| b == LF) {
            self.current.clear();
            self.escape = Escape::None;
            self.pending_carriage_return = false;
            rest = &data[newline + 1..];
        }
        for &byte in rest {
            self.consume(byte);
        }
    }

    fn is_at_prompt(&self) -> bool {
        !self.skipping_echo && !self.pending_carriage_return && self.text_before_prompt().is_some()
    }

    /// The current, unfinished line minus the saved prompt at its end; `None` when it doesn't end with
    /// the prompt.
    fn text_before_prompt(&self) -> Option<Vec<u8>> {
        let prompt = self.prompt.trim_end().as_bytes();
        if prompt.is_empty() {
            return None;
        }
        let line = String::from_utf8_lossy(&self.current);
        let line = line.trim_end().as_bytes();
        line.strip_suffix(prompt).map(<[u8]>::to_vec)
    }

    fn consume(&mut self, byte: u8) {
        match self.escape {
            Escape::Esc => {
                self.escape = match byte {
                    b'[' => {
                        self.csi_parameters.clear();
                        Escape::Csi
                    }
                    b']' => Escape::Osc,
                    0x20..=0x2F => Escape::EscIntermediate,
                    _ => Escape::None,
                };
                return;
            }
            Escape::EscIntermediate => {
                if (0x30..=0x7E).contains(&byte) {
                    self.escape = Escape::None;
                }
                return;
            }
            Escape::Csi => {
                if (0x40..=0x7E).contains(&byte) {
                    self.escape = Escape::None;
                    if starts_new_line(byte, &self.csi_parameters) {
                        self.current.clear();
                    }
                } else {
                    self.csi_parameters.push(byte);
                }
                return;
            }
            Escape::Osc => {
                if byte == 0x07 {
                    self.escape = Escape::None;
                } else if byte == 0x1B {
                    self.escape = Escape::OscEsc;
                }
                return;
            }
            Escape::OscEsc => {
                self.escape = if byte == b'\\' { Escape::None } else { Escape::Osc };
                return;
            }
            Escape::None => {}
        }

        if self.pending_carriage_return && byte != LF {
            // A lone CR returns to the start of the line: what follows overwrites it (redraws, progress
            // bars).
            self.current.clear();
        }
        self.pending_carriage_return = false;

        match byte {
            0x1B => self.escape = Escape::Esc,
            CR => self.pending_carriage_return = true,
            LF => {
                if self.skipping_echo {
                    self.skipping_echo = false;
                } else if self.phase == Phase::Capturing {
                    self.finished.extend_from_slice(&self.current);
                    self.finished.push(LF);
                }
                self.current.clear();
            }
            0x08 => self.remove_last_character(),
            0x09 | 0x20..=0x7E | 0x80..=0xFF => self.current.push(byte),
            _ => {}
        }
    }

    fn remove_last_character(&mut self) {
        while self.current.last().is_some_and(|b| (0x80..=0xBF).contains(b)) {
            self.current.pop();
        }
        self.current.pop();
    }

    /// Back to idle. The current line stays: it is the prompt the next `arm()` takes.
    fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.escape = Escape::None;
        self.pending_carriage_return = false;
        self.skipping_echo = false;
        self.finished.clear();
    }
}

/// Cursor jumps and erases that leave nothing of the current line before the cursor: cursor position
/// (`H`, `f`; bash's Ctrl+L repaints the prompt after `ESC[H ESC[2J` without CR or LF), erase display
/// (`J`), erase to start / whole line (`1K`, `2K`) and column 1 (`G`). Erase to end of line (`K`) keeps it.
fn starts_new_line(final_byte: u8, parameters: &[u8]) -> bool {
    match final_byte {
        b'H' | b'f' | b'J' => true,
        b'K' => parameters == b"1" || parameters == b"2",
        b'G' => parameters.is_empty() || parameters == b"1",
        _ => false,
    }
}

/// `OutputRecorderTests` (29).
#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &str = "root@radius:~# ";

    /// A recorder that has seen `prompt`, been armed and got Enter after `command`'s echo.
    fn capturing_with(prompt: &str, command: &str, max_bytes: usize) -> OutputRecorder {
        let mut recorder = OutputRecorder::new(max_bytes);
        recorder.output(format!("last login\r\n{prompt}").as_bytes());
        recorder.arm();
        recorder.output(command.as_bytes());
        assert!(recorder.user_input(b"\r"));
        recorder
    }

    fn capturing() -> OutputRecorder {
        capturing_with(PROMPT, "cat radiusd.conf", OutputRecorder::DEFAULT_MAX_BYTES)
    }

    fn command(command: &str) -> OutputRecorder {
        capturing_with(PROMPT, command, OutputRecorder::DEFAULT_MAX_BYTES)
    }

    fn out(recorder: &mut OutputRecorder, text: &str) -> Event {
        recorder.output(text.as_bytes())
    }

    // Basic flow

    #[test]
    fn captures_output_without_echo_and_prompt() {
        let mut r = capturing();
        assert_eq!(r.phase(), Phase::Capturing);
        assert_eq!(out(&mut r, "\r\nline 1\r\nline 2\r\n"), Event::None);
        assert_eq!(out(&mut r, PROMPT), Event::AtPrompt);
        assert_eq!(r.finish(), "line 1\nline 2");
        assert_eq!(r.phase(), Phase::Idle);
    }

    #[test]
    fn arm_takes_the_last_output_line_as_prompt() {
        let mut r = OutputRecorder::default();
        out(
            &mut r,
            &format!("motd\r\nmore\r\n\u{1b}]0;root@radius: ~\u{07}{PROMPT}"),
        );
        r.arm();
        assert_eq!(r.prompt(), PROMPT);
        assert_eq!(r.phase(), Phase::Armed);
    }

    #[test]
    fn echo_arriving_in_chunks_is_skipped() {
        let mut r = OutputRecorder::default();
        out(&mut r, PROMPT);
        r.arm();
        assert!(r.user_input(b"\r"));
        out(&mut r, "cat ");
        out(&mut r, "file\r");
        out(&mut r, "\nhello\r\n");
        out(&mut r, PROMPT);
        assert_eq!(r.finish(), "hello");
    }

    #[test]
    fn empty_output_gives_empty_text() {
        let mut r = command("true");
        assert_eq!(out(&mut r, &format!("\r\n{PROMPT}")), Event::AtPrompt);
        assert_eq!(r.finish(), "");
    }

    #[test]
    fn output_without_trailing_newline_keeps_text_before_prompt() {
        let mut r = command("printf abc");
        assert_eq!(out(&mut r, &format!("\r\nabc{PROMPT}")), Event::AtPrompt);
        assert_eq!(r.finish(), "abc");
    }

    // Arming and input

    #[test]
    fn input_before_enter_does_not_start_capture() {
        let mut r = OutputRecorder::default();
        out(&mut r, PROMPT);
        r.arm();
        assert!(!r.user_input(b"cat file"));
        assert_eq!(r.phase(), Phase::Armed);
        assert_eq!(out(&mut r, "cat file"), Event::None);
        assert!(r.user_input(b"\r"));
        assert_eq!(r.phase(), Phase::Capturing);
    }

    #[test]
    fn idle_recorder_ignores_enter() {
        let mut r = OutputRecorder::default();
        assert!(!r.user_input(b"\r"));
        assert_eq!(r.phase(), Phase::Idle);
    }

    #[test]
    fn output_while_armed_is_not_captured() {
        let mut r = OutputRecorder::default();
        out(&mut r, PROMPT);
        r.arm();
        assert_eq!(out(&mut r, &format!("background noise\r\n{PROMPT}")), Event::None);
        r.user_input(b"\r");
        out(&mut r, &format!("\r\nreal\r\n{PROMPT}"));
        assert_eq!(r.finish(), "real");
    }

    #[test]
    fn cancel_resets_to_idle() {
        let mut r = capturing();
        out(&mut r, "\r\npartial\r\n");
        r.cancel();
        assert_eq!(r.phase(), Phase::Idle);
        assert_eq!(out(&mut r, "more\r\n"), Event::None);
    }

    #[test]
    fn recorder_can_be_armed_again_after_finishing() {
        let mut r = capturing();
        out(&mut r, &format!("\r\nfirst\r\n{PROMPT}"));
        assert_eq!(r.finish(), "first");
        r.arm();
        assert_eq!(r.prompt(), PROMPT);
        r.user_input(b"\r");
        out(&mut r, &format!("\r\nsecond\r\n{PROMPT}"));
        assert_eq!(r.finish(), "second");
    }

    // Escape sequences and control characters

    #[test]
    fn colors_and_titles_are_stripped() {
        let mut r = capturing();
        out(
            &mut r,
            "\r\n\u{1b}[01;34mdir\u{1b}[0m \u{1b}]0;title\u{07}file\u{1b}]2;other\u{1b}\\\r\n",
        );
        out(&mut r, &format!("\u{1b}[?2004h{PROMPT}"));
        assert_eq!(r.finish(), "dir file");
    }

    #[test]
    fn escape_sequence_split_across_chunks_is_stripped() {
        let mut r = capturing();
        out(&mut r, "\r\nred: \u{1b}[3");
        out(&mut r, "1mtext\u{1b}");
        out(&mut r, &format!("[0m\r\n{PROMPT}"));
        assert_eq!(r.finish(), "red: text");
    }

    /// Ctrl+L / ⌘K: bash repaints the prompt after cursor home + erase display, without CR or LF.
    #[test]
    fn clear_screen_starts_a_new_line() {
        let mut r = OutputRecorder::default();
        out(&mut r, PROMPT);
        out(&mut r, &format!("\u{1b}[H\u{1b}[2J{PROMPT}"));
        r.arm();
        assert_eq!(r.prompt(), PROMPT);
    }

    #[test]
    fn cursor_jump_or_line_erase_starts_a_new_line() {
        for sequence in [
            "\u{1b}[H",
            "\u{1b}[5;1H",
            "\u{1b}[1;1f",
            "\u{1b}[2J",
            "\u{1b}[2K",
            "\u{1b}[1K",
            "\u{1b}[G",
            "\u{1b}[1G",
        ] {
            let mut r = capturing();
            out(&mut r, &format!("\r\nkeep\r\nold{sequence}new\r\n{PROMPT}"));
            assert_eq!(r.finish(), "keep\nnew", "{sequence:?}");
        }
    }

    #[test]
    fn erase_to_end_of_line_keeps_the_line() {
        let mut r = capturing();
        out(&mut r, &format!("\r\ntext\u{1b}[K\u{1b}[0K more\r\n{PROMPT}"));
        assert_eq!(r.finish(), "text more");
    }

    #[test]
    fn charset_selection_is_stripped() {
        let mut r = capturing();
        out(&mut r, &format!("\r\n\u{1b}(Bplain\r\n{PROMPT}"));
        assert_eq!(r.finish(), "plain");
    }

    #[test]
    fn lone_carriage_return_overwrites_the_line() {
        let mut r = capturing();
        out(&mut r, &format!("\r\n 10%\r 50%\r100%\r\ndone\r\n{PROMPT}"));
        assert_eq!(r.finish(), "100%\ndone");
    }

    #[test]
    fn carriage_return_split_from_line_feed_is_a_newline() {
        let mut r = capturing();
        out(&mut r, "\r\nfirst\r");
        out(&mut r, &format!("\nsecond\r\n{PROMPT}"));
        assert_eq!(r.finish(), "first\nsecond");
    }

    #[test]
    fn backspace_deletes_one_character() {
        let mut r = capturing();
        out(&mut r, &format!("\r\nabcx\u{08}d árvíz\u{08}\r\n{PROMPT}"));
        assert_eq!(r.finish(), "abcd árví");
    }

    #[test]
    fn tabs_and_unicode_are_kept() {
        let mut r = capturing();
        out(&mut r, &format!("\r\nkey\tvalue ő\r\n{PROMPT}"));
        assert_eq!(r.finish(), "key\tvalue ő");
    }

    #[test]
    fn other_control_bytes_are_dropped() {
        let mut r = capturing();
        out(&mut r, &format!("\r\na\u{07}b\u{00}c\r\n{PROMPT}"));
        assert_eq!(r.finish(), "abc");
    }

    // Prompt detection

    #[test]
    fn prompt_inside_the_output_does_not_end_the_capture() {
        let mut r = capturing();
        assert_eq!(out(&mut r, &format!("\r\n{PROMPT}echo old\r\nnext")), Event::None);
        out(&mut r, &format!("\r\n{PROMPT}"));
        assert_eq!(r.finish(), format!("{PROMPT}echo old\nnext"));
    }

    #[test]
    fn prompt_arriving_in_chunks_is_detected() {
        let mut r = capturing();
        out(&mut r, "\r\nout\r\n");
        assert_eq!(out(&mut r, "root@ra"), Event::None);
        assert_eq!(out(&mut r, "dius:~# "), Event::AtPrompt);
        assert_eq!(r.finish(), "out");
    }

    #[test]
    fn trailing_spaces_do_not_matter() {
        let mut r = capturing_with("$   ", "cat radiusd.conf", OutputRecorder::DEFAULT_MAX_BYTES);
        assert_eq!(out(&mut r, "\r\nout\r\n$"), Event::AtPrompt);
        assert_eq!(r.finish(), "out");
    }

    #[test]
    fn pending_carriage_return_is_not_at_prompt() {
        let mut r = capturing();
        assert_eq!(out(&mut r, &format!("\r\nout\r\n{PROMPT}\r")), Event::None);
    }

    #[test]
    fn mikrotik_prompt_with_colors_is_detected() {
        let prompt = "\u{1b}[m\u{1b}[36m[admin@MikroTik] > \u{1b}[m";
        let mut r = OutputRecorder::default();
        out(&mut r, &format!("\r\n\r\n{prompt}"));
        r.arm();
        assert_eq!(r.prompt(), "[admin@MikroTik] > ");
        out(&mut r, "/export");
        r.user_input(b"\r");
        out(
            &mut r,
            "\r\n# 2026-10-15 by RouterOS 7.16\r\n\u{1b}[32m/interface bridge\u{1b}[m\r\nadd name=bridge\r\n",
        );
        assert_eq!(out(&mut r, &format!("\r\r\n{prompt}")), Event::AtPrompt);
        assert_eq!(
            r.finish(),
            "# 2026-10-15 by RouterOS 7.16\n/interface bridge\nadd name=bridge\n"
        );
    }

    #[test]
    fn empty_prompt_never_ends_the_capture() {
        let mut r = OutputRecorder::default();
        r.arm();
        r.user_input(b"\r");
        assert_eq!(out(&mut r, "\r\nout\r\n"), Event::None);
        assert_eq!(r.finish(), "out");
    }

    #[test]
    fn manual_stop_drops_a_changed_prompt() {
        let mut r = command("cd /etc && ls");
        assert_eq!(out(&mut r, "\r\nhosts\r\nroot@radius:/etc# "), Event::None);
        assert_eq!(r.finish(), "hosts");
    }

    // Size limit

    #[test]
    fn large_output_is_refused() {
        let mut r = capturing_with(PROMPT, "cat radiusd.conf", 10);
        assert_eq!(out(&mut r, "\r\n0123456789"), Event::None);
        assert_eq!(out(&mut r, "A"), Event::TooLarge);
        assert_eq!(r.phase(), Phase::Idle);
    }
}
