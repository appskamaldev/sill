//! What the host does to a window: the commands (kind 15), bringing it forward, fitting it to the
//! device's panel and putting it back, and its thumbnail.

use windows::core::{Result, GUID};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ, HPALETTE,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, IWICImagingFactory,
    WICBitmapEncoderNoCache, WICBitmapIgnoreAlpha, WICBitmapInterpolationModeFant,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, IsIconic, IsWindow, IsZoomed, PostMessageW, SetForegroundWindow, SetWindowPos,
    ShowWindow, PW_RENDERFULLCONTENT, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SW_RESTORE, WM_SYSCOMMAND,
};

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut _)
}

fn sys(h: isize, cmd: u32) -> std::result::Result<(), String> {
    unsafe {
        if !IsWindow(Some(hwnd(h))).as_bool() {
            return Err("the window is gone".into());
        }
        PostMessageW(Some(hwnd(h)), WM_SYSCOMMAND, WPARAM(cmd as usize), LPARAM(0)).map_err(|e| e.to_string())
    }
}

/// Asks the window to close, as its title bar's close button does (it may ask to save first).
pub fn close(h: isize) -> std::result::Result<(), String> {
    sys(h, SC_CLOSE)
}

pub fn minimize(h: isize) -> std::result::Result<(), String> {
    sys(h, SC_MINIMIZE)
}

/// Upstream's "full screen": maximize, or restore a maximized window.
pub fn toggle_maximize(h: isize) -> std::result::Result<(), String> {
    let zoomed = unsafe { IsZoomed(hwnd(h)).as_bool() };
    sys(h, if zoomed { SC_RESTORE } else { SC_MAXIMIZE })
}

/// Brings the window forward and gives it the keyboard, if it is not already. Windows lets this
/// process do so while it holds the last input (it just injected the device's), and refuses
/// otherwise; nothing here works around that.
pub fn bring_forward(h: isize) -> bool {
    unsafe {
        let w = hwnd(h);
        if GetForegroundWindow() == w {
            return true;
        }
        if IsIconic(w).as_bool() {
            let _ = ShowWindow(w, SW_RESTORE);
        }
        SetForegroundWindow(w).as_bool()
    }
}

pub fn is_foreground(h: isize) -> bool {
    unsafe { GetForegroundWindow() == hwnd(h) }
}

/// The window's outer rectangle (what `SetWindowPos` sizes), in physical pixels.
pub fn outer_rect(h: isize) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd(h), &mut r).ok()? };
    Some(r)
}

/// Sizes the window so its visible frame is `width`×`height` physical pixels, keeping its
/// top-left where it is. The outer rectangle includes an invisible resize border (and shadow) the
/// visible frame does not, so that margin is added.
pub fn set_frame_size(h: isize, width: i32, height: i32) -> std::result::Result<(), String> {
    let outer = outer_rect(h).ok_or("the window is gone")?;
    let frame = crate::windows_list::frame_of(hwnd(h)).unwrap_or(outer);
    let dw = (outer.right - outer.left) - (frame.right - frame.left);
    let dh = (outer.bottom - outer.top) - (frame.bottom - frame.top);
    unsafe {
        SetWindowPos(hwnd(h), None, 0, 0, width + dw, height + dh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| e.to_string())
    }
}

/// Puts the window's outer rectangle back to `r`.
pub fn set_outer_rect(h: isize, r: RECT) -> std::result::Result<(), String> {
    unsafe {
        SetWindowPos(hwnd(h), None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| e.to_string())
    }
}

pub fn is_window(h: isize) -> bool {
    unsafe { IsWindow(Some(hwnd(h))).as_bool() }
}

pub fn is_maximized(h: isize) -> bool {
    unsafe { IsZoomed(hwnd(h)).as_bool() }
}

