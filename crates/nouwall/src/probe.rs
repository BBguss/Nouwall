//! Fill `HardwareInfo` from DXGI, D3D11 video decoders, monitors and power state.

use nouwall_core::{Codec, DecodeCaps, GpuInfo, GpuKind, HardwareInfo, MonitorInfo, PowerSource};
use windows::core::{Interface, GUID};
use windows::core::BOOL;
use windows::Win32::Foundation::{HMODULE, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_12_0,
    D3D_FEATURE_LEVEL_12_1,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, ID3D12Device, D3D12_FEATURE_ARCHITECTURE, D3D12_FEATURE_DATA_ARCHITECTURE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, DEVMODEW, ENUM_CURRENT_SETTINGS, HDC, HMONITOR,
    MONITORINFOEXW,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use crate::util::{from_wide, wide};

const VENDOR_INTEL: u32 = 0x8086;
const VENDOR_MICROSOFT: u32 = 0x1414;

pub fn probe() -> HardwareInfo {
    let (gpu, adapter) = probe_gpu();
    let decode = adapter.as_ref().map(probe_decode).unwrap_or_default();
    let (power, battery_percent) = power_status();
    HardwareInfo {
        gpu,
        decode,
        monitors: monitors(),
        logical_cpus: logical_cpus(),
        ram_mb: ram_mb(),
        power,
        battery_percent,
        os_build: os_build(),
    }
}

fn probe_gpu() -> (GpuInfo, Option<IDXGIAdapter1>) {
    let unknown = GpuInfo {
        name: "Unknown".into(),
        vendor_id: 0,
        kind: GpuKind::Software,
        dedicated_vram_mb: 0,
        feature_level: 0,
    };
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return (unknown, None);
    };
    // Adapter 0 drives the primary display; that is where video decode and
    // composition happen, so it is the one the tier is based on.
    let mut chosen = None;
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
        if chosen.is_none() || (!software && i == 0) {
            chosen = Some((adapter, desc, software));
        }
        if i == 0 && !software {
            break;
        }
    }
    let Some((adapter, desc, software)) = chosen else { return (unknown, None) };
    let name = from_wide(&desc.Description);
    let vendor_id = desc.VendorId;
    let dedicated_vram_mb = (desc.DedicatedVideoMemory / (1024 * 1024)) as u64;
    let kind = if software || vendor_id == VENDOR_MICROSOFT {
        GpuKind::Software
    } else {
        match is_uma(&adapter) {
            Some(true) => GpuKind::Integrated,
            Some(false) => GpuKind::Discrete,
            None if vendor_id == VENDOR_INTEL && !name.contains("Arc(TM) A") => GpuKind::Integrated,
            None if dedicated_vram_mb >= 2048 => GpuKind::Discrete,
            None => GpuKind::Integrated,
        }
    };
    let feature_level = d3d11_feature_level(&adapter);
    (GpuInfo { name, vendor_id, kind, dedicated_vram_mb, feature_level }, Some(adapter))
}

/// D3D12 reports unified memory architecture for integrated GPUs.
fn is_uma(adapter: &IDXGIAdapter1) -> Option<bool> {
    unsafe {
        let mut device: Option<ID3D12Device> = None;
        D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device).ok()?;
        let device = device?;
        let mut arch = D3D12_FEATURE_DATA_ARCHITECTURE::default();
        device
            .CheckFeatureSupport(
                D3D12_FEATURE_ARCHITECTURE,
                &mut arch as *mut _ as *mut _,
                std::mem::size_of::<D3D12_FEATURE_DATA_ARCHITECTURE>() as u32,
            )
            .ok()?;
        Some(arch.UMA.as_bool())
    }
}

const LEVELS: [D3D_FEATURE_LEVEL; 4] =
    [D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];

fn create_d3d11(adapter: &IDXGIAdapter1, flags: D3D11_CREATE_DEVICE_FLAG) -> Option<(ID3D11Device, D3D_FEATURE_LEVEL)> {
    unsafe {
        let mut device = None;
        let mut level = D3D_FEATURE_LEVEL::default();
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            flags,
            Some(&LEVELS),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            None,
        )
        .ok()?;
        Some((device?, level))
    }
}

