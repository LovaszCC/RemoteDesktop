//! Connect with VPN (macOS): MacVPN connections brought up through its control API while sessions need them.
//!
//! MacVPN takes at most 8 clients at once (all apps together), so the app keeps **one socket (= one
//! MacVPN lease) per profile**, shared by every [`VpnHold`]. The first hold opens the socket and sends
//! `up`; dropping the last one closes it, and MacVPN brings the VPN down unless something else needs it or
//! the user had it up already. A watcher thread per socket keeps reading it (MacVPN drops clients that
//! don't) and notices a drop: the slot goes away, holds report `is_alive() == false`, and the next hold
//! opens a new lease. Holds that arrive while a profile is coming up join that `up`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

use serde::Serialize;

use super::error::CommandError;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One MacVPN connection Remote Desktop may use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProfileDto {
    /// The MacVPN connection id (case-sensitive).
    pub id: String,
    /// Its name in MacVPN.
    pub name: String,
    /// `openvpn`, `wireguard`, `system`…
    pub kind: String,
    /// `disconnected`, `connecting`, `connected`…
    pub state: String,
}

/// Where a VPN coming up is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnPhase {
    /// Coming up.
    Connecting,
    /// MacVPN shows a code or sign-in prompt.
    WaitingForMfa,
    /// MacVPN asks for a missing password.
    WaitingForCredentials,
    /// Up.
    Connected,
    /// Down: disconnected, failed or dropped.
    Down,
}

/// Cancels an `up` in progress: sets the flag and closes the socket that waits.
#[derive(Default)]
pub struct CancelFlag {
    cancelled: AtomicBool,
    closer: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl CancelFlag {
    /// Whether [`cancel`](Self::cancel) was called.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Cancels; closes the waiting socket if one is registered.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(close) = lock(&self.closer).as_ref() {
            close();
        }
    }

    /// Registers how to close the waiting socket (at once when already cancelled).
    pub fn set_closer(&self, close: Box<dyn Fn() + Send>) {
        if self.is_cancelled() {
            close();
        }
        *lock(&self.closer) = Some(close);
    }

    /// Forgets the socket once the `up` is over, so a late cancel can't close a lease.
    pub fn clear_closer(&self) {
        *lock(&self.closer) = None;
    }
}

/// An open MacVPN socket holding one lease. Dropping it (or calling its closer) ends the lease.
pub trait VpnLease: Send {
    /// The next state change of the leased connection; `None` once the socket closed.
    fn next_change(&mut self) -> Option<VpnPhase>;
    /// Closes the socket from another thread.
    fn closer(&self) -> Box<dyn Fn() + Send>;
}

/// MacVPN as the registry sees it (a fake in tests).
pub trait VpnBackend: Send + Sync {
    /// The connections Remote Desktop may use.
    ///
    /// # Errors
    /// `vpnNotRunning`, `vpnNotPaired`, `vpnLocked` or `vpnFailed`.
    fn list(&self) -> Result<Vec<VpnProfileDto>, CommandError>;

    /// Opens a socket and brings `profile_id` up on it, reporting the phases; blocks (MFA can take
    /// minutes) until it is up, failed or `cancel`led.
    ///
    /// # Errors
    /// The `vpn*` codes; `vpnCancelled` after a cancel.
    fn up(
        &self,
        profile_id: &str,
        on_phase: &mut dyn FnMut(VpnPhase),
        cancel: &CancelFlag,
    ) -> Result<Box<dyn VpnLease>, CommandError>;

    /// Pairs with MacVPN (it shows its window; up to about 2½ minutes) and stores the token.
    ///
    /// # Errors
    /// `vpnNotRunning`, `vpnPairingDenied`, `vpnBusy` or `vpnFailed`.
    fn pair(&self) -> Result<(), CommandError>;

    /// Forgets the token.
    fn unpair(&self);

    /// Whether a token is stored.
    fn is_paired(&self) -> bool;
}

/// Where a connect's VPN gate is, for the UI's progress dialog (event `vpn-progress`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProgressDto {
    /// One gate run; pass it to `vpn_cancel`.
    pub request_id: String,
    /// `testing`, `connecting`, `waitingForMfa`, `waitingForCredentials`, `done` or `failed`.
    pub phase: String,
    /// The MacVPN connection's name.
    pub profile_name: String,
    /// The entry being connected.
    pub entry_name: String,
    /// The address that did not answer the ping (why the VPN is needed); `None` when it was up already.
    pub unreachable: Option<String>,
}

