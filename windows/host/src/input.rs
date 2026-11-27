//! The input sink (Windows): each session's events through its own `Injector` to `SendInput`, on
//! the primary monitor the desktop pipeline streams. Nothing goes while the input desktop is not
//! the user's (lock screen, sign-in, UAC), and a session that left gets every key it held
//! released.

use sill_input::{win, Injector, Rect};
use sill_net::input::InputSink;
use sill_wire::InputEvent;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info};

#[derive(Default)]
pub struct DesktopInput {
    sessions: Mutex<HashMap<u64, Injector>>,
    /// The primary monitor's rectangle, read again at most once a second.
    rect: Mutex<Option<(Instant, Rect)>>,
    /// When "input refused: not the user's desktop" was last logged.
    refused_line: Mutex<Option<Instant>>,
}

impl DesktopInput {
    fn rect(&self) -> Option<Rect> {
        let mut r = self.rect.lock().unwrap();
        if let Some((t, rect)) = *r {
            if t.elapsed() < Duration::from_secs(1) {
                return Some(rect);
            }
        }
        let m = sill_capture::primary_monitor()?;
        let rect = Rect { left: m.left, top: m.top, width: m.width, height: m.height, dpi_scale: m.dpi_scale };
        *r = Some((Instant::now(), rect));
        Some(rect)
    }
}

impl InputSink for DesktopInput {
    fn event(&self, session: u64, literal: bool, ev: &InputEvent) {
        let mut sessions = self.sessions.lock().unwrap();
        let inj =
            sessions.entry(session).or_insert_with(|| if literal { Injector::literal() } else { Injector::new() });
        if inj.literal != literal {
            win::send(&inj.release_all());
            *inj = if literal { Injector::literal() } else { Injector::new() };
        }
        if !win::user_desktop_active() {
            // Locked, signing in, or a UAC prompt: nothing reaches it, and nothing stays held.
            win::send(&inj.release_all());
            let mut last = self.refused_line.lock().unwrap();
            if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                *last = Some(Instant::now());
                info!("Input refused: the screen is locked or another desktop has the input (lock screen, UAC).");
            }
            return;
        }
        let Some(rect) = self.rect() else { return };
        let actions = inj.handle(ev, &rect);
        let sent = win::send(&actions);
        if sent < actions.len() {
            debug!("Windows took {sent} of {} input events (an elevated window may have focus).", actions.len());
        }
    }

    fn left(&self, session: u64) {
        if let Some(mut inj) = self.sessions.lock().unwrap().remove(&session) {
            win::send(&inj.release_all());
        }
    }
}
