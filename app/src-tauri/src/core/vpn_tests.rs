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
    pub(super) lists: AtomicUsize,
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
        self.lists.fetch_add(1, Ordering::SeqCst);
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

// The gate

use super::AppCore;
use super::tests::{Env, entry_draft};
use super::vpn::{VpnProgressDto, VpnStatusDto};
use rd_core::ConnectionType;
use rd_core::VpnTestMode;
use rd_core::draft::{ConnectionDraft, EntryDraft, EntryVpnDraft};
use std::sync::atomic::AtomicBool;

struct Gate {
    _env: Env,
    core: Arc<AppCore>,
    connection: String,
    entry: String,
    fake: SharedFake,
    pings: Arc<Mutex<Vec<String>>>,
    reachable: Arc<AtomicBool>,
    progress: Arc<Mutex<Vec<VpnProgressDto>>>,
}

fn gate_lab(vpn: Option<EntryVpnDraft>) -> Gate {
    let env = Env::new();
    let core = env.unlocked();
    gate_lab_on(env, core, vpn, ConnectionDraft::new(ConnectionType::Ssh))
}

/// Like [`gate_lab`] with another core (a recording launcher) and connection draft.
fn gate_lab_on(env: Env, core: AppCore, vpn: Option<EntryVpnDraft>, connection: ConnectionDraft) -> Gate {
    let core = Arc::new(core);
    let draft = EntryDraft {
        vpn,
        ..entry_draft("web01", "10.20.0.15")
    };
    let entry = core
        .save_entry(
            None,
            &draft,
            &[super::ConnectionSave {
                id: None,
                draft: connection,
                password: Some("pw".into()),
                passphrase: None,
                is_default: true,
            }],
        )
        .unwrap();
    let connection = core.library().entries[0].connection_ids[0].clone();
    let fake = SharedFake::default();
    *fake.0.profiles.lock().unwrap() = vec![VpnProfileDto {
        id: PROFILE.into(),
        name: "Office".into(),
        kind: "openvpn".into(),
        state: "disconnected".into(),
    }];
    core.set_vpn_backend(Box::new(fake.clone()));
    let (pings, reachable) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
    let (log, answer) = (Arc::clone(&pings), Arc::clone(&reachable));
    core.set_vpn_pinger(Box::new(move |host: &str| {
        log.lock().unwrap().push(host.to_owned());
        answer.load(Ordering::SeqCst)
    }));
    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&progress);
    core.set_vpn_hooks(Arc::new(move |notice| {
        if let VpnNotice::Progress(p) = notice {
            sink.lock().unwrap().push(p);
        }
    }));
    Gate {
        _env: env,
        core,
        connection,
        entry,
        fake,
        pings,
        reachable,
        progress,
    }
}

fn office(mode: VpnTestMode, address: &str) -> EntryVpnDraft {
    EntryVpnDraft {
        connection_id: PROFILE.into(),
        test_mode: mode,
        test_address: address.into(),
    }
}

fn phases(lab: &Gate) -> Vec<String> {
    lab.progress
        .lock()
        .unwrap()
        .iter()
        .map(|p| p.phase.clone())
        .collect()
}

#[test]
fn no_profile_skips_the_gate_without_ping() {
    let lab = gate_lab(None);
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_none());
    assert!(lab.pings.lock().unwrap().is_empty());
    assert!(lab.progress.lock().unwrap().is_empty());
    assert!(lab.core.vpn_gate("no-such-connection").unwrap().is_none());
}

#[test]
fn reachable_host_connects_without_a_hold() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    lab.reachable.store(true, Ordering::SeqCst);
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_none());
    assert_eq!(*lab.pings.lock().unwrap(), ["10.20.0.15"]);
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 0);
    assert_eq!(phases(&lab), ["testing", "done"]);
}

#[test]
fn unreachable_host_brings_the_vpn_up() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let hold = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    assert!(hold.is_alive());
    assert_eq!(hold.entry_id(), lab.entry);
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 1);
    assert_eq!(phases(&lab), ["testing", "connecting", "connecting", "done"]);
    let connecting = &lab.progress.lock().unwrap()[1];
    assert_eq!(
        (
            connecting.profile_name.as_str(),
            connecting.entry_name.as_str(),
            connecting.unreachable.as_deref()
        ),
        ("Office", "web01", Some("10.20.0.15"))
    );
}

#[test]
fn address_mode_pings_the_test_address() {
    let lab = gate_lab(Some(office(VpnTestMode::Address, "10.20.0.1")));
    lab.reachable.store(true, Ordering::SeqCst);
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_none());
    assert_eq!(*lab.pings.lock().unwrap(), ["10.20.0.1"]);
}

