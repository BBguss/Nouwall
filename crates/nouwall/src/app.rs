//! The engine: one hidden controller window, one host window per monitor and a
//! message loop that wakes for window messages or the next frame deadline.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use nouwall_core::pause::{self, PauseInputs};
use nouwall_core::tier::effective_shader_fps;
use nouwall_core::{
    Config, Display, Entry, GeneralSettings, HardwareInfo, Kind, Library, MonitorInfo, MonitorTarget, PauseReason,
    PowerSource, Profiles, Severity, TierReport,
};
use nouwall_core::{i18n, tr};
use serde::Deserialize;
use serde_json::{json, Value};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::WebContext;

use crate::render::shader::{Gpu, Language, ShaderRenderer};
use crate::render::solid::Solid;
use crate::render::video::VideoRenderer;
use crate::render::web::WebRenderer;
use crate::render::{RenderOptions, Renderer};
use crate::settings::{self, SettingsWindow};
use crate::tray::{Tray, TrayAction};
use crate::util::{hstring, Hwnd};
use crate::{autostart, builtin, commands, desktop, instance, paths, probe};

const TICK: Duration = Duration::from_millis(500);
const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;

#[derive(Debug)]
enum ControllerEvent {
    Command(String),
    DisplayChanged,
    ExplorerRestarted,
    Locked(bool),
    DisplayOn(bool),
}

