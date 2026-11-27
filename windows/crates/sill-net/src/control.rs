//! The local control protocol the Sill app (tray flyout and window) speaks to the host over a
//! per-user named pipe: one JSON object per line each way, a request then its reply. Every request
//! is untrusted input: unknown commands, wrong types and out-of-range values are refused with an
//! error, never acted on. Pairing codes go out only in a `status` while a window is open, and are
//! never logged.
//!
//! Requests: `{"cmd":"status"}`, `{"cmd":"pair"}`, `{"cmd":"cancelPairing"}`,
//! `{"cmd":"unpair","key":K}`, `{"cmd":"disconnect","key":K}`, `{"cmd":"pause"}`, `{"cmd":"resume"}`,
//! `{"cmd":"set", "requirePairing"?, "bitrate"?, "maxFps"?, "captureScale"?, "startAtLogin"?}`,
//! `{"cmd":"logs","lines":N}`, `{"cmd":"quit"}`. Replies: `{"ok":true, …}` or `{"ok":false,"error":E}`.

use crate::host::Host;
use crate::server::Running;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// What only the host binary can do (start at sign-in, quitting the process).
pub trait Platform: Send + Sync {
    fn start_at_login(&self) -> Option<bool>;
    fn set_start_at_login(&self, on: bool) -> Result<(), String>;
    /// Ends the host (goodbye to every device first).
    fn quit(&self);
}

/// The longest request line taken.
pub const MAX_LINE: usize = 4096;

pub struct Control {
    pub host: Arc<Host>,
    pub running: Arc<Running>,
    pub platform: Arc<dyn Platform>,
}

fn err(e: impl Into<String>) -> Value {
    json!({"ok": false, "error": e.into()})
}

/// The QR code of `text` as rows of '1' (dark) and '0', with no quiet zone.
pub fn qr_rows(text: &str) -> Option<Vec<String>> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()?;
    let w = code.width();
    let colors = code.to_colors();
    Some(
        colors
            .chunks(w)
            .map(|row| row.iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect())
            .collect(),
    )
}

fn source_word(s: sill_wire::StreamSource) -> String {
    match s {
        sill_wire::StreamSource::Desktop => "desktop".into(),
        sill_wire::StreamSource::Window(id) => format!("window:{id}"),
        sill_wire::StreamSource::None => "none".into(),
    }
}

impl Control {
    pub fn status(&self) -> Value {
        let h = &self.host;
        let live: Vec<crate::host::Live> = h.live.lock().unwrap().values().cloned().collect();
        let sessions: Vec<Value> = live
            .iter()
            .map(|l| json!({"name": l.name, "key": l.key, "peer": l.peer, "door": l.door, "app": l.app, "since": l.since}))
            .collect();
        let devices: Vec<Value> = h
            .paired()
            .iter()
            .map(|d| {
                json!({
                    "key": d.fingerprint, "name": d.name, "model": d.model, "app": d.app,
                    "pairedAt": d.paired_at, "method": d.method,
                    "connected": live.iter().any(|l| l.key == d.fingerprint),
                })
            })
            .collect();
        let addresses: Vec<Value> = self
            .running
            .addresses()
            .iter()
            .filter(|ip| !ip.is_loopback())
            .map(|ip| {
                let kind = match crate::bind::classify(*ip) {
                    crate::bind::AddrClass::Tailscale => "tailscale",
                    _ => "lan",
                };
                json!({"ip": ip.to_string(), "kind": kind})
            })
            .collect();
        let pairing = if h.pairing_open() {
            h.offer.lock().unwrap().as_ref().map(|(o, ends)| {
                json!({
                    "code": sill_pairing::code::grouped(&o.code),
                    "link": o.link,
                    "endsAt": ends,
                    "qr": qr_rows(&o.link),
                })
            })
        } else {
            None
        };
        let v = h.hub.video();
        let s = h.last_second.lock().unwrap().clone();
        json!({
            "ok": true,
            "name": h.config.name,
            "macId": h.mac_id,
            "version": env!("CARGO_PKG_VERSION"),
            "now": crate::wall_clock(),
            "paused": h.paused.load(Ordering::Relaxed),
            "source": source_word(h.hub.source()),
            "picture": h.hub.picture(),
            "sessions": sessions,
            "devices": devices,
            "addresses": addresses,
            "remotePort": self.running.remote_port,
            "pairing": pairing,
            "settings": {
                "requirePairing": h.require_pairing(),
                "bitrate": v.bitrate,
                "maxFps": v.max_fps,
                "captureScale": v.capture_scale,
                "startAtLogin": self.platform.start_at_login(),
            },
            "stats": {"fps": s.fps, "kbps": s.kbps, "latencyMs": s.latency_ms, "cpu": s.cpu_percent, "memoryMb": s.memory_mb},
        })
    }

