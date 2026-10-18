//! Video wallpapers through the Media Foundation Media Engine in frame-server
//! mode: the engine decodes on the GPU (DXGI device manager) and we copy each
//! new frame into a flip-model swap chain, placed according to the wallpaper's
//! display settings (fit, zoom, pan). The swap chain may be smaller than the
//! monitor (`Display::resolution`); DXGI stretches it to the window.

use std::path::Path;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nouwall_core::{tr, Display, Kind};
use windows::core::{implement, Interface, BSTR};
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use super::{RenderOptions, Renderer};

#[derive(Default)]
struct EngineState {
    /// Last MF_MEDIA_ENGINE_ERR code (0 = none).
    error: AtomicU32,
    hresult: AtomicI32,
    loaded: AtomicU32,
}

#[implement(IMFMediaEngineNotify)]
struct Notify {
    state: Arc<EngineState>,
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, param2: u32) -> windows::core::Result<()> {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 {
            self.state.error.store(param1 as u32, Ordering::Relaxed);
            self.state.hresult.store(param2 as i32, Ordering::Relaxed);
        } else if event == MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA.0 as u32 {
            self.state.loaded.store(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

pub struct VideoRenderer {
    engine: IMFMediaEngine,
    _manager: IMFDXGIDeviceManager,
    _device: ID3D11Device,
    swapchain: IDXGISwapChain1,
    state: Arc<EngineState>,
    /// Window (monitor) size; placement is computed against it.
    size: (u32, u32),
    /// Swap chain size: `size` capped by `display.resolution`.
    swap_size: (u32, u32),
    display: Display,
    tick: Duration,
    next: Instant,
    last_pts: i64,
    paused: bool,
    frames: u64,
    reported_error: bool,
}

fn create_device() -> windows::core::Result<ID3D11Device> {
    unsafe {
        let mut device = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let device = device.ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            let _ = mt.SetMultithreadProtected(true);
        }
        Ok(device)
    }
}

fn create_swapchain(device: &ID3D11Device, hwnd: HWND, (w, h): (u32, u32)) -> windows::core::Result<IDXGISwapChain1> {
    unsafe {
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = dxgi.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w.max(1),
            Height: h.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let sc = factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None)?;
        let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
        Ok(sc)
    }
}

fn error_name(code: u32) -> String {
    match code {
        1 => tr!("aborted", "dibatalkan"),
        2 => tr!("network", "jaringan"),
        3 => tr!("decode failed", "gagal decode"),
        4 => tr!("format not supported", "format tidak didukung"),
        5 => tr!("encrypted", "terenkripsi"),
        _ => tr!("unknown", "tidak diketahui"),
    }
}

impl VideoRenderer {
    pub fn new(
        hwnd: HWND,
        size: (u32, u32),
        path: &Path,
        video_fps: u32,
        display: Display,
        opts: RenderOptions,
    ) -> windows::core::Result<Self> {
        let display = display.sanitized();
        unsafe {
            let device = create_device()?;
            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager: IMFDXGIDeviceManager =
                manager.ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
            manager.ResetDevice(&device, token)?;

            let state = Arc::new(EngineState::default());
            let notify: IMFMediaEngineNotify = Notify { state: state.clone() }.into();

            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3)?;
            let attrs = attrs.ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
            attrs.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &manager)?;
            attrs.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)?;
            attrs.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32)?;

            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
            let engine = factory.CreateInstance(0, &attrs)?;
            engine.SetLoop(true)?;
            engine.SetMuted(opts.mute)?;
            engine.SetVolume(f64::from(opts.volume.clamp(0.0, 1.0)))?;
            engine.SetAutoPlay(true)?;
            engine.SetSource(&BSTR::from(path.to_string_lossy().as_ref()))?;
            engine.Play()?;

            let swap_size = display.render_size(size);
            let swapchain = create_swapchain(&device, hwnd, swap_size)?;
            // Tick at twice the video rate (capped) so frames are picked up promptly.
            let fps = if video_fps == 0 { 30 } else { video_fps };
            let tick = Duration::from_secs_f64(1.0 / f64::from((fps * 2).clamp(20, 120)));
            Ok(Self {
                engine,
                _manager: manager,
                _device: device,
                swapchain,
                state,
                size,
                swap_size,
                display,
                tick,
                next: Instant::now(),
                last_pts: -1,
                paused: false,
                frames: 0,
                reported_error: false,
            })
        }
    }

    fn present_frame(&mut self) -> windows::core::Result<bool> {
        unsafe {
            let Ok(pts) = self.engine.OnVideoStreamTick() else { return Ok(false) };
            if pts == self.last_pts || pts < 0 {
                return Ok(false);
            }
            let (mut vw, mut vh) = (0u32, 0u32);
            self.engine.GetNativeVideoSize(Some(&mut vw), Some(&mut vh))?;
            if vw == 0 || vh == 0 {
                return Ok(false);
            }
            self.last_pts = pts;
            let target: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            let placement = self.display.place((vw, vh), self.size);
            // Bars (contain, zoom out, centre) must be black, not last frame's pixels.
            if placement.is_none_or(|p| !p.dst.is_full()) {
                self.clear(&target)?;
            }
            if let Some(p) = placement {
                let (w, h) = (self.swap_size.0 as f32, self.swap_size.1 as f32);
                let src = MFVideoNormalizedRect { left: p.src.left, top: p.src.top, right: p.src.right, bottom: p.src.bottom };
                let dst = RECT {
                    left: (p.dst.left * w).round() as i32,
                    top: (p.dst.top * h).round() as i32,
                    right: (p.dst.right * w).round() as i32,
                    bottom: (p.dst.bottom * h).round() as i32,
                };
                let border = MFARGB { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbAlpha: 255 };
                self.engine.TransferVideoFrame(&target, Some(&src), &dst, Some(&border))?;
            }
            drop(target);
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.frames += 1;
            Ok(true)
        }
    }
}

