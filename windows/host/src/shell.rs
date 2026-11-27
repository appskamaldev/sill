//! The host's place on the Windows desktop: the tray icon and its menu, the control pipe the Sill
//! app talks to, start at sign-in, and opening the app. The app (`app\Sill.exe` beside this exe,
//! C#/WinUI 3, ADR-7) is started on demand and exits when its window closes; the host and its tray
//! icon stay.
//!
//! The control pipe `\\.\pipe\sill-host-<user SID>` admits only this Windows user and SYSTEM (its
//! DACL), never a remote client, and only a client process whose executable lies in this exe's
//! folder (`--control-any-client` lifts that last check for development, and says so in the log).

use sill_net::control::{Control, Platform};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tracing::{debug, info, warn};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Sill");

/// This exe's folder.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

/// The Sill app beside this exe.
pub fn app_exe() -> PathBuf {
    exe_dir().join("app").join("Sill.exe")
}

/// Opens the Sill app (`args`: "" for its window, "--flyout", "--pair").
pub fn open_app(args: &str) {
    let exe = app_exe();
    if !exe.exists() {
        warn!("The Sill app is not installed beside the host ({}).", exe.display());
        return;
    }
    unsafe {
        // The click on the tray icon gave this process the foreground; pass it on so the flyout
        // and the window come up in front.
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX);
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(exe.as_os_str()),
            &HSTRING::from(args),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// When launched from a console (`sill-host` typed in a terminal), print there: this exe is a
/// Windows (GUI) program so it opens no console window of its own.
pub fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    unsafe {
        // Redirected output (a file, a pipe) is inherited already; only a bare console needs this.
        if GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null()) {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            use windows::Win32::Storage::FileSystem::{
                CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_WRITE, FILE_SHARE_WRITE, OPEN_EXISTING,
            };
            use windows::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
            if let Ok(h) = CreateFileW(
                w!("CONOUT$"),
                FILE_GENERIC_WRITE.0,
                FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            ) {
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, h);
                let _ = SetStdHandle(STD_ERROR_HANDLE, h);
            }
        }
    }
}

/// This Windows user's SID as text ("S-1-5-21-…").
pub fn user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let r = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len);
        let _ = CloseHandle(token);
        r.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).ok()?;
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0 as *mut c_void)));
        out
    }
}

pub fn pipe_name(sid: &str) -> String {
    format!(r"\\.\pipe\sill-host-{sid}")
}

// MARK: start at sign-in

fn run_command() -> String {
    format!("\"{}\"", std::env::current_exe().unwrap_or_default().display())
}

pub fn start_at_login() -> Option<bool> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return Some(false);
    }
    let v = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    Some(v.eq_ignore_ascii_case(&run_command()))
}

pub fn set_start_at_login(on: bool) -> Result<(), String> {
    unsafe {
        let r = if on {
            let v: Vec<u16> = run_command().encode_utf16().chain([0]).collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_VALUE,
                REG_SZ.0,
                Some(v.as_ptr() as *const c_void),
                (v.len() * 2) as u32,
            )
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
            if r.0 == 2 {
                windows::Win32::Foundation::WIN32_ERROR(0)
            } else {
                r
            }
        };
        if r.is_ok() {
            info!("Start at sign-in {}.", if on { "on" } else { "off" });
            Ok(())
        } else {
            Err(format!("the Run key could not be changed ({})", r.0))
        }
    }
}

// MARK: the platform hooks the control protocol calls

pub struct WinPlatform {
    pub quit: Arc<tokio::sync::Notify>,
}

impl Platform for WinPlatform {
    fn start_at_login(&self) -> Option<bool> {
        start_at_login()
    }
    fn set_start_at_login(&self, on: bool) -> Result<(), String> {
        set_start_at_login(on)
    }
    fn quit(&self) {
        self.quit.notify_one();
    }
}

// MARK: the control pipe

