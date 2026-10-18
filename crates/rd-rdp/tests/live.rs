//! End-to-end checks of the session engine against real RDP servers. Both are skipped (they pass
//! without doing anything) unless their environment is set:
//!
//! - `RD_TEST_XRDP=127.0.0.1:3390`: the lab's xrdp (`docker compose -f tests/lab/docker-compose.yml up -d
//!   xrdp`, user `tester` / `tester-pass`, TLS without NLA): connect, certificate prompt and store,
//!   frames, resize, typing, disconnect. With `RD_TEST_XRDP_CONTAINER=lab-xrdp-1` the clipboard round
//!   trip is checked too, through `docker exec … xclip` in the session's X display, and files copied in
//!   the session (`text/uri-list`) are downloaded through the file clipboard. Local files are announced
//!   and xrdp takes their list; their contents can't be checked here (xrdp serves them through FUSE,
//!   which the container lacks), that needs a Windows host or a manual check.
//! - `RD_TEST_RDP_HOST`, `RD_TEST_RDP_USER`, `RD_TEST_RDP_PASS` (optional `RD_TEST_RDP_DOMAIN`,
//!   `RD_TEST_RDP_PORT`): a Windows host with NLA: connect, a frame, a resize. Nothing about the host
//!   or the credentials is printed. With `RD_TEST_RDP_TYPING=1` also type clipboard into Notepad there,
//!   read back through the clipboard.

use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use rd_rdp::frame::HEADER_LEN;
use rd_rdp::known_hosts::{KnownHosts, Verdict};
use rd_rdp::{
    CertificateChoice, CertificateRequest, Credentials, CredentialsRequest, InputEvent, RdpConfig, RdpEvent,
    RdpHandler, RdpSession,
};
use zeroize::Zeroizing;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Records what the session reports; acknowledges every frame at once (a page that paints instantly).
struct Recorder {
    session: OnceLock<Weak<RdpSession>>,
    state: Mutex<State>,
    changed: Condvar,
    certificate_answer: CertificateChoice,
    credentials_answers: Mutex<Vec<Credentials>>,
}

#[derive(Default)]
struct State {
    events: Vec<RdpEvent>,
    /// (seq, width, height, rectangle count)
    frames: Vec<(u32, u16, u16, u16)>,
    certificates: Vec<CertificateRequest>,
    credentials: Vec<CredentialsRequest>,
}

impl Recorder {
    fn new(certificate_answer: CertificateChoice) -> Arc<Self> {
        Arc::new(Self {
            session: OnceLock::new(),
            state: Mutex::default(),
            changed: Condvar::new(),
            certificate_answer,
            credentials_answers: Mutex::new(Vec::new()),
        })
    }

    fn wait<T>(&self, what: &str, found: impl Fn(&State) -> Option<T>) -> T {
        let deadline = Instant::now() + TIMEOUT;
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(value) = found(&state) {
                return value;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for {what}; events: {:?}",
                state.events
            );
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    fn connected(&self) -> (u16, u16) {
        self.wait("connected", |s| {
            s.events.iter().find_map(|e| match e {
                RdpEvent::Connected { width, height } => Some((*width, *height)),
                _ => None,
            })
        })
    }

    fn frames(&self) -> usize {
        self.state.lock().unwrap().frames.len()
    }
}

impl RdpHandler for Recorder {
    fn frame(&self, frame: Vec<u8>) {
        assert!(frame.len() >= HEADER_LEN);
        let u16_at = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
        let seq = u32::from_le_bytes(frame[0..4].try_into().unwrap());
        self.state
            .lock()
            .unwrap()
            .frames
            .push((seq, u16_at(4), u16_at(6), u16_at(8)));
        self.changed.notify_all();
        if let Some(session) = self.session.get().and_then(Weak::upgrade) {
            session.ack_frame();
        }
    }

    fn event(&self, event: RdpEvent) {
        self.state.lock().unwrap().events.push(event);
        self.changed.notify_all();
    }

    fn certificate(&self, request: CertificateRequest) -> CertificateChoice {
        self.state.lock().unwrap().certificates.push(request);
        self.changed.notify_all();
        self.certificate_answer
    }

