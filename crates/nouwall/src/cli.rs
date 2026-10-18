//! Command-line handling shared by `nouwall.exe` (GUI) and `nouwallctl.exe` (console).

use std::path::PathBuf;

use nouwall_core::{tr, MonitorTarget, Severity};
use serde_json::json;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

use crate::{app, builtin, commands, instance, logging, media_info, paths, probe};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Frontend {
    Gui,
    Console,
}

const HELP_EN: &str = "nouwall - live wallpapers for Windows

Usage:
  nouwall                      run the engine (tray icon)
  nouwallctl probe [--json]    show hardware, tier and decode support
  nouwallctl list              list wallpapers in the library
  nouwallctl apply <id> [--monitor N] [--force]
                               apply a wallpaper (all monitors unless --monitor is given)
  nouwallctl import <file|folder>
                               import a video, shader (.glsl/.frag/.wgsl) or web page
  nouwallctl settings          open the settings window
  nouwallctl pause | resume | toggle | reload | quit
  nouwallctl paths             config, library and log locations
  nouwallctl capture [folder]  save wallpaper windows as BMP (diagnostics)
  nouwallctl --version
";

fn help() -> &'static str {
    match nouwall_core::i18n::current() {
        nouwall_core::Lang::Id => HELP_ID,
        nouwall_core::Lang::En => HELP_EN,
    }
}

const HELP_ID: &str = "nouwall - live wallpaper untuk Windows

Pemakaian:
  nouwall                      jalankan engine (tray icon)
  nouwallctl probe [--json]    tampilkan hardware, tier dan kemampuan decode
  nouwallctl list              daftar wallpaper di library
  nouwallctl apply <id> [--monitor N] [--force]
                               pasang wallpaper (semua monitor bila --monitor tidak diberikan)
  nouwallctl import <file|folder>
                               impor video, shader (.glsl/.frag/.wgsl) atau halaman web
  nouwallctl settings          buka jendela pengaturan
  nouwallctl pause | resume | toggle | reload | quit
  nouwallctl paths             lokasi config, library dan log
  nouwallctl capture [folder]  simpan isi jendela wallpaper sebagai BMP (diagnostik)
  nouwallctl --version
";

struct Out(Frontend);

impl Out {
    fn line(&self, s: impl AsRef<str>) {
        if self.0 == Frontend::Console {
            println!("{}", s.as_ref());
        } else {
            log::info!("{}", s.as_ref());
        }
    }
}

fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    media_info::startup();
}

pub fn main(frontend: Frontend) -> i32 {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--verbose" && a != "-v").collect();
    logging::init(frontend == Frontend::Console && verbose, verbose);
    let out = Out(frontend);
    commands::apply_language(&commands::load_config());

    let cmd = args.first().map(String::as_str).unwrap_or(if frontend == Frontend::Gui { "run" } else { "help" });
    match cmd {
        "run" => run_engine(),
        "help" | "--help" | "-h" => {
            out.line(help());
            0
        }
        "--version" | "version" => {
            out.line(format!("nouwall {}", env!("CARGO_PKG_VERSION")));
            0
        }
        "paths" => {
            out.line(format!("config  : {}", paths::config_file().display()));
            out.line(format!("profiles: {}", paths::profiles_file().display()));
            out.line(format!("library : {}", paths::library_dir().display()));
            out.line(format!("log     : {}", paths::log_file().display()));
            0
        }
        "probe" | "tier" => {
            init_com();
            probe_cmd(&out, args.iter().any(|a| a == "--json"))
        }
        "list" => {
            init_com();
            list_cmd(&out)
        }
        "apply" => {
            init_com();
            apply_cmd(&out, &args[1..])
        }
        "import" => {
            init_com();
            let Some(p) = args.get(1) else {
                out.line(tr!("usage: nouwallctl import <file|folder>", "pemakaian: nouwallctl import <file|folder>"));
                return 2;
            };
            let lib = commands::library();
            match commands::import_path(&lib, &PathBuf::from(p)) {
                Ok(e) => {
                    out.line(tr!("imported as \"{}\" ({})", "diimpor sebagai \"{}\" ({})", e.id, e.manifest.kind.label()));
                    if instance::is_running() {
                        instance::send("reload");
                    }
                    0
                }
                Err(e) => {
                    out.line(tr!("failed: {e}", "gagal: {e}"));
                    1
                }
            }
        }
        "capture" => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| paths::local_dir().join("capture"));
            let results = crate::capture::capture_all(&dir);
            if results.is_empty() {
                out.line(tr!("no wallpaper windows (is nouwall running?)", "tidak ada jendela wallpaper (nouwall tidak berjalan?)"));
                return 1;
            }
            for r in results {
                match r {
                    Ok(p) => out.line(tr!("saved {p}", "disimpan {p}")),
                    Err(e) => out.line(tr!("failed: {e}", "gagal: {e}")),
                }
            }
            0
        }
        "settings" | "pause" | "resume" | "toggle" | "reload" | "quit" => {
            if instance::send(cmd) {
                0
            } else if cmd == "settings" {
                run_engine()
            } else {
                out.line(tr!("nouwall is not running", "nouwall tidak sedang berjalan"));
                1
            }
        }
        other => {
            out.line(tr!("unknown command: {other}\n\n{}", "perintah tidak dikenal: {other}\n\n{}", help()));
            2
        }
    }
}

fn run_engine() -> i32 {
    let Some(_lock) = instance::acquire() else {
        // Already running: bring up its settings window instead.
        instance::send("settings");
        return 0;
    };
    init_com();
    log::info!("nouwall {} starting", env!("CARGO_PKG_VERSION"));
    match app::App::new() {
        Ok(a) => a.run(),
        Err(e) => {
            log::error!("cannot start: {e}");
            1
        }
    }
}