fn d3d11_feature_level(adapter: &IDXGIAdapter1) -> u32 {
    create_d3d11(adapter, D3D11_CREATE_DEVICE_FLAG(0)).map(|(_, l)| l.0 as u32).unwrap_or(0)
}

const PROFILES: [(Codec, GUID); 4] = [
    (Codec::H264, D3D11_DECODER_PROFILE_H264_VLD_NOFGT),
    (Codec::Hevc, D3D11_DECODER_PROFILE_HEVC_VLD_MAIN),
    (Codec::Vp9, D3D11_DECODER_PROFILE_VP9_VLD_PROFILE0),
    (Codec::Av1, D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0),
];

const SIZES: [(u32, u32); 6] = [(8192, 4320), (7680, 4320), (4096, 2304), (3840, 2160), (2560, 1440), (1920, 1080)];

/// Ask the driver which decode profiles exist and the largest size each accepts.
fn probe_decode(adapter: &IDXGIAdapter1) -> Vec<DecodeCaps> {
    let Some((device, _)) = create_d3d11(adapter, D3D11_CREATE_DEVICE_VIDEO_SUPPORT) else { return Vec::new() };
    let Ok(video) = device.cast::<ID3D11VideoDevice>() else { return Vec::new() };
    let available: Vec<GUID> = unsafe {
        (0..video.GetVideoDecoderProfileCount()).filter_map(|i| video.GetVideoDecoderProfile(i).ok()).collect()
    };
    let mut caps = Vec::new();
    for (codec, guid) in PROFILES {
        if !available.contains(&guid) {
            continue;
        }
        let nv12 = unsafe { video.CheckVideoDecoderFormat(&guid, DXGI_FORMAT_NV12) }.map(|b| b.as_bool()).unwrap_or(false);
        if !nv12 {
            continue;
        }
        let max = SIZES.iter().copied().find(|&(w, h)| {
            let desc = D3D11_VIDEO_DECODER_DESC { Guid: guid, SampleWidth: w, SampleHeight: h, OutputFormat: DXGI_FORMAT_NV12 };
            unsafe { video.GetVideoDecoderConfigCount(&desc) }.map(|n| n > 0).unwrap_or(false)
        });
        let (max_width, max_height) = max.unwrap_or((1280, 720));
        caps.push(DecodeCaps { codec, max_width, max_height });
    }
    caps
}

pub fn monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HMONITOR>);
        list.push(h);
        TRUE
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let mut out: Vec<MonitorInfo> = handles
        .into_iter()
        .filter_map(|h| unsafe {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(h, &mut info as *mut _ as *mut _).as_bool() {
                return None;
            }
            let r = info.monitorInfo.rcMonitor;
            let device = from_wide(&info.szDevice);
            let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
            let dev = wide(&device);
            let refresh = if EnumDisplaySettingsW(windows::core::PCWSTR(dev.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool() {
                mode.dmDisplayFrequency
            } else {
                60
            };
            Some(MonitorInfo {
                index: 0,
                device,
                x: r.left,
                y: r.top,
                width: (r.right - r.left) as u32,
                height: (r.bottom - r.top) as u32,
                refresh_hz: if refresh <= 1 { 60 } else { refresh },
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            })
        })
        .collect();
    out.sort_by_key(|m| (!m.primary, m.x, m.y));
    for (i, m) in out.iter_mut().enumerate() {
        m.index = i;
    }
    out
}

fn logical_cpus() -> u32 {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut info) };
    info.dwNumberOfProcessors
}

fn ram_mb() -> u64 {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
        m.ullTotalPhys / (1024 * 1024)
    } else {
        0
    }
}

pub fn power_status() -> (PowerSource, Option<u8>) {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return (PowerSource::Unknown, None);
    }
    let no_battery = s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255;
    let percent = (!no_battery && s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent);
    let source = match s.ACLineStatus {
        0 if !no_battery => PowerSource::Battery,
        1 => PowerSource::Ac,
        _ if no_battery => PowerSource::Ac,
        _ => PowerSource::Unknown,
    };
    (source, percent)
}

fn os_build() -> u32 {
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let value = wide("CurrentBuildNumber");
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            windows::core::PCWSTR(key.as_ptr()),
            windows::core::PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    if ok.is_ok() {
        from_wide(&buf).trim().parse().unwrap_or(0)
    } else {
        0
    }
}
