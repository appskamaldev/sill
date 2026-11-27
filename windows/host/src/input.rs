//! The input sink (Windows): each session's events through its own `Injector` to `SendInput`, on
//! what streams: the primary monitor, or the streamed window's visible frame (what WGC captures).
//! A streamed window is brought forward on a click, a key or text when it is not in front
//! (upstream `raiseIfInteracting`, at most every 0.5 s); Windows allows that because this process
//! has just injected the device's input, and nothing here bypasses the foreground rules. Nothing
//! goes while the input desktop is not the user's (lock screen, sign-in, UAC), and a session that
//! left gets every key it held released.

use crate::catalog::Catalog;
use sill_input::{win, Injector, Rect};
use sill_net::input::InputSink;
use sill_net::Hub;
use sill_wire::{InputEvent, PointerAction, StreamSource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info};

pub struct DesktopInput {
    hub: Arc<Hub>,
    catalog: Arc<Catalog>,
    sessions: Mutex<HashMap<u64, Injector>>,
    /// The primary monitor's rectangle, read again at most once a second.
    rect: Mutex<Option<(Instant, Rect)>>,
    /// When "input refused: not the user's desktop" was last logged.
    refused_line: Mutex<Option<Instant>>,
    /// When the streamed window was last checked for being in front.
    raised: Mutex<Option<Instant>>,
}

/// An event that needs the window in front (upstream: a click, a key down, text).
fn wants_focus(ev: &InputEvent) -> bool {
    match ev {
        InputEvent::Pointer { action, .. } => matches!(action, PointerAction::LeftDown | PointerAction::RightDown),
        InputEvent::Key { down, .. } => *down,
        InputEvent::Text(_) => true,
        _ => false,
    }
}

impl DesktopInput {
    pub fn new(hub: Arc<Hub>, catalog: Arc<Catalog>) -> DesktopInput {
        DesktopInput {
            hub,
            catalog,
            sessions: Mutex::new(HashMap::new()),
            rect: Mutex::new(None),
            refused_line: Mutex::new(None),
            raised: Mutex::new(None),
        }
    }

    fn monitor_rect(&self) -> Option<Rect> {
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

    /// Where frame fractions land, and the streamed window's HWND if a window streams.
    fn target(&self) -> Option<(Rect, Option<isize>)> {
        match self.hub.source() {
            StreamSource::Window(id) => {
                let h = self.catalog.hwnd(id)?;
                let hw = windows::Win32::Foundation::HWND(h as *mut _);
                let f = sill_catalog::windows_list::frame_of(hw)?;
                let rect = Rect {
                    left: f.left,
                    top: f.top,
                    width: (f.right - f.left).max(1) as u32,
                    height: (f.bottom - f.top).max(1) as u32,
                    dpi_scale: sill_catalog::windows_list::dpi_scale_of(hw),
                };
                Some((rect, Some(h)))
            }
            StreamSource::Desktop => Some((self.monitor_rect()?, None)),
            StreamSource::None => None,
        }
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
        // Nothing streams: input has nowhere to land, but held keys still go up.
        let Some((rect, window)) = self.target() else {
            if let InputEvent::Key { down: false, .. } = ev {
                win::send(&inj.handle(ev, &Rect { left: 0, top: 0, width: 1, height: 1, dpi_scale: 1.0 }));
            }
            return;
        };
        if let Some(h) = window {
            if wants_focus(ev) && !sill_catalog::window_ops::is_foreground(h) {
                let mut last = self.raised.lock().unwrap();
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(500)) {
                    *last = Some(Instant::now());
                    if !sill_catalog::window_ops::bring_forward(h) {
                        debug!("The streamed window could not be brought forward (Windows refused).");
                    }
                }
            }
        }
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
