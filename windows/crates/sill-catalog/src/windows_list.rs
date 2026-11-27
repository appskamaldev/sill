//! The window list (kind 2, Windows): visible top-level app windows, in first-seen order, each
//! with the app ID the Apps list uses (so one icon serves both) and its size in DIPs (the Mac's
//! points).

use std::collections::HashMap;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

/// One listed window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Win {
    pub hwnd: isize,
    pub title: String,
    pub pid: u32,
    /// The executable's full path, if readable.
    pub exe: Option<String>,
    /// The window's AppUserModelID, if it has one (packaged apps, and desktop apps that set one).
    pub aumid: Option<String>,
    /// The visible frame in DIPs (its normal size when minimized is not known: 0).
    pub width: i64,
    pub height: i64,
    pub minimized: bool,
}

impl Win {
    /// The protocol's id: the HWND's low 32 bits (HWNDs are 32-bit significant on x64).
    pub fn id(&self) -> u32 {
        self.hwnd as u32
    }
}

/// Window classes that are the shell, never apps.
const SHELL_CLASSES: [&str; 6] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
];

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title_of(h: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The executable path of process `pid`.
pub fn exe_of(pid: u32) -> Option<String> {
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(p);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The window's AppUserModelID: its own property, else its process's package's (packaged apps such
/// as Photos and Notepad set none on the window itself).
fn aumid_of(h: HWND, pid: u32) -> Option<String> {
    let from_window = unsafe {
        SHGetPropertyStoreForWindow::<IPropertyStore>(h)
            .ok()
            .and_then(|store| store.GetValue(&PKEY_AppUserModel_ID).ok())
            .map(|v| v.to_string())
            .filter(|s| !s.is_empty())
    };
    from_window.or_else(|| package_aumid(pid))
}

fn package_aumid(pid: u32) -> Option<String> {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let r = GetApplicationUserModelId(p, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(p);
        if r.0 != 0 || len == 0 {
            return None;
        }
        // `len` counts the terminating NUL.
        Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]))
    }
}

fn cloaked(h: HWND) -> bool {
    let mut c = 0u32;
    unsafe {
        DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut c as *mut u32 as *mut _, std::mem::size_of::<u32>() as u32).is_ok()
            && c != 0
    }
}

/// The visible frame (without the drop shadow) in physical pixels.
pub fn frame_of(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
    }
    Some(r)
}

/// Windows' scale for the window's monitor (1.5 at 144 dpi).
pub fn dpi_scale_of(h: HWND) -> f64 {
    let d = unsafe { GetDpiForWindow(h) };
    if d == 0 {
        1.0
    } else {
        f64::from(d) / 96.0
    }
}

unsafe extern "system" fn collect(h: HWND, l: LPARAM) -> BOOL {
    let v = &mut *(l.0 as *mut Vec<HWND>);
    v.push(h);
    true.into()
}

/// The windows a device lists, front to back (z order).
pub fn windows() -> Vec<Win> {
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    let me = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for h in all {
        unsafe {
            if !IsWindowVisible(h).as_bool() || cloaked(h) {
                continue;
            }
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            let owned = GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid());
            if ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 {
                continue;
            }
            if owned && ex & WS_EX_APPWINDOW.0 == 0 {
                continue;
            }
            if SHELL_CLASSES.contains(&class_of(h).as_str()) {
                continue;
            }
            let title = title_of(h);
            if title.trim().is_empty() {
                continue;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == me {
                continue;
            }
            // Upstream lists on-screen windows only: a minimized window is not listed (and a
            // streamed one that minimizes stops being the source).
            let minimized = IsIconic(h).as_bool();
            if minimized {
                continue;
            }
            let (width, height) = match frame_of(h) {
                Some(r) if !minimized => {
                    let s = dpi_scale_of(h);
                    ((f64::from(r.right - r.left) / s).round() as i64, (f64::from(r.bottom - r.top) / s).round() as i64)
                }
                _ => (0, 0),
            };
            if !minimized && (width < 100 || height < 100) {
                continue;
            }
            out.push(Win {
                hwnd: h.0 as isize,
                title,
                pid,
                exe: exe_of(pid),
                aumid: aumid_of(h, pid),
                width,
                height,
                minimized,
            });
        }
    }
    out
}

/// What a window shows as its app: the Apps list's (ID, name) for its AppUserModelID or its
/// executable, else its executable path and file name. `by_path` maps a lower-case full exe path to
/// an app.
pub fn app_of(w: &Win, apps: &[crate::App], by_path: &HashMap<String, usize>) -> (String, String) {
    if let Some(a) = w.aumid.as_ref().and_then(|id| apps.iter().find(|a| &a.id == id)) {
        return (a.id.clone(), a.name.clone());
    }
    if let Some(i) = w.exe.as_ref().and_then(|e| by_path.get(&e.to_lowercase())) {
        return (apps[*i].id.clone(), apps[*i].name.clone());
    }
    match &w.exe {
        Some(e) => {
            let stem =
                std::path::Path::new(e).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            (e.clone(), stem)
        }
        None => (format!("pid:{}", w.pid), "App".into()),
    }
}
