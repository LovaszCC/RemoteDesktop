//! "Set up SSH key login": file names, key generation without overwriting, public key validation and the
//! remote command (run in a local `sh` on Unix), which record gets the key, refusals, and (with
//! `RD_TEST_SSHD`) the whole flow against the lab `sshd-gnu`: host key prompt, upload, key-only login.

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rd_core::draft::ConnectionDraft;
use rd_core::{Connection, ConnectionType, Credential, Library};
use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, LineEnding, PrivateKey};

use super::forwards::Paths;
use super::ssh_keys::{
    KeyOwner, generate_key_pair, install_command, key_comment, key_file_stem, key_owner, validate_public_key,
};
use super::tests::{Env, entry_draft, save_single};
use super::*;

fn public_line(comment: &str) -> String {
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
    key.set_comment(comment);
    key.public_key().to_openssh().unwrap()
}

#[test]
fn file_stems() {
    assert_eq!(key_file_stem("Web Server #1"), "rdnext_web_server_1_ed25519");
    assert_eq!(
        key_file_stem("gw-01.example.com"),
        "rdnext_gw-01_example_com_ed25519"
    );
    assert_eq!(key_file_stem("  ÁrvíztűrŐ  "), "rdnext_rv_zt_r_ed25519");
    assert_eq!(key_file_stem(""), "rdnext_key_ed25519");
    assert_eq!(key_file_stem("../.."), "rdnext_key_ed25519");
    let long = key_file_stem(&"a".repeat(100));
    assert_eq!(long, format!("rdnext_{}_ed25519", "a".repeat(40)));
}

#[test]
fn the_comment_has_no_quotes() {
    let comment = key_comment();
    assert!(comment.ends_with("RemoteDesktop"));
    assert!(!comment.contains(['\'', '"', '\\', '\n']));
}

#[test]
fn generates_key_pairs_without_overwriting() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".ssh");
    let first = generate_key_pair(&dir, "rdnext_gw_ed25519", "me@pc RemoteDesktop").unwrap();
    assert_eq!(first, dir.join("rdnext_gw_ed25519"));
    let private = PrivateKey::from_openssh(std::fs::read(&first).unwrap()).unwrap();
    assert!(!private.is_encrypted());
    assert_eq!(private.algorithm(), Algorithm::Ed25519);
    let public = std::fs::read_to_string(dir.join("rdnext_gw_ed25519.pub")).unwrap();
    assert!(public.ends_with(" me@pc RemoteDesktop\n"));
    assert_eq!(public.trim(), private.public_key().to_openssh().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&first), 0o600);
        assert_eq!(mode(&dir.join("rdnext_gw_ed25519.pub")), 0o644);
    }

    // The name is taken: `_2`; a lone `.pub` also blocks a name.
    let second = generate_key_pair(&dir, "rdnext_gw_ed25519", "c").unwrap();
    assert_eq!(second, dir.join("rdnext_gw_ed25519_2"));
    std::fs::write(dir.join("rdnext_gw_ed25519_3.pub"), "keep").unwrap();
    let fourth = generate_key_pair(&dir, "rdnext_gw_ed25519", "c").unwrap();
    assert_eq!(fourth, dir.join("rdnext_gw_ed25519_4"));
    assert_eq!(
        std::fs::read_to_string(dir.join("rdnext_gw_ed25519_3.pub")).unwrap(),
        "keep"
    );
    assert_ne!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
}

