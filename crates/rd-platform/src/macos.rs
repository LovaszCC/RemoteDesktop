//! macOS app lookups through Launch Services (`NSWorkspace`).

use objc2::rc::Retained;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSFileManager, NSString, NSURL};

/// The app with `bundle_id`, if Launch Services knows it.
#[must_use]
pub fn app_url(bundle_id: &str) -> Option<Retained<NSURL>> {
    NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
}

/// Whether an app with `bundle_id` is installed.
#[must_use]
pub fn is_installed(bundle_id: &str) -> bool {
    app_url(bundle_id).is_some()
}

/// The path of the app with `bundle_id`.
#[must_use]
pub fn app_path(bundle_id: &str) -> Option<String> {
    app_url(bundle_id)?.path().map(|p| p.to_string())
}

/// Finder's name of an app bundle, without `.app`.
#[must_use]
pub fn display_name(app: &NSURL) -> Option<String> {
    let path = app.path()?;
    let name = NSFileManager::defaultManager()
        .displayNameAtPath(&path)
        .to_string();
    Some(name.strip_suffix(".app").unwrap_or(&name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_is_always_installed() {
        assert!(is_installed("com.apple.finder"));
        assert!(app_path("com.apple.finder").is_some_and(|p| p.ends_with("Finder.app")));
        assert_eq!(
            display_name(&app_url("com.apple.finder").unwrap()).as_deref(),
            Some("Finder")
        );
        assert!(!is_installed("hu.example.no-such-app"));
    }
}
