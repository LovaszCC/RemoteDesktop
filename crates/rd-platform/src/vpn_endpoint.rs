//! Where the VPN app listens for the control API (`MacVPN` on macOS, `WinVPN` on Windows, `LinVPN` on Linux):
//! the one place that picks between the copied client's per-OS defaults ([`crate::macvpn`]).

use std::io;
use std::path::PathBuf;

/// The control endpoint for this user: WinVPN's named pipe on Windows, the Unix socket elsewhere. `NotFound`
/// when it can't be named (Linux without `XDG_RUNTIME_DIR`).
///
/// # Errors
/// When the OS can't name the endpoint (no home directory, runtime directory or user SID).
pub fn control_endpoint() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::macvpn::default_endpoint()
    }
    #[cfg(not(windows))]
    {
        crate::macvpn::default_socket_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_uses_the_clients_socket_path() {
        assert_eq!(
            control_endpoint().unwrap(),
            crate::macvpn::default_socket_path().unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_winvpns_pipe_for_this_user() {
        let pipe = control_endpoint().unwrap();
        assert!(
            pipe.to_string_lossy()
                .starts_with(r"\\.\pipe\WinVPN-control-S-1-"),
            "{}",
            pipe.display()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_socket_is_macvpns() {
        assert!(
            control_endpoint()
                .unwrap()
                .ends_with("MacVPN/control/macvpn.sock")
        );
    }
}