/// The entries an external client holds a VPN for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalHoldDto {
    /// The entry.
    pub entry_id: String,
    /// Its name when it was launched.
    pub entry_name: String,
}

/// One VPN Remote Desktop holds up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldVpnDto {
    /// The MacVPN connection id.
    pub profile_id: String,
    /// Its name.
    pub profile_name: String,
    /// External launches holding it (released by "Release").
    pub external_entries: Vec<ExternalHoldDto>,
    /// Every entry holding it (the sidebar badge).
    pub entry_ids: Vec<String>,
    /// Open tabs, panes, files windows and forwards holding it.
    pub open_holders: u32,
}

/// The VPN tab of Settings and the sidebar badges (`vpn_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatusDto {
    /// Connect with VPN works here (macOS).
    pub supported: bool,
    /// A MacVPN token is stored.
    pub paired: bool,
    /// The VPNs held up, by name.
    pub held: Vec<HeldVpnDto>,
}

/// What the registry tells the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpnNotice {
    /// A gate's progress.
    Progress(VpnProgressDto),
    /// The held VPNs changed.
    Changed,
    /// A held VPN went down on its own (the user disconnected it in MacVPN, or it dropped).
    Dropped {
        /// Its name.
        profile_name: String,
    },
}

/// The reachability test (one ping; a fake in tests).
type Pinger = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Receives [`VpnNotice`]s (the Tauri side emits them).
pub type VpnNotifier = Arc<dyn Fn(VpnNotice) + Send + Sync>;

enum Slot {
    /// An `up` is running; joiners wait for it.
    Connecting {
        generation: u64,
    },
    Up(UpSlot),
}

struct UpSlot {
    generation: u64,
    profile_name: String,
    holders: usize,
    /// Holders per entry, for the sidebar badge.
    entries: HashMap<String, usize>,
    close: Box<dyn Fn() + Send>,
}

#[derive(Default)]
struct Registry {
    profiles: HashMap<String, Slot>,
    /// The error of a failed `up`, for the holds that joined it.
    failures: HashMap<u64, CommandError>,
    closed: bool,
}

/// The VPN state shared by the core and the holds.
pub struct VpnShared {
    backend: Mutex<Arc<dyn VpnBackend>>,
    /// Whether the gate runs (macOS, or a test backend).
    enabled: AtomicBool,
    pinger: Mutex<Pinger>,
    /// External launches' holds by entry: (entry name, hold).
    external: Mutex<HashMap<String, (String, VpnHold)>>,
    /// Running gates' cancel flags by request id.
    requests: Mutex<HashMap<String, Arc<CancelFlag>>>,
    registry: Mutex<Registry>,
    changed: Condvar,
    notifier: Mutex<Option<VpnNotifier>>,
    next_generation: AtomicU64,
}

impl VpnShared {
    /// A registry over `backend`.
    pub fn new(backend: Box<dyn VpnBackend>) -> Arc<Self> {
        Arc::new(Self {
            backend: Mutex::new(Arc::from(backend)),
            enabled: AtomicBool::new(cfg!(target_os = "macos")),
            pinger: Mutex::new(Arc::new(rd_platform::ping::reachable)),
            external: Mutex::default(),
            requests: Mutex::default(),
            registry: Mutex::default(),
            changed: Condvar::new(),
            notifier: Mutex::new(None),
            next_generation: AtomicU64::new(1),
        })
    }

    /// The backend (list, pair).
    pub fn backend(&self) -> Arc<dyn VpnBackend> {
        Arc::clone(&lock(&self.backend))
    }

