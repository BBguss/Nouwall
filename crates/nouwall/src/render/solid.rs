//! Fallback renderer: paints a solid color with GDI when a wallpaper fails to load.

use std::time::Instant;

use nouwall_core::Kind;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, GetDC, ReleaseDC};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::Renderer;

pub struct Solid {
    hwnd: HWND,
    color: COLORREF,
    reason: String,
}

impl Solid {
    pub fn new(hwnd: HWND, reason: impl Into<String>) -> Self {
        let s = Self { hwnd, color: COLORREF(0x0024_1a14), reason: reason.into() };
        s.paint();
        s
    }

    fn paint(&self) {
        unsafe {
            let dc = GetDC(Some(self.hwnd));
            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            let brush = CreateSolidBrush(self.color);
            FillRect(dc, &r, brush);
            let _ = DeleteObject(brush.into());
            ReleaseDC(Some(self.hwnd), dc);
        }
    }
}

impl Renderer for Solid {
    fn kind(&self) -> Option<Kind> {
        None
    }

    fn resize(&mut self, _w: u32, _h: u32) {
        self.paint();
    }

    fn set_paused(&mut self, _paused: bool) {}

    fn frame(&mut self, _now: Instant) -> Option<Instant> {
        None
    }

    fn status(&self) -> String {
        self.reason.clone()
    }
}
