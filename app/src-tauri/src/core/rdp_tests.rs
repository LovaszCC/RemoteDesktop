//! The embedded RDP flows against a fake engine: nothing connects.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rd_core::ConnectionType;
use rd_rdp::{
    CertificateChoice, CertificateRequest, CredentialsRequest, InputEvent, MouseButton, RdpConfig, RdpEvent,
    RdpHandler,
};

use rd_rdp::clipfiles::LocalFiles;

use super::launch_tests::save;
use super::rdp::wheel_units;
use super::tests::Env;
use super::*;

/// Records what the core asks of a run.
#[derive(Default)]
struct FakeRun {
    inputs: Mutex<Vec<InputEvent>>,
    resizes: Mutex<Vec<(u32, u32)>>,
    clipboard: Mutex<Vec<String>>,
    files: Mutex<Vec<Option<usize>>>,
    cancels: Mutex<usize>,
    typed: Mutex<Vec<String>>,
    acks: Mutex<usize>,
    closed: AtomicBool,
    connected: AtomicBool,
    busy: AtomicBool,
}

impl RdpRun for FakeRun {
    fn input(&self, events: Vec<InputEvent>) {
        self.inputs.lock().unwrap().extend(events);
    }
    fn resize(&self, width: u32, height: u32) {
        self.resizes.lock().unwrap().push((width, height));
    }
    fn set_clipboard(&self, text: String) {
        self.clipboard.lock().unwrap().push(text);
    }
    fn set_clipboard_files(&self, files: Option<Arc<LocalFiles>>) {
        self.files.lock().unwrap().push(files.map(|f| f.sources().len()));
    }
    fn cancel_file_transfer(&self) {
        *self.cancels.lock().unwrap() += 1;
    }
    fn type_text(&self, text: &str) -> bool {
        if self.busy.load(Ordering::SeqCst) {
            return false;
        }
        self.typed.lock().unwrap().push(text.to_owned());
        true
    }
    fn ack_frame(&self) {
        *self.acks.lock().unwrap() += 1;
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    fn is_running(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst) && self.is_running()
    }
}

struct Started {
    config: RdpConfig,
    handler: Arc<dyn RdpHandler>,
    run: Arc<FakeRun>,
}

type Starts = Arc<Mutex<Vec<Started>>>;

fn fake_engine(core: &AppCore) -> Starts {
    let starts: Starts = Arc::default();
    let record = Arc::clone(&starts);
    core.set_rdp_starter(Box::new(move |config, handler| {
        let run = Arc::new(FakeRun::default());
        record.lock().unwrap().push(Started {
            config,
            handler,
            run: Arc::clone(&run),
        });
        Ok(run)
    }));
    starts
}

#[derive(Default)]
struct Sink {
    frames: Mutex<Vec<Vec<u8>>>,
    events: Mutex<Vec<RdpEvent>>,
}