    /// Replaces the backend and turns the gate on (tests).
    pub fn set_backend(&self, backend: Box<dyn VpnBackend>) {
        *lock(&self.backend) = Arc::from(backend);
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Installs (or removes) the notifier.
    pub fn set_notifier(&self, notifier: Option<VpnNotifier>) {
        *lock(&self.notifier) = notifier;
    }

    fn notify(&self, notice: VpnNotice) {
        let notifier = lock(&self.notifier).clone();
        if let Some(notifier) = notifier {
            notifier(notice);
        }
    }

    /// Whether `profile_id` is up for the holds right now.
    pub fn is_up(&self, profile_id: &str) -> bool {
        matches!(lock(&self.registry).profiles.get(profile_id), Some(Slot::Up(_)))
    }

    /// A hold on `profile_id` for `entry_id`: joins the profile's lease, or the `up` already running for
    /// it, or brings it up.
    ///
    /// # Errors
    /// The backend's `vpn*` errors (also for holds that joined a failed `up`); `vpnFailed` after
    /// [`shutdown`](Self::shutdown).
    pub fn acquire(
        self: &Arc<Self>,
        profile_id: &str,
        profile_name: &str,
        entry_id: &str,
        on_phase: &mut dyn FnMut(VpnPhase),
        cancel: &CancelFlag,
    ) -> Result<VpnHold, CommandError> {
        let generation = {
            let mut registry = lock(&self.registry);
            loop {
                if registry.closed {
                    return Err(CommandError::new("vpnFailed", "The app is quitting."));
                }
                match registry.profiles.get_mut(profile_id) {
                    Some(Slot::Up(up)) => {
                        up.holders += 1;
                        *up.entries.entry(entry_id.to_owned()).or_default() += 1;
                        let generation = up.generation;
                        drop(registry);
                        on_phase(VpnPhase::Connected);
                        return Ok(self.hold(profile_id, entry_id, generation));
                    }
                    Some(Slot::Connecting { generation }) => {
                        let joined = *generation;
                        on_phase(VpnPhase::Connecting);
                        registry = self
                            .changed
                            .wait_while(registry, |r| {
                                !cancel.is_cancelled()
                                    && matches!(r.profiles.get(profile_id),
                                        Some(Slot::Connecting { generation }) if *generation == joined)
                            })
                            .unwrap_or_else(PoisonError::into_inner);
                        if cancel.is_cancelled() {
                            return Err(cancelled());
                        }
                        if let Some(error) = registry.failures.get(&joined) {
                            return Err(error.clone());
                        }
                    }
                    None => {
                        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
                        registry
                            .profiles
                            .insert(profile_id.to_owned(), Slot::Connecting { generation });
                        break generation;
                    }
                }
            }
        };

        let result = self.backend().up(profile_id, on_phase, cancel);
        let mut registry = lock(&self.registry);
        let ours = matches!(registry.profiles.get(profile_id),
            Some(Slot::Connecting { generation: g }) if *g == generation);
        let outcome = match result {
            Ok(lease) if ours && !registry.closed => {
                registry.profiles.insert(
                    profile_id.to_owned(),
                    Slot::Up(UpSlot {
                        generation,
                        profile_name: profile_name.to_owned(),
                        holders: 1,
                        entries: HashMap::from([(entry_id.to_owned(), 1)]),
                        close: lease.closer(),
                    }),
                );
                self.watch(lease, profile_id, generation);
                Ok(self.hold(profile_id, entry_id, generation))
            }
            Ok(lease) => {
                lease.closer()();
                if ours {
                    registry.profiles.remove(profile_id);
                }
                Err(CommandError::new("vpnFailed", "The app is quitting."))
            }
            Err(error) => {
                if ours {
                    registry.profiles.remove(profile_id);
                }
                registry.failures.retain(|g, _| *g + 64 > generation);
                registry.failures.insert(generation, error.clone());
                Err(error)
            }
        };
        drop(registry);
        self.changed.notify_all();
        if outcome.is_ok() {
            self.notify(VpnNotice::Changed);
        }
        outcome
    }

    fn hold(self: &Arc<Self>, profile_id: &str, entry_id: &str, generation: u64) -> VpnHold {
        VpnHold {
            shared: Arc::downgrade(self),
            profile_id: profile_id.to_owned(),
            entry_id: entry_id.to_owned(),
            generation,
        }
    }

    /// Reads the lease until it ends; a drop (not our own close) removes the slot and tells the UI.
    fn watch(self: &Arc<Self>, mut lease: Box<dyn VpnLease>, profile_id: &str, generation: u64) {
        let shared = Arc::downgrade(self);
        let id = profile_id.to_owned();
        let spawned = std::thread::Builder::new()
            .name("vpn-lease".into())
            .spawn(move || {
                loop {
                    match lease.next_change() {
                        Some(VpnPhase::Down) | None => break,
                        Some(_) => {}
                    }
                }
                if let Some(shared) = shared.upgrade() {
                    shared.dropped(&id, generation);
                }
            });
        if spawned.is_err() {
            self.dropped(profile_id, generation);
        }
    }

    fn dropped(&self, profile_id: &str, generation: u64) {
        let mut registry = lock(&self.registry);
        let Some(Slot::Up(up)) = registry.profiles.get(profile_id) else {
            return;
        };
        if up.generation != generation {
            return;
        }
        let Some(Slot::Up(up)) = registry.profiles.remove(profile_id) else {
            return;
        };
        drop(registry);
        (up.close)();
        self.notify(VpnNotice::Dropped {
            profile_name: up.profile_name,
        });
        self.notify(VpnNotice::Changed);
    }

    fn release(&self, profile_id: &str, entry_id: &str, generation: u64) {
        let mut registry = lock(&self.registry);
        let Some(Slot::Up(up)) = registry.profiles.get_mut(profile_id) else {
            return;
        };
        if up.generation != generation {
            return;
        }
        up.holders = up.holders.saturating_sub(1);
        if let Some(count) = up.entries.get_mut(entry_id) {
            *count -= 1;
            if *count == 0 {
                up.entries.remove(entry_id);
            }
        }
        let last = up.holders == 0;
        let removed = if last {
            registry.profiles.remove(profile_id)
        } else {
            None
        };
        drop(registry);
        if let Some(Slot::Up(up)) = removed {
            (up.close)();
        }
        self.notify(VpnNotice::Changed);
    }

    /// The held profiles: id, name, holder count and the entries holding them.
    pub fn held(&self) -> Vec<(String, String, usize, Vec<String>)> {
        let registry = lock(&self.registry);
        let mut held: Vec<_> = registry
            .profiles
            .iter()
            .filter_map(|(id, slot)| match slot {
                Slot::Up(up) => {
                    let mut entries: Vec<String> = up.entries.keys().cloned().collect();
                    entries.sort();
                    Some((id.clone(), up.profile_name.clone(), up.holders, entries))
                }
                Slot::Connecting { .. } => None,
            })
            .collect();
        held.sort_by(|a, b| a.1.cmp(&b.1));
        held
    }

    /// On quit: closes every lease; later acquires fail.
    pub fn shutdown(&self) {
        let external = std::mem::take(&mut *lock(&self.external));
        for flag in lock(&self.requests).values() {
            flag.cancel();
        }
        let slots: Vec<Slot> = {
            let mut registry = lock(&self.registry);
            registry.closed = true;
            registry.profiles.drain().map(|(_, slot)| slot).collect()
        };
        self.changed.notify_all();
        for slot in slots {
            if let Slot::Up(up) = slot {
                (up.close)();
            }
        }
        drop(external);
    }
}

impl super::AppCore {
    /// Replaces MacVPN (tests); the gate then runs on any OS.
    pub fn set_vpn_backend(&self, backend: Box<dyn VpnBackend>) {
        self.vpn.set_backend(backend);
    }

