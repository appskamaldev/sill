//! The synthetic stream: a pre-encoded HEVC clip (`windows/testdata/clip.bin`, made by
//! `windows/tools/testclip`) looped at its frame rate, fanned out to every session that shows the
//! Desktop. Kind 0 goes before every keyframe; a join or a keyframe request restarts the clip at
//! its first frame (an IDR). Per client at most `MAX_IN_FLIGHT` frames wait to be written: a delta
//! beyond that is dropped and the client then waits for the next keyframe (upstream
//! `StreamServer.broadcast`).
//!
//! Clip format, all integers big-endian:
//! ```text
//! 0   8  "SILLCLIP"
//! 8   1  version = 1
//! 9   1  fps
//! 10  2  width
//! 12  2  height
//! 14  2  frame count N (≥ 1)
//! 16  4  L, then L bytes: kind 0's payload (nalLen 4, count, {len u16, VPS|SPS|PPS}…)
//! then N times: flags u8 (bit 0 = IDR), length u32, the access unit (NAL units with 4-byte
//! length prefixes, no parameter sets). Frame 0 is an IDR.
//! ```

use crate::stats::Stats;
use sill_wire::{Kind, ParameterSets, HEADER_LENGTH};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

pub const MAX_IN_FLIGHT: usize = 2;

#[derive(Debug)]
pub struct ClipFrame {
    pub keyframe: bool,
    pub au: Arc<[u8]>,
}

#[derive(Debug)]
pub struct Clip {
    pub fps: u8,
    pub width: u16,
    pub height: u16,
    /// Kind 0's payload.
    pub parameter_sets: Arc<[u8]>,
    pub frames: Vec<ClipFrame>,
}

fn take<'a>(d: &mut &'a [u8], n: usize) -> Result<&'a [u8], String> {
    if d.len() < n {
        return Err("clip: truncated".into());
    }
    let (a, b) = d.split_at(n);
    *d = b;
    Ok(a)
}

fn be(b: &[u8]) -> u32 {
    b.iter().fold(0u32, |a, &x| (a << 8) | x as u32)
}

impl Clip {
    pub fn parse(mut d: &[u8]) -> Result<Clip, String> {
        let d = &mut d;
        if take(d, 8)? != b"SILLCLIP" {
            return Err("clip: not a SILLCLIP file".into());
        }
        if take(d, 1)?[0] != 1 {
            return Err("clip: unknown version".into());
        }
        let fps = take(d, 1)?[0];
        let width = be(take(d, 2)?) as u16;
        let height = be(take(d, 2)?) as u16;
        let count = be(take(d, 2)?) as usize;
        if fps == 0 || count == 0 {
            return Err("clip: no frames".into());
        }
        let ps_len = be(take(d, 4)?) as usize;
        let ps = take(d, ps_len)?;
        let parsed = ParameterSets::decode(ps).ok_or("clip: bad parameter sets")?;
        if parsed.nal_unit_header_length != 4 || parsed.sets.len() < 3 {
            return Err("clip: parameter sets must be VPS, SPS, PPS with 4-byte prefixes".into());
        }
        let mut frames = Vec::with_capacity(count);
        for i in 0..count {
            let flags = take(d, 1)?[0];
            let len = be(take(d, 4)?) as usize;
            let au = take(d, len)?;
            // Every NAL's 4-byte prefix must account for the AU exactly.
            let mut rest = au;
            while !rest.is_empty() {
                let n = be(take(&mut rest, 4)?) as usize;
                take(&mut rest, n).map_err(|_| format!("clip: frame {i}'s NAL lengths overrun it"))?;
            }
            frames.push(ClipFrame { keyframe: flags & 1 == 1, au: Arc::from(au) });
        }
        if !frames[0].keyframe {
            return Err("clip: frame 0 is not an IDR".into());
        }
        Ok(Clip { fps, width, height, parameter_sets: Arc::from(ps), frames })
    }

    /// The clip shipped in the repository.
    pub fn bundled() -> Clip {
        Clip::parse(include_bytes!("../../../testdata/clip.bin")).expect("testdata/clip.bin is valid")
    }
}

/// The 14-byte header for a payload of `len` bytes.
pub fn header(kind: Kind, timestamp: f64, keyframe: bool, len: usize) -> [u8; HEADER_LENGTH] {
    let mut h = [0u8; HEADER_LENGTH];
    h[0] = kind.as_u8();
    h[1..9].copy_from_slice(&timestamp.to_bits().to_be_bytes());
    h[9] = u8::from(keyframe);
    h[10..14].copy_from_slice(&(len as u32).to_be_bytes());
    h
}

/// What a session's writer sends.
pub enum Out {
    Control(Vec<u8>),
    /// Kind 1: counted in flight until written.
    Frame {
        header: [u8; HEADER_LENGTH],
        au: Arc<[u8]>,
    },
}

/// One session's place in the fan-out.
pub struct ClientVideo {
    pub tx: mpsc::UnboundedSender<Out>,
    pub in_flight: Arc<AtomicUsize>,
    pub needs_keyframe: AtomicBool,
    /// It shows the Desktop (the only source this host has).
    pub wants: AtomicBool,
    /// Microseconds since the host started, of its last message queued (ticks skip a busy client).
    pub last_sent: Arc<AtomicU64>,
}

pub struct Hub {
    pub clip: Clip,
    clients: Mutex<HashMap<u64, Arc<ClientVideo>>>,
    restart: AtomicBool,
    next_id: AtomicU64,
    stats: Arc<Stats>,
}

impl Hub {
    pub fn new(clip: Clip, stats: Arc<Stats>) -> Hub {
        Hub {
            clip,
            clients: Mutex::new(HashMap::new()),
            restart: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
            stats,
        }
    }

