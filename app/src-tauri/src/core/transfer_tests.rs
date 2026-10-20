//! Export → preview → commit through `AppCore`.

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;
use rd_store::TransferAction;
use secrecy::ExposeSecret;

use super::library::CredentialInput;
use super::tests::{Env, entry_draft, save_single};
use super::*;
use crate::dto::TransferDecisionDto;

/// An unlocked core with entry "dc01" (RDP linked to "Domain admin") and "web" (own password).
fn seeded(env: &Env) -> (AppCore, String, String) {
    let core = env.unlocked();
    let credential = core
        .save_credential(
            None,
            &CredentialInput {
                name: "Domain admin".into(),
                username: "admin".into(),
                ssh_key_path: "~/.ssh/id_ed25519".into(),
                ..CredentialInput::default()
            },
            Some("cred-pw"),
            None,
        )
        .unwrap();
    let mut rdp = ConnectionDraft::new(ConnectionType::Rdp);
    rdp.credential_id = Some(credential.clone());
    save_single(
        &core,
        None,
        &entry_draft("dc01", "dc01.acme.local"),
        rdp,
        None,
        None,
    )
    .unwrap();
    let mut ssh = ConnectionDraft::new(ConnectionType::Ssh);
    ssh.username = "root".into();
    save_single(
        &core,
        None,
        &entry_draft("web", "web.acme.local"),
        ssh,
        Some("own-pw"),
        None,
    )
    .unwrap();
    let entry = core
        .library()
        .entries
        .iter()
        .find(|e| e.name == "dc01")
        .unwrap()
        .id
        .clone();
    (core, entry, credential)
}

fn all_entries(core: &AppCore) -> Vec<String> {
    core.library().entries.iter().map(|e| e.id.clone()).collect()
}

fn decide(pairs: &[(&str, TransferAction)]) -> Vec<TransferDecisionDto> {
    pairs
        .iter()
        .map(|(id, action)| TransferDecisionDto {
            id: (*id).to_owned(),
            action: *action,
        })
        .collect()
}

#[test]
fn encrypted_export_round_trips_into_another_library() {
    let env = Env::new();
    let (core, _, credential) = seeded(&env);
    let path = env.db("share.rdexport");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let result = core
        .export_library(&path, &all_entries(&core), &[credential], Some("share"))
        .unwrap();
    assert_eq!(
        (result.entries, result.credentials, result.encrypted),
        (2, 1, true)
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("cred-pw") && !text.contains("dc01"));

    let other = env.core_for("other.sqlite", rd_core::shortcuts::Platform::MacOS);
    other.create_vault("master2", false).unwrap();
    assert_eq!(
        other.transfer_preview(&path, None).unwrap_err().code,
        "transferNeedsPassword"
    );
    assert_eq!(
        other.transfer_preview(&path, Some("nope")).unwrap_err().code,
        "transferWrongPassword"
    );
    let preview = other.transfer_preview(&path, Some("share")).unwrap();
    assert!(preview.encrypted);
    let ids: Vec<_> = preview.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["e0", "e1", "k0"]);
    assert!(preview.rows.iter().all(|r| !r.conflict));
    assert_eq!(preview.rows[0].credential_ids, ["k0"]);
    assert!(preview.rows[1].has_password && preview.rows[2].has_password);
    let json = serde_json::to_string(&preview).unwrap();
    assert!(!json.contains("cred-pw") && !json.contains("own-pw"));

    let result = other
        .transfer_commit(
            &path,
            Some("share"),
            &decide(&[
                ("e0", TransferAction::New),
                ("e1", TransferAction::New),
                ("k0", TransferAction::New),
            ]),
        )
        .unwrap();
    assert_eq!((result.entries_created, result.credentials_created), (2, 1));
    let (password, own, key_path, linked) = {
        let inner = other.inner();
        let vault = inner.vault.as_ref().unwrap();
        let library = inner.db.as_ref().unwrap().library().unwrap();
        let credential = &library.credentials[0];
        let ssh = library
            .connections
            .iter()
            .find(|c| c.kind == ConnectionType::Ssh)
            .unwrap();
        let rdp = library
            .connections
            .iter()
            .find(|c| c.kind == ConnectionType::Rdp)
            .unwrap();
        (
            vault
                .open(credential.password_enc.as_ref().unwrap())
                .unwrap()
                .expose_secret()
                .to_owned(),
            vault
                .open(ssh.password_enc.as_ref().unwrap())
                .unwrap()
                .expose_secret()
                .to_owned(),
            credential.ssh_key_path.clone(),
            rdp.credential_id.as_deref() == Some(credential.id.as_str()),
        )
    };
    assert_eq!((password.as_str(), own.as_str()), ("cred-pw", "own-pw"));
    assert_eq!(key_path, None);
    assert!(linked);
}

#[test]
fn plain_export_has_no_passwords_and_conflicts_are_flagged() {
    let env = Env::new();
    let (core, entry, _) = seeded(&env);
    let path = env.db("plain.rdexport");
    let result = core
        .export_library(&path, std::slice::from_ref(&entry), &[], None)
        .unwrap();
    assert!(!result.encrypted);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("dc01") && !text.contains("password"));

    let preview = core.transfer_preview(&path, None).unwrap();
    assert!(!preview.encrypted);
    assert_eq!(preview.rows.len(), 1);
    assert!(preview.rows[0].conflict);
    assert!(preview.rows[0].credential_ids.is_empty());

    // Rows without a decision are skipped.
    let skipped = core.transfer_commit(&path, None, &[]).unwrap();
    assert_eq!(skipped, crate::dto::TransferResultDto::default());
    let replaced = core
        .transfer_commit(&path, None, &decide(&[("e0", TransferAction::Overwrite)]))
        .unwrap();
    assert_eq!(replaced.entries_updated, 1);
    // The local credential link stays when the file has none.
    let library = core.library();
    assert_eq!(library.entries.len(), 2);
    let rdp = library.connections.iter().find(|c| c.entry_id == entry).unwrap();
    assert!(rdp.credential_id.is_some());
}

#[test]
fn refusals() {
    let env = Env::new();
    let (core, entry, _) = seeded(&env);
    let path = env.db("x.rdexport");
    assert_eq!(
        core.export_library(&path, std::slice::from_ref(&entry), &[], Some(""))
            .unwrap_err()
            .code,
        "emptyPassword"
    );
    let missing = env.db("missing/dir/x.rdexport");
    assert_eq!(
        core.export_library(&missing, std::slice::from_ref(&entry), &[], None)
            .unwrap_err()
            .code,
        "exportWrite"
    );
    std::fs::write(&path, b"{}").unwrap();
    assert_eq!(
        core.transfer_preview(&path, None).unwrap_err().code,
        "transferUnreadable"
    );
    std::fs::write(
        &path,
        br#"{"format":"remotedesktop-export","version":9,"encrypted":false}"#,
    )
    .unwrap();
    assert_eq!(
        core.transfer_preview(&path, None).unwrap_err().code,
        "transferNewer"
    );

    core.export_library(&path, std::slice::from_ref(&entry), &[], None)
        .unwrap();
    core.lock();
    assert_eq!(
        core.export_library(&path, &[entry], &[], None).unwrap_err().code,
        "locked"
    );
    assert_eq!(core.transfer_preview(&path, None).unwrap_err().code, "locked");
    assert_eq!(core.transfer_commit(&path, None, &[]).unwrap_err().code, "locked");
}