    /// Replaces the ping (tests).
    pub fn set_vpn_pinger(&self, pinger: Box<dyn Fn(&str) -> bool + Send + Sync>) {
        *lock(&self.vpn.pinger) = Arc::from(pinger);
    }

    /// Installs the notifier of VPN progress, changes and drops. Called once in setup.
    pub fn set_vpn_hooks(&self, notifier: VpnNotifier) {
        self.vpn.set_notifier(Some(notifier));
    }

    /// The VPN gate, run before every connection to a library connection: `None` when its entry has no
    /// VPN profile, the gate is off (not macOS), or the test address answered; otherwise a hold on the
    /// profile's VPN, brought up through MacVPN when needed. A profile that is up already (ours or the
    /// user's) is held without a ping, so a ping that only answers through the VPN never skips the hold.
    ///
    /// # Errors
    /// The `vpn*` codes when the VPN is needed and can't come up (the connect stops).
    pub fn vpn_gate(&self, connection_id: &str) -> Result<Option<VpnHold>, CommandError> {
        if !self.vpn.enabled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let Some((entry_id, entry_name, host, vpn)) = self.entry_vpn(connection_id)? else {
            return Ok(None);
        };
        let backend = self.vpn.backend();
        let profile = backend
            .list()
            .ok()
            .and_then(|list| list.into_iter().find(|p| p.id == vpn.connection_id));
        let profile_name = profile
            .as_ref()
            .map_or_else(|| "VPN".to_owned(), |p| p.name.clone());
        let request = Arc::new(CancelFlag::default());
        let request_id = rd_core::new_id();
        lock(&self.vpn.requests).insert(request_id.clone(), Arc::clone(&request));
        let progress = |phase: &str, unreachable: Option<&str>| {
            self.vpn.notify(VpnNotice::Progress(VpnProgressDto {
                request_id: request_id.clone(),
                phase: phase.to_owned(),
                profile_name: profile_name.clone(),
                entry_name: entry_name.clone(),
                unreachable: unreachable.map(str::to_owned),
            }));
        };
        progress("testing", None);
        let already_up =
            self.vpn.is_up(&vpn.connection_id) || profile.as_ref().is_some_and(|p| p.state == "connected");
        let target = match vpn.test_mode {
            rd_core::VpnTestMode::Host => host,
            rd_core::VpnTestMode::Address => vpn.test_address.clone(),
        };
        let result = if !already_up && (lock(&self.vpn.pinger).clone())(&target) {
            Ok(None)
        } else {
            let unreachable = (!already_up).then_some(target.as_str());
            if unreachable.is_some() {
                progress("connecting", unreachable);
            }
            self.vpn
                .acquire(
                    &vpn.connection_id,
                    &profile_name,
                    &entry_id,
                    &mut |phase| match phase {
                        VpnPhase::Connecting => progress("connecting", unreachable),
                        VpnPhase::WaitingForMfa => progress("waitingForMfa", unreachable),
                        VpnPhase::WaitingForCredentials => progress("waitingForCredentials", unreachable),
                        VpnPhase::Connected | VpnPhase::Down => {}
                    },
                    &request,
                )
                .map(Some)
        };
        lock(&self.vpn.requests).remove(&request_id);
        progress(if result.is_ok() { "done" } else { "failed" }, None);
        result
    }