thread_local! {
    static EVENTS: RefCell<Vec<ControllerEvent>> = const { RefCell::new(Vec::new()) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn push(ev: ControllerEvent) {
    EVENTS.with(|e| e.borrow_mut().push(ev));
}

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.with(|c| c.get()) {
        push(ControllerEvent::ExplorerRestarted);
        return LRESULT(0);
    }
    match msg {
        WM_COPYDATA => {
            let data = &*(l.0 as *const COPYDATASTRUCT);
            if data.dwData == instance::COPYDATA_TAG && !data.lpData.is_null() {
                let bytes = std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize);
                push(ControllerEvent::Command(String::from_utf8_lossy(bytes).into_owned()));
            }
            LRESULT(1)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            push(ControllerEvent::DisplayChanged);
            LRESULT(0)
        }
        WM_SETTINGCHANGE if w.0 == SPI_SETWORKAREA.0 as usize => {
            push(ControllerEvent::DisplayChanged);
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match w.0 {
                WTS_SESSION_LOCK => push(ControllerEvent::Locked(true)),
                WTS_SESSION_UNLOCK => push(ControllerEvent::Locked(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if w.0 == PBT_POWERSETTINGCHANGE as usize {
                let s = &*(l.0 as *const POWERBROADCAST_SETTING);
                if s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                    push(ControllerEvent::DisplayOn(s.Data[0] != 0));
                }
            }
            LRESULT(1)
        }
        WM_ENDSESSION => {
            push(ControllerEvent::Command("quit".into()));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn create_controller() -> windows::core::Result<HWND> {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(controller_proc),
            hInstance: hinstance,
            lpszClassName: instance::CONTROLLER_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        TASKBAR_CREATED.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
        // A hidden top-level window (not message-only) so broadcasts reach it.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            instance::CONTROLLER_CLASS,
            w!("nouwall controller"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )?;
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        let _ = RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        Ok(hwnd)
    }
}

struct Host {
    hwnd: HWND,
    monitor: MonitorInfo,
    wallpaper: Option<String>,
    renderer: Option<Box<dyn Renderer>>,
    paused: Option<PauseReason>,
    next: Option<Instant>,
}

impl Host {
    fn size(&self) -> (u32, u32) {
        (self.monitor.width, self.monitor.height)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // Renderer first: it holds the swap chain / surface of this window.
        self.renderer = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[derive(Deserialize)]
struct IpcRequest {
    id: u64,
    cmd: String,
    #[serde(default)]
    args: Value,
}

pub struct App {
    controller: HWND,
    timer: HANDLE,
    desktop: Option<desktop::Desktop>,
    hosts: Vec<Host>,
    hw: HardwareInfo,
    profiles: Profiles,
    report: TierReport,
    config: Config,
    config_mtime: Option<SystemTime>,
    library: Library,
    entries: Vec<Entry>,
    gpu: Option<Rc<Gpu>>,
    web_ctx: Rc<RefCell<WebContext>>,
    settings: Option<SettingsWindow>,
    ipc: Rc<RefCell<Vec<String>>>,
    tray: Option<Tray>,
    user_paused: bool,
    locked: bool,
    display_off: bool,
    power: PowerSource,
    quit: bool,
    next_tick: Instant,
    ticks: u64,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for ch in p.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3F"),
            c => out.push(c),
        }
    }
    out
}

impl App {
    pub fn new() -> Result<Self, String> {
        let controller = create_controller().map_err(|e| format!("controller window: {e}"))?;
        let timer = unsafe {
            CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)
                .or_else(|_| CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0))
                .map_err(|e| format!("timer: {e}"))?
        };
        let hw = probe::probe();
        let profiles = commands::load_profiles();
        let mut config = commands::load_config();
        commands::apply_language(&config);
        let library = commands::library();
        if builtin::install(&library, &mut config) {
            let _ = commands::save_config(&config);
        }
        let report = commands::tier_report(&hw, &profiles, &config);
        log::info!(
            "GPU {} ({:?}, {} MB), tier {} [{}], {} monitor",
            hw.gpu.name,
            hw.gpu.kind,
            hw.gpu.dedicated_vram_mb,
            report.tier.label(),
            report.reasons.join("; "),
            hw.monitors.len()
        );
        let _ = std::fs::create_dir_all(paths::webview_dir());
        let web_ctx = Rc::new(RefCell::new(WebContext::new(Some(paths::webview_dir()))));
        let power = hw.power;
        let mut app = Self {
            controller,
            timer,
            desktop: None,
            hosts: Vec::new(),
            hw,
            profiles,
            report,
            config,
            config_mtime: mtime(&paths::config_file()),
            library,
            entries: Vec::new(),
            gpu: None,
            web_ctx,
            settings: None,
            ipc: Rc::new(RefCell::new(Vec::new())),
            tray: None,
            user_paused: false,
            locked: false,
            display_off: false,
            power,
            quit: false,
            next_tick: Instant::now(),
            ticks: 0,
        };
        app.rescan_library();
        app.tray = match Tray::new(&app.entries, app.current_all(), false, autostart::is_enabled()) {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!("tray: {e}");
                None
            }
        };
        app.attach();
        Ok(app)
    }

    fn current_all(&self) -> Option<&str> {
        self.config.wallpaper_for(0)
    }

    fn rescan_library(&mut self) {
        let (mut entries, errors) = self.library.scan();
        for e in errors {
            log::warn!("library: {e}");
        }
        for e in entries.iter_mut() {
            commands::refresh_video_meta(&self.library, e);
        }
        self.entries = entries;
    }

    fn refresh_tray(&self) {
        if let Some(t) = &self.tray {
            t.update(&self.entries, self.current_all(), self.user_paused, autostart::is_enabled());
        }
    }

    /// Find the desktop and (re)create every host.
    fn attach(&mut self) {
        self.hosts.clear();
        self.desktop = desktop::find();
        let Some(desk) = self.desktop else {
            log::warn!("desktop (Progman/WorkerW) not found yet, retrying");
            return;
        };
        log::info!("desktop layout {:?}", desk.layout);
        for m in self.hw.monitors.clone() {
            match desktop::create_host(&desk, &m) {
                Ok(hwnd) => self.hosts.push(Host {
                    hwnd,
                    monitor: m,
                    wallpaper: None,
                    renderer: None,
                    paused: None,
                    next: None,
                }),
                Err(e) => log::error!("host monitor {}: {e}", m.index),
            }
        }
        self.load_wallpapers(true);
    }

    fn options(&self, m: &MonitorInfo) -> RenderOptions {
        let g = &self.config.general;
        RenderOptions {
            fps: effective_shader_fps(&self.report, g.fps_cap.min(m.refresh_hz.max(1)), self.power, g.battery_saver),
            render_scale: self.report.budget.render_scale,
            auto_downgrade: g.auto_downgrade,
            mute: g.mute,
            volume: g.volume,
            monitor_x: m.x,
            monitor_y: m.y,
        }
    }

    fn make_renderer(&mut self, idx: usize, id: &str) -> Box<dyn Renderer> {
        let hwnd = self.hosts[idx].hwnd;
        let size = self.hosts[idx].size();
        let opts = self.options(&self.hosts[idx].monitor);
        let entry = match self.library.get(id) {
            Ok(e) => e,
            Err(e) => return Box::new(Solid::new(hwnd, tr!("error: {e}", "galat: {e}"))),
        };
        let path = entry.entry_path();
        let display = entry.manifest.display.sanitized();
        let mut opts = opts;
        if let Some(res) = display.resolution {
            // An explicit resolution replaces the tier's default render scale;
            // the governor may still lower it when frames run late.
            opts.render_scale = (res as f32 / size.1.max(1) as f32).clamp(0.25, 1.0);
        }
        let result: Result<Box<dyn Renderer>, String> = match entry.manifest.kind {
            Kind::Shader => {
                if self.gpu.is_none() {
                    match Gpu::new(Hwnd(hwnd)) {
                        Ok(g) => self.gpu = Some(g),
                        Err(e) => log::error!("wgpu: {e}"),
                    }
                }
                match (&self.gpu, std::fs::read_to_string(&path)) {
                    (Some(gpu), Ok(src)) => {
                        ShaderRenderer::new(gpu.clone(), Hwnd(hwnd), size, Language::from_path(&path), &src, opts)
                            .map(|r| Box::new(r) as Box<dyn Renderer>)
                    }
                    (None, _) => Err(tr!("no GPU available for shaders", "GPU tidak tersedia untuk shader")),
                    (_, Err(e)) => Err(format!("{}: {e}", path.display())),
                }
            }
            Kind::Video => {
                let fps = entry.manifest.video.as_ref().map(|v| v.fps).unwrap_or(0);
                VideoRenderer::new(hwnd, size, &path, fps, display, opts)
                    .map(|r| Box::new(r) as Box<dyn Renderer>)
                    .map_err(|e| e.to_string())
            }
            Kind::Web => {
                let url = if entry.manifest.is_remote() { entry.manifest.entry.clone() } else { file_url(&path) };
                WebRenderer::new(&self.web_ctx, Hwnd(hwnd), size, &url, display.zoom, opts)
                    .map(|r| Box::new(r) as Box<dyn Renderer>)
            }
        };
        match result {
            Ok(r) => {
                log::info!("monitor {}: {} ({})", idx, entry.manifest.title, entry.manifest.kind.label());
                r
            }
            Err(e) => {
                log::error!("monitor {}: cannot load {}: {e}", idx, entry.id);
                Box::new(Solid::new(hwnd, tr!("error: {e}", "galat: {e}")))
            }
        }
    }

    /// Start the configured wallpaper on every host whose assignment changed
    /// (or on all of them with `force`).
    fn load_wallpapers(&mut self, force: bool) {
        for i in 0..self.hosts.len() {
            let want = self.config.wallpaper_for(self.hosts[i].monitor.index).map(str::to_string);
            if !force && want == self.hosts[i].wallpaper && self.hosts[i].renderer.is_some() {
                continue;
            }
            // A window can hold only one flip-model swap chain and wgpu frees its
            // surface lazily, so every renderer gets a fresh window.
            self.hosts[i].renderer = None;
            if let Some(gpu) = &self.gpu {
                let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(1)) });
            }
            if let Some(desk) = self.desktop {
                match desktop::create_host(&desk, &self.hosts[i].monitor) {
                    Ok(hwnd) => {
                        let old = std::mem::replace(&mut self.hosts[i].hwnd, hwnd);
                        unsafe {
                            let _ = DestroyWindow(old);
                        }
                    }
                    Err(e) => log::error!("host monitor {}: {e}", self.hosts[i].monitor.index),
                }
            }
            let renderer = match &want {
                Some(id) => self.make_renderer(i, id),
                None => Box::new(Solid::new(self.hosts[i].hwnd, tr!("no wallpaper", "tidak ada wallpaper"))),
            };
            let host = &mut self.hosts[i];
            host.renderer = Some(renderer);
            host.wallpaper = want;
            host.paused = None;
            host.next = Some(Instant::now());
        }
        self.update_pause();
    }

    fn update_pause(&mut self) {
        let g = self.config.general.clone();
        for host in self.hosts.iter_mut() {
            let inputs = PauseInputs {
                coverage: desktop::coverage(&host.monitor),
                power: Some(self.power),
                session_locked: self.locked,
                display_off: self.display_off,
                user_paused: self.user_paused,
            };
            let reason = pause::decide(&inputs, &g);
            if reason != host.paused {
                log::info!("monitor {} pause: {:?}", host.monitor.index, reason);
                host.paused = reason;
                if let Some(r) = host.renderer.as_mut() {
                    r.set_paused(reason.is_some());
                }
                host.next = if reason.is_some() { None } else { Some(Instant::now()) };
            }
        }
    }

    fn reprobe(&mut self, full: bool) {
        if full {
            self.hw = probe::probe();
        } else {
            self.hw.monitors = probe::monitors();
        }
        self.report = commands::tier_report(&self.hw, &self.profiles, &self.config);
    }

    fn periodic(&mut self) {
        self.ticks += 1;
        // Power source: shader fps depends on it.
        let (power, percent) = probe::power_status();
        self.hw.battery_percent = percent;
        if power != self.power {
            log::info!("power: {:?} -> {:?}", self.power, power);
            self.power = power;
            self.hw.power = power;
            if self.config.general.battery_saver {
                self.load_wallpapers(true);
            }
        }
        if self.desktop.is_none() || self.hosts.is_empty() {
            if self.ticks.is_multiple_of(4) {
                self.attach();
            }
        } else if self.desktop.is_some_and(|d| unsafe { !IsWindow(Some(d.parent)).as_bool() }) {
            log::warn!("desktop window gone, attaching again");
            self.attach();
        }
        // Config edited by hand or by `nouwallctl`.
        if self.ticks.is_multiple_of(4) {
            let m = mtime(&paths::config_file());
            if m != self.config_mtime {
                self.config_mtime = m;
                self.reload_config();
            }
        }
        self.update_pause();
    }

    fn reload_config(&mut self) {
        self.config = commands::load_config();
        commands::apply_language(&self.config);
        self.report = commands::tier_report(&self.hw, &self.profiles, &self.config);
        self.rescan_library();
        self.load_wallpapers(false);
        for h in self.hosts.iter_mut() {
            if let Some(r) = h.renderer.as_mut() {
                r.set_audio(self.config.general.mute, self.config.general.volume);
            }
        }
        self.refresh_tray();
        self.push_state();
    }

    fn save_config(&mut self) {
        if let Err(e) = commands::save_config(&self.config) {
            log::error!("save config: {e}");
        }
        self.config_mtime = mtime(&paths::config_file());
    }

    fn set_user_paused(&mut self, paused: bool) {
        self.user_paused = paused;
        self.update_pause();
        self.refresh_tray();
        self.push_state();
    }

    fn apply(&mut self, id: &str, target: MonitorTarget, force: bool) -> Result<Value, String> {
        let entry = self.library.get(id).map_err(|e| e.to_string())?;
        let warnings = commands::warnings(&entry, &self.hw, &self.report, &target);
        let serious = warnings.iter().any(|w| w.severity >= Severity::Warn);
        if serious && !force && !self.config.general.suppress_warnings {
            return Ok(json!({ "applied": false, "warnings": warnings }));
        }
        self.config.assign(target, id);
        self.save_config();
        self.load_wallpapers(false);
        self.refresh_tray();
        Ok(json!({ "applied": true, "warnings": warnings }))
    }

    fn open_settings(&mut self) {
        if let Some(s) = &self.settings {
            s.focus();
            return;
        }
        match SettingsWindow::open(&self.web_ctx, self.ipc.clone()) {
            Ok(s) => self.settings = Some(s),
            Err(e) => log::error!("settings window: {e}"),
        }
    }

    fn open_path(&self, path: &Path) {
        let _ = std::fs::create_dir_all(path);
        unsafe {
            ShellExecuteW(None, w!("open"), &hstring(&path.to_string_lossy()), None, None, SW_SHOWNORMAL);
        }
    }

    fn state(&self) -> Value {
        let target = MonitorTarget::ALL;
        let library: Vec<Value> = self
            .entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.manifest.title,
                    "kind": e.manifest.kind,
                    "entry": e.manifest.entry,
                    "display": e.manifest.display,
                    "author": e.manifest.author,
                    "description": e.manifest.description,
                    "video": e.manifest.video,
                    "shader_cost": e.manifest.shader_cost,
                    "remote": e.manifest.is_remote(),
                    "warnings": commands::warnings(e, &self.hw, &self.report, &target),
                })
            })
            .collect();
        let monitors: Vec<Value> = self
            .hw
            .monitors
            .iter()
            .map(|m| {
                let host = self.hosts.iter().find(|h| h.monitor.index == m.index);
                json!({
                    "index": m.index,
                    "device": m.device,
                    "x": m.x,
                    "y": m.y,
                    "width": m.width,
                    "height": m.height,
                    "refresh_hz": m.refresh_hz,
                    "primary": m.primary,
                    "wallpaper": self.config.wallpaper_for(m.index),
                    "status": host.and_then(|h| h.renderer.as_ref()).map(|r| r.status()).unwrap_or_default(),
                    "paused": host.and_then(|h| h.paused),
                })
            })
            .collect();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "lang": i18n::current(),
            "hardware": self.hw,
            "tier": self.report,
            "monitors": monitors,
            "library": library,
            "config": self.config,
            "autostart": autostart::is_enabled(),
            "paused": self.user_paused,
            "attached": self.desktop.is_some(),
            "paths": { "library": paths::library_dir(), "log": paths::log_file(), "config": paths::config_file() },
        })
    }

    fn push_state(&self) {
        if let Some(s) = &self.settings {
            s.send(&format!("window.__nouwall && window.__nouwall.emit('state', {})", self.state()));
        }
    }

    fn handle_ipc(&mut self, raw: &str) {
        let req: IpcRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("ipc: {e}");
                return;
            }
        };
        let result = self.ipc_command(&req.cmd, &req.args);
        let (ok, payload) = match result {
            Ok(v) => (true, v),
            Err(e) => (false, json!(e)),
        };
        if let Some(s) = &self.settings {
            s.send(&format!("window.__nouwall && window.__nouwall.resolve({}, {}, {})", req.id, ok, payload));
        }
        if req.cmd != "state" {
            self.push_state();
        }
    }

    fn parse_target(args: &Value) -> MonitorTarget {
        match args.get("monitor") {
            Some(Value::Number(n)) => MonitorTarget::Index(n.as_u64().unwrap_or(0) as usize),
            _ => MonitorTarget::ALL,
        }
    }

    fn parent_hwnd(&self) -> Option<Hwnd> {
        self.settings.as_ref().map(|s| Hwnd(s.hwnd))
    }

    fn ipc_command(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        let str_arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match cmd {
            "state" => Ok(self.state()),
            "apply" => {
                let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
                self.apply(&str_arg("id"), Self::parse_target(args), force)
            }
            "import" => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title(tr!("Import wallpaper", "Impor wallpaper"))
                    .add_filter("Wallpaper", &["mp4", "m4v", "mov", "webm", "mkv", "wmv", "glsl", "frag", "wgsl", "html", "htm"])
                    .add_filter("Video", &["mp4", "m4v", "mov", "webm", "mkv", "wmv"])
                    .add_filter("Shader", &["glsl", "frag", "wgsl"])
                    .add_filter(tr!("Web page", "Halaman web"), &["html", "htm"]);
                if let Some(p) = self.parent_hwnd() {
                    dialog = dialog.set_parent(&p);
                }
                let Some(path) = dialog.pick_file() else { return Ok(Value::Null) };
                self.import(&path)
            }
            "import_folder" => {
                let mut dialog = rfd::FileDialog::new().set_title(tr!("Choose a folder with index.html", "Pilih folder berisi index.html"));
                if let Some(p) = self.parent_hwnd() {
                    dialog = dialog.set_parent(&p);
                }
                let Some(path) = dialog.pick_folder() else { return Ok(Value::Null) };
                self.import(&path)
            }
            "import_url" => {
                let entry = commands::import_url(&self.library, &str_arg("url"), &str_arg("title"))?;
                self.after_import(entry)
            }
            "remove" => {
                let id = str_arg("id");
                self.library.remove(&id).map_err(|e| e.to_string())?;
                self.config.unassign_wallpaper(&id);
                self.save_config();
                self.rescan_library();
                self.load_wallpapers(false);
                self.refresh_tray();
                Ok(Value::Null)
            }
            "settings" => {
                let general: GeneralSettings =
                    serde_json::from_value(args.get("general").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
                let audio_changed = general.mute != self.config.general.mute || general.volume != self.config.general.volume;
                // A language switch alone must not restart the wallpapers.
                let only_language = GeneralSettings { language: self.config.general.language, ..general.clone() } == self.config.general;
                if general.autostart != autostart::is_enabled() {
                    autostart::set(general.autostart)?;
                }
                self.config.general = general;
                commands::apply_language(&self.config);
                self.report = commands::tier_report(&self.hw, &self.profiles, &self.config);
                self.save_config();
                if only_language {
                    self.refresh_tray();
                    return Ok(Value::Null);
                }
                if audio_changed {
                    for h in self.hosts.iter_mut() {
                        if let Some(r) = h.renderer.as_mut() {
                            r.set_audio(self.config.general.mute, self.config.general.volume);
                        }
                    }
                }
                // Frame-rate / tier changes need fresh renderers.
                self.load_wallpapers(true);
                self.refresh_tray();
                Ok(Value::Null)
            }
            "display" => {
                let id = str_arg("id");
                let display: Display =
                    serde_json::from_value(args.get("display").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
                let mut entry = self.library.get(&id).map_err(|e| e.to_string())?;
                entry.manifest.display = display.sanitized();
                self.library.update_manifest(&id, &entry.manifest).map_err(|e| e.to_string())?;
                self.rescan_library();
                // Restart the wallpaper wherever it is showing.
                for h in self.hosts.iter_mut().filter(|h| h.wallpaper.as_deref() == Some(id.as_str())) {
                    h.renderer = None;
                }
                self.load_wallpapers(false);
                Ok(json!(entry.manifest.display))
            }
            "pause" => {
                let paused = args.get("paused").and_then(Value::as_bool).unwrap_or(!self.user_paused);
                self.set_user_paused(paused);
                Ok(json!(paused))
            }
            "rescan" => {
                self.profiles = commands::load_profiles();
                self.reprobe(true);
                self.rescan_library();
                Ok(self.state())
            }
            "open_library" => {
                self.open_path(&paths::library_dir());
                Ok(Value::Null)
            }
            "open_log" => {
                unsafe {
                    ShellExecuteW(None, w!("open"), &hstring(&paths::log_file().to_string_lossy()), None, None, SW_SHOWNORMAL);
                }
                Ok(Value::Null)
            }
            "open_url" => {
                let url = str_arg("url");
                if url.starts_with("https://") {
                    unsafe {
                        ShellExecuteW(None, w!("open"), &hstring(&url), None, None, SW_SHOWNORMAL);
                    }
                }
                Ok(Value::Null)
            }
            "quit" => {
                self.quit = true;
                Ok(Value::Null)
            }
            other => Err(tr!("unknown command: {other}", "perintah tidak dikenal: {other}")),
        }
    }

    fn import(&mut self, path: &Path) -> Result<Value, String> {
        let entry = commands::import_path(&self.library, path)?;
        self.after_import(entry)
    }

    fn after_import(&mut self, entry: Entry) -> Result<Value, String> {
        log::info!("import {} ({})", entry.id, entry.manifest.kind.label());
        let warnings = commands::warnings(&entry, &self.hw, &self.report, &MonitorTarget::ALL);
        self.rescan_library();
        self.refresh_tray();
        Ok(json!({ "id": entry.id, "title": entry.manifest.title, "warnings": warnings }))
    }

    fn handle_command(&mut self, cmd: &str) {
        log::info!("command: {cmd}");
        let mut parts = cmd.split_whitespace();
        match parts.next().unwrap_or("") {
            "settings" => self.open_settings(),
            "pause" => self.set_user_paused(true),
            "resume" => self.set_user_paused(false),
            "toggle" => self.set_user_paused(!self.user_paused),
            "reload" => self.reload_config(),
            "quit" => self.quit = true,
            other => log::warn!("unknown command: {other}"),
        }
    }

    fn handle_tray(&mut self, action: TrayAction) {
        match action {
            TrayAction::OpenSettings => self.open_settings(),
            TrayAction::TogglePause => self.set_user_paused(!self.user_paused),
            TrayAction::Apply(id) => match self.apply(&id, MonitorTarget::ALL, false) {
                Ok(v) if v["applied"] == json!(false) => {
                    // Needs confirmation: open the settings window with the warning dialog.
                    self.open_settings();
                    if let Some(s) = &self.settings {
                        s.send(&format!("window.__nouwall && window.__nouwall.emit('confirm', {})", json!({ "id": id, "monitor": "all", "warnings": v["warnings"] })));
                    }
                }
                Ok(_) => self.push_state(),
                Err(e) => log::error!("apply {id}: {e}"),
            },
            TrayAction::ToggleAutostart => {
                if let Err(e) = autostart::set(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
                self.config.general.autostart = autostart::is_enabled();
                self.save_config();
                self.refresh_tray();
                self.push_state();
            }
            TrayAction::OpenLibrary => self.open_path(&paths::library_dir()),
            TrayAction::Quit => self.quit = true,
        }
    }

    fn drain_events(&mut self) {
        for ev in EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut())) {
            match ev {
                ControllerEvent::Command(c) => self.handle_command(&c),
                ControllerEvent::DisplayChanged => {
                    log::info!("display changed, attaching again");
                    self.reprobe(false);
                    self.attach();
                    self.push_state();
                }
                ControllerEvent::ExplorerRestarted => {
                    log::info!("Explorer restarted, attaching again");
                    self.attach();
                }
                ControllerEvent::Locked(l) => {
                    self.locked = l;
                    self.update_pause();
                }
                ControllerEvent::DisplayOn(on) => {
                    self.display_off = !on;
                    self.update_pause();
                }
            }
        }
        for ev in settings::take_events() {
            match ev {
                settings::WindowEvent::Resized => {
                    if let Some(s) = &self.settings {
                        s.fit();
                    }
                }
                settings::WindowEvent::Closed => self.settings = None,
            }
        }
        let pending: Vec<String> = std::mem::take(&mut *self.ipc.borrow_mut());
        for raw in pending {
            self.handle_ipc(&raw);
        }
        let actions = self.tray.as_ref().map(|t| t.poll()).unwrap_or_default();
        for a in actions {
            self.handle_tray(a);
        }
    }

    fn render_due(&mut self, now: Instant) {
        for host in self.hosts.iter_mut() {
            if let (Some(next), Some(r)) = (host.next, host.renderer.as_mut()) {
                if next <= now {
                    host.next = r.frame(now);
                }
            }
        }
    }

    fn next_deadline(&self) -> Instant {
        self.hosts.iter().filter_map(|h| h.next).fold(self.next_tick, Instant::min)
    }

    fn wait_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline <= now {
            return;
        }
        let wait = deadline - now;
        unsafe {
            // Negative = relative time in 100 ns units.
            let due = -((wait.as_nanos() / 100) as i64).max(1);
            let handles = [self.timer];
            if SetWaitableTimer(self.timer, &due, 0, None, None, false).is_ok() {
                MsgWaitForMultipleObjectsEx(Some(&handles), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            } else {
                MsgWaitForMultipleObjectsEx(None, wait.as_millis() as u32, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
    }

    fn pump(&mut self) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    self.quit = true;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn run(mut self) -> i32 {
        if self.config.assignments.is_empty() || self.entries.is_empty() {
            self.open_settings();
        }
        while !self.quit {
            self.wait_until(self.next_deadline());
            self.pump();
            self.drain_events();
            let now = Instant::now();
            if now >= self.next_tick {
                self.periodic();
                self.next_tick = now + TICK;
            }
            self.render_due(Instant::now());
        }
        log::info!("exit");
        if let Some(s) = self.settings.take() {
            s.close();
        }
        self.hosts.clear();
        self.tray = None;
        desktop::refresh_static_wallpaper();
        unsafe {
            let _ = DestroyWindow(self.controller);
        }
        0
    }
}
