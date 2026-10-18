//! `wallpaper.toml`: the manifest stored next to every wallpaper in the library.

use serde::{Deserialize, Serialize};

use crate::hardware::Codec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Shader,
    Video,
    Web,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Shader => "Shader",
            Kind::Video => "Video",
            Kind::Web => "Web",
        }
    }

    /// Guess the kind from a file name extension.
    pub fn from_extension(ext: &str) -> Option<Kind> {
        match ext.to_ascii_lowercase().as_str() {
            "glsl" | "frag" | "fs" | "wgsl" => Some(Kind::Shader),
            "mp4" | "m4v" | "mov" | "webm" | "mkv" | "wmv" | "avi" => Some(Kind::Video),
            "html" | "htm" => Some(Kind::Web),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub codec: Option<Codec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub title: String,
    pub kind: Kind,
    /// File relative to the wallpaper folder, or an `https://` URL for web wallpapers.
    pub entry: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoMeta>,
    /// Shader cost hint from the author: 1 (trivial) .. 5 (very heavy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_cost: Option<u8>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("manifest serializes")
    }

    pub fn is_remote(&self) -> bool {
        self.entry.starts_with("https://") || self.entry.starts_with("http://")
    }
}

/// Turn a title or file name into a folder-safe id: lowercase ascii, digits and dashes.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "wallpaper".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_video_manifest() {
        let m = Manifest::parse(
            r#"
            title = "Ocean"
            kind = "video"
            entry = "ocean.mp4"
            [video]
            width = 3840
            height = 2160
            fps = 30
            codec = "hevc"
            "#,
        )
        .unwrap();
        assert_eq!(m.kind, Kind::Video);
        let v = m.video.unwrap();
        assert_eq!((v.width, v.height, v.codec), (3840, 2160, Some(Codec::Hevc)));
    }

    #[test]
    fn roundtrip() {
        let m = Manifest {
            title: "Waves".into(),
            kind: Kind::Shader,
            entry: "waves.glsl".into(),
            author: "nou".into(),
            description: String::new(),
            video: None,
            shader_cost: Some(2),
        };
        assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Ocean (4K).mp4"), "my-ocean-4k-mp4");
        assert_eq!(slugify("***"), "wallpaper");
        assert_eq!(Kind::from_extension("MP4"), Some(Kind::Video));
        assert_eq!(Kind::from_extension("frag"), Some(Kind::Shader));
    }
}
