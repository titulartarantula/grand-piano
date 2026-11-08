//! WASAPI output written directly against the Windows API.
//!
//! Why not cpal: cpal's WASAPI host always uses plain shared mode at the
//! engine's default period (10 ms), which can't get under the 10 ms target.
//! This module supports:
//! - shared mode through IAudioClient3 with the smallest engine period the
//!   driver allows (often 128-256 frames at 48 kHz on Windows' in-box drivers),
//! - exclusive mode, event-driven, at the requested buffer size,
//! - plain shared mode as a fallback.

use grand_piano::settings::AudioMode;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use windows::Win32::Devices::Properties;
use windows::Win32::Foundation::{HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ};
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::core::{GUID, HSTRING, Interface, PCWSTR};

/// Something that produces interleaved stereo f32.
pub trait Render: Send {
    fn render(&mut self, out: &mut [f32]);
}

impl Render for grand_piano::engine::Engine {
    fn render(&mut self, out: &mut [f32]) {
        self.process(out)
    }
}

pub struct Silence;
impl Render for Silence {
    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0)
    }
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub device_name: String,
    pub mode: AudioMode,
    pub sample_rate: u32,
    pub format: String,
    /// frames per callback (engine period / exclusive buffer)
    pub period_frames: u32,
    /// size of the WASAPI buffer in frames
    pub buffer_frames: u32,
    /// driver-reported stream latency (ms)
    pub stream_latency_ms: f32,
    /// smallest/default shared-mode engine periods the driver offers (frames), if known
    pub engine_periods: Option<(u32, u32, u32, u32)>,
    /// estimated key-to-sound output latency (ms) excluding the USB MIDI hop
    pub estimated_latency_ms: f32,
    pub note: String,
}

pub struct AudioHandle {
    stop: Arc<AtomicBool>,
    pub failed: Arc<AtomicBool>,
    pub error: Arc<Mutex<Option<String>>>,
    pub info: StreamInfo,
    /// callbacks that arrived late (buffer ran dry)
    pub glitches: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct ComInit;
fn com_init() -> ComInit {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    ComInit
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

unsafe fn device_id(dev: &IMMDevice) -> windows::core::Result<String> {
    unsafe {
        let p = dev.GetId()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        Ok(s)
    }
}

unsafe fn device_name(dev: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = dev.OpenPropertyStore(STGM_READ) else { return "Unknown device".into() };
        let key = &Properties::DEVPKEY_Device_FriendlyName as *const _ as *const PROPERTYKEY;
        let Ok(mut pv) = store.GetValue(key) else { return "Unknown device".into() };
        let inner = &pv.Anonymous.Anonymous;
        let mut out = String::from("Unknown device");
        if inner.vt == VT_LPWSTR {
            let p = inner.Anonymous.pwszVal;
            if !p.is_null() {
                out = p.to_string().unwrap_or(out);
            }
        }
        let _ = PropVariantClear(&mut pv);
        out
    }
}

pub fn list_output_devices() -> anyhow::Result<Vec<DeviceInfo>> {
    let _c = com_init();
    unsafe {
        let en = enumerator()?;
        let default_id = en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| device_id(&d).ok());
        let coll = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut out = Vec::new();
        for i in 0..coll.GetCount()? {
            let d = coll.Item(i)?;
            let id = device_id(&d)?;
            out.push(DeviceInfo { is_default: Some(&id) == default_id.as_ref(), name: device_name(&d), id });
        }
        out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        Ok(out)
    }
}