fn client_exe(server: &NamedPipeServer) -> Option<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    unsafe {
        let mut pid = 0u32;
        GetNamedPipeClientProcessId(HANDLE(server.as_raw_handle()), &mut pid).ok()?;
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(p);
        r.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Whether `exe` lies in `dir` (or below it), compared case-insensitively.
pub fn inside(exe: &Path, dir: &Path) -> bool {
    let e = exe.to_string_lossy().to_lowercase();
    let mut d = dir.to_string_lossy().to_lowercase();
    if !d.ends_with('\\') {
        d.push('\\');
    }
    !dir.as_os_str().is_empty() && e.starts_with(&d)
}

fn create_server(name: &str, sddl: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), SDDL_REVISION_1, &mut sd, None)
            .map_err(std::io::Error::other)?;
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let r = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &mut sa as *mut SECURITY_ATTRIBUTES as *mut c_void);
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        r
    }
}

/// Serves the control pipe until the process ends.
pub async fn serve_control(control: Arc<Control>, any_client: bool) {
    let Some(sid) = user_sid() else {
        warn!("Control pipe: this user's SID could not be read; the Sill app cannot reach the host.");
        return;
    };
    let name = pipe_name(&sid);
    // Only this user and SYSTEM; P: no inherited entries.
    let sddl = format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)");
    if any_client {
        warn!("DEV ONLY: the control pipe takes any client process of this user (--control-any-client).");
    }
    let dir = exe_dir();
    let mut first = true;
    loop {
        let server = match create_server(&name, &sddl, first) {
            Ok(s) => s,
            Err(e) => {
                warn!("Control pipe could not be created ({e}); is another host running for this user?");
                return;
            }
        };
        first = false;
        if server.connect().await.is_err() {
            continue;
        }
        let exe = client_exe(&server);
        let allowed = any_client || exe.as_deref().is_some_and(|e| inside(e, &dir));
        if !allowed {
            warn!("Control pipe: refused a client ({}).", exe.map_or("unknown".into(), |e| e.display().to_string()));
            continue;
        }
        let c = control.clone();
        tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(server);
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let reply = c.handle(&line);
                let mut out = serde_json::to_vec(&reply).unwrap_or_default();
                out.push(b'\n');
                if w.write_all(&out).await.is_err() {
                    break;
                }
            }
            debug!("Control pipe: a client left.");
        });
    }
}

// MARK: the tray icon

const WM_TRAY: u32 = WM_APP + 1;
const ID_OPEN: usize = 1;
const ID_PAIR: usize = 2;
const ID_PAUSE: usize = 3;
const ID_QUIT: usize = 4;

struct Tray {
    control: Arc<Control>,
    quit: Arc<tokio::sync::Notify>,
    taskbar_created: u32,
}

static TRAY: OnceLock<Tray> = OnceLock::new();
static TIP: Mutex<String> = Mutex::new(String::new());
static ICON_ADDED: AtomicBool = AtomicBool::new(false);

fn load_icon(size: i32) -> HICON {
    static ICO: &[u8] = include_bytes!("../assets/sill.ico");
    unsafe {
        let off = LookupIconIdFromDirectoryEx(ICO.as_ptr(), true, size, size, LR_DEFAULTCOLOR);
        if off <= 0 || off as usize >= ICO.len() {
            return HICON::default();
        }
        // The entry's bytes: find its length in the directory.
        let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
        for i in 0..count {
            let e = &ICO[6 + i * 16..6 + i * 16 + 16];
            let len = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
            let at = u32::from_le_bytes(e[12..16].try_into().unwrap()) as usize;
            if at == off as usize {
                return CreateIconFromResourceEx(&ICO[at..at + len], true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
                    .unwrap_or_default();
            }
        }
        HICON::default()
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: load_icon(unsafe { GetSystemMetrics(SM_CXSMICON) }),
        ..Default::default()
    };
    let tip = TIP.lock().unwrap().clone();
    let t: Vec<u16> = (if tip.is_empty() { "Sill".to_string() } else { tip }).encode_utf16().take(127).collect();
    nid.szTip[..t.len()].copy_from_slice(&t);
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    nid
}

fn add_icon(hwnd: HWND) {
    let nid = icon_data(hwnd);
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
            ICON_ADDED.store(true, Ordering::Relaxed);
        }
    }
}

