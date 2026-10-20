//! Port forwards in `AppCore`: validation codes, save/edit/delete, tunnel states, restart on edit, and
//! stopping on lock, database switch, connection delete and quit. No SSH server is needed: a listener
//! that never answers keeps a tunnel `starting`, a closed port fails it.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;
use rd_core::forward::PortForwardDraft;
use rd_ssh::background::tunnel::ForwardState;

use super::forwards::Paths;
use super::tests::{Env, entry_draft, save_single};
use super::*;

struct Notices {
    receiver: mpsc::Receiver<ForwardNotice>,
    _home: tempfile::TempDir,
}

impl Notices {
    fn next_status(&self) -> ForwardStatusDto {
        loop {
            if let ForwardNotice::Status(status) =
                self.receiver.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                return status;
            }
        }
    }

    fn quiet(&self) -> bool {
        self.receiver.recv_timeout(Duration::from_millis(200)).is_err()
    }
}

/// Collects notices; background connections use a temp home without an agent.
fn hook(core: &AppCore) -> Notices {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    core.set_forward_hooks(Arc::new(move |notice| {
        let _ = sender.lock().unwrap().send(notice);
    }));
    let home = tempfile::tempdir().unwrap();
    core.set_forward_paths(Some(Paths {
        home: Some(home.path().to_path_buf()),
        known_hosts: Some(vec![home.path().join("known_hosts")]),
        no_agent: true,
    }));
    Notices {
        receiver,
        _home: home,
    }
}

fn ssh_connection(core: &AppCore, host: &str, port: u16) -> String {
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = port.to_string();
    draft.username = "tester".into();
    save_single(core, None, &entry_draft("gw", host), draft, Some("secret"), None).unwrap()
}

