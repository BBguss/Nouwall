//! Minimal file logger (`%LOCALAPPDATA%\nouwall\nouwall.log`), also echoed to
//! stderr for the console front-end.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

use crate::paths;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct Logger {
    file: Mutex<Option<File>>,
    echo: bool,
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= log::Level::Debug && (m.target().starts_with("nouwall") || m.level() <= log::Level::Warn)
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        let line = format!("{h:02}:{m:02}:{s:02}Z {:<5} {}: {}\n", r.level(), r.target(), r.args());
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
        }
        if self.echo {
            eprint!("{line}");
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

pub fn init(echo: bool, verbose: bool) {
    let path = paths::log_file();
    let _ = fs::create_dir_all(paths::local_dir());
    if fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let logger = Box::new(Logger { file: Mutex::new(file), echo });
    if log::set_boxed_logger(logger).is_ok() {
        log::set_max_level(if verbose { LevelFilter::Debug } else { LevelFilter::Info });
    }
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
        log::logger().flush();
    }));
}
