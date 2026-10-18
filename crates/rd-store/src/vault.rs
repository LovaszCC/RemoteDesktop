//! The master-password vault, bit-compatible with the Swift app.
//!
//! - Key: PBKDF2-HMAC-SHA256 over the password's UTF-8 bytes, 32 bytes; salt and iteration count live in
//!   `meta` (`kdf_salt`, 16 bytes; `kdf_iterations`, u32 big-endian).
//! - Seal: AES-256-GCM, `nonce(12) ‖ ciphertext ‖ tag(16)`, no associated data.
//! - `verifier` in `meta` is the seal of `remotedesktop-vault-v1` and proves the password is right.

use aes_gcm::aead::{Aead, Generate, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rd_core::Sealed;
use rusqlite::params;
use secrecy::{ExposeSecret, SecretBox, SecretString};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{Database, StoreError};

const SALT_KEY: &str = "kdf_salt";
const ITERATIONS_KEY: &str = "kdf_iterations";
const VERIFIER_KEY: &str = "verifier";
const VERIFIER_PLAINTEXT: &[u8] = b"remotedesktop-vault-v1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Why the vault refused.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The database has no vault yet.
    #[error("the vault is not set up")]
    NotInitialized,
    /// The database already has a vault.
    #[error("the vault is already set up")]
    AlreadyInitialized,
    /// The password is empty.
    #[error("the password is empty")]
    EmptyPassword,
    /// The password or cached key does not open the vault.
    #[error("wrong password")]
    WrongPassword,
    /// A sealed value is damaged or not valid UTF-8.
    #[error("a stored secret is damaged")]
    CorruptData,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for VaultError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// An unlocked vault. The key is zeroized when the vault is dropped (that is, on lock).
pub struct Vault {
    key: SecretBox<[u8; 32]>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Vault(..)")
    }
}

impl Vault {
    /// PBKDF2 rounds for a new vault, as in the Swift app.
    pub const DEFAULT_ITERATIONS: u32 = 600_000;

    /// Whether the database has a vault.
    ///
    /// # Errors
    /// If the database fails.
    pub fn is_initialized(db: &Database) -> Result<bool, VaultError> {
        Ok(db.meta(VERIFIER_KEY)?.is_some())
    }

    /// Sets up a vault in a database that has none.
    ///
    /// # Errors
    /// [`VaultError::EmptyPassword`], [`VaultError::AlreadyInitialized`], or a database error.
    pub fn create(db: &Database, password: &str, iterations: u32) -> Result<Self, VaultError> {
        if password.is_empty() {
            return Err(VaultError::EmptyPassword);
        }
        if Self::is_initialized(db)? {
            return Err(VaultError::AlreadyInitialized);
        }
        let salt = random_salt();
        let vault = Self::with_key(&derive_key(password, &salt, iterations));
        db.set_meta(SALT_KEY, &salt)?;
        db.set_meta(ITERATIONS_KEY, &iterations.to_be_bytes())?;
        db.set_meta(VERIFIER_KEY, vault.seal_bytes(VERIFIER_PLAINTEXT).as_bytes())?;
        Ok(vault)
    }

    /// Opens the vault with the master password.
    ///
    /// # Errors
    /// [`VaultError::NotInitialized`], [`VaultError::WrongPassword`], or a database error.
    pub fn unlock(db: &Database, password: &str) -> Result<Self, VaultError> {
        let salt = db.meta(SALT_KEY)?.ok_or(VaultError::NotInitialized)?;
        let iterations = db
            .meta(ITERATIONS_KEY)?
            .and_then(|bytes| <[u8; 4]>::try_from(bytes.as_slice()).ok())
            .map(u32::from_be_bytes)
            .ok_or(VaultError::NotInitialized)?;
        let vault = Self::with_key(&derive_key(password, &salt, iterations));
        vault.verify(db)?;
        Ok(vault)
    }

    /// Opens the vault with a key remembered in the OS keyring.
    ///
    /// # Errors
    /// [`VaultError::WrongPassword`] if the key doesn't match, [`VaultError::NotInitialized`], or a
    /// database error.
    pub fn unlock_with_key(db: &Database, key: [u8; 32]) -> Result<Self, VaultError> {
        let vault = Self::with_key(&Zeroizing::new(key));
        vault.verify(db)?;
        Ok(vault)
    }

