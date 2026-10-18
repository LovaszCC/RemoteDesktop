//! Errors from the store.

/// Anything that can go wrong opening or using the database.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// The database folder could not be created.
    #[error("cannot create the database folder: {0}")]
    Io(#[from] std::io::Error),
    /// The record doesn't exist.
    #[error("not found")]
    NotFound,
    /// Moving the folder would put it inside itself.
    #[error("a folder cannot be moved into itself")]
    FolderCycle,
    /// A migration left rows that violate a foreign key.
    #[error("migration {id} failed its foreign key check")]
    ForeignKeyViolation {
        /// The migration id.
        id: &'static str,
    },
}