/// The window's picture (PrintWindow, full content) fitted inside `max_w`×`max_h`, as JPEG at
/// `quality` (0..1). Off the video path: a GDI copy into memory, a few times a minute.
pub fn thumbnail(h: isize, max_w: u32, max_h: u32, quality: f32) -> Result<Vec<u8>> {
    let r = crate::windows_list::frame_of(hwnd(h))
        .or_else(|| outer_rect(h))
        .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let outer = outer_rect(h).unwrap_or(r);
    let (ow, oh) = ((outer.right - outer.left).max(1), (outer.bottom - outer.top).max(1));
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ow,
                biHeight: -oh,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        let result = (|| -> Result<Vec<u8>> {
            let bmp = bmp?;
            let old = SelectObject(dc, HGDIOBJ(bmp.0));
            let ok = PrintWindow(hwnd(h), dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
            SelectObject(dc, old);
            let r2 = if ok {
                jpeg_of(
                    bmp,
                    (r.left - outer.left, r.top - outer.top, r.right - r.left, r.bottom - r.top),
                    max_w,
                    max_h,
                    quality,
                )
            } else {
                Err(windows::core::Error::from(E_FAIL))
            };
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            r2
        })();
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        result
    }
}

/// The JPEG of `crop` (x, y, w, h within the bitmap) fitted in `max_w`×`max_h`.
unsafe fn jpeg_of(
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    crop: (i32, i32, i32, i32),
    max_w: u32,
    max_h: u32,
    quality: f32,
) -> Result<Vec<u8>> {
    let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
    let full = wic.CreateBitmapFromHBITMAP(bmp, HPALETTE::default(), WICBitmapIgnoreAlpha)?;
    let clip = wic.CreateBitmapClipper()?;
    let rect = windows::Win32::Graphics::Imaging::WICRect {
        X: crop.0.max(0),
        Y: crop.1.max(0),
        Width: crop.2.max(2),
        Height: crop.3.max(2),
    };
    clip.Initialize(&full, &rect)?;
    let (cw, ch) = (rect.Width as f64, rect.Height as f64);
    let sf = (f64::from(max_w) / cw).min(f64::from(max_h) / ch).min(1.0);
    let (tw, th) = (((cw * sf) as u32).max(2), ((ch * sf) as u32).max(2));
    let scaler = wic.CreateBitmapScaler()?;
    scaler.Initialize(&clip, tw, th, WICBitmapInterpolationModeFant)?;
    let conv = wic.CreateFormatConverter()?;
    conv.Initialize(
        &scaler,
        &GUID_WICPixelFormat24bppBGR,
        windows::Win32::Graphics::Imaging::WICBitmapDitherTypeNone,
        None,
        0.0,
        windows::Win32::Graphics::Imaging::WICBitmapPaletteTypeCustom,
    )?;
    let stream = CreateStreamOnHGlobal(Default::default(), true)?;
    let enc = wic.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null::<GUID>())?;
    enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
    let mut frame = None;
    let mut props = None;
    enc.CreateNewFrame(&mut frame, &mut props)?;
    let frame = frame.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    if let Some(bag) = props {
        let name = windows::core::w!("ImageQuality");
        let opt = windows::Win32::System::Com::StructuredStorage::PROPBAG2 {
            pstrName: windows::core::PWSTR(name.0 as *mut _),
            ..Default::default()
        };
        let v = windows::Win32::System::Variant::VARIANT::from(quality);
        let _ = bag.Write(1, &opt, &v);
        frame.Initialize(&bag)?;
    } else {
        frame.Initialize(None)?;
    }
    frame.SetSize(tw, th)?;
    let mut fmt = GUID_WICPixelFormat24bppBGR;
    frame.SetPixelFormat(&mut fmt)?;
    frame.WriteSource(&conv, std::ptr::null())?;
    frame.Commit()?;
    enc.Commit()?;
    let hg = GetHGlobalFromStream(&stream)?;
    let len = GlobalSize(hg);
    let p = GlobalLock(hg) as *const u8;
    if p.is_null() {
        return Err(windows::core::Error::from(E_FAIL));
    }
    let mut stat = Default::default();
    let written = match stream.Stat(&mut stat, windows::Win32::System::Com::STATFLAG_NONAME) {
        Ok(()) => (stat.cbSize as usize).min(len),
        Err(_) => len,
    };
    let out = std::slice::from_raw_parts(p, written).to_vec();
    let _ = GlobalUnlock(hg);
    Ok(out)
}