impl RdpSink for Sink {
    fn frame(&self, frame: Vec<u8>) {
        self.frames.lock().unwrap().push(frame);
    }
    fn event(&self, event: RdpEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Notices and session changes, as the Tauri layer would emit them.
#[derive(Default)]
struct Hooks {
    notices: Mutex<Vec<RdpNotice>>,
    changes: Mutex<usize>,
}

fn hooks(core: &AppCore) -> Arc<Hooks> {
    let hooks = Arc::new(Hooks::default());
    let notices = Arc::clone(&hooks);
    let changes = Arc::clone(&hooks);
    core.set_rdp_hooks(
        Arc::new(move |notice| notices.notices.lock().unwrap().push(notice)),
        Arc::new(move || *changes.changes.lock().unwrap() += 1),
    );
    hooks
}

impl Hooks {
    /// Waits for the next notice (prompts arrive from the handler's thread).
    fn next(&self, count: usize) -> RdpNotice {
        for _ in 0..500 {
            if let Some(notice) = self.notices.lock().unwrap().get(count) {
                return notice.clone();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no notice");
    }
}

fn rdp_connection(core: &AppCore, user: &str, password: Option<&str>) -> String {
    save(
        core,
        ConnectionType::Rdp,
        |e, d| {
            e.host = " dc.example.test ".into();
            d.port = "3390".into();
            d.username = user.into();
            d.domain = "OTHER".into();
            d.rdp_admin_session = true;
        },
        password,
    )
}

fn open(core: &AppCore, id: &str) -> (String, Arc<Sink>) {
    let sink = Arc::new(Sink::default());
    let handed: Arc<dyn RdpSink> = sink.clone();
    let session = core.open_rdp_session(id, 1600, 1200, |_| handed).unwrap();
    (session, sink)
}

#[test]
fn opening_resolves_the_connection_and_its_password() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "AD\\alice", Some("pw"));
    let (session, _) = open(&core, &id);
    assert!(!session.is_empty());
    let starts = starts.lock().unwrap();
    let config = &starts[0].config;
    assert_eq!(config.host, "dc.example.test");
    assert_eq!(config.port, 3390);
    assert_eq!(
        (config.username.as_str(), config.domain.as_str()),
        ("alice", "AD")
    );
    assert_eq!(config.password.as_str(), "pw");
    assert!(config.admin_session);
    assert!(config.auto_reconnect);
    assert_eq!((config.width, config.height), (1600, 1200));
    assert_eq!(
        config.known_hosts.as_deref(),
        Some(env.settings_dir().join("rdp_known_hosts").as_path())
    );
    assert!(!config.client_name.is_empty());
    let folder = config.clipboard_dir.as_deref().unwrap();
    assert_eq!(
        folder.parent(),
        Some(rd_platform::clipboard::rdp_clipboard_root().as_path())
    );
    drop(starts);
    assert_eq!(core.rdp_session_count(), 1);
    assert_eq!(core.running_rdp_sessions(), 1);
    assert_eq!(core.running_count(), 1);
}

#[test]
fn opening_errors() {
    let env = Env::new();
    let core = env.unlocked();
    fake_engine(&core);
    let ssh = save(&core, ConnectionType::Ssh, |_, _| {}, None);
    let sink = || -> Arc<dyn RdpSink> { Arc::new(Sink::default()) };
    assert_eq!(
        core.open_rdp_session(&ssh, 800, 600, |_| sink())
            .unwrap_err()
            .code,
        "wrongType"
    );
    assert_eq!(
        core.open_rdp_session("nope", 800, 600, |_| sink())
            .unwrap_err()
            .code,
        "notFound"
    );
    core.set_rdp_starter(Box::new(|_, _| Err(std::io::Error::other("no threads"))));
    let id = rdp_connection(&core, "u", None);
    assert_eq!(
        core.open_rdp_session(&id, 800, 600, |_| sink()).unwrap_err().code,
        "rdpFailed"
    );
    assert_eq!(core.rdp_session_count(), 0);
    core.lock();
    assert_eq!(
        core.open_rdp_session(&id, 800, 600, |_| sink()).unwrap_err().code,
        "locked"
    );
    assert_eq!(core.rdp_ack_frame("gone").unwrap_err().code, "sessionNotFound");
    assert_eq!(core.rdp_resize("gone", 1, 1).unwrap_err().code, "sessionNotFound");
}

#[test]
fn frames_and_events_reach_the_sink_and_the_menu() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let hooks = hooks(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, sink) = open(&core, &id);
    let handler = Arc::clone(&starts.lock().unwrap()[0].handler);
    handler.frame(vec![1, 2, 3]);
    handler.event(RdpEvent::Connected {
        width: 800,
        height: 600,
    });
    assert_eq!(sink.frames.lock().unwrap().len(), 1);
    assert_eq!(sink.events.lock().unwrap().len(), 1);
    // Opening and connecting refresh the menu.
    assert_eq!(*hooks.changes.lock().unwrap(), 2);
    core.rdp_ack_frame(&session).unwrap();
    assert_eq!(*starts.lock().unwrap()[0].run.acks.lock().unwrap(), 1);
    handler.event(RdpEvent::Disconnected {
        clean: false,
        code: 0,
        message: String::new(),
    });
    assert_eq!(*hooks.changes.lock().unwrap(), 3);
}

#[test]
fn input_goes_through_the_translator_and_onto_the_desktop() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, _) = open(&core, &id);
    let (handler, run) = {
        let starts = starts.lock().unwrap();
        (Arc::clone(&starts[0].handler), Arc::clone(&starts[0].run))
    };
    // The desktop is still 800×600 in a 1600×1200 view: points are scaled.
    handler.event(RdpEvent::Connected {
        width: 800,
        height: 600,
    });
    core.rdp_input(
        &session,
        vec![
            RdpInput::Key {
                code: "KeyC".into(),
                key: "c".into(),
                down: true,
            },
            RdpInput::MouseMove { x: 100.0, y: 50.0 },
            RdpInput::MouseButton {
                button: 2,
                down: true,
                x: 1599.0,
                y: 1199.0,
            },
            RdpInput::MouseButton {
                button: 9,
                down: true,
                x: 0.0,
                y: 0.0,
            },
            RdpInput::Wheel {
                delta_x: 0.0,
                delta_y: 100.0,
                delta_mode: 0,
                x: 0.0,
                y: 0.0,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        *run.inputs.lock().unwrap(),
        [
            InputEvent::Scancode {
                code: 0x2E,
                down: true
            },
            InputEvent::MouseMove { x: 50, y: 25 },
            InputEvent::MouseButton {
                button: MouseButton::Right,
                down: true,
                x: 799,
                y: 599
            },
            InputEvent::Wheel {
                delta: -120,
                horizontal: false,
                x: 0,
                y: 0
            },
        ]
    );
    run.inputs.lock().unwrap().clear();
    core.rdp_release_all_keys(&session).unwrap();
    assert_eq!(
        *run.inputs.lock().unwrap(),
        [InputEvent::Scancode {
            code: 0x2E,
            down: false
        }]
    );
    // A resize after the server caught up: 1:1.
    core.rdp_resize(&session, 1000, 700).unwrap();
    assert_eq!(*run.resizes.lock().unwrap(), [(1000, 700)]);
    handler.event(RdpEvent::Resized {
        width: 1000,
        height: 700,
    });
    run.inputs.lock().unwrap().clear();
    core.rdp_input(&session, vec![RdpInput::MouseMove { x: 10.0, y: 20.0 }])
        .unwrap();
    assert_eq!(
        *run.inputs.lock().unwrap(),
        [InputEvent::MouseMove { x: 10, y: 20 }]
    );
    core.rdp_raw_input(
        &session,
        vec![InputEvent::Scancode {
            code: 0x15B,
            down: true,
        }],
    )
    .unwrap();
    assert_eq!(run.inputs.lock().unwrap().len(), 2);
}

#[test]
fn wheel_deltas_become_rdp_units() {
    assert_eq!(wheel_units(100.0, 0), 120);
    assert_eq!(wheel_units(-50.0, 0), -60);
    assert_eq!(wheel_units(3.0, 1), 120);
    assert_eq!(wheel_units(1.0, 2), 360);
    assert_eq!(wheel_units(0.2, 0), 0);
    assert_eq!(wheel_units(1e9, 0), 30_000);
}

#[test]
fn type_clipboard_asks_first_for_long_texts() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, _) = open(&core, &id);
    let run = Arc::clone(&starts.lock().unwrap()[0].run);
    assert_eq!(
        core.rdp_type_text(&session, None, false),
        Ok(TypeClipboardDto::Empty)
    );
    assert_eq!(
        core.rdp_type_text(&session, Some(""), false),
        Ok(TypeClipboardDto::Empty)
    );
    assert_eq!(
        core.rdp_type_text(&session, Some("hello"), false),
        Ok(TypeClipboardDto::Typing { chars: 5 })
    );
    let long = "ő".repeat(2500);
    assert_eq!(
        core.rdp_type_text(&session, Some(&long), false),
        Ok(TypeClipboardDto::NeedsConfirmation {
            chars: 2500,
            seconds: 51
        })
    );
    assert_eq!(
        core.rdp_type_text(&session, Some(&long), true),
        Ok(TypeClipboardDto::Typing { chars: 2500 })
    );
    assert_eq!(run.typed.lock().unwrap().len(), 2);
    run.busy.store(true, Ordering::SeqCst);
    assert_eq!(
        core.rdp_type_text(&session, Some("x"), false),
        Ok(TypeClipboardDto::Busy)
    );
    assert_eq!(
        core.rdp_type_text("gone", Some("x"), false).unwrap_err().code,
        "sessionNotFound"
    );
}

