//! Who each door admits (upstream `DoorPolicy.swift`: `trusts`, `atReady`, `pairing`).
//! `afterGate`, `stillServes` and the origin rules need OriginPolicy and come later.

use sill_pairing::{PAIRING_ALPN, SESSION_ALPN};
use sill_wire::{Goodbye, PairRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Door {
    /// Bonjour, a dynamic port.
    Home,
    /// Port 7455.
    Remote,
}

/// A remote door's sessions at once; the ninth gets goodbye `busy`.
pub const MAX_REMOTE_SESSIONS: usize = 8;

/// What admission knows about one connection: its key, and the host's switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trust {
    /// The peer presented a P-256 key.
    pub has_key: bool,
    /// Its key is on the trust list.
    pub paired: bool,
    /// The home door's Require pairing.
    pub require_pairing: bool,
    pub remote_access: bool,
    pub internet_access: bool,
    /// A pairing window is open: the home door takes its proofs.
    pub pairing_open: bool,
    /// A window the remote door takes proofs for (opened by the host's user, never by an ask).
    pub remote_pairing_open: bool,
}

impl Default for Trust {
    fn default() -> Trust {
        Trust {
            has_key: true,
            paired: false,
            require_pairing: true,
            remote_access: false,
            internet_access: false,
            pairing_open: false,
            remote_pairing_open: false,
        }
    }
}

/// The TLS handshake's verify decision, with the negotiated ALPN: `sill/1` for a paired key, and
/// at home for any key while Require pairing is off; `sill-pair/1` always at home, and at the
/// remote door only while a window it takes proofs for is open; anything else never.
pub fn trusts(door: Door, alpn: Option<&str>, t: &Trust) -> bool {
    if !t.has_key {
        return false;
    }
    match alpn {
        Some(SESSION_ALPN) => match door {
            Door::Home => t.paired || !t.require_pairing,
            Door::Remote => t.paired,
        },
        Some(PAIRING_ALPN) => match door {
            Door::Home => true,
            Door::Remote => t.remote_pairing_open,
        },
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AtReady {
    /// A session.
    Session,
    /// A pairing connection: read exactly one kind 19.
    Pairing,
    /// A paired key the door does not serve now: this goodbye, then close.
    Goodbye(&'static str),
    /// Cancelled with no Sill byte, counted under this word.
    Refuse(&'static str),
}

/// What `.ready` does with a connection the verify block let through, judged again now.
pub fn at_ready(door: Door, alpn: Option<&str>, t: &Trust, remote_sessions: usize) -> AtReady {
    if !t.has_key {
        return AtReady::Refuse("unpaired");
    }
    match (door, alpn) {
        (Door::Home, Some(SESSION_ALPN)) => {
            if trusts(Door::Home, alpn, t) {
                AtReady::Session
            } else {
                AtReady::Refuse("unpaired")
            }
        }
        (Door::Home, Some(PAIRING_ALPN)) => AtReady::Pairing,
        (Door::Remote, Some(SESSION_ALPN)) => {
            if !t.paired {
                AtReady::Refuse("unpaired")
            } else if !t.remote_access {
                AtReady::Goodbye(Goodbye::REMOTE_OFF)
            } else if remote_sessions >= MAX_REMOTE_SESSIONS {
                AtReady::Goodbye(Goodbye::BUSY)
            } else {
                AtReady::Session
            }
        }
        (Door::Remote, Some(PAIRING_ALPN)) if t.remote_pairing_open => AtReady::Pairing,
        _ => AtReady::Refuse("unpaired"),
    }
}

/// How a door treats the one kind 19 of a pairing connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pairing {
    /// The home door's "pair me" (the ask rule; Phase 2).
    Ask,
    /// The device's own Cancel at home: answered `closed`, no try counted.
    Withdraw,
    /// The pairing window judges the proof ("qr", "code" and any other method).
    Window,
    /// Answered `closed` at once with no try: an ask or cancel at the remote door, or `v ≠ 1`.
    Closed,
}

pub fn pairing(door: Door, method: &str, version: i64) -> Pairing {
    if version != PairRequest::VERSION {
        return Pairing::Closed;
    }
    if method == PairRequest::CANCEL {
        return if door == Door::Home { Pairing::Withdraw } else { Pairing::Closed };
    }
    if method != PairRequest::ASK {
        return Pairing::Window;
    }
    if door == Door::Home {
        Pairing::Ask
    } else {
        Pairing::Closed
    }
}

/// From accept to `.ready`: the whole TLS handshake (`DoorPolicy.handshakeDeadline`).
pub const HANDSHAKE_DEADLINE: f64 = 4.0;
/// From `.ready` to a pairing connection's one kind 19 (`DoorPolicy.requestDeadline`).
pub const REQUEST_DEADLINE: f64 = 4.0;
/// From accept to admission, at the latest; a known address's handshake deadline too.
pub const ADMISSION_DEADLINE: f64 = 10.0;

/// When a connection still waiting for admission is refused, in seconds after accept
/// (`DoorPolicy.deadline`): before `.ready` the handshake deadline (`ADMISSION_DEADLINE` for an
/// address that recently proved a paired key); once ready (a pairing connection waiting for its
/// kind 19) `REQUEST_DEADLINE` after `.ready`, never later than `ADMISSION_DEADLINE`.
pub fn deadline(known: bool, ready_after: Option<f64>) -> f64 {
    match ready_after {
        None if known => ADMISSION_DEADLINE,
        None => HANDSHAKE_DEADLINE,
        Some(r) => (r + REQUEST_DEADLINE).min(ADMISSION_DEADLINE),
    }
}