impl VideoRenderer {
    fn clear(&self, target: &ID3D11Texture2D) -> windows::core::Result<()> {
        unsafe {
            let mut rtv = None;
            self._device.CreateRenderTargetView(target, None, Some(&mut rtv))?;
            if let (Some(rtv), Ok(ctx)) = (rtv, self._device.GetImmediateContext()) {
                ctx.ClearRenderTargetView(&rtv, &[0.0, 0.0, 0.0, 1.0]);
            }
        }
        Ok(())
    }
}

impl Renderer for VideoRenderer {
    fn kind(&self) -> Option<Kind> {
        Some(Kind::Video)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        self.swap_size = self.display.render_size(self.size);
        unsafe {
            let (w, h) = self.swap_size;
            if let Err(e) = self.swapchain.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) {
                log::warn!("video resize: {e}");
            }
        }
        self.last_pts = -1;
    }

    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        unsafe {
            let _ = if paused { self.engine.Pause() } else { self.engine.Play() };
        }
        self.next = Instant::now();
    }

    fn frame(&mut self, now: Instant) -> Option<Instant> {
        if self.paused {
            return None;
        }
        if now < self.next {
            return Some(self.next);
        }
        let err = self.state.error.load(Ordering::Relaxed);
        if err != 0 {
            if !self.reported_error {
                self.reported_error = true;
                log::error!(
                    "video error: {} (hr 0x{:08x})",
                    error_name(err),
                    self.state.hresult.load(Ordering::Relaxed) as u32
                );
            }
            return None;
        }
        if let Err(e) = self.present_frame() {
            log::warn!("video frame: {e}");
        }
        self.next = (self.next + self.tick).max(now + self.tick / 4);
        Some(self.next)
    }

    fn status(&self) -> String {
        let err = self.state.error.load(Ordering::Relaxed);
        if err != 0 {
            tr!("error: {}", "galat: {}", error_name(err))
        } else if self.paused {
            tr!("paused", "dijeda")
        } else if self.state.loaded.load(Ordering::Relaxed) == 0 {
            tr!("loading", "memuat")
        } else {
            tr!("playing ({} frames, render {}x{})", "diputar ({} frame, render {}x{})", self.frames, self.swap_size.0, self.swap_size.1)
        }
    }

    fn set_audio(&mut self, mute: bool, volume: f32) {
        unsafe {
            let _ = self.engine.SetMuted(mute);
            let _ = self.engine.SetVolume(f64::from(volume.clamp(0.0, 1.0)));
        }
    }
}

impl Drop for VideoRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
        }
    }
}