    /// The VPN setting of `connection_id`'s entry: (entry id, entry name, host, setting).
    fn entry_vpn(
        &self,
        connection_id: &str,
    ) -> Result<Option<(String, String, String, rd_core::EntryVpn)>, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::unlocked(&inner)?;
        let library = db.library()?;
        let Some(connection) = library.connections.iter().find(|c| c.id == connection_id) else {
            return Ok(None);
        };
        Ok(library.entry(&connection.entry_id).and_then(|entry| {
            let vpn = entry.vpn.clone()?;
            Some((entry.id.clone(), entry.name.clone(), entry.host.clone(), vpn))
        }))
    }

    /// Cancels a running gate (its `vpn-progress` request id); unknown ids are ignored.
    pub fn vpn_cancel(&self, request_id: &str) {
        let flag = lock(&self.vpn.requests).get(request_id).cloned();
        if let Some(flag) = flag {
            flag.cancel();
        }
    }

    /// Keeps `hold` for an external launch of `entry_id` until it is released or the app quits; one per
    /// entry (a newer one replaces the older).
    pub fn hold_external(&self, entry_id: &str, entry_name: &str, hold: VpnHold) {
        let old = lock(&self.vpn.external).insert(entry_id.to_owned(), (entry_name.to_owned(), hold));
        drop(old);
        self.vpn.notify(VpnNotice::Changed);
    }

    /// Keeps the gate's hold of an external launch (when the gate took one), named after its entry.
    pub(super) fn keep_external(&self, hold: Option<VpnHold>) {
        let Some(hold) = hold else { return };
        let name = {
            let inner = self.inner();
            Self::unlocked(&inner)
                .ok()
                .and_then(|(db, _)| db.entry(hold.entry_id()).ok().flatten())
                .map_or_else(|| hold.entry_id().to_owned(), |entry| entry.name)
        };
        let entry_id = hold.entry_id().to_owned();
        self.hold_external(&entry_id, &name, hold);
    }