    fn credentials(&self, request: CredentialsRequest) -> Option<Credentials> {
        self.state.lock().unwrap().credentials.push(request);
        self.changed.notify_all();
        let mut answers = self.credentials_answers.lock().unwrap();
        (!answers.is_empty()).then(|| answers.remove(0))
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn config(host: &str, port: u16, user: &str, password: &str, store: &std::path::Path) -> RdpConfig {
    RdpConfig {
        host: host.to_owned(),
        port,
        username: user.to_owned(),
        domain: String::new(),
        password: Zeroizing::new(password.to_owned()),
        admin_session: false,
        width: 1024,
        height: 768,
        client_name: "rd-rdp-test".to_owned(),
        known_hosts: Some(store.to_path_buf()),
        auto_reconnect: false,
        clipboard_dir: Some(store.with_file_name("clipboard")),
    }
}

fn start(config: RdpConfig, recorder: &Arc<Recorder>) -> Arc<RdpSession> {
    let handler: Arc<dyn RdpHandler> = recorder.clone();
    let session = Arc::new(RdpSession::start(config, handler).unwrap());
    recorder.session.set(Arc::downgrade(&session)).unwrap();
    session
}

fn split(address: &str) -> (String, u16) {
    let (host, port) = address.rsplit_once(':').expect("host:port");
    (host.to_owned(), port.parse().expect("port"))
}

fn docker(container: &str, script: &str) -> String {
    // As the session's user, on the display xrdp logged the last session in on.
    let wrapped = "display=$(grep -o 'login successful for user tester on display [0-9]*' /var/log/xrdp.log | \
                   tail -1 | awk '{print $NF}'); runuser -u tester -- env DISPLAY=:$display sh -c \"$0\"";
    let output = std::process::Command::new("docker")
        .args(["exec", container, "sh", "-c", wrapped, script])
        .output()
        .expect("docker exec");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario, step by step
fn xrdp_session() {
    let Some(address) = env("RD_TEST_XRDP") else {
        eprintln!("RD_TEST_XRDP not set: skipped");
        return;
    };
    let (host, port) = split(&address);
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("rdp_known_hosts");

    // First connection: the certificate is new and accepted permanently.
    let recorder = Recorder::new(CertificateChoice::AcceptPermanently);
    let session = start(config(&host, port, "tester", "tester-pass", &store), &recorder);
    let (width, height) = recorder.connected();
    assert_eq!((width, height), (1024, 768));
    {
        let state = recorder.state.lock().unwrap();
        assert_eq!(state.certificates.len(), 1);
        let request = &state.certificates[0];
        assert!(!request.changed);
        assert_eq!(request.fingerprint.len(), 95);
        assert_eq!(
            KnownHosts::new(&store).check(&host, port, &request.fingerprint),
            Verdict::Same
        );
        assert!(state.credentials.is_empty(), "TLS without NLA asks nothing");
    }
    recorder.wait("a full first frame", |s| {
        s.frames
            .iter()
            .find(|f| f.1 == 1024 && f.2 == 768 && f.3 > 0)
            .copied()
    });
    assert!(session.is_connected());

    // The pointer comes as a shape for the view's cursor, not drawn into the desktop.
    let shape = |s: &State| {
        s.events
            .iter()
            .any(|e| matches!(e, RdpEvent::PointerShape { .. }))
    };
    recorder.wait("the pointer shape", |s| shape(s).then_some(()));

    // Input: move the mouse and press a key; frames keep coming.
    session.input(vec![
        InputEvent::MouseMove { x: 100, y: 100 },
        InputEvent::Scancode {
            code: 0x2A,
            down: true,
        },
        InputEvent::Scancode {
            code: 0x2A,
            down: false,
        },
    ]);

    // Resize: xrdp answers with a bare Deactivate All and a Demand Active; the frames follow the new size.
    session.resize(1281, 721);
    let resized = recorder.wait("frames at the new size", |s| {
        s.frames
            .iter()
            .find(|f| (f.1, f.2) != (1024, 768))
            .map(|f| (f.1, f.2))
    });
    assert_eq!(resized, (1280, 721));
    recorder.wait("the resized event", |s| {
        s.events
            .iter()
            .any(|e| {
                *e == RdpEvent::Resized {
                    width: 1280,
                    height: 721,
                }
            })
            .then_some(())
    });

    // Typing.
    assert!(session.type_text("ab\n"));
    assert!(!session.type_text("again"), "one typing at a time");
    recorder.wait("typing finished", |s| {
        s.events
            .contains(&RdpEvent::Typed { finished: true })
            .then_some(())
    });
    assert!(!session.is_typing());

    // Clipboard round trip through the session's X display, then files.
    match env("RD_TEST_XRDP_CONTAINER") {
        Some(container) => {
            check_clipboard(&session, &recorder, &container);
            check_file_clipboard(&session, &recorder, &container, dir.path());
        }
        None => eprintln!("RD_TEST_XRDP_CONTAINER not set: clipboard round trip skipped"),
    }
    let frames = recorder.frames();
    assert!(frames >= 2, "frames: {frames}");
    session.close_and_wait();
    assert!(!session.is_running());
    assert!(
        !dir.path().join("clipboard").exists(),
        "the session's downloads are gone"
    );
    // Closing reports nothing.
    assert!(
        !recorder
            .state
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|e| matches!(e, RdpEvent::Disconnected { .. }))
    );

    if let Some(container) = env("RD_TEST_XRDP_CONTAINER") {
        check_logoff(&host, port, &store, &container);
    }
    check_certificates(&host, port, &store, dir.path());
}

/// Local text pasted in the X session, X clipboard text coming back.
fn check_clipboard(session: &RdpSession, recorder: &Recorder, container: &str) {
    session.set_clipboard("from-local-ő".to_owned());
    let deadline = Instant::now() + TIMEOUT;
    let mut pasted = String::new();
    while Instant::now() < deadline && pasted != "from-local-ő" {
        std::thread::sleep(Duration::from_millis(300));
        pasted = docker(container, "timeout 5 xclip -o -selection clipboard");
    }
    assert_eq!(pasted, "from-local-ő");
    docker(
        container,
        "printf 'from-remote\\nline' | xclip -i -selection clipboard -loops 3 >/dev/null 2>&1 &",
    );
    let text = recorder.wait("remote clipboard text", |s| {
        s.events.iter().find_map(|e| match e {
            RdpEvent::RemoteClipboard { text, .. } if text.starts_with("from-remote") => Some(text.clone()),
            _ => None,
        })
    });
    assert_eq!(text, "from-remote\nline");
}

/// Files copied in the X session (as `text/uri-list`) arrive in the session's clipboard folder with
/// their contents, the descriptor's modification time and the quarantine mark. (xrdp 0.9 sends no folders: "is a directory,
/// not supported".)
fn check_file_clipboard(session: &RdpSession, recorder: &Recorder, container: &str, dir: &std::path::Path) {
    docker(
        container,
        "rm -rf /tmp/rd-clip && mkdir -p /tmp/rd-clip && printf 'hello from xrdp' > /tmp/rd-clip/hello.txt && \
         head -c 3000000 /dev/urandom > /tmp/rd-clip/big.bin && \
         touch -d '2024-01-02 03:04:05 UTC' /tmp/rd-clip/big.bin && \
         printf 'file:///tmp/rd-clip/hello.txt\\r\\nfile:///tmp/rd-clip/big.bin\\r\\n' | \
         xclip -i -selection clipboard -t text/uri-list -loops 10 >/dev/null 2>&1 &",
    );
    let ((count, paths), ready_at) = recorder.wait("remote files on the clipboard", |s| {
        s.events.iter().enumerate().find_map(|(i, e)| match e {
            RdpEvent::FilesReady { paths, count } => Some(((*count, paths.clone()), i)),
            RdpEvent::FileTransferFailed { message } => panic!("file transfer failed: {message}"),
            _ => None,
        })
    });
    assert_eq!(count, 2);
    assert!(
        recorder.state.lock().unwrap().events[..ready_at]
            .iter()
            .any(|e| matches!(e, RdpEvent::FileProgress { .. })),
        "progress before the end"
    );
    let hello = paths
        .iter()
        .find(|p| p.ends_with("hello.txt"))
        .expect("the text file");
    let big = paths
        .iter()
        .find(|p| p.ends_with("big.bin"))
        .expect("the big file");
    assert!(hello.starts_with(dir.join("clipboard")));
    assert_eq!(std::fs::read_to_string(hello).unwrap(), "hello from xrdp");
    let remote_sum = docker(container, "sha256sum /tmp/rd-clip/big.bin | cut -d' ' -f1");
    let local = std::fs::read(big).unwrap();
    assert_eq!(local.len(), 3_000_000);
    assert_eq!(sha256_hex(&local), remote_sum.trim());
    // The descriptor's write time is stamped on the file; xrdp 0.9 sends the time of the copy, not the
    // file's own (2024-01-02), so the stamp is recent.
    let modified = std::fs::metadata(big).unwrap().modified().unwrap();
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    assert!(age < Duration::from_secs(600), "stamped {age:?} ago");
    #[cfg(target_os = "macos")]
    assert!(
        rustix::fs::getxattr(big.as_path(), "com.apple.quarantine", &mut [0u8; 0][..]).unwrap() > 0,
        "quarantined"
    );
    assert!(session.is_connected());

    // Local files: xrdp fetches the descriptors and claims the X clipboard with file targets (which the
    // `xclip` above doesn't offer). Their contents would go through FUSE, which the container lacks.
    let local = dir.join("upload.txt");
    std::fs::write(&local, b"from the client").unwrap();
    let files = rd_rdp::clipfiles::LocalFiles::scan(&[local]).unwrap();
    session.set_clipboard_files(Some(Arc::new(files)));
    let deadline = Instant::now() + TIMEOUT;
    let mut targets = String::new();
    while Instant::now() < deadline && !targets.contains("x-special/gnome-copied-files") {
        std::thread::sleep(Duration::from_millis(300));
        targets = docker(container, "timeout 5 xclip -o -selection clipboard -t TARGETS");
    }
    assert!(targets.contains("x-special/gnome-copied-files"), "{targets}");
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    sha2::Sha256::digest(data)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// Logging off inside the session: xrdp sends no error info, only an MCS Disconnect Provider
/// Ultimatum "user requested", which `FreeRDP` reports as "logoff by user": clean, not reconnected.
fn check_logoff(host: &str, port: u16, store: &std::path::Path, container: &str) {
    let recorder = Recorder::new(CertificateChoice::Reject);
    let mut reconnecting = config(host, port, "tester", "tester-pass", store);
    reconnecting.auto_reconnect = true;
    let session = start(reconnecting, &recorder);
    recorder.connected();
    std::thread::sleep(Duration::from_secs(2));
    docker(container, "xfce4-session-logout --logout --fast");
    let ended = recorder.wait("disconnected after logoff", |s| {
        s.events.iter().find_map(|e| match e {
            RdpEvent::Disconnected { .. } => Some(e.clone()),
            _ => None,
        })
    });
    assert!(
        matches!(
            ended,
            RdpEvent::Disconnected {
                clean: true,
                code: 0xC,
                ..
            }
        ),
        "{ended:?}"
    );
    assert!(
        !recorder
            .state
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|e| matches!(e, RdpEvent::Reconnecting { .. }))
    );
    session.close_and_wait();
}

/// The stored certificate is not asked about again; a changed one is flagged and can be rejected; a
/// refused connection is a failure with a message.
fn check_certificates(host: &str, port: u16, store: &std::path::Path, dir: &std::path::Path) {
    // Second connection: the stored certificate is not asked about again.
    let again = Recorder::new(CertificateChoice::Reject);
    let session = start(config(host, port, "tester", "tester-pass", store), &again);
    again.connected();
    assert!(again.state.lock().unwrap().certificates.is_empty());
    session.close_and_wait();

    // A changed certificate is flagged; rejecting it ends the session as a failure.
    let other = dir.join("other");
    KnownHosts::new(&other).accept(host, port, "00:11").unwrap();
    let rejecting = Recorder::new(CertificateChoice::Reject);
    let session = start(config(host, port, "tester", "tester-pass", &other), &rejecting);
    let ended = rejecting.wait("disconnected", |s| {
        s.events.iter().find_map(|e| match e {
            RdpEvent::Disconnected { clean, .. } => Some(*clean),
            _ => None,
        })
    });
    assert!(!ended);
    assert!(rejecting.state.lock().unwrap().certificates[0].changed);
    session.close_and_wait();

    // Nothing listens: a failure with a message.
    let refused = Recorder::new(CertificateChoice::AcceptOnce);
    let session = start(config("127.0.0.1", 9, "tester", "x", store), &refused);
    let message = refused.wait("disconnected", |s| {
        s.events.iter().find_map(|e| match e {
            RdpEvent::Disconnected {
                clean: false,
                message,
                ..
            } => Some(message.clone()),
            _ => None,
        })
    });
    assert!(message.starts_with("Couldn't connect"), "{message}");
    session.close_and_wait();
}

#[test]
fn windows_host_with_nla() {
    let (Some(host), Some(user), Some(password)) = (
        env("RD_TEST_RDP_HOST"),
        env("RD_TEST_RDP_USER"),
        env("RD_TEST_RDP_PASS"),
    ) else {
        eprintln!("RD_TEST_RDP_HOST/USER/PASS not set: skipped");
        return;
    };
    let port = env("RD_TEST_RDP_PORT").map_or(3389, |p| p.parse().expect("port"));
    let dir = tempfile::tempdir().unwrap();
    let (username, domain) = rd_core::rdp::split_user(&user, &env("RD_TEST_RDP_DOMAIN").unwrap_or_default());
    let mut config = config(&host, port, &username, &password, &dir.path().join("store"));
    config.domain = domain;

    // Missing password: the NLA prompt asks; a wrong answer is rejected and asked again; the right one
    // connects.
    let recorder = Recorder::new(CertificateChoice::AcceptOnce);
    let answer = |password: Zeroizing<String>| Credentials {
        username: config.username.clone(),
        domain: config.domain.clone(),
        password,
    };
    *recorder.credentials_answers.lock().unwrap() = vec![
        answer(Zeroizing::new("not-the-password".to_owned())),
        answer(config.password.clone()),
    ];
    config.password = Zeroizing::new(String::new());
    let session = start(config, &recorder);
    let size = recorder.connected();
    assert_eq!(size, (1024, 768), "connected at another size");
    {
        let state = recorder.state.lock().unwrap();
        assert_eq!(state.credentials.len(), 2, "two NLA prompts");
        assert!(!state.credentials[0].rejected);
        assert!(state.credentials[1].rejected);
        assert_eq!(
            state.certificates.len(),
            1,
            "the certificate is asked about once per session"
        );
    }
    recorder.wait("a frame", |s| s.frames.iter().find(|f| f.3 > 0).copied());

    // Windows resizes through Deactivate/Reactivate.
    session.resize(1280, 720);
    recorder.wait("the resized event", |s| {
        s.events
            .iter()
            .any(|e| {
                *e == RdpEvent::Resized {
                    width: 1280,
                    height: 720,
                }
            })
            .then_some(())
    });
    recorder.wait("a frame at the new size", |s| {
        s.frames
            .iter()
            .find(|f| (f.1, f.2) == (1280, 720) && f.3 > 0)
            .copied()
    });
    session.close_and_wait();
}

/// Type clipboard into Notepad on the Windows host and read it back through the clipboard. Opt-in
/// (`RD_TEST_RDP_TYPING=1` next to the Windows host variables): it opens Notepad on that machine, types,
/// copies, then empties the document and closes it.
#[test]
fn windows_notepad_typing() {
    let (Some(host), Some(user), Some(password), Some(_)) = (
        env("RD_TEST_RDP_HOST"),
        env("RD_TEST_RDP_USER"),
        env("RD_TEST_RDP_PASS"),
        env("RD_TEST_RDP_TYPING"),
    ) else {
        eprintln!("RD_TEST_RDP_HOST/USER/PASS/TYPING not set: skipped");
        return;
    };
    let port = env("RD_TEST_RDP_PORT").map_or(3389, |p| p.parse().expect("port"));
    let dir = tempfile::tempdir().unwrap();
    let (username, domain) = rd_core::rdp::split_user(&user, &env("RD_TEST_RDP_DOMAIN").unwrap_or_default());
    let mut config = config(&host, port, &username, &password, &dir.path().join("store"));
    config.domain = domain;
    let recorder = Recorder::new(CertificateChoice::AcceptOnce);
    let session = start(config, &recorder);
    recorder.connected();
    recorder.wait("a frame", |s| s.frames.iter().find(|f| f.3 > 0).copied());
    std::thread::sleep(Duration::from_secs(3));

    let key = |code: u16, down: bool| InputEvent::Scancode { code, down };
    let chord = |modifier: u16, code: u16| {
        vec![
            key(modifier, true),
            key(code, true),
            key(code, false),
            key(modifier, false),
        ]
    };
    let typed = |count: usize| {
        recorder.wait("typing finished", |s| {
            (s.events
                .iter()
                .filter(|e| matches!(e, RdpEvent::Typed { finished: true }))
                .count()
                >= count)
                .then_some(())
        });
    };

    // Win+R, "notepad", Enter.
    session.input(chord(0x15B, 0x13));
    std::thread::sleep(Duration::from_secs(2));
    assert!(session.type_text("notepad\n"));
    typed(1);
    std::thread::sleep(Duration::from_secs(4));

    // `RD_TEST_RDP_TYPING=stress`: 1000 characters without spaces, so Notepad's autocorrect stays out.
    let stress: String = (0..1000u32)
        .map(|i| {
            if i % 50 == 49 {
                '\n'
            } else {
                char::from(b"abcdefghijklmnopqrstuvwxyz0123456789"[(i * 7 + i / 50) as usize % 36])
            }
        })
        .collect();
    let text = if env("RD_TEST_RDP_TYPING").as_deref() == Some("stress") {
        stress.as_str()
    } else {
        "Első sor: árvíztűrő tükörfúrógép\nMásodik sor, 0123456789 !\"#$%&/()=?\n\tharmadik, tabbal\nNegyedik: The quick brown fox jumps over the lazy dog.\nötödik\n"
    };
    assert!(session.type_text(text));
    typed(2);
    std::thread::sleep(Duration::from_secs(2));

    // Ctrl+A, Ctrl+C: the remote clipboard comes back as an event.
    session.input(chord(0x1D, 0x1E));
    std::thread::sleep(Duration::from_millis(500));
    session.input(chord(0x1D, 0x2E));
    let copied = recorder.wait("the copied text", |s| {
        s.events.iter().rev().find_map(|e| match e {
            RdpEvent::RemoteClipboard { text, .. } => Some(text.clone()),
            _ => None,
        })
    });

    // Leave nothing behind: delete the text, close Notepad (an empty document closes without asking).
    session.input(vec![key(0x153, true), key(0x153, false)]);
    std::thread::sleep(Duration::from_millis(500));
    session.input(chord(0x38, 0x3E));
    std::thread::sleep(Duration::from_secs(1));
    session.close_and_wait();

    let normalized = copied.replace("\r\n", "\n");
    let (want, got) = (
        text.trim_end().to_lowercase(),
        normalized.trim_end().to_lowercase(),
    );
    let first = want.chars().zip(got.chars()).position(|(a, b)| a != b);
    eprintln!(
        "typed {} chars, copied {}; first difference at {first:?}: {:?}",
        want.chars().count(),
        got.chars().count(),
        first.map(|i| got.chars().skip(i.saturating_sub(5)).take(30).collect::<String>())
    );
    // Notepad's autocorrect capitalizes some words ("The Quick Brown"); every character must arrive once.
    assert_eq!(
        normalized.trim_end().to_lowercase(),
        text.trim_end().to_lowercase()
    );
}