fn show_menu(hwnd: HWND) {
    let Some(t) = TRAY.get() else { return };
    let paused = t.control.host.paused.load(Ordering::Relaxed);
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("Open Sill"));
        let _ = AppendMenuW(menu, MF_STRING, ID_PAIR, w!("Pair a Device…"));
        let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE, if paused { w!("Resume Hosting") } else { w!("Pause Hosting") });
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit Sill"));
        let _ = SetMenuDefaultItem(menu, ID_OPEN as u32, 0);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            ID_OPEN => open_app(""),
            ID_PAIR => open_app("--pair"),
            ID_PAUSE => {
                let _ = t.control.handle(if paused { r#"{"cmd":"resume"}"# } else { r#"{"cmd":"pause"}"# });
            }
            ID_QUIT => t.quit.notify_one(),
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TRAY {
        // NOTIFYICON_VERSION_4: the event is the low word of lParam.
        match (lp.0 as u32) & 0xFFFF {
            WM_LBUTTONUP | 0x0400 /* NIN_SELECT */ | 0x0401 /* NIN_KEYSELECT */ => open_app("--flyout"),
            WM_CONTEXTMENU | WM_RBUTTONUP => show_menu(hwnd),
            _ => {}
        }
        return LRESULT(0);
    }
    if TRAY.get().is_some_and(|t| t.taskbar_created != 0 && msg == t.taskbar_created) {
        // Explorer restarted: the icon is gone, add it again.
        add_icon(hwnd);
        return LRESULT(0);
    }
    if msg == WM_USER + 7 {
        let nid = icon_data(hwnd);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

static TRAY_HWND: Mutex<isize> = Mutex::new(0);

/// Puts the icon in the notification area, on a thread of its own that runs its message loop.
pub fn start_tray(control: Arc<Control>, quit: Arc<tokio::sync::Notify>) {
    let _ = std::thread::Builder::new().name("sill-tray".into()).spawn(move || unsafe {
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let _ = TRAY.set(Tray { control, quit, taskbar_created });
        let class = w!("SillHostTray");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Sill"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) else {
            warn!("Tray: no window for the icon.");
            return;
        };
        *TRAY_HWND.lock().unwrap() = hwnd.0 as isize;
        add_icon(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Updates the icon's tooltip ("Sill · Streaming to iPhone").
pub fn set_tooltip(text: &str) {
    let mut t = TIP.lock().unwrap();
    if *t == text {
        return;
    }
    *t = text.to_string();
    drop(t);
    let h = *TRAY_HWND.lock().unwrap();
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_USER + 7, WPARAM(0), LPARAM(0));
        }
    }
}

/// Takes the icon away (quitting).
pub fn remove_tray() {
    let h = *TRAY_HWND.lock().unwrap();
    if h != 0 && ICON_ADDED.load(Ordering::Relaxed) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(h as *mut c_void),
            uID: 1,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clients_inside_the_folder() {
        let dir = Path::new(r"C:\Program Files\Sill");
        assert!(inside(Path::new(r"C:\Program Files\Sill\app\Sill.exe"), dir));
        assert!(inside(Path::new(r"c:\program files\sill\sill-host.exe"), dir));
        assert!(!inside(Path::new(r"C:\Program Files\Sill2\evil.exe"), dir));
        assert!(!inside(Path::new(r"C:\Windows\System32\cmd.exe"), dir));
        assert!(!inside(Path::new(r"C:\x.exe"), Path::new("")));
    }
}