    /// The raw key, for remembering it in the OS keyring.
    #[must_use]
    pub fn export_key(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(*self.key.expose_secret())
    }

    /// Re-seals every secret and the verifier under a new password and salt, in one transaction.
    /// `updatedAt` is left alone, as in the Swift app.
    ///
    /// # Errors
    /// [`VaultError::EmptyPassword`], [`VaultError::CorruptData`] if a stored secret doesn't open, or a
    /// database error. On error nothing changes.
    pub fn change_password(&mut self, db: &Database, new_password: &str) -> Result<(), VaultError> {
        if new_password.is_empty() {
            return Err(VaultError::EmptyPassword);
        }
        let iterations = db
            .meta(ITERATIONS_KEY)?
            .and_then(|bytes| <[u8; 4]>::try_from(bytes.as_slice()).ok())
            .map_or(Self::DEFAULT_ITERATIONS, u32::from_be_bytes);
        let salt = random_salt();
        let next = Self::with_key(&derive_key(new_password, &salt, iterations));

        let mut conn = db.conn();
        let tx = conn.transaction()?;
        for (table, columns) in [
            ("connection", &["passwordEnc", "sshKeyPassphraseEnc"][..]),
            (
                "credential",
                &["passwordEnc", "sshKeyPassphraseEnc", "sshKeyUnlockedEnc"][..],
            ),
        ] {
            for column in columns {
                let rows: Vec<(String, Vec<u8>)> = tx
                    .prepare(&format!(
                        "SELECT id, \"{column}\" FROM \"{table}\" WHERE \"{column}\" IS NOT NULL"
                    ))?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                for (id, sealed) in rows {
                    let plain = self.open_bytes(&sealed)?;
                    let resealed = next.seal_bytes(&plain);
                    tx.execute(
                        &format!("UPDATE \"{table}\" SET \"{column}\" = ?1 WHERE id = ?2"),
                        params![resealed.as_bytes(), id],
                    )?;
                }
            }
        }
        let upsert = "INSERT INTO meta(key, value) VALUES (?1, ?2) \
                      ON CONFLICT(key) DO UPDATE SET value = excluded.value";
        tx.execute(upsert, params![SALT_KEY, salt.as_slice()])?;
        tx.execute(
            upsert,
            params![ITERATIONS_KEY, iterations.to_be_bytes().as_slice()],
        )?;
        tx.execute(
            upsert,
            params![VERIFIER_KEY, next.seal_bytes(VERIFIER_PLAINTEXT).as_bytes()],
        )?;
        tx.commit()?;
        drop(conn);

        *self = next;
        Ok(())
    }

    /// Seals a secret with a fresh random nonce.
    #[must_use]
    pub fn seal(&self, secret: &str) -> Sealed {
        self.seal_bytes(secret.as_bytes())
    }

    /// Seals a secret; `None` and `""` are stored as NULL.
    #[must_use]
    pub fn seal_optional(&self, secret: Option<&str>) -> Option<Sealed> {
        secret.filter(|s| !s.is_empty()).map(|s| self.seal(s))
    }

    /// Opens a sealed secret.
    ///
    /// # Errors
    /// [`VaultError::CorruptData`] if the data is damaged, sealed with another key, or not UTF-8.
    pub fn open(&self, sealed: &Sealed) -> Result<SecretString, VaultError> {
        let bytes = Zeroizing::new(self.open_bytes(sealed.as_bytes())?);
        let text = std::str::from_utf8(&bytes).map_err(|_| VaultError::CorruptData)?;
        Ok(SecretString::from(text))
    }

    /// Opens a sealed secret that may be absent.
    ///
    /// # Errors
    /// As [`Vault::open`].
    pub fn open_optional(&self, sealed: Option<&Sealed>) -> Result<Option<SecretString>, VaultError> {
        sealed.map(|s| self.open(s)).transpose()
    }

    fn with_key(key: &[u8; 32]) -> Self {
        Self {
            key: SecretBox::new(Box::new(*key)),
        }
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(self.key.expose_secret().into())
    }

