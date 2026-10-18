//! Attaching windows behind the desktop icons.
//!
//! Sending the undocumented message 0x052C to Progman makes Explorer create a
//! WorkerW window between the wallpaper and the icon layer. Before Windows 11
//! 24H2 that WorkerW is a top-level window right after the one holding
//! `SHELLDLL_DefView`; on 24H2 and later it is a child of Progman, and our
//! window must become a layered child of Progman placed just above it.

use std::sync::OnceLock;

use nouwall_core::pause::Coverage;
use nouwall_core::MonitorInfo;
use windows::core::{w, PCWSTR};
use windows::core::BOOL;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MapWindowPoints, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::util::wide;

pub const HOST_CLASS: PCWSTR = w!("nouwall-host");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Pre-24H2: our windows are children of a top-level WorkerW.
    Classic,
    /// 24H2+: children of Progman, above its WorkerW child.
    Raised,
}

#[derive(Debug, Clone, Copy)]
pub struct Desktop {
    pub parent: HWND,
    pub layout: Layout,
    /// Raised layout: our windows go just above this sibling in z-order.
    pub insert_after: HWND,
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn register_host_class() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: hinstance,
            lpszClassName: HOST_CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

/// Find (or ask Explorer to create) the window wallpapers attach to.
pub fn find() -> Option<Desktop> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), None).ok()?;
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut result));

        // 24H2+: WorkerW lives inside Progman.
        if let Ok(worker) = FindWindowExW(Some(progman), None, w!("WorkerW"), None) {
            return Some(Desktop { parent: progman, layout: Layout::Raised, insert_after: worker });
        }

        unsafe extern "system" fn find_worker(top: HWND, out: LPARAM) -> BOOL {
            if FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), None).is_ok() {
                if let Ok(worker) = FindWindowExW(None, Some(top), w!("WorkerW"), None) {
                    *(out.0 as *mut HWND) = worker;
                }
            }
            TRUE
        }
        let mut worker = HWND::default();
        let _ = EnumWindows(Some(find_worker), LPARAM(&mut worker as *mut _ as isize));
        if !worker.is_invalid() {
            return Some(Desktop { parent: worker, layout: Layout::Classic, insert_after: HWND::default() });
        }
        // Some setups (icons hidden, third-party shells) only have Progman.
        Some(Desktop { parent: progman, layout: Layout::Classic, insert_after: HWND::default() })
    }
}

/// Monitor rectangle converted into the parent window's client coordinates.
pub fn monitor_rect_in(parent: HWND, m: &MonitorInfo) -> RECT {
    let mut pts = [POINT { x: m.x, y: m.y }, POINT { x: m.x + m.width as i32, y: m.y + m.height as i32 }];
    unsafe {
        MapWindowPoints(None, Some(parent), &mut pts);
    }
    RECT { left: pts[0].x, top: pts[0].y, right: pts[1].x, bottom: pts[1].y }
}

/// Create a borderless child window covering `monitor` behind the icons.
pub fn create_host(desktop: &Desktop, monitor: &MonitorInfo) -> windows::core::Result<HWND> {
    register_host_class();
    let r = monitor_rect_in(desktop.parent, monitor);
    let title = wide(&format!("nouwall {}", monitor.index));
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();
        let ex = match desktop.layout {
            Layout::Classic => WS_EX_NOACTIVATE,
            Layout::Raised => WS_EX_NOACTIVATE | WS_EX_LAYERED,
        };
        let hwnd = CreateWindowExW(
            ex,
            HOST_CLASS,
            PCWSTR(title.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(desktop.parent),
            None,
            Some(hinstance),
            None,
        )?;
        if desktop.layout == Layout::Raised {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            let _ = SetWindowPos(
                hwnd,
                Some(desktop.insert_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Ok(hwnd)
    }
}

/// Re-apply the user's static wallpaper so the desktop repaints after we exit.
pub fn refresh_static_wallpaper() {
    unsafe {
        let mut buf = [0u16; 520];
        if SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr() as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() {
            let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(buf.as_mut_ptr() as *mut _), SPIF_SENDCHANGE);
        }
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// How the foreground window covers `monitor`.
pub fn coverage(monitor: &MonitorInfo) -> Coverage {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                // Exclusive fullscreen: only the monitor holding the foreground window.
                let fg = GetForegroundWindow();
                if on_monitor(fg, monitor) {
                    return Coverage::Fullscreen;
                }
            }
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || !IsWindowVisible(fg).as_bool() {
            return Coverage::None;
        }
        let class = class_name(fg);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
            || class.starts_with("nouwall")
        {
            return Coverage::None;
        }
        if !on_monitor(fg, monitor) {
            return Coverage::None;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return Coverage::None;
        }
        let covers = r.left <= monitor.x
            && r.top <= monitor.y
            && r.right >= monitor.x + monitor.width as i32
            && r.bottom >= monitor.y + monitor.height as i32;
        if covers {
            Coverage::Fullscreen
        } else if IsZoomed(fg).as_bool() {
            Coverage::Maximized
        } else {
            Coverage::None
        }
    }
}

fn on_monitor(hwnd: HWND, monitor: &MonitorInfo) -> bool {
    unsafe {
        let hm = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if hm.is_invalid() {
            return false;
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(hm, &mut info).as_bool() {
            return false;
        }
        info.rcMonitor.left == monitor.x && info.rcMonitor.top == monitor.y
    }
}