#[test]
fn clipboard_goes_to_connected_sessions_only() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    open(&core, &id);
    open(&core, &id);
    assert!(!core.rdp_any_connected());
    let (first, second) = {
        let starts = starts.lock().unwrap();
        (Arc::clone(&starts[0].run), Arc::clone(&starts[1].run))
    };
    first.connected.store(true, Ordering::SeqCst);
    assert!(core.rdp_any_connected());
    core.rdp_set_clipboard_all("text");
    assert_eq!(*first.clipboard.lock().unwrap(), ["text"]);
    assert!(second.clipboard.lock().unwrap().is_empty());
}

#[test]
fn clipboard_files_go_to_connected_sessions_and_failures_are_reported() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, sink) = open(&core, &id);
    let (_, idle_sink) = open(&core, &id);
    let (first, second) = {
        let starts = starts.lock().unwrap();
        (Arc::clone(&starts[0].run), Arc::clone(&starts[1].run))
    };
    first.connected.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
    let files = LocalFiles::scan(&[dir.path().join("a.txt")]).unwrap();
    core.rdp_set_clipboard_files_all(&Ok(Arc::new(files)));
    core.rdp_set_clipboard_files_all(&Err("x cannot be copied".into()));
    assert_eq!(*first.files.lock().unwrap(), [Some(1), None]);
    assert!(second.files.lock().unwrap().is_empty());
    assert!(
        sink.events
            .lock()
            .unwrap()
            .contains(&RdpEvent::FileTransferFailed {
                message: "x cannot be copied".into()
            })
    );
    assert!(idle_sink.events.lock().unwrap().is_empty());

    core.rdp_cancel_file_transfer(&session).unwrap();
    assert_eq!(*first.cancels.lock().unwrap(), 1);
    assert_eq!(
        core.rdp_cancel_file_transfer("gone").unwrap_err().code,
        "sessionNotFound"
    );
}

