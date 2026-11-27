//! Counters behind the once-a-second stats line.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Stats {
    pub sent: AtomicU64,
    pub dropped: AtomicU64,
    pub wait_key: AtomicU64,
    pub keyframes: AtomicU64,
    pub bytes: AtomicU64,
    pub pongs: AtomicU64,
    pub ticks: AtomicU64,
    pub refused: AtomicU64,
}

/// One second's worth, taken (and reset) by `Stats::take`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Second {
    pub sent: u64,
    pub dropped: u64,
    pub wait_key: u64,
    pub keyframes: u64,
    pub bytes: u64,
    pub pongs: u64,
    pub ticks: u64,
    pub refused: u64,
}

impl Stats {
    pub fn take(&self) -> Second {
        let t = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
        Second {
            sent: t(&self.sent),
            dropped: t(&self.dropped),
            wait_key: t(&self.wait_key),
            keyframes: t(&self.keyframes),
            bytes: t(&self.bytes),
            pongs: t(&self.pongs),
            ticks: t(&self.ticks),
            refused: t(&self.refused),
        }
    }
}
