//! Every JSON payload, in the exact shapes Swift's synthesized `Codable` gives them.
//!
//! Rules (upstream `HostSettings.swift`, measured in `vectors/json.json`): an optional field is
//! left out when `None` and decodes from missing or `null`; unknown keys are ignored; a missing
//! required field fails the whole message. Key order is never relied on (Swift's varies by run).

use crate::swift_json::{int, int_from_value, obj, obj_list, opt_data, opt_int, opt_obj, ser_opt_data};
use serde::de::{Deserializer, Error as _};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

// MARK: - Enums with associated values: {"case":{"_0":…,"label":…}}

/// What the host streams (kind 6, and `WindowList.active`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamSource {
    None,
    /// A window id (CGWindowID on the Mac; the Windows host's own ids).
    Window(u32),
    Desktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerAction {
    Move,
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollPhase {
    Began,
    Ended,
    MomentumBegan,
    MomentumEnded,
}

/// Client → host input (kind 8). Positions are fractions of the streamed frame.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Pointer {
        action: PointerAction,
        x: f64,
        y: f64,
    },
    /// dx/dy: fractions of the frame the content moves, natural-scrolling sign.
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    /// "\n", "\t" and "\u{8}" mean Return, Tab and Backspace.
    Text(String),
    ScrollGesture {
        phase: ScrollPhase,
        x: f64,
        y: f64,
    },
    /// USB HID usage, UIKeyModifierFlags bits.
    Key {
        hid_usage: u16,
        down: bool,
        modifiers: u64,
    },
}

/// The one case of a Swift enum's JSON: the outer object must hold exactly one of `cases` (other
/// keys are ignored), and its value must be an object.
fn one_case<'a, E: serde::de::Error>(v: &'a Value, cases: &[&str]) -> Result<(String, &'a Map<String, Value>), E> {
    let Value::Object(outer) = v else { return Err(E::custom("expected an object")) };
    let present: Vec<&str> = cases.iter().copied().filter(|c| outer.contains_key(*c)).collect();
    if present.len() != 1 {
        return Err(E::custom(format!("expected exactly one of {cases:?}")));
    }
    match &outer[present[0]] {
        Value::Object(inner) => Ok((present[0].to_string(), inner)),
        _ => Err(E::custom("a case's value must be an object")),
    }
}

fn field<'a, E: serde::de::Error>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, E> {
    match m.get(key) {
        Some(Value::Null) | None => Err(E::custom(format!("missing {key}"))),
        Some(v) => Ok(v),
    }
}

fn f64_field<E: serde::de::Error>(m: &Map<String, Value>, key: &str) -> Result<f64, E> {
    field::<E>(m, key)?.as_f64().ok_or_else(|| E::custom(format!("{key} is not a number")))
}

fn int_field<E: serde::de::Error, T: TryFrom<i128>>(m: &Map<String, Value>, key: &str) -> Result<T, E> {
    int_from_value(field::<E>(m, key)?).ok_or_else(|| E::custom(format!("{key} is not an integer in range")))
}

fn typed_field<E: serde::de::Error, T: serde::de::DeserializeOwned>(m: &Map<String, Value>, key: &str) -> Result<T, E> {
    T::deserialize(field::<E>(m, key)?).map_err(|e| E::custom(format!("{key}: {e}")))
}

impl Serialize for StreamSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            StreamSource::None => json!({"none": {}}),
            StreamSource::Desktop => json!({"desktop": {}}),
            StreamSource::Window(id) => json!({"window": {"_0": id}}),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for StreamSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let (case, m) = one_case::<D::Error>(&v, &["none", "window", "desktop"])?;
        Ok(match case.as_str() {
            "none" => StreamSource::None,
            "desktop" => StreamSource::Desktop,
            _ => StreamSource::Window(int_field::<D::Error, u32>(m, "_0")?),
        })
    }
}

impl Serialize for InputEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            InputEvent::Pointer { action, x, y } => json!({"pointer": {"_0": action, "x": x, "y": y}}),
            InputEvent::Scroll { x, y, dx, dy } => json!({"scroll": {"x": x, "y": y, "dx": dx, "dy": dy}}),
            InputEvent::Text(t) => json!({"text": {"_0": t}}),
            InputEvent::ScrollGesture { phase, x, y } => json!({"scrollGesture": {"_0": phase, "x": x, "y": y}}),
            InputEvent::Key { hid_usage, down, modifiers } => {
                json!({"key": {"hidUsage": hid_usage, "down": down, "modifiers": modifiers}})
            }
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for InputEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let (case, m) = one_case::<D::Error>(&v, &["pointer", "scroll", "text", "scrollGesture", "key"])?;
        Ok(match case.as_str() {
            "pointer" => InputEvent::Pointer {
                action: typed_field::<D::Error, _>(m, "_0")?,
                x: f64_field::<D::Error>(m, "x")?,
                y: f64_field::<D::Error>(m, "y")?,
            },
            "scroll" => InputEvent::Scroll {
                x: f64_field::<D::Error>(m, "x")?,
                y: f64_field::<D::Error>(m, "y")?,
                dx: f64_field::<D::Error>(m, "dx")?,
                dy: f64_field::<D::Error>(m, "dy")?,
            },
            "text" => InputEvent::Text(typed_field::<D::Error, _>(m, "_0")?),
            "scrollGesture" => InputEvent::ScrollGesture {
                phase: typed_field::<D::Error, _>(m, "_0")?,
                x: f64_field::<D::Error>(m, "x")?,
                y: f64_field::<D::Error>(m, "y")?,
            },
            _ => InputEvent::Key {
                hid_usage: int_field::<D::Error, u16>(m, "hidUsage")?,
                down: match field::<D::Error>(m, "down")? {
                    Value::Bool(b) => *b,
                    _ => return Err(D::Error::custom("down is not a Bool")),
                },
                modifiers: int_field::<D::Error, u64>(m, "modifiers")?,
            },
        })
    }
}

