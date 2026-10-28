//! The VPN registry and gate against a fake MacVPN backend.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::error::CommandError;
use super::vpn::{CancelFlag, VpnBackend, VpnLease, VpnNotice, VpnPhase, VpnProfileDto, VpnShared};

pub(super) const PROFILE: &str = "6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01";

/// A scripted MacVPN: counts `up`s and closes, lets a test drop a lease, can fail or block an `up`.
#[derive(Default)]
pub(super) struct FakeBackend {
    pub(super) ups: AtomicUsize,
    pub(super) closes: Arc<AtomicUsize>,
    /// Senders of the open leases, newest last: sending `Down` (or dropping one) ends that lease.
    pub(super) leases: Mutex<Vec<Sender<VpnPhase>>>,
    /// Profiles `list` reports, with their state.
    pub(super) profiles: Mutex<Vec<VpnProfileDto>>,
    /// The next `up`s fail with these errors, in order.
    pub(super) failures: Mutex<Vec<CommandError>>,
    /// When set, `up` waits for a message (or the cancel) before it answers.
    pub(super) gate: Mutex<Option<Receiver<()>>>,
    pub(super) list_error: Mutex<Option<CommandError>>,
}

struct FakeLease {
    changes: Receiver<VpnPhase>,
    closes: Arc<AtomicUsize>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl VpnLease for FakeLease {
    fn next_change(&mut self) -> Option<VpnPhase> {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return None;
            }
            match self.changes.recv_timeout(Duration::from_millis(10)) {
                Ok(phase) => return Some(phase),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn closer(&self) -> Box<dyn Fn() + Send> {
        let (count, done) = (Arc::clone(&self.closes), Arc::clone(&self.closed));
        Box::new(move || {
            if !done.swap(true, Ordering::SeqCst) {
                count.fetch_add(1, Ordering::SeqCst);
            }
        })
    }
}

impl VpnBackend for FakeBackend {
    fn list(&self) -> Result<Vec<VpnProfileDto>, CommandError> {
        if let Some(error) = self.list_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(self.profiles.lock().unwrap().clone())
    }

    fn up(
        &self,
        _profile_id: &str,
        on_phase: &mut dyn FnMut(VpnPhase),
        cancel: &CancelFlag,
    ) -> Result<Box<dyn VpnLease>, CommandError> {
        self.ups.fetch_add(1, Ordering::SeqCst);
        on_phase(VpnPhase::Connecting);
        if let Some(gate) = self.gate.lock().unwrap().take() {
            on_phase(VpnPhase::WaitingForMfa);
            let deadline = Instant::now() + Duration::from_secs(5);
            while gate.recv_timeout(Duration::from_millis(10)).is_err() {
                if cancel.is_cancelled() || Instant::now() > deadline {
                    return Err(CommandError::new("vpnCancelled", "Cancelled."));
                }
            }
        }
        let failure = {
            let mut failures = self.failures.lock().unwrap();
            (!failures.is_empty()).then(|| failures.remove(0))
        };
        if let Some(error) = failure {
            return Err(error);
        }
        on_phase(VpnPhase::Connected);
        let (sender, changes) = channel();
        self.leases.lock().unwrap().push(sender);
        Ok(Box::new(FakeLease {
            changes,
            closes: Arc::clone(&self.closes),
            closed: Arc::default(),
        }))
    }

    fn pair(&self) -> Result<(), CommandError> {
        Ok(())
    }

    fn unpair(&self) {}

    fn is_paired(&self) -> bool {
        true
    }
}

/// A backend shared with the test, so it can be inspected after `VpnShared` took it.
#[derive(Clone, Default)]
pub(super) struct SharedFake(pub(super) Arc<FakeBackend>);

impl VpnBackend for SharedFake {
    fn list(&self) -> Result<Vec<VpnProfileDto>, CommandError> {
        self.0.list()
    }
    fn up(
        &self,
        profile_id: &str,
        on_phase: &mut dyn FnMut(VpnPhase),
        cancel: &CancelFlag,
    ) -> Result<Box<dyn VpnLease>, CommandError> {
        self.0.up(profile_id, on_phase, cancel)
    }
    fn pair(&self) -> Result<(), CommandError> {
        self.0.pair()
    }
    fn unpair(&self) {
        self.0.unpair();
    }
    fn is_paired(&self) -> bool {
        self.0.is_paired()
    }
}

pub(super) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn shared() -> (Arc<VpnShared>, SharedFake, Arc<Mutex<Vec<VpnNotice>>>) {
    let fake = SharedFake::default();
    let shared = VpnShared::new(Box::new(fake.clone()));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notices);
    shared.set_notifier(Some(Arc::new(move |n| sink.lock().unwrap().push(n))));
    (shared, fake, notices)
}

fn hold(shared: &Arc<VpnShared>, entry: &str) -> super::vpn::VpnHold {
    shared
        .acquire(PROFILE, "Office", entry, &mut |_| {}, &CancelFlag::default())
        .unwrap()
}

#[test]
fn holds_share_one_lease_per_profile() {
    let (shared, fake, _) = shared();
    let first = hold(&shared, "E1");
    let second = hold(&shared, "E2");
    assert_eq!(fake.0.ups.load(Ordering::SeqCst), 1);
    assert!(first.is_alive() && second.is_alive());
    assert_eq!((first.profile_id(), first.entry_id()), (PROFILE, "E1"));
}

#[test]
fn last_hold_closes_the_lease() {
    let (shared, fake, notices) = shared();
    let first = hold(&shared, "E1");
    let second = hold(&shared, "E1");
    drop(first);
    assert_eq!(fake.0.closes.load(Ordering::SeqCst), 0);
    assert!(second.is_alive());
    drop(second);
    assert_eq!(fake.0.closes.load(Ordering::SeqCst), 1);
    let _again = hold(&shared, "E1");
    assert_eq!(fake.0.ups.load(Ordering::SeqCst), 2);
    assert!(
        notices
            .lock()
            .unwrap()
            .iter()
            .all(|n| !matches!(n, VpnNotice::Dropped { .. }))
    );
}

#[test]
fn a_dropped_lease_is_not_reused() {
    let (shared, fake, notices) = shared();
    let first = hold(&shared, "E1");
    fake.0.leases.lock().unwrap()[0].send(VpnPhase::Down).unwrap();
    wait_until("the drop", || !first.is_alive());
    assert!(
        notices
            .lock()
            .unwrap()
            .iter()
            .any(|n| matches!(n, VpnNotice::Dropped { profile_name } if profile_name == "Office"))
    );
    let second = hold(&shared, "E1");
    assert_eq!(fake.0.ups.load(Ordering::SeqCst), 2);
    let closes = fake.0.closes.load(Ordering::SeqCst); // the dropped socket was closed
    drop(first); // a stale hold changes nothing
    assert!(second.is_alive());
    assert_eq!(fake.0.closes.load(Ordering::SeqCst), closes);
}

#[test]
fn shutdown_closes_every_lease() {
    let (shared, fake, _) = shared();
    let first = hold(&shared, "E1");
    shared.shutdown();
    assert_eq!(fake.0.closes.load(Ordering::SeqCst), 1);
    assert!(!first.is_alive());
    drop(first);
    assert_eq!(fake.0.closes.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn macvpn_errors_map_to_codes() {
    use super::vpn::macvpn_error;
    use rd_platform::macvpn::{Error, ErrorCode};
    let api = |code, wire: &str, message: Option<&str>| Error::Api {
        code,
        wire: wire.into(),
        message: message.map(Into::into),
    };
    let io = |kind| Error::Io(std::io::Error::from(kind));
    let cases = [
        (io(std::io::ErrorKind::NotFound), "vpnNotRunning"),
        (io(std::io::ErrorKind::ConnectionRefused), "vpnNotRunning"),
        (api(ErrorCode::Unauthorized, "unauthorized", None), "vpnNotPaired"),
        (api(ErrorCode::NotAllowed, "not_allowed", None), "vpnNotAllowed"),
        (
            api(ErrorCode::UnknownConnection, "unknown_connection", None),
            "vpnUnknownProfile",
        ),
        (
            api(ErrorCode::SetupRequired, "setup_required", None),
            "vpnSetupRequired",
        ),
        (
            api(ErrorCode::MfaCancelled, "mfa_cancelled", None),
            "vpnCancelled",
        ),
        (
            api(ErrorCode::CredentialsCancelled, "credentials_cancelled", None),
            "vpnCancelled",
        ),
        (api(ErrorCode::MfaTimeout, "mfa_timeout", None), "vpnMfaTimeout"),
        (api(ErrorCode::Busy, "busy", None), "vpnBusy"),
        (api(ErrorCode::Failed, "locked", None), "vpnLocked"),
        (api(ErrorCode::Denied, "denied", None), "vpnPairingDenied"),
        (api(ErrorCode::Disabled, "disabled", None), "vpnNotRunning"),
        (api(ErrorCode::AppQuitting, "app_quitting", None), "vpnNotRunning"),
        (Error::Closed, "vpnFailed"),
    ];
    for (error, code) in cases {
        assert_eq!(macvpn_error(&error).code, code, "{error:?}");
    }
    let failed = macvpn_error(&api(ErrorCode::Failed, "failed", Some("Timed out")));
    assert_eq!(
        (failed.code.as_str(), failed.detail.as_deref()),
        ("vpnFailed", Some("Timed out"))
    );
}

#[cfg(unix)]
#[test]
fn macvpn_states_map_to_phases() {
    use super::vpn::phase_of;
    use rd_platform::macvpn::State;
    assert_eq!(phase_of(&State::Connecting), VpnPhase::Connecting);
    assert_eq!(phase_of(&State::Other("new".into())), VpnPhase::Connecting);
    assert_eq!(phase_of(&State::WaitingForMfa), VpnPhase::WaitingForMfa);
    assert_eq!(
        phase_of(&State::WaitingForCredentials),
        VpnPhase::WaitingForCredentials
    );
    assert_eq!(phase_of(&State::Connected), VpnPhase::Connected);
    assert_eq!(phase_of(&State::Failed), VpnPhase::Down);
    assert_eq!(phase_of(&State::Disconnected), VpnPhase::Down);
}
