//! The Windows half: D3D11 device, monitor lookup, WGC capture and the NV12 converter.

use std::mem::ManuallyDrop;
use windows::core::{Interface, Result};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, POINT, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromPoint, DEVMODEW, ENUM_CURRENT_SETTINGS, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};

/// The GPU texture the converter hands out (NV12), for callers that keep one.
pub type Texture = ID3D11Texture2D;

/// Per-monitor-v2 DPI awareness, so monitor rectangles and input are in physical pixels. Call once,
/// before any window or monitor call; a second call (or a manifest that set it) is harmless.
pub fn set_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// A 1 ms system timer for as long as this lives; hold it only while a picture streams (a fine
/// timer costs battery). Media Foundation's hardware encoder waits on timers internally: at
/// Windows' default 15.6 ms tick a 720p frame came out 14.5 ms after it went in, at 1 ms 8.6 ms
/// (Intel Arc, 2026-11-27).
pub struct FineTimer(());

impl FineTimer {
    pub fn new() -> FineTimer {
        unsafe {
            let _ = windows::Win32::Media::timeBeginPeriod(1);
        }
        FineTimer(())
    }
}

impl Default for FineTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FineTimer {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Media::timeEndPeriod(1);
        }
    }
}

/// The hardware D3D11 device capture, conversion and the encoder share (zero copy).
pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    /// The adapter's description ("Intel(R) Arc(TM) Graphics").
    pub adapter: String,
}

impl Gpu {
    pub fn new() -> Result<Gpu> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device =
            device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
        let context = context.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
        // Media Foundation uses the device from its own threads.
        let mt: ID3D11Multithread = device.cast()?;
        unsafe {
            let _ = mt.SetMultithreadProtected(true);
        }
        let adapter = unsafe {
            let desc = device.cast::<IDXGIDevice>()?.GetAdapter()?.GetDesc()?;
            let n = desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len());
            String::from_utf16_lossy(&desc.Description[..n])
        };
        Ok(Gpu { device, context, adapter })
    }
}

/// A monitor, in physical pixels of the virtual desktop.
#[derive(Clone, Debug)]
pub struct Monitor {
    pub handle: HMONITOR,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    /// Windows' scale for this monitor (1.5 at 144 dpi).
    pub dpi_scale: f64,
    /// `\\.\DISPLAY1`
    pub device: String,
}

/// The primary monitor (the one holding the virtual desktop's origin).
pub fn primary_monitor() -> Option<Monitor> {
    unsafe {
        let handle = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
            return None;
        }
        let r = info.monitorInfo.rcMonitor;
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        let refresh_hz =
            if EnumDisplaySettingsW(windows::core::PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode)
                .as_bool()
            {
                mode.dmDisplayFrequency
            } else {
                60
            };
        let n = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
        let (mut dx, mut dy) = (96u32, 96u32);
        let dpi_scale = if GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            f64::from(dx) / 96.0
        } else {
            1.0
        };
        Some(Monitor {
            dpi_scale,
            handle,
            left: r.left,
            top: r.top,
            width: (r.right - r.left).max(0) as u32,
            height: (r.bottom - r.top).max(0) as u32,
            refresh_hz: if refresh_hz <= 1 { 60 } else { refresh_hz },
            device: String::from_utf16_lossy(&info.szDevice[..n]),
        })
    }
}

/// A frame WGC delivered; the texture is valid while this lives.
pub struct Frame {
    _frame: Direct3D11CaptureFrame,
    pub texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    /// When Windows composed it (its `SystemRelativeTime`, a QPC time), as an `Instant`.
    pub captured: std::time::Instant,
}

/// The `Instant` of a QPC-based time in 100 ns units (WGC's `SystemRelativeTime`); now, if it is
/// in the future or more than a second ago (not a QPC time after all).
fn instant_of_qpc_100ns(t: i64) -> std::time::Instant {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let now = std::time::Instant::now();
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        if QueryPerformanceCounter(&mut c).is_err() || QueryPerformanceFrequency(&mut f).is_err() || f <= 0 {
            return now;
        }
    }
    let now_100ns = (i128::from(c) * 10_000_000 / i128::from(f)) as i64;
    let age = now_100ns - t;
    if !(0..=10_000_000).contains(&age) {
        return now;
    }
    now.checked_sub(std::time::Duration::from_nanos(age as u64 * 100)).unwrap_or(now)
}

/// Windows Graphics Capture of one monitor on a free-threaded frame pool.
pub struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _item: GraphicsCaptureItem,
    token: i64,
    pub width: u32,
    pub height: u32,
}

impl Capture {
    /// Starts capturing `monitor`; `on_frame` runs on a system thread whenever a new frame waits
    /// (it should only wake the caller, who then takes it with `newest`).
    pub fn start(gpu: &Gpu, monitor: &Monitor, on_frame: impl Fn() + Send + Sync + 'static) -> Result<Capture> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor.handle)? };
        let dxgi: IDXGIDevice = gpu.device.cast()?;
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let size: SizeInt32 = item.Size()?;
        let pool =
            Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let token = pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            on_frame();
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        // The device draws its own cursor (PROTOCOL.md §8); Windows 11 can drop the yellow border.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Capture {
            pool,
            session,
            _item: item,
            token,
            width: size.Width.max(0) as u32,
            height: size.Height.max(0) as u32,
        })
    }

    /// The newest waiting frame; older waiting ones are released unseen (newest wins).
    pub fn newest(&self) -> Result<Option<Frame>> {
        let mut last = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            last = Some(f);
        }
        let Some(frame) = last else { return Ok(None) };
        let size = frame.ContentSize()?;
        let captured = frame
            .SystemRelativeTime()
            .map(|t| instant_of_qpc_100ns(t.Duration))
            .unwrap_or_else(|_| std::time::Instant::now());
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        Ok(Some(Frame {
            _frame: frame,
            texture,
            width: size.Width.max(0) as u32,
            height: size.Height.max(0) as u32,
            captured,
        }))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn texture(gpu: &Gpu, width: u32, height: u32, format: DXGI_FORMAT, bind: D3D11_BIND_FLAG) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut t = None;
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut t))? };
    t.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

