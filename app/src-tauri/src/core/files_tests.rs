//! The file browsers in `AppCore`: ids, the local listing, refusals, a failed connection, closing on lock,
//! and (with `RD_TEST_SSHD`) the whole flow against the lab `sshd-gnu`: host key prompt, listing, a
//! transfer with a conflict, progress and the done report.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;
use rd_ssh::files::{ConflictChoice, Direction, ItemStatus, RemoteMode};

use super::forwards::Paths;
use super::tests::Env;
use super::*;

struct Notices {
    receiver: mpsc::Receiver<FilesNotice>,
    _home: tempfile::TempDir,
}

impl Notices {
    fn next(&self) -> FilesNotice {
        self.receiver.recv_timeout(Duration::from_secs(20)).unwrap()
    }

    fn next_status(&self) -> FilesStatusDto {
        loop {
            if let FilesNotice::Status(status) = self.next() {
                return status;
            }
        }
    }
}

fn hook(core: &AppCore) -> Notices {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    core.set_files_hooks(Arc::new(move |notice| {
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

fn connection(core: &AppCore, kind: ConnectionType, host: &str, port: u16, password: &str) -> String {
    let mut draft = ConnectionDraft::new(kind, None);
    draft.name = "lab".into();
    draft.host = host.into();
    draft.port = port.to_string();
    draft.username = "tester".into();
    core.save_connection(None, &draft, Some(password), None).unwrap()
}

fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn browser_ids_and_the_local_listing() {
    assert_eq!(browser_id("AB-12_x"), "files-AB-12_x");
    assert_eq!(browser_id("a:b/c d"), "files-a_b_c_d");
    assert!(is_files_label("files-x"));
    assert!(!is_files_label("main"));

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("a.txt"), "abc").unwrap();
    std::fs::write(dir.path().join(".hidden"), "x").unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let listing = AppCore::files_list_local(Some(&path), false).unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["sub", "a.txt"]);
    assert!(listing.parent.is_some());
    assert_eq!(
        AppCore::files_list_local(Some(&path), true)
            .unwrap()
            .entries
            .len(),
        3
    );
    assert!(AppCore::files_list_local(None, false).is_ok());
    let missing = dir.path().join("none").to_string_lossy().into_owned();
    assert_eq!(
        AppCore::files_list_local(Some(&missing), false).unwrap_err().code,
        "listFailed"
    );
}

#[test]
fn refusals_and_unknown_browsers() {
    let env = Env::new();
    let core = env.unlocked();
    let rdp = {
        let mut draft = ConnectionDraft::new(ConnectionType::Rdp, None);
        draft.name = "dc".into();
        draft.host = "dc.example.com".into();
        core.save_connection(None, &draft, None, None).unwrap()
    };
    assert_eq!(core.files_open(&rdp, None, None).unwrap_err().code, "notSsh");
    assert_eq!(
        core.files_open("missing", None, None).unwrap_err().code,
        "notFound"
    );
    assert_eq!(core.files_info("files-x").unwrap_err().code, "browserNotFound");
    assert_eq!(
        core.files_reconnect("files-x").unwrap_err().code,
        "browserNotFound"
    );
    let input = TransferInput {
        direction: Direction::Download,
        sources: vec!["/x".into()],
        destination: "/tmp".into(),
        gzip: true,
    };
    assert_eq!(
        core.files_transfer("files-x", input).unwrap_err().code,
        "browserNotFound"
    );
    let runtime = rd_ssh::background::runtime();
    let error = runtime
        .block_on(core.files_list_remote("files-x", "~", false))
        .unwrap_err();
    assert_eq!(error.code, "browserNotFound");
    core.files_cancel("files-x");
    core.files_conflict_answer("nope", ConflictChoice::Skip, false);
    core.host_key_answer("nope", true);
    core.files_close("files-x");

    assert!(!core.files_target());
    assert!(core.set_files_target(true));
    assert!(!core.set_files_target(true));
    assert!(core.files_target());

    core.lock();
    assert_eq!(core.files_open(&rdp, None, None).unwrap_err().code, "locked");
}

#[test]
fn a_failed_connection_and_closing_on_lock() {
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let id = connection(&core, ConnectionType::Mikrotik, "127.0.0.1", closed_port(), "pw");
    let opened = core
        .files_open(&id, Some("/var/log".into()), Some("no-session"))
        .unwrap();
    assert!(opened.created);
    assert_eq!(opened.browser_id, browser_id(&id));
    assert_eq!(opened.connection_name, "lab");
    assert_eq!(notices.next_status().state, BrowserState::Connecting);
    let failed = notices.next_status();
    assert!(
        matches!(&failed.state, BrowserState::Failed { code, .. } if code == "connectFailed"),
        "{failed:?}"
    );
    assert_eq!(failed.start_path, "/var/log");
    assert!(!failed.transferring);

    let again = core.files_open(&id, None, None).unwrap();
    assert!(!again.created, "one browser per connection");
    let runtime = rd_ssh::background::runtime();
    let error = runtime
        .block_on(core.files_list_remote(&opened.browser_id, "~", false))
        .unwrap_err();
    assert_eq!(error.code, "notConnected");
    let mut input = TransferInput {
        direction: Direction::Upload,
        sources: Vec::new(),
        destination: "/tmp".into(),
        gzip: false,
    };
    assert_eq!(
        core.files_transfer(&opened.browser_id, input.clone())
            .unwrap_err()
            .code,
        "nothingToCopy"
    );
    input.sources.push("/etc/hosts".into());
    assert_eq!(
        core.files_transfer(&opened.browser_id, input).unwrap_err().code,
        "notConnected"
    );
    assert_eq!(core.running_count(), 0);

    core.files_reconnect(&opened.browser_id).unwrap();
    assert_eq!(notices.next_status().state, BrowserState::Connecting);

    core.lock();
    loop {
        if let FilesNotice::Closed(closed) = notices.next() {
            assert_eq!(closed, opened.browser_id);
            break;
        }
    }
    assert_eq!(
        core.files_info(&opened.browser_id).unwrap_err().code,
        "browserNotFound"
    );
}

#[test]
fn start_path_defaults_to_home() {
    let env = Env::new();
    let core = env.unlocked();
    let _notices = hook(&core);
    let id = connection(&core, ConnectionType::Ssh, "127.0.0.1", closed_port(), "pw");
    let opened = core.files_open(&id, Some("  ".into()), None).unwrap();
    assert_eq!(core.files_info(&opened.browser_id).unwrap().start_path, "~");
    core.files_close(&opened.browser_id);
    assert!(core.files_info(&opened.browser_id).is_err());
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario against the lab server
fn lab_sshd_file_browser() {
    let Ok(target) = std::env::var("RD_TEST_SSHD") else {
        return;
    };
    let (host, port) = target.rsplit_once(':').expect("RD_TEST_SSHD is host:port");
    let env = Env::new();
    let core = env.unlocked();
    let notices = hook(&core);
    let id = connection(
        &core,
        ConnectionType::Ssh,
        host,
        port.parse().unwrap(),
        "tester-pass",
    );
    let browser = core.files_open(&id, None, None).unwrap().browser_id;
    let prompt = loop {
        match notices.next() {
            FilesNotice::HostKeyPrompt(prompt) => break prompt,
            FilesNotice::Status(status) => assert_eq!(status.state, BrowserState::Connecting),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(prompt.browser_id, browser);
    assert!(prompt.fingerprint.starts_with("SHA256:"));
    core.host_key_answer(&prompt.request_id, true);
    assert_eq!(
        notices.next_status().state,
        BrowserState::Ready {
            mode: RemoteMode::Tar
        }
    );

    let runtime = rd_ssh::background::runtime();
    let home = runtime
        .block_on(core.files_list_remote(&browser, "~", true))
        .unwrap();
    assert_eq!(home.path, "/home/tester");
    assert_eq!(home.parent.as_deref(), Some("/home"));
    let root = runtime
        .block_on(core.files_list_remote(&browser, "/", false))
        .unwrap();
    assert_eq!(root.parent, None);
    assert!(root.entries.iter().all(|e| !e.name.starts_with('.')));
    let error = runtime
        .block_on(core.files_list_remote(&browser, "/no/such/dir", false))
        .unwrap_err();
    assert_eq!(error.code, "listFailed");

    // Upload a file twice: the second time asks, Skip.
    let local = tempfile::tempdir().unwrap();
    let name = format!(
        "rd-core-it-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let file = local.path().join(&name);
    std::fs::write(&file, "core\n").unwrap();
    let input = TransferInput {
        direction: Direction::Upload,
        sources: vec![file.to_string_lossy().into_owned()],
        destination: "/tmp".into(),
        gzip: true,
    };
    let done = |notices: &Notices| loop {
        if let FilesNotice::Done(done) = notices.next() {
            return done;
        }
    };
    let first = core.files_transfer(&browser, input.clone()).unwrap();
    let report = done(&notices);
    assert_eq!(report.transfer_id, first);
    assert_eq!(report.items[0].status, ItemStatus::Done, "{report:?}");
    assert_eq!(core.running_count(), 0);

    core.files_transfer(&browser, input.clone()).unwrap();
    assert_eq!(
        core.files_transfer(&browser, input).unwrap_err().code,
        "transferRunning",
        "one transfer at a time"
    );
    let conflict = loop {
        match notices.next() {
            FilesNotice::Conflict(conflict) => break conflict,
            FilesNotice::Done(done) => panic!("{done:?}"),
            _ => {}
        }
    };
    assert_eq!(conflict.question.name, name);
    assert!(!conflict.question.is_folder);
    assert_eq!(
        core.running_count(),
        1,
        "a running transfer counts for the quit dialog"
    );
    core.files_conflict_answer(&conflict.request_id, ConflictChoice::Skip, false);
    assert_eq!(done(&notices).items[0].status, ItemStatus::Skipped);

    // Download it back.
    let back = tempfile::tempdir().unwrap();
    core.files_transfer(
        &browser,
        TransferInput {
            direction: Direction::Download,
            sources: vec![format!("/tmp/{name}")],
            destination: back.path().to_string_lossy().into_owned(),
            gzip: false,
        },
    )
    .unwrap();
    assert_eq!(done(&notices).items[0].status, ItemStatus::Done);
    assert_eq!(
        std::fs::read_to_string(back.path().join(&name)).unwrap(),
        "core\n"
    );
    core.close();
}
