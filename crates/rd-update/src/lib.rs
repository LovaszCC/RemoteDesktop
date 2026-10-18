//! Release checks.
//!
//! - [`UpdateChecker`] / [`UpdateDownloader`] / [`AppVersion`]: the Swift app's GitHub release check
//!   (`releases/latest`, `.dmg` + `.dmg.sha256`), ported with its tests. It is the compatibility contract
//!   for the Swift→Rust handover: the Rust app's macOS releases keep publishing assets it understands.
//!   The transport is injected, so this crate has no HTTP client.
//! - [`schedule`]: when the silent launch check runs (enabled, at most once per 24 h). The app uses it
//!   for its own updates, which go through `tauri-plugin-updater` (minisign-signed, `latest.json`).

mod checker;
mod downloader;
pub mod schedule;
mod version;

pub use checker::{
    Fetch, HttpRequest, HttpResponse, LATEST_RELEASE_URL, TransportError, UpdateCheckResult, UpdateChecker,
    UpdateError, UpdateRelease, check_status, fetch_data,
};
pub use downloader::{Download, UpdateDownloader, checksum_from_file, sha256_of};
pub use version::AppVersion;
