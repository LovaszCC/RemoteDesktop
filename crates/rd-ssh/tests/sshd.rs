//! End-to-end check against a real `sshd`: the system `ssh` in a PTY, this test binary as the askpass
//! helper (exactly as the app executable is), the host key prompt forwarded, the password answered from
//! the "vault", output, title, alternate screen, recording, exit codes and the auth failure.
//!
//! Runs only with `RD_TEST_SSHD=127.0.0.1:2221` (the `sshd-gnu` lab container:
//! `docker compose -f tests/lab/docker-compose.yml up -d sshd-gnu`); otherwise it reports itself ignored.
//! It has its own `main` (`harness = false`) because `ssh` starts the binary as the helper;
//! `RD_TEST_HELPER=target/debug/rd-app` uses the app executable instead.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rd_core::{Connection, ConnectionType};
use rd_ssh::askpass::server::{AskpassHandler, AskpassRequest, AskpassServer};
use rd_ssh::askpass::{PromptKind, askpass_env, client};
use rd_ssh::recorder::Phase;
use rd_ssh::session::RecordOutcome;
use rd_ssh::{SessionEvent, Sink, SpawnSpec, SshSession, find_ssh, ssh_arguments};
use secrecy::SecretString;
use zeroize::Zeroizing;

#[derive(Default)]
struct Collect {
    output: Mutex<Vec<u8>>,
    output_changed: Condvar,
    events: Mutex<Vec<SessionEvent>>,
    events_changed: Condvar,
}

impl Sink for Collect {
    fn output(&self, bytes: Vec<u8>) {
        self.output.lock().unwrap().extend(bytes);
        self.output_changed.notify_all();
    }
    fn event(&self, event: SessionEvent) {
        self.events.lock().unwrap().push(event);
        self.events_changed.notify_all();
    }
}

const TIMEOUT: Duration = Duration::from_secs(20);

impl Collect {
    fn wait_output(&self, needle: &str) {
        let output = self
            .output_changed
            .wait_timeout_while(self.output.lock().unwrap(), TIMEOUT, |o| {
                !String::from_utf8_lossy(o).contains(needle)
            })
            .unwrap()
            .0;
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains(needle), "waiting for {needle:?} in {text:?}");
    }

    fn wait_event(&self, wanted: impl Fn(&SessionEvent) -> bool) -> SessionEvent {
        let events = self
            .events_changed
            .wait_timeout_while(self.events.lock().unwrap(), TIMEOUT, |e| !e.iter().any(&wanted))
            .unwrap()
            .0;
        events
            .iter()
            .find(|e| wanted(e))
            .cloned()
            .unwrap_or_else(|| panic!("event not seen; got {events:?}"))
    }
}

struct Lab {
    host: String,
    port: u16,
    server: AskpassServer,
    asked: Arc<Mutex<Vec<AskpassRequest>>>,
    known_hosts: tempfile::TempDir,
}

impl Lab {
    fn new(target: &str) -> Self {
        let (host, port) = target.rsplit_once(':').expect("RD_TEST_SSHD is host:port");
        let asked: Arc<Mutex<Vec<AskpassRequest>>> = Arc::default();
        let log = Arc::clone(&asked);
        // The "user": accepts host keys, refuses every password the vault didn't answer.
        let handler: AskpassHandler = Arc::new(move |request: AskpassRequest| {
            let kind = request.kind;
            log.lock().unwrap().push(request);
            (kind == PromptKind::Confirm).then(|| Zeroizing::new("yes".to_owned()))
        });
        Self {
            host: host.to_owned(),
            port: port.parse().expect("port"),
            server: AskpassServer::start(handler).unwrap(),
            asked,
            known_hosts: tempfile::tempdir().unwrap(),
        }
    }

