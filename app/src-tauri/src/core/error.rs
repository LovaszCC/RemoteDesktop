//! The one error type every command returns to the webview.

use rd_core::draft::DraftError;
use rd_core::forward::PortForwardError;
use rd_core::shortcuts::{Platform, ShortcutError};
use rd_store::{StoreError, VaultError};
use serde::Serialize;

/// A failed command: a stable `code` the UI translates, the English `message`, and an optional `detail`
/// (a path, a command id or the underlying error text). Never carries a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// Stable id, for example `wrongPassword` or `readOnly`.
    pub code: String,
    /// English text for logs and as the translation key fallback.
    pub message: String,
    /// Extra context: a file path, a command id or an error text.
    pub detail: Option<String>,
}

impl CommandError {
    /// An error without detail.
    #[must_use]
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            detail: None,
        }
    }

    /// Adds the detail.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// No database is open.
    #[must_use]
    pub fn no_database() -> Self {
        Self::new("noDatabase", "No database is open.")
    }

    /// The vault is locked.
    #[must_use]
    pub fn locked() -> Self {
        Self::new("locked", "RemoteDesktop is locked.")
    }

    /// The database was opened read-only because another instance holds it.
    #[must_use]
    pub fn read_only() -> Self {
        Self::new(
            "readOnly",
            "The database is open read-only because another computer is using it.",
        )
    }

    /// A record id that doesn't exist.
    #[must_use]
    pub fn not_found(id: &str) -> Self {
        Self::new("notFound", "The item doesn't exist any more.").with_detail(id)
    }

    /// A required name is empty.
    #[must_use]
    pub fn empty_name() -> Self {
        Self::new("emptyName", "The name is required.")
    }

    /// The settings file could not be written.
    #[must_use]
    pub fn settings_write(error: &std::io::Error) -> Self {
        Self::new("settingsWrite", "Couldn't save the settings.").with_detail(error.to_string())
    }

    /// A shortcut refusal on `platform`.
    #[must_use]
    pub fn shortcut(error: ShortcutError, platform: Platform) -> Self {
        let mut result = Self::new(error.code(), error.message(platform));
        if let ShortcutError::InUse(command) = error {
            result.detail = Some(command.as_str().to_owned());
        }
        result
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}

impl From<StoreError> for CommandError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound => Self::new("notFound", "The item doesn't exist any more."),
            StoreError::FolderCycle => Self::new("folderCycle", "A folder can't be moved into itself"),
            other => Self::new("database", "The database failed.").with_detail(other.to_string()),
        }
    }
}

impl From<VaultError> for CommandError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::WrongPassword => Self::new("wrongPassword", "Wrong password"),
            VaultError::EmptyPassword => Self::new("emptyPassword", "The password is empty."),
            VaultError::NotInitialized => Self::new("vaultNotSetUp", "The vault is not set up."),
            VaultError::AlreadyInitialized => Self::new("vaultAlreadySetUp", "The vault is already set up."),
            VaultError::CorruptData => Self::new("corruptData", "A stored secret is damaged."),
            VaultError::Store(store) => store.into(),
        }
    }
}

impl From<DraftError> for CommandError {
    fn from(error: DraftError) -> Self {
        Self::new(error.code(), &error.to_string())
    }
}

impl From<PortForwardError> for CommandError {
    fn from(error: PortForwardError) -> Self {
        Self::new(error.code(), &error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rd_core::shortcuts::ShortcutCommand;

    use super::*;

    #[test]
    fn serializes_code_message_detail() {
        let json = serde_json::to_value(CommandError::not_found("X")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"code": "notFound", "message": "The item doesn't exist any more.", "detail": "X"})
        );
        let err = CommandError::shortcut(ShortcutError::InUse(ShortcutCommand::Lock), Platform::MacOS);
        assert_eq!(
            (err.code.as_str(), err.detail.as_deref()),
            ("inUse", Some("lock"))
        );
        assert_eq!(CommandError::from(DraftError::InvalidPort).code, "invalidPort");
        assert_eq!(
            CommandError::from(VaultError::WrongPassword).code,
            "wrongPassword"
        );
        assert_eq!(CommandError::from(StoreError::FolderCycle).code, "folderCycle");
    }
}
