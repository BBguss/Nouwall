//! Tray icon and its menu.

use nouwall_core::{tr, Entry};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
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

fn build_menu(entries: &[Entry], current: Option<&str>, paused: bool, autostart: bool) -> Menu {
    let menu = Menu::new();
    let wallpapers = Submenu::new(tr!("Change wallpaper", "Ganti wallpaper"), !entries.is_empty());
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
        &MenuItem::with_id("open", tr!("Open nouwall", "Buka nouwall"), true, None),
        &PredefinedMenuItem::separator(),
        &wallpapers,
        &CheckMenuItem::with_id("pause", tr!("Pause wallpaper", "Jeda wallpaper"), true, paused, None),
        &MenuItem::with_id("library", tr!("Open library folder", "Buka folder library"), true, None),
        &PredefinedMenuItem::separator(),
        &CheckMenuItem::with_id("autostart", tr!("Start with Windows", "Mulai bersama Windows"), true, autostart, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", tr!("Quit", "Keluar"), true, None),
    ]);
    menu
}

impl Tray {
    pub fn new(entries: &[Entry], current: Option<&str>, paused: bool, autostart: bool) -> Result<Self, String> {
        // Resource 1 is the application icon embedded by build.rs.
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
        let icon = Icon::from_resource(1, Some((size, size))).map_err(|e| e.to_string())?;
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
        let tip = if paused { tr!("nouwall (paused)", "nouwall (dijeda)") } else { "nouwall".to_string() };
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