    /// "Release VPN" on an entry: drops its external launch's hold.
    pub fn vpn_release_entry(&self, entry_id: &str) {
        let old = lock(&self.vpn.external).remove(entry_id);
        if old.is_some() {
            drop(old);
            self.vpn.notify(VpnNotice::Changed);
        }
    }

    /// "Release" in Settings: drops every external launch's hold on `profile_id`; open tabs keep theirs.
    pub fn vpn_release_profile(&self, profile_id: &str) {
        let released: Vec<(String, VpnHold)> = {
            let mut external = lock(&self.vpn.external);
            let entries: Vec<String> = external
                .iter()
                .filter(|(_, (_, hold))| hold.profile_id() == profile_id)
                .map(|(entry, _)| entry.clone())
                .collect();
            entries.iter().filter_map(|e| external.remove(e)).collect()
        };
        if !released.is_empty() {
            drop(released);
            self.vpn.notify(VpnNotice::Changed);
        }
    }

    /// Pairing state and the held VPNs.
    pub fn vpn_status(&self) -> VpnStatusDto {
        let supported = self.vpn.enabled.load(Ordering::SeqCst);
        let external: Vec<(String, String, String)> = lock(&self.vpn.external)
            .iter()
            .filter(|(_, (_, hold))| hold.is_alive())
            .map(|(entry, (name, hold))| (hold.profile_id().to_owned(), entry.clone(), name.clone()))
            .collect();
        let held = self
            .vpn
            .held()
            .into_iter()
            .map(|(profile_id, profile_name, holders, entry_ids)| {
                let mut external_entries: Vec<ExternalHoldDto> = external
                    .iter()
                    .filter(|(p, _, _)| *p == profile_id)
                    .map(|(_, entry_id, entry_name)| ExternalHoldDto {
                        entry_id: entry_id.clone(),
                        entry_name: entry_name.clone(),
                    })
                    .collect();
                external_entries.sort_by(|a, b| a.entry_name.cmp(&b.entry_name));
                let open = holders.saturating_sub(external_entries.len());
                HeldVpnDto {
                    profile_id,
                    profile_name,
                    external_entries,
                    entry_ids,
                    open_holders: u32::try_from(open).unwrap_or(u32::MAX),
                }
            })
            .collect();
        VpnStatusDto {
            supported,
            paired: supported && self.vpn.backend().is_paired(),
            held,
        }
    }

    /// The MacVPN connections Remote Desktop may use (the entry editor's picker).
    ///
    /// # Errors
    /// `vpnNotRunning`, `vpnNotPaired`, `vpnLocked` or `vpnFailed`.
    pub fn vpn_profiles(&self) -> Result<Vec<VpnProfileDto>, CommandError> {
        self.vpn.backend().list()
    }

    /// Pairs with MacVPN (blocks until the user answers MacVPN's window).
    ///
    /// # Errors
    /// `vpnNotRunning`, `vpnPairingDenied`, `vpnBusy` or `vpnFailed`.
    pub fn vpn_pair(&self) -> Result<(), CommandError> {
        let result = self.vpn.backend().pair();
        self.vpn.notify(VpnNotice::Changed);
        result
    }

    /// Forgets the MacVPN token (MacVPN keeps the pairing until revoked there).
    pub fn vpn_unpair(&self) {
        self.vpn.backend().unpair();
        self.vpn.notify(VpnNotice::Changed);
    }
}

fn cancelled() -> CommandError {
    CommandError::new("vpnCancelled", "The VPN connection was cancelled.")
}

/// Keeps a profile's VPN up while it lives. Dropping the last hold of a profile ends its lease.
pub struct VpnHold {
    shared: Weak<VpnShared>,
    profile_id: String,
    entry_id: String,
    generation: u64,
}

impl std::fmt::Debug for VpnHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VpnHold")
            .field("profile_id", &self.profile_id)
            .field("entry_id", &self.entry_id)
            .finish_non_exhaustive()
    }
}

impl VpnHold {
    /// The MacVPN connection id.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// The entry it was taken for.
    #[must_use]
    pub fn entry_id(&self) -> &str {
        &self.entry_id
    }

