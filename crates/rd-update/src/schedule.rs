//! When the silent launch check runs (Swift `UpdateSchedule`): enabled in Settings (default on) and at most
//! once per 24 hours. Used by the app for the Tauri updater's launch check.

use rd_core::Timestamp;
use rd_platform::Settings;

/// The minimum time between two automatic checks, in seconds.
pub const INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Whether automatic checks are on (`checkForUpdatesAutomatically`).
#[must_use]
pub fn automatic_checks_enabled(settings: &Settings) -> bool {
    settings.check_for_updates_automatically
}

/// When updates were last checked (`lastUpdateCheck`).
#[must_use]
pub fn last_check(settings: &Settings) -> Option<Timestamp> {
    settings.last_update_check
}

/// Whether the launch check should run at `now`.
#[must_use]
pub fn is_due(settings: &Settings, now: Timestamp) -> bool {
    if !automatic_checks_enabled(settings) {
        return false;
    }
    last_check(settings)
        .is_none_or(|last| (now.datetime() - last.datetime()).whole_seconds() >= INTERVAL_SECS)
}

/// Records a completed check at `now`.
pub fn mark_checked(settings: &mut Settings, now: Timestamp) {
    settings.last_update_check = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_when_never_checked() {
        let settings = Settings::default();
        assert!(automatic_checks_enabled(&settings));
        assert_eq!(last_check(&settings), None);
        assert!(is_due(&settings, Timestamp::now()));
    }

    #[test]
    fn due_once_per_day() {
        let mut settings = Settings::default();
        let checked = Timestamp::from_unix(1_000_000);
        mark_checked(&mut settings, checked);
        assert_eq!(last_check(&settings), Some(checked));
        assert!(!is_due(&settings, Timestamp::from_unix(1_000_000 + 23 * 3600)));
        assert!(is_due(&settings, Timestamp::from_unix(1_000_000 + 24 * 3600)));
    }

    #[test]
    fn never_due_when_turned_off() {
        let settings = Settings {
            check_for_updates_automatically: false,
            ..Settings::default()
        };
        assert!(!automatic_checks_enabled(&settings));
        assert!(!is_due(&settings, Timestamp::now()));
    }
}
