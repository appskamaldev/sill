//! Input for the Windows host (PROTOCOL.md §9): kind 8 events to the actions `SendInput` takes.
//! The decisions are portable and tested here, and `win` (Windows) sends them.
//!
//! - Pointer: `x, y` in 0…1 of the frame land on the captured monitor's rectangle, in physical
//!   pixels of the virtual desktop. A move with a button held is a drag, because Windows sees the
//!   button down.
//! - Scroll: `dx, dy` (fractions of the frame the content moves, natural sign) become wheel
//!   units, carrying the remainder so slow scrolls still move.
//! - Text: UTF-16 units as Unicode key events; "\n", "\t" and "\u{8}" are Return, Tab and
//!   Backspace.
//! - Keys: HID usage to scan code. The modifiers follow the event's flags on every key down.
//!   A client that sent `pcKeys: 1` in its hello sends literal PC keys (Command is the Windows
//!   key, Control is Ctrl, Option is Alt). For any other client the Mac habits are kept: Command →
//!   Ctrl, Control → Ctrl, Option → Alt, and Command+Space taps the Windows key (search).
//! - Every key this device holds down is released when it leaves.

pub mod keymap;
#[cfg(windows)]
pub mod win;

use keymap::Scan;
use sill_wire::{InputEvent, PointerAction};
use std::collections::BTreeSet;

/// UIKeyModifierFlags bits.
pub const SHIFT: u64 = 1 << 17;
pub const CONTROL: u64 = 1 << 18;
pub const OPTION: u64 = 1 << 19;
pub const COMMAND: u64 = 1 << 20;

/// Wheel units per point the content moves (Windows' 120 per notch over about 50 points).
pub const WHEEL_PER_POINT: f64 = 2.4;
/// The longest text event typed (anything longer is cut there).
pub const MAX_TEXT_UNITS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// What `SendInput` gets, one event each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// To this physical pixel of the virtual desktop.
    MoveTo { x: i32, y: i32 },
    Button { button: Button, down: bool },
    /// Wheel units, positive up (vertical) or right (horizontal).
    Wheel { delta: i32, horizontal: bool },
    Key { scan: Scan, down: bool },
    Unicode { unit: u16, down: bool },
}

/// The streamed monitor in physical pixels of the virtual desktop, and its scale (points to pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f64,
}

impl Rect {
    /// The pixel at frame fraction `(x, y)`; `None` for a position that is not a number.
    pub fn at(&self, x: f64, y: f64) -> Option<(i32, i32)> {
        if !x.is_finite() || !y.is_finite() || self.width == 0 || self.height == 0 {
            return None;
        }
        let px = |f: f64, origin: i32, len: u32| origin + ((f.clamp(0.0, 1.0) * f64::from(len)).floor() as i32).min(len as i32 - 1);
        Some((px(x, self.left, self.width), px(y, self.top, self.height)))
    }

    fn points(&self, pixels: u32) -> f64 {
        let s = if self.dpi_scale.is_finite() && self.dpi_scale >= 1.0 { self.dpi_scale } else { 1.0 };
        f64::from(pixels) / s
    }
}

/// One device's input state: keys it holds and the scroll remainder.
#[derive(Debug, Default)]
pub struct Injector {
    /// The client sends literal PC keys (hello `pcKeys` ≥ 1, a fork extension).
    pub literal: bool,
    /// Keys pressed for the device, by scan code.
    held: BTreeSet<(u16, bool)>,
    /// Modifiers pressed because the event's flags said so.
    flag_mods: BTreeSet<(u16, bool)>,
    /// Space went down as Command+Space (the Windows key was tapped); its up is swallowed.
    space_as_search: bool,
    wheel_rest: (f64, f64),
}

