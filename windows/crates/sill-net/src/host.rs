//! The host's shared state: identity, paired devices, the pairing window, session counts, and the
//! kind 19 judge (upstream `RemoteAccess.judge`).

use crate::cert::{self, HostCertificate};
use crate::stats::Stats;
use crate::video::{Clip, Hub};
use sill_pairing::{address, base64url, code, link::PairLink, proof, safe_text, spki, SESSION_ALPN};
use sill_policy::door::{self, AtReady, Door, Trust};
use sill_policy::pairing_window::{CloseReason, PairingWindow, Verdict, LIFETIME};
use sill_store::{Identity, PairedDevice, Store};
use sill_wire::{PairRequest, PairResult, SillVersion};
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

/// How long an address that proved a paired key counts as known (`DoorPolicy.knownFor`).
const KNOWN_FOR: Duration = Duration::from_secs(86_400);

#[derive(Clone, Debug)]
pub struct Config {
    /// The host's name: Bonjour instance, window list `macName`, pairing results.
    pub name: String,
    pub remote_port: u16,
    /// 0: a free port, chosen at launch.
    pub home_port: u16,
    /// Empty: loopback, private LAN and Tailscale addresses (rescanned every 10 s).
    pub bind: Vec<IpAddr>,
    pub allow_internet: bool,
    pub bonjour: bool,
    pub device_floor: SillVersion,
    /// A remote client that sends nothing for this long is dropped (upstream 8 s).
    pub remote_silence: Duration,
    /// Drain time after a goodbye (upstream `DeviceGate.closeWait`, 1 s).
    pub close_wait: Duration,
}

impl Config {
    pub fn new(name: impl Into<String>) -> Config {
        Config {
            name: name.into(),
            remote_port: 7455,
            home_port: 0,
            bind: Vec::new(),
            allow_internet: false,
            bonjour: true,
            device_floor: sill_policy::device_gate::floor(),
            remote_silence: Duration::from_secs(8),
            close_wait: Duration::from_secs(1),
        }
    }
}

/// What `--pair` shows.
#[derive(Clone, Debug)]
pub struct PairingOffer {
    pub code: String,
    pub link: String,
    pub expires_in: f64,
}

struct State {
    paired: Vec<PairedDevice>,
    paired_fps: HashSet<Vec<u8>>,
    require_pairing: bool,
    window: PairingWindow,
    remote_sessions: usize,
    home_sessions: usize,
    known: HashMap<IpAddr, Instant>,
    bound: Vec<IpAddr>,
    paired_modified: Option<std::time::SystemTime>,
}

pub struct Host {
    pub config: Config,
    pub store: Store,
    pub identity: Identity,
    pub fingerprint: [u8; 32],
    pub mac_id: String,
    /// Per launch (`WindowList.launchID`).
    pub launch_id: String,
    pub cert: HostCertificate,
    pub stats: Arc<Stats>,
    pub hub: Arc<Hub>,
    started: Instant,
    state: Mutex<State>,
    pub shutdown: watch::Sender<bool>,
    /// Fingerprints taken off the list: their sessions get goodbye `removed`.
    pub removed: broadcast::Sender<Vec<u8>>,
}

/// Counts a session for as long as it lives.
pub struct SessionGuard {
    host: Arc<Host>,
    door: Door,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        let mut s = self.host.state.lock().unwrap();
        match self.door {
            Door::Remote => s.remote_sessions = s.remote_sessions.saturating_sub(1),
            Door::Home => s.home_sessions = s.home_sessions.saturating_sub(1),
        }
    }
}

