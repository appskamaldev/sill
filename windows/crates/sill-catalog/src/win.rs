//! The shell's Apps folder, icons as PNG through WIC, and the launch.

use crate::App;
use windows::core::{Result, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, SIZE};
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICBitmapEncoderNoCache, WICBitmapUseAlpha,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, ShellExecuteW,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn display(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// The apps of `shell:AppsFolder` (Start's "All apps"), tidied (`crate::tidy`).
pub fn scan() -> Result<Vec<App>> {
    com();
    let folder: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from("shell:AppsFolder"), None)? };
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems)? };
    let mut apps = Vec::new();
    loop {
        let mut got = [None];
        let mut n = 0u32;
        if unsafe { items.Next(&mut got, Some(&mut n)) }.is_err() || n == 0 {
            break;
        }
        let Some(item) = got[0].take() else { break };
        if let (Some(id), Some(name)) =
            (display(&item, SIGDN_PARENTRELATIVEPARSING), display(&item, SIGDN_NORMALDISPLAY))
        {
            apps.push(App { id, name });
        }
        if apps.len() > 4 * crate::MAX_APPS {
            break;
        }
    }
    Ok(crate::tidy(apps))
}

/// The full path of an Apps list ID of the form "{KNOWNFOLDERID}\\rest\\app.exe" (a desktop app);
/// `None` for an AppUserModelID or an ID that names no known folder.
pub fn app_path(id: &str) -> Option<String> {
    let rest = id.strip_prefix('{')?;
    let (guid, tail) = rest.split_once("}\\")?;
    let g = GUID::try_from(guid).ok()?;
    unsafe {
        let p =
            windows::Win32::UI::Shell::SHGetKnownFolderPath(&g, windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0), None)
                .ok()?;
        let base = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        Some(format!("{}\\{tail}", base?))
    }
}

/// The app's icon as a `size`×`size` PNG with alpha. `id` is an Apps list ID, or a full path (a
/// window's executable that is not on the list).
pub fn icon_png(id: &str, size: i32) -> Result<Vec<u8>> {
    com();
    let path = if id.len() > 2 && id.as_bytes()[1] == b':' {
        HSTRING::from(id)
    } else {
        HSTRING::from(format!("shell:AppsFolder\\{id}"))
    };
    let item: IShellItemImageFactory = unsafe { SHCreateItemFromParsingName(&path, None)? };
    let bmp = unsafe { item.GetImage(SIZE { cx: size, cy: size }, SIIGBF_RESIZETOFIT | SIIGBF_ICONONLY)? };
    let r = encode_png(bmp);
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bmp.0));
    }
    r
}

fn encode_png(bmp: windows::Win32::Graphics::Gdi::HBITMAP) -> Result<Vec<u8>> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bitmap = wic.CreateBitmapFromHBITMAP(bmp, HPALETTE::default(), WICBitmapUseAlpha)?;
        let (mut w, mut h) = (0u32, 0u32);
        bitmap.GetSize(&mut w, &mut h)?;
        let stream = CreateStreamOnHGlobal(Default::default(), true)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null::<GUID>())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        frame.Initialize(None)?;
        frame.SetSize(w, h)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&bitmap, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
        let hg = GetHGlobalFromStream(&stream)?;
        let len = GlobalSize(hg);
        let p = GlobalLock(hg) as *const u8;
        if p.is_null() {
            return Err(windows::core::Error::from(E_FAIL));
        }
        // The stream's HGLOBAL may be larger than what was written: take the stream's own length.
        let mut stat = Default::default();
        let written = match stream.Stat(&mut stat, windows::Win32::System::Com::STATFLAG_NONAME) {
            Ok(()) => (stat.cbSize as usize).min(len),
            Err(_) => len,
        };
        let out = std::slice::from_raw_parts(p, written).to_vec();
        let _ = GlobalUnlock(hg);
        Ok(out)
    }
}

/// Opens `shell:AppsFolder\<id>`, as Start does. The caller has checked `id` is listed.
pub fn launch(id: &str) -> Result<()> {
    com();
    // The host has just injected the device's tap, so it may hand the foreground on; without this,
    // Windows can open the app behind the window that has focus.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
            windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
        );
    }
    let target = HSTRING::from(format!("shell:AppsFolder\\{id}"));
    let r =
        unsafe { ShellExecuteW(None, &HSTRING::from("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute returns a value above 32 on success.
    if r.0 as usize > 32 {
        Ok(())
    } else {
        Err(windows::core::Error::from(E_FAIL))
    }
}
