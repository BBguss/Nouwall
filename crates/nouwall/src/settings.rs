//! Settings window: a top-level Win32 window hosting the embedded HTML UI in
//! WebView2. Messages from the page are queued and handled by the app loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{WebContext, WebView};

use crate::render::web;
use crate::tray::icon_rgba;
use crate::util::{rect_size, Hwnd};

const CLASS: PCWSTR = w!("nouwall-settings");
const UI_HTML: &str = include_str!("../assets/ui/settings.html");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    Resized,
    Closed,
}

thread_local! {
    static EVENTS: RefCell<Vec<WindowEvent>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            EVENTS.with(|e| e.borrow_mut().push(WindowEvent::Resized));
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(l.0 as *mut MINMAXINFO);
            info.ptMinTrackSize.x = 760;
            info.ptMinTrackSize.y = 520;
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            EVENTS.with(|e| e.borrow_mut().push(WindowEvent::Closed));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn register_class() -> HINSTANCE {
    static ONCE: OnceLock<isize> = OnceLock::new();
    let h = *ONCE.get_or_init(|| unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: hinstance,
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x0016_0f0d)).0),
            hIcon: app_icon(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        hinstance.0 as isize
    });
    HINSTANCE(h as *mut _)
}

/// Window icon from the same artwork as the tray icon.
pub fn app_icon() -> HICON {
    let size = 32;
    let rgba = icon_rgba(size);
    let mut bgra = rgba.clone();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let and_mask = vec![0u8; (size * size / 8) as usize];
    unsafe {
        CreateIcon(None, size as i32, size as i32, 1, 32, and_mask.as_ptr(), bgra.as_ptr()).unwrap_or_default()
    }
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    pub webview: WebView,
}

impl SettingsWindow {
    pub fn open(ctx: &Rc<RefCell<WebContext>>, queue: Rc<RefCell<Vec<String>>>) -> Result<Self, String> {
        let hinstance = register_class();
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS,
                w!("nouwall"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1100,
                720,
                None,
                None,
                Some(hinstance),
                None,
            )
            .map_err(|e| e.to_string())?
        };
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            // Scale the default size to the monitor DPI, then centre on screen.
            let dpi = GetDpiForWindow(hwnd).max(96);
            let (w, h) = (1100 * dpi / 96, 720 * dpi / 96);
            let sw = GetSystemMetrics(SM_CXSCREEN) as u32;
            let sh = GetSystemMetrics(SM_CYSCREEN) as u32;
            let _ = SetWindowPos(
                hwnd,
                None,
                (sw.saturating_sub(w) / 2) as i32,
                (sh.saturating_sub(h) / 2) as i32,
                w as i32,
                h as i32,
                SWP_NOZORDER,
            );
        }
        let (cw, ch) = client_size(hwnd);
        let webview = {
            let mut ctx = ctx.borrow_mut();
            web::builder(&mut ctx)
                .with_html(UI_HTML)
                .with_bounds(web::bounds(cw, ch))
                .with_ipc_handler(move |req| queue.borrow_mut().push(req.body().clone()))
                .build_as_child(&Hwnd(hwnd))
                .map_err(|e| e.to_string())?
        };
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(Self { hwnd, webview })
    }

    pub fn focus(&self) {
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub fn fit(&self) {
        let (w, h) = client_size(self.hwnd);
        let _ = self.webview.set_bounds(web::bounds(w, h));
    }

    pub fn send(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            log::warn!("settings script: {e}");
        }
    }

    pub fn close(&self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    rect_size(&r)
}

pub fn take_events() -> Vec<WindowEvent> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}