fn wanted(flags: u64, literal: bool) -> Vec<Scan> {
    let mut w = Vec::new();
    let ctrl = if literal { CONTROL } else { COMMAND | CONTROL };
    if flags & ctrl != 0 {
        w.push(keymap::LEFT_CTRL);
    }
    if literal && flags & COMMAND != 0 {
        w.push(keymap::LEFT_WIN);
    }
    if flags & OPTION != 0 {
        w.push(keymap::LEFT_ALT);
    }
    if flags & SHIFT != 0 {
        w.push(keymap::LEFT_SHIFT);
    }
    w
}

fn is_modifier_usage(u: u16) -> bool {
    (keymap::HID_LEFT_CTRL..=keymap::HID_RIGHT_GUI).contains(&u)
}

fn key(scan: Scan, down: bool) -> Action {
    Action::Key { scan, down }
}

impl Injector {
    pub fn new() -> Injector {
        Injector::default()
    }

    /// For a client that sends literal PC keys (`pcKeys`).
    pub fn literal() -> Injector {
        Injector { literal: true, ..Injector::default() }
    }

    /// The actions for one event on `rect`.
    pub fn handle(&mut self, ev: &InputEvent, rect: &Rect) -> Vec<Action> {
        match ev {
            InputEvent::Pointer { action, x, y } => {
                let Some((px, py)) = rect.at(*x, *y) else { return Vec::new() };
                let mut a = vec![Action::MoveTo { x: px, y: py }];
                let b = match action {
                    PointerAction::Move => None,
                    PointerAction::LeftDown => Some((Button::Left, true)),
                    PointerAction::LeftUp => Some((Button::Left, false)),
                    PointerAction::RightDown => Some((Button::Right, true)),
                    PointerAction::RightUp => Some((Button::Right, false)),
                };
                if let Some((button, down)) = b {
                    a.push(Action::Button { button, down });
                }
                a
            }
            InputEvent::Scroll { x, y, dx, dy } => self.scroll(rect, *x, *y, *dx, *dy),
            InputEvent::ScrollGesture { .. } => Vec::new(),
            InputEvent::Text(t) => text(t),
            InputEvent::Key { hid_usage, down, modifiers } => self.key(*hid_usage, *down, *modifiers),
        }
    }

    fn scroll(&mut self, rect: &Rect, x: f64, y: f64, dx: f64, dy: f64) -> Vec<Action> {
        if !dx.is_finite() || !dy.is_finite() {
            return Vec::new();
        }
        let mut a = Vec::new();
        if let Some((px, py)) = rect.at(x, y) {
            a.push(Action::MoveTo { x: px, y: py });
        }
        // Content moving down (dy > 0) is the wheel turned up (positive); content moving left
        // (dx < 0) is the wheel turned right (positive).
        let v = dy.clamp(-1.0, 1.0) * rect.points(rect.height) * WHEEL_PER_POINT + self.wheel_rest.1;
        let h = -dx.clamp(-1.0, 1.0) * rect.points(rect.width) * WHEEL_PER_POINT + self.wheel_rest.0;
        let (vi, hi) = (v.trunc(), h.trunc());
        self.wheel_rest = (h - hi, v - vi);
        if vi != 0.0 {
            a.push(Action::Wheel { delta: vi as i32, horizontal: false });
        }
        if hi != 0.0 {
            a.push(Action::Wheel { delta: hi as i32, horizontal: true });
        }
        a
    }

    fn press(&mut self, s: Scan, a: &mut Vec<Action>) {
        if self.held.insert((s.code, s.extended)) {
            a.push(key(s, true));
        }
    }

    fn release(&mut self, s: Scan, a: &mut Vec<Action>) {
        if self.held.remove(&(s.code, s.extended)) {
            a.push(key(s, false));
        }
    }

