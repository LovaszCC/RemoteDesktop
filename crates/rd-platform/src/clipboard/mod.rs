//! The OS clipboard, with the "don't record this" markers for secrets.
//!
//! One `arboard::Clipboard` lives for the whole process: on Linux the contents are served by that instance,
//! so dropping it after each copy would empty the clipboard.
//!
//! [`copy_concealed`] marks the text for clipboard managers and history, through `arboard`'s `Set`
//! extensions:
//! - macOS: an empty `org.nspasteboard.ConcealedType` item next to the text (the nspasteboard.org
//!   convention that Raycast, Alfred, Maccy and Paste honour);
//! - Windows: the `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0` and
//!   `CanUploadToCloudClipboard = 0` formats (Win+V history, cloud clipboard and monitoring apps);
//! - Linux: `x-kde-passwordManagerHint = secret` (Klipper and the clipboard managers that copied it). This
//!   is best effort: GNOME's managers ignore it, and this build serves the clipboard over X11 only
//!   (`arboard` without its `wayland-data-control` feature), so a Wayland-only session reaches it through
//!   `XWayland`.
//!
//! [`copy_text`] copies without a marker: the web launcher's password and username copies stay in the
//! clipboard history on purpose (Swift `WebLauncher`).
//!
//! File lists (RDP file clipboard) are in [`files`].

mod files;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
mod x11_files;

pub use files::{
    copy_file_list, file_uri, gnome_copied_files, purge_stale_rdp_clipboard_folders, rdp_clipboard_root,
    rdp_clipboard_session_dir, read_file_list, uri_list,
};

use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// How long a copied password stays on the clipboard.
pub const PASSWORD_LIFETIME: Duration = Duration::from_secs(30);

static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

/// The clipboard can't be reached or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(pub String);

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ClipboardError {}

impl From<arboard::Error> for ClipboardError {
    fn from(error: arboard::Error) -> Self {
        Self(error.to_string())
    }
}

fn with_clipboard<T>(
    work: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, ClipboardError> {
    let mut slot = CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(arboard::Clipboard::new()?);
    }
    let clipboard = slot
        .as_mut()
        .ok_or_else(|| ClipboardError("no clipboard".to_owned()))?;
    Ok(work(clipboard)?)
}

/// Asks clipboard managers and the OS history not to record what `set` writes.
fn concealed(set: arboard::Set<'_>) -> arboard::Set<'_> {
    #[cfg(target_os = "macos")]
    {
        use arboard::SetExtApple;
        set.exclude_from_history()
    }
    #[cfg(windows)]
    {
        use arboard::SetExtWindows;
        set.exclude_from_monitoring()
            .exclude_from_history()
            .exclude_from_cloud()
    }
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    {
        use arboard::SetExtLinux;
        set.exclude_from_history()
    }
    #[cfg(not(any(unix, windows)))]
    {
        set
    }
}

/// Puts plain text on the clipboard, unmarked.
///
/// # Errors
/// When the OS clipboard is unavailable.
pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
    with_clipboard(|c| c.set_text(text))
}

/// Copies `values` one after another, unmarked and never cleared, waiting `interval` between them so
/// clipboard managers record each one (Swift `WebLauncher`). The first copy happens before returning;
/// the rest follow on a background thread.
///
/// # Errors
/// When the first copy fails; later failures are ignored.
pub fn copy_sequence(
    values: Vec<zeroize::Zeroizing<String>>,
    interval: Duration,
) -> Result<(), ClipboardError> {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return Ok(());
    };
    copy_text(&first)?;
    let rest: Vec<_> = values.collect();
    if !rest.is_empty() {
        std::thread::spawn(move || {
            for value in rest {
                std::thread::sleep(interval);
                let _ = copy_text(&value);
            }
        });
    }
    Ok(())
}

/// The clipboard's plain text; `None` when it holds no text.
///
/// # Errors
/// When the OS clipboard is unavailable.
pub fn read_text() -> Result<Option<String>, ClipboardError> {
    with_clipboard(|c| match c.get_text() {
        Ok(text) => Ok(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(e),
    })
}

/// Puts a secret on the clipboard marked as concealed, and clears it after `lifetime` unless something
/// else was copied meanwhile. `secret` is wiped from memory once the timer has run.
///
/// # Errors
/// When the OS clipboard is unavailable.
pub fn copy_concealed(secret: zeroize::Zeroizing<String>, lifetime: Duration) -> Result<(), ClipboardError> {
    with_clipboard(|c| concealed(c.set()).text(secret.as_str()))?;
    std::thread::spawn(move || {
        std::thread::sleep(lifetime);
        let _ = with_clipboard(|c| {
            if c.get_text().is_ok_and(|now| now == *secret) {
                c.clear()?;
            }
            Ok(())
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real clipboard; run by hand with
    /// `cargo test -p rd-platform -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "needs a desktop clipboard"]
    fn secret_is_cleared_only_if_unchanged() {
        copy_concealed("secret-value".to_owned().into(), Duration::from_millis(100)).unwrap();
        assert_eq!(read_text().unwrap().as_deref(), Some("secret-value"));
        std::thread::sleep(Duration::from_millis(300));
        assert!(read_text().unwrap().is_none_or(|t| t.is_empty()));

        copy_concealed("secret-value".to_owned().into(), Duration::from_millis(100)).unwrap();
        copy_text("other").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(read_text().unwrap().as_deref(), Some("other"));
    }

    /// Touches the real clipboard; run by hand with
    /// `cargo test -p rd-platform -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "needs a desktop clipboard"]
    fn sequence_ends_with_the_last_value() {
        copy_sequence(
            vec!["first".to_owned().into(), "second".to_owned().into()],
            Duration::from_millis(100),
        )
        .unwrap();
        assert_eq!(read_text().unwrap().as_deref(), Some("first"));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(read_text().unwrap().as_deref(), Some("second"));
    }

    #[test]
    fn empty_sequence_touches_nothing() {
        assert_eq!(copy_sequence(Vec::new(), Duration::ZERO), Ok(()));
    }

    #[test]
    fn errors_carry_the_message() {
        let error = ClipboardError::from(arboard::Error::ContentNotAvailable);
        assert!(!error.to_string().is_empty());
    }
}
