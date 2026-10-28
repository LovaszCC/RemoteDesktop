//! The SSH session lifecycle against a fake `ssh` (a shell script that prints its arguments and askpass
//! environment), with the askpass server reached directly.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;
use rd_core::shortcuts::Platform;
use rd_platform::Settings;
use rd_ssh::askpass::client::ask;
use rd_ssh::askpass::{PromptKind, WireRequest};
use rd_ssh::{SessionEvent, Sink};
use rd_store::MemoryKeyCache;
use tempfile::TempDir;

use super::ssh_unlock::tests::write_encrypted_key;
use super::tests::{entry_draft, save_single};
use super::*;

const TIMEOUT: Duration = Duration::from_secs(10);

const FAKE_SSH: &str = r#"#!/bin/sh
printf 'ARGS=%s\n' "$*"
printf 'TOKEN=%s\nSOCK=%s\nASKPASS=%s\nREQUIRE=%s\nTERM=%s\n' "$RD_ASKPASS_TOKEN" "$RD_ASKPASS_SOCKET" \
  "$SSH_ASKPASS" "$SSH_ASKPASS_REQUIRE" "$TERM"
for a in "$@"; do case "$a" in */key) printf 'KEY=%s\n' "$(head -n 1 "$a")";; esac; done
printf 'READY\n'
read line
echo "IN=$line"
exit 7
"#;

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

impl Collect {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn wait_output(&self, needle: &str) -> String {
        let output = self
            .output_changed
            .wait_timeout_while(self.output.lock().unwrap(), TIMEOUT, |o| {
                !String::from_utf8_lossy(o).contains(needle)
            })
            .unwrap()
            .0;
        let text = String::from_utf8_lossy(&output).into_owned();
        assert!(text.contains(needle), "waiting for {needle:?} in {text:?}");
        text
    }

    fn wait_exits(&self, count: usize) -> Vec<SessionEvent> {
        let is_exit = |e: &&SessionEvent| matches!(e, SessionEvent::Exit { .. });
        let events = self
            .events_changed
            .wait_timeout_while(self.events.lock().unwrap(), TIMEOUT, |e| {
                e.iter().filter(is_exit).count() < count
            })
            .unwrap()
            .0;
        let exits: Vec<SessionEvent> = events.iter().filter(is_exit).cloned().collect();
        assert_eq!(exits.len(), count, "{events:?}");
        exits
    }

    /// `NAME=value` from the fake ssh's output.
    fn value(&self, name: &str) -> String {
        let text = self.text();
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .unwrap_or_else(|| panic!("{name} missing in {text:?}"))
            .trim_end_matches('\r')
            .to_owned()
    }
}

struct Lab {
    dir: TempDir,
    core: AppCore,
    notices: mpsc::Receiver<AskpassNotice>,
    changes: Arc<Mutex<usize>>,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let settings_dir = dir.path().join("config");
        let settings = Settings {
            database_path: Some(dir.path().join("data").join("connections.sqlite")),
            ..Settings::default()
        };
        settings.save(&settings_dir).unwrap();
        let core = AppCore::with_options(
            settings_dir,
            Box::new(MemoryKeyCache::default()),
            1_000,
            Platform::MacOS,
        );
        core.open_current();
        core.create_vault("master", false).unwrap();

        let fake = dir.path().join("fake-ssh");
        std::fs::write(&fake, FAKE_SSH).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        core.set_ssh_program(Some(fake), Some(PathBuf::from("/opt/app/helper")));