    /// Presses the modifiers `flags` name that are not down, releases the ones pressed for an
    /// earlier event's flags that it no longer names.
    fn sync_modifiers(&mut self, flags: u64, a: &mut Vec<Action>) {
        let want: BTreeSet<(u16, bool)> = wanted(flags, self.literal).iter().map(|s| (s.code, s.extended)).collect();
        let stale: Vec<(u16, bool)> = self.flag_mods.difference(&want).copied().collect();
        for (code, extended) in stale {
            self.flag_mods.remove(&(code, extended));
            self.release(Scan { code, extended }, a);
        }
        for &(code, extended) in &want {
            if !self.held.contains(&(code, extended)) {
                self.flag_mods.insert((code, extended));
                self.press(Scan { code, extended }, a);
            }
        }
    }

    fn key(&mut self, usage: u16, down: bool, flags: u64) -> Vec<Action> {
        let mut a = Vec::new();
        if is_modifier_usage(usage) {
            // A modifier key itself: Command (GUI) is Ctrl for a Mac-habit client, the Windows
            // key for a literal one.
            let s = match usage {
                keymap::HID_LEFT_GUI if !self.literal => keymap::LEFT_CTRL,
                keymap::HID_RIGHT_GUI if !self.literal => keymap::RIGHT_CTRL,
                u => keymap::scan(u).expect("modifier usages all map"),
            };
            if down {
                self.press(s, &mut a);
            } else {
                self.flag_mods.remove(&(s.code, s.extended));
                self.release(s, &mut a);
            }
            return a;
        }
        if !self.literal && usage == keymap::HID_SPACE && down && flags & COMMAND != 0 {
            // Command+Space: Windows search. Ctrl is let go first, so the Windows key goes alone.
            for c in [keymap::LEFT_CTRL, keymap::RIGHT_CTRL] {
                self.flag_mods.remove(&(c.code, c.extended));
                self.release(c, &mut a);
            }
            a.push(key(keymap::LEFT_WIN, true));
            a.push(key(keymap::LEFT_WIN, false));
            self.space_as_search = true;
            return a;
        }
        if usage == keymap::HID_SPACE && !down && self.space_as_search {
            self.space_as_search = false;
            return a;
        }
        let Some(s) = keymap::scan(usage) else { return a };
        if down {
            self.sync_modifiers(flags, &mut a);
            // A repeat of a key already down goes again (the device's own key repeat).
            self.held.remove(&(s.code, s.extended));
            self.press(s, &mut a);
        } else {
            self.release(s, &mut a);
            if self.held.iter().all(|h| self.flag_mods.contains(h)) {
                // Nothing but flag modifiers left: let them go with the last key.
                self.sync_modifiers(0, &mut a);
            }
        }
        a
    }

    /// Everything this device holds, released (it left, or the screen locked).
    pub fn release_all(&mut self) -> Vec<Action> {
        let mut a = Vec::new();
        let held: Vec<(u16, bool)> = self.held.iter().rev().copied().collect();
        for (code, extended) in held {
            a.push(key(Scan { code, extended }, false));
        }
        self.held.clear();
        self.flag_mods.clear();
        self.space_as_search = false;
        self.wheel_rest = (0.0, 0.0);
        a
    }
}

