//! One interactive `ssh` (or the local shell, [`SessionKind::Local`]) in a PTY (portable-pty): input,
//! resize, a coalescing output pump to a [`Sink`], the stream analyzer, the root warning, "copy next command
//! output" and the exit code.
//!
//! Threads per session: the reader (PTY → analyzer → pending buffer), the flusher (pending buffer → sink,
//! every [`FLUSH_INTERVAL`] or at [`FLUSH_BYTES`], so the webview never gets one message per read), and
//! the waiter (child exit → final flush → [`SessionEvent::Exit`]).

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use portable_pty::{Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use crate::analyzer::{self, Analyzer, Change, ClearAction};
use crate::known_hosts::ChangedHostKey;
use crate::recorder::{Event as RecorderEvent, OutputRecorder, Phase};

/// The longest output waits before it is sent.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(6);
/// Output is sent at once when this much is pending.
pub const FLUSH_BYTES: usize = 256 * 1024;
/// The reader stops reading (and the PTY applies back-pressure to `ssh`) above this much unsent output.
const MAX_PENDING: usize = 8 * 1024 * 1024;
/// Quiet time after the prompt reappears before a capture counts as finished (Swift `promptSettleTime`).
pub const PROMPT_SETTLE_TIME: Duration = Duration::from_millis(150);

/// How a "copy next command output" recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordOutcome {
    /// The text was captured.
    Copied,
    /// The command printed nothing.
    Empty,
    /// A full-screen program started; the recording stopped.
    AlternateScreen,
    /// More than 10 MB; the recording stopped.
    TooLarge,
}

/// What a session reports besides its output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    /// The terminal title changed.
    Title {
        /// The new title.
        title: String,
    },
    /// A full-screen program entered or left the alternate screen.
    AltScreen {
        /// On the alternate screen now.
        active: bool,
    },
    /// The remote side switched bracketed paste.
    BracketedPaste {
        /// Bracketed paste is on.
        enabled: bool,
    },
    /// The root warning; sent at start and on every change.
    Root {
        /// The session currently runs as root.
        root: bool,
    },
    /// The recorder changed phase.
    Recording {
        /// The new phase.
        phase: Phase,
    },
    /// A recording ended with a result (not when cancelled).
    Recorded {
        /// How it ended.
        outcome: RecordOutcome,
        /// The captured text (empty unless [`RecordOutcome::Copied`]).
        text: String,
    },
    /// `ssh` exited; the last event of a run.
    Exit {
        /// The normalised exit code, `None` when unknown.
        code: Option<i64>,
        /// Exit 255 after `ssh`'s "Permission denied (…)" line.
        auth_failed: bool,
        /// Exit 255 after `ssh` refused a changed host key: its report (left out of the JSON when
        /// `None`).
        #[serde(skip_serializing_if = "Option::is_none")]
        host_key_changed: Option<ChangedHostKey>,
    },
}

/// Where a session's output and events go. Called from the session's threads.
pub trait Sink: Send + Sync + 'static {
    /// A coalesced chunk of PTY output.
    fn output(&self, bytes: Vec<u8>);
    /// An event.
    fn event(&self, event: SessionEvent);
}

/// What runs in the PTY.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionKind {
    /// The system `ssh`: its auth failure and changed host key reports count at exit 255.
    #[default]
    Ssh,
    /// The local shell: no `ssh` reports, and the askpass variables ([`crate::local::REMOVED_ENV`]) are
    /// removed from the inherited environment.
    Local,
}

/// What to run.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// `ssh`, or the local shell.
    pub kind: SessionKind,
    /// The `ssh` (or shell) executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Extra environment (the askpass variables); `TERM=xterm-256color` is always set.
    pub env: Vec<(String, OsString)>,
    /// The resolved login user, for the root warning.
    pub login_user: String,
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Terminal {
    analyzer: Analyzer,
    recorder: OutputRecorder,
    root: bool,
    login_user: String,
    kind: SessionKind,
}