    fn open(
        &self,
        user: &str,
        password: &str,
        label: &str,
    ) -> (SshSession, Arc<Collect>, rd_ssh::askpass::server::Ticket) {
        let mut connection = Connection::new(ConnectionType::Ssh, label, &self.host);
        connection.username = user.into();
        connection.port = Some(self.port);
        let ticket = self
            .server
            .register(label, Some(SecretString::from(password)), None);
        let known_hosts = self.known_hosts.path().join("known_hosts");
        let mut args = vec![
            "-F".to_owned(),
            "none".to_owned(),
            "-o".to_owned(),
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "-o".to_owned(),
            "StrictHostKeyChecking=ask".to_owned(),
            "-o".to_owned(),
            "PubkeyAuthentication=no".to_owned(),
            "-o".to_owned(),
            "NumberOfPasswordPrompts=2".to_owned(),
        ];
        args.extend(ssh_arguments(&connection, None, None).unwrap());
        // `RD_TEST_HELPER` checks another askpass helper, such as the built app executable.
        let helper =
            std::env::var_os("RD_TEST_HELPER").map_or_else(|| std::env::current_exe().unwrap(), Into::into);
        let spec = SpawnSpec {
            kind: rd_ssh::SessionKind::Ssh,
            program: find_ssh().expect("an ssh client"),
            args,
            env: askpass_env(&helper, self.server.endpoint(), ticket.token()),
            login_user: user.into(),
            cols: 100,
            rows: 30,
        };
        let sink = Arc::new(Collect::default());
        let session = SshSession::spawn(spec, sink.clone()).unwrap();
        (session, sink, ticket)
    }

    fn asked(&self, label: &str) -> Vec<PromptKind> {
        let asked = self.asked.lock().unwrap();
        asked
            .iter()
            .filter(|r| r.session == label)
            .map(|r| r.kind)
            .collect()
    }
}

fn interactive_session(lab: &Lab) {
    let (session, sink, _ticket) = lab.open("tester", "tester-pass", "gnu");
    sink.wait_output("$ ");
    assert_eq!(
        lab.asked("gnu"),
        [PromptKind::Confirm],
        "only the host key reached the user"
    );
    let title = sink.wait_event(|e| matches!(e, SessionEvent::Title { .. }));
    let SessionEvent::Title { title } = title else {
        unreachable!()
    };
    assert!(title.starts_with("tester@"), "{title}");
    assert!(!session.is_root());

    session.write(b"echo rd-$((40+2))\r").unwrap();
    sink.wait_output("rd-42");

    session
        .write(b"printf '\\033[?1049h'; sleep 0.3; printf '\\033[?1049l'\r")
        .unwrap();
    sink.wait_event(|e| *e == SessionEvent::AltScreen { active: true });
    sink.wait_event(|e| *e == SessionEvent::AltScreen { active: false });

    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(session.toggle_recording(), Phase::Armed);
    session.write(b"printf 'a\\nb\\n'\r").unwrap();
    let recorded = sink.wait_event(|e| matches!(e, SessionEvent::Recorded { .. }));
    assert_eq!(
        recorded,
        SessionEvent::Recorded {
            outcome: RecordOutcome::Copied,
            text: "a\nb".into()
        }
    );

    session.resize(120, 40).unwrap();
    session.write(b"stty size\r").unwrap();
    sink.wait_output("40 120");

    session.write(b"exit 3\r").unwrap();
    let exit = sink.wait_event(|e| matches!(e, SessionEvent::Exit { .. }));
    assert_eq!(
        exit,
        SessionEvent::Exit {
            code: Some(3),
            auth_failed: false,
            host_key_changed: None,
        }
    );
}

fn root_login(lab: &Lab) {
    let (session, sink, _ticket) = lab.open("root", "root-pass", "root");
    sink.wait_event(|e| *e == SessionEvent::Root { root: true });
    sink.wait_output("# ");
    session.write(b"exit\r").unwrap();
    let exit = sink.wait_event(|e| matches!(e, SessionEvent::Exit { .. }));
    assert_eq!(
        exit,
        SessionEvent::Exit {
            code: Some(0),
            auth_failed: false,
            host_key_changed: None,
        }
    );
}

fn wrong_password(lab: &Lab) {
    let (_session, sink, _ticket) = lab.open("tester", "not-the-password", "wrong");
    let exit = sink.wait_event(|e| matches!(e, SessionEvent::Exit { .. }));
    assert_eq!(
        exit,
        SessionEvent::Exit {
            code: Some(255),
            auth_failed: true,
            host_key_changed: None,
        }
    );
    assert_eq!(
        lab.asked("wrong"),
        [PromptKind::Password],
        "the vault answered the first prompt, the user refused the retry"
    );
}

fn main() {
    if client::helper_requested() {
        std::process::exit(client::run_helper());
    }
    let Ok(target) = std::env::var("RD_TEST_SSHD") else {
        println!("test sshd_session ... ignored (set RD_TEST_SSHD=127.0.0.1:2221)");
        return;
    };
    let lab = Lab::new(&target);
    interactive_session(&lab);
    root_login(&lab);
    wrong_password(&lab);
    println!("test sshd_session ... ok");
}
