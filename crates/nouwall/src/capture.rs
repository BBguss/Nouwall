//! `nouwallctl capture <dir>`: save what each wallpaper window shows as BMP,
//! for checking a running wallpaper without uncovering the desktop.

use std::fs;
use std::path::Path;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, EnumWindows, FindWindowW, GetClassNameW, GetClientRect};

const PW_RENDERFULLCONTENT: u32 = 2;

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe extern "system" fn collect_child(hwnd: HWND, out: LPARAM) -> BOOL {
    if class_name(hwnd) == "nouwall-host" {
        (*(out.0 as *mut Vec<HWND>)).push(hwnd);
    }
    TRUE
}

unsafe extern "system" fn collect_top(hwnd: HWND, out: LPARAM) -> BOOL {
    let _ = EnumChildWindows(Some(hwnd), Some(collect_child), out);
    TRUE
}

pub fn host_windows() -> Vec<HWND> {
    let mut out: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_top), LPARAM(&mut out as *mut _ as isize));
    }
    out.dedup();
    out
}

fn capture_one(hwnd: HWND, path: &Path) -> Result<(u32, u32), String> {
    unsafe {
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).map_err(|e| e.to_string())?;
        let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok {
            return Err("PrintWindow gagal".into());
        }
        // BMP file: 14-byte file header + 40-byte info header + bottom-up BGRA rows.
        let mut file = Vec::with_capacity(54 + pixels.len());
        let size = 54 + pixels.len() as u32;
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&size.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&54u32.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&w.to_le_bytes());
        file.extend_from_slice(&h.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&[0u8; 24]);
        file.extend_from_slice(&pixels);
        fs::write(path, file).map_err(|e| e.to_string())?;
        Ok((w as u32, h as u32))
    }
}

/// Capture every wallpaper window into `dir\monitor-N.bmp`, plus the
/// settings window (`settings.bmp`) when it is open.
pub fn capture_all(dir: &Path) -> Vec<Result<String, String>> {
    let _ = fs::create_dir_all(dir);
    let mut targets: Vec<(HWND, String)> =
        host_windows().into_iter().enumerate().map(|(i, h)| (h, format!("monitor-{i}.bmp"))).collect();
    if let Ok(h) = unsafe { FindWindowW(windows::core::w!("nouwall-settings"), None) } {
        targets.push((h, "settings.bmp".into()));
    }
    targets
        .into_iter()
        .map(|(hwnd, name)| {
            let path = dir.join(name);
            capture_one(hwnd, &path).map(|(w, h)| format!("{} ({w}x{h})", path.display()))
        })
        .collect()
}