#[test]
fn certificate_prompt_round_trip_and_cancel_on_close() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let hooks = hooks(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, _) = open(&core, &id);
    let handler = Arc::clone(&starts.lock().unwrap()[0].handler);
    let request = CertificateRequest {
        host: "dc.example.test".into(),
        port: 3390,
        fingerprint: "AA:BB".into(),
        changed: true,
        name_mismatch: false,
        common_name: "dc".into(),
        subject: "CN=dc".into(),
        issuer: "CN=dc".into(),
    };
    let asking = Arc::clone(&handler);
    let pending = request.clone();
    let answer = std::thread::spawn(move || asking.certificate(pending));
    let RdpNotice::Certificate(prompt) = hooks.next(0) else {
        panic!("certificate prompt expected");
    };
    assert_eq!(prompt.session_id, session);
    assert_eq!(prompt.connection_name, "Target");
    assert!(prompt.changed && !prompt.name_mismatch);
    assert_eq!(prompt.fingerprint, "AA:BB");
    let json = serde_json::to_value(&prompt).unwrap();
    assert_eq!(json["nameMismatch"], false);
    assert_eq!(json["commonName"], "dc");
    core.rdp_cert_answer(&prompt.request_id, CertificateChoice::AcceptOnce);
    assert_eq!(answer.join().unwrap(), CertificateChoice::AcceptOnce);
    // Unknown ids are ignored.
    core.rdp_cert_answer("gone", CertificateChoice::Reject);

    // Closing the tab refuses the open prompt and closes its dialog.
    let asking = Arc::clone(&handler);
    let answer = std::thread::spawn(move || asking.certificate(request));
    let RdpNotice::Certificate(prompt) = hooks.next(1) else {
        panic!("certificate prompt expected");
    };
    core.rdp_close(&session);
    assert_eq!(answer.join().unwrap(), CertificateChoice::Reject);
    assert_eq!(hooks.next(2), RdpNotice::Closed(prompt.request_id));
    assert!(starts.lock().unwrap()[0].run.closed.load(Ordering::SeqCst));
    assert_eq!(core.rdp_session_count(), 0);
    assert_eq!(core.running_count(), 0);
}

fn ask_credentials(handler: &Arc<dyn RdpHandler>) -> std::thread::JoinHandle<Option<rd_rdp::Credentials>> {
    let asking = Arc::clone(handler);
    std::thread::spawn(move || {
        asking.credentials(CredentialsRequest {
            username: "alice".into(),
            domain: "AD".into(),
            rejected: true,
        })
    })
}

fn answer(username: &str, password: &str, save: bool) -> RdpCredentialsAnswer {
    serde_json::from_value(serde_json::json!({
        "username": username,
        "domain": "IGNORED",
        "password": password,
        "save": save,
    }))
    .unwrap()
}

