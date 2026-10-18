//! `http://nouwall.localhost/`: the custom protocol behind the settings window.
//!
//! `/` serves the embedded UI and `/lib/<id>/<file>` serves files from the
//! wallpaper library so the page can preview videos, shaders and web pages.
//! Byte ranges are honoured so large videos stream instead of loading whole.

use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use wry::http::{header, Request, Response, StatusCode};

use crate::paths;

pub const SCHEME: &str = "nouwall";
/// URL the settings page is loaded from (wry maps custom schemes to this on Windows).
pub const ORIGIN: &str = "http://nouwall.localhost/";

const UI_HTML: &str = include_str!("../assets/ui/settings.html");
/// Largest slice returned for one range request.
const MAX_CHUNK: u64 = 4 * 1024 * 1024;

type Resp = Response<Cow<'static, [u8]>>;

pub fn handle(request: Request<Vec<u8>>) -> Resp {
    let path = request.uri().path();
    if path == "/" || path == "/index.html" {
        return respond(StatusCode::OK, "text/html; charset=utf-8", UI_HTML.as_bytes().to_vec());
    }
    let Some(rest) = path.strip_prefix("/lib/") else {
        return not_found();
    };
    let Some(file) = resolve(&paths::library_dir(), &percent_decode(rest)) else {
        return not_found();
    };
    let range = request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
    match serve_file(&file, range.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            log::debug!("ui_server {}: {e}", file.display());
            not_found()
        }
    }
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Resp {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

fn not_found() -> Resp {
    respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec())
}

/// Map `<id>/<relative path>` into the library, refusing anything that could
/// escape it (`..`, absolute paths, drive letters, alternate data streams).
fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    let mut parts = 0;
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        if part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        out.push(part);
        parts += 1;
    }
    if parts < 2 {
        return None;
    }
    let canon = out.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    (canon.starts_with(&root) && canon.is_file()).then_some(canon)
}

fn serve_file(path: &Path, range: Option<&str>) -> std::io::Result<Resp> {
    let mime = mime_for(path);
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let Some((start, end)) = range.and_then(|r| parse_range(r, len)) else {
        if range.is_some() {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Cow::Borrowed(&[][..]))
                .unwrap_or_else(|_| not_found()));
        }
        let mut body = Vec::with_capacity(len as usize);
        f.read_to_end(&mut body)?;
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Cow::Owned(body))
            .unwrap_or_else(|_| not_found()));
    };
    let end = end.min(start + MAX_CHUNK - 1);
    let mut body = vec![0u8; (end - start + 1) as usize];
    f.seek(SeekFrom::Start(start))?;
    f.read_exact(&mut body)?;
    Ok(Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| not_found()))
}

/// Parse a single `bytes=` range into inclusive offsets within `len`.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        // Suffix range: the last N bytes.
        let n: u64 = b.parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { len - 1 } else { b.parse::<u64>().ok()?.min(len - 1) };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("glsl" | "frag" | "fs" | "wgsl" | "txt" | "toml") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        Some("mp4" | "m4v" | "mov") => "video/mp4",
        Some("webm" | "mkv") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }

    #[test]
    fn decode() {
        assert_eq!(percent_decode("my%20clip.mp4"), "my clip.mp4");
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    #[test]
    fn rejects_escapes() {
        let root = std::env::temp_dir();
        assert!(resolve(&root, "../x/y").is_none());
        assert!(resolve(&root, "a/../../y").is_none());
        assert!(resolve(&root, "C:/Windows/win.ini").is_none());
        assert!(resolve(&root, "only-one-part").is_none());
    }
}