#[derive(Default)]
struct Pending {
    buf: Vec<u8>,
    done: bool,
}

struct Shared {
    sink: Arc<dyn Sink>,
    terminal: Mutex<Terminal>,
    running: AtomicBool,
    /// Bumped by every output chunk while capturing, so a pending prompt check can tell it is stale.
    capture_generation: AtomicU64,
    pending: Mutex<Pending>,
    pending_changed: Condvar,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
}

impl Shared {
    fn emit(&self, events: Vec<SessionEvent>) {
        for event in events {
            self.sink.event(event);
        }
    }

    fn root_event(terminal: &mut Terminal, running: bool, events: &mut Vec<SessionEvent>) {
        let root = running && analyzer::root::is_root(&terminal.login_user, terminal.analyzer.shell_title());
        if root != terminal.root {
            terminal.root = root;
            events.push(SessionEvent::Root { root });
        }
    }

    /// Runs the analyzer and the recorder over a chunk (Swift `titleChanged` / `recordOutput`).
    fn analyze(self: &Arc<Self>, chunk: &[u8]) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        let mut terminal = lock(&self.terminal);
        for change in terminal.analyzer.feed(chunk) {
            events.push(match change {
                Change::Title(title) => SessionEvent::Title { title },
                Change::AltScreen(active) => SessionEvent::AltScreen { active },
                Change::BracketedPaste(enabled) => SessionEvent::BracketedPaste { enabled },
            });
        }
        Self::root_event(&mut terminal, self.running.load(Ordering::SeqCst), &mut events);

        let was_capturing = terminal.recorder.phase() == Phase::Capturing;
        let result = terminal.recorder.output(chunk);
        if !was_capturing {
            return events;
        }
        let generation = self.capture_generation.fetch_add(1, Ordering::SeqCst) + 1;
        if terminal.analyzer.alt_screen() {
            terminal.recorder.cancel();
            events.push(SessionEvent::Recording { phase: Phase::Idle });
            events.push(recorded(RecordOutcome::AlternateScreen, String::new()));
            return events;
        }
        match result {
            RecorderEvent::None => {}
            RecorderEvent::TooLarge => {
                terminal.recorder.cancel();
                events.push(SessionEvent::Recording { phase: Phase::Idle });
                events.push(recorded(RecordOutcome::TooLarge, String::new()));
            }
            RecorderEvent::AtPrompt => {
                let shared = Arc::downgrade(self);
                std::thread::spawn(move || {
                    std::thread::sleep(PROMPT_SETTLE_TIME);
                    let Some(shared) = shared.upgrade() else { return };
                    if shared.capture_generation.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let events = {
                        let mut terminal = lock(&shared.terminal);
                        if terminal.recorder.phase() != Phase::Capturing {
                            return;
                        }
                        finish_recording(&mut terminal.recorder)
                    };
                    shared.emit(events);
                });
            }
        }
        events
    }
}

fn recorded(outcome: RecordOutcome, text: String) -> SessionEvent {
    SessionEvent::Recorded { outcome, text }
}

fn finish_recording(recorder: &mut OutputRecorder) -> Vec<SessionEvent> {
    let text = recorder.finish();
    let outcome = if text.is_empty() {
        RecordOutcome::Empty
    } else {
        RecordOutcome::Copied
    };
    vec![
        SessionEvent::Recording { phase: Phase::Idle },
        recorded(outcome, text),
    ]
}

