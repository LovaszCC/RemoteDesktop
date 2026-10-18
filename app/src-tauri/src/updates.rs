//! In-app updates through `tauri-plugin-updater`: minisign-signed artifacts listed in a `latest.json` on
//! this repo's GitHub Releases.
//!
//! The plain parts live here (tested without Tauri); the commands are in `commands::updates`. The updater is
//! inert while `tauri.conf.json` still carries the [`PUBKEY_PLACEHOLDER`]: development builds and builds made
//! before the release key exists report "not configured" instead of failing.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

/// The start of the placeholder public key committed in `tauri.conf.json`.
pub const PUBKEY_PLACEHOLDER: &str = "REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY";

/// Whether `pubkey` looks like a real minisign public key as `tauri signer generate` prints it: base64 of a
/// text starting with `untrusted comment:`. The placeholder and empty values are not.
#[must_use]
pub fn pubkey_configured(pubkey: &str) -> bool {
    let pubkey = pubkey.trim();
    if pubkey.is_empty() || pubkey.starts_with(PUBKEY_PLACEHOLDER) {
        return false;
    }
    STANDARD
        .decode(pubkey)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .is_some_and(|text| text.starts_with("untrusted comment:"))
}

/// The `pubkey` of the `plugins.updater` section of the app config, if any.
#[must_use]
pub fn configured_pubkey(plugins: &serde_json::Value) -> Option<&str> {
    plugins.get("updater")?.get("pubkey")?.as_str()
}

/// Result of `update_check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateCheckDto {
    /// This build has no updater key (development build): nothing was checked.
    NotConfigured {
        /// The running version.
        current_version: String,
    },
    /// The automatic check is turned off or ran less than 24 hours ago: nothing was checked.
    Skipped,
    /// No newer release.
    UpToDate {
        /// The running version.
        current_version: String,
    },
    /// A newer release; `update_install` installs it.
    Available {
        /// The running version.
        current_version: String,
        /// The new version.
        version: String,
        /// Release notes (Markdown), if any.
        notes: Option<String>,
        /// Publish date (RFC 3339), if announced.
        date: Option<String>,
    },
}

/// Payload of the `update-progress` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressDto {
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// The full size, when the server announced it.
    pub total: Option<u64>,
}

/// Limits `update-progress` events to roughly one per percent (or per 256 KiB without a known size).
#[derive(Debug, Default)]
pub struct ProgressThrottle {
    downloaded: u64,
    last_emitted: Option<u64>,
}

impl ProgressThrottle {
    const UNKNOWN_STEP: u64 = 256 * 1024;

    /// Adds a chunk; returns the progress to emit, or `None` while it hasn't moved enough.
    pub fn chunk(&mut self, length: usize, total: Option<u64>) -> Option<UpdateProgressDto> {
        self.downloaded += length as u64;
        let step = total.map_or(Self::UNKNOWN_STEP, |t| (t / 100).max(1));
        let due = self
            .last_emitted
            .is_none_or(|last| self.downloaded - last >= step || total.is_some_and(|t| self.downloaded >= t));
        due.then(|| {
            self.last_emitted = Some(self.downloaded);
            UpdateProgressDto {
                downloaded: self.downloaded,
                total,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_and_garbage_keys_are_not_configured() {
        assert!(!pubkey_configured(""));
        assert!(!pubkey_configured("   "));
        assert!(!pubkey_configured(&format!(
            "{PUBKEY_PLACEHOLDER}: paste the public key here"
        )));
        assert!(!pubkey_configured("not base64 at all!"));
        assert!(!pubkey_configured(&STANDARD.encode("something else")));
    }

    #[test]
    fn a_generated_key_is_configured() {
        // The shape `tauri signer generate` prints (key bytes are made up).
        let key = STANDARD
            .encode("untrusted comment: minisign public key: 0123456789ABCDEF\nRWQBI0VniavN7wAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n");
        assert!(pubkey_configured(&key));
        assert!(pubkey_configured(&format!(" {key}\n")));
    }

    #[test]
    fn the_committed_config_carries_the_placeholder() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pubkey = configured_pubkey(&config["plugins"]).expect("plugins.updater.pubkey");
        assert!(
            pubkey.starts_with(PUBKEY_PLACEHOLDER) || pubkey_configured(pubkey),
            "the updater pubkey is neither the placeholder nor a minisign key"
        );
        let endpoints = &config["plugins"]["updater"]["endpoints"];
        assert_eq!(
            endpoints[0],
            "https://github.com/LovaszCC/RemoteDesktop/releases/latest/download/latest.json"
        );
        assert_eq!(configured_pubkey(&serde_json::json!({})), None);
    }

    #[test]
    fn check_results_serialize_for_the_ui() {
        let available = UpdateCheckDto::Available {
            current_version: "0.1.0".into(),
            version: "0.2.0".into(),
            notes: Some("Fixes".into()),
            date: None,
        };
        assert_eq!(
            serde_json::to_value(&available).unwrap(),
            serde_json::json!({"kind": "available", "currentVersion": "0.1.0", "version": "0.2.0", "notes": "Fixes", "date": null})
        );
        assert_eq!(
            serde_json::to_value(UpdateCheckDto::Skipped).unwrap(),
            serde_json::json!({"kind": "skipped"})
        );
    }

    #[test]
    fn progress_is_throttled_to_percent_steps() {
        let mut throttle = ProgressThrottle::default();
        let total = Some(10_000);
        assert_eq!(
            throttle.chunk(10, total),
            Some(UpdateProgressDto {
                downloaded: 10,
                total
            })
        );
        assert_eq!(throttle.chunk(50, total), None);
        assert_eq!(throttle.chunk(50, total).map(|p| p.downloaded), Some(110));
        assert_eq!(throttle.chunk(9_890, total).map(|p| p.downloaded), Some(10_000));
    }

    #[test]
    fn progress_without_a_size_steps_by_256_kib() {
        let mut throttle = ProgressThrottle::default();
        assert!(throttle.chunk(1, None).is_some());
        assert!(throttle.chunk(100_000, None).is_none());
        assert_eq!(throttle.chunk(200_000, None).map(|p| p.downloaded), Some(300_001));
    }
}
