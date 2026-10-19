//! SQLite store with GRDB-compatible migrations, the repository and the vault.
//!
//! The database file stays readable by the Swift app: same schema, same migration ids, same encodings.

mod app_version;
mod database;
mod error;
#[cfg(test)]
mod fixture_tests;
mod import;
mod key_cache;
mod lock;
mod meta;
mod migrations;
mod repository;
mod rows;
mod vault;

pub use app_version::{APP_VERSION_KEY, is_newer, pending_migrations, stored_app_version};
pub use database::Database;
pub use error::StoreError;
pub use import::{ImportBatch, ImportCounts};
pub use key_cache::{KeyCache, KeyCacheError, KeyringCache, MemoryKeyCache};
pub use lock::{DbLock, LockHolder, LockStatus, REFRESH_EVERY, STALE_AFTER};
pub use migrations::{MIGRATION_IDS, migrate_up_to};
pub use repository::EntryWrite;
pub use vault::{Vault, VaultError};