#[test]
fn gate_takes_a_hold_when_the_profile_is_already_connected() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    lab.reachable.store(true, Ordering::SeqCst);
    lab.fake.0.profiles.lock().unwrap()[0].state = "connected".into();
    let hold = lab.core.vpn_gate(&lab.connection).unwrap();
    assert!(hold.is_some(), "a ping through the VPN must not skip the hold");
    assert!(lab.pings.lock().unwrap().is_empty());
    let again = lab.core.vpn_gate(&lab.connection).unwrap();
    assert!(again.is_some());
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_gates_join_one_up() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let (open, gate) = channel();
    *lab.fake.0.gate.lock().unwrap() = Some(gate);
    let run = || {
        let (core, connection) = (Arc::clone(&lab.core), lab.connection.clone());
        std::thread::spawn(move || core.vpn_gate(&connection))
    };
    let (first, second) = (run(), run());
    wait_until("both gates waiting", || {
        gate_connecting(&lab, 0) && gate_connecting(&lab, 1)
    });
    open.send(()).unwrap();
    let holds = [first.join().unwrap().unwrap(), second.join().unwrap().unwrap()];
    assert!(
        holds
            .iter()
            .all(|h| h.as_ref().is_some_and(super::vpn::VpnHold::is_alive))
    );
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 1);
}

#[test]
fn unreachable_macvpn_still_connects_a_reachable_host() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    *lab.fake.0.list_error.lock().unwrap() = Some(CommandError::new("vpnNotRunning", "x"));
    lab.reachable.store(true, Ordering::SeqCst);
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_none());
}

#[test]
fn macvpn_errors_stop_the_connect() {
    for code in [
        "vpnSetupRequired",
        "vpnNotAllowed",
        "vpnCancelled",
        "vpnNotPaired",
    ] {
        let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
        lab.fake
            .0
            .failures
            .lock()
            .unwrap()
            .push(CommandError::new(code, "x"));
        assert_eq!(lab.core.vpn_gate(&lab.connection).unwrap_err().code, code);
        assert_eq!(phases(&lab).last().map(String::as_str), Some("failed"));
        assert!(lab.core.vpn_status().held.is_empty());
    }
}

#[test]
fn cancel_closes_the_socket() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let (_open, gate) = channel::<()>();
    *lab.fake.0.gate.lock().unwrap() = Some(gate);
    let (core, connection) = (Arc::clone(&lab.core), lab.connection.clone());
    let running = std::thread::spawn(move || core.vpn_gate(&connection).map(|h| h.is_some()));
    wait_until("the MFA prompt", || {
        phases(&lab).iter().any(|p| p == "waitingForMfa")
    });
    let request = lab.progress.lock().unwrap()[0].request_id.clone();
    lab.core.vpn_cancel(&request);
    assert_eq!(running.join().unwrap().unwrap_err().code, "vpnCancelled");
    assert!(lab.core.vpn_status().held.is_empty());
    lab.core.vpn_cancel("unknown");
}

#[test]
fn external_hold_is_one_per_entry() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let first = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    lab.core.hold_external(&lab.entry, "web01", first);
    let second = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    lab.core.hold_external(&lab.entry, "web01", second);
    let status = lab.core.vpn_status();
    assert_eq!(status.held.len(), 1);
    assert_eq!(status.held[0].external_entries.len(), 1);
    assert_eq!(status.held[0].open_holders, 0);
    lab.core.vpn_release_entry(&lab.entry);
    assert!(lab.core.vpn_status().held.is_empty());
    assert_eq!(lab.fake.0.closes.load(Ordering::SeqCst), 1);
}

#[test]
fn release_profile_keeps_open_session_holds() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let session = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    let external = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    lab.core.hold_external(&lab.entry, "web01", external);
    lab.core.vpn_release_profile(PROFILE);
    assert!(session.is_alive());
    let status: VpnStatusDto = lab.core.vpn_status();
    assert_eq!(
        (status.held[0].external_entries.len(), status.held[0].open_holders),
        (0, 1)
    );
    assert_eq!(status.held[0].entry_ids, std::slice::from_ref(&lab.entry));
    drop(session);
    assert!(lab.core.vpn_status().held.is_empty());
}

#[test]
fn status_lists_holders() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let status = lab.core.vpn_status();
    assert!(status.supported && status.paired && status.held.is_empty());
    let _hold = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    let status = lab.core.vpn_status();
    assert_eq!(status.held[0].profile_id, PROFILE);
    assert_eq!(status.held[0].profile_name, "Office");
    assert_eq!(status.held[0].open_holders, 1);
    assert_eq!(lab.core.vpn_profiles().unwrap().len(), 1);
}

