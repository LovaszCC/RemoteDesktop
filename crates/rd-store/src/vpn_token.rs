//! The MacVPN control API token, kept in the OS keyring.
//!
//! MacVPN pins a pairing to the paired program's executable path, so the account is that path: a
//! development build and the installed app each keep their own token.

use std::sync::Mutex;

use rd_platform::APP_IDENTITY;

use crate::KeyCacheError;

/// Where the MacVPN token lives.
pub trait VpnTokens: Send + Sync {
    /// The stored token; `None` when there is none or the keyring can't be read (not paired then).
    fn load(&self) -> Option<String>;

    /// Stores `token`, replacing an older one.
    ///
    /// # Errors
    /// If the keyring is unavailable.
    fn save(&self, token: &str) -> Result<(), KeyCacheError>;

    /// Forgets the token; forgetting a missing one is fine, and a keyring failure is ignored.
    fn delete(&self);
}

/// The OS keyring (the Keychain on macOS).
#[derive(Debug, Clone)]
pub struct KeyringVpnTokens {
    service: String,
    account: String,
}

impl KeyringVpnTokens {
    /// The token of this executable.
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: APP_IDENTITY.macvpn_token_service.to_owned(),
            account: std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, KeyCacheError> {
        keyring::Entry::new(&self.service, &self.account).map_err(|e| KeyCacheError(e.to_string()))
    }
}

impl Default for KeyringVpnTokens {
    fn default() -> Self {
        Self::new()
    }
}

impl VpnTokens for KeyringVpnTokens {
    fn load(&self) -> Option<String> {
        self.entry().ok()?.get_password().ok()
    }

    fn save(&self, token: &str) -> Result<(), KeyCacheError> {
        self.entry()?
            .set_password(token)
            .map_err(|e| KeyCacheError(e.to_string()))
    }

    fn delete(&self) {
        if let Ok(entry) = self.entry() {
            let _ = entry.delete_credential();
        }
    }
}

/// An in-process token for tests.
#[derive(Default)]
pub struct MemoryVpnTokens {
    token: Mutex<Option<String>>,
}

impl std::fmt::Debug for MemoryVpnTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryVpnTokens(..)")
    }
}

impl VpnTokens for MemoryVpnTokens {
    fn load(&self) -> Option<String> {
        self.token.lock().ok()?.clone()
    }

    fn save(&self, token: &str) -> Result<(), KeyCacheError> {
        *self.token.lock().map_err(|e| KeyCacheError(e.to_string()))? = Some(token.to_owned());
        Ok(())
    }

    fn delete(&self) {
        if let Ok(mut token) = self.token.lock() {
            *token = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_tokens_round_trip() {
        let tokens = MemoryVpnTokens::default();
        assert_eq!(tokens.load(), None);
        tokens.save("token-1").unwrap();
        assert_eq!(tokens.load().as_deref(), Some("token-1"));
        tokens.save("token-2").unwrap();
        assert_eq!(tokens.load().as_deref(), Some("token-2"));
        tokens.delete();
        tokens.delete();
        assert_eq!(tokens.load(), None);
    }

    #[test]
    fn keyring_tokens_use_the_macvpn_service_and_the_executable_path() {
        let tokens = KeyringVpnTokens::new();
        assert_eq!(tokens.service, APP_IDENTITY.macvpn_token_service);
        assert_eq!(tokens.account, std::env::current_exe().unwrap().to_string_lossy());
    }
}
