//! Per-OS integration: the app identity, paths, external apps and clipboard shims.

pub mod clipboard;
mod identity;
pub mod keyboard_capture;
pub mod launch;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod rdp;
pub mod settings;
pub mod swift_defaults;
pub mod web;
pub mod winbox;

pub use identity::{APP_IDENTITY, AppIdentity};
pub use settings::{Settings, config_dir, default_database_path};