/// A running (or exited) `ssh` in a PTY. Dropping it kills the process.
pub struct SshSession {
    shared: Arc<Shared>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl SshSession {
    /// Spawns `spec` in a new PTY and starts the reader, flusher and waiter threads. Sends the initial
    /// [`SessionEvent::Root`] when the login user is root.
    ///
    /// # Errors
    /// When the PTY can't be opened or the program can't be started.
    pub fn spawn(spec: SpawnSpec, sink: Arc<dyn Sink>) -> io::Result<Self> {
        let pty = native_pty_system()
            .openpty(size(spec.cols, spec.rows))
            .map_err(io::Error::other)?;
        let mut command = CommandBuilder::new(&spec.program);
        command.args(&spec.args);
        if spec.kind == SessionKind::Local {
            for key in crate::local::REMOVED_ENV {
                command.env_remove(key);
            }
        }
        command.env("TERM", "xterm-256color");
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        if let Some(home) = dirs::home_dir() {
            command.cwd(home);
        }
        let mut child = pty.slave.spawn_command(command).map_err(io::Error::other)?;
        drop(pty.slave);
        let killer = child.clone_killer();
        let reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pty.master.take_writer().map_err(io::Error::other)?;

        let shared = Arc::new(Shared {
            sink,
            terminal: Mutex::new(Terminal {
                analyzer: Analyzer::default(),
                recorder: OutputRecorder::default(),
                root: false,
                login_user: spec.login_user,
                kind: spec.kind,
            }),
            running: AtomicBool::new(true),
            capture_generation: AtomicU64::new(0),
            pending: Mutex::default(),
            pending_changed: Condvar::new(),
            writer: Mutex::new(Some(writer)),
            master: Mutex::new(Some(pty.master)),
        });

        let mut initial = Vec::new();
        Shared::root_event(&mut lock(&shared.terminal), true, &mut initial);
        shared.emit(initial);

        let (reader_done_tx, reader_done) = mpsc::channel();
        let reader_thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("ssh-reader".into())
                .spawn(move || {
                    read_loop(&shared, reader);
                    let _ = reader_done_tx.send(());
                })?
        };
        let flusher = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("ssh-flusher".into())
                .spawn(move || flush_loop(&shared))?
        };
        {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("ssh-waiter".into())
                .spawn(move || {
                    let status = wait_loop(&shared, &mut child, &reader_done);
                    let _ = reader_thread.join();
                    let _ = flusher.join();
                    finish(&shared, status);
                })?;
        }
        Ok(Self {
            shared,
            killer: Mutex::new(killer),
        })
    }

    /// The process still runs.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
    }

    /// Writes user input (keys, paste) to the process. Enter while recording is armed starts the capture.
    ///
    /// # Errors
    /// `BrokenPipe` after the process exited, or the write error.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        if !self.is_running() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let started = lock(&self.shared.terminal).recorder.user_input(bytes);
        if started {
            self.shared.sink.event(SessionEvent::Recording {
                phase: Phase::Capturing,
            });
        }
        let mut writer = lock(&self.shared.writer);
        let writer = writer.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Resizes the PTY.
    ///
    /// # Errors
    /// When the process exited or the OS refuses.
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let master = lock(&self.shared.master);
        let master = master.as_ref().ok_or(io::ErrorKind::BrokenPipe)?;
        master.resize(size(cols, rows)).map_err(io::Error::other)
    }

    /// Ends the process; the waiter then reports [`SessionEvent::Exit`].
    pub fn kill(&self) {
        if self.is_running() {
            let _ = lock(&self.killer).kill();
        }
    }

    /// A full-screen program owns the alternate screen.
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        lock(&self.shared.terminal).analyzer.alt_screen()
    }

    /// The remote side asked for bracketed paste.
    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        lock(&self.shared.terminal).analyzer.bracketed_paste()
    }

    /// The last terminal title.
    #[must_use]
    pub fn title(&self) -> Option<String> {
        lock(&self.shared.terminal).analyzer.title().map(str::to_owned)
    }

    /// The shell's working directory, when the shell reported it (OSC 7 or a `user@host: dir` title).
    #[must_use]
    pub fn directory(&self) -> Option<String> {
        lock(&self.shared.terminal)
            .analyzer
            .directory()
            .map(str::to_owned)
    }

    /// The session currently runs as root.
    #[must_use]
    pub fn is_root(&self) -> bool {
        lock(&self.shared.terminal).root
    }

    /// What "Clear terminal" should do now. The UI clears xterm.js and, for
    /// [`ClearAction::LocalAndRedraw`], sends Ctrl+L as input.
    #[must_use]
    pub fn clear_action(&self) -> ClearAction {
        analyzer::clear_action(self.alt_screen(), self.is_running())
    }

    /// The recorder's phase.
    #[must_use]
    pub fn recording_phase(&self) -> Phase {
        lock(&self.shared.terminal).recorder.phase()
    }

    /// The record button (Swift `toggleRecording`): arms when idle (only while running), cancels while
    /// waiting for Enter, and finishes (with [`SessionEvent::Recorded`]) while capturing. Returns the new
    /// phase.
    pub fn toggle_recording(&self) -> Phase {
        let events = {
            let mut terminal = lock(&self.shared.terminal);
            match terminal.recorder.phase() {
                Phase::Idle if self.is_running() => {
                    terminal.recorder.arm();
                    vec![SessionEvent::Recording { phase: Phase::Armed }]
                }
                Phase::Idle => Vec::new(),
                Phase::Armed => {
                    terminal.recorder.cancel();
                    vec![SessionEvent::Recording { phase: Phase::Idle }]
                }
                Phase::Capturing => {
                    self.shared.capture_generation.fetch_add(1, Ordering::SeqCst);
                    finish_recording(&mut terminal.recorder)
                }
            }
        };
        self.shared.emit(events);
        self.recording_phase()
    }

    /// Esc while recording: back to idle without a result.
    pub fn cancel_recording(&self) {
        let cancelled = {
            let mut terminal = lock(&self.shared.terminal);
            let active = terminal.recorder.phase() != Phase::Idle;
            terminal.recorder.cancel();
            active
        };
        if cancelled {
            self.shared.capture_generation.fetch_add(1, Ordering::SeqCst);
            self.shared
                .sink
                .event(SessionEvent::Recording { phase: Phase::Idle });
        }
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.kill();
    }
}

