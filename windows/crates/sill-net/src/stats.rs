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
    /// Frames the capture delivered (WGC sends one per change on screen).
    pub captured: AtomicU64,
    /// Captured frames replaced by a newer one before they were encoded (the 1-slot mailbox).
    pub superseded: AtomicU64,
    /// Microseconds spent in BGRA to NV12 (submitting the GPU work), summed.
    pub convert_us: AtomicU64,
    /// Access units out of the encoder.
    pub encoded: AtomicU64,
    /// Microseconds from submitting a frame to its access unit coming out, summed, and the most.
    pub encode_us: AtomicU64,
    pub encode_us_max: AtomicU64,
    /// Access unit bytes out of the encoder.
    pub au_bytes: AtomicU64,
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
    pub captured: u64,
    pub superseded: u64,
    pub convert_us: u64,
    pub encoded: u64,
    pub encode_us: u64,
    pub encode_us_max: u64,
    pub au_bytes: u64,
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
            captured: t(&self.captured),
            superseded: t(&self.superseded),
            convert_us: t(&self.convert_us),
            encoded: t(&self.encoded),
            encode_us: t(&self.encode_us),
            encode_us_max: t(&self.encode_us_max),
            au_bytes: t(&self.au_bytes),
        }
    }

    /// Notes one access unit that took `us` microseconds to encode.
    pub fn note_encoded(&self, us: u64, bytes: usize) {
        self.encoded.fetch_add(1, Ordering::Relaxed);
        self.encode_us.fetch_add(us, Ordering::Relaxed);
        self.encode_us_max.fetch_max(us, Ordering::Relaxed);
        self.au_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

impl Second {
    /// The capture and encoder part of the stats line; empty while nothing was captured or encoded.
    pub fn media_words(&self) -> String {
        if self.captured == 0 && self.encoded == 0 {
            return String::new();
        }
        let avg = |sum: u64, n: u64| if n == 0 { 0.0 } else { sum as f64 / n as f64 / 1000.0 };
        format!(
            " | capture {} fps (superseded {}) convert {:.1} ms | encode {} fps {:.1} ms (max {:.1}) AU {} kB/s",
            self.captured,
            self.superseded,
            avg(self.convert_us, self.captured.saturating_sub(self.superseded).max(1)),
            self.encoded,
            avg(self.encode_us, self.encoded),
            self.encode_us_max as f64 / 1000.0,
            self.au_bytes / 1000
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_words_only_when_there_is_media() {
        assert_eq!(Second::default().media_words(), "");
        let s = Stats::default();
        s.captured.store(30, Ordering::Relaxed);
        s.superseded.store(2, Ordering::Relaxed);
        s.convert_us.store(28_000, Ordering::Relaxed);
        s.note_encoded(4_000, 10_000);
        s.note_encoded(6_000, 20_000);
        assert_eq!(
            s.take().media_words(),
            " | capture 30 fps (superseded 2) convert 1.0 ms | encode 2 fps 5.0 ms (max 6.0) AU 30 kB/s"
        );
        assert_eq!(s.take().encode_us_max, 0, "taken and reset");
    }
}
