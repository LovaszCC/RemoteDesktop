//! The `meta` key/value table (vault salt, iteration count, verifier).

use rusqlite::{OptionalExtension, params};

use crate::{Database, StoreError};

impl Database {
    /// The value stored under `key`, if any.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn meta(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// Stores `value` under `key`, replacing an existing value.
    ///
    /// # Errors
    /// If SQLite fails.
    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        self.conn().execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Database;

    #[test]
    fn meta_upserts() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.meta("k").unwrap(), None);
        db.set_meta("k", &[1]).unwrap();
        db.set_meta("k", &[2]).unwrap();
        assert_eq!(db.meta("k").unwrap(), Some(vec![2]));
    }
}