/// Owned copy of a WAVEFORMATEXTENSIBLE.
#[derive(Clone, Copy)]
struct Format {
    ext: WAVEFORMATEXTENSIBLE,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SampleKind {
    F32,
    I16,
    I24Packed,
    I32 { valid: u16 },
}

impl Format {
    unsafe fn from_ptr(p: *const WAVEFORMATEX) -> Format {
        unsafe {
            let base = *p;
            if base.wFormatTag as u32 == WAVE_FORMAT_EXTENSIBLE && base.cbSize >= 22 {
                Format { ext: *(p as *const WAVEFORMATEXTENSIBLE) }
            } else {
                let sub = if base.wFormatTag as u32 == WAVE_FORMAT_IEEE_FLOAT { KSDATAFORMAT_SUBTYPE_IEEE_FLOAT } else { KSDATAFORMAT_SUBTYPE_PCM };
                Format::new(base.nSamplesPerSec, base.nChannels, base.wBitsPerSample, base.wBitsPerSample, sub)
            }
        }
    }
    fn new(rate: u32, channels: u16, container: u16, valid: u16, sub: GUID) -> Format {
        let block = channels * container / 8;
        let ext = WAVEFORMATEXTENSIBLE {
            Format: WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
                nChannels: channels,
                nSamplesPerSec: rate,
                nAvgBytesPerSec: rate * block as u32,
                nBlockAlign: block,
                wBitsPerSample: container,
                cbSize: 22,
            },
            Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: valid },
            dwChannelMask: if channels == 2 { 0x3 } else { 0 },
            SubFormat: sub,
        };
        Format { ext }
    }
    fn ptr(&self) -> *const WAVEFORMATEX {
        &self.ext as *const _ as *const WAVEFORMATEX
    }
    fn rate(&self) -> u32 {
        self.ext.Format.nSamplesPerSec
    }
    fn channels(&self) -> u16 {
        self.ext.Format.nChannels
    }
    fn block(&self) -> usize {
        self.ext.Format.nBlockAlign as usize
    }
    fn kind(&self) -> Option<SampleKind> {
        let bits = self.ext.Format.wBitsPerSample;
        let valid = unsafe { self.ext.Samples.wValidBitsPerSample };
        let sub = self.ext.SubFormat;
        if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && bits == 32 {
            Some(SampleKind::F32)
        } else if sub == KSDATAFORMAT_SUBTYPE_PCM {
            match bits {
                16 => Some(SampleKind::I16),
                24 => Some(SampleKind::I24Packed),
                32 => Some(SampleKind::I32 { valid: if valid == 0 { 32 } else { valid } }),
                _ => None,
            }
        } else {
            None
        }
    }
    fn describe(&self) -> String {
        let k = match self.kind() {
            Some(SampleKind::F32) => "32-bit float".to_string(),
            Some(SampleKind::I16) => "16-bit".to_string(),
            Some(SampleKind::I24Packed) => "24-bit".to_string(),
            Some(SampleKind::I32 { valid }) => format!("{valid}-bit in 32"),
            None => "unknown format".to_string(),
        };
        format!("{} Hz, {} ch, {}", self.rate(), self.channels(), k)
    }
}

/// Converts interleaved stereo f32 into the device buffer.
unsafe fn write_frames(dst: *mut u8, src: &[f32], frames: usize, fmt: &Format, kind: SampleKind) {
    unsafe {
        let ch = fmt.channels() as usize;
        let block = fmt.block();
        let bps = block / ch;
        for f in 0..frames {
            for c in 0..ch {
                let v = if c < 2 { src[f * 2 + c] } else { 0.0 };
                let v = v.clamp(-1.0, 1.0);
                let p = dst.add(f * block + c * bps);
                match kind {
                    SampleKind::F32 => (p as *mut f32).write_unaligned(v),
                    SampleKind::I16 => (p as *mut i16).write_unaligned((v * 32767.0) as i16),
                    SampleKind::I24Packed => {
                        let x = (v * 8_388_607.0) as i32;
                        *p = x as u8;
                        *p.add(1) = (x >> 8) as u8;
                        *p.add(2) = (x >> 16) as u8;
                    }
                    SampleKind::I32 { .. } => (p as *mut i32).write_unaligned((v as f64 * 2_147_483_647.0) as i32),
                }
            }
        }
    }
}

struct Opened {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    fmt: Format,
    kind: SampleKind,
    info: StreamInfo,
    exclusive: bool,
}

const HNS_PER_SEC: f64 = 10_000_000.0;

