//! CSV sample → preview → commit through `AppCore`.

use rd_core::ConnectionType;
use rd_core::transfer::CsvSkipReason;
use rd_store::TransferAction;

use super::tests::Env;
use crate::dto::TransferDecisionDto;

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
fn the_sample_imports_and_a_second_import_finds_conflicts() {
    let env = Env::new();
    let core = env.unlocked();
    let path = env.db("sample.csv");
    core.save_csv_sample(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("folder;name;host;type"));

    let preview = core.csv_preview(&path).unwrap();
    assert_eq!(preview.file_name, "sample.csv");
    assert!(preview.skipped.is_empty());
    let ids: Vec<_> = preview.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["e0", "e1", "e2"]);
    assert_eq!(
        preview.rows[0].connection_types,
        [ConnectionType::Rdp, ConnectionType::Ssh]
    );
    assert!(preview.rows.iter().all(|r| !r.conflict && !r.has_password));

    let all = decide(&[
        ("e0", TransferAction::New),
        ("e1", TransferAction::New),
        ("e2", TransferAction::New),
    ]);
    let result = core.csv_commit(&path, &all).unwrap();
    assert_eq!((result.entries_created, result.folders_created), (3, 2));
    let library = core.library();
    let dc01 = library.entries.iter().find(|e| e.name == "dc01").unwrap();
    assert_eq!(dc01.folder_path, "Clients / Acme");
    assert_eq!(dc01.connection_ids.len(), 2);

    let again = core.csv_preview(&path).unwrap();
    assert!(again.rows.iter().all(|r| r.conflict));
    let replaced = core
        .csv_commit(&path, &decide(&[("e0", TransferAction::Overwrite)]))
        .unwrap();
    assert_eq!((replaced.entries_updated, replaced.entries_created), (1, 0));
    assert_eq!(core.library().entries.len(), 3);
}

#[test]
fn the_sample_follows_the_language() {
    let env = Env::new();
    let core = env.unlocked();
    core.inner().settings.language = "hu".into();
    let path = env.db("minta.csv");
    core.save_csv_sample(&path).unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("mappa;név;host;típus")
    );
}

#[test]
fn skipped_rows_and_refusals() {
    let env = Env::new();
    let core = env.unlocked();
    let path = env.db("list.csv");
    std::fs::write(&path, "name;host;type;folder\nok;h;rdp;\nbad;h;vnc;\n").unwrap();
    let preview = core.csv_preview(&path).unwrap();
    assert_eq!(preview.rows.len(), 1);
    assert_eq!(preview.skipped[0].line, 3);
    assert_eq!(
        preview.skipped[0].reason,
        CsvSkipReason::UnknownType("vnc".into())
    );

    let refusal = |content: &[u8]| {
        std::fs::write(&path, content).unwrap();
        let error = core.csv_preview(&path).unwrap_err();
        (error.code, error.detail)
    };
    assert_eq!(refusal(b"").0, "importEmpty");
    assert_eq!(refusal(b"name;host;type;folder\n").0, "importEmpty");
    assert_eq!(refusal(&[0xff, 0xfe]).0, "csvEncoding");
    assert_eq!(refusal(b"name;host;type;folder\n\"x").0, "csvUnreadable");
    assert_eq!(
        refusal(b"name;host\nx;h\n"),
        ("csvMissingColumns".to_owned(), Some("folder, type".to_owned()))
    );
    assert_eq!(
        core.save_csv_sample(&env.db("missing/dir/x.csv"))
            .unwrap_err()
            .code,
        "exportWrite"
    );

    core.lock();
    assert_eq!(core.csv_preview(&path).unwrap_err().code, "locked");
    assert_eq!(core.csv_commit(&path, &[]).unwrap_err().code, "locked");
}
