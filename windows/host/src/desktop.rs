//! The picture (Windows): the primary monitor or one window → WGC → BGRA or NV12 on the GPU →
//! hardware HEVC → `Hub::deliver`. One thread owns the whole pipeline. It builds the pipeline when a
//! session starts viewing, and rebuilds it when the source (the hub's `active`), the size, the
//! wanted frame rate or the bitrate changes. It tears the pipeline down a few seconds after the last
//! viewer leaves.
//!
//! A streamed window is fitted to the device's panel when the device sends a text scale (the
//! upstream viewport fit, which keeps text 1:1 and crisp). It is put back when it stops being the
//! source, the fit ends, or the host quits.
//!
//! Latency over quality: the capture's newest frame wins (older waiting ones are released
//! unseen), frames are paced to the stream's fps, and a keyframe wanted on a still screen
//! re-encodes the last picture.

use crate::catalog::Catalog;
use sill_capture::{Capture, Converter, Gpu, Monitor};
use sill_encode::{annexb, mf};
use sill_net::stats::Stats;
use sill_net::{Encoded, Hub};
use sill_wire::StreamSource;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// The pipeline stays this long after the last viewer leaves (a quick reconnect reuses it).
const IDLE_TEARDOWN: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// A cap on the share of the source's pixels (`--capture-scale`, 1 = none), on top of the
    /// devices' Retina or Standard.
    pub capture_scale: f64,
    /// Give the encoder the captured BGRA (it converts on its own engine) instead of converting to
    /// NV12 on the video processor first. Full size only (a scaled stream still goes through the
    /// video processor). On Intel Arc at 1920×1200 this cut capture → sent from about 37 ms to 22 ms
    /// and freed the video processor (2026-11-27); `--encoder-input nv12` turns it off.
    pub bgra_input: bool,
}

/// What is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Monitor,
    Window { id: u32, hwnd: isize },
}

/// The window fitted to the device's panel, and how to put it back.
struct Fit {
    hwnd: isize,
    original: windows::Win32::Foundation::RECT,
    /// The visible frame size asked for, in physical pixels.
    asked: (i32, i32),
}

#[derive(Default)]
struct Fitter {
    fit: Option<Fit>,
    last_check: Option<Instant>,
}

impl Fitter {
    /// Fits the streamed window `hwnd` to `viewport` (points and text scale), or puts a fitted window
    /// back when it is no longer the source or the fit ended. At most every 250 ms.
    fn update(&mut self, hwnd: Option<isize>, viewport: Option<(f64, f64, f64)>) {
        if self.last_check.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        self.last_check = Some(Instant::now());
        use sill_catalog::window_ops as ops;
        if let Some(f) = &self.fit {
            if hwnd != Some(f.hwnd) || viewport.is_none() {
                self.restore();
            }
        }
        let (Some(h), Some((vw, vh, scale))) = (hwnd, viewport) else { return };
        if !ops::is_window(h) || ops::is_maximized(h) {
            return;
        }
        let dpi = sill_catalog::windows_list::dpi_scale_of(windows::Win32::Foundation::HWND(h as *mut _));
        // Upstream: round(panel / scale) points; here points are DIPs.
        // Even pixel sides: the stream needs them, and a window of exactly the stream's size keeps
        // the encoder's BGRA input (an odd side was cut by one and went through the video processor).
        let even = |v: f64| ((v.round() as i32) / 2) * 2;
        let want = (even((vw / scale).round() * dpi), even((vh / scale).round() * dpi));
        if want.0 < 100 || want.1 < 100 {
            return;
        }
        let Some(frame) = sill_catalog::windows_list::frame_of(windows::Win32::Foundation::HWND(h as *mut _)) else {
            return;
        };
        let cur = (frame.right - frame.left, frame.bottom - frame.top);
        let tol = (2.0 * dpi).ceil() as i32;
        if (cur.0 - want.0).abs() <= tol && (cur.1 - want.1).abs() <= tol {
            return;
        }
        if self.fit.is_none() {
            let Some(original) = ops::outer_rect(h) else { return };
            self.fit = Some(Fit { hwnd: h, original, asked: want });
        }
        match ops::set_frame_size(h, want.0, want.1) {
            Ok(()) => {
                if let Some(f) = &mut self.fit {
                    f.asked = want;
                }
                info!(
                    "Window fitted to the device: {}×{} DIPs ({}×{} px) for a {:.0}×{:.0} pt panel at scale {scale}.",
                    (vw / scale).round(),
                    (vh / scale).round(),
                    want.0,
                    want.1,
                    vw,
                    vh
                );
            }
            Err(e) => warn!("Window fit refused: {e}"),
        }
    }

