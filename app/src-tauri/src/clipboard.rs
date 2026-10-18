//! The OS clipboard for the commands: [`rd_platform::clipboard`] with its errors as `clipboard`.

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

use crate::core::CommandError;

pub use rd_platform::clipboard::PASSWORD_LIFETIME;

fn failed(error: &rd_platform::clipboard::ClipboardError) -> CommandError {
    CommandError::new("clipboard", "The clipboard is not available.").with_detail(error.to_string())
}

/// Puts plain text on the clipboard, unmarked.
///
/// # Errors
/// `clipboard` when the OS clipboard is unavailable.
pub fn copy_text(text: &str) -> Result<(), CommandError> {
    rd_platform::clipboard::copy_text(text).map_err(|e| failed(&e))
}

/// The clipboard's plain text; `None` when it holds no text.
///
/// # Errors
/// `clipboard` when the OS clipboard is unavailable.
pub fn read_text() -> Result<Option<String>, CommandError> {
    rd_platform::clipboard::read_text().map_err(|e| failed(&e))
}

/// The files and folders on the clipboard; `None` when it holds none.
///
/// # Errors
/// `clipboard` when the OS clipboard is unavailable.
pub fn read_file_list() -> Result<Option<Vec<std::path::PathBuf>>, CommandError> {
    rd_platform::clipboard::read_file_list().map_err(|e| failed(&e))
}

/// Puts files and folders on the clipboard.
///
/// # Errors
/// `clipboard` when the OS clipboard is unavailable.
pub fn copy_file_list(paths: &[std::path::PathBuf]) -> Result<(), CommandError> {
    rd_platform::clipboard::copy_file_list(paths).map_err(|e| failed(&e))
}

/// Puts a secret on the clipboard marked as concealed and clears it after `lifetime`, unless something
/// else was copied meanwhile.
///
/// # Errors
/// `clipboard` when the OS clipboard is unavailable.
pub fn copy_secret(secret: &SecretString, lifetime: Duration) -> Result<(), CommandError> {
    rd_platform::clipboard::copy_concealed(secret.expose_secret().to_owned().into(), lifetime)
        .map_err(|e| failed(&e))
}