/// How many NV12 textures the converter cycles through: the encoder may still read one while the
/// next is written.
const OUTPUTS: usize = 3;

/// BGRA (the captured frame) to NV12 (the encoder's input), scaled, on the GPU.
pub struct Converter {
    context: ID3D11DeviceContext,
    vctx: ID3D11VideoContext,
    vp: ID3D11VideoProcessor,
    input: ID3D11Texture2D,
    input_view: ID3D11VideoProcessorInputView,
    outputs: Vec<(ID3D11Texture2D, ID3D11VideoProcessorOutputView)>,
    next: usize,
    pub in_width: u32,
    pub in_height: u32,
    pub out_width: u32,
    pub out_height: u32,
}

impl Converter {
    pub fn new(gpu: &Gpu, in_width: u32, in_height: u32, out_width: u32, out_height: u32) -> Result<Converter> {
        let vdev: ID3D11VideoDevice = gpu.device.cast()?;
        let vctx: ID3D11VideoContext = gpu.context.cast()?;
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
            InputWidth: in_width,
            InputHeight: in_height,
            OutputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
            OutputWidth: out_width,
            OutputHeight: out_height,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let ve = unsafe { vdev.CreateVideoProcessorEnumerator(&content)? };
        let vp = unsafe { vdev.CreateVideoProcessor(&ve, 0)? };

        let input = texture(
            gpu,
            in_width,
            in_height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            D3D11_BIND_FLAG(D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0),
        )?;
        let in_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 },
            },
        };
        let mut input_view = None;
        unsafe { vdev.CreateVideoProcessorInputView(&input, &ve, &in_desc, Some(&mut input_view))? };
        let input_view = input_view.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;

        let out_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
        };
        let mut outputs = Vec::with_capacity(OUTPUTS);
        for _ in 0..OUTPUTS {
            // The encoder binding lets a hardware MFT read the texture directly; not every driver
            // takes it on a render target, so fall back without it.
            let t = texture(
                gpu,
                out_width,
                out_height,
                DXGI_FORMAT_NV12,
                D3D11_BIND_FLAG(D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_VIDEO_ENCODER.0),
            )
            .or_else(|_| texture(gpu, out_width, out_height, DXGI_FORMAT_NV12, D3D11_BIND_RENDER_TARGET))?;
            let mut v = None;
            unsafe { vdev.CreateVideoProcessorOutputView(&t, &ve, &out_desc, Some(&mut v))? };
            outputs.push((t, v.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?));
        }

        unsafe {
            vctx.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            vctx.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
            let src = RECT { left: 0, top: 0, right: in_width as i32, bottom: in_height as i32 };
            let dst = RECT { left: 0, top: 0, right: out_width as i32, bottom: out_height as i32 };
            vctx.VideoProcessorSetStreamSourceRect(&vp, 0, true, Some(&src));
            vctx.VideoProcessorSetStreamDestRect(&vp, 0, true, Some(&dst));
            vctx.VideoProcessorSetOutputTargetRect(&vp, true, Some(&dst));
            // Full range BT.709 out, as the Mac host's 420f (PROTOCOL.md §8).
            if let Ok(v1) = vctx.cast::<ID3D11VideoContext1>() {
                v1.VideoProcessorSetStreamColorSpace1(&vp, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
                v1.VideoProcessorSetOutputColorSpace1(&vp, DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P709);
            } else {
                // Usage 0, RGB full range; YCbCr BT.709 (bit 2), nominal range 0-255 (2 << 4).
                vctx.VideoProcessorSetStreamColorSpace(&vp, 0, &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 });
                vctx.VideoProcessorSetOutputColorSpace(
                    &vp,
                    &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 4 | (2 << 4) },
                );
            }
        }
        Ok(Converter {
            context: gpu.context.clone(),
            vctx,
            vp,
            input,
            input_view,
            outputs,
            next: 0,
            in_width,
            in_height,
            out_width,
            out_height,
        })
    }

    /// Copies `frame` (GPU to GPU) and converts it into the next NV12 texture, which it returns.
    pub fn convert(&mut self, frame: &Frame) -> Result<ID3D11Texture2D> {
        let w = frame.width.min(self.in_width);
        let h = frame.height.min(self.in_height);
        let b = D3D11_BOX { left: 0, top: 0, front: 0, right: w, bottom: h, back: 1 };
        unsafe {
            self.context.CopySubresourceRegion(&self.input, 0, 0, 0, 0, &frame.texture, 0, Some(&b));
        }
        let (out, view) = &self.outputs[self.next];
        self.next = (self.next + 1) % self.outputs.len();
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(self.input_view.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        let streams = [stream];
        let r = unsafe { self.vctx.VideoProcessorBlt(&self.vp, view, 0, &streams) };
        let [mut s] = streams;
        unsafe { ManuallyDrop::drop(&mut s.pInputSurface) };
        r?;
        // Submit now: the encoder waits for this work, and D3D11 would otherwise hold it until its
        // command buffer fills or something else flushes (30-60 ms of latency on Intel Arc).
        unsafe { self.context.Flush() };
        Ok(out.clone())
    }
}
