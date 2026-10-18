//! GUI entry point: runs the wallpaper engine with a tray icon.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(nouwall::cli::main(nouwall::cli::Frontend::Gui));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("nouwall only runs on Windows");
    std::process::exit(1);
}