fn uuid_like(b: [u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

impl Host {
    pub fn new(config: Config, store: Store, clip: Clip) -> io::Result<Arc<Host>> {
        let identity = store.identity()?;
        let paired = store.paired()?;
        let settings = store.settings()?;
        let cert = cert::make(&identity, sill_store::random).map_err(io::Error::other)?;
        let fingerprint = identity.fingerprint();
        let mut launch = [0u8; 16];
        if !sill_store::random(&mut launch) {
            return Err(io::Error::other("the OS random generator failed"));
        }
        let stats = Arc::new(Stats::default());
        let paired_fps = paired.iter().filter_map(|d| d.fingerprint_bytes()).collect();
        Ok(Arc::new(Host {
            mac_id: spki::mac_id(&fingerprint),
            fingerprint,
            identity,
            launch_id: uuid_like(launch),
            cert,
            hub: Arc::new(Hub::new(clip, stats.clone())),
            stats,
            started: Instant::now(),
            state: Mutex::new(State {
                paired,
                paired_fps,
                require_pairing: settings.require_pairing,
                window: PairingWindow::new(),
                remote_sessions: 0,
                home_sessions: 0,
                known: HashMap::new(),
                bound: Vec::new(),
                paired_modified: store.paired_modified(),
            }),
            shutdown: watch::channel(false).0,
            removed: broadcast::channel(16).0,
            config,
            store,
        }))
    }

    /// Monotonic seconds since launch (the pairing window's clock).
    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    pub fn require_pairing(&self) -> bool {
        self.state.lock().unwrap().require_pairing
    }

    pub fn set_require_pairing(&self, on: bool) -> io::Result<()> {
        self.store.save_settings(&sill_store::Settings { require_pairing: on })?;
        self.state.lock().unwrap().require_pairing = on;
        Ok(())
    }

    pub fn paired(&self) -> Vec<PairedDevice> {
        self.state.lock().unwrap().paired.clone()
    }

    pub fn set_bound(&self, bound: Vec<IpAddr>) {
        self.state.lock().unwrap().bound = bound;
    }

    pub fn sessions(&self) -> (usize, usize) {
        let s = self.state.lock().unwrap();
        (s.home_sessions, s.remote_sessions)
    }

    pub fn is_known(&self, ip: IpAddr) -> bool {
        self.state.lock().unwrap().known.get(&ip).is_some_and(|t| t.elapsed() < KNOWN_FOR)
    }

    fn trust_locked(&self, s: &mut State, fp: Option<&[u8]>) -> Trust {
        s.window.expire_if_due(self.now());
        Trust {
            has_key: fp.is_some(),
            paired: fp.is_some_and(|f| s.paired_fps.contains(f)),
            require_pairing: s.require_pairing,
            // The remote door runs for as long as the host does.
            remote_access: true,
            internet_access: self.config.allow_internet,
            pairing_open: s.window.is_open(),
            remote_pairing_open: s.window.current().is_some_and(|w| w.for_remote),
        }
    }

    /// The handshake's verify decision for this key (`None`: not a P-256 key).
    pub fn trusts(&self, door: Door, alpn: Option<&str>, fp: Option<&[u8]>) -> bool {
        let mut s = self.state.lock().unwrap();
        let t = self.trust_locked(&mut s, fp);
        door::trusts(door, alpn, &t)
    }

    /// `.ready`'s decision, judged again now; a session is counted at once (the 9th remote one
    /// gets `busy`).
    pub fn admit(self: &Arc<Self>, door: Door, alpn: Option<&str>, fp: Option<&[u8]>, peer: IpAddr) -> (AtReady, Option<SessionGuard>) {
        let mut s = self.state.lock().unwrap();
        let t = self.trust_locked(&mut s, fp);
        let verdict = door::at_ready(door, alpn, &t, s.remote_sessions);
        if verdict != AtReady::Session {
            return (verdict, None);
        }
        match door {
            Door::Remote => s.remote_sessions += 1,
            Door::Home => s.home_sessions += 1,
        }
        if t.paired && alpn == Some(SESSION_ALPN) {
            s.known.insert(peer, Instant::now());
        }
        (verdict, Some(SessionGuard { host: self.clone(), door }))
    }

    /// The name a device has on the list, if it is paired.
    pub fn paired_name(&self, fp: &[u8]) -> Option<String> {
        let b = base64url::encode(fp);
        self.state.lock().unwrap().paired.iter().find(|d| d.fingerprint == b).map(|d| d.name.clone())
    }

    /// Opens a 300 s window (a fresh code and QR secret) that both doors take, as the host's own
    /// user opening it does upstream. K for the code is derived off the network threads.
    pub fn open_pairing(self: &Arc<Self>) -> io::Result<PairingOffer> {
        let code = code::generate(sill_store::random).ok_or_else(|| io::Error::other("random generator failed"))?;
        let mut secret = [0u8; 16];
        if !sill_store::random(&mut secret) {
            return Err(io::Error::other("random generator failed"));
        }
        let addresses: Vec<_> = {
            let mut s = self.state.lock().unwrap();
            s.window.open(&secret, &code, self.now(), LIFETIME, None, None, true);
            crate::bind::reachable_addresses(&s.bound, sill_pairing::link::MAX_ADDRESSES)
                .iter()
                .filter_map(|ip| address::parse(&ip.to_string()).ok())
                .collect()
        };
        let host = self.clone();
        let c = code.clone();
        std::thread::spawn(move || {
            if let Some(k) = proof::code_key(&c, &host.fingerprint) {
                host.state.lock().unwrap().window.set_code_key(k, &c);
            }
        });
        let link = PairLink::new(&self.fingerprint, &secret, &self.config.name, self.config.remote_port, &addresses);
        Ok(PairingOffer { code, link: link.url(), expires_in: LIFETIME })
    }

    pub fn pairing_open(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        s.window.expire_if_due(self.now());
        s.window.is_open()
    }

    /// Kind 19 → kind 20 (upstream `RemoteAccess.judge`). `fp_device` comes from the TLS session.
    pub fn judge(&self, door: Door, req: &PairRequest, fp_device: &[u8], source: &str) -> PairResult {
        let name = safe_text::label64(&req.name);
        let shown = if name.is_empty() { source.to_string() } else { format!("{name} at {source}") };
        let mut s = self.state.lock().unwrap();
        match door::pairing(door, &req.method, req.v) {
            door::Pairing::Closed => return PairResult::refusal(PairResult::CLOSED),
            // No window here is ever opened by a device's ask, so a Cancel has nothing to withdraw.
            door::Pairing::Withdraw => return PairResult::refusal(PairResult::CLOSED),
            door::Pairing::Ask => {
                // This host has no window to show a code in when nobody opened one: an ask never
                // opens a window (upstream's console host prints one; here it would be shown only in
                // a console the device's user cannot see). A window already open is "shown".
                return if s.window.is_open() {
                    info!("Pairing: {shown} asked to pair; the code is in this console.");
                    PairResult::refusal(PairResult::SHOWN)
                } else {
                    info!("Pairing: {shown} asked to pair; no window is open (run sill-host --pair).");
                    PairResult::refusal(PairResult::OPEN_ON_MAC)
                };
            }
            door::Pairing::Window => {}
        }
        if door == Door::Remote && s.window.current().is_some_and(|w| !w.for_remote) {
            return PairResult::refusal(PairResult::CLOSED);
        }
        let was_open = s.window.is_open();
        let proof_bytes = base64url::decode(&req.proof).unwrap_or_default();
        let now = self.now();
        match s.window.try_proof(&req.method, &proof_bytes, fp_device, &self.fingerprint, source, now) {
            Verdict::Accept { proof_m } => {
                let model = req.model.as_deref().map(safe_text::label64).filter(|m| !m.is_empty());
                let device = PairedDevice {
                    fingerprint: base64url::encode(fp_device),
                    name: name.clone(),
                    model,
                    paired_at: crate::wall_clock(),
                    method: req.method.clone(),
                };
                // On the list before kind 20 goes out, so the session dial that follows is admitted.
                s.paired.retain(|d| d.fingerprint != device.fingerprint);
                s.paired.push(device);
                s.paired_fps.insert(fp_device.to_vec());
                if let Err(e) = self.store.save_paired(&s.paired) {
                    error!("Pairing: could not save the paired devices ({e}); {name} is paired until the host quits.");
                }
                s.paired_modified = self.store.paired_modified();
                info!(
                    "Paired {} (key {}…) from {source}, with {}.",
                    if name.is_empty() { "a device" } else { &name },
                    spki::short_name(fp_device),
                    if req.method == PairRequest::QR { "the QR code" } else { "the code" }
                );
                PairResult {
                    ok: true,
                    proof: Some(base64url::encode(&proof_m)),
                    mac_id: Some(self.mac_id.clone()),
                    name: Some(self.config.name.clone()),
                    recognition_key: Some(base64url::encode(&self.identity.recognition_key)),
                    ..PairResult::default()
                }
            }
            Verdict::Reject { tries_left } => {
                warn!("Pairing: a wrong code from {shown} ({tries_left} tr{} left).", if tries_left == 1 { "y" } else { "ies" });
                PairResult { tries_left: Some(i64::from(tries_left)), ..PairResult::refusal(PairResult::CODE) }
            }
            Verdict::Busy { retry_after } => {
                PairResult { retry_after: Some((retry_after * 10.0).ceil() / 10.0), ..PairResult::refusal(PairResult::BUSY) }
            }
            Verdict::Closed(reason) => {
                if was_open && reason == CloseReason::Stopped {
                    warn!("Pairing stopped after {} wrong codes.", sill_policy::pairing_window::MAX_FAILURES);
                }
                PairResult::refusal(PairingWindow::closed_reason(reason, was_open))
            }
        }
    }

    /// Removes devices by name or fingerprint (prefix of 6+); returns them.
    pub fn unpair(&self, which: &str) -> io::Result<Vec<PairedDevice>> {
        let mut s = self.state.lock().unwrap();
        let mut next = s.paired.clone();
        let removed = sill_store::remove_matching(&mut next, which);
        if !removed.is_empty() {
            self.store.save_paired(&next)?;
            s.paired_modified = self.store.paired_modified();
            self.replace_paired(&mut s, next);
        }
        Ok(removed)
    }

    fn replace_paired(&self, s: &mut State, next: Vec<PairedDevice>) {
        let fps: HashSet<Vec<u8>> = next.iter().filter_map(|d| d.fingerprint_bytes()).collect();
        for gone in s.paired_fps.difference(&fps) {
            let _ = self.removed.send(gone.clone());
        }
        s.paired_fps = fps;
        s.paired = next;
    }

    /// Reads the paired devices again when another process (`sill-host --unpair`) changed them.
    pub fn reload_paired_if_changed(&self) {
        let m = self.store.paired_modified();
        let mut s = self.state.lock().unwrap();
        if m == s.paired_modified {
            return;
        }
        s.paired_modified = m;
        match self.store.paired() {
            Ok(next) => {
                info!("Paired devices changed on disk: {} now.", next.len());
                self.replace_paired(&mut s, next);
            }
            Err(e) => error!("Could not read the paired devices again: {e}"),
        }
    }

    /// Asks every task to wind down (goodbye `quit` to every session).
    pub fn quit(&self) {
        let _ = self.shutdown.send(true);
    }
}