unsafe fn open_device(device_id: Option<&str>) -> anyhow::Result<IMMDevice> {
    unsafe {
        let en = enumerator()?;
        if let Some(id) = device_id {
            if let Ok(d) = en.GetDevice(&HSTRING::from(id)) {
                return Ok(d);
            }
            log::warn!("audio device {id} not found, using the default device");
        }
        Ok(en.GetDefaultAudioEndpoint(eRender, eConsole)?)
    }
}

unsafe fn open_stream(device_id: Option<&str>, mode: AudioMode, want_frames: u32) -> anyhow::Result<Opened> {
    unsafe {
        let dev = open_device(device_id)?;
        let name = device_name(&dev);
        let event = CreateEventW(None, false, false, PCWSTR::null())?;
        match mode {
            AudioMode::Auto => {
                // Prefer low-latency shared mode (other apps keep sound) when the
                // driver really offers small periods; otherwise go exclusive.
                let shared = open_shared(&dev, name.clone(), event, want_frames, true)?;
                if shared.info.mode == AudioMode::SharedLowLatency && shared.info.period_frames as f32 / shared.info.sample_rate as f32 <= 0.0055 {
                    return Ok(shared);
                }
                drop(shared);
                match open_exclusive(&dev, name.clone(), event, want_frames) {
                    Ok(mut o) => {
                        o.info.note = format!("Auto chose exclusive mode: the driver's shared-mode period is too long for low latency. {}", o.info.note);
                        Ok(o)
                    }
                    Err(e) => {
                        let mut o = open_shared(&dev, name, event, want_frames, true)?;
                        o.info.note = format!("exclusive mode unavailable ({e}); {}", o.info.note);
                        Ok(o)
                    }
                }
            }
            AudioMode::Exclusive => open_exclusive(&dev, name, event, want_frames),
            AudioMode::SharedLowLatency => open_shared(&dev, name, event, want_frames, true),
            AudioMode::Shared => open_shared(&dev, name, event, want_frames, false),
        }
    }
}

unsafe fn open_shared(dev: &IMMDevice, name: String, event: HANDLE, want_frames: u32, low_latency: bool) -> anyhow::Result<Opened> {
    unsafe {
        let client3: IAudioClient3 = dev.Activate(CLSCTX_ALL, None)?;
        let mix_ptr = client3.GetMixFormat()?;
        let fmt = Format::from_ptr(mix_ptr);
        CoTaskMemFree(Some(mix_ptr as *const _));
        let kind = fmt.kind().ok_or_else(|| anyhow::anyhow!("unsupported mix format"))?;
        let (mut def, mut fund, mut minp, mut maxp) = (0u32, 0u32, 0u32, 0u32);
        let periods = client3
            .GetSharedModeEnginePeriod(fmt.ptr(), &mut def, &mut fund, &mut minp, &mut maxp)
            .ok()
            .map(|_| (def, fund, minp, maxp));
        let mut note = String::new();
        let mut used_low = false;
        if low_latency {
            if let Some((def, fund, minp, maxp)) = periods {
                // smallest allowed period >= requested buffer (multiples of the fundamental)
                let mut p = minp.max(1);
                while p < want_frames && p + fund <= maxp && fund > 0 {
                    p += fund;
                }
                if minp < def {
                    client3.InitializeSharedAudioStream(AUDCLNT_STREAMFLAGS_EVENTCALLBACK, p, fmt.ptr(), None)?;
                    used_low = true;
                } else {
                    note = format!("driver offers no period below {def} frames; low-latency shared mode not available");
                }
            }
        }
        if !used_low {
            client3.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 0, 0, fmt.ptr(), None)?;
        }
        let client: IAudioClient = client3.cast()?;
        client.SetEventHandle(event)?;
        let buffer = client.GetBufferSize()?;
        let mut period_frames = 0u32;
        let mut cur_fmt = std::ptr::null_mut();
        if client3.GetCurrentSharedModeEnginePeriod(&mut cur_fmt, &mut period_frames).is_ok() && !cur_fmt.is_null() {
            CoTaskMemFree(Some(cur_fmt as *const _));
        }
        if period_frames == 0 {
            let mut dp = 0i64;
            client.GetDevicePeriod(Some(&mut dp), None)?;
            period_frames = (dp as f64 / HNS_PER_SEC * fmt.rate() as f64).round() as u32;
        }
        let stream_latency_ms = client.GetStreamLatency().map(|h| h as f32 / 10_000.0).unwrap_or(0.0);
        let render: IAudioRenderClient = client.GetService()?;
        let rate = fmt.rate() as f32;
        // one period of rendering ahead + one engine period + driver latency + limiter look-ahead
        let est = 2.0 * period_frames as f32 / rate * 1000.0 + stream_latency_ms + 1.0;
        let info = StreamInfo {
            device_name: name,
            mode: if used_low { AudioMode::SharedLowLatency } else { AudioMode::Shared },
            sample_rate: fmt.rate(),
            format: fmt.describe(),
            period_frames,
            buffer_frames: buffer,
            stream_latency_ms,
            engine_periods: periods,
            estimated_latency_ms: est,
            note,
        };
        Ok(Opened { client, render, event, fmt, kind, info, exclusive: false })
    }
}

