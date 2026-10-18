//! nouwall: live wallpaper engine for Windows.
//!
//! The binary `nouwall` runs the engine with a tray icon; `nouwallctl` is the
//! console front-end for scripting (`probe`, `list`, `apply`, ...).

pub use nouwall_core as core;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod builtin;
#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod cli;
#[cfg(windows)]
pub mod commands;
#[cfg(windows)]
pub mod desktop;
#[cfg(windows)]
pub mod instance;
#[cfg(windows)]
pub mod logging;
#[cfg(windows)]
pub mod media_info;
#[cfg(windows)]
pub mod paths;
#[cfg(windows)]
pub mod probe;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod settings;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod util;
