//! "Set up SSH key login" and the editors' "Generate…". An unknown host key of the upload connection
//! arrives as `host-key-prompt` (its `forwardId` is `ssh-key-<connection id>`); answer it with
//! [`host_key_answer`](super::forwards::host_key_answer).

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use super::forwards::HOST_KEY_PROMPT;
use super::{Core, blocking, emit_library};
use crate::core::{AppCore, CommandError, KeySetupDto, KeySetupNotifier};

/// Connects the key setup's host key prompts to `host-key-prompt`. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: KeySetupNotifier = Arc::new(move |prompt| {
        let _ = handle.emit(HOST_KEY_PROMPT, prompt);
    });
    core.set_key_setup_hooks(notifier);
}

/// Generates an ed25519 key pair in `~/.ssh` named after `name`; the `~`-relative path. Nothing is saved.
/// Errors: `noHome`, `keyGenerateFailed`.
#[tauri::command]
pub async fn ssh_key_generate(core: State<'_, Core>, name: String) -> Result<String, CommandError> {
    blocking(&core, move |_| AppCore::ssh_key_generate(&name)).await
}

/// Installs the connection's SSH key on its server, generating and saving one first when it has none;
/// emits `library-changed`. The VPN gate runs first. Errors: the `vpn*` codes, `locked`, `notFound`, `notSsh`, `emptyHost`, `readOnly`, `noHome`,
/// `keyGenerateFailed`, `cannotReadKey`, `invalidPublicKey`, a connection code or `keyInstallFailed`.
#[tauri::command]
pub async fn ssh_key_setup(
    app: AppHandle,
    core: State<'_, Core>,
    connection_id: String,
) -> Result<KeySetupDto, CommandError> {
    // The VPN gate may wait minutes for MacVPN's sign-in: off the async workers; the hold lives until
    // the key is installed.
    let id = connection_id.clone();
    let _vpn = blocking(&core, move |c| c.vpn_gate(&id)).await?;
    let core = Arc::clone(&core);
    let result = core.ssh_key_setup(&connection_id).await;
    // A generated key is saved even when the upload fails.
    emit_library(&app, &core);
    result
}
