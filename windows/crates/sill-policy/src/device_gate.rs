//! Which devices this host serves, by the version in their hello (upstream `DeviceGate.swift`).
//! With the floor at "0" nothing waits and nothing is refused.

use sill_pairing::safe_text;
use sill_wire::{Goodbye, Hello, SillVersion};

/// The oldest Sill for iPhone and iPad this host serves. "0" admits every device.
pub const MINIMUM_DEVICE_VERSION: &str = "0";
/// A refused device's first message must come within this long of `.ready` (seconds).
pub const FIRST_MESSAGE_DEADLINE: f64 = 2.0;
pub const LOOP_REFUSALS: usize = 5;
pub const LOOP_WINDOW: f64 = 60.0;
pub const LOOP_DELAY: f64 = 2.0;
pub const CLOSE_WAIT: f64 = 1.0;

/// The shipped floor as a version.
pub fn floor() -> SillVersion {
    SillVersion::parse(MINIMUM_DEVICE_VERSION).unwrap_or(SillVersion::ZERO)
}

/// A device's version: its hello's `appVersion`, else 0.
pub fn version(hello: Option<&Hello>) -> SillVersion {
    hello.and_then(|h| h.app_version.as_deref()).and_then(SillVersion::parse).unwrap_or(SillVersion::ZERO)
}

pub fn admits(hello: Option<&Hello>, floor: &SillVersion) -> bool {
    version(hello) >= *floor
}

/// Whether the next refusal of a source is slowed.
pub fn slows(refused_in_window: usize) -> bool {
    refused_in_window >= LOOP_REFUSALS
}

/// {"reason":"update", message, "minimumVersion", "reconnect":false}.
pub fn refusal(hello: Option<&Hello>, floor: &SillVersion, mac_name: &str) -> Goodbye {
    Goodbye {
        reason: Goodbye::UPDATE.to_string(),
        message: Some(message(hello, floor, mac_name)),
        minimum_version: Some(floor.to_string()),
        reconnect: Some(false),
    }
}

pub fn message(hello: Option<&Hello>, floor: &SillVersion, mac_name: &str) -> String {
    format!("Update Sill on your {} to keep using {mac_name}. It needs version {floor} or later.", device_word(hello))
}

/// "iPhone" or "iPad" from the start of the hello's cleaned name, else "device".
pub fn device_word(hello: Option<&Hello>) -> &'static str {
    let name = safe_text::label64(hello.and_then(|h| h.device.as_deref()).unwrap_or(""));
    if name.starts_with("iPhone") {
        "iPhone"
    } else if name.starts_with("iPad") {
        "iPad"
    } else {
        "device"
    }
}

pub fn refused_line(hello: Option<&Hello>, endpoint: &str, floor: &SillVersion) -> String {
    let name = safe_text::label64(hello.and_then(|h| h.device.as_deref()).unwrap_or(""));
    let what = match hello {
        Some(h) => {
            let v = safe_text::label(h.app_version.as_deref().unwrap_or(""), 32);
            if v.is_empty() {
                "no version".to_string()
            } else {
                format!("Sill {v}")
            }
        }
        None => "an older Sill".to_string(),
    };
    format!("Refused {} ({what}): needs {floor} or later.", if name.is_empty() { endpoint } else { &name })
}

pub fn count_line(n: usize, source: &str, floor: &SillVersion) -> String {
    format!(
        "Refused {n} more connection{} from {source} in the last minute: too old for this Mac (needs {floor} or later).",
        if n == 1 { "" } else { "s" }
    )
}

/// "Client hello: iPad (iPad14,1), Sill 1.0 (42), protocol 1 (192.168.1.23:52344)".
/// "Sill 0.6.2 (1607)" from a hello; `None` when it names no version.
pub fn app_label(hello: &Hello) -> Option<String> {
    let v = safe_text::label(hello.app_version.as_deref().unwrap_or(""), 32);
    let build = safe_text::label(hello.build.as_deref().unwrap_or(""), 32);
    match (v.is_empty(), build.is_empty()) {
        (true, _) => None,
        (false, true) => Some(format!("Sill {v}")),
        (false, false) => Some(format!("Sill {v} ({build})")),
    }
}

pub fn hello_line(hello: &Hello, endpoint: &str) -> String {
    let name = safe_text::label64(hello.device.as_deref().unwrap_or(""));
    let v = safe_text::label(hello.app_version.as_deref().unwrap_or(""), 32);
    let build = safe_text::label(hello.build.as_deref().unwrap_or(""), 32);
    let mut line = format!("Client hello: {}, ", if name.is_empty() { endpoint } else { &name });
    if v.is_empty() {
        line += "no version";
    } else {
        line += &format!("Sill {v}");
        if !build.is_empty() {
            line += &format!(" ({build})");
        }
    }
    if let Some(p) = hello.protocol {
        line += &format!(", protocol {p}");
    }
    if !name.is_empty() {
        line += &format!(" ({endpoint})");
    }
    line
}
