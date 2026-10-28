//! Background SSH (russh) against a real `sshd`: password auth, the unknown host key prompt appending to
//! a temp `known_hosts`, a known key needing no prompt, a rejected key refusing, a wrong password, a
//! port forward to the container's own sshd that must deliver its banner, and a changed key: refused
//! when declined, probed without logging in, replaced in the file when accepted.
//!
//! Runs only with `RD_TEST_SSHD=127.0.0.1:2221` (the `sshd-gnu` lab container:
//! `docker compose -f tests/lab/docker-compose.yml up -d sshd-gnu`); otherwise it passes without doing
//! anything. The user's own `~/.ssh` is never read or written: the home folder is a temp dir.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rd_ssh::background::tunnel::{ForwardState, Opener, Tunnel, TunnelSpec};
use rd_ssh::background::{Client, ConnectSpec, HostKeyPrompt, HostKeyQuestion, fetch_host_key, runtime};
use rd_ssh::{SessionEvent, Sink, SpawnSpec, SshSecrets, SshSession};
use secrecy::SecretString;
use tokio::io::AsyncReadExt;

fn target() -> Option<(String, u16)> {
    let target = std::env::var("RD_TEST_SSHD").ok()?;
    let (host, port) = target.rsplit_once(':').expect("RD_TEST_SSHD is host:port");
    Some((host.to_owned(), port.parse().expect("a port")))
}

fn spec(host: &str, port: u16, home: &Path, known_hosts: &Path, password: &str) -> ConnectSpec {
    ConnectSpec {
        host: host.to_owned(),
        port: Some(port),
        user: "tester".to_owned(),
        key_path: None,
        secrets: SshSecrets::new(Some(SecretString::from(password.to_owned())), None, None),
        home: Some(home.to_path_buf()),
        known_hosts: Some(vec![known_hosts.to_path_buf()]),
        use_agent: false,
    }
}

/// A prompt that answers `accept` and records the questions.
fn prompt(accept: bool, asked: &Arc<Mutex<Vec<HostKeyQuestion>>>) -> HostKeyPrompt {
    let asked = Arc::clone(asked);
    Arc::new(move |question| {
        asked.lock().unwrap().push(question);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let _ = sender.send(accept);
        receiver
    })
}

fn known_line(file: &PathBuf) -> String {
    std::fs::read_to_string(file).unwrap_or_default()
}

#[test]
fn background_connection_and_port_forward() {
    let Some((host, port)) = target() else {
        eprintln!("RD_TEST_SSHD not set: skipping the background SSH check");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let known_hosts = home.path().join("ssh-dir").join("known_hosts");
    let rt = runtime();

    // Unknown key, rejected: nothing is written.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let error = rt
        .block_on(Client::connect(
            spec(&host, port, home.path(), &known_hosts, "tester-pass"),
            Some(prompt(false, &asked)),
        ))
        .unwrap_err();
    assert_eq!(error.code(), "hostKeyRejected", "{error}");
    assert!(!known_hosts.exists());

    // Unknown key, accepted: appended in the [host]:port form, then authenticated by password.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let client = rt
        .block_on(Client::connect(
            spec(&host, port, home.path(), &known_hosts, "tester-pass"),
            Some(prompt(true, &asked)),
        ))
        .expect("connects after accepting the key");
    let question = asked.lock().unwrap()[0].clone();
    assert_eq!((question.host.as_str(), question.port), (host.as_str(), port));
    assert!(question.fingerprint.starts_with("SHA256:"));
    let line = known_line(&known_hosts);
    assert!(
        line.starts_with(&format!("[{host}]:{port} {} ", question.key_type)),
        "{line}"
    );
    rt.block_on(client.disconnect());

    // Known now: no prompt needed.
    let client = rt
        .block_on(Client::connect(
            spec(&host, port, home.path(), &known_hosts, "tester-pass"),
            None,
        ))
        .expect("known key");

    // A forward to the container's own sshd delivers its banner.
    let local = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (states, received) = std::sync::mpsc::channel();
    let states = Mutex::new(states);
    let opener: Arc<dyn Opener> = Arc::new(client);
    let tunnel = Tunnel::start(
        rt.handle(),
        TunnelSpec {
            local_port: local,
            remote_host: "127.0.0.1".to_owned(),
            remote_port: 22,
        },
        move |_| async move { Ok(opener) },
        Arc::new(move |state| {
            let _ = states.lock().unwrap().send(state);
        }),
    );
    let next = || received.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(next(), ForwardState::Starting);
    assert_eq!(next(), ForwardState::Active);
    let banner = rt.block_on(async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", local))
            .await
            .unwrap();
        let mut buffer = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8_lossy(&buffer[..n]).into_owned()
    });
    assert!(banner.starts_with("SSH-2.0-"), "{banner:?}");
    tunnel.stop();

    // Wrong password.
    let error = rt
        .block_on(Client::connect(
            spec(&host, port, home.path(), &known_hosts, "wrong"),
            None,
        ))
        .unwrap_err();
    assert_eq!(error.code(), "authFailed", "{error}");

    changed_key(&host, port, home.path(), &known_hosts, &question.fingerprint);
}