unsafe fn open_exclusive(dev: &IMMDevice, name: String, event: HANDLE, want_frames: u32) -> anyhow::Result<Opened> {
    unsafe {
        let probe: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let mix_ptr = probe.GetMixFormat()?;
        let mix = Format::from_ptr(mix_ptr);
        CoTaskMemFree(Some(mix_ptr as *const _));
        let mut rates = vec![mix.rate(), 48000, 44100];
        rates.dedup();
        let candidates = [
            (32u16, 32u16, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT),
            (32, 24, KSDATAFORMAT_SUBTYPE_PCM),
            (24, 24, KSDATAFORMAT_SUBTYPE_PCM),
            (32, 32, KSDATAFORMAT_SUBTYPE_PCM),
            (16, 16, KSDATAFORMAT_SUBTYPE_PCM),
        ];
        let mut chosen = None;
        'outer: for &rate in &rates {
            for &(container, valid, sub) in &candidates {
                let f = Format::new(rate, 2, container, valid, sub);
                if probe.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, f.ptr(), None).is_ok() {
                    chosen = Some(f);
                    break 'outer;
                }
            }
        }
        let fmt = chosen.ok_or_else(|| anyhow::anyhow!("device has no stereo exclusive-mode format we can use"))?;
        let kind = fmt.kind().unwrap();
        let (mut def, mut minp) = (0i64, 0i64);
        probe.GetDevicePeriod(Some(&mut def), Some(&mut minp))?;
        let want_hns = (want_frames as f64 / fmt.rate() as f64 * HNS_PER_SEC).round() as i64;
        let mut hns = want_hns.max(minp);
        let mut client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        if let Err(e) = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, hns, hns, fmt.ptr(), None) {
            if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                let frames = client.GetBufferSize()?;
                hns = (frames as f64 / fmt.rate() as f64 * HNS_PER_SEC).round() as i64;
                client = dev.Activate(CLSCTX_ALL, None)?;
                client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, hns, hns, fmt.ptr(), None)?;
            } else {
                return Err(e.into());
            }
        }
        client.SetEventHandle(event)?;
        let buffer = client.GetBufferSize()?;
        let stream_latency_ms = client.GetStreamLatency().map(|h| h as f32 / 10_000.0).unwrap_or(0.0);
        let render: IAudioRenderClient = client.GetService()?;
        let rate = fmt.rate() as f32;
        let est = 2.0 * buffer as f32 / rate * 1000.0 + 1.0;
        let info = StreamInfo {
            device_name: name,
            mode: AudioMode::Exclusive,
            sample_rate: fmt.rate(),
            format: fmt.describe(),
            period_frames: buffer,
            buffer_frames: buffer,
            stream_latency_ms,
            engine_periods: None,
            estimated_latency_ms: est,
            note: format!("device minimum period {:.2} ms", minp as f64 / 10_000.0),
        };
        Ok(Opened { client, render, event, fmt, kind, info, exclusive: true })
    }
}

