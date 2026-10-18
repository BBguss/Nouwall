//! How a wallpaper is placed on a monitor: fit mode, size, position and the
//! resolution it is rendered at. Stored per wallpaper in `wallpaper.toml`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Fill the monitor, cropping what does not fit.
    #[default]
    Cover,
    /// Show the whole source, with bars where the aspect ratio differs.
    Contain,
    /// Fill the monitor exactly, distorting the aspect ratio.
    Stretch,
    /// Native pixel size, centred.
    Center,
}

pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;
/// Lowest render height accepted for `resolution`.
pub const MIN_RESOLUTION: u32 = 360;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub fit: Fit,
    /// Size multiplier on top of the fit mode, `MIN_ZOOM..=MAX_ZOOM`.
    pub zoom: f32,
    /// Alignment when the image is larger or smaller than the monitor:
    /// -1 = left/top edge, 0 = centre, 1 = right/bottom edge.
    pub pan_x: f32,
    pub pan_y: f32,
    /// Render height cap in pixels. `None` follows the tier (shaders) or the
    /// monitor (video); a smaller value saves GPU time at the cost of sharpness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<u32>,
}

impl Default for Display {
    fn default() -> Self {
        Self { fit: Fit::Cover, zoom: 1.0, pan_x: 0.0, pan_y: 0.0, resolution: None }
    }
}

/// Rectangle in normalized coordinates (0..1 on both axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl NormRect {
    pub const FULL: NormRect = NormRect { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };

    pub fn is_full(&self) -> bool {
        const E: f32 = 1e-4;
        self.left <= E && self.top <= E && self.right >= 1.0 - E && self.bottom >= 1.0 - E
    }
}

/// Which part of the source is shown (`src`) and where it lands on the monitor (`dst`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub src: NormRect,
    pub dst: NormRect,
}

impl Display {
    pub fn is_default(&self) -> bool {
        *self == Display::default()
    }

    /// Clamp every field into its valid range (values come from user files and the UI).
    pub fn sanitized(mut self) -> Self {
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.zoom = finite(self.zoom, 1.0).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan_x = finite(self.pan_x, 0.0).clamp(-1.0, 1.0);
        self.pan_y = finite(self.pan_y, 0.0).clamp(-1.0, 1.0);
        self.resolution = self.resolution.map(|r| r.max(MIN_RESOLUTION));
        self
    }

    /// Render target size for a monitor: the monitor size, or scaled down so its
    /// height does not exceed `resolution` (aspect ratio kept).
    pub fn render_size(&self, monitor: (u32, u32)) -> (u32, u32) {
        let (w, h) = (monitor.0.max(1), monitor.1.max(1));
        match self.resolution {
            Some(cap) if cap < h => {
                let cap = cap.max(MIN_RESOLUTION.min(h));
                (((w as u64 * cap as u64 + h as u64 / 2) / h as u64).max(1) as u32, cap)
            }
            _ => (w, h),
        }
    }