// Every connect path

fn held(lab: &Gate) -> Vec<(usize, u32)> {
    lab.core
        .vpn_status()
        .held
        .iter()
        .map(|h| (h.external_entries.len(), h.open_holders))
        .collect()
}

#[test]
fn external_launch_keeps_the_vpn() {
    let env = Env::new();
    let (core, recorder) = super::launch_tests::recording(&env, None);
    let mut draft = ConnectionDraft::new(ConnectionType::Web);
    draft.port = "8443".into();
    let lab = gate_lab_on(env, core, Some(office(VpnTestMode::Host, "")), draft);
    lab.core.connect_web(&lab.connection).unwrap();
    assert_eq!(
        recorder.calls().len(),
        2,
        "the browser opened, the password was copied"
    );
    assert_eq!(held(&lab), [(1, 0)]);
    lab.core.connect_web(&lab.connection).unwrap();
    assert_eq!(held(&lab), [(1, 0)], "one hold per entry");
    lab.core.vpn_release_entry(&lab.entry);
    assert!(held(&lab).is_empty());
}

#[test]
fn a_failed_gate_launches_nothing() {
    let env = Env::new();
    let (core, recorder) = super::launch_tests::recording(&env, None);
    let lab = gate_lab_on(
        env,
        core,
        Some(office(VpnTestMode::Host, "")),
        ConnectionDraft::new(ConnectionType::Rdp),
    );
    lab.fake
        .0
        .failures
        .lock()
        .unwrap()
        .push(CommandError::new("vpnNotAllowed", "x"));
    assert_eq!(
        lab.core.connect_rdp_external(&lab.connection).unwrap_err().code,
        "vpnNotAllowed"
    );
    assert!(recorder.calls().is_empty());
}

#[test]
fn forward_stop_releases_the_vpn() {
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = server.local_addr().unwrap().port().to_string();
    draft.username = "tester".into();
    let env = Env::new();
    let core = env.unlocked();
    let lab = gate_lab_on(env, core, Some(office(VpnTestMode::Host, "")), draft);
    let local = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let id = lab
        .core
        .port_forward_save(&rd_core::forward::PortForwardDraft {
            id: None,
            connection_id: lab.connection.clone(),
            name: String::new(),
            local_port: local.to_string(),
            remote_host: "127.0.0.1".into(),
            remote_port: "5432".into(),
        })
        .unwrap();
    lab.core.port_forward_start(&id).unwrap();
    assert_eq!(held(&lab), [(0, 1)]);
    lab.core.port_forward_stop(&id);
    assert!(held(&lab).is_empty());
}

#[test]
fn files_close_releases_the_vpn() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.port = port.to_string();
    draft.username = "tester".into();
    let env = Env::new();
    let core = env.unlocked();
    let lab = gate_lab_on(env, core, Some(office(VpnTestMode::Host, "")), draft);
    let opened = lab.core.files_open(&lab.connection, None, None).unwrap();
    assert_eq!(held(&lab), [(0, 1)]);
    lab.core.files_close(&opened.browser_id);
    assert!(held(&lab).is_empty());
}

// Final review fixes

/// Starts a gate on its own thread.
fn gate_in_background(
    lab: &Gate,
) -> std::thread::JoinHandle<Result<Option<super::vpn::VpnHold>, CommandError>> {
    let (core, connection) = (Arc::clone(&lab.core), lab.connection.clone());
    std::thread::spawn(move || core.vpn_gate(&connection))
}

fn request_ids(lab: &Gate) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for p in lab.progress.lock().unwrap().iter() {
        if !ids.contains(&p.request_id) {
            ids.push(p.request_id.clone());
        }
    }
    ids
}

/// Whether the `n`th gate (in start order) reported "connecting" (it runs or joined the `up`).
fn gate_connecting(lab: &Gate, n: usize) -> bool {
    let ids = request_ids(lab);
    ids.get(n).is_some_and(|id| {
        lab.progress
            .lock()
            .unwrap()
            .iter()
            .any(|p| &p.request_id == id && p.phase == "connecting")
    })
}

