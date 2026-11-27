//! Where a session's kind 8 events go. `sill-net` is portable and knows no `SendInput`: the host
//! hands it a sink (Windows: `SendInput`), and without one input is only logged.

use sill_wire::InputEvent;

pub trait InputSink: Send + Sync {
    /// One event from session `session`; `literal`: its hello said `pcKeys` ≥ 1.
    fn event(&self, session: u64, literal: bool, ev: &InputEvent);
    /// The session ended (or stopped viewing): release everything it holds.
    fn left(&self, session: u64);
}