    /// Whether the lease it belongs to is still up (not dropped, not shut down).
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.shared.upgrade().is_some_and(|shared| {
            matches!(lock(&shared.registry).profiles.get(&self.profile_id),
                Some(Slot::Up(up)) if up.generation == self.generation)
        })
    }
}

impl Drop for VpnHold {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.release(&self.profile_id, &self.entry_id, self.generation);
        }
    }
}

/// The backend where the control API is not used (off macOS): never paired, nothing to list.
#[cfg_attr(target_os = "macos", allow(dead_code, reason = "the backend off macOS"))]
pub struct UnsupportedBackend;

impl VpnBackend for UnsupportedBackend {
    fn list(&self) -> Result<Vec<VpnProfileDto>, CommandError> {
        Err(unsupported())
    }

    fn up(
        &self,
        _profile_id: &str,
        _on_phase: &mut dyn FnMut(VpnPhase),
        _cancel: &CancelFlag,
    ) -> Result<Box<dyn VpnLease>, CommandError> {
        Err(unsupported())
    }

    fn pair(&self) -> Result<(), CommandError> {
        Err(unsupported())
    }

    fn unpair(&self) {}

    fn is_paired(&self) -> bool {
        false
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code, reason = "the backend off macOS"))]
fn unsupported() -> CommandError {
    CommandError::new("vpnNotRunning", "Connect with VPN needs MacVPN on macOS.")
}

/// The backend for this system: MacVPN's control socket on macOS, none elsewhere.
pub fn system_backend() -> Box<dyn VpnBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacVpnBackend::new(Box::new(rd_store::KeyringVpnTokens::new())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(UnsupportedBackend)
    }
}

/// The phase a MacVPN state means (unknown states are in progress).
#[cfg(unix)]
pub fn phase_of(state: &rd_platform::macvpn::State) -> VpnPhase {
    use rd_platform::macvpn::State;
    match state {
        State::Connected => VpnPhase::Connected,
        State::WaitingForMfa => VpnPhase::WaitingForMfa,
        State::WaitingForCredentials => VpnPhase::WaitingForCredentials,
        State::Disconnected | State::Failed => VpnPhase::Down,
        State::Connecting | State::Disconnecting | State::Other(_) => VpnPhase::Connecting,
    }
}

/// The app's error for a MacVPN client error. `locked` arrives as `failed` with the wire code kept.
#[cfg(unix)]
pub fn macvpn_error(error: &rd_platform::macvpn::Error) -> CommandError {
    use rd_platform::macvpn::{Error, ErrorCode};
    match error {
        Error::Io(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            CommandError::new(
                "vpnNotRunning",
                "MacVPN is not running, or it doesn't let other apps control VPNs.",
            )
        }
        Error::Api { wire, .. } if wire == "locked" => {
            CommandError::new("vpnLocked", "MacVPN is locked. Unlock it, then connect again.")
        }
        Error::Api { code, message, .. } => {
            let (code, text) = match code {
                ErrorCode::Unauthorized => ("vpnNotPaired", "Remote Desktop is not paired with MacVPN."),
                ErrorCode::NotAllowed => ("vpnNotAllowed", "MacVPN doesn't let Remote Desktop use this VPN."),
                ErrorCode::UnknownConnection => ("vpnUnknownProfile", "This VPN no longer exists in MacVPN."),
                ErrorCode::SetupRequired => ("vpnSetupRequired", "MacVPN's setup is incomplete."),
                ErrorCode::MfaCancelled | ErrorCode::CredentialsCancelled => {
                    ("vpnCancelled", "The VPN connection was cancelled.")
                }
                ErrorCode::MfaTimeout => ("vpnMfaTimeout", "MacVPN's sign-in prompt timed out."),
                ErrorCode::Busy => ("vpnBusy", "MacVPN is busy with this VPN. Try again."),
                ErrorCode::Denied => ("vpnPairingDenied", "MacVPN did not allow the pairing."),
                ErrorCode::Disabled | ErrorCode::AppQuitting => (
                    "vpnNotRunning",
                    "MacVPN is not running, or it doesn't let other apps control VPNs.",
                ),
                _ => ("vpnFailed", "The VPN could not connect."),
            };
            let error = CommandError::new(code, text);
            match message {
                Some(message) => error.with_detail(message.clone()),
                None => error,
            }
        }
        Error::Io(e) => {
            CommandError::new("vpnFailed", "The VPN could not connect.").with_detail(e.to_string())
        }
        Error::Closed => CommandError::new("vpnFailed", "MacVPN closed the connection."),
        Error::Protocol(text) => {
            CommandError::new("vpnFailed", "MacVPN sent an unexpected answer.").with_detail(text.clone())
        }
    }
}