    /// One request line to its reply.
    pub fn handle(&self, line: &str) -> Value {
        if line.len() > MAX_LINE {
            return err("request too long");
        }
        let Ok(Value::Object(req)) = serde_json::from_str::<Value>(line) else { return err("not a JSON object") };
        let Some(cmd) = req.get("cmd").and_then(Value::as_str) else { return err("no cmd") };
        let key = || req.get("key").and_then(Value::as_str).filter(|k| (6..=64).contains(&k.len()));
        match cmd {
            "status" => self.status(),
            "pair" => match self.host.open_pairing() {
                Ok(_) => {
                    tracing::info!("Pairing window opened from the Sill app.");
                    self.status()
                }
                Err(e) => err(e.to_string()),
            },
            "cancelPairing" => {
                self.host.close_pairing();
                tracing::info!("Pairing window closed from the Sill app.");
                json!({"ok": true})
            }
            "unpair" => match key() {
                Some(k) => match self.host.unpair(k) {
                    Ok(gone) if !gone.is_empty() => {
                        tracing::info!(
                            "Unpaired {} from the Sill app.",
                            gone.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")
                        );
                        json!({"ok": true})
                    }
                    Ok(_) => err("no paired device has that key"),
                    Err(e) => err(e.to_string()),
                },
                None => err("key: 6 to 64 characters"),
            },
            "disconnect" => match key() {
                Some(k) => {
                    if self.host.disconnect(k) {
                        json!({"ok": true})
                    } else {
                        err("that device is not connected")
                    }
                }
                None => err("key: 6 to 64 characters"),
            },
            "pause" => {
                self.host.paused.store(true, Ordering::Relaxed);
                for l in self.host.live.lock().unwrap().values() {
                    if let Some(fp) = sill_pairing::base64url::decode(&l.key) {
                        let _ = self.host.kicked.send(fp);
                    }
                }
                tracing::info!("Hosting paused from the Sill app.");
                json!({"ok": true})
            }
            "resume" => {
                self.host.paused.store(false, Ordering::Relaxed);
                tracing::info!("Hosting resumed from the Sill app.");
                json!({"ok": true})
            }
            "set" => self.set(&req),
            "logs" => {
                let n = req.get("lines").and_then(Value::as_u64).unwrap_or(200).clamp(1, 2000) as usize;
                let path = self.host.store.dir().join("logs").join("host.log");
                let text = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                let tail = &lines[lines.len().saturating_sub(n)..];
                json!({"ok": true, "lines": tail})
            }
            "quit" => {
                tracing::info!("Quit from the Sill app.");
                self.platform.quit();
                json!({"ok": true})
            }
            _ => err("unknown cmd"),
        }
    }

    fn set(&self, req: &serde_json::Map<String, Value>) -> Value {
        if let Some(v) = req.get("requirePairing") {
            let Some(on) = v.as_bool() else { return err("requirePairing: true or false") };
            if let Err(e) = self.host.set_require_pairing(on) {
                return err(e.to_string());
            }
        }
        if let Some(v) = req.get("startAtLogin") {
            let Some(on) = v.as_bool() else { return err("startAtLogin: true or false") };
            if let Err(e) = self.platform.set_start_at_login(on) {
                return err(e);
            }
        }
        let change = sill_wire::HostSettingsChange {
            max_fps: req.get("maxFps").and_then(Value::as_i64),
            bitrate: req.get("bitrate").and_then(Value::as_i64),
            capture_scale: req.get("captureScale").and_then(Value::as_f64),
            ..Default::default()
        };
        let before = self.host.hub.video();
        let (after, refused) = crate::settings::accept(&before, &change);
        if !refused.is_empty() {
            return err(format!("refused: {}", refused.join(", ")));
        }
        if after != before {
            tracing::info!("Settings from the Sill app: {}", crate::settings::changes(&before, &after));
            self.host.hub.settings.send_replace(after);
        }
        json!({"ok": true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_of_a_pairing_link() {
        let rows = qr_rows("sill://pair?v=1&m=V9FZXNXN5T6883YH&k=2l_-17UujIQP0aCrZRG5SPfwWkXutRDSERUFZs8jE6Q").unwrap();
        assert!(rows.len() >= 21 && rows.len() % 4 == 1, "a QR version's width");
        assert!(rows.iter().all(|r| r.len() == rows.len() && r.chars().all(|c| c == '0' || c == '1')));
        // The finder patterns' corners are dark.
        assert!(rows[0].starts_with("1111111") && rows[0].ends_with("1111111"));
    }
}