fn read_loop(shared: &Arc<Shared>, mut reader: Box<dyn Read + Send>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let chunk = &buf[..n];
        let events = shared.analyze(chunk);
        shared.emit(events);
        let mut pending = lock(&shared.pending);
        while pending.buf.len() >= MAX_PENDING && !pending.done {
            pending = shared
                .pending_changed
                .wait(pending)
                .unwrap_or_else(PoisonError::into_inner);
        }
        pending.buf.extend_from_slice(chunk);
        shared.pending_changed.notify_all();
    }
    lock(&shared.pending).done = true;
    shared.pending_changed.notify_all();
}

fn flush_loop(shared: &Shared) {
    loop {
        let chunk = {
            let mut pending = shared
                .pending_changed
                .wait_while(lock(&shared.pending), |p| p.buf.is_empty() && !p.done)
                .unwrap_or_else(PoisonError::into_inner);
            if pending.buf.is_empty() && pending.done {
                break;
            }
            if !pending.done && pending.buf.len() < FLUSH_BYTES {
                pending = shared
                    .pending_changed
                    .wait_timeout_while(pending, FLUSH_INTERVAL, |p| p.buf.len() < FLUSH_BYTES && !p.done)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            let chunk = std::mem::take(&mut pending.buf);
            shared.pending_changed.notify_all();
            chunk
        };
        shared.sink.output(chunk);
    }
}

/// Waits for the process, then for the reader to drain. `ConPTY` only ends the reader once the
/// pseudo-console is closed, so the master and writer are dropped if the reader lingers.
fn wait_loop(
    shared: &Shared,
    child: &mut Box<dyn Child + Send + Sync>,
    reader_done: &mpsc::Receiver<()>,
) -> Option<ExitStatus> {
    let status = child.wait().ok();
    if reader_done.recv_timeout(Duration::from_millis(500)).is_err() {
        lock(&shared.writer).take();
        lock(&shared.master).take();
        let _ = reader_done.recv_timeout(Duration::from_secs(2));
    }
    status
}

fn finish(shared: &Shared, status: Option<ExitStatus>) {
    shared.running.store(false, Ordering::SeqCst);
    lock(&shared.writer).take();
    lock(&shared.master).take();
    let code = analyzer::normalize_exit_code(status.map(|s| i64::from(s.exit_code())));
    let mut events = Vec::new();
    let (auth_failed, host_key_changed) = {
        let mut terminal = lock(&shared.terminal);
        if terminal.recorder.phase() != Phase::Idle {
            terminal.recorder.cancel();
            events.push(SessionEvent::Recording { phase: Phase::Idle });
        }
        Shared::root_event(&mut terminal, false, &mut events);
        let refused = code == Some(255) && terminal.kind == SessionKind::Ssh;
        (
            refused && terminal.analyzer.auth_failed(),
            refused.then(|| terminal.analyzer.host_key_changed()).flatten(),
        )
    };
    events.push(SessionEvent::Exit {
        code,
        auth_failed,
        host_key_changed,
    });
    shared.emit(events);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Collect {
        output: Mutex<Vec<u8>>,
        events: Mutex<Vec<SessionEvent>>,
        changed: Condvar,
        output_changed: Condvar,
        messages: AtomicU64,
    }

    impl Sink for Collect {
        fn output(&self, bytes: Vec<u8>) {
            self.messages.fetch_add(1, Ordering::SeqCst);
            lock(&self.output).extend(bytes);
            self.output_changed.notify_all();
        }
        fn event(&self, event: SessionEvent) {
            lock(&self.events).push(event);
            self.changed.notify_all();
        }
    }

    impl Collect {
        fn wait_exit(&self) -> SessionEvent {
            let events = self
                .changed
                .wait_timeout_while(lock(&self.events), Duration::from_secs(10), |e| {
                    !e.iter().any(|e| matches!(e, SessionEvent::Exit { .. }))
                })
                .unwrap()
                .0;
            events.last().cloned().unwrap()
        }
    }

    fn sh(script: &str, login_user: &str) -> SpawnSpec {
        SpawnSpec {
            kind: SessionKind::Ssh,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![("RD_TEST_VALUE".into(), "from-env".into())],
            login_user: login_user.into(),
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn runs_a_program_and_reports_output_events_and_exit() {
        let sink = Arc::new(Collect::default());
        let script = r#"printf '\033]0;root@box: ~\007\033[?2004h'; echo "$TERM $RD_TEST_VALUE"; exit 3"#;
        let session = SshSession::spawn(sh(script, "admin"), sink.clone()).unwrap();
        let exit = sink.wait_exit();
        assert_eq!(
            exit,
            SessionEvent::Exit {
                code: Some(3),
                auth_failed: false,
                host_key_changed: None,
            }
        );
        let output = String::from_utf8_lossy(&lock(&sink.output)).into_owned();
        assert!(output.contains("xterm-256color from-env"), "{output:?}");
        let events = lock(&sink.events).clone();
        assert!(events.contains(&SessionEvent::Title {
            title: "root@box: ~".into()
        }));
        assert!(events.contains(&SessionEvent::BracketedPaste { enabled: true }));
        let roots: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::Root { root } => Some(*root),
                _ => None,
            })
            .collect();
        assert_eq!(roots, [true, false], "root from the title, cleared on exit");
        assert!(!session.is_running());
        assert!(session.bracketed_paste());
        assert_eq!(session.clear_action(), ClearAction::LocalOnly);
        assert!(session.write(b"x").is_err());
    }

    #[test]
    fn root_login_is_reported_at_start() {
        let sink = Arc::new(Collect::default());
        let _session = SshSession::spawn(sh("exit 0", "root"), sink.clone()).unwrap();
        sink.wait_exit();
        let events = lock(&sink.events).clone();
        assert_eq!(events.first(), Some(&SessionEvent::Root { root: true }));
        assert_eq!(
            events.last(),
            Some(&SessionEvent::Exit {
                code: Some(0),
                auth_failed: false,
                host_key_changed: None,
            })
        );
    }

    #[test]
    fn auth_failure_needs_exit_255_and_the_permission_denied_line() {
        let sink = Arc::new(Collect::default());
        let script = "echo 'u@h: Permission denied (publickey,password).'; exit 255";
        let _session = SshSession::spawn(sh(script, "u"), sink.clone()).unwrap();
        assert_eq!(
            sink.wait_exit(),
            SessionEvent::Exit {
                code: Some(255),
                auth_failed: true,
                host_key_changed: None,
            }
        );
        let sink = Arc::new(Collect::default());
        let _session = SshSession::spawn(sh("exit 255", "u"), sink.clone()).unwrap();
        assert_eq!(
            sink.wait_exit(),
            SessionEvent::Exit {
                code: Some(255),
                auth_failed: false,
                host_key_changed: None,
            }
        );
    }

    #[test]
    fn coalesces_bulk_output_into_few_messages() {
        let sink = Arc::new(Collect::default());
        // 4 MB, read from the PTY in many small chunks.
        let script = "head -c 4000000 /dev/zero | tr '\\0' x";
        let _session = SshSession::spawn(sh(script, "u"), sink.clone()).unwrap();
        sink.wait_exit();
        let output = lock(&sink.output).clone();
        let total = String::from_utf8_lossy(&output).matches('x').count();
        assert_eq!(total, 4_000_000);
        let messages = sink.messages.load(Ordering::SeqCst);
        assert!(messages < 1000, "{messages} messages");
    }

    #[test]
    fn input_resize_kill_and_recording() {
        let sink = Arc::new(Collect::default());
        let script = "printf 'P> '; while read -r line; do \
                      if [ \"$line\" = size ]; then stty size; else echo \"got:$line\"; fi; printf 'P> '; done";
        let session = SshSession::spawn(sh(script, "u"), sink.clone()).unwrap();
        let wait_output = |needle: &str| {
            let output = sink
                .output_changed
                .wait_timeout_while(lock(&sink.output), Duration::from_secs(10), |o| {
                    !String::from_utf8_lossy(o).contains(needle)
                })
                .unwrap()
                .0;
            assert!(String::from_utf8_lossy(&output).contains(needle), "{needle}");
        };
        wait_output("P> ");
        session.resize(100, 30).unwrap();
        session.write(b"size\r").unwrap();
        wait_output("30 100");

        assert_eq!(session.toggle_recording(), Phase::Armed);
        session.write(b"hello\r").unwrap();
        assert_eq!(session.recording_phase(), Phase::Capturing);
        let events = sink
            .changed
            .wait_timeout_while(lock(&sink.events), Duration::from_secs(10), |e| {
                !e.iter().any(|e| matches!(e, SessionEvent::Recorded { .. }))
            })
            .unwrap()
            .0
            .clone();
        assert!(events.contains(&SessionEvent::Recorded {
            outcome: RecordOutcome::Copied,
            text: "got:hello".into()
        }));
        assert!(events.contains(&SessionEvent::Recording { phase: Phase::Armed }));
        assert!(events.contains(&SessionEvent::Recording {
            phase: Phase::Capturing
        }));

        assert_eq!(session.toggle_recording(), Phase::Armed);
        session.cancel_recording();
        assert_eq!(session.recording_phase(), Phase::Idle);

        assert_eq!(session.clear_action(), ClearAction::LocalAndRedraw);
        session.kill();
        assert!(matches!(sink.wait_exit(), SessionEvent::Exit { .. }));
        assert!(!session.is_running());
        assert_eq!(session.toggle_recording(), Phase::Idle, "no arming after exit");
    }

    #[test]
    fn changed_host_key_report_needs_exit_255() {
        let report = "printf '@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\\n\
            The fingerprint for the RSA key sent by the remote host is\\nSHA256:new.\\n\
            Offending RSA key in /tmp/kh:2\\n\
            Host key for [h.example.com]:2222 has changed and you have requested strict checking.\\n\
            Host key verification failed.\\n'";
        let expected = ChangedHostKey {
            host: "[h.example.com]:2222".into(),
            key_type: "RSA".into(),
            fingerprint: "SHA256:new".into(),
            file: "/tmp/kh".into(),
            line: 2,
        };
        let sink = Arc::new(Collect::default());
        let _session = SshSession::spawn(sh(&format!("{report}; exit 255"), "u"), sink.clone()).unwrap();
        assert_eq!(
            sink.wait_exit(),
            SessionEvent::Exit {
                code: Some(255),
                auth_failed: false,
                host_key_changed: Some(expected),
            }
        );
        let sink = Arc::new(Collect::default());
        let _session = SshSession::spawn(sh(&format!("{report}; exit 1"), "u"), sink.clone()).unwrap();
        assert!(matches!(
            sink.wait_exit(),
            SessionEvent::Exit {
                host_key_changed: None,
                ..
            }
        ));
    }

    #[test]
    fn local_sessions_skip_the_ssh_reports() {
        let script = "echo 'u@h: Permission denied (publickey).'; \
                      echo '@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @'; exit 255";
        let sink = Arc::new(Collect::default());
        let spec = SpawnSpec {
            kind: SessionKind::Local,
            ..sh(script, "u")
        };
        let _session = SshSession::spawn(spec, sink.clone()).unwrap();
        assert_eq!(
            sink.wait_exit(),
            SessionEvent::Exit {
                code: Some(255),
                auth_failed: false,
                host_key_changed: None,
            }
        );
    }

    #[test]
    fn the_local_shell_runs_commands() {
        let shell = crate::local::local_shell();
        let sink = Arc::new(Collect::default());
        let spec = SpawnSpec {
            kind: SessionKind::Local,
            program: shell.program,
            args: shell.args,
            env: Vec::new(),
            login_user: crate::local::login_user(|n| std::env::var_os(n)),
            cols: 80,
            rows: 24,
        };
        let session = SshSession::spawn(spec, sink.clone()).unwrap();
        // The arithmetic keeps the typed echo of the command from matching.
        session.write(b"echo hello-$((40+2))\r").unwrap();
        let output = sink
            .output_changed
            .wait_timeout_while(lock(&sink.output), Duration::from_secs(30), |o| {
                !String::from_utf8_lossy(o).contains("hello-42")
            })
            .unwrap()
            .0
            .clone();
        assert!(
            String::from_utf8_lossy(&output).contains("hello-42"),
            "{output:?}"
        );
        session.write(b"exit 0\r").unwrap();
        assert!(matches!(
            sink.wait_exit(),
            SessionEvent::Exit {
                code: Some(0),
                auth_failed: false,
                host_key_changed: None,
            }
        ));
    }

    #[test]
    fn events_serialize_for_the_webview() {
        let json = |e: SessionEvent| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(SessionEvent::Exit {
                code: Some(255),
                auth_failed: true,
                host_key_changed: None,
            }),
            serde_json::json!({"kind": "exit", "code": 255, "authFailed": true})
        );
        assert_eq!(
            json(SessionEvent::Exit {
                code: Some(255),
                auth_failed: false,
                host_key_changed: Some(ChangedHostKey {
                    host: "h".into(),
                    key_type: "ED25519".into(),
                    fingerprint: "SHA256:x".into(),
                    file: "/kh".into(),
                    line: 3,
                }),
            }),
            serde_json::json!({"kind": "exit", "code": 255, "authFailed": false, "hostKeyChanged": {
                "host": "h", "keyType": "ED25519", "fingerprint": "SHA256:x", "file": "/kh", "line": 3
            }})
        );
        assert_eq!(
            json(SessionEvent::AltScreen { active: true }),
            serde_json::json!({"kind": "altScreen", "active": true})
        );
        assert_eq!(
            json(SessionEvent::Recorded {
                outcome: RecordOutcome::AlternateScreen,
                text: String::new()
            }),
            serde_json::json!({"kind": "recorded", "outcome": "alternateScreen", "text": ""})
        );
        assert_eq!(
            json(SessionEvent::Recording {
                phase: Phase::Capturing
            }),
            serde_json::json!({"kind": "recording", "phase": "capturing"})
        );
    }
}