// MARK: - Structs

/// One window in the list (kind 2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    #[serde(deserialize_with = "int")]
    pub id: u32,
    pub title: String,
    pub app_name: String,
    #[serde(rename = "bundleID")]
    pub bundle_id: String,
    #[serde(deserialize_with = "int")]
    pub width: i64,
    #[serde(deserialize_with = "int")]
    pub height: i64,
}

/// An installed app (kind 5 is a JSON array of these).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    #[serde(rename = "bundleID")]
    pub bundle_id: String,
}

/// Kind 2. Every field after `active` is optional and left out when `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowList {
    pub mac_name: String,
    #[serde(deserialize_with = "obj_list")]
    pub windows: Vec<WindowInfo>,
    pub active: StreamSource,
    #[serde(rename = "launchID", default, skip_serializing_if = "Option::is_none")]
    pub launch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub protocol: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub gestures: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub clipboard: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub key_repeat: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub desktop_displays: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub desktop_display: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_display_names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_console: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_console_view: Option<bool>,
}

impl WindowList {
    /// The required fields only.
    pub fn new(mac_name: impl Into<String>, windows: Vec<WindowInfo>, active: StreamSource) -> WindowList {
        WindowList {
            mac_name: mac_name.into(),
            windows,
            active,
            launch_id: None,
            host_version: None,
            protocol: None,
            gestures: None,
            clipboard: None,
            key_repeat: None,
            desktop_displays: None,
            desktop_display: None,
            desktop_display_names: None,
            locked: None,
            off_console: None,
            off_console_view: None,
        }
    }
}

/// Kind 7.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchApp {
    #[serde(rename = "bundleID")]
    pub bundle_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowAction {
    Close,
    Minimize,
    FullScreen,
}

/// Kind 15.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowCommand {
    #[serde(deserialize_with = "int")]
    pub id: u32,
    pub action: WindowAction,
}

/// Kind 9: the device's panel in points, its text scale and its rate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_cursor: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub fps: Option<i64>,
}

/// Kind 12, once a second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStats {
    #[serde(deserialize_with = "int")]
    pub fps: i64,
    #[serde(deserialize_with = "int")]
    pub frame_age_ms: i64,
    #[serde(deserialize_with = "int")]
    pub rtt_ms: i64,
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub frame_age_max_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub rtt_max_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub audio_behind_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub audio_late: Option<i64>,
}

/// Kind 23, the first message of every session. Every field is optional: `{}` decodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub protocol: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub off_console_view: Option<i64>,
}

/// The settings a device sees (inside kind 16).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettings {
    #[serde(rename = "maxFPS", deserialize_with = "int")]
    pub max_fps: i64,
    /// Bits per second per 60 fps.
    #[serde(deserialize_with = "int")]
    pub bitrate: i64,
    /// 2 = Retina, 1 = Standard.
    pub capture_scale: f64,
    pub prioritize_speed: bool,
    pub virtual_display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_wireless: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_audio: Option<bool>,
}

/// What the pipeline runs (inside kind 16).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningStream {
    #[serde(deserialize_with = "int")]
    pub width: i64,
    #[serde(deserialize_with = "int")]
    pub height: i64,
    #[serde(deserialize_with = "int")]
    pub fps: i64,
    #[serde(deserialize_with = "int")]
    pub mbps: i64,
    pub on_virtual_display: bool,
}

/// Home and away qualities (inside kind 16). Every field is required.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayQuality {
    #[serde(deserialize_with = "int")]
    pub home_bitrate: i64,
    pub home_capture_scale: f64,
    #[serde(deserialize_with = "int")]
    pub away_bitrate: i64,
    pub away_capture_scale: f64,
    pub this_connection_away: bool,
    pub away_running: bool,
}

/// A link that does not keep up (inside kind 16).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkReport {
    /// "behind" or "stalled"; a device reads any other value as keeping up.
    pub state: String,
    #[serde(deserialize_with = "int")]
    pub withheld_per_second: i64,
    #[serde(deserialize_with = "int")]
    pub bitrate: i64,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub carried_kbps: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub suggested_bitrate: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_capture_scale: Option<f64>,
}

