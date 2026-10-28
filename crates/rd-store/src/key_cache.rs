//! Remembering the vault key in the OS keyring, so the app can unlock without the master password.
//!
//! One entry per database: service = the app identity's keyring service, account = the database's
//! canonical absolute path, value = the raw 32-byte key (the Swift app's Keychain layout).

use std::collections::HashMap;
use std::sync::Mutex;

use rd_platform::APP_IDENTITY;
use zeroize::Zeroizing;

/// The OS keyring failed.
#[derive(Debug, thiserror::Error)]
#[error("keyring: {0}")]
pub struct KeyCacheError(pub(crate) String);

/// Where remembered vault keys live.
pub trait KeyCache: Send + Sync {
    /// The key remembered for `account`, if any. A stored value of the wrong length counts as none.
    ///
    /// # Errors
    /// If the keyring is unavailable or refuses access (the caller falls back to the password).
    fn load(&self, account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, KeyCacheError>;

    /// Remembers `key` for `account`, replacing an older one.
    ///
    /// # Errors
    /// If the keyring is unavailable.
    fn save(&self, account: &str, key: &[u8; 32]) -> Result<(), KeyCacheError>;

    /// Forgets the key for `account`; forgetting a missing key is fine.
    ///
    /// # Errors
    /// If the keyring is unavailable.
    fn delete(&self, account: &str) -> Result<(), KeyCacheError>;
}

/// The OS keyring: Keychain on macOS, Credential Manager on Windows, Secret Service on Linux.
#[derive(Debug, Clone)]
pub struct KeyringCache {
    service: String,
}

impl KeyringCache {
    /// A cache under this build's keyring service.
    #[must_use]
    pub fn new() -> Self {
        Self::with_service(APP_IDENTITY.keyring_service)
    }

    /// A cache under another service name (tests use a throwaway one).
    #[must_use]
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, KeyCacheError> {
        keyring::Entry::new(&self.service, account).map_err(|e| KeyCacheError(e.to_string()))
    }
}

impl Default for KeyringCache {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyCache for KeyringCache {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, KeyCacheError> {
        match self.entry(account)?.get_secret() {
            Ok(secret) => {
                let secret = Zeroizing::new(secret);
                Ok(<[u8; 32]>::try_from(secret.as_slice()).ok().map(Zeroizing::new))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyCacheError(e.to_string())),
        }
    }

    fn save(&self, account: &str, key: &[u8; 32]) -> Result<(), KeyCacheError> {
        self.entry(account)?
            .set_secret(key)
            .map_err(|e| KeyCacheError(e.to_string()))
    }

    fn delete(&self, account: &str) -> Result<(), KeyCacheError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyCacheError(e.to_string())),
        }
    }
}

/// An in-process cache for tests and for platforms without a keyring.
#[derive(Default)]
pub struct MemoryKeyCache {
    keys: Mutex<HashMap<String, Zeroizing<[u8; 32]>>>,
}

impl std::fmt::Debug for MemoryKeyCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryKeyCache(..)")
    }
}

impl KeyCache for MemoryKeyCache {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, KeyCacheError> {
        Ok(self
            .keys
            .lock()
            .map_err(|e| KeyCacheError(e.to_string()))?
            .get(account)
            .cloned())
    }

    fn save(&self, account: &str, key: &[u8; 32]) -> Result<(), KeyCacheError> {
        self.keys
            .lock()
            .map_err(|e| KeyCacheError(e.to_string()))?
            .insert(account.to_owned(), Zeroizing::new(*key));
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), KeyCacheError> {
        self.keys
            .lock()
            .map_err(|e| KeyCacheError(e.to_string()))?
            .remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(cache: &dyn KeyCache, account: &str) {
        cache.delete(account).unwrap();
        assert!(cache.load(account).unwrap().is_none());
        cache.save(account, &[1; 32]).unwrap();
        cache.save(account, &[2; 32]).unwrap();
        assert_eq!(*cache.load(account).unwrap().unwrap(), [2; 32]);
        cache.delete(account).unwrap();
        cache.delete(account).unwrap();
        assert!(cache.load(account).unwrap().is_none());
    }

    #[test]
    fn memory_cache_save_load_delete() {
        exercise(&MemoryKeyCache::default(), "/tmp/a.sqlite");
    }

    #[test]
    fn default_service_comes_from_identity() {
        assert_eq!(KeyringCache::default().service, APP_IDENTITY.keyring_service);
    }

    /// Touches the real OS keyring; run by hand with `cargo test -p rd-store -- --ignored`.
    #[test]
    #[ignore = "needs an unlocked OS keyring"]
    fn os_keyring_save_load_delete() {
        exercise(
            &KeyringCache::with_service(&format!("{}.test", APP_IDENTITY.keyring_service)),
            "/tmp/rd-keyring-test.sqlite",
        );
    }
}