        let (tx, notices) = mpsc::channel();
        let tx = Mutex::new(tx);
        let changes: Arc<Mutex<usize>> = Arc::default();
        let counter = Arc::clone(&changes);
        core.set_ssh_hooks(
            Arc::new(move |notice| {
                let _ = tx.lock().unwrap().send(notice);
            }),
            Arc::new(move || *counter.lock().unwrap() += 1),
        );
        Self {
            dir,
            core,
            notices,
            changes,
        }
    }

    fn ssh_connection(&self, credential: Option<&str>) -> String {
        let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
        draft.port = "2222".into();
        draft.username = "u".into();
        draft.credential_id = credential.map(Into::into);
        save_single(
            &self.core,
            None,
            &entry_draft("web", "h"),
            draft,
            Some("pw"),
            None,
        )
        .unwrap()
    }

    fn open(&self, connection_id: &str) -> (String, Arc<Collect>) {
        let sink = Arc::new(Collect::default());
        let for_core: Arc<dyn Sink> = sink.clone();
        let id = self
            .core
            .open_ssh_session(connection_id, 80, 24, move |_| for_core)
            .unwrap();
        sink.wait_output("READY");
        (id, sink)
    }

    fn prompt(&self) -> AskpassPromptDto {
        match self.notices.recv_timeout(TIMEOUT).unwrap() {
            AskpassNotice::Prompt(prompt) => prompt,
            other @ AskpassNotice::Closed(_) => panic!("expected a prompt, got {other:?}"),
        }
    }
}

fn request(token: &str, prompt: &str) -> WireRequest {
    WireRequest {
        token: token.into(),
        hint: String::new(),
        prompt: prompt.into(),
    }
}

fn ask_in_background(sink: &Collect, prompt: &str) -> std::thread::JoinHandle<Option<String>> {
    let (sock, token, prompt) = (sink.value("SOCK"), sink.value("TOKEN"), prompt.to_owned());
    std::thread::spawn(move || {
        ask(&sock, &request(&token, &prompt))
            .unwrap()
            .map(|a| a.as_str().to_owned())
    })
}

#[test]
fn session_runs_ssh_with_the_askpass_environment_and_reports_its_exit() {
    let lab = Lab::new();
    let connection = lab.ssh_connection(None);
    let (id, sink) = lab.open(&connection);

    assert_eq!(sink.value("ARGS"), "-p 2222 u@h");
    assert_eq!(sink.value("ASKPASS"), "/opt/app/helper");
    assert_eq!(sink.value("REQUIRE"), "force");
    assert_eq!(sink.value("TERM"), "xterm-256color");
    assert_eq!(sink.value("TOKEN").len(), 64);
    assert_eq!(lab.core.running_count(), 1);
    assert_eq!(lab.core.ssh_session_count(), 1);
    assert!(*lab.changes.lock().unwrap() >= 1);

    // The vault answers the first password; the retry goes to the user.
    let first = ask_in_background(&sink, "u@h's password: ").join().unwrap();
    assert_eq!(first.as_deref(), Some("pw"));
    let retry = ask_in_background(&sink, "u@h's password: ");
    let prompt = lab.prompt();
    assert_eq!(
        (prompt.session_id.as_str(), prompt.kind),
        (id.as_str(), PromptKind::Password)
    );
    lab.core.askpass_answer(&prompt.request_id, Some("typed".into()));
    assert_eq!(retry.join().unwrap().as_deref(), Some("typed"));
    lab.core.askpass_answer(&prompt.request_id, Some("late".into()));

    assert_eq!(
        lab.core.ssh_clear_decision(&id).unwrap(),
        rd_ssh::analyzer::ClearAction::LocalAndRedraw
    );
    assert_eq!(
        lab.core.ssh_record_next_output(&id).unwrap(),
        rd_ssh::recorder::Phase::Armed
    );
    lab.core.ssh_record_cancel(&id).unwrap();
    lab.core.ssh_resize(&id, 100, 30).unwrap();
    lab.core.ssh_input(&id, b"hello\r").unwrap();
    sink.wait_output("IN=hello");
    let exits = sink.wait_exits(1);
    assert_eq!(
        exits,
        [SessionEvent::Exit {
            code: Some(7),
            auth_failed: false,
            host_key_changed: None,
        }]
    );
    assert_eq!(lab.core.running_count(), 0);
    assert_eq!(lab.core.ssh_session_count(), 1, "the tab stays to show the exit");
    assert_eq!(lab.core.ssh_input(&id, b"x").unwrap_err().code, "sessionEnded");
    assert_eq!(
        lab.core.ssh_clear_decision(&id).unwrap(),
        rd_ssh::analyzer::ClearAction::LocalOnly
    );

    lab.core.ssh_close(&id);
    assert_eq!(lab.core.ssh_session_count(), 0);
    assert_eq!(lab.core.ssh_input(&id, b"x").unwrap_err().code, "sessionNotFound");
    assert_eq!(
        lab.core.ssh_resize(&id, 1, 1).unwrap_err().code,
        "sessionNotFound"
    );
}

