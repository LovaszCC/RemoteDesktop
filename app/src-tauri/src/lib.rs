//! The Tauri shell. The logic lives in [`core::AppCore`] (plain Rust, tested without Tauri); the commands
//! are thin wrappers that call it and emit events. The dialog, opener and updater plugins are used from Rust
//! only: the webview has no plugin permissions.

mod clipboard;
mod commands;
pub mod core;
pub mod dto;
pub mod menu;
pub mod quit;
pub mod updates;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use rd_platform::APP_IDENTITY;
use rd_store::{KeyringCache, REFRESH_EVERY};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::commands::Core;
use crate::core::AppCore;

/// Where `settings.json` lives; a temp folder when the OS has no config dir.
fn settings_dir() -> PathBuf {
    rd_platform::config_dir().unwrap_or_else(|| std::env::temp_dir().join(APP_IDENTITY.identifier))
}

/// Brings the existing window forward when a second instance starts.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Entries asked for before `setup` made the core: macOS hands over the `.rdentry` that launched the app
/// (`RunEvent::Opened`) before it finishes launching. `setup` passes them on to the core.
#[derive(Default)]
struct EarlyEntryRequests(Mutex<Vec<String>>);

impl EarlyEntryRequests {
    fn push(&self, id: String) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(id);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// A system search launcher asked for an entry: remember it, tell the UI and bring the window forward.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn request_entry(app: &tauri::AppHandle, id: String) {
    if let Some(core) = app.try_state::<Core>() {
        core.request_entry(id);
        let _ = app.emit(commands::ENTRY_REQUESTED, ());
    } else if let Some(early) = app.try_state::<EarlyEntryRequests>() {
        early.push(id);
    }
    focus_main_window(app);
}

/// The entry ids in `.rdentry` documents macOS hands to the app.
#[cfg(target_os = "macos")]
fn opened_entries(urls: &[tauri::Url]) -> Vec<String> {
    urls.iter()
        .filter_map(|url| url.to_file_path().ok())
        .filter_map(|path| std::fs::read(path).ok())
        .filter_map(|bytes| rd_platform::launchers::read_rdentry(&bytes))
        .collect()
}

/// A file browser window closed: its connection ends. The main window closed: the file browsers go too.
fn on_window_destroyed(window: &tauri::Window) {
    let app = window.app_handle();
    let label = window.label();
    if core::is_files_label(label) {
        if let Some(core) = app.try_state::<Core>() {
            core.files_close(label);
        }
    } else if label == "main" {
        for (other, files) in app.webview_windows() {
            if core::is_files_label(&other) {
                let _ = files.destroy();
            }
        }
    }
}

/// Every command the webview may invoke.
#[allow(clippy::too_many_lines)] // one flat list of command paths
fn invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::ping,
        commands::session::app_status,
        commands::session::take_entry_request,
        commands::session::dismiss_handover,
        commands::session::create_vault,
        commands::session::unlock,
        commands::session::lock,
        commands::session::change_password,
        commands::session::running_count,
        commands::session::app_quit,
        commands::database::open_database,
        commands::database::create_database,
        commands::database::reopen_database,
        commands::database::resolve_db_lock,
        commands::database::reveal_database,
        commands::database::pick_database_file,
        commands::database::pick_ssh_key,
        commands::import::pick_import_file,
        commands::import::import_preview,
        commands::import::import_commit,
        commands::transfer::pick_export_path,
        commands::transfer::pick_transfer_file,
        commands::transfer::export_library,
        commands::transfer::transfer_preview,
        commands::transfer::transfer_commit,
        commands::csv::save_csv_sample,
        commands::csv::pick_csv_file,
        commands::csv::csv_preview,
        commands::csv::csv_commit,
        commands::library::library,
        commands::library::search,
        commands::library::link_candidates,
        commands::library::matching,
        commands::library::save_entry,
        commands::library::merge_entries,
        commands::library::duplicate_entry,
        commands::library::save_credential,
        commands::library::save_folder,
        commands::library::delete_items,
        commands::library::move_items,
        commands::library::set_favorite,
        commands::library::set_default_connection,
        commands::library::set_credential,
        commands::library::save_saved_split,
        commands::library::rename_saved_split,
        commands::library::delete_saved_split,
        commands::library::reorder_saved_splits,
        commands::library::copy_password,
        commands::library::copy_text,
        commands::settings::settings_get,
        commands::settings::settings_set,
        commands::settings::shortcuts_get,
        commands::settings::shortcut_set,
        commands::settings::shortcut_reset,
        commands::settings::shortcuts_reset_all,
        commands::ssh::open_ssh_session,
        commands::ssh::open_local_terminal,
        commands::ssh::ssh_input,
        commands::ssh::ssh_broadcast_input,
        commands::ssh::ssh_resize,
        commands::ssh::ssh_close,
        commands::ssh::ssh_reconnect,
        commands::ssh::ssh_replace_host_key,
        commands::ssh::ssh_record_next_output,
        commands::ssh::ssh_record_cancel,
        commands::ssh::ssh_clear_decision,
        commands::ssh::askpass_answer,
        commands::ssh::open_external_terminal,
        commands::ssh::installed_terminals,
        commands::ssh::terminal_paste_text,
        commands::ssh::terminal_open_url,
        commands::forwards::port_forward_save,
        commands::forwards::port_forward_delete,
        commands::forwards::port_forward_start,
        commands::forwards::port_forward_stop,
        commands::forwards::port_forward_statuses,
        commands::forwards::host_key_answer,
        commands::vpn::vpn_status,
        commands::vpn::vpn_profiles,
        commands::vpn::vpn_pair,
        commands::vpn::vpn_unpair,
        commands::vpn::vpn_cancel,
        commands::vpn::vpn_release_entry,
        commands::vpn::vpn_release_profile,
        commands::keys::ssh_key_generate,
        commands::keys::ssh_key_setup,
        commands::files::files_open,
        commands::files::files_info,
        commands::files::files_reconnect,
        commands::files::files_list_remote,
        commands::files::files_list_local,
        commands::files::files_transfer,
        commands::files::files_cancel,
        commands::files::files_conflict_answer,
        commands::files::menu_files_target,
        commands::launch::connect_web,
        commands::launch::installed_browsers,
        commands::launch::connect_winbox,
        commands::launch::winbox_status,
        commands::launch::pick_winbox,
        commands::launch::connect_rdp_external,
        commands::launch::accessibility_status,
        commands::launch::open_accessibility_settings,
        commands::rdp::open_rdp_session,
        commands::rdp::rdp_ack_frame,
        commands::rdp::rdp_input,
        commands::rdp::rdp_release_all_keys,
        commands::rdp::rdp_resize,
        commands::rdp::rdp_close,
        commands::rdp::rdp_reconnect,
        commands::rdp::rdp_type_clipboard,
        commands::rdp::rdp_cancel_file_transfer,
        commands::rdp::rdp_cert_answer,
        commands::rdp::rdp_credentials_answer,
        commands::rdp::rdp_engine_info,
        commands::rdp::rdp_capture_keyboard,
        commands::updates::update_check,
        commands::updates::update_install,
        commands::updates::update_relaunch,
    ]
}