fn forward(connection: &str, local: u16, host: &str, remote: &str) -> PortForwardDraft {
    PortForwardDraft {
        id: None,
        connection_id: connection.to_owned(),
        name: String::new(),
        local_port: local.to_string(),
        remote_host: host.to_owned(),
        remote_port: remote.to_owned(),
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A TCP server that accepts and never says anything: the SSH handshake hangs.
fn silent_server() -> (std::net::TcpListener, u16) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

#[test]
fn save_validates_and_edits_keep_the_id() {
    let env = Env::new();
    let core = env.unlocked();
    let gw = ssh_connection(&core, "gw.example.com", 22);
    let refusal = |draft: &PortForwardDraft| core.port_forward_save(draft).unwrap_err().code;

    assert_eq!(refusal(&forward(&gw, 0, "h", "22")), "invalidLocalPort");
    let mut big = forward(&gw, 1, "h", "22");
    big.local_port = "65536".into();
    assert_eq!(refusal(&big), "invalidLocalPort");
    assert_eq!(refusal(&forward(&gw, 8080, " ", "22")), "missingRemoteHost");
    assert_eq!(refusal(&forward(&gw, 8080, "h", "0")), "invalidRemotePort");
    assert_eq!(refusal(&forward(&gw, 8080, "h", "x")), "invalidRemotePort");
    assert_eq!(refusal(&forward("missing", 8080, "h", "22")), "notFound");
    let rdp = ConnectionDraft::new(ConnectionType::Rdp);
    let rdp = save_single(&core, None, &entry_draft("dc", "dc.example.com"), rdp, None, None).unwrap();
    assert_eq!(refusal(&forward(&rdp, 8080, "h", "22")), "notSsh");

    let id = core
        .port_forward_save(&forward(&gw, 13389, "192.0.2.20", "3389"))
        .unwrap();
    assert_eq!(
        refusal(&forward(&gw, 13389, "192.0.2.21", "22")),
        "duplicateLocalPort"
    );
    let mut edit = forward(&gw, 13389, "192.0.2.21", "3390");
    edit.id = Some(id.clone());
    assert_eq!(core.port_forward_save(&edit).unwrap(), id);
    let library = core.library();
    assert_eq!(library.port_forwards.len(), 1);
    assert_eq!(library.port_forwards[0].remote_host, "192.0.2.21");

    core.port_forward_delete(&id).unwrap();
    assert!(core.library().port_forwards.is_empty());

    core.lock();
    assert_eq!(refusal(&forward(&gw, 1, "h", "1")), "locked");
    assert_eq!(core.port_forward_start(&id).unwrap_err().code, "locked");
}

#[test]
fn failures_are_reported() {
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    // Nothing listens on the SSH port.
    let gw = ssh_connection(&core, "127.0.0.1", free_port());
    let id = core
        .port_forward_save(&forward(&gw, free_port(), "127.0.0.1", "22"))
        .unwrap();
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    let failed = notices.next_status();
    assert_eq!(failed.id, id);
    assert_eq!(failed.connection_id, gw);
    assert!(
        matches!(&failed.state, ForwardState::Failed { code, .. } if code == "connectFailed"),
        "{failed:?}"
    );
    assert_eq!(core.running_count(), 0);
    // Failed tunnels keep their error for the UI.
    assert_eq!(core.port_forward_statuses(), [failed]);

    // The local port is taken.
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy_port = busy.local_addr().unwrap().port();
    let id = core
        .port_forward_save(&forward(&gw, busy_port, "127.0.0.1", "22"))
        .unwrap();
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    assert!(matches!(
        notices.next_status().state,
        ForwardState::Failed { code, .. } if code == "portInUse"
    ));
    assert_eq!(core.port_forward_start("missing").unwrap_err().code, "notFound");
}

#[test]
fn running_tunnels_count_restart_on_edit_and_stop() {
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let (_server, port) = silent_server();
    let gw = ssh_connection(&core, "127.0.0.1", port);
    let id = core
        .port_forward_save(&forward(&gw, free_port(), "127.0.0.1", "22"))
        .unwrap();

    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    assert_eq!(core.running_count(), 1);
    assert_eq!(core.running_forwards(), 1);

    // Editing a running forward restarts it.
    let mut edit = forward(&gw, free_port(), "127.0.0.1", "2222");
    edit.id = Some(id.clone());
    core.port_forward_save(&edit).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    assert_eq!(core.running_count(), 1);

    core.port_forward_stop(&id);
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(core.running_count(), 0);
    assert!(core.port_forward_statuses().is_empty());
    // Stopping again changes nothing; editing a stopped forward doesn't start it.
    core.port_forward_stop(&id);
    core.port_forward_save(&edit).unwrap();
    assert!(notices.quiet());

    // Lock stops everything.
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    core.lock();
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(core.running_count(), 0);
}

#[test]
fn deleting_switching_and_quitting_stop_tunnels() {
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let (_server, port) = silent_server();
    let gw = ssh_connection(&core, "127.0.0.1", port);
    let id = core
        .port_forward_save(&forward(&gw, free_port(), "127.0.0.1", "22"))
        .unwrap();

    // Deleting the connection (its forwards cascade) stops the tunnel.
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    core.delete_items(&[format!("c:{gw}")], false).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(core.running_count(), 0);

    // A database switch stops it.
    let gw = ssh_connection(&core, "127.0.0.1", port);
    let id = core
        .port_forward_save(&forward(&gw, free_port(), "127.0.0.1", "22"))
        .unwrap();
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    core.open_database(&env.db("other.sqlite"));
    assert_eq!(notices.next_status().state, ForwardState::Stopped);

    // Quitting stops it.
    core.open_database(&env.db("connections.sqlite"));
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    core.close();
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(core.running_count(), 0);
}

#[test]
fn deleting_a_running_forward_stops_it() {
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let (_server, port) = silent_server();
    let gw = ssh_connection(&core, "127.0.0.1", port);
    let id = core
        .port_forward_save(&forward(&gw, free_port(), "127.0.0.1", "22"))
        .unwrap();
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    core.port_forward_delete(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    assert_eq!(core.running_count(), 0);
    // Answering an unknown prompt is harmless.
    core.host_key_answer("nope", true);
}

#[test]
fn status_serialization() {
    let dto = ForwardStatusDto {
        id: "F".into(),
        connection_id: "C".into(),
        state: ForwardState::failed("timeout", "late"),
    };
    assert_eq!(
        serde_json::to_value(&dto).unwrap(),
        serde_json::json!({"id": "F", "connectionId": "C", "state": "failed", "code": "timeout", "error": "late"})
    );
    let prompt = HostKeyPromptDto {
        request_id: "R".into(),
        forward_id: "F".into(),
        host: "example.com".into(),
        port: 22,
        key_type: "ssh-ed25519".into(),
        fingerprint: "SHA256:x".into(),
        changed: None,
    };
    assert_eq!(serde_json::to_value(&prompt).unwrap()["keyType"], "ssh-ed25519");
    assert_eq!(
        serde_json::to_value(&prompt).unwrap()["changed"],
        serde_json::Value::Null
    );
    let changed = HostKeyPromptDto {
        changed: Some(rd_ssh::background::StoredKey {
            file: "/kh".into(),
            line: 3,
        }),
        ..prompt
    };
    assert_eq!(
        serde_json::to_value(&changed).unwrap()["changed"],
        serde_json::json!({"file": "/kh", "line": 3})
    );
}

/// The whole app flow against the lab `sshd-gnu` (`RD_TEST_SSHD=127.0.0.1:2221`): the vault password,
/// the host key prompt event answered with Accept, the tunnel active and carrying the container's own
/// SSH banner. Without the variable it does nothing.
#[test]
fn lab_sshd_tunnel_with_host_key_prompt() {
    use std::io::Read;

    let Ok(target) = std::env::var("RD_TEST_SSHD") else {
        return;
    };
    let (host, port) = target.rsplit_once(':').expect("RD_TEST_SSHD is host:port");
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = port.into();
    draft.username = "tester".into();
    let gw = save_single(
        &core,
        None,
        &entry_draft("lab", host),
        draft,
        Some("tester-pass"),
        None,
    )
    .unwrap();
    let local = free_port();
    let id = core
        .port_forward_save(&forward(&gw, local, "127.0.0.1", "22"))
        .unwrap();

    core.port_forward_start(&id).unwrap();
    let prompt = loop {
        match notices.receiver.recv_timeout(Duration::from_secs(20)).unwrap() {
            ForwardNotice::HostKeyPrompt(prompt) => break prompt,
            ForwardNotice::Status(status) => assert_eq!(status.state, ForwardState::Starting),
            ForwardNotice::HostKeyClosed(_) => panic!("closed"),
        }
    };
    assert_eq!(prompt.forward_id, id);
    assert!(prompt.fingerprint.starts_with("SHA256:"));
    // The prompt pauses the 30 s start clock; answer it.
    core.host_key_answer(&prompt.request_id, true);
    assert_eq!(notices.next_status().state, ForwardState::Active);
    assert_eq!(core.running_count(), 1);

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", local)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut banner = [0u8; 8];
    stream.read_exact(&mut banner).unwrap();
    assert_eq!(&banner, b"SSH-2.0-");

    core.port_forward_stop(&id);
    assert_eq!(notices.next_status().state, ForwardState::Stopped);
    // Known now: the next start needs no prompt.
    core.port_forward_start(&id).unwrap();
    assert_eq!(notices.next_status().state, ForwardState::Starting);
    assert_eq!(notices.next_status().state, ForwardState::Active);
    core.close();
}
