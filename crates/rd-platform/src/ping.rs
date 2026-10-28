//! The VPN gate's reachability test: one ICMP ping through the system's `ping`.

/// `ping`'s arguments for `host`: one packet, a 1.5 s wait for the reply and a 2 s overall limit (macOS
/// flags).
#[must_use]
pub fn ping_args(host: &str) -> Vec<String> {
    ["-c", "1", "-W", "1500", "-t", "2", host]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// Whether `host` answers one ping. A name that doesn't resolve, an empty host and one starting with `-`
/// (it would be read as an option) are not reachable; so is everything off macOS, where the gate is off.
#[must_use]
pub fn reachable(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() || host.starts_with('-') {
        return false;
    }
    run(host)
}

#[cfg(target_os = "macos")]
fn run(host: &str) -> bool {
    use std::process::{Command, Stdio};
    Command::new("/sbin/ping")
        .args(ping_args(host))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(target_os = "macos"))]
fn run(_host: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_args_are_one_packet_with_timeouts() {
        assert_eq!(
            ping_args("10.20.0.1"),
            ["-c", "1", "-W", "1500", "-t", "2", "10.20.0.1"]
        );
    }

    #[test]
    fn a_dash_host_is_never_pinged() {
        assert!(!reachable("-f"));
        assert!(!reachable(""));
        assert!(!reachable("  "));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn localhost_is_reachable() {
        assert!(reachable("127.0.0.1"));
        assert!(!reachable("no-such-host.invalid"));
    }
}