#[test]
fn validates_public_key_lines() {
    let line = public_line("me@pc RemoteDesktop");
    assert_eq!(validate_public_key(&format!("  {line}\n")).unwrap(), line);
    let (algorithm, rest) = line.split_once(' ').unwrap();
    let (data, _) = rest.split_once(' ').unwrap();
    // Whitespace between the parts is normalized; the comment keeps its own spaces.
    assert_eq!(
        validate_public_key(&format!("{algorithm}\t {data}   a  b")).unwrap(),
        format!("{algorithm} {data} a  b")
    );
    assert_eq!(
        validate_public_key(&format!("{algorithm} {data}")).unwrap(),
        format!("{algorithm} {data}")
    );
    for bad in [
        String::new(),
        "   ".to_owned(),
        algorithm.to_owned(),
        format!("{algorithm} {data} it's"),
        format!("{algorithm} {data} \"x\""),
        format!("{algorithm} {data} a\\b"),
        format!("{algorithm} {data}\nssh-ed25519 AAAA"),
        format!("{algorithm} {data}$(reboot)"),
        format!("ssh;rm {data}"),
        format!("{algorithm} AAAAnotakey"),
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA==".to_owned(),
    ] {
        assert!(validate_public_key(&bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn the_install_command_quotes_the_key() {
    let key = validate_public_key(&public_line("a b")).unwrap();
    let command = install_command(&key);
    assert!(command.starts_with("umask 077; mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys"));
    assert_eq!(command.matches(&format!("'{key}'")).count(), 2);
}

/// Runs the command in a local `sh` with `HOME` pointing at a temp folder: twice, and against a file
/// without a final newline.
#[cfg(unix)]
#[test]
fn the_install_command_is_idempotent() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let run = |command: &str| {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .env("HOME", home.path())
            .status()
            .unwrap();
        assert!(status.success());
    };
    let key = validate_public_key(&public_line("first")).unwrap();
    run(&install_command(&key));
    run(&install_command(&key));
    let file = home.path().join(".ssh/authorized_keys");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{key}\n"));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&home.path().join(".ssh")), 0o700);
    assert_eq!(mode(&file), 0o600);

    std::fs::write(&file, "ssh-rsa AAAA old").unwrap();
    let other = validate_public_key(&public_line("second")).unwrap();
    run(&install_command(&other));
    run(&install_command(&other));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("ssh-rsa AAAA old\n{other}\n")
    );
}

#[test]
fn the_credential_owns_the_key_when_linked() {
    let credential = Credential::new("ops");
    let mut linked = Connection::new(ConnectionType::Ssh, "a", "a.example.com");
    linked.credential_id = Some(credential.id.clone());
    let own = Connection::new(ConnectionType::Ssh, "b", "b.example.com");
    let mut dangling = Connection::new(ConnectionType::Ssh, "c", "c.example.com");
    dangling.credential_id = Some("gone".into());
    let library = Library {
        credentials: vec![credential.clone()],
        connections: vec![linked.clone(), own.clone(), dangling.clone()],
        ..Library::default()
    };
    assert_eq!(key_owner(&library, &linked), KeyOwner::Credential(&credential));
    assert_eq!(key_owner(&library, &own), KeyOwner::Connection(&own));
    assert_eq!(key_owner(&library, &dangling), KeyOwner::Connection(&dangling));
}

fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn ssh_draft(port: u16) -> ConnectionDraft {
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = port.to_string();
    draft.username = "tester".into();
    draft
}

/// Background connections and generated keys use `home`.
fn use_home(core: &AppCore, home: &Path) {
    core.set_forward_paths(Some(Paths {
        home: Some(home.to_path_buf()),
        known_hosts: Some(vec![home.join("known_hosts")]),
        no_agent: true,
    }));
}

fn setup(core: &AppCore, id: &str) -> Result<KeySetupDto, CommandError> {
    rd_ssh::background::runtime().block_on(core.ssh_key_setup(id))
}

#[test]
fn refusals() {
    let env = Env::new();
    let core = env.core();
    assert_eq!(setup(&core, "x").unwrap_err().code, "locked");
    core.create_vault("master", true).unwrap();
    assert_eq!(setup(&core, "missing").unwrap_err().code, "notFound");
    let rdp = {
        let draft = ConnectionDraft::new(ConnectionType::Rdp);
        save_single(
            &core,
            None,
            &entry_draft("dc", "dc.example.com"),
            draft,
            None,
            None,
        )
        .unwrap()
    };
    assert_eq!(setup(&core, &rdp).unwrap_err().code, "notSsh");
    // An unknown request id is not a key setup prompt.
    core.host_key_answer("nope", true);
}

#[test]
fn a_missing_key_is_generated_and_saved_on_its_owner() {
    let env = Env::new();
    let core = env.unlocked();
    let home = tempfile::tempdir().unwrap();
    use_home(&core, home.path());
    let port = closed_port();

    // Own login: the connection gets the key, even though the upload then fails.
    let web1 = entry_draft("Web 1", "127.0.0.1");
    let own = save_single(&core, None, &web1, ssh_draft(port), Some("pw"), None).unwrap();
    assert_eq!(setup(&core, &own).unwrap_err().code, "connectFailed");
    let library = core.library();
    let saved = library.connections.iter().find(|c| c.id == own).unwrap();
    assert_eq!(saved.ssh_key_path.as_deref(), Some("~/.ssh/rdnext_web_1_ed25519"));
    assert!(home.path().join(".ssh/rdnext_web_1_ed25519.pub").is_file());

    // Again: the existing key is used, nothing new is generated.
    assert_eq!(setup(&core, &own).unwrap_err().code, "connectFailed");
    assert!(!home.path().join(".ssh/rdnext_web_1_ed25519_2").exists());

    // Linked credential: the credential gets the key; the connection's own field stays empty.
    let credential = core
        .save_credential(
            None,
            &CredentialInput {
                name: "ops".into(),
                username: "tester".into(),
                ..CredentialInput::default()
            },
            Some("pw"),
            None,
        )
        .unwrap();
    let linked = save_single(&core, None, &web1, ssh_draft(port), None, None).unwrap();
    core.set_credential(Some(&credential), &[], std::slice::from_ref(&linked))
        .unwrap();
    assert_eq!(setup(&core, &linked).unwrap_err().code, "connectFailed");
    let library = core.library();
    let saved = library.credentials.iter().find(|c| c.id == credential).unwrap();
    assert_eq!(
        saved.ssh_key_path.as_deref(),
        Some("~/.ssh/rdnext_web_1_ed25519_2")
    );
    let connection = library.connections.iter().find(|c| c.id == linked).unwrap();
    assert_eq!(connection.ssh_key_path, None);
}

#[test]
fn an_unreadable_key_and_read_only_mode() {
    let env = Env::new();
    let core = env.unlocked();
    let home = tempfile::tempdir().unwrap();
    use_home(&core, home.path());
    let gw = entry_draft("gw", "127.0.0.1");
    let mut draft = ssh_draft(closed_port());
    draft.ssh_key_path = "~/.ssh/missing".into();
    let id = save_single(&core, None, &gw, draft.clone(), None, None).unwrap();
    let error = setup(&core, &id).unwrap_err();
    assert_eq!(error.code, "cannotReadKey");
    assert!(error.detail.unwrap().ends_with("missing"));

    // The private key alone is enough: its public half is read without the passphrase.
    let key_path = home.path().join(".ssh/enc");
    std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
    super::ssh_unlock::tests::write_encrypted_key(&key_path, "secret");
    draft.ssh_key_path = "~/.ssh/enc".into();
    save_single(&core, Some(&id), &gw, draft, None, None).unwrap();
    assert_eq!(setup(&core, &id).unwrap_err().code, "connectFailed");

    // A broken `.pub` is refused before connecting.
    std::fs::write(home.path().join(".ssh/enc.pub"), "ssh-ed25519 AAAA 'x'").unwrap();
    assert_eq!(setup(&core, &id).unwrap_err().code, "invalidPublicKey");
}

#[test]
fn the_dto_serializes_camel_case() {
    let dto = KeySetupDto {
        host: "gw".into(),
        key_path: "~/.ssh/k".into(),
        generated: true,
        verified: false,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["keyPath"], "~/.ssh/k");
    assert_eq!(json["generated"], true);
    let encoded = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)
        .unwrap()
        .to_openssh(LineEnding::LF)
        .unwrap();
    assert!(encoded.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
}

/// The whole flow against the lab `sshd-gnu` (`RD_TEST_SSHD=127.0.0.1:2221`): a password connection
/// without a key gets a new one, the host key prompt is accepted, the key is installed and a key-only
/// login works; a second run reuses the key. Without the variable it does nothing.
#[test]
fn lab_sshd_key_setup() {
    let Ok(target) = std::env::var("RD_TEST_SSHD") else {
        return;
    };
    let (host, port) = target.rsplit_once(':').expect("RD_TEST_SSHD is host:port");
    let env = Env::new();
    let core = Arc::new(env.unlocked());
    let home = tempfile::tempdir().unwrap();
    use_home(&core, home.path());
    let (sender, receiver) = mpsc::channel();
    core.set_key_setup_hooks(Arc::new(move |prompt| {
        let _ = sender.send(prompt);
    }));
    let answerer = Arc::clone(&core);
    let prompts = std::thread::spawn(move || {
        let prompt = receiver.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(prompt.forward_id.starts_with("ssh-key-"));
        assert!(prompt.fingerprint.starts_with("SHA256:"));
        answerer.host_key_answer(&prompt.request_id, true);
        // Known now: no more prompts.
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_err());
    });
    let lab = entry_draft("lab key", host);
    let id = save_single(
        &core,
        None,
        &lab,
        ssh_draft(port.parse().unwrap()),
        Some("tester-pass"),
        None,
    )
    .unwrap();
    let first = setup(&core, &id).unwrap();
    assert_eq!(first.host, host);
    assert_eq!(first.key_path, "~/.ssh/rdnext_lab_key_ed25519");
    assert!(first.generated);
    assert!(first.verified);

    // The password is no longer needed.
    let mut draft = ssh_draft(port.parse().unwrap());
    draft.ssh_key_path = first.key_path.clone();
    save_single(&core, Some(&id), &lab, draft, Some(""), None).unwrap();
    let second = setup(&core, &id).unwrap();
    assert!(!second.generated);
    assert!(second.verified);
    prompts.join().unwrap();
}