/// MacVPN's control socket, with the token in the Keychain.
#[cfg(target_os = "macos")]
pub struct MacVpnBackend {
    tokens: Box<dyn rd_store::VpnTokens>,
}

#[cfg(target_os = "macos")]
impl MacVpnBackend {
    /// A backend keeping its token in `tokens`.
    pub fn new(tokens: Box<dyn rd_store::VpnTokens>) -> Self {
        Self { tokens }
    }

    fn socket() -> Result<std::path::PathBuf, CommandError> {
        rd_platform::macvpn::default_socket_path().map_err(|e| macvpn_error(&e.into()))
    }

    /// Maps an error; an `unauthorized` forgets the token (the pairing is gone).
    fn error(&self, error: &rd_platform::macvpn::Error) -> CommandError {
        let mapped = macvpn_error(error);
        if mapped.code == "vpnNotPaired" {
            self.tokens.delete();
        }
        mapped
    }

    fn open(&self) -> Result<rd_platform::macvpn::Client, CommandError> {
        let token = self
            .tokens
            .load()
            .ok_or_else(|| CommandError::new("vpnNotPaired", "Remote Desktop is not paired with MacVPN."))?;
        rd_platform::macvpn::Client::open(&Self::socket()?, &token).map_err(|e| self.error(&e))
    }
}

#[cfg(target_os = "macos")]
impl VpnBackend for MacVpnBackend {
    fn list(&self) -> Result<Vec<VpnProfileDto>, CommandError> {
        let connections = self.open()?.list().map_err(|e| self.error(&e))?;
        Ok(connections
            .into_iter()
            .map(|c| VpnProfileDto {
                id: c.id,
                name: c.name,
                kind: c.kind,
                state: c.state.as_str().to_owned(),
            })
            .collect())
    }

    fn up(
        &self,
        profile_id: &str,
        on_phase: &mut dyn FnMut(VpnPhase),
        cancel: &CancelFlag,
    ) -> Result<Box<dyn VpnLease>, CommandError> {
        let mut client = self.open()?;
        let closer = client
            .closer()
            .map_err(|e| self.error(&rd_platform::macvpn::Error::Io(e)))?;
        cancel.set_closer(Box::new(move || closer.close()));
        let result = client.up(profile_id, |event| on_phase(phase_of(&event.state)));
        cancel.clear_closer();
        match result {
            _ if cancel.is_cancelled() => Err(cancelled()),
            Ok(()) => Ok(Box::new(MacVpnLease {
                client,
                profile: profile_id.to_owned(),
            })),
            Err(e) => Err(self.error(&e)),
        }
    }

    fn pair(&self) -> Result<(), CommandError> {
        let pairing = rd_platform::macvpn::Client::pair(&Self::socket()?, "Remote Desktop")
            .map_err(|e| macvpn_error(&e))?;
        self.tokens.save(&pairing.token).map_err(|e| {
            CommandError::new("vpnFailed", "The MacVPN token could not be saved.").with_detail(e.to_string())
        })
    }

    fn unpair(&self) {
        self.tokens.delete();
    }

    fn is_paired(&self) -> bool {
        self.tokens.load().is_some()
    }
}

#[cfg(target_os = "macos")]
struct MacVpnLease {
    client: rd_platform::macvpn::Client,
    profile: String,
}

#[cfg(target_os = "macos")]
impl VpnLease for MacVpnLease {
    fn next_change(&mut self) -> Option<VpnPhase> {
        loop {
            match self.client.next_event() {
                Ok(Some(event)) if event.connection == self.profile => return Some(phase_of(&event.state)),
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return None,
            }
        }
    }

    fn closer(&self) -> Box<dyn Fn() + Send> {
        match self.client.closer() {
            Ok(closer) => Box::new(move || closer.close()),
            Err(_) => Box::new(|| {}),
        }
    }
}