fn probe_cmd(out: &Out, as_json: bool) -> i32 {
    let hw = probe::probe();
    let profiles = commands::load_profiles();
    let config = commands::load_config();
    let report = commands::tier_report(&hw, &profiles, &config);
    if as_json {
        out.line(serde_json::to_string_pretty(&json!({ "hardware": hw, "tier": report })).unwrap_or_default());
        return 0;
    }
    out.line(format!("GPU      : {} ({:?}, VRAM {} MB, vendor 0x{:04x}, FL 0x{:x})", hw.gpu.name, hw.gpu.kind, hw.gpu.dedicated_vram_mb, hw.gpu.vendor_id, hw.gpu.feature_level));
    out.line(format!("CPU/RAM  : {} thread, {} MB", hw.logical_cpus, hw.ram_mb));
    out.line(format!(
        "{}: {:?}{}",
        tr!("Power    ", "Daya     "),
        hw.power,
        hw.battery_percent.map(|p| tr!(" (battery {p}%)", " (baterai {p}%)")).unwrap_or_default()
    ));
    out.line(format!("Windows  : build {}", hw.os_build));
    for m in &hw.monitors {
        out.line(format!(
            "Monitor {}: {}x{} @ {} Hz{} {}",
            m.index,
            m.width,
            m.height,
            m.refresh_hz,
            if m.primary { tr!(" (primary)", " (utama)") } else { String::new() },
            m.device
        ));
    }
    if hw.decode.is_empty() {
        out.line(tr!("Decode   : no hardware decoder found", "Decode   : tidak ada decoder hardware terdeteksi"));
    }
    for d in &hw.decode {
        out.line(tr!("Decode   : {:<5} up to {}x{}", "Decode   : {:<5} hingga {}x{}", d.codec.label(), d.max_width, d.max_height));
    }
    out.line(format!("Tier     : {}{}", report.tier.label(), if report.overridden { tr!(" (detected {})", " (terdeteksi {})", report.detected.label()) } else { String::new() }));
    for r in &report.reasons {
        out.line(format!("           - {r}"));
    }
    let b = report.budget;
    out.line(format!(
        "Budget   : video <= {}x{}@{}, shader {} fps, render scale {:.0}%",
        b.max_video_width,
        b.max_video_height,
        b.max_video_fps,
        b.shader_fps,
        b.render_scale * 100.0
    ));
    0
}

fn list_cmd(out: &Out) -> i32 {
    let lib = commands::library();
    let mut config = commands::load_config();
    if builtin::install(&lib, &mut config) {
        let _ = commands::save_config(&config);
    }
    let (entries, errors) = lib.scan();
    let current = config.wallpaper_for(0).map(str::to_string);
    for e in &entries {
        let mark = if current.as_deref() == Some(e.id.as_str()) { "*" } else { " " };
        let extra = match &e.manifest.video {
            Some(v) => format!(
                " {}x{}@{} {}",
                v.width,
                v.height,
                v.fps,
                v.codec.map(|c| c.label()).unwrap_or("?")
            ),
            None => String::new(),
        };
        out.line(format!("{mark} {:<24} {:<7} {}{extra}", e.id, e.manifest.kind.label(), e.manifest.title));
    }
    for e in errors {
        out.line(format!("! {e}"));
    }
    0
}

fn apply_cmd(out: &Out, args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        out.line(tr!("usage: nouwallctl apply <id> [--monitor N] [--force]", "pemakaian: nouwallctl apply <id> [--monitor N] [--force]"));
        return 2;
    };
    let force = args.iter().any(|a| a == "--force");
    let target = args
        .iter()
        .position(|a| a == "--monitor")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .map(MonitorTarget::Index)
        .unwrap_or(MonitorTarget::ALL);
    let lib = commands::library();
    let mut entry = match lib.get(id) {
        Ok(e) => e,
        Err(e) => {
            out.line(tr!("failed: {e}", "gagal: {e}"));
            return 1;
        }
    };
    commands::refresh_video_meta(&lib, &mut entry);
    let hw = probe::probe();
    let mut config = commands::load_config();
    let report = commands::tier_report(&hw, &commands::load_profiles(), &config);
    let warnings = commands::warnings(&entry, &hw, &report, &target);
    for w in &warnings {
        let tag = match w.severity {
            Severity::Critical => tr!("CRITICAL", "KRITIS"),
            Severity::Warn => tr!("WARNING", "PERINGATAN"),
            Severity::Info => "info".to_string(),
        };
        out.line(format!("[{tag}] {}\n         {}", w.title, w.message));
    }
    if warnings.iter().any(|w| w.severity >= Severity::Warn) && !force && !config.general.suppress_warnings {
        out.line(tr!("Not applied. Run again with --force to use this wallpaper anyway.", "Tidak dipasang. Ulangi dengan --force untuk tetap memakai wallpaper ini."));
        return 3;
    }
    config.assign(target, id.as_str());
    if let Err(e) = commands::save_config(&config) {
        out.line(tr!("cannot save config: {e}", "gagal menyimpan config: {e}"));
        return 1;
    }
    if instance::send("reload") {
        out.line(tr!("\"{}\" applied", "\"{}\" dipasang", entry.manifest.title));
    } else {
        // The engine may run in another session (e.g. when called over SSH);
        // it polls config.toml and picks the change up within ~2 s.
        out.line(tr!("\"{}\" saved; the running engine picks it up automatically", "\"{}\" disimpan; engine yang berjalan memuatnya otomatis", entry.manifest.title));
    }
    0
}
