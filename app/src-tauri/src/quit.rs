//! The quit confirmation (Swift `AppDelegate.applicationShouldTerminate`): while sessions, port forwards or
//! transfers run, closing the window or quitting asks "Sessions are running" with [Quit] [Cancel].

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::commands::Core;
use crate::menu::tr;

/// What closing the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitDecision {
    /// Nothing is running, or the user already confirmed.
    Proceed,
    /// Ask first; `count` things would be closed.
    Confirm {
        /// The running count.
        count: usize,
    },
}

/// Asks only when something runs and the user hasn't confirmed yet.
#[must_use]
pub fn quit_decision(running: usize, confirmed: bool) -> QuitDecision {
    if running == 0 || confirmed {
        QuitDecision::Proceed
    } else {
        QuitDecision::Confirm { count: running }
    }
}

/// The dialog's texts in `language`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuitTexts {
    /// "Sessions are running".
    pub title: String,
    /// "N sessions are still running. Quitting closes all of them."
    pub message: String,
    /// The confirm button.
    pub quit: String,
    /// The cancel button.
    pub cancel: String,
}

/// The confirmation texts for `count` running items.
#[must_use]
pub fn quit_texts(language: &str, count: usize) -> QuitTexts {
    QuitTexts {
        title: tr(language, "Sessions are running").to_owned(),
        message: tr(
            language,
            "%lld sessions are still running. Quitting closes all of them.",
        )
        .replace("%lld", &count.to_string()),
        quit: tr(language, "Quit").to_owned(),
        cancel: tr(language, "Cancel").to_owned(),
    }
}

/// Set once the user chose Quit, so the exit that follows isn't asked again.
static CONFIRMED: AtomicBool = AtomicBool::new(false);

fn decision(app: &AppHandle) -> QuitDecision {
    let running = app.try_state::<Core>().map_or(0, |core| core.running_count());
    quit_decision(running, CONFIRMED.load(Ordering::SeqCst))
}

/// Whether a close or exit must wait for the user. When it must, the dialog is shown and Quit exits the app.
#[must_use]
pub fn intercept(app: &AppHandle) -> bool {
    intercept_then(app, |app| app.exit(0))
}

/// Like [`intercept`], but Quit runs `proceed` (exit or restart).
fn intercept_then(app: &AppHandle, proceed: impl FnOnce(&AppHandle) + Send + 'static) -> bool {
    let QuitDecision::Confirm { count } = decision(app) else {
        return false;
    };
    let language = app
        .try_state::<Core>()
        .map_or_else(|| "en".to_owned(), |core| core.settings_get().language);
    let texts = quit_texts(&language, count);
    let handle = app.clone();
    app.dialog()
        .message(texts.message)
        .title(texts.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(texts.quit, texts.cancel))
        .show(move |quit| {
            if quit {
                CONFIRMED.store(true, Ordering::SeqCst);
                proceed(&handle);
            }
        });
    true
}

/// The Quit menu item: confirm if needed, then exit.
pub fn request_quit(app: &AppHandle) {
    if !intercept(app) {
        CONFIRMED.store(true, Ordering::SeqCst);
        app.exit(0);
    }
}

/// Relaunch after an update: confirm if needed, then restart (the exit path still closes the database).
pub fn request_relaunch(app: &AppHandle) {
    if !intercept_then(app, AppHandle::request_restart) {
        CONFIRMED.store(true, Ordering::SeqCst);
        app.request_restart();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_only_while_something_runs() {
        assert_eq!(quit_decision(0, false), QuitDecision::Proceed);
        assert_eq!(quit_decision(3, false), QuitDecision::Confirm { count: 3 });
        assert_eq!(quit_decision(3, true), QuitDecision::Proceed);
    }

    #[test]
    fn texts_are_translated_with_the_count() {
        let en = quit_texts("en", 2);
        assert_eq!(en.title, "Sessions are running");
        assert_eq!(
            en.message,
            "2 sessions are still running. Quitting closes all of them."
        );
        assert_eq!((en.quit.as_str(), en.cancel.as_str()), ("Quit", "Cancel"));
        let hu = quit_texts("hu", 5);
        assert_eq!(hu.title, "Futó sessionök");
        assert_eq!(hu.message, "5 session még fut. Kilépéskor mind lezárul.");
        assert_eq!((hu.quit.as_str(), hu.cancel.as_str()), ("Kilépés", "Mégse"));
    }
}