#[test]
fn reconnect_resolves_again_in_the_same_session() {
    let lab = Lab::new();
    let connection = lab.ssh_connection(None);
    let (id, sink) = lab.open(&connection);
    let first_token = sink.value("TOKEN");
    lab.core.ssh_input(&id, b"one\r").unwrap();
    sink.wait_exits(1);

    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.username = "v".into();
    save_single(
        &lab.core,
        Some(&connection),
        &entry_draft("web", "h2"),
        draft,
        None,
        None,
    )
    .unwrap();

    lab.core.ssh_reconnect(&id, 80, 24).unwrap();
    let text = sink.wait_output("ARGS=v@h2");
    assert!(text.contains("— Reconnecting —"));
    sink.wait_output("READY\r\n");
    assert_eq!(lab.core.running_count(), 1);
    // The old token is gone, the new one answers from the vault.
    let old = ask(&sink.value("SOCK"), &request(&first_token, "password: ")).unwrap();
    assert!(old.is_none());

    // Reconnecting a running session replaces it without reporting the old exit.
    lab.core.ssh_reconnect(&id, 80, 24).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    lab.core.ssh_input(&id, b"two\r").unwrap();
    sink.wait_output("IN=two");
    let exits = sink.wait_exits(2);
    assert_eq!(
        exits.len(),
        2,
        "one exit per finished run, none for the replaced one"
    );
    lab.core.ssh_close(&id);
}

