//! Blt-model DXGI swap chains for wallpaper windows.
//!
//! DWM may promote a flip-model swap chain that covers a monitor to a hardware
//! overlay plane (independent flip), and on some drivers (Intel Gen9) the
//! taskbar above it blinks out whenever DWM switches planes. A blt-model swap
//! chain is always composed by DWM, so the shell stays on top. Every renderer
//! that draws with the GPU presents through one of these.

use windows::core::Interface;
use windows::Win32::Foundation::{E_FAIL, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

pub fn create_device() -> windows::core::Result<ID3D11Device> {
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
        let device = device.ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            let _ = mt.SetMultithreadProtected(true);
        }
        Ok(device)
    }
}

/// A `w`×`h` swap chain on `hwnd`; DXGI stretches it to the window.
pub fn create_swapchain(
    device: &ID3D11Device,
    hwnd: HWND,
    (w, h): (u32, u32),
    format: DXGI_FORMAT,
) -> windows::core::Result<IDXGISwapChain1> {
    unsafe {
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = dxgi.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w.max(1),
            Height: h.max(1),
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 1,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let sc = factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None)?;
        let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
        Ok(sc)
    }
}

/// Uploads CPU-side RGBA frames into a blt-model swap chain.
pub struct Presenter {
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    _device: ID3D11Device,
}

impl Presenter {
    pub fn new(hwnd: HWND, size: (u32, u32)) -> windows::core::Result<Self> {
        let device = create_device()?;
        let swapchain = create_swapchain(&device, hwnd, size, DXGI_FORMAT_R8G8B8A8_UNORM)?;
        let context = unsafe { device.GetImmediateContext()? };
        Ok(Self { context, swapchain, _device: device })
    }

    pub fn resize(&mut self, (w, h): (u32, u32)) -> windows::core::Result<()> {
        unsafe { self.swapchain.ResizeBuffers(0, w.max(1), h.max(1), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) }
    }

    /// Copy one frame (rows `row_pitch` bytes apart, swap chain sized) and show it.
    pub fn present(&self, pixels: &[u8], row_pitch: u32) -> windows::core::Result<()> {
        unsafe {
            let target: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            self.context.UpdateSubresource(&target, 0, None, pixels.as_ptr().cast(), row_pitch, 0);
            drop(target);
            // No vsync wait: DWM composes blt-model content at its own cadence,
            // and blocking here would count against the frame-time governor.
            self.swapchain.Present(0, DXGI_PRESENT(0)).ok()
        }
    }
}