/// A different stored key of the same type (`known_hosts` holds the right key on entry): refused unless
/// the changed-key prompt is accepted, which replaces the entry. `fingerprint` is the server's.
fn changed_key(host: &str, port: u16, home: &Path, known_hosts: &PathBuf, fingerprint: &str) {
    let rt = runtime();
    let line = known_line(known_hosts);
    let mut parts: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    let mut blob: Vec<char> = parts[2].chars().collect();
    let last = blob.len() - 5;
    blob[last] = if blob[last] == 'A' { 'B' } else { 'A' };
    parts[2] = blob.into_iter().collect();
    let tampered = format!("# kept\n{}\n", parts.join(" "));
    std::fs::write(known_hosts, &tampered).unwrap();

    // The system `ssh` refuses too; its report names the entry and the server's fingerprint.
    if let Some(exit) = system_ssh(host, port, known_hosts) {
        let SessionEvent::Exit {
            code: Some(255),
            host_key_changed: Some(changed),
            ..
        } = exit
        else {
            panic!("{exit:?}");
        };
        assert_eq!(changed.host, format!("[{host}]:{port}"));
        assert_eq!(changed.fingerprint, fingerprint);
        assert_eq!(changed.line, 2);
        assert_eq!(
            std::fs::canonicalize(&changed.file).unwrap(),
            std::fs::canonicalize(known_hosts).unwrap()
        );
    }

    // Without a prompt, or declined: refused as changed, the file untouched.
    let error = rt
        .block_on(Client::connect(
            spec(host, port, home, known_hosts, "tester-pass"),
            None,
        ))
        .unwrap_err();
    assert_eq!(error.code(), "hostKeyChanged", "{error}");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let error = rt
        .block_on(Client::connect(
            spec(host, port, home, known_hosts, "tester-pass"),
            Some(prompt(false, &asked)),
        ))
        .unwrap_err();
    assert_eq!(error.code(), "hostKeyChanged", "{error}");
    assert_eq!(known_line(known_hosts), tampered);
    let changed = asked.lock().unwrap()[0]
        .changed
        .clone()
        .expect("asked as changed");
    assert_eq!(
        (changed.file.as_str(), changed.line),
        (known_hosts.display().to_string().as_str(), 2)
    );

    // The probe reads the server's key without logging in: the fingerprint the prompt showed.
    let probed = rt
        .block_on(fetch_host_key(&spec(host, port, home, known_hosts, "wrong")))
        .expect("probe");
    assert_eq!(probed.key.fingerprint(), fingerprint);
    assert_eq!(probed.name, format!("[{host}]:{port}"));
    assert_eq!(probed.files, [known_hosts.as_path()]);

    // Accepted: the old entry is replaced, the comment kept, and the connection goes on.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let client = rt
        .block_on(Client::connect(
            spec(host, port, home, known_hosts, "tester-pass"),
            Some(prompt(true, &asked)),
        ))
        .expect("connects after replacing the key");
    assert!(asked.lock().unwrap()[0].changed.is_some());
    assert_eq!(known_line(known_hosts), format!("# kept\n{line}"));
    rt.block_on(client.disconnect());

    // The system `ssh` accepts the replaced key: it gets as far as asking for a password (refused
    // in batch mode).
    if let Some(exit) = system_ssh(host, port, known_hosts) {
        assert!(
            matches!(
                exit,
                SessionEvent::Exit {
                    auth_failed: true,
                    host_key_changed: None,
                    ..
                }
            ),
            "{exit:?}"
        );
    }
}

/// Collects a session's output until its exit.
#[derive(Default)]
struct ExitSink {
    exit: Mutex<Option<SessionEvent>>,
}

impl Sink for ExitSink {
    fn output(&self, _bytes: Vec<u8>) {}
    fn event(&self, event: SessionEvent) {
        if matches!(event, SessionEvent::Exit { .. }) {
            *self.exit.lock().unwrap() = Some(event);
        }
    }
}

/// Runs the system `ssh` in a PTY against the lab with only `known_hosts` (no config, no agent, no
/// prompts) and returns its exit event; `None` when no `ssh` is installed.
fn system_ssh(host: &str, port: u16, known_hosts: &Path) -> Option<SessionEvent> {
    let program = rd_ssh::find_ssh()?;
    let option = |o: &str| ["-o".to_owned(), o.to_owned()];
    let mut args = vec!["-F".to_owned(), "none".to_owned()];
    args.extend(option(&format!("UserKnownHostsFile={}", known_hosts.display())));
    args.extend(option("GlobalKnownHostsFile=none"));
    args.extend(option("BatchMode=yes"));
    args.extend(option("IdentityAgent=none"));
    args.extend([
        "-p".to_owned(),
        port.to_string(),
        format!("tester@{host}"),
        "true".to_owned(),
    ]);
    let sink = Arc::new(ExitSink::default());
    let _session = SshSession::spawn(
        SpawnSpec {
            kind: rd_ssh::SessionKind::Ssh,
            program,
            args,
            env: Vec::new(),
            login_user: "tester".to_owned(),
            cols: 120,
            rows: 40,
            cwd: None,
        },
        sink.clone(),
    )
    .unwrap();
    for _ in 0..200 {
        if let Some(exit) = sink.exit.lock().unwrap().take() {
            return Some(exit);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("ssh didn't exit");
}