/// Kind 16.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSettingsState {
    #[serde(deserialize_with = "obj")]
    pub settings: StreamSettings,
    pub persistent: bool,
    pub virtual_display_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_display_note: Option<String>,
    pub software_encoder: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_obj")]
    pub stream: Option<RunningStream>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub answering: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_obj")]
    pub away: Option<AwayQuality>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_obj")]
    pub link: Option<LinkReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_paired: Option<bool>,
}

/// Kind 17: only the fields one control changed. `{}` decodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSettingsChange {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub token: Option<i64>,
    #[serde(rename = "maxFPS", default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub max_fps: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub bitrate: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prioritize_speed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_display: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_wireless: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_audio: Option<bool>,
}

/// Kind 22: why the host is about to close this session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goodbye {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect: Option<bool>,
}

impl Goodbye {
    pub const REMOVED: &'static str = "removed";
    pub const REMOTE_OFF: &'static str = "remoteOff";
    pub const INTERNET_OFF: &'static str = "internetOff";
    pub const QUIT: &'static str = "quit";
    pub const BUSY: &'static str = "busy";
    pub const PAIRING_REQUIRED: &'static str = "pairingRequired";
    pub const UPDATE: &'static str = "update";
    pub const RESTARTING: &'static str = "restarting";

    pub fn reason(reason: &str) -> Goodbye {
        Goodbye { reason: reason.to_string(), message: None, minimum_version: None, reconnect: None }
    }
}

/// Kind 19: the device's one message on a pairing connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairRequest {
    #[serde(deserialize_with = "int")]
    pub v: i64,
    pub method: String,
    pub proof: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cable: Option<bool>,
}

impl PairRequest {
    pub const VERSION: i64 = 1;
    pub const QR: &'static str = "qr";
    pub const CODE: &'static str = "code";
    pub const ASK: &'static str = "ask";
    pub const CANCEL: &'static str = "cancel";
}

/// Kind 20: the host's answer to kind 19.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(rename = "macID", default, skip_serializing_if = "Option::is_none")]
    pub mac_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recognition_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub tries_left: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl PairResult {
    pub const CODE: &'static str = "code";
    pub const CLOSED: &'static str = "closed";
    pub const EXPIRED: &'static str = "expired";
    pub const STOPPED: &'static str = "stopped";
    pub const BUSY: &'static str = "busy";
    pub const SHOWN: &'static str = "shown";
    pub const OPEN_ON_MAC: &'static str = "openOnMac";
    pub const LOCKED: &'static str = "locked";
    pub const CABLE: &'static str = "cable";
    /// Frozen with the first public build: a later reason comes with a `message`.
    pub const KNOWN_REASONS: [&'static str; 8] =
        ["busy", "closed", "code", "expired", "locked", "openOnMac", "shown", "stopped"];

    /// A refusal: `ok` false with `reason`.
    pub fn refusal(reason: &str) -> PairResult {
        PairResult { ok: false, reason: Some(reason.to_string()), ..PairResult::default() }
    }
}

/// One way to reach the host from afar (inside `MacInfo`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacAddress {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub port: Option<i64>,
    /// "vpn", "lan" or "internet".
    pub kind: String,
    pub via: String,
}

/// Inside kind 18: who this host is and how to reach it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacInfo {
    #[serde(deserialize_with = "int")]
    pub v: i64,
    #[serde(rename = "macID")]
    pub mac_id: String,
    pub name: String,
    pub issued_at: f64,
    pub remote_access: bool,
    #[serde(deserialize_with = "int")]
    pub remote_port: i64,
    pub internet: bool,
    #[serde(deserialize_with = "obj_list")]
    pub addresses: Vec<MacAddress>,
}

/// Kind 18: base64 of MacInfo's exact JSON bytes, the X9.63 public key, and a DER ECDSA
/// P-256/SHA-256 signature over those bytes (signing and checking: sill-pairing).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedMacInfo {
    pub info: String,
    pub key: String,
    pub sig: String,
}

/// Kind 28.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackpadGesture {
    pub gesture: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub fingers: Option<i64>,
}

/// Kind 32, both ways.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Standard base64 on the wire.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "opt_data",
        serialize_with = "ser_opt_data"
    )]
    pub image: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<bool>,
}

impl Clipboard {
    pub const MAX_TEXT_BYTES: usize = 256 * 1024;
}

/// Kind 26. Every field optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MacPointer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "opt_int")]
    pub seen: Option<i64>,
}

/// `JSONEncoder` output for a payload (no failure for these types).
pub fn encode<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("payload types always serialize")
}

/// `JSONDecoder` for an object payload: `None` for anything Swift would refuse.
pub fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Option<T> {
    let v: Value = serde_json::from_slice(data).ok()?;
    if !v.is_object() {
        return None;
    }
    T::deserialize(v).ok()
}

/// `JSONDecoder` for an array of objects (kind 5's `[AppInfo]`).
pub fn decode_list<T: serde::de::DeserializeOwned>(data: &[u8]) -> Option<Vec<T>> {
    let v: Value = serde_json::from_slice(data).ok()?;
    match &v {
        Value::Array(items) if items.iter().all(Value::is_object) => Vec::<T>::deserialize(v).ok(),
        _ => None,
    }
}