    /// Place a `source`-sized image on a `target`-sized monitor. `None` when
    /// nothing of the source ends up visible.
    pub fn place(&self, source: (u32, u32), target: (u32, u32)) -> Option<Placement> {
        let d = self.sanitized();
        let (sw, sh) = (source.0.max(1) as f32, source.1.max(1) as f32);
        let (tw, th) = (target.0.max(1) as f32, target.1.max(1) as f32);
        let (sx, sy) = match d.fit {
            Fit::Cover => {
                let s = (tw / sw).max(th / sh);
                (s, s)
            }
            Fit::Contain => {
                let s = (tw / sw).min(th / sh);
                (s, s)
            }
            Fit::Stretch => (tw / sw, th / sh),
            Fit::Center => (1.0, 1.0),
        };
        let (w, h) = (sw * sx * d.zoom, sh * sy * d.zoom);
        // Image origin: centred, shifted by pan across the free (or overflowing) space.
        let ox = (tw - w) / 2.0 * (1.0 + d.pan_x);
        let oy = (th - h) / 2.0 * (1.0 + d.pan_y);
        let (vl, vt) = (ox.max(0.0), oy.max(0.0));
        let (vr, vb) = ((ox + w).min(tw), (oy + h).min(th));
        if vr - vl < 0.5 || vb - vt < 0.5 {
            return None;
        }
        Some(Placement {
            src: NormRect { left: (vl - ox) / w, top: (vt - oy) / h, right: (vr - ox) / w, bottom: (vb - oy) / h },
            dst: NormRect { left: vl / tw, top: vt / th, right: vr / tw, bottom: vb / th },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn cover_crops_the_wider_source_evenly() {
        let p = Display::default().place((3840, 1080), (1920, 1080)).unwrap();
        assert!(close(p.src.left, 0.25) && close(p.src.right, 0.75));
        assert!(close(p.src.top, 0.0) && close(p.src.bottom, 1.0));
        assert!(p.dst.is_full());
    }

    #[test]
    fn contain_letterboxes() {
        let d = Display { fit: Fit::Contain, ..Display::default() };
        let p = d.place((1920, 1080), (1920, 1200)).unwrap();
        assert!(p.src.is_full());
        assert!(close(p.dst.top, 0.05) && close(p.dst.bottom, 0.95));
    }

    #[test]
    fn stretch_fills_without_crop() {
        let d = Display { fit: Fit::Stretch, ..Display::default() };
        let p = d.place((1000, 1000), (1920, 1080)).unwrap();
        assert!(p.src.is_full() && p.dst.is_full());
    }

    #[test]
    fn center_keeps_native_size() {
        let d = Display { fit: Fit::Center, ..Display::default() };
        let p = d.place((960, 540), (1920, 1080)).unwrap();
        assert!(p.src.is_full());
        assert!(close(p.dst.left, 0.25) && close(p.dst.right, 0.75));
    }

    #[test]
    fn zoom_crops_and_pan_moves_the_window() {
        let d = Display { zoom: 2.0, pan_x: -1.0, ..Display::default() };
        let p = d.place((1920, 1080), (1920, 1080)).unwrap();
        // Left half of the source, top/bottom quarter cropped.
        assert!(close(p.src.left, 0.0) && close(p.src.right, 0.5));
        assert!(close(p.src.top, 0.25) && close(p.src.bottom, 0.75));
    }

    #[test]
    fn zoom_out_shrinks_into_the_middle() {
        let d = Display { zoom: 0.5, ..Display::default() };
        let p = d.place((1920, 1080), (1920, 1080)).unwrap();
        assert!(p.src.is_full());
        assert!(close(p.dst.left, 0.25) && close(p.dst.bottom, 0.75));
    }

    #[test]
    fn render_size_caps_height_and_keeps_aspect() {
        let d = Display { resolution: Some(1080), ..Display::default() };
        assert_eq!(d.render_size((3840, 2160)), (1920, 1080));
        assert_eq!(d.render_size((1920, 1080)), (1920, 1080));
        assert_eq!(Display::default().render_size((2560, 1440)), (2560, 1440));
        let ultrawide = Display { resolution: Some(720), ..Display::default() };
        assert_eq!(ultrawide.render_size((3440, 1440)), (1720, 720));
    }

    #[test]
    fn sanitize_clamps_garbage() {
        let d = Display { zoom: f32::NAN, pan_x: 9.0, resolution: Some(10), ..Display::default() }.sanitized();
        assert_eq!((d.zoom, d.pan_x, d.resolution), (1.0, 1.0, Some(MIN_RESOLUTION)));
    }

    #[test]
    fn default_display_is_omitted_from_toml() {
        assert!(Display::default().is_default());
        let d: Display = toml::from_str("fit = \"contain\"\nzoom = 1.25").unwrap();
        assert_eq!((d.fit, d.zoom, d.pan_x), (Fit::Contain, 1.25, 0.0));
    }
}