#[test]
fn credentials_prompt_saves_into_the_connection() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let hooks = hooks(&core);
    let id = rdp_connection(&core, "", None);
    let (session, _) = open(&core, &id);
    let handler = Arc::clone(&starts.lock().unwrap()[0].handler);

    let reply = ask_credentials(&handler);
    let RdpNotice::Credentials(prompt) = hooks.next(0) else {
        panic!("credentials prompt expected");
    };
    assert_eq!(prompt.session_id, session);
    assert!(prompt.can_save && prompt.rejected);
    assert_eq!(
        (prompt.username.as_str(), prompt.domain.as_str()),
        ("alice", "AD")
    );
    assert_eq!(
        core.rdp_credentials_answer(&prompt.request_id, Some(answer("CORP\\bob", "s3cret", true))),
        Ok(true)
    );
    let credentials = reply.join().unwrap().unwrap();
    assert_eq!(
        (credentials.username.as_str(), credentials.domain.as_str()),
        ("bob", "CORP")
    );
    assert_eq!(credentials.password.as_str(), "s3cret");
    assert!(!format!("{:?}", answer("u", "hidden", false)).contains("hidden"));

    // The next session uses what was saved.
    open(&core, &id);
    let config = &starts.lock().unwrap()[1].config;
    assert_eq!(
        (config.username.as_str(), config.domain.as_str()),
        ("bob", "CORP")
    );
    assert_eq!(config.password.as_str(), "s3cret");

    // Cancelling answers nothing.
    let reply = ask_credentials(&handler);
    let RdpNotice::Credentials(prompt) = hooks.next(1) else {
        panic!("credentials prompt expected");
    };
    assert_eq!(core.rdp_credentials_answer(&prompt.request_id, None), Ok(false));
    assert!(reply.join().unwrap().is_none());
    assert_eq!(core.rdp_credentials_answer("gone", None), Ok(false));
}

#[test]
fn linked_credentials_are_never_overwritten() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let hooks = hooks(&core);
    let credential = core
        .save_credential(
            None,
            &CredentialInput {
                name: "shared".into(),
                username: "svc".into(),
                domain: "CORP".into(),
                ..CredentialInput::default()
            },
            Some("linked-pw"),
            None,
        )
        .unwrap();
    let id = rdp_connection(&core, "own", Some("own-pw"));
    core.set_credential(Some(&credential), &[], std::slice::from_ref(&id))
        .unwrap();
    open(&core, &id);
    let handler = Arc::clone(&starts.lock().unwrap()[0].handler);
    {
        let config = &starts.lock().unwrap()[0].config;
        assert_eq!(
            (config.username.as_str(), config.domain.as_str()),
            ("svc", "CORP")
        );
        assert_eq!(config.password.as_str(), "linked-pw");
    }
    let reply = ask_credentials(&handler);
    let RdpNotice::Credentials(prompt) = hooks.next(0) else {
        panic!("credentials prompt expected");
    };
    assert!(!prompt.can_save);
    assert_eq!(
        core.rdp_credentials_answer(&prompt.request_id, Some(answer("x", "y", true))),
        Ok(false)
    );
    assert!(reply.join().unwrap().is_some());
    open(&core, &id);
    assert_eq!(starts.lock().unwrap()[1].config.password.as_str(), "linked-pw");
}

#[test]
fn reconnect_replaces_the_run_and_silences_the_old_one() {
    let env = Env::new();
    let core = env.unlocked();
    let starts = fake_engine(&core);
    let id = rdp_connection(&core, "u", Some("p"));
    let (session, sink) = open(&core, &id);
    let old = {
        let starts = starts.lock().unwrap();
        (Arc::clone(&starts[0].handler), Arc::clone(&starts[0].run))
    };
    core.rdp_reconnect(&session, 1280, 800).unwrap();
    assert!(old.1.closed.load(Ordering::SeqCst));
    old.0.frame(vec![0]);
    old.0.event(RdpEvent::Typed { finished: false });
    assert!(sink.frames.lock().unwrap().is_empty());
    assert!(sink.events.lock().unwrap().is_empty());
    let new = Arc::clone(&starts.lock().unwrap()[1].handler);
    let size = {
        let starts = starts.lock().unwrap();
        (starts[1].config.width, starts[1].config.height)
    };
    assert_eq!(size, (1280, 800));
    new.frame(vec![1]);
    assert_eq!(sink.frames.lock().unwrap().len(), 1);
    // A stale handler can't ask anything either.
    assert_eq!(
        old.0.certificate(CertificateRequest {
            host: String::new(),
            port: 0,
            fingerprint: String::new(),
            changed: false,
            name_mismatch: false,
            common_name: String::new(),
            subject: String::new(),
            issuer: String::new(),
        }),
        CertificateChoice::Reject
    );
    assert_eq!(
        core.rdp_reconnect("gone", 1, 1).unwrap_err().code,
        "sessionNotFound"
    );
    core.shutdown_rdp();
    assert_eq!(core.rdp_session_count(), 0);
}

#[test]
fn engine_info_names_freerdp() {
    let info = AppCore::rdp_engine_info();
    assert_eq!(info.name, "FreeRDP");
    assert!(!info.version.is_empty());
}