#[test]
fn a_refused_changed_host_key_is_removed_before_the_reconnect() {
    let lab = Lab::new();
    // A port nobody listens on: the probe can't confirm the key, so the entry is only removed.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let name = format!("[127.0.0.1]:{port}");
    let known_hosts = lab.dir.path().join("known_hosts");
    let ed = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDJil+C6OKJkr9cMsZdSuOx85x1QVJ3qbT8HcpDPmmMU";
    let others = format!("{name} ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQC7\nother.example.com {ed}\n");
    std::fs::write(&known_hosts, format!("# kept\n{name} {ed}\n{others}")).unwrap();
    let kept = format!("# kept\n{others}");
    lab.core.set_forward_paths(Some(super::forwards::Paths {
        home: Some(lab.dir.path().to_path_buf()),
        known_hosts: Some(vec![known_hosts.clone()]),
        no_agent: true,
    }));

    let refusing = lab.dir.path().join("refusing-ssh");
    std::fs::write(
        &refusing,
        format!(
            "#!/bin/sh\nprintf '@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\\n\
             The fingerprint for the ED25519 key sent by the remote host is\\nSHA256:new.\\n\
             Offending ED25519 key in {file}:2\\n\
             Host key for {name} has changed and you have requested strict checking.\\n\
             Host key verification failed.\\nREADY\\n'\nexit 255\n",
            file = known_hosts.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&refusing, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    lab.core
        .set_ssh_program(Some(refusing), Some(PathBuf::from("/opt/app/helper")));
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = port.to_string();
    draft.username = "u".into();
    let connection = save_single(
        &lab.core,
        None,
        &entry_draft("changed", "127.0.0.1"),
        draft,
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        lab.core.ssh_replace_host_key("nope").unwrap_err().code,
        "sessionNotFound"
    );
    let (id, sink) = lab.open(&connection);
    let exits = sink.wait_exits(1);
    let SessionEvent::Exit {
        code: Some(255),
        host_key_changed: Some(changed),
        ..
    } = &exits[0]
    else {
        panic!("{exits:?}");
    };
    assert_eq!((changed.host.as_str(), changed.line), (name.as_str(), 2));

    // A report naming a file that isn't one of the user's known_hosts files is not trusted.
    let paths = |known: PathBuf| super::forwards::Paths {
        home: Some(lab.dir.path().to_path_buf()),
        known_hosts: Some(vec![known]),
        no_agent: true,
    };
    lab.core
        .set_forward_paths(Some(paths(lab.dir.path().join("elsewhere"))));
    assert_eq!(
        lab.core.ssh_replace_host_key(&id).unwrap_err().code,
        "hostKeyReplace"
    );
    assert!(std::fs::read_to_string(&known_hosts).unwrap().contains(ed));
    lab.core.set_forward_paths(Some(paths(known_hosts.clone())));

    lab.core.ssh_replace_host_key(&id).unwrap();
    assert_eq!(std::fs::read_to_string(&known_hosts).unwrap(), kept);
    // Done once: the change is forgotten.
    assert_eq!(
        lab.core.ssh_replace_host_key(&id).unwrap_err().code,
        "noHostKeyChange"
    );
    lab.core.ssh_close(&id);
}

#[test]
fn closing_a_session_refuses_its_open_prompt() {
    let lab = Lab::new();
    let connection = lab.ssh_connection(None);
    let (id, sink) = lab.open(&connection);
    let pending = ask_in_background(&sink, "Verification code: ");
    let prompt = lab.prompt();
    assert_eq!(prompt.kind, PromptKind::Other);
    lab.core.ssh_close(&id);
    assert_eq!(pending.join().unwrap(), None);
    match lab.notices.recv_timeout(TIMEOUT).unwrap() {
        AskpassNotice::Closed(request_id) => assert_eq!(request_id, prompt.request_id),
        other @ AskpassNotice::Prompt(_) => panic!("{other:?}"),
    }
    assert_eq!(lab.core.running_count(), 0);
}

#[test]
fn unlocked_key_goes_to_a_private_file_deleted_when_the_run_ends() {
    let lab = Lab::new();
    let key_path = lab.dir.path().join("id_ed25519");
    write_encrypted_key(&key_path, "key-pass");
    let input = CredentialInput {
        name: "deploy".into(),
        username: "deploy".into(),
        ssh_key_path: key_path.to_str().unwrap().into(),
        ..CredentialInput::default()
    };
    let credential = lab
        .core
        .save_credential(None, &input, None, Some("key-pass"))
        .unwrap();
    let connection = lab.ssh_connection(Some(&credential));
    let (id, sink) = lab.open(&connection);

    let args = sink.value("ARGS");
    let (before, key) = args.split_once("-i ").unwrap();
    assert_eq!(before, "-p 2222 ");
    let key = key.split(' ').next().unwrap().to_owned();
    assert!(args.ends_with(" -o IdentitiesOnly=yes deploy@h"), "{args}");
    assert_ne!(Path::new(&key), key_path);
    assert_eq!(sink.value("KEY"), "-----BEGIN OPENSSH PRIVATE KEY-----");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    // No passphrase prompt is answered: the unlocked key replaces it.
    let pending = ask_in_background(&sink, "Enter passphrase for key: ");
    let prompt = lab.prompt();
    assert_eq!(prompt.kind, PromptKind::Passphrase);
    lab.core.askpass_answer(&prompt.request_id, None);
    assert_eq!(pending.join().unwrap(), None);

    lab.core.ssh_input(&id, b"bye\r").unwrap();
    sink.wait_exits(1);
    assert!(
        !Path::new(&key).exists(),
        "the key file is deleted when ssh exits"
    );
    lab.core.ssh_close(&id);
}

#[test]
fn only_unlocked_ssh_connections_open() {
    let lab = Lab::new();
    let sink = || -> Arc<dyn Sink> { Arc::new(Collect::default()) };
    let err = lab
        .core
        .open_ssh_session("missing", 80, 24, |_| sink())
        .unwrap_err();
    assert_eq!(err.code, "notFound");

    let rdp = ConnectionDraft::new(ConnectionType::Rdp);
    let rdp = save_single(&lab.core, None, &entry_draft("rdp", "h"), rdp, None, None).unwrap();
    assert_eq!(
        lab.core
            .open_ssh_session(&rdp, 80, 24, |_| sink())
            .unwrap_err()
            .code,
        "notSsh"
    );

    let dash = ConnectionDraft::new(ConnectionType::Mikrotik);
    let dash = save_single(
        &lab.core,
        None,
        &entry_draft("router", "-oProxyCommand=x"),
        dash,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        lab.core
            .open_ssh_session(&dash, 80, 24, |_| sink())
            .unwrap_err()
            .code,
        "invalidHost"
    );
    assert_eq!(lab.core.ssh_session_count(), 0);

    let connection = lab.ssh_connection(None);
    lab.core.lock();
    assert_eq!(
        lab.core
            .open_ssh_session(&connection, 80, 24, |_| sink())
            .unwrap_err()
            .code,
        "locked"
    );
    assert_eq!(
        lab.core.open_external_terminal(&connection).unwrap_err().code,
        "locked"
    );
}

#[test]
fn quitting_ends_every_session() {
    let lab = Lab::new();
    let connection = lab.ssh_connection(None);
    let (_a, sink_a) = lab.open(&connection);
    let (_b, sink_b) = lab.open(&connection);
    assert_eq!(lab.core.running_count(), 2);
    lab.core.close();
    assert_eq!(lab.core.ssh_session_count(), 0);
    // Closed sessions report nothing more, but their processes end.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        sink_a
            .events
            .lock()
            .unwrap()
            .iter()
            .all(|e| !matches!(e, SessionEvent::Exit { .. }))
    );
    assert!(sink_b.text().contains("READY"));
}

#[test]
fn installed_terminals_are_listed() {
    let list = AppCore::installed_terminals();
    if cfg!(target_os = "macos") {
        assert!(list.iter().any(|t| t.id == "terminal"));
    }
}

/// A fake `ssh` whose remote side turns bracketed paste on and shows escapes as `E`.
const FAKE_SSH_BRACKETED: &str = r#"#!/bin/sh
printf '\033[?2004hREADY\n'
IFS= read -r line
printf 'IN=%s\n' "$line" | tr '\033' 'E'
"#;

#[test]
fn broadcast_input_drops_paste_markers_for_targets_without_bracketed_paste() {
    let lab = Lab::new();
    let connection = lab.ssh_connection(None);
    let (source, source_sink) = lab.open(&connection);
    let (plain, plain_sink) = lab.open(&connection);

    let fake = lab.dir.path().join("fake-ssh-bracketed");
    std::fs::write(&fake, FAKE_SSH_BRACKETED).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    lab.core
        .set_ssh_program(Some(fake), Some(PathBuf::from("/opt/app/helper")));
    let (bracketed, bracketed_sink) = lab.open(&connection);
    assert_eq!(lab.core.running_count(), 3, "every pane counts");

    lab.core
        .ssh_broadcast_input(
            &source,
            &[plain.clone(), "gone".into(), bracketed.clone()],
            b"\x1b[200~x\x1b[201~\r",
        )
        .unwrap();
    plain_sink.wait_output("IN=x");
    bracketed_sink.wait_output("IN=E[200~xE[201~");
    source_sink.wait_output("IN=\x1b[200~x");

    // The source must be alive; the ended targets are skipped.
    source_sink.wait_exits(1);
    assert_eq!(
        lab.core
            .ssh_broadcast_input(&source, &[plain], b"y")
            .unwrap_err()
            .code,
        "sessionEnded"
    );
}

/// A fake local shell that prints what it was started with.
const FAKE_SHELL: &str = r#"#!/bin/sh
printf 'ARGS=%s\nDIR=%s\nTERM=%s\nTOKEN=%s\n' "$*" "$(pwd -P)" "$TERM" "${RD_ASKPASS_TOKEN-unset}"
printf 'READY\n'
read line
echo "IN=$line"
exit 3
"#;

#[test]
fn local_terminal_runs_the_shell_without_a_connection_or_the_vault() {
    let lab = Lab::new();
    let shell = lab.dir.path().join("fake-shell");
    std::fs::write(&shell, FAKE_SHELL).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    lab.core.set_local_shell(Some(rd_ssh::local::LocalShell {
        program: shell,
        args: vec!["-l".into()],
    }));
    // No secret is involved: a locked app opens it too.
    lab.core.lock();

    let sink = Arc::new(Collect::default());
    let for_core: Arc<dyn Sink> = sink.clone();
    let id = lab
        .core
        .open_local_terminal(80, 24, None, move |_| for_core)
        .unwrap();
    sink.wait_output("READY");
    assert_eq!(sink.value("ARGS"), "-l");
    assert_eq!(sink.value("TERM"), "xterm-256color");
    assert_eq!(sink.value("TOKEN"), "unset");
    let home = std::fs::canonicalize(dirs::home_dir().unwrap()).unwrap();
    assert_eq!(PathBuf::from(sink.value("DIR")), home);
    assert_eq!(lab.core.running_count(), 1);
    assert_eq!(
        lab.core.ssh_session_directory(&id),
        None,
        "no connection behind it"
    );

    lab.core.ssh_resize(&id, 100, 30).unwrap();
    lab.core.ssh_input(&id, b"hi\r").unwrap();
    sink.wait_output("IN=hi");
    assert_eq!(
        sink.wait_exits(1),
        [SessionEvent::Exit {
            code: Some(3),
            auth_failed: false,
            host_key_changed: None,
        }]
    );
    assert_eq!(
        lab.core.ssh_replace_host_key(&id).unwrap_err().code,
        "noHostKeyChange"
    );

    // Reconnect starts a new shell in the same session.
    lab.core.ssh_reconnect(&id, 80, 24).unwrap();
    let ready_twice = |o: &Vec<u8>| String::from_utf8_lossy(o).matches("READY").count() >= 2;
    let output = sink
        .output_changed
        .wait_timeout_while(sink.output.lock().unwrap(), TIMEOUT, |o| !ready_twice(o))
        .unwrap()
        .0
        .clone();
    let text = String::from_utf8_lossy(&output);
    assert!(ready_twice(&output), "{text:?}");
    assert!(text.contains("— Reconnecting —"));
    assert_eq!(lab.core.running_count(), 1);
    lab.core.ssh_close(&id);
    assert_eq!(lab.core.ssh_session_count(), 0);
}

#[test]
fn a_local_terminal_opened_from_another_starts_in_its_directory() {
    let lab = Lab::new();
    let inner = lab.dir.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    let marker = lab.dir.path().join("first");
    // The first shell moves into `inner`; the later ones stay where they started.
    let script = format!(
        "#!/bin/sh\nif [ ! -e '{marker}' ]; then touch '{marker}'; cd '{inner}'; fi\n\
         printf 'DIR=%s\\nREADY\\n' \"$(pwd -P)\"\nread line\n",
        marker = marker.display(),
        inner = inner.display(),
    );
    let shell = lab.dir.path().join("fake-shell");
    std::fs::write(&shell, script).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    lab.core.set_local_shell(Some(rd_ssh::local::LocalShell {
        program: shell,
        args: Vec::new(),
    }));
    let open = |from: Option<&str>| {
        let sink = Arc::new(Collect::default());
        let for_core: Arc<dyn Sink> = sink.clone();
        let id = lab
            .core
            .open_local_terminal(80, 24, from, move |_| for_core)
            .unwrap();
        sink.wait_output("READY");
        (id, PathBuf::from(sink.value("DIR")))
    };
    let home = std::fs::canonicalize(dirs::home_dir().unwrap()).unwrap();
    let inner = std::fs::canonicalize(inner).unwrap();

    let (first, dir) = open(None);
    assert_eq!(dir, inner);
    let (second, dir) = open(Some(&first));
    assert_eq!(dir, inner, "Cmd+T keeps the directory");
    let (_, dir) = open(None);
    assert_eq!(dir, home, "the sidebar starts in home");
    let (_, dir) = open(Some("no-such-session"));
    assert_eq!(dir, home);

    lab.core.ssh_close(&second);
    lab.core.ssh_close(&first);
    let (_, dir) = open(Some(&first));
    assert_eq!(dir, home, "a closed terminal leaves nothing to follow");
}

// Connect with VPN

/// A lab whose MacVPN is the fake and whose ping never answers; the connection's entry uses the profile.
fn vpn_lab() -> (Lab, String, super::vpn_tests::SharedFake, Arc<Mutex<Vec<String>>>) {
    vpn_lab_on("u")
}

fn vpn_lab_on(user: &str) -> (Lab, String, super::vpn_tests::SharedFake, Arc<Mutex<Vec<String>>>) {
    let lab = Lab::new();
    let fake = super::vpn_tests::SharedFake::default();
    lab.core.set_vpn_backend(Box::new(fake.clone()));
    let pings: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&pings);
    lab.core.set_vpn_pinger(Box::new(move |host: &str| {
        log.lock().unwrap().push(host.to_owned());
        false
    }));
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = "2222".into();
    draft.username = user.into();
    let entry = rd_core::draft::EntryDraft {
        vpn: Some(rd_core::draft::EntryVpnDraft {
            connection_id: super::vpn_tests::PROFILE.into(),
            ..Default::default()
        }),
        ..entry_draft("web", "h")
    };
    let id = save_single(&lab.core, None, &entry, draft, Some("pw"), None).unwrap();
    (lab, id, fake, pings)
}

