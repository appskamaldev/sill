//! `SendInput` for the actions `Injector` decides, and the one check before any of them: input
//! goes only to the user's own desktop, never while the input desktop is another one (the lock or
//! sign-in screen, a UAC prompt), which a user-session process cannot reach anyway (risk R6).

use crate::{Action, Button};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, UOI_NAME,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Whether the input desktop is the user's ("Default"): not the lock screen, sign-in or a UAC
/// prompt. Read at most every 250 ms.
pub fn user_desktop_active() -> bool {
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    if let Some((t, v)) = *c {
        if t.elapsed() < Duration::from_millis(250) {
            return v;
        }
    }
    let v = unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(d) => {
                let mut name = [0u16; 64];
                let mut needed = 0u32;
                let ok = GetUserObjectInformationW(
                    windows::Win32::Foundation::HANDLE(d.0),
                    UOI_NAME,
                    Some(name.as_mut_ptr().cast()),
                    (name.len() * 2) as u32,
                    Some(&mut needed),
                )
                .is_ok();
                let _ = CloseDesktop(d);
                let n = name.iter().position(|c| *c == 0).unwrap_or(name.len());
                ok && String::from_utf16_lossy(&name[..n]).eq_ignore_ascii_case("Default")
            }
            // The secure desktop cannot even be opened from here.
            Err(_) => false,
        }
    };
    *c = Some((Instant::now(), v));
    v
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn keyboard(scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// The virtual desktop's rectangle (left, top, width, height) in physical pixels.
fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

/// Sends `actions` in order, in one `SendInput` call. Returns how many Windows took (fewer when
/// another process blocks input, e.g. an elevated window has focus: UIPI, risk R7).
pub fn send(actions: &[Action]) -> usize {
    if actions.is_empty() {
        return 0;
    }
    let (vx, vy, vw, vh) = virtual_screen();
    // Absolute coordinates are 0..65535 across the virtual desktop.
    let norm = |p: i32, origin: i32, len: i32| -> i32 {
        let span = (len - 1).max(1) as i64;
        (((i64::from(p - origin)) * 65535 + span / 2) / span).clamp(0, 65535) as i32
    };
    let mut inputs = Vec::with_capacity(actions.len());
    let mut last = (0, 0);
    for a in actions {
        match *a {
            Action::MoveTo { x, y } => {
                last = (norm(x, vx, vw), norm(y, vy, vh));
                inputs.push(mouse(last.0, last.1, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK));
            }
            Action::Button { button, down } => {
                let f = match (button, down) {
                    (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (Button::Left, false) => MOUSEEVENTF_LEFTUP,
                    (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
                };
                inputs.push(mouse(last.0, last.1, 0, f | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK));
            }
            Action::Wheel { delta, horizontal } => {
                let f = if horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL };
                inputs.push(mouse(0, 0, delta as u32, f));
            }
            Action::Key { scan, down } => {
                let mut f = KEYEVENTF_SCANCODE;
                if scan.extended {
                    f |= KEYEVENTF_EXTENDEDKEY;
                }
                if !down {
                    f |= KEYEVENTF_KEYUP;
                }
                inputs.push(keyboard(scan.code, f));
            }
            Action::Unicode { unit, down } => {
                let mut f = KEYEVENTF_UNICODE;
                if !down {
                    f |= KEYEVENTF_KEYUP;
                }
                inputs.push(keyboard(unit, f));
            }
        }
    }
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize }
}