    fn verify(&self, db: &Database) -> Result<(), VaultError> {
        let verifier = db.meta(VERIFIER_KEY)?.ok_or(VaultError::NotInitialized)?;
        match self.open_bytes(&verifier) {
            Ok(plain) if plain == VERIFIER_PLAINTEXT => Ok(()),
            _ => Err(VaultError::WrongPassword),
        }
    }

    fn seal_bytes(&self, plain: &[u8]) -> Sealed {
        let nonce = Nonce::generate();
        let ciphertext = self
            .cipher()
            .encrypt(&nonce, plain)
            .expect("AES-GCM sealing cannot fail");
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Sealed::from_bytes(out)
    }

    fn open_bytes(&self, sealed: &[u8]) -> Result<Vec<u8>, VaultError> {
        if sealed.len() < NONCE_LEN + TAG_LEN {
            return Err(VaultError::CorruptData);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).map_err(|_| VaultError::CorruptData)?;
        self.cipher()
            .decrypt(&nonce, ciphertext)
            .map_err(|_| VaultError::CorruptData)
    }
}

/// PBKDF2-HMAC-SHA256 → 32-byte key.
fn derive_key(password: &str, salt: &[u8], iterations: u32) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, key.as_mut());
    key
}

fn random_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("the OS random source is available");
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITERATIONS: u32 = 1_000;

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn create_then_unlock_with_same_password() {
        let db = db();
        assert!(!Vault::is_initialized(&db).unwrap());
        let created = Vault::create(&db, "s3cret", ITERATIONS).unwrap();
        assert!(Vault::is_initialized(&db).unwrap());
        let unlocked = Vault::unlock(&db, "s3cret").unwrap();
        assert_eq!(*created.export_key(), *unlocked.export_key());
    }

    #[test]
    fn wrong_password_is_rejected() {
        let db = db();
        Vault::create(&db, "right", ITERATIONS).unwrap();
        assert!(matches!(
            Vault::unlock(&db, "wrong"),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn unlock_before_create_fails() {
        assert!(matches!(
            Vault::unlock(&db(), "x"),
            Err(VaultError::NotInitialized)
        ));
        assert!(matches!(
            Vault::unlock_with_key(&db(), [0; 32]),
            Err(VaultError::NotInitialized)
        ));
    }

    #[test]
    fn create_twice_and_empty_password_fail() {
        let db = db();
        assert!(matches!(
            Vault::create(&db, "", ITERATIONS),
            Err(VaultError::EmptyPassword)
        ));
        Vault::create(&db, "pw", ITERATIONS).unwrap();
        assert!(matches!(
            Vault::create(&db, "pw", ITERATIONS),
            Err(VaultError::AlreadyInitialized)
        ));
    }

    #[test]
    fn unlock_with_cached_key() {
        let db = db();
        let key = Vault::create(&db, "pw", ITERATIONS).unwrap().export_key();
        assert!(Vault::unlock_with_key(&db, *key).is_ok());
        assert!(matches!(
            Vault::unlock_with_key(&db, [0; 32]),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn seal_open_roundtrip_uses_fresh_nonces() {
        let vault = Vault::create(&db(), "pw", ITERATIONS).unwrap();
        let a = vault.seal("hunter2");
        let b = vault.seal("hunter2");
        assert_ne!(a, b);
        assert_eq!(a.as_bytes().len(), NONCE_LEN + 7 + TAG_LEN);
        assert_eq!(vault.open(&a).unwrap().expose_secret(), "hunter2");
        assert_eq!(vault.open(&b).unwrap().expose_secret(), "hunter2");
    }

    #[test]
    fn optional_helpers_treat_empty_as_none() {
        let vault = Vault::create(&db(), "pw", ITERATIONS).unwrap();
        assert_eq!(vault.seal_optional(None), None);
        assert_eq!(vault.seal_optional(Some("")), None);
        let sealed = vault.seal_optional(Some("x"));
        assert_eq!(
            vault
                .open_optional(sealed.as_ref())
                .unwrap()
                .unwrap()
                .expose_secret(),
            "x"
        );
        assert!(vault.open_optional(None).unwrap().is_none());
    }

    #[test]
    fn tampered_data_throws() {
        let vault = Vault::create(&db(), "pw", ITERATIONS).unwrap();
        let mut bytes = vault.seal("secret").as_bytes().to_vec();
        *bytes.last_mut().unwrap() ^= 1;
        assert!(matches!(
            vault.open(&Sealed::from_bytes(bytes)),
            Err(VaultError::CorruptData)
        ));
        assert!(matches!(
            vault.open(&Sealed::from_bytes(vec![1, 2, 3])),
            Err(VaultError::CorruptData)
        ));
        let other = Vault::create(&db(), "other", ITERATIONS).unwrap();
        assert!(matches!(
            other.open(&vault.seal("x")),
            Err(VaultError::CorruptData)
        ));
    }

    #[test]
    fn non_utf8_plaintext_is_corrupt() {
        let vault = Vault::create(&db(), "pw", ITERATIONS).unwrap();
        let sealed = vault.seal_bytes(&[0xFF, 0xFE]);
        assert!(matches!(vault.open(&sealed), Err(VaultError::CorruptData)));
    }

    #[test]
    fn key_derivation_is_deterministic() {
        let salt = [7u8; 16];
        assert_eq!(*derive_key("pw", &salt, 1_000), *derive_key("pw", &salt, 1_000));
        assert_ne!(*derive_key("pw", &salt, 1_000), *derive_key("pw", &salt, 1_001));
        assert_ne!(*derive_key("pw", &salt, 1_000), *derive_key("pw2", &salt, 1_000));
    }

    #[test]
    fn iterations_are_stored_big_endian() {
        let db = db();
        Vault::create(&db, "pw", ITERATIONS).unwrap();
        assert_eq!(db.meta(ITERATIONS_KEY).unwrap(), Some(vec![0, 0, 3, 0xE8]));
        assert_eq!(db.meta(SALT_KEY).unwrap().unwrap().len(), 16);
    }

    #[test]
    fn change_password_reseals_everything() {
        let db = db();
        let mut vault = Vault::create(&db, "old", ITERATIONS).unwrap();
        let sealed = vault.seal("conn-secret");
        let cred = vault.seal("cred-key");
        db.conn()
            .execute_batch(
                "INSERT INTO credential (id, name, createdAt, updatedAt) VALUES ('C', 'c', 't', 't');
                 INSERT INTO connection (id, type, name, host, createdAt, updatedAt) VALUES ('A', 'ssh', 'a', 'h', 't', 't');",
            )
            .unwrap();
        db.conn()
            .execute("UPDATE connection SET passwordEnc = ?1", [sealed.as_bytes()])
            .unwrap();
        db.conn()
            .execute("UPDATE credential SET sshKeyUnlockedEnc = ?1", [cred.as_bytes()])
            .unwrap();

        assert!(matches!(
            vault.change_password(&db, ""),
            Err(VaultError::EmptyPassword)
        ));
        vault.change_password(&db, "new").unwrap();

        assert!(matches!(
            Vault::unlock(&db, "old"),
            Err(VaultError::WrongPassword)
        ));
        let reopened = Vault::unlock(&db, "new").unwrap();
        let read = |sql: &str| Sealed::from_bytes(db.conn().query_row(sql, [], |r| r.get(0)).unwrap());
        let conn_secret = read("SELECT passwordEnc FROM connection");
        let cred_secret = read("SELECT sshKeyUnlockedEnc FROM credential");
        assert_eq!(
            reopened.open(&conn_secret).unwrap().expose_secret(),
            "conn-secret"
        );
        assert_eq!(reopened.open(&cred_secret).unwrap().expose_secret(), "cred-key");
        let updated: String = db
            .conn()
            .query_row("SELECT updatedAt FROM connection", [], |r| r.get(0))
            .unwrap();
        assert_eq!(updated, "t");
    }

    #[test]
    fn debug_hides_the_key() {
        let vault = Vault::create(&db(), "pw", ITERATIONS).unwrap();
        assert_eq!(format!("{vault:?}"), "Vault(..)");
    }
}