fn open_holders(core: &AppCore) -> Vec<u32> {
    core.vpn_status().held.iter().map(|h| h.open_holders).collect()
}

#[test]
fn ssh_session_holds_the_vpn_until_closed() {
    let (lab, connection, fake, pings) = vpn_lab();
    let (id, _sink) = lab.open(&connection);
    assert_eq!(*pings.lock().unwrap(), ["h"]);
    assert_eq!(fake.0.ups.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(open_holders(&lab.core), [1]);
    lab.core.ssh_close(&id);
    assert!(open_holders(&lab.core).is_empty());
}

#[test]
fn ssh_reconnect_regates_after_a_drop() {
    let (lab, connection, fake, _) = vpn_lab();
    let (id, _sink) = lab.open(&connection);
    lab.core.ssh_reconnect(&id, 80, 24).unwrap();
    assert_eq!(
        fake.0.ups.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a live hold is kept"
    );
    fake.0.leases.lock().unwrap()[0]
        .send(super::vpn::VpnPhase::Down)
        .unwrap();
    super::vpn_tests::wait_until("the drop", || open_holders(&lab.core).is_empty());
    lab.core.ssh_reconnect(&id, 80, 24).unwrap();
    assert_eq!(fake.0.ups.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(open_holders(&lab.core), [1]);
}

#[test]
fn a_failed_gate_opens_no_session() {
    let (lab, connection, fake, _) = vpn_lab();
    fake.0
        .failures
        .lock()
        .unwrap()
        .push(CommandError::new("vpnSetupRequired", "x"));
    let error = lab
        .core
        .open_ssh_session(&connection, 80, 24, |_| Arc::new(Collect::default()))
        .unwrap_err();
    assert_eq!(error.code, "vpnSetupRequired");
    assert_eq!(lab.core.ssh_session_count(), 0);
}

#[test]
fn local_terminal_never_gates() {
    let (lab, _, _, pings) = vpn_lab();
    let shell = lab.dir.path().join("fake-shell");
    std::fs::write(&shell, FAKE_SHELL).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    lab.core.set_local_shell(Some(rd_ssh::local::LocalShell {
        program: shell,
        args: Vec::new(),
    }));
    let sink = Arc::new(Collect::default());
    let for_core: Arc<dyn Sink> = sink.clone();
    lab.core
        .open_local_terminal(80, 24, None, move |_| for_core)
        .unwrap();
    sink.wait_output("READY");
    assert!(pings.lock().unwrap().is_empty());
}

#[test]
fn an_invalid_connection_fails_before_the_vpn_comes_up() {
    let (lab, connection, fake, pings) = vpn_lab_on("-oProxyCommand=evil");
    let error = lab
        .core
        .open_ssh_session(&connection, 80, 24, |_| Arc::new(Collect::default()))
        .unwrap_err();
    assert_eq!(error.code, "invalidHost");
    assert_eq!(fake.0.ups.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(pings.lock().unwrap().is_empty());
}
