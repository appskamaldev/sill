//! The stream settings a device can change (kind 17) and the state the host reports (kind 16), as
//! upstream `DeviceSettings.accepted` and `SettingsChoices` decide them: frame rate limit 60 or
//! 120, a bitrate preset (per 60 fps), capture scale 2 (Retina: the monitor's own pixels) or 1
//! (Standard: its points, Windows' DIPs). Every change is answered with the state as it then
//! stands, so a refused field goes back on the device.

use sill_wire::{HostSettingsChange, HostSettingsState, StreamSettings};

pub const MAX_FPS: [i64; 2] = [60, 120];
pub const CAPTURE_SCALE: [f64; 2] = [2.0, 1.0];
/// `QualityPreset`: Low, Efficient, Balanced, High, Pro, Ultra, Extreme.
pub const BITRATES: [i64; 7] = [4_000_000, 8_000_000, 15_000_000, 25_000_000, 40_000_000, 80_000_000, 150_000_000];
/// Changes honoured per second per session (upstream `settingsPerSecond`).
pub const PER_SECOND: usize = 4;

/// What the picture runs at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Video {
    pub max_fps: i64,
    /// Bits per second at 60 fps; the stream gets `bitrate × fps / 60`.
    pub bitrate: i64,
    /// 2 Retina, 1 Standard.
    pub capture_scale: f64,
    pub prioritize_speed: bool,
}

impl Default for Video {
    /// Upstream's home defaults: 60 fps, Balanced, Retina.
    fn default() -> Video {
        Video { max_fps: 60, bitrate: 15_000_000, capture_scale: 2.0, prioritize_speed: true }
    }
}

/// `change` applied to `current` as far as this host takes it, and what it refused, in words.
pub fn accept(current: &Video, c: &HostSettingsChange) -> (Video, Vec<String>) {
    let mut v = *current;
    let mut refused = Vec::new();
    if let Some(f) = c.max_fps {
        if MAX_FPS.contains(&f) {
            v.max_fps = f;
        } else {
            refused.push(format!("frame rate limit {f}"));
        }
    }
    if let Some(b) = c.bitrate {
        if BITRATES.contains(&b) {
            v.bitrate = b;
        } else {
            refused.push(format!("bitrate {b}"));
        }
    }
    if let Some(s) = c.capture_scale {
        if CAPTURE_SCALE.contains(&s) {
            v.capture_scale = s;
        } else {
            refused.push(format!("capture scale {s}"));
        }
    }
    if let Some(p) = c.prioritize_speed {
        v.prioritize_speed = p;
    }
    if c.virtual_display == Some(true) {
        refused.push("virtual display (not on this host)".into());
    }
    if c.direct_wireless.is_some() {
        refused.push("direct wireless (not on this host)".into());
    }
    if c.send_audio == Some(true) {
        refused.push("send audio (not on this host yet)".into());
    }
    (v, refused)
}

/// "frame rate limit 60 → 120, bitrate 15 → 25 Mbps"; empty when nothing changed.
pub fn changes(before: &Video, after: &Video) -> String {
    let mut parts = Vec::new();
    if before.max_fps != after.max_fps {
        parts.push(format!("frame rate limit {} → {}", before.max_fps, after.max_fps));
    }
    if before.bitrate != after.bitrate {
        parts.push(format!("bitrate {} → {} Mbps", mbps(before.bitrate), mbps(after.bitrate)));
    }
    if before.capture_scale != after.capture_scale {
        parts.push(format!("capture scale {} → {}", scale_word(before.capture_scale), scale_word(after.capture_scale)));
    }
    if before.prioritize_speed != after.prioritize_speed {
        parts.push(format!("prioritize speed {} → {}", before.prioritize_speed, after.prioritize_speed));
    }
    parts.join(", ")
}

fn mbps(b: i64) -> String {
    let m = b as f64 / 1e6;
    if m.fract() == 0.0 {
        format!("{m:.0}")
    } else {
        format!("{m:.1}")
    }
}

fn scale_word(s: f64) -> &'static str {
    if s >= 1.5 {
        "Retina"
    } else {
        "Standard"
    }
}

/// Kind 16 for `v`, answering a change's token when there is one.
pub fn state(v: &Video, answering: Option<i64>) -> HostSettingsState {
    HostSettingsState {
        settings: StreamSettings {
            max_fps: v.max_fps,
            bitrate: v.bitrate,
            capture_scale: v.capture_scale,
            prioritize_speed: v.prioritize_speed,
            virtual_display: false,
            direct_wireless: None,
            send_audio: None,
        },
        persistent: false,
        virtual_display_available: false,
        virtual_display_note: None,
        software_encoder: false,
        stream: None,
        answering,
        away: None,
        link: None,
        audio_note: None,
        session_paired: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(f: impl FnOnce(&mut HostSettingsChange)) -> HostSettingsChange {
        let mut c = HostSettingsChange::default();
        f(&mut c);
        c
    }

    #[test]
    fn presets_and_choices_are_taken() {
        let v = Video::default();
        let (n, refused) = accept(
            &v,
            &change(|c| {
                c.max_fps = Some(120);
                c.bitrate = Some(25_000_000);
                c.capture_scale = Some(1.0);
            }),
        );
        assert!(refused.is_empty());
        assert_eq!((n.max_fps, n.bitrate, n.capture_scale), (120, 25_000_000, 1.0));
        assert_eq!(changes(&v, &n), "frame rate limit 60 → 120, bitrate 15 → 25 Mbps, capture scale Retina → Standard");
    }

    #[test]
    fn anything_else_is_refused_and_keeps_the_value() {
        let v = Video::default();
        let (n, refused) = accept(
            &v,
            &change(|c| {
                c.max_fps = Some(90);
                c.bitrate = Some(12_000_000);
                c.capture_scale = Some(1.5);
                c.virtual_display = Some(true);
                c.direct_wireless = Some(true);
                c.send_audio = Some(true);
            }),
        );
        assert_eq!(n, v);
        assert_eq!(refused.len(), 6);
        assert_eq!(changes(&v, &n), "");
    }

    #[test]
    fn the_answer_carries_the_token_and_the_values() {
        let s = state(&Video { max_fps: 120, ..Video::default() }, Some(7));
        assert_eq!(s.answering, Some(7));
        assert_eq!(s.settings.max_fps, 120);
        assert_eq!(s.settings.capture_scale, 2.0);
        let json = serde_json::to_value(state(&Video::default(), None)).unwrap();
        assert!(json.get("answering").is_none(), "no token, no answering: {json}");
    }
}