    /// Puts the fitted window back, unless it is gone or someone resized it since (upstream
    /// WindowRestore: "resized on the Mac" is left alone).
    fn restore(&mut self) {
        use sill_catalog::window_ops as ops;
        let Some(f) = self.fit.take() else { return };
        if !ops::is_window(f.hwnd) {
            return;
        }
        let h = windows::Win32::Foundation::HWND(f.hwnd as *mut _);
        let dpi = sill_catalog::windows_list::dpi_scale_of(h);
        let tol = (2.0 * dpi).ceil() as i32;
        let ours = sill_catalog::windows_list::frame_of(h).is_some_and(|r| {
            ((r.right - r.left) - f.asked.0).abs() <= tol && ((r.bottom - r.top) - f.asked.1).abs() <= tol
        });
        if !ours {
            info!("Window not put back: it was resized on this PC since the fit.");
            return;
        }
        match ops::set_outer_rect(f.hwnd, f.original) {
            Ok(()) => info!("Window put back to its own size."),
            Err(e) => warn!("Window could not be put back: {e}"),
        }
    }
}

/// Starts the pipeline's thread; it runs until `stop` is set.
pub fn spawn(
    hub: Arc<Hub>,
    catalog: Arc<Catalog>,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
    now_us: impl Fn() -> u64 + Send + 'static,
    o: Options,
) {
    let _ = std::thread::Builder::new().name("sill-desktop".into()).spawn(move || {
        if let Err(e) = mf::startup() {
            warn!("Media Foundation did not start ({e}); no picture.");
            return;
        }
        let mut pipeline: Option<Pipeline> = None;
        let mut idle_since: Option<Instant> = None;
        let mut retry_at: Option<Instant> = None;
        let mut fitter = Fitter::default();
        while !stop.load(Ordering::Relaxed) {
            let target = match hub.source() {
                StreamSource::Desktop => Some(Target::Monitor),
                StreamSource::Window(id) => catalog.hwnd(id).map(|hwnd| Target::Window { id, hwnd }),
                StreamSource::None => None,
            };
            let window = match target {
                Some(Target::Window { hwnd, .. }) => Some(hwnd),
                _ => None,
            };
            fitter.update(if hub.streaming() { window } else { None }, *hub.viewport.borrow());
            let Some(target) = target.filter(|_| hub.streaming()) else {
                if pipeline.is_some() {
                    let since = *idle_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= IDLE_TEARDOWN || hub.source() == StreamSource::None {
                        info!("Capture stopped: nothing to stream.");
                        pipeline = None;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };
            idle_since = None;
            let monitor = match sill_capture::primary_monitor() {
                Some(m) => m,
                None => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            let video = hub.video();
            let fps = sill_capture::stream_fps(hub.max_fps(), video.max_fps, monitor.refresh_hz);
            let dpi = match target {
                Target::Monitor => monitor.dpi_scale,
                Target::Window { hwnd, .. } => {
                    sill_catalog::windows_list::dpi_scale_of(windows::Win32::Foundation::HWND(hwnd as *mut _))
                }
            };
            let fraction = sill_capture::scale_fraction(video.capture_scale, dpi) * o.capture_scale;
            let bitrate = video.bitrate.clamp(500_000, 200_000_000) as u32;
            let stale = pipeline.as_ref().is_some_and(|p| {
                p.target != target
                    || p.fps != fps
                    || p.fraction != fraction
                    || p.bitrate != bitrate
                    || p.resized
                    || (target == Target::Monitor
                        && (p.monitor.width, p.monitor.height) != (monitor.width, monitor.height))
            });
            if stale {
                let p = pipeline.as_ref().expect("stale implies some");
                if p.target != target {
                    info!("The source changes: rebuilding the pipeline.");
                } else if p.resized {
                    info!("The source's size changed: rebuilding the pipeline.");
                } else {
                    info!("The picture's settings changed: rebuilding the pipeline.");
                }
                pipeline = None;
            }
            if pipeline.is_none() {
                if retry_at.is_some_and(|t| Instant::now() < t) {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                match Pipeline::new(&monitor, target, fraction, fps, bitrate, o.bgra_input) {
                    Ok(p) => {
                        let what = match target {
                            Target::Monitor => format!(
                                "{} {}×{} at {} Hz (scale {:.2})",
                                monitor.device, monitor.width, monitor.height, monitor.refresh_hz, monitor.dpi_scale
                            ),
                            Target::Window { id, .. } => {
                                format!("window {id} {}×{} px (scale {:.2})", p.capture.width, p.capture.height, dpi)
                            }
                        };
                        info!(
                            "Capture: {what} → {}×{} at up to {} fps, {} kbit/s, {} on {}{}.",
                            p.size.0,
                            p.size.1,
                            fps,
                            p.encoder.settings.bitrate / 1000,
                            p.encoder.name,
                            p.gpu.adapter,
                            if p.bgra { ", BGRA input" } else { "" }
                        );
                        hub.set_picture(match target {
                            Target::Monitor => {
                                format!("the desktop streams at {}×{} up to {} fps", p.size.0, p.size.1, fps)
                            }
                            Target::Window { id, .. } => {
                                format!("window {id} streams at {}×{} up to {} fps", p.size.0, p.size.1, fps)
                            }
                        });
                        hub.request_keyframe();
                        retry_at = None;
                        pipeline = Some(p);
                    }
                    Err(e) => {
                        warn!("Capture could not start: {e}. Retrying in 2 s.");
                        retry_at = Some(Instant::now() + Duration::from_secs(2));
                        continue;
                    }
                }
            }
            let p = pipeline.as_mut().expect("built above");
            if let Err(e) = p.step(&hub, &stats, &now_us) {
                warn!("Capture failed: {e}. Rebuilding.");
                pipeline = None;
                retry_at = Some(Instant::now() + Duration::from_secs(1));
            }
        }
        // Quitting: a fitted window goes back to its own size.
        fitter.last_check = None;
        fitter.restore();
    });
}

struct Pipeline {
    /// The 1 ms timer the encoder needs, released with the pipeline (3 s after the last viewer).
    _timer: sill_capture::FineTimer,
    gpu: Gpu,
    monitor: Monitor,
    target: Target,
    /// The share of the source's pixels streamed (Retina/Standard and `--capture-scale`).
    fraction: f64,
    /// The source's size changed (a window was resized): rebuild.
    resized: bool,
    capture: Capture,
    wake: mpsc::Receiver<()>,
    converter: Converter,
    encoder: mf::Encoder,
    size: (u32, u32),
    fps: u32,
    /// Bits per second at 60 fps, as set (the encoder gets `bitrate × fps / 60`).
    bitrate: u32,
    /// The encoder takes BGRA: frames are copied, not converted.
    bgra: bool,
    /// The newest NV12 picture, encoded again when a keyframe is wanted on a still screen.
    last: Option<sill_capture::Texture>,
    /// A new picture waits to be encoded.
    fresh: bool,
    keyframe_wanted: bool,
    next_slot: Instant,
    frame_index: i64,
    parameter_sets: Option<Arc<[u8]>>,
    /// When each frame the encoder holds went in, and when its picture was captured, oldest first.
    in_flight: std::collections::VecDeque<(Instant, Instant)>,
    /// When the picture in `last` was captured.
    last_captured: Option<Instant>,
}

impl Pipeline {
    fn new(
        monitor: &Monitor,
        target: Target,
        fraction: f64,
        fps: u32,
        bitrate: u32,
        bgra: bool,
    ) -> Result<Pipeline, String> {
        let gpu = Gpu::new().map_err(|e| format!("no D3D11 hardware device ({e})"))?;
        let (tx, wake) = mpsc::sync_channel::<()>(1);
        let on_frame = move || {
            let _ = tx.try_send(());
        };
        let capture = match target {
            Target::Monitor => Capture::start(&gpu, monitor, on_frame)
                .map_err(|e| format!("Windows Graphics Capture refused the monitor ({e})"))?,
            Target::Window { id, hwnd } => Capture::start_window(&gpu, hwnd, on_frame)
                .map_err(|e| format!("Windows Graphics Capture refused window {id} ({e})"))?,
        };
        let size = sill_capture::stream_size(capture.width, capture.height, fraction);
        let converter = Converter::new(&gpu, capture.width, capture.height, size.0, size.1)
            .map_err(|e| format!("the GPU video processor refused BGRA to NV12 ({e})"))?;
        let bgra_ok = bgra && size == (capture.width, capture.height);
        let per_stream = (u64::from(bitrate) * u64::from(fps) / 60).clamp(500_000, u64::from(u32::MAX)) as u32;
        let encoder = mf::Encoder::new(
            &gpu.device,
            mf::Settings { width: size.0, height: size.1, fps, bitrate: per_stream, bgra_input: bgra_ok },
        )
        .map_err(|e| format!("no hardware HEVC encoder took {}×{} at {fps} fps ({e})", size.0, size.1))?;
        Ok(Pipeline {
            _timer: sill_capture::FineTimer::new(),
            gpu,
            monitor: monitor.clone(),
            target,
            fraction,
            resized: false,
            capture,
            wake,
            converter,
            encoder,
            size,
            fps,
            bitrate,
            bgra: bgra_ok,
            last: None,
            fresh: false,
            keyframe_wanted: true,
            next_slot: Instant::now(),
            frame_index: 0,
            parameter_sets: None,
            in_flight: std::collections::VecDeque::new(),
            last_captured: None,
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
        if let Some((t, _)) = self.in_flight.front() {
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
                // A resized window (or a new monitor mode): rebuilt at once by the loop.
                self.resized = true;
                return Ok(());
            }
            let t = Instant::now();
            let nv12 = if self.bgra {
                self.converter.copy_bgra(&frame).map_err(|e| format!("BGRA copy: {e}"))?
            } else {
                self.converter.convert(&frame).map_err(|e| format!("BGRA to NV12: {e}"))?
            };
            stats.convert_us.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
            self.last = Some(nv12);
            self.last_captured = Some(frame.captured);
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
        let now = Instant::now();
        // A still screen's picture encoded again (a keyframe) is timed from now, not its capture.
        let captured = if self.fresh { self.last_captured.unwrap_or(now) } else { now };
        self.in_flight.push_back((now, captured));
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
                    let (submitted, captured) = self.in_flight.pop_front().unwrap_or((Instant::now(), Instant::now()));
                    self.publish(hub, stats, now_us, &out.annexb, submitted.elapsed().as_micros() as u64, captured);
                    wait = Duration::ZERO;
                }
                Ok(None) => return Ok(()),
                Err(e) => return Err(format!("encoder output: {e}")),
            }
        }
    }

    fn publish(
        &mut self,
        hub: &Hub,
        stats: &Stats,
        now_us: &impl Fn() -> u64,
        annexb_bytes: &[u8],
        us: u64,
        captured: Instant,
    ) {
        let mut r = annexb::repack(annexb_bytes);
        if self.bgra {
            // The MFT's own RGB → YUV conversion writes limited range (Y 16–235) but its SPS says
            // full range, so the device showed black as 16 and white as 235 (colour chart, rig,
            // 2026-11-27). Say limited, and the device expands it back to 0–255.
            for n in r.parameter_sets.iter_mut() {
                if annexb::nal_type(n) == Some(annexb::SPS) {
                    if let Some(fixed) = sill_encode::sps::set_full_range(n, false) {
                        *n = fixed;
                    }
                }
            }
        }
        if !r.parameter_sets.is_empty() {
            match annexb::kind0(&r.parameter_sets) {
                Some(k0) => {
                    if self.parameter_sets.as_deref() != Some(&k0[..]) {
                        let sps = r.parameter_sets.iter().find(|n| annexb::nal_type(n) == Some(annexb::SPS));
                        match sps.and_then(|n| sill_encode::sps::vui(n)) {
                            Some(v) => info!(
                                "Encoder signals {} ({}).",
                                v.words(),
                                if self.bgra { "BGRA input" } else { "NV12 input" }
                            ),
                            None => warn!("The encoder's SPS did not parse; its colour signalling is unknown."),
                        }
                    }
                    self.parameter_sets = Some(Arc::from(k0));
                }
                None => warn!("The encoder's parameter sets are incomplete; waiting for the next keyframe."),
            }
        }
        if r.au.is_empty() {
            return;
        }
        stats.note_encoded(us, r.au.len());
        stats.note_au_latency(captured.elapsed());
        let Some(ps) = self.parameter_sets.clone() else {
            // A delta before the first parameter sets: nobody could decode it.
            self.keyframe_wanted = true;
            return;
        };
        hub.deliver(
            &Encoded { keyframe: r.keyframe, au: Arc::from(r.au), parameter_sets: ps, captured: Some(captured) },
            now_us(),
        );
    }
}
