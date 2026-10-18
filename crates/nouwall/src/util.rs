//! Small Win32 helpers shared across modules.

use std::num::NonZeroIsize;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};

/// NUL-terminated UTF-16 for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}

pub fn hstring(s: &str) -> HSTRING {
    HSTRING::from(s)
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn rect_size(r: &RECT) -> (u32, u32) {
    ((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32)
}

/// HWND wrapper so wgpu and wry can target our own Win32 windows.
#[derive(Clone, Copy, Debug)]
pub struct Hwnd(pub HWND);

// SAFETY: an HWND is a plain handle value; every use stays on the UI thread.
unsafe impl Send for Hwnd {}
unsafe impl Sync for Hwnd {}

impl HasWindowHandle for Hwnd {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = NonZeroIsize::new(self.0 .0 as isize).ok_or(HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(raw);
        // SAFETY: the HWND outlives every renderer that borrows it (owned by Host).
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for Hwnd {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}
