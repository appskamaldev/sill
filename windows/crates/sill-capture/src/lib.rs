//! Desktop capture for the Windows host (ADR-4): one hardware D3D11 device shared with the
//! encoder, Windows Graphics Capture of a monitor (cursor off, no yellow border on Windows 11),
//! and BGRA to NV12 on the GPU (`ID3D11VideoProcessor`, full range BT.709, as the Mac's 420f).
//! Frames arrive only when something on the monitor changes; the caller keeps the last NV12
//! texture to encode again when a keyframe is wanted on a still screen. Nothing here copies a
//! frame to the CPU.
//!
//! The size rules (`stream_size`) are portable and tested everywhere.

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

/// The streamed size for a source of `width`×`height` pixels at `scale` (upstream CaptureScale):
/// never upscaled (`scale` above 1 or not a number counts as 1), each side even (NV12 and HEVC
/// 4:2:0 need it) and at least 2, aspect kept by scaling both sides alike.
pub fn stream_size(width: u32, height: u32, scale: f64) -> (u32, u32) {
    let s = if scale.is_finite() && scale > 0.0 { scale.min(1.0) } else { 1.0 };
    let side = |v: u32| ((((f64::from(v) * s).floor() as u32) / 2) * 2).max(2);
    (side(width), side(height))
}

/// The frame rate to encode at: the highest a viewer asked for (the top when none said), within
/// 24 and the smaller of the frame rate limit (60 or 120) and the monitor's refresh rate.
pub fn stream_fps(asked: Option<i64>, limit: i64, refresh_hz: u32) -> u32 {
    let limit = limit.clamp(24, 240) as u32;
    let top = refresh_hz.min(limit).max(24);
    let asked = asked.filter(|f| *f > 0).map_or(top, |f| f.min(i64::from(u32::MAX)) as u32);
    asked.clamp(24, top)
}

/// The share of the monitor's pixels to stream for capture scale `capture_scale` (2 Retina, 1
/// Standard) on a monitor at `dpi_scale` (1.5 at 144 dpi): Standard is its points (DIPs), Retina
/// its pixels, never more.
pub fn scale_fraction(capture_scale: f64, dpi_scale: f64) -> f64 {
    let dpi = if dpi_scale.is_finite() && dpi_scale >= 1.0 { dpi_scale } else { 1.0 };
    let cs = if capture_scale.is_finite() && capture_scale > 0.0 { capture_scale } else { 2.0 };
    (cs / dpi).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_upscaled_and_even() {
        assert_eq!(stream_size(1920, 1200, 1.0), (1920, 1200));
        assert_eq!(stream_size(1920, 1200, 2.0), (1920, 1200));
        assert_eq!(stream_size(2880, 1800, 0.5), (1440, 900));
        assert_eq!(stream_size(1366, 767, 1.0), (1366, 766));
        assert_eq!(stream_size(1001, 1001, 0.5), (500, 500));
        assert_eq!(stream_size(3, 3, 0.1), (2, 2));
        assert_eq!(stream_size(1920, 1080, f64::NAN), (1920, 1080));
        assert_eq!(stream_size(1920, 1080, -1.0), (1920, 1080));
    }

    #[test]
    fn fps_within_24_the_limit_and_the_refresh_rate() {
        assert_eq!(stream_fps(None, 60, 60), 60);
        assert_eq!(stream_fps(Some(120), 60, 60), 60);
        assert_eq!(stream_fps(Some(120), 60, 144), 60);
        assert_eq!(stream_fps(Some(120), 120, 144), 120);
        assert_eq!(stream_fps(Some(120), 120, 60), 60);
        assert_eq!(stream_fps(None, 120, 144), 120);
        assert_eq!(stream_fps(Some(30), 60, 60), 30);
        assert_eq!(stream_fps(Some(10), 60, 60), 24);
        assert_eq!(stream_fps(Some(60), 60, 48), 48);
        assert_eq!(stream_fps(Some(0), 60, 60), 60);
        assert_eq!(stream_fps(Some(-5), 60, 60), 60);
        assert_eq!(stream_fps(Some(60), 60, 0), 24);
    }

    #[test]
    fn standard_is_points_retina_is_pixels() {
        assert_eq!(scale_fraction(2.0, 1.0), 1.0);
        assert_eq!(scale_fraction(1.0, 1.0), 1.0);
        assert_eq!(scale_fraction(1.0, 1.5), 1.0 / 1.5);
        assert_eq!(scale_fraction(2.0, 1.5), 1.0);
        assert_eq!(scale_fraction(1.0, 2.0), 0.5);
        assert_eq!(scale_fraction(f64::NAN, 1.25), 1.0);
        assert_eq!(scale_fraction(1.0, 0.0), 1.0);
    }
}