/// A text event as key events: Return, Tab and Backspace as keys, the rest as Unicode units (a
/// character outside the BMP as its two surrogates), at most `MAX_TEXT_UNITS`.
pub fn text(t: &str) -> Vec<Action> {
    let mut a = Vec::new();
    let mut units = 0;
    for ch in t.chars() {
        let special = match ch {
            '\n' | '\r' => Some(keymap::ENTER),
            '\t' => Some(keymap::TAB),
            '\u{8}' => Some(keymap::BACKSPACE),
            _ => None,
        };
        let mut buf = [0u16; 2];
        let enc = ch.encode_utf16(&mut buf);
        units += enc.len();
        if units > MAX_TEXT_UNITS {
            break;
        }
        if let Some(s) = special {
            a.push(key(s, true));
            a.push(key(s, false));
            continue;
        }
        if ch.is_control() {
            continue;
        }
        for &u in enc.iter() {
            a.push(Action::Unicode { unit: u, down: true });
        }
        for &u in enc.iter() {
            a.push(Action::Unicode { unit: u, down: false });
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: Rect = Rect { left: 0, top: 0, width: 1920, height: 1200, dpi_scale: 1.0 };

    fn keys(a: &[Action]) -> Vec<(u16, bool, bool)> {
        a.iter()
            .filter_map(|x| match x {
                Action::Key { scan, down } => Some((scan.code, scan.extended, *down)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn pointer_lands_on_the_monitor_in_pixels() {
        let mut i = Injector::new();
        let r = Rect { left: -1920, top: 100, width: 1920, height: 1080, dpi_scale: 1.0 };
        assert_eq!(
            i.handle(&InputEvent::Pointer { action: PointerAction::LeftDown, x: 0.5, y: 0.25 }, &r),
            vec![Action::MoveTo { x: -960, y: 370 }, Action::Button { button: Button::Left, down: true }]
        );
        // Clamped to the last pixel, never off the monitor.
        assert_eq!(
            i.handle(&InputEvent::Pointer { action: PointerAction::Move, x: 1.0, y: 7.0 }, &r),
            vec![Action::MoveTo { x: -1, y: 1179 }]
        );
        assert!(i.handle(&InputEvent::Pointer { action: PointerAction::Move, x: f64::NAN, y: 0.0 }, &r).is_empty());
    }

    #[test]
    fn scroll_carries_the_remainder_with_natural_sign() {
        let mut i = Injector::new();
        // 1/1200 of the frame is 1 point: 2.4 units, then 2.4 more (4.8 → 4, rest 0.8).
        let one_point = 1.0 / 1200.0;
        let a = i.handle(&InputEvent::Scroll { x: 0.5, y: 0.5, dx: 0.0, dy: one_point }, &RECT);
        assert_eq!(a[1], Action::Wheel { delta: 2, horizontal: false });
        let a = i.handle(&InputEvent::Scroll { x: 0.5, y: 0.5, dx: 0.0, dy: one_point }, &RECT);
        assert_eq!(a[1], Action::Wheel { delta: 2, horizontal: false });
        let a = i.handle(&InputEvent::Scroll { x: 0.5, y: 0.5, dx: 0.0, dy: one_point }, &RECT);
        assert_eq!(a[1], Action::Wheel { delta: 3, horizontal: false }, "the carried 0.8 adds up");
        // Content moving up (dy < 0) is the wheel down; content moving left is the wheel right.
        let a = i.handle(&InputEvent::Scroll { x: 0.5, y: 0.5, dx: -0.01, dy: -0.01 }, &RECT);
        assert!(matches!(a[1], Action::Wheel { delta, horizontal: false } if delta < 0));
        assert!(matches!(a[2], Action::Wheel { delta, horizontal: true } if delta > 0));
        assert!(i.handle(&InputEvent::Scroll { x: 0.5, y: 0.5, dx: f64::INFINITY, dy: 0.0 }, &RECT).is_empty());
    }

    #[test]
    fn command_c_is_ctrl_c_and_lets_go_after() {
        let mut i = Injector::new();
        // The device sends C with the Command flag only (no separate Command key event).
        let down = i.handle(&InputEvent::Key { hid_usage: 0x06, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&down), vec![(0x1D, false, true), (0x2E, false, true)]);
        let up = i.handle(&InputEvent::Key { hid_usage: 0x06, down: false, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&up), vec![(0x2E, false, false), (0x1D, false, false)]);
    }

    #[test]
    fn a_held_command_key_is_ctrl_until_it_goes_up() {
        let mut i = Injector::new();
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_LEFT_GUI, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x1D, false, true)]);
        // V with Command: Ctrl is down already, not pressed twice.
        let a = i.handle(&InputEvent::Key { hid_usage: 0x19, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x2F, false, true)]);
        let a = i.handle(&InputEvent::Key { hid_usage: 0x19, down: false, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x2F, false, false)], "the Command key still holds Ctrl");
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_LEFT_GUI, down: false, modifiers: 0 }, &RECT);
        assert_eq!(keys(&a), vec![(0x1D, false, false)]);
    }

    #[test]
    fn option_shift_and_arrows() {
        let mut i = Injector::new();
        let a = i.handle(&InputEvent::Key { hid_usage: 0x50, down: true, modifiers: OPTION | SHIFT }, &RECT);
        assert_eq!(keys(&a), vec![(0x2A, false, true), (0x38, false, true), (0x4B, true, true)]);
    }

    #[test]
    fn literal_pc_keys_are_taken_as_they_come() {
        let mut i = Injector::literal();
        // The Windows key itself, then E with it held: Win+E (File Explorer).
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_LEFT_GUI, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x5B, true, true)]);
        let a = i.handle(&InputEvent::Key { hid_usage: 0x08, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x12, false, true)], "Win is held already, nothing more");
        // Ctrl+C from the flag alone is Ctrl, not Win.
        let mut i = Injector::literal();
        let a = i.handle(&InputEvent::Key { hid_usage: 0x06, down: true, modifiers: CONTROL }, &RECT);
        assert_eq!(keys(&a), vec![(0x1D, false, true), (0x2E, false, true)]);
        // The command flag on its own is the Windows key held.
        let mut i = Injector::literal();
        let a = i.handle(&InputEvent::Key { hid_usage: 0x07, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x5B, true, true), (0x20, false, true)], "Win+D");
        // No Command+Space mapping: Space goes as Space with Win held.
        let mut i = Injector::literal();
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_SPACE, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x5B, true, true), (0x39, false, true)]);
        // Ctrl+Shift+Esc (Task Manager), as the client offers it instead of Ctrl+Alt+Del.
        let mut i = Injector::literal();
        let a = i.handle(&InputEvent::Key { hid_usage: 0x29, down: true, modifiers: CONTROL | SHIFT }, &RECT);
        assert_eq!(keys(&a), vec![(0x1D, false, true), (0x2A, false, true), (0x01, false, true)]);
    }

    #[test]
    fn command_space_taps_the_windows_key() {
        let mut i = Injector::new();
        i.handle(&InputEvent::Key { hid_usage: keymap::HID_LEFT_GUI, down: true, modifiers: COMMAND }, &RECT);
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_SPACE, down: true, modifiers: COMMAND }, &RECT);
        assert_eq!(keys(&a), vec![(0x1D, false, false), (0x5B, true, true), (0x5B, true, false)]);
        let a = i.handle(&InputEvent::Key { hid_usage: keymap::HID_SPACE, down: false, modifiers: COMMAND }, &RECT);
        assert!(a.is_empty(), "the space up is swallowed");
    }

    #[test]
    fn leaving_releases_everything() {
        let mut i = Injector::new();
        i.handle(&InputEvent::Key { hid_usage: 0x04, down: true, modifiers: SHIFT }, &RECT);
        i.handle(&InputEvent::Key { hid_usage: 0x05, down: true, modifiers: SHIFT }, &RECT);
        let mut released = keys(&i.release_all());
        released.sort();
        assert_eq!(released, vec![(0x1E, false, false), (0x2A, false, false), (0x30, false, false)]);
        assert!(i.release_all().is_empty());
    }

    #[test]
    fn text_types_units_and_special_keys() {
        let a = text("hé\n😀\t\u{8}\u{7}");
        let mut units = Vec::new();
        for x in &a {
            if let Action::Unicode { unit, down: true } = x {
                units.push(*unit);
            }
        }
        assert_eq!(units, vec![0x68, 0xE9, 0xD83D, 0xDE00]);
        assert_eq!(keys(&a), vec![(0x1C, false, true), (0x1C, false, false), (0x0F, false, true), (0x0F, false, false), (0x0E, false, true), (0x0E, false, false)]);
        assert_eq!(text(&"x".repeat(5000)).len(), MAX_TEXT_UNITS * 2);
    }
}