#[test]
fn cancel_wakes_a_joined_gate() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let (open, gate) = channel();
    *lab.fake.0.gate.lock().unwrap() = Some(gate);
    let first = gate_in_background(&lab);
    wait_until("the first gate's MFA", || {
        phases(&lab).iter().any(|p| p == "waitingForMfa")
    });
    let second = gate_in_background(&lab);
    wait_until("the join", || gate_connecting(&lab, 1));
    lab.core.vpn_cancel(&request_ids(&lab)[1]);
    let started = Instant::now();
    assert_eq!(second.join().unwrap().unwrap_err().code, "vpnCancelled");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the joined gate woke at once"
    );
    open.send(()).unwrap();
    assert!(first.join().unwrap().unwrap().is_some());
}

#[test]
fn joined_gate_takes_over_when_the_owner_cancels() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let (_open, gate) = channel::<()>();
    *lab.fake.0.gate.lock().unwrap() = Some(gate);
    let first = gate_in_background(&lab);
    wait_until("the first gate's MFA", || {
        phases(&lab).iter().any(|p| p == "waitingForMfa")
    });
    let second = gate_in_background(&lab);
    wait_until("the join", || gate_connecting(&lab, 1));
    lab.core.vpn_cancel(&request_ids(&lab)[0]);
    assert_eq!(first.join().unwrap().unwrap_err().code, "vpnCancelled");
    let hold = second.join().unwrap().unwrap();
    assert!(
        hold.is_some_and(|h| h.is_alive()),
        "the other connect ran its own up"
    );
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 2);
}

#[test]
fn gate_joins_a_profile_that_is_coming_up() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let (open, gate) = channel();
    *lab.fake.0.gate.lock().unwrap() = Some(gate);
    let first = gate_in_background(&lab);
    wait_until("the first gate's MFA", || {
        phases(&lab).iter().any(|p| p == "waitingForMfa")
    });
    lab.reachable.store(true, Ordering::SeqCst); // the tunnel already routes
    let second = gate_in_background(&lab);
    wait_until("the second gate", || request_ids(&lab).len() == 2);
    open.send(()).unwrap();
    let holds = [first.join().unwrap().unwrap(), second.join().unwrap().unwrap()];
    assert!(
        holds.iter().all(Option::is_some),
        "a ping through a VPN coming up must not skip the hold"
    );
    assert_eq!(lab.pings.lock().unwrap().len(), 1, "only the first gate pinged");
}

#[test]
fn a_busy_profile_is_retried() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    lab.fake.0.failures.lock().unwrap().extend([
        CommandError::new("vpnBusy", "x"),
        CommandError::new("vpnBusy", "x"),
    ]);
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_some());
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 3);
}

#[test]
fn a_profile_going_down_or_coming_up_skips_the_ping() {
    for state in ["disconnecting", "connecting"] {
        let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
        lab.reachable.store(true, Ordering::SeqCst);
        lab.fake.0.profiles.lock().unwrap()[0].state = state.into();
        assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_some(), "{state}");
        assert!(lab.pings.lock().unwrap().is_empty(), "{state}");
    }
}

#[test]
fn gate_skips_list_when_the_profile_is_held() {
    let lab = gate_lab(Some(office(VpnTestMode::Host, "")));
    let _hold = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    let lists = lab.fake.0.lists.load(Ordering::SeqCst);
    let _again = lab.core.vpn_gate(&lab.connection).unwrap().unwrap();
    assert_eq!(lab.fake.0.lists.load(Ordering::SeqCst), lists);
    assert_eq!(
        lab.progress.lock().unwrap().last().unwrap().profile_name,
        "Office"
    );
}

#[test]
fn forward_failure_releases_the_vpn() {
    let mut draft = ConnectionDraft::new(ConnectionType::Ssh);
    draft.username = "tester".into();
    let env = Env::new();
    let core = env.unlocked();
    let lab = gate_lab_on(env, core, Some(office(VpnTestMode::Host, "")), draft);
    // The local port is taken: the tunnel fails at once.
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let id = lab
        .core
        .port_forward_save(&rd_core::forward::PortForwardDraft {
            id: None,
            connection_id: lab.connection.clone(),
            name: String::new(),
            local_port: busy.local_addr().unwrap().port().to_string(),
            remote_host: "127.0.0.1".into(),
            remote_port: "5432".into(),
        })
        .unwrap();
    lab.core.port_forward_start(&id).unwrap();
    wait_until("the failed tunnel to release its VPN", || held(&lab).is_empty());
}

#[test]
fn an_empty_imported_profile_id_is_no_vpn() {
    let lab = gate_lab(Some(EntryVpnDraft {
        connection_id: "  ".into(),
        ..office(VpnTestMode::Host, "")
    }));
    assert!(lab.core.vpn_gate(&lab.connection).unwrap().is_none());
    assert_eq!(lab.fake.0.ups.load(Ordering::SeqCst), 0);
}
