//! Operations shared by the CLI, the tray and the settings window.

use std::fs;
use std::path::Path;

use nouwall_core::policy::{self, Warning};
use nouwall_core::tier::{classify, TierReport};
use nouwall_core::{Config, Entry, HardwareInfo, Kind, Library, Manifest, MonitorInfo, MonitorTarget, Profiles};

use crate::{media_info, paths};

pub fn library() -> Library {
    Library::new(paths::library_dir())
}

/// Load `config.toml`; a broken file is kept as `.bak` and defaults are used.
pub fn load_config() -> Config {
    let path = paths::config_file();
    match fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).unwrap_or_else(|e| {
            log::error!("config.toml rusak ({e}), memakai default; salinan di config.toml.bak");
            let _ = fs::copy(&path, path.with_extension("toml.bak"));
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

pub fn save_config(config: &Config) -> std::io::Result<()> {
    fs::create_dir_all(paths::config_dir())?;
    let path = paths::config_file();
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, config.to_toml())?;
    fs::rename(tmp, path)
}

pub fn load_profiles() -> Profiles {
    match fs::read_to_string(paths::profiles_file()) {
        Ok(text) => Profiles::with_override(&text).unwrap_or_else(|e| {
            log::error!("profiles.toml tidak valid: {e}");
            Profiles::builtin()
        }),
        Err(_) => Profiles::builtin(),
    }
}

pub fn tier_report(hw: &HardwareInfo, profiles: &Profiles, config: &Config) -> TierReport {
    classify(hw, profiles, config.general.tier_override)
}

/// Monitor a warning check is made against: the chosen one, or the largest.
pub fn target_monitor(hw: &HardwareInfo, target: &MonitorTarget) -> Option<MonitorInfo> {
    match target {
        MonitorTarget::Index(i) => hw.monitors.get(*i).cloned(),
        MonitorTarget::All(_) => hw.monitors.iter().max_by_key(|m| u64::from(m.width) * u64::from(m.height)).cloned(),
    }
    .or_else(|| hw.monitors.first().cloned())
}

pub fn warnings(entry: &Entry, hw: &HardwareInfo, report: &TierReport, target: &MonitorTarget) -> Vec<Warning> {
    let fallback = MonitorInfo {
        index: 0,
        device: String::new(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        refresh_hz: 60,
        primary: true,
    };
    let monitor = target_monitor(hw, target).unwrap_or(fallback);
    policy::evaluate(&entry.manifest, hw, report, &monitor)
}

fn title_from(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Wallpaper".into())
}

/// Import a file (shader / video / html) or a folder containing `index.html`.
pub fn import_path(lib: &Library, path: &Path) -> Result<Entry, String> {
    if path.is_dir() {
        let index = ["index.html", "index.htm"].iter().find(|n| path.join(n).exists()).ok_or("folder tidak berisi index.html")?;
        let manifest = Manifest {
            title: title_from(path),
            kind: Kind::Web,
            entry: (*index).into(),
            author: String::new(),
            description: String::new(),
            video: None,
            shader_cost: None,
        };
        return lib.import_dir(path, manifest).map_err(|e| e.to_string());
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let kind = Kind::from_extension(ext).ok_or_else(|| format!("format .{ext} tidak didukung"))?;
    let video = if kind == Kind::Video {
        match media_info::read(path) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("gagal membaca info video {}: {e}", path.display());
                None
            }
        }
    } else {
        None
    };
    let manifest = Manifest {
        title: title_from(path),
        kind,
        entry: String::new(),
        author: String::new(),
        description: String::new(),
        video,
        shader_cost: (kind == Kind::Shader).then_some(2),
    };
    lib.import_file(path, manifest).map_err(|e| e.to_string())
}

pub fn import_url(lib: &Library, url: &str, title: &str) -> Result<Entry, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("URL harus diawali http:// atau https://".into());
    }
    let title = if title.trim().is_empty() { url.to_string() } else { title.trim().to_string() };
    let manifest = Manifest {
        title: title.clone(),
        kind: Kind::Web,
        entry: url.into(),
        author: String::new(),
        description: String::new(),
        video: None,
        shader_cost: None,
    };
    let id = lib.free_id(&title);
    lib.create(&id, &manifest, &[]).map_err(|e| e.to_string())
}

/// Re-read video metadata for entries imported before the info was available.
pub fn refresh_video_meta(lib: &Library, entry: &mut Entry) {
    if entry.manifest.kind != Kind::Video || entry.manifest.video.is_some() {
        return;
    }
    if let Ok(v) = media_info::read(&entry.entry_path()) {
        entry.manifest.video = Some(v);
        let _ = lib.update_manifest(&entry.id, &entry.manifest);
    }
}
