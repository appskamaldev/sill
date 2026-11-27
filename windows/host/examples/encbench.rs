//! Encoder bench (development): converts one desktop frame and feeds it to the hardware HEVC
//! encoder at FPS (default 60) for 3 s, whenever the MFT asks for input, and prints the latency
//! from each submit to its access unit. SIZE=720 or 480 scales the frame first.
//!
//!     cargo run --release -p sill-host --example encbench
#[cfg(windows)]
fn main() {
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};
    sill_capture::set_dpi_awareness();
    let _timer = sill_capture::FineTimer::new();
    sill_encode::mf::startup().unwrap();
    let fps: u32 = std::env::var("FPS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let gpu = sill_capture::Gpu::new().unwrap();
    let m = sill_capture::primary_monitor().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    let cap = sill_capture::Capture::start(&gpu, &m, move || {
        let _ = tx.try_send(());
    })
    .unwrap();
    let _ = rx.recv_timeout(Duration::from_millis(500));
    let f = cap.newest().unwrap().expect("a first frame");
    let (w, h): (u32, u32) = match std::env::var("SIZE").ok().as_deref() {
        Some("720") => (1280, 720),
        Some("480") => (854, 480),
        _ => (cap.width, cap.height),
    };
    let mut conv = sill_capture::Converter::new(&gpu, cap.width, cap.height, w, h).unwrap();
    let nv = conv.convert(&f).unwrap();
    let mut enc = sill_encode::mf::Encoder::new(
        &gpu.device,
        sill_encode::mf::Settings { width: w, height: h, fps, bitrate: 15_000_000 },
    )
    .unwrap();
    let period = Duration::from_secs_f64(1.0 / fps as f64);
    let mut sent: VecDeque<Instant> = VecDeque::new();
    let start = Instant::now();
    let mut next = Instant::now();
    let mut i = 0i64;
    let mut lat = Vec::new();
    while start.elapsed() < Duration::from_secs(3) {
        if Instant::now() >= next && enc.ready(Duration::ZERO).unwrap() {
            if i % 30 == 0 {
                enc.force_keyframe();
            }
            enc.submit(&nv, i * 10_000_000 / fps as i64, 10_000_000 / fps as i64).unwrap();
            sent.push_back(Instant::now());
            i += 1;
            next += period;
        }
        while let Some(o) = enc.output(Duration::from_millis(1)).unwrap() {
            let t = sent.pop_front().unwrap();
            lat.push(t.elapsed().as_secs_f64() * 1e3);
            if lat.len() <= 5 || o.clean_point {
                println!(
                    "AU {} B clean {} latency {:.1} ms in flight {}",
                    o.annexb.len(),
                    o.clean_point,
                    lat.last().unwrap(),
                    sent.len()
                );
            }
        }
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "fps {fps}: {} AUs, latency p50 {:.1} p95 {:.1} max {:.1} ms",
        lat.len(),
        lat[lat.len() / 2],
        lat[lat.len() * 95 / 100],
        lat[lat.len() - 1]
    );
}
#[cfg(not(windows))]
fn main() {}