    /// A new viewer: it waits for a keyframe, and the clip restarts at one.
    pub fn join(&self, c: Arc<ClientVideo>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.clients.lock().unwrap().insert(id, c);
        self.request_keyframe();
        id
    }

    pub fn leave(&self, id: u64) {
        self.clients.lock().unwrap().remove(&id);
    }

    pub fn request_keyframe(&self) {
        self.restart.store(true, Ordering::SeqCst);
    }

    /// Whether any session shows the picture now (ticks run only then).
    pub fn streaming(&self) -> bool {
        self.clients.lock().unwrap().values().any(|c| c.wants.load(Ordering::Relaxed))
    }

    /// Loops the clip until `stop` says so.
    pub async fn run(self: Arc<Self>, mut stop: tokio::sync::watch::Receiver<bool>, now_us: impl Fn() -> u64) {
        let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / f64::from(self.clip.fps)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut index = 0usize;
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = stop.changed() => return,
            }
            if !self.streaming() {
                continue;
            }
            if self.restart.swap(false, Ordering::SeqCst) {
                index = 0;
            }
            let frame = &self.clip.frames[index];
            self.deliver(frame, now_us());
            index = (index + 1) % self.clip.frames.len();
        }
    }

    fn deliver(&self, frame: &ClipFrame, now_us: u64) {
        let ts = crate::wall_clock();
        let clients: Vec<Arc<ClientVideo>> = self.clients.lock().unwrap().values().cloned().collect();
        let mut want_keyframe = false;
        if frame.keyframe {
            self.stats.keyframes.fetch_add(1, Ordering::Relaxed);
        }
        for c in clients {
            if !c.wants.load(Ordering::Relaxed) {
                continue;
            }
            if c.needs_keyframe.load(Ordering::Relaxed) {
                if !frame.keyframe {
                    self.stats.wait_key.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            } else if !frame.keyframe && c.in_flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT {
                // Every later delta references this one: wait for a keyframe instead of sending them.
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                c.needs_keyframe.store(true, Ordering::Relaxed);
                want_keyframe = true;
                continue;
            }
            if frame.keyframe {
                let ps = &self.clip.parameter_sets;
                let mut m = header(Kind::ParameterSets, ts, false, ps.len()).to_vec();
                m.extend_from_slice(ps);
                if c.tx.send(Out::Control(m)).is_err() {
                    continue;
                }
            }
            c.in_flight.fetch_add(1, Ordering::SeqCst);
            let h = header(Kind::Frame, ts, frame.keyframe, frame.au.len());
            if c.tx.send(Out::Frame { header: h, au: frame.au.clone() }).is_err() {
                c.in_flight.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            c.needs_keyframe.store(false, Ordering::Relaxed);
            c.last_sent.store(now_us, Ordering::Relaxed);
            self.stats.sent.fetch_add(1, Ordering::Relaxed);
            self.stats.bytes.fetch_add((frame.au.len() + HEADER_LENGTH) as u64, Ordering::Relaxed);
        }
        if want_keyframe {
            self.request_keyframe();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_clip_parses() {
        let c = Clip::bundled();
        assert!(c.frames.len() >= 2);
        assert!(c.frames[0].keyframe);
        assert_eq!((c.width, c.height), (1280, 800));
        let ps = ParameterSets::decode(&c.parameter_sets).unwrap();
        // VPS (32), SPS (33), PPS (34): HEVC nal_unit_type is bits 1..6 of the first byte.
        let types: Vec<u8> = ps.sets.iter().map(|s| (s[0] >> 1) & 0x3F).collect();
        assert_eq!(&types[..3], &[32, 33, 34]);
    }

    #[test]
    fn at_most_two_frames_in_flight_then_wait_for_a_keyframe() {
        let hub = Hub::new(Clip::bundled(), Arc::new(Stats::default()));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let c = Arc::new(ClientVideo {
            tx,
            in_flight: Arc::new(AtomicUsize::new(0)),
            needs_keyframe: AtomicBool::new(true),
            wants: AtomicBool::new(true),
            last_sent: Arc::new(AtomicU64::new(0)),
        });
        hub.join(c.clone());
        assert!(hub.restart.swap(false, Ordering::SeqCst), "a join asks for a keyframe");
        // Nothing is written (the writer is stuck): frames 0 (IDR) and 1 go, 2 is dropped.
        for f in &hub.clip.frames[..4] {
            hub.deliver(f, 0);
        }
        let mut got = Vec::new();
        while let Ok(o) = rx.try_recv() {
            got.push(match o {
                Out::Control(b) => (b[0], false),
                Out::Frame { header, .. } => (header[0], header[9] == 1),
            });
        }
        assert_eq!(got, vec![(0, false), (1, true), (1, false)]);
        assert_eq!(c.in_flight.load(Ordering::SeqCst), 2);
        assert!(c.needs_keyframe.load(Ordering::SeqCst));
        assert!(hub.restart.load(Ordering::SeqCst), "a drop asks for a keyframe");
        assert_eq!(hub.stats.take().dropped, 1);
        // The writer catches up; deltas still wait for the keyframe, which then goes with kind 0.
        c.in_flight.store(0, Ordering::SeqCst);
        hub.deliver(&hub.clip.frames[5], 0);
        assert!(rx.try_recv().is_err());
        hub.deliver(&hub.clip.frames[0], 0);
        assert!(matches!(rx.try_recv(), Ok(Out::Control(b)) if b[0] == 0));
        assert!(matches!(rx.try_recv(), Ok(Out::Frame { header, .. }) if header[9] == 1));
    }

    #[test]
    fn junk_is_refused() {
        assert!(Clip::parse(b"").is_err());
        assert!(Clip::parse(b"SILLCLIP\x02").is_err());
    }
}