/// Builds and runs the app until the last window closes.
///
/// # Panics
/// If the Tauri runtime fails to start, which leaves nothing to recover.
pub fn run() {
    let builder = tauri::Builder::default().manage(EarlyEntryRequests::default());
    // Single instance must be the first plugin.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        match rd_platform::launchers::entry_request(&args) {
            Some(id) => request_entry(app, id),
            None => focus_main_window(app),
        }
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        // Used from Rust only (commands::updates); inert while the config has the placeholder key.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let settings_dir = settings_dir();
            // First launch after the Swift app: carry its settings over (database path first) before the
            // core reads settings.json.
            let handover = rd_platform::swift_defaults::swift_defaults_path()
                .and_then(|plist| rd_platform::swift_defaults::import_swift_defaults(&settings_dir, &plist))
                .is_some();
            // The build's version, not the crate's: a beta build is `X.Y.Z-beta.N` (tauri --config override),
            // and the database version guard must stamp that.
            let core: Core = Arc::new(
                AppCore::new(settings_dir, Box::new(KeyringCache::new()))
                    .with_app_version(&app.package_info().version.to_string()),
            );
            if handover {
                core.mark_handover();
            }
            core.open_current();
            let args: Vec<String> = std::env::args().collect();
            if let Some(id) = rd_platform::launchers::entry_request(&args) {
                core.request_entry(id);
            }
            for id in app.state::<EarlyEntryRequests>().take() {
                core.request_entry(id);
            }
            let refresher = Arc::downgrade(&core);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(REFRESH_EVERY);
                    let Some(core) = refresher.upgrade() else { break };
                    core.refresh_lock();
                }
            });
            commands::ssh::install_hooks(app.handle(), &core);
            commands::forwards::install_hooks(app.handle(), &core);
            commands::vpn::install_hooks(app.handle(), &core);
            commands::keys::install_hooks(app.handle(), &core);
            commands::files::install_hooks(app.handle(), &core);
            commands::launch::install_hooks(app.handle(), &core);
            commands::rdp::install_hooks(app.handle(), &core);
            app.manage(core);
            commands::sync_launchers(app.handle());
            commands::updates::install_state(app.handle());
            menu::refresh(app.handle());
            Ok(())
        })
        .on_menu_event(|app, event| menu::on_menu_event(app, &event))
        .on_window_event(|window, event| match event {
            // Closing a file browser window only ends its connection; the main window asks first.
            WindowEvent::CloseRequested { api, .. }
                if window.label() == "main" && quit::intercept(window.app_handle()) =>
            {
                api.prevent_close();
            }
            WindowEvent::Destroyed => on_window_destroyed(window),
            // Local clipboard text goes to the RDP sessions when the app comes to the front.
            WindowEvent::Focused(true) if window.label() == "main" => {
                if let Some(core) = window.app_handle().try_state::<Core>() {
                    commands::rdp::sync_clipboard(&core);
                }
            }
            _ => {}
        })
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri app")
        .run(|app, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                if quit::intercept(app) {
                    api.prevent_exit();
                }
            }
            // Spotlight / Finder opened a `.rdentry` document (also the one that launched the app).
            #[cfg(target_os = "macos")]
            RunEvent::Opened { urls } => {
                for id in opened_entries(&urls) {
                    request_entry(app, id);
                }
            }
            RunEvent::Exit => {
                if let Some(core) = app.try_state::<Core>() {
                    core.close();
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::EarlyEntryRequests;

    #[test]
    fn early_entry_requests_are_taken_once_in_order() {
        let early = EarlyEntryRequests::default();
        early.push("A".into());
        early.push("B".into());
        assert_eq!(early.take(), ["A", "B"]);
        assert!(early.take().is_empty());
    }
}
