//! The one place that names the app.
//!
//! The app uses the Swift app's identity: the same bundle id, data
//! folder and keyring service, so it picks up the Swift app's database and remembered key. Defined
//! here plus `app/src-tauri/tauri.conf.json`.

/// Names and ids that keep this build's data apart from any other build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppIdentity {
    /// Shown in window titles, menus and the bundle name.
    pub product_name: &'static str,
    /// Reverse-DNS id: the macOS bundle id and the Tauri `identifier` on every OS.
    pub identifier: &'static str,
    /// Folder name under the OS data directory that holds `connections.sqlite`.
    pub data_dir_name: &'static str,
    /// OS keyring service for the remembered vault key.
    pub keyring_service: &'static str,
}

/// The identity of this build.
pub const APP_IDENTITY: AppIdentity = AppIdentity {
    product_name: "RemoteDesktop",
    identifier: "hu.softworks.RemoteDesktop",
    data_dir_name: "RemoteDesktop",
    keyring_service: "hu.softworks.RemoteDesktop.vault",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_service_derives_from_identifier() {
        assert_eq!(
            APP_IDENTITY.keyring_service,
            format!("{}.vault", APP_IDENTITY.identifier)
        );
    }

    #[test]
    fn identity_matches_the_swift_app() {
        // Swift: Resources/Info.plist (CFBundleIdentifier, CFBundleName), KeychainStore's service and
        // ~/Library/Application Support/RemoteDesktop.
        assert_eq!(APP_IDENTITY.product_name, "RemoteDesktop");
        assert_eq!(APP_IDENTITY.identifier, "hu.softworks.RemoteDesktop");
        assert_eq!(APP_IDENTITY.data_dir_name, "RemoteDesktop");
        assert_eq!(APP_IDENTITY.keyring_service, "hu.softworks.RemoteDesktop.vault");
    }
}
