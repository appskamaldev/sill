//! Media Foundation's hardware HEVC encoder (an async MFT, found with `MFTEnumEx`), fed NV12
//! textures on the capture's D3D11 device through a DXGI device manager: no copy to the CPU on the
//! way in; only the encoded bitstream comes out to memory.
//!
//! Settings (PROTOCOL.md §8): Main 8-bit, low-latency mode, no B-frames, CBR, a GOP of fps×4 as a
//! safety net (keyframes are otherwise sent on request: join, drop, kind 44), full range BT.709 in
//! the VUI.
//!
//! The MFT's events (need input, have output) are read by a small thread that blocks on
//! `IMFMediaEventGenerator::GetEvent` and forwards them over a channel, so the caller can wait on
//! them with a timeout alongside its own wake-ups.

use std::sync::mpsc;
use std::time::Duration;
use windows::core::{Interface, Result, GUID};
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_UI4};

/// Starts Media Foundation and COM (multithreaded) on this thread; harmless to repeat.
pub fn startup() -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
    }
}

fn u32_variant(v: u32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_UI4;
        inner.Anonymous.ulVal = v;
    }
    var
}

fn bool_variant(v: bool) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = if v { VARIANT_TRUE } else { VARIANT_FALSE };
    }
    var
}

/// What `MFTEnumEx` lists: hardware HEVC encoders taking NV12, best first.
fn hardware_hevc_encoders() -> Result<Vec<IMFActivate>> {
    let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
    let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_HEVC };
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut list,
            &mut count,
        )?;
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            if let Some(a) = (*list.add(i)).take() {
                out.push(a);
            }
        }
        if !list.is_null() {
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(out)
    }
}

fn friendly_name(a: &IMFActivate) -> String {
    unsafe {
        let mut p = windows::core::PWSTR::null();
        let mut len = 0u32;
        if a.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut p, &mut len).is_ok() && !p.is_null() {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            return s;
        }
    }
    "an unnamed hardware HEVC encoder".into()
}

