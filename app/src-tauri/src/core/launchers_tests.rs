//! System search launchers: the folder follows the setting and the open library; entry requests wait
//! for the unlock.

use std::fs;
use std::path::PathBuf;

use rd_platform::launchers::LauncherFormat;
use tempfile::TempDir;

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;

use super::tests::{Env, HOST, entry_draft, save_single};
use super::*;

/// A launcher folder inside its own temp dir (it doesn't exist until the first reconcile).
struct Folder {
    _temp: TempDir,
    dir: PathBuf,
}

impl Folder {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("launchers");
        Self { _temp: temp, dir }
    }

    fn names(&self) -> Vec<String> {
        let Ok(items) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = items
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn file(stem: &str) -> String {
    format!("{stem}.{}", LauncherFormat::current().extension())
}

fn with_folder(core: AppCore, folder: &Folder) -> AppCore {
    core.with_launcher_dir(Some(folder.dir.clone()))
}

fn show(core: &AppCore, on: bool) {
    core.settings_set(&SettingsPatch {
        show_in_system_search: Some(on),
        ..SettingsPatch::default()
    })
    .unwrap();
    core.sync_launchers();
}

/// A one-SSH-connection entry; returns its connection id (renames go through it).
fn add_entry(core: &AppCore, name: &str) -> String {
    save_single(
        core,
        None,
        &entry_draft(name, HOST),
        ConnectionDraft::new(ConnectionType::Ssh),
        None,
        None,
    )
    .unwrap()
}

#[test]
fn sync_on_writes_one_file_per_entry() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    add_entry(&core, "Home Server");
    add_entry(&core, "Router");
    core.sync_launchers();
    assert!(folder.names().is_empty(), "off by default");

    show(&core, true);
    assert_eq!(folder.names(), [file("Home Server"), file("Router")]);
}

#[test]
fn sync_follows_rename_and_delete() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    show(&core, true);
    let home = add_entry(&core, "Home Server");
    let router = add_entry(&core, "Router");
    core.sync_launchers();

    save_single(
        &core,
        Some(&home),
        &entry_draft("NAS", HOST),
        ConnectionDraft::new(ConnectionType::Ssh),
        None,
        None,
    )
    .unwrap();
    core.delete_items(&[format!("c:{router}")], false).unwrap();
    core.sync_launchers();
    assert_eq!(folder.names(), [file("NAS")]);
}

#[test]
fn sync_regenerates_a_deleted_launcher() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    add_entry(&core, "Home Server");
    show(&core, true);
    fs::remove_file(folder.dir.join(file("Home Server"))).unwrap();
    core.sync_launchers();
    assert_eq!(folder.names(), [file("Home Server")]);
}

#[test]
fn sync_off_removes_folder() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    add_entry(&core, "Home Server");
    show(&core, true);
    show(&core, false);
    assert!(!folder.dir.exists());
}

#[test]
fn sync_while_locked_leaves_files_and_off_still_removes() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    add_entry(&core, "Home Server");
    show(&core, true);

    core.lock();
    core.sync_launchers();
    assert_eq!(folder.names(), [file("Home Server")]);

    show(&core, false);
    assert!(!folder.dir.exists());
}

#[test]
fn switching_database_rebuilds_for_its_entries() {
    let env = Env::new();
    let folder = Folder::new();
    let core = with_folder(env.unlocked(), &folder);
    add_entry(&core, "Home Server");
    show(&core, true);

    core.create_database(&env.db("other.sqlite"));
    core.create_vault("master", true).unwrap();
    add_entry(&core, "Office");
    core.sync_launchers();
    assert_eq!(folder.names(), [file("Office")]);
}

#[test]
fn no_launcher_dir_does_nothing() {
    let env = Env::new();
    let core = env.unlocked().with_launcher_dir(None);
    show(&core, true);
    add_entry(&core, "Home Server");
    core.sync_launchers();
}

#[test]
fn entry_request_waits_for_unlock() {
    let env = Env::new();
    let core = env.unlocked();
    core.lock();
    core.request_entry("OLD".into());
    core.request_entry("ABC".into());
    assert_eq!(core.take_entry_request(), None);

    core.unlock("master", false).unwrap();
    assert_eq!(core.take_entry_request().as_deref(), Some("ABC"));
    assert_eq!(core.take_entry_request(), None);
}

#[test]
fn settings_patch_toggles_show_in_system_search() {
    let env = Env::new();
    let core = env.unlocked();
    assert!(!core.settings_get().show_in_system_search);
    show(&core, true);
    assert!(core.settings_get().show_in_system_search);
    assert!(rd_platform::Settings::load(&env.settings_dir()).show_in_system_search);
}
