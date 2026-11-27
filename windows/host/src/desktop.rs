//! The desktop as the picture (Windows): primary monitor → WGC → NV12 on the GPU → hardware HEVC
//! → `Hub::deliver`. One thread owns the whole pipeline and builds it when a session starts
//! viewing, rebuilds it when the monitor's size or the wanted frame rate changes, and tears it
//! down a few seconds after the last viewer leaves.
//!
//! Latency over quality: the capture's newest frame wins (older waiting ones are released
//! unseen), frames are paced to the stream's fps, and a keyframe wanted on a still screen
//! re-encodes the last NV12 texture.

use sill_capture::{Capture, Converter, Gpu, Monitor};
use sill_encode::{annexb, mf};
use sill_net::stats::Stats;
use sill_net::{Encoded, Hub};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// The pipeline stays this long after the last viewer leaves (a quick reconnect reuses it).
const IDLE_TEARDOWN: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub bitrate: u32,
    pub capture_scale: f64,
}

/// Starts the desktop pipeline's thread; it runs until `stop` is set.
pub fn spawn(
    hub: Arc<Hub>,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
    now_us: impl Fn() -> u64 + Send + 'static,
    o: Options,
) {
    let _ = std::thread::Builder::new().name("sill-desktop".into()).spawn(move || {
        if let Err(e) = mf::startup() {
            warn!("Media Foundation did not start ({e}); no desktop picture.");
            return;
        }
        let mut pipeline: Option<Pipeline> = None;
        let mut idle_since: Option<Instant> = None;
        let mut retry_at: Option<Instant> = None;
        while !stop.load(Ordering::Relaxed) {
            if !hub.streaming() {
                if pipeline.is_some() {
                    let since = *idle_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= IDLE_TEARDOWN {
                        info!("Desktop capture stopped: nobody is viewing.");
                        pipeline = None;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            idle_since = None;
            let monitor = match sill_capture::primary_monitor() {
                Some(m) => m,
                None => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            let fps = sill_capture::stream_fps(hub.max_fps(), monitor.refresh_hz);
            let size = sill_capture::stream_size(monitor.width, monitor.height, o.capture_scale);
            let stale = pipeline.as_ref().is_some_and(|p| {
                p.fps != fps || (p.monitor.width, p.monitor.height) != (monitor.width, monitor.height) || p.size != size
            });
            if stale {
                info!("Desktop picture changes: rebuilding the pipeline.");
                pipeline = None;
            }
            if pipeline.is_none() {
                if retry_at.is_some_and(|t| Instant::now() < t) {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                match Pipeline::new(&monitor, size, fps, o.bitrate) {
                    Ok(p) => {
                        info!(
                            "Desktop capture: {} {}×{} at {} Hz → {}×{} at up to {} fps, {} kbit/s, {} on {}.",
                            monitor.device,
                            monitor.width,
                            monitor.height,
                            monitor.refresh_hz,
                            size.0,
                            size.1,
                            fps,
                            p.encoder.settings.bitrate / 1000,
                            p.encoder.name,
                            p.gpu.adapter
                        );
                        hub.set_picture(format!("the desktop streams at {}×{} up to {} fps", size.0, size.1, fps));
                        hub.request_keyframe();
                        retry_at = None;
                        pipeline = Some(p);
                    }
                    Err(e) => {
                        warn!("Desktop capture could not start: {e}. Retrying in 2 s.");
                        retry_at = Some(Instant::now() + Duration::from_secs(2));
                        continue;
                    }
                }
            }
            let p = pipeline.as_mut().expect("built above");
            if let Err(e) = p.step(&hub, &stats, &now_us) {
                warn!("Desktop capture failed: {e}. Rebuilding.");
                pipeline = None;
                retry_at = Some(Instant::now() + Duration::from_secs(1));
            }
        }
    });
}

struct Pipeline {
    /// The 1 ms timer the encoder needs, released with the pipeline (3 s after the last viewer).
    _timer: sill_capture::FineTimer,
    gpu: Gpu,
    monitor: Monitor,
    capture: Capture,
    wake: mpsc::Receiver<()>,
    converter: Converter,
    encoder: mf::Encoder,
    size: (u32, u32),
    fps: u32,
    /// The newest NV12 picture, encoded again when a keyframe is wanted on a still screen.
    last: Option<sill_capture::Texture>,
    /// A new picture waits to be encoded.
    fresh: bool,
    keyframe_wanted: bool,
    next_slot: Instant,
    frame_index: i64,
    parameter_sets: Option<Arc<[u8]>>,
    /// When each frame the encoder holds went in, oldest first.
    in_flight: std::collections::VecDeque<Instant>,
}

impl Pipeline {
    fn new(monitor: &Monitor, size: (u32, u32), fps: u32, bitrate: u32) -> Result<Pipeline, String> {
        let gpu = Gpu::new().map_err(|e| format!("no D3D11 hardware device ({e})"))?;
        let (tx, wake) = mpsc::sync_channel::<()>(1);
        let capture = Capture::start(&gpu, monitor, move || {
            let _ = tx.try_send(());
        })
        .map_err(|e| format!("Windows Graphics Capture refused the monitor ({e})"))?;
        let converter = Converter::new(&gpu, capture.width, capture.height, size.0, size.1)
            .map_err(|e| format!("the GPU video processor refused BGRA to NV12 ({e})"))?;
        let per_stream = (u64::from(bitrate) * u64::from(fps) / 60).clamp(500_000, u64::from(u32::MAX)) as u32;
        let encoder =
            mf::Encoder::new(&gpu.device, mf::Settings { width: size.0, height: size.1, fps, bitrate: per_stream })
                .map_err(|e| format!("no hardware HEVC encoder took {}×{} at {fps} fps ({e})", size.0, size.1))?;
        Ok(Pipeline {
            _timer: sill_capture::FineTimer::new(),
            gpu,
            monitor: monitor.clone(),
            capture,
            wake,
            converter,
            encoder,
            size,
            fps,
            last: None,
            fresh: false,
            keyframe_wanted: true,
            next_slot: Instant::now(),
            frame_index: 0,
            parameter_sets: None,
            in_flight: std::collections::VecDeque::new(),
        })
    }

    fn step(&mut self, hub: &Hub, stats: &Stats, now_us: &impl Fn() -> u64) -> Result<(), String> {
        // Sleep until a frame arrives or the next slot comes (when a picture waits for one); poll
        // every 2 ms while the encoder holds a frame, so its output goes out as soon as it is out.
        let mut wait = if self.fresh || (self.keyframe_wanted && self.last.is_some()) {
            self.next_slot.saturating_duration_since(Instant::now())
        } else {
            Duration::from_millis(50)
        };
        if !self.in_flight.is_empty() {
            wait = wait.min(Duration::from_millis(2));
        }
        let _ = self.wake.recv_timeout(wait);
        // Every output first: the MFT asks for no more input while one waits to be taken (a slow
        // keyframe missed a 50 ms wait and stalled the stream for good, Duo test 2026-11-27).
        self.drain(hub, stats, now_us, Duration::ZERO)?;
        if let Some(t) = self.in_flight.front() {
            if t.elapsed() > Duration::from_secs(2) {
                return Err("the encoder has held a frame for 2 s".into());
            }
        }
        if hub.take_keyframe_request() {
            self.keyframe_wanted = true;
        }
        if let Some(frame) = self.capture.newest().map_err(|e| format!("taking a frame: {e}"))? {
            stats.captured.fetch_add(1, Ordering::Relaxed);
            if self.fresh {
                stats.superseded.fetch_add(1, Ordering::Relaxed);
            }
            if (frame.width, frame.height) != (self.capture.width, self.capture.height) {
                return Err(format!("the monitor is now {}×{}", frame.width, frame.height));
            }
            let t = Instant::now();
            let nv12 = self.converter.convert(&frame).map_err(|e| format!("BGRA to NV12: {e}"))?;
            stats.convert_us.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
            self.last = Some(nv12);
            self.fresh = true;
        }
        let due = Instant::now() >= self.next_slot;
        let Some(nv12) = self.last.clone() else { return Ok(()) };
        if !due || !(self.fresh || self.keyframe_wanted) {
            return Ok(());
        }
        if !self.encoder.ready(Duration::ZERO).map_err(|e| format!("encoder: {e}"))? {
            return Ok(());
        }
        if self.keyframe_wanted {
            self.encoder.force_keyframe();
        }
        let interval = 10_000_000 / i64::from(self.fps);
        self.encoder.submit(&nv12, self.frame_index * interval, interval).map_err(|e| format!("encoder input: {e}"))?;
        self.in_flight.push_back(Instant::now());
        self.frame_index += 1;
        self.fresh = false;
        self.keyframe_wanted = false;
        let slot = Duration::from_secs_f64(1.0 / f64::from(self.fps));
        self.next_slot = (self.next_slot + slot).max(Instant::now() + slot / 2);
        // Low-latency mode: one AU per input, some milliseconds later; what is not out within
        // 40 ms is taken by a later step.
        self.drain(hub, stats, now_us, Duration::from_millis(40))
    }

    /// Takes every output the encoder has, waiting up to `first` for one.
    fn drain(&mut self, hub: &Hub, stats: &Stats, now_us: &impl Fn() -> u64, first: Duration) -> Result<(), String> {
        let mut wait = first;
        loop {
            match self.encoder.output(wait) {
                Ok(Some(out)) => {
                    let us = self.in_flight.pop_front().map_or(0, |t| t.elapsed().as_micros() as u64);
                    self.publish(hub, stats, now_us, &out.annexb, us);
                    wait = Duration::ZERO;
                }
                Ok(None) => return Ok(()),
                Err(e) => return Err(format!("encoder output: {e}")),
            }
        }
    }

    fn publish(&mut self, hub: &Hub, stats: &Stats, now_us: &impl Fn() -> u64, annexb_bytes: &[u8], us: u64) {
        let r = annexb::repack(annexb_bytes);
        if !r.parameter_sets.is_empty() {
            match annexb::kind0(&r.parameter_sets) {
                Some(k0) => self.parameter_sets = Some(Arc::from(k0)),
                None => warn!("The encoder's parameter sets are incomplete; waiting for the next keyframe."),
            }
        }
        if r.au.is_empty() {
            return;
        }
        stats.note_encoded(us, r.au.len());
        let Some(ps) = self.parameter_sets.clone() else {
            // A delta before the first parameter sets: nobody could decode it.
            self.keyframe_wanted = true;
            return;
        };
        hub.deliver(&Encoded { keyframe: r.keyframe, au: Arc::from(r.au), parameter_sets: ps }, now_us());
    }
}
