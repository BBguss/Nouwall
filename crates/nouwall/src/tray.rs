//! Tray icon and its menu.

use nouwall_core::Entry;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayAction {
    OpenSettings,
    TogglePause,
    Apply(String),
    ToggleAutostart,
    OpenLibrary,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
}

/// 32x32 icon drawn in code: a rounded gradient tile with a light wave.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let radius = s * 0.22;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-rectangle coverage.
            let cx = fx.clamp(radius, s - radius);
            let cy = fy.clamp(radius, s - radius);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let alpha = (radius - d + 0.5).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let t = (fx + fy) / (2.0 * s);
            let (mut r, mut g, mut b) = (
                120.0 + (40.0 - 120.0) * t,
                80.0 + (200.0 - 80.0) * t,
                240.0 + (190.0 - 240.0) * t,
            );
            let wave = s * (0.55 + 0.12 * (fx / s * std::f32::consts::TAU).sin());
            let line = 1.0 - ((fy - wave).abs() - s * 0.05).clamp(0.0, 1.0);
            r += (255.0 - r) * line;
            g += (255.0 - g) * line;
            b += (255.0 - b) * line;
            let i = ((y * size + x) * 4) as usize;
            px[i] = r as u8;
            px[i + 1] = g as u8;
            px[i + 2] = b as u8;
            px[i + 3] = (alpha * 255.0) as u8;
        }
    }
    px
}

fn build_menu(entries: &[Entry], current: Option<&str>, paused: bool, autostart: bool) -> Menu {
    let menu = Menu::new();
    let wallpapers = Submenu::new("Ganti wallpaper", !entries.is_empty());
    for e in entries {
        let item = CheckMenuItem::with_id(
            format!("wp:{}", e.id),
            format!("{}  ({})", e.manifest.title, e.manifest.kind.label()),
            true,
            current == Some(e.id.as_str()),
            None,
        );
        let _ = wallpapers.append(&item);
    }
    let _ = menu.append_items(&[
        &MenuItem::with_id("open", "Buka nouwall", true, None),
        &PredefinedMenuItem::separator(),
        &wallpapers,
        &CheckMenuItem::with_id("pause", "Jeda wallpaper", true, paused, None),
        &MenuItem::with_id("library", "Buka folder library", true, None),
        &PredefinedMenuItem::separator(),
        &CheckMenuItem::with_id("autostart", "Mulai bersama Windows", true, autostart, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Keluar", true, None),
    ]);
    menu
}

impl Tray {
    pub fn new(entries: &[Entry], current: Option<&str>, paused: bool, autostart: bool) -> Result<Self, String> {
        let icon = Icon::from_rgba(icon_rgba(32), 32, 32).map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("nouwall")
            .with_icon(icon)
            .with_menu(Box::new(build_menu(entries, current, paused, autostart)))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { icon })
    }

    pub fn update(&self, entries: &[Entry], current: Option<&str>, paused: bool, autostart: bool) {
        self.icon.set_menu(Some(Box::new(build_menu(entries, current, paused, autostart))));
        let tip = if paused { "nouwall (dijeda)" } else { "nouwall" };
        let _ = self.icon.set_tooltip(Some(tip));
    }

    /// Drain pending tray and menu events.
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut out = Vec::new();
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            match ev {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => out.push(TrayAction::OpenSettings),
                _ => {}
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = ev.id().0.as_str();
            out.push(match id {
                "open" => TrayAction::OpenSettings,
                "pause" => TrayAction::TogglePause,
                "autostart" => TrayAction::ToggleAutostart,
                "library" => TrayAction::OpenLibrary,
                "quit" => TrayAction::Quit,
                other => match other.strip_prefix("wp:") {
                    Some(wp) => TrayAction::Apply(wp.to_string()),
                    None => continue,
                },
            });
        }
        // A click opens settings once even if Up and DoubleClick both arrive.
        let mut opened = false;
        out.retain(|a| !matches!(a, TrayAction::OpenSettings) || !std::mem::replace(&mut opened, true));
        out
    }
}
