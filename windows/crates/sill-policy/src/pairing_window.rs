//! One pairing window's rules (upstream `PairingWindow.swift`): open 300 s, at most five wrong
//! proofs, spaced 1/2/4/8 s apart, one try per source per 5 s. Pure: `now` is passed in (use a
//! monotonic clock, in seconds).

use sill_pairing::proof;
use sill_wire::{PairRequest, PairResult};
use std::collections::HashMap;

pub const LIFETIME: f64 = 300.0;
pub const MAX_FAILURES: u32 = 5;
/// The same source may try once per this many seconds.
pub const SOURCE_SPACING: f64 = 5.0;
/// After the 1st, 2nd, 3rd and 4th wrong proof, the next is taken this much later.
pub const SPACING: [f64; 4] = [1.0, 2.0, 4.0, 8.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// A device paired with it.
    Used,
    /// Its time ran out.
    Expired,
    /// Five wrong proofs.
    Stopped,
    /// The host's user closed it, or Remote Access went off.
    Cancelled,
    /// The device whose ask opened it no longer needs it.
    Withdrawn,
    /// No window was ever opened.
    None,
}

/// Who put a window up by asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAsk {
    pub fingerprint: Vec<u8>,
    pub source: String,
    /// "on this network", "nearby" or "on this Mac".
    pub from: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    /// The QR path's key (16 bytes).
    pub secret: Vec<u8>,
    /// The typed path's 12 digits.
    pub code: String,
    /// K from the code (PBKDF2, derived off the network thread); `None` until then.
    pub code_key: Option<[u8; 32]>,
    pub expires_at: f64,
    pub failures: u32,
    pub next_allowed_at: f64,
    pub last_attempt_by_source: HashMap<String, f64>,
    pub requested_by: Option<String>,
    pub last_wrong_from: Option<String>,
    pub by_device: Option<DeviceAsk>,
    /// The remote door takes its proofs.
    pub for_remote: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Closed(CloseReason),
    Open(Box<Open>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// The proof holds: send proof_M. The window is now closed (used).
    Accept { proof_m: [u8; 32] },
    /// A wrong proof; this many are still taken.
    Reject { tries_left: u32 },
    /// Not now; nothing counted.
    Busy { retry_after: f64 },
    /// No window, or it just closed.
    Closed(CloseReason),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PairingWindow {
    state: State,
}

impl Default for PairingWindow {
    fn default() -> Self {
        PairingWindow { state: State::Closed(CloseReason::None) }
    }
}

impl PairingWindow {
    pub fn new() -> PairingWindow {
        PairingWindow::default()
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, State::Open(_))
    }

    pub fn current(&self) -> Option<&Open> {
        match &self.state {
            State::Open(w) => Some(w),
            State::Closed(_) => None,
        }
    }

    pub fn close_reason(&self) -> Option<CloseReason> {
        match self.state {
            State::Closed(r) => Some(r),
            State::Open(_) => None,
        }
    }

    /// Opens a fresh window (a new secret and code), replacing any other.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &mut self,
        secret: &[u8],
        code: &str,
        now: f64,
        lifetime: f64,
        requested_by: Option<String>,
        by_device: Option<DeviceAsk>,
        for_remote: bool,
    ) {
        self.state = State::Open(Box::new(Open {
            secret: secret.to_vec(),
            code: code.to_string(),
            code_key: None,
            expires_at: now + lifetime,
            failures: 0,
            next_allowed_at: 0.0,
            last_attempt_by_source: HashMap::new(),
            requested_by,
            last_wrong_from: None,
            by_device,
            for_remote,
        }));
    }

    /// The host's user opened a window while a device-opened one is up: the remote door takes
    /// it too. Whether it changed anything.
    pub fn make_remote(&mut self) -> bool {
        match &mut self.state {
            State::Open(w) if !w.for_remote => {
                w.for_remote = true;
                true
            }
            _ => false,
        }
    }

    /// K for `code`, once derived; ignored when the window has moved on to another code.
    pub fn set_code_key(&mut self, key: [u8; 32], code: &str) {
        if let State::Open(w) = &mut self.state {
            if w.code == code {
                w.code_key = Some(key);
            }
        }
    }

    pub fn close(&mut self, reason: CloseReason) {
        if self.is_open() {
            self.state = State::Closed(reason);
        }
    }

    /// Closes it as expired when its time is up. Whether it did.
    pub fn expire_if_due(&mut self, now: f64) -> bool {
        match &self.state {
            State::Open(w) if now >= w.expires_at => {
                self.state = State::Closed(CloseReason::Expired);
                true
            }
            _ => false,
        }
    }

    /// Kind 20's reason for a proof turned away closed.
    pub fn closed_reason(reason: CloseReason, stopped_by_this_proof: bool) -> &'static str {
        if reason == CloseReason::Stopped && stopped_by_this_proof {
            PairResult::STOPPED
        } else if reason == CloseReason::Expired {
            PairResult::EXPIRED
        } else {
            PairResult::CLOSED
        }
    }

    /// Judges one PairRequest's proof. `fp_device` and `fp_mac` come from the TLS session;
    /// `source` is the connection's address.
    pub fn try_proof(
        &mut self,
        method: &str,
        proof: &[u8],
        fp_device: &[u8],
        fp_mac: &[u8],
        source: &str,
        now: f64,
    ) -> Verdict {
        let w = match &mut self.state {
            State::Open(w) => w,
            State::Closed(r) => return Verdict::Closed(*r),
        };
        if now >= w.expires_at {
            self.state = State::Closed(CloseReason::Expired);
            return Verdict::Closed(CloseReason::Expired);
        }
        if now < w.next_allowed_at {
            return Verdict::Busy { retry_after: w.next_allowed_at - now };
        }
        if let Some(&last) = w.last_attempt_by_source.get(source) {
            if now - last < SOURCE_SPACING {
                return Verdict::Busy { retry_after: SOURCE_SPACING - (now - last) };
            }
        }
        let key: Option<Vec<u8>> = match method {
            PairRequest::QR => Some(w.secret.clone()),
            PairRequest::CODE => match w.code_key {
                Some(k) => Some(k.to_vec()),
                None => return Verdict::Busy { retry_after: 1.0 },
            },
            // An unknown method never matches: a wrong proof.
            _ => None,
        };
        w.last_attempt_by_source.insert(source.to_string(), now);
        if let Some(key) = key {
            if proof::is_valid_device_proof(proof, &key, fp_device, fp_mac) {
                let proof_m = proof::mac_proof(&key, fp_mac, fp_device);
                self.state = State::Closed(CloseReason::Used);
                return Verdict::Accept { proof_m };
            }
        }
        w.failures += 1;
        w.last_wrong_from = Some(source.to_string());
        if w.failures >= MAX_FAILURES {
            self.state = State::Closed(CloseReason::Stopped);
            return Verdict::Closed(CloseReason::Stopped);
        }
        w.next_allowed_at = now + SPACING[(w.failures as usize - 1).min(SPACING.len() - 1)];
        Verdict::Reject { tries_left: MAX_FAILURES - w.failures }
    }
}
