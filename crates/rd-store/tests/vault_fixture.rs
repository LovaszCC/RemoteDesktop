//! The vault in a Swift-made database opens with its password and with its raw key.

use std::path::PathBuf;

use rd_store::{Database, Vault, VaultError};

fn swift_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.sqlite");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/swift-v13.sqlite"),
        &path,
    )
    .unwrap();
    let db = Database::open(&path).unwrap();
    (dir, db)
}

#[test]
fn swift_vault_unlocks_with_its_password() {
    let (_dir, db) = swift_db();
    assert!(Vault::is_initialized(&db).unwrap());
    let vault = Vault::unlock(&db, "fixture-password").unwrap();
    assert!(Vault::unlock_with_key(&db, *vault.export_key()).is_ok());
    assert!(matches!(
        Vault::unlock(&db, "Fixture-password"),
        Err(VaultError::WrongPassword)
    ));
}
