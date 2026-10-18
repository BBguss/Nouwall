//! Per-user locations: `%APPDATA%\nouwall` for config and library,
//! `%LOCALAPPDATA%\nouwall` for logs and the WebView2 cache.

use std::path::PathBuf;

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

pub fn config_dir() -> PathBuf {
    env_dir("APPDATA").join("nouwall")
}

pub fn local_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("nouwall")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn profiles_file() -> PathBuf {
    config_dir().join("profiles.toml")
}

pub fn library_dir() -> PathBuf {
    config_dir().join("library")
}

pub fn log_file() -> PathBuf {
    local_dir().join("nouwall.log")
}

pub fn webview_dir() -> PathBuf {
    local_dir().join("webview2")
}

/// Folder holding the running executable (release zips ship `samples\` next to it).
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}