/// The hardware HEVC encoders this machine has, by name. Empty: the host cannot stream (ADR-4: no
/// software encoder).
pub fn probe() -> Result<Vec<String>> {
    startup()?;
    Ok(hardware_hevc_encoders()?.iter().map(friendly_name).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Bits per second for this stream (the setting is per 60 fps; the caller scales it).
    pub bitrate: u32,
}

/// One encoded output: Annex-B bytes as the MFT wrote them.
pub struct Output {
    pub annexb: Vec<u8>,
    /// The MFT marked it a clean point (the bitstream's NAL types decide in the end).
    pub clean_point: bool,
}

enum Event {
    NeedInput,
    HaveOutput,
    Failed(windows::core::Error),
}

struct SendBox<T>(T);
// SAFETY: Media Foundation's event generator is free-threaded (MTA); it is only moved to the
// event thread, never shared.
unsafe impl<T> Send for SendBox<T> {}

pub struct Encoder {
    mft: IMFTransform,
    codec: Option<ICodecAPI>,
    _manager: IMFDXGIDeviceManager,
    events: mpsc::Receiver<Event>,
    input_id: u32,
    output_id: u32,
    /// Inputs the MFT asked for and has not been given.
    need_input: u32,
    /// Outputs announced and not yet taken.
    have_output: u32,
    pub settings: Settings,
    pub name: String,
}

impl Encoder {
    pub fn new(device: &ID3D11Device, settings: Settings) -> Result<Encoder> {
        startup()?;
        let mut last = windows::core::Error::from(E_FAIL);
        for a in &hardware_hevc_encoders()? {
            match Self::with(a, device, settings) {
                Ok(e) => return Ok(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn with(activate: &IMFActivate, device: &ID3D11Device, s: Settings) -> Result<Encoder> {
        let name = friendly_name(activate);
        let mft: IMFTransform = unsafe { activate.ActivateObject()? };
        unsafe {
            let attrs = mft.GetAttributes()?;
            attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
        }
        let mut token = 0u32;
        let mut manager = None;
        unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager)? };
        let manager = manager.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        unsafe {
            manager.ResetDevice(device, token)?;
            mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
        }

        let (mut input_id, mut output_id) = (0u32, 0u32);
        unsafe {
            let (mut i, mut o) = ([0u32], [0u32]);
            match mft.GetStreamIDs(&mut i, &mut o) {
                Ok(()) => {
                    input_id = i[0];
                    output_id = o[0];
                }
                Err(e) if e.code() == E_NOTIMPL => {}
                Err(e) => return Err(e),
            }
        }

        let codec: Option<ICodecAPI> = mft.cast().ok();
        let set = |api: &GUID, v: VARIANT| {
            if let Some(c) = &codec {
                let _ = unsafe { c.SetValue(api, &v) };
            }
        };
        // Rate control before the output type: some encoders fix it there.
        set(&CODECAPI_AVEncCommonRateControlMode, u32_variant(eAVEncCommonRateControlMode_CBR.0 as u32));
        set(&CODECAPI_AVEncCommonMeanBitRate, u32_variant(s.bitrate));
        set(&CODECAPI_AVLowLatencyMode, bool_variant(true));
        set(&CODECAPI_AVEncMPVDefaultBPictureCount, u32_variant(0));
        set(&CODECAPI_AVEncMPVGOPSize, u32_variant(s.fps.saturating_mul(4)));
        // 0 is fastest, 100 best quality. Intel Arc at 1920×1200: 0 encodes in 17 ms, 33-50 in
        // 27 ms and 100 in 60 ms (with the 1 ms timer, `sill_capture::fine_timer`).
        set(&CODECAPI_AVEncCommonQualityVsSpeed, u32_variant(0));
        let frame_size = (u64::from(s.width) << 32) | u64::from(s.height);
        let frame_rate = (u64::from(s.fps) << 32) | 1;
        unsafe {
            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_HEVC)?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, s.bitrate)?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
            out.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32)?;
            let _ = out.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_0_255.0 as u32);
            let _ = out.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
            let _ = out.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32);
            let _ = out.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32);
            mft.SetOutputType(output_id, &out, 0)?;

            let inp = MFCreateMediaType()?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            inp.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
            inp.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
            inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            let _ = inp.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_0_255.0 as u32);
            let _ = inp.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
            mft.SetInputType(input_id, &inp, 0)?;
        }
        // Some encoders take these only once the types are set.
        set(&CODECAPI_AVLowLatencyMode, bool_variant(true));
        set(&CODECAPI_AVEncMPVDefaultBPictureCount, u32_variant(0));

        let generator: IMFMediaEventGenerator = mft.cast()?;
        let (tx, events) = mpsc::channel();
        let g = SendBox(generator);
        std::thread::Builder::new()
            .name("sill-encode-events".into())
            .spawn(move || {
                let g = g;
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                loop {
                    let ev = match unsafe { g.0.GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)) } {
                        Ok(ev) => ev,
                        Err(e) => {
                            let _ = tx.send(Event::Failed(e));
                            return;
                        }
                    };
                    let t = unsafe { ev.GetType() }.unwrap_or(0);
                    let status = unsafe { ev.GetStatus() }.unwrap_or(windows::core::HRESULT(0));
                    let e = if status.is_err() {
                        Event::Failed(status.into())
                    } else if t == METransformNeedInput.0 as u32 {
                        Event::NeedInput
                    } else if t == METransformHaveOutput.0 as u32 {
                        Event::HaveOutput
                    } else {
                        continue;
                    };
                    let stop = matches!(e, Event::Failed(_));
                    if tx.send(e).is_err() || stop {
                        return;
                    }
                }
            })
            .map_err(|_| windows::core::Error::from(E_FAIL))?;

        unsafe {
            mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        Ok(Encoder {
            mft,
            codec,
            _manager: manager,
            events,
            input_id,
            output_id,
            need_input: 0,
            have_output: 0,
            settings: s,
            name,
        })
    }

    /// The next input becomes an IDR (with VPS, SPS and PPS before it).
    pub fn force_keyframe(&self) {
        if let Some(c) = &self.codec {
            let _ = unsafe { c.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &u32_variant(1)) };
        }
    }

    /// Takes the MFT's pending events, waiting up to `wait` for the first.
    fn pump(&mut self, wait: Duration) -> Result<()> {
        let mut next = if wait.is_zero() { self.events.try_recv().ok() } else { self.events.recv_timeout(wait).ok() };
        while let Some(e) = next {
            match e {
                Event::NeedInput => self.need_input += 1,
                Event::HaveOutput => self.have_output += 1,
                Event::Failed(e) => return Err(e),
            }
            next = self.events.try_recv().ok();
        }
        Ok(())
    }

    /// Whether the MFT takes an input now (waiting up to `wait` for it to ask).
    pub fn ready(&mut self, wait: Duration) -> Result<bool> {
        if self.need_input == 0 {
            self.pump(wait)?;
        } else {
            self.pump(Duration::ZERO)?;
        }
        Ok(self.need_input > 0)
    }

    /// Outputs announced and not yet taken.
    pub fn pending_outputs(&mut self) -> Result<u32> {
        self.pump(Duration::ZERO)?;
        Ok(self.have_output)
    }

    /// Gives the MFT one NV12 texture; `time` and `duration` are in 100 ns units. Call only after
    /// `ready` said yes.
    pub fn submit(&mut self, nv12: &ID3D11Texture2D, time: i64, duration: i64) -> Result<()> {
        unsafe {
            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, nv12, 0, false)?;
            if let Ok(b2) = buffer.cast::<IMF2DBuffer>() {
                let len = b2.GetContiguousLength()?;
                buffer.SetCurrentLength(len)?;
            }
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(duration)?;
            self.mft.ProcessInput(self.input_id, &sample, 0)?;
        }
        self.need_input = self.need_input.saturating_sub(1);
        Ok(())
    }

    /// The next encoded output, waiting up to `wait` for the MFT to announce one.
    pub fn output(&mut self, wait: Duration) -> Result<Option<Output>> {
        if self.have_output == 0 {
            self.pump(wait)?;
        }
        if self.have_output == 0 {
            return Ok(None);
        }
        self.have_output -= 1;
        let mut buf = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: self.output_id,
            pSample: std::mem::ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: std::mem::ManuallyDrop::new(None),
        }];
        let mut status = 0u32;
        let r = unsafe { self.mft.ProcessOutput(0, &mut buf, &mut status) };
        let sample = unsafe { std::mem::ManuallyDrop::take(&mut buf[0].pSample) };
        let _events = unsafe { std::mem::ManuallyDrop::take(&mut buf[0].pEvents) };
        if let Err(e) = r {
            if e.code() == MF_E_TRANSFORM_STREAM_CHANGE {
                // The MFT changed its output type: take the one it offers and carry on.
                unsafe {
                    let t = self.mft.GetOutputAvailableType(self.output_id, 0)?;
                    self.mft.SetOutputType(self.output_id, &t, 0)?;
                }
                return Ok(None);
            }
            if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                return Ok(None);
            }
            return Err(e);
        }
        let Some(sample) = sample else { return Ok(None) };
        unsafe {
            let clean_point = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut p = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut p, None, Some(&mut len))?;
            let annexb = std::slice::from_raw_parts(p, len as usize).to_vec();
            buffer.Unlock()?;
            Ok(Some(Output { annexb, clean_point }))
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            // Ends the event thread's GetEvent with MF_E_SHUTDOWN.
            if let Ok(s) = self.mft.cast::<IMFShutdown>() {
                let _ = s.Shutdown();
            }
        }
    }
}
