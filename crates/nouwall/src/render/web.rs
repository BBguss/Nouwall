//! Web wallpapers in WebView2 (through wry).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use nouwall_core::{tr, Kind};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

use super::{RenderOptions, Renderer};
use crate::util::Hwnd;

const MUTE_SCRIPT: &str = r#"(() => {
  const mute = () => document.querySelectorAll('video,audio').forEach(e => { e.muted = true; });
  document.addEventListener('DOMContentLoaded', mute);
  new MutationObserver(mute).observe(document, { subtree: true, childList: true });
})();"#;

const PAUSE_SCRIPT: &str = "document.querySelectorAll('video,audio').forEach(e => { e.dataset.nouwallPlaying = String(!e.paused); e.pause(); }); window.dispatchEvent(new CustomEvent('nouwall:pause'));";
const RESUME_SCRIPT: &str = "document.querySelectorAll('video,audio').forEach(e => { if (e.dataset.nouwallPlaying === 'true') e.play(); }); window.dispatchEvent(new CustomEvent('nouwall:resume'));";

/// Builder options shared by every WebView2 in the process. WebView2 refuses a
/// second environment with different options on the same data folder, so the
/// settings window uses exactly the same ones.
pub fn builder(ctx: &mut WebContext) -> WebViewBuilder<'_> {
    WebViewBuilder::new_with_web_context(ctx).with_autoplay(true).with_devtools(cfg!(debug_assertions))
}

pub fn bounds(width: u32, height: u32) -> Rect {
    Rect { position: PhysicalPosition::new(0, 0).into(), size: PhysicalSize::new(width, height).into() }
}

pub struct WebRenderer {
    webview: WebView,
    paused: bool,
    url: String,
}

impl WebRenderer {
    pub fn new(
        ctx: &Rc<RefCell<WebContext>>,
        hwnd: Hwnd,
        size: (u32, u32),
        url: &str,
        zoom: f32,
        opts: RenderOptions,
    ) -> Result<Self, String> {
        let mut ctx = ctx.borrow_mut();
        let mut b = builder(&mut ctx).with_url(url).with_bounds(bounds(size.0, size.1)).with_focused(false);
        if opts.mute {
            b = b.with_initialization_script(MUTE_SCRIPT);
        }
        let webview = b.build_as_child(&hwnd).map_err(|e| e.to_string())?;
        if (zoom - 1.0).abs() > 0.01 {
            let _ = webview.zoom(f64::from(zoom));
        }
        Ok(Self { webview, paused: false, url: url.to_string() })
    }
}

impl Renderer for WebRenderer {
    fn kind(&self) -> Option<Kind> {
        Some(Kind::Web)
    }

    fn resize(&mut self, width: u32, height: u32) {
        let _ = self.webview.set_bounds(bounds(width, height));
    }

    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        let _ = self.webview.evaluate_script(if paused { PAUSE_SCRIPT } else { RESUME_SCRIPT });
        // Hidden WebView2 throttles timers and requestAnimationFrame.
        let _ = self.webview.set_visible(!paused);
    }

    fn frame(&mut self, _now: Instant) -> Option<Instant> {
        None
    }

    fn status(&self) -> String {
        if self.paused {
            tr!("paused", "dijeda")
        } else {
            let page = self.url.rsplit(['/', '\\']).next().unwrap_or("");
            tr!("page {page}", "halaman {page}")
        }
    }
}
