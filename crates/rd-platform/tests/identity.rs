//! The app identity is defined once: `rd-platform`'s identity module plus `tauri.conf.json`.

use std::fs;
use std::path::{Path, PathBuf};

use rd_platform::APP_IDENTITY;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn tauri_config_matches_identity() {
    let path = workspace_root().join("app/src-tauri/tauri.conf.json");
    let config: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(config["productName"], APP_IDENTITY.product_name);
    assert_eq!(config["identifier"], APP_IDENTITY.identifier);
}

#[test]
fn identifier_is_not_hard_coded_elsewhere() {
    let root = workspace_root();
    let allowed = [
        root.join("crates/rd-platform/src/identity.rs"),
        root.join("app/src-tauri/tauri.conf.json"),
    ];
    let mut offenders = Vec::new();
    for dir in ["crates", "app/src-tauri/src", "app/ui/src"] {
        visit(&root.join(dir), &mut |file| {
            if allowed.contains(&file.to_path_buf()) || file.ends_with("tests/identity.rs") {
                return;
            }
            if fs::read_to_string(file).is_ok_and(|text| text.contains("hu.softworks.RemoteDesktop")) {
                offenders.push(file.display().to_string());
            }
        });
    }
    assert!(offenders.is_empty(), "identifier hard-coded in: {offenders:?}");
}

fn visit(dir: &Path, on_file: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "node_modules" || name == "gen" {
            continue;
        }
        if path.is_dir() {
            visit(&path, on_file);
        } else {
            on_file(&path);
        }
    }
}