/// Opens the device and starts the render thread. `make` builds the renderer once
/// the sample rate is known. Falls back to plain shared mode if the requested mode
/// fails (the reason ends up in `info.note`).
pub fn start<F>(device_id: Option<String>, mode: AudioMode, want_frames: u32, make: F) -> anyhow::Result<AudioHandle>
where
    F: FnOnce(u32) -> Box<dyn Render> + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let glitches = Arc::new(AtomicU32::new(0));
    let (tx, rx) = mpsc::channel::<anyhow::Result<StreamInfo>>();
    let (s2, f2, e2, g2) = (stop.clone(), failed.clone(), error.clone(), glitches.clone());
    let thread = std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let _c = com_init();
            let opened = unsafe {
                match open_stream(device_id.as_deref(), mode, want_frames) {
                    Ok(o) => Ok(o),
                    Err(e) if mode != AudioMode::Shared => {
                        log::warn!("{} failed: {e:#}; falling back to standard shared mode", mode.label());
                        open_stream(device_id.as_deref(), AudioMode::Shared, want_frames).map(|mut o| {
                            o.info.note = format!("{} unavailable ({e}); using standard shared mode", mode.label());
                            o
                        })
                    }
                    Err(e) => Err(e),
                }
            };
            let o = match opened {
                Ok(o) => o,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let renderer = make(o.info.sample_rate);
            let _ = tx.send(Ok(o.info.clone()));
            if let Err(e) = unsafe { run(o, renderer, &s2, &g2) } {
                log::error!("audio stopped: {e:#}");
                *e2.lock().unwrap() = Some(format!("{e:#}"));
                f2.store(true, Ordering::Relaxed);
            }
        })?;
    let info = rx.recv().map_err(|_| anyhow::anyhow!("audio thread exited"))??;
    Ok(AudioHandle { stop, failed, error, info, glitches, thread: Some(thread) })
}

unsafe fn run(o: Opened, mut r: Box<dyn Render>, stop: &AtomicBool, glitches: &AtomicU32) -> anyhow::Result<()> {
    unsafe {
        let mut task_index = 0u32;
        let mmcss = AvSetMmThreadCharacteristicsW(&HSTRING::from("Pro Audio"), &mut task_index).ok();
        let buffer = o.info.buffer_frames as usize;
        // Shared mode: keep only ~2 periods queued, however big the WASAPI buffer is,
        // so latency follows the engine period rather than the buffer size.
        let target = if o.exclusive { buffer } else { (2 * o.info.period_frames as usize).clamp(1, buffer) };
        let mut scratch = vec![0f32; buffer * 2];
        // pre-fill with silence (exclusive: whole buffer, shared: one period)
        let pre = target;
        let p = o.render.GetBuffer(pre as u32)?;
        std::ptr::write_bytes(p, 0, pre * o.fmt.block());
        o.render.ReleaseBuffer(pre as u32, 0)?;
        o.client.Start()?;
        let result = (|| -> anyhow::Result<()> {
            while !stop.load(Ordering::Relaxed) {
                let w = WaitForSingleObject(o.event, 200);
                if w != WAIT_OBJECT_0 {
                    continue; // timeout: check the stop flag
                }
                let frames = if o.exclusive {
                    buffer
                } else {
                    let padding = o.client.GetCurrentPadding()? as usize;
                    if padding == 0 {
                        glitches.fetch_add(1, Ordering::Relaxed);
                    }
                    target.saturating_sub(padding)
                };
                if frames == 0 {
                    continue;
                }
                let out = &mut scratch[..frames * 2];
                r.render(out);
                let p = o.render.GetBuffer(frames as u32)?;
                write_frames(p, out, frames, &o.fmt, o.kind);
                o.render.ReleaseBuffer(frames as u32, 0)?;
            }
            Ok(())
        })();
        let _ = o.client.Stop();
        if let Some(h) = mmcss {
            let _ = AvRevertMmThreadCharacteristics(h);
        }
        let _ = windows::Win32::Foundation::CloseHandle(o.event);
        result
    }
}
