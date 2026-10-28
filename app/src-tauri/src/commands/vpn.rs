//! Connect with VPN (macOS) commands. Events:
//! - `vpn-progress` ([`VpnProgressDto`]): a connect's VPN gate (testing, connecting, MFA, done, failed);
//! - `vpn-changed` (no payload): the held VPNs or the pairing changed; fetch [`vpn_status`];
//! - `vpn-dropped` (`{profileName}`): a held VPN went down on its own.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{Core, blocking};
use crate::core::{AppCore, CommandError, VpnNotice, VpnNotifier, VpnProfileDto, VpnStatusDto};

/// Event: a VPN gate's progress.
pub const VPN_PROGRESS: &str = "vpn-progress";
/// Event: the held VPNs or the pairing changed.
pub const VPN_CHANGED: &str = "vpn-changed";
/// Event: a held VPN went down on its own.
pub const VPN_DROPPED: &str = "vpn-dropped";

/// Payload of [`VPN_DROPPED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnDroppedDto {
    /// The VPN's name.
    pub profile_name: String,
}

/// Connects the VPN notices to events. Called once in setup.
pub fn install_hooks(app: &AppHandle, core: &AppCore) {
    let handle = app.clone();
    let notifier: VpnNotifier = Arc::new(move |notice| {
        let _ = match notice {
            VpnNotice::Progress(progress) => handle.emit(VPN_PROGRESS, progress),
            VpnNotice::Changed => handle.emit(VPN_CHANGED, ()),
            VpnNotice::Dropped { profile_name } => handle.emit(VPN_DROPPED, VpnDroppedDto { profile_name }),
        };
    });
    core.set_vpn_hooks(notifier);
}

/// Pairing state and the held VPNs.
#[tauri::command]
pub fn vpn_status(core: State<'_, Core>) -> VpnStatusDto {
    core.vpn_status()
}

/// The MacVPN connections Remote Desktop may use. Errors: `vpnNotRunning`, `vpnNotPaired`, `vpnLocked`,
/// `vpnFailed`.
#[tauri::command]
pub async fn vpn_profiles(core: State<'_, Core>) -> Result<Vec<VpnProfileDto>, CommandError> {
    blocking(&core, AppCore::vpn_profiles).await
}

/// Pairs with MacVPN; waits (up to about 2½ minutes) for the user to answer MacVPN's window. Errors:
/// `vpnNotRunning`, `vpnPairingDenied`, `vpnBusy`, `vpnFailed`.
#[tauri::command]
pub async fn vpn_pair(core: State<'_, Core>) -> Result<(), CommandError> {
    blocking(&core, AppCore::vpn_pair).await
}

/// Forgets the MacVPN token.
#[tauri::command]
pub fn vpn_unpair(core: State<'_, Core>) {
    core.vpn_unpair();
}

/// Cancels a running VPN gate (the connect then fails with `vpnCancelled`).
#[tauri::command]
pub fn vpn_cancel(core: State<'_, Core>, request_id: String) {
    core.vpn_cancel(&request_id);
}

/// "Release VPN" on an entry: drops its external launch's hold.
#[tauri::command]
pub fn vpn_release_entry(core: State<'_, Core>, entry_id: String) {
    core.vpn_release_entry(&entry_id);
}

/// "Release" in Settings: drops the external launches' holds on a profile; open tabs keep theirs.
#[tauri::command]
pub fn vpn_release_profile(core: State<'_, Core>, profile_id: String) {
    core.vpn_release_profile(&profile_id);
}
