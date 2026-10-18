//! Hardware description filled in by the platform probe.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
    Vp9,
    Av1,
}

impl Codec {
    pub const ALL: [Codec; 4] = [Codec::H264, Codec::Hevc, Codec::Vp9, Codec::Av1];

    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::Vp9 => "VP9",
            Codec::Av1 => "AV1",
        }
    }

    /// Best-effort mapping from a FourCC or codec name to a codec.
    pub fn from_name(name: &str) -> Option<Codec> {
        let n = name.trim().to_ascii_lowercase();
        match n.as_str() {
            "h264" | "avc" | "avc1" | "h.264" | "x264" => Some(Codec::H264),
            "hevc" | "h265" | "h.265" | "hvc1" | "hev1" | "x265" => Some(Codec::Hevc),
            "vp9" | "vp90" | "vp09" => Some(Codec::Vp9),
            "av1" | "av01" => Some(Codec::Av1),
            _ => None,
        }
    }
}

/// Largest resolution the hardware decoder accepts for one codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodeCaps {
    pub codec: Codec,
    pub max_width: u32,
    pub max_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuKind {
    Integrated,
    Discrete,
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor_id: u32,
    pub kind: GpuKind,
    pub dedicated_vram_mb: u64,
    /// D3D feature level encoded as e.g. `0xb100` for 11_1, `0xc100` for 12_1.
    pub feature_level: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: usize,
    /// Stable device name such as `\\.\DISPLAY1`.
    pub device: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub gpu: GpuInfo,
    pub decode: Vec<DecodeCaps>,
    pub monitors: Vec<MonitorInfo>,
    pub logical_cpus: u32,
    pub ram_mb: u64,
    pub power: PowerSource,
    pub battery_percent: Option<u8>,
    pub os_build: u32,
}

impl HardwareInfo {
    pub fn decode_caps(&self, codec: Codec) -> Option<DecodeCaps> {
        self.decode.iter().copied().find(|c| c.codec == codec)
    }

    /// Whether the hardware decoder handles `codec` at `width`x`height`.
    pub fn can_hw_decode(&self, codec: Codec, width: u32, height: u32) -> bool {
        self.decode_caps(codec).is_some_and(|c| {
            // Allow portrait videos: compare the long and short edges.
            let (long, short) = (width.max(height), width.min(height));
            let (cap_long, cap_short) = (c.max_width.max(c.max_height), c.max_width.min(c.max_height));
            long <= cap_long && short <= cap_short
        })
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn hd620() -> HardwareInfo {
        HardwareInfo {
            gpu: GpuInfo {
                name: "Intel(R) HD Graphics 620".into(),
                vendor_id: 0x8086,
                kind: GpuKind::Integrated,
                dedicated_vram_mb: 128,
                feature_level: 0xc100,
            },
            decode: vec![
                DecodeCaps { codec: Codec::H264, max_width: 4096, max_height: 2304 },
                DecodeCaps { codec: Codec::Hevc, max_width: 4096, max_height: 2304 },
                DecodeCaps { codec: Codec::Vp9, max_width: 4096, max_height: 2304 },
            ],
            monitors: vec![MonitorInfo {
                index: 0,
                device: r"\\.\DISPLAY1".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                refresh_hz: 60,
                primary: true,
            }],
            logical_cpus: 4,
            ram_mb: 8192,
            power: PowerSource::Ac,
            battery_percent: Some(90),
            os_build: 22621,
        }
    }

    pub fn rtx() -> HardwareInfo {
        let mut hw = hd620();
        hw.gpu = GpuInfo {
            name: "NVIDIA GeForce RTX 3060".into(),
            vendor_id: 0x10de,
            kind: GpuKind::Discrete,
            dedicated_vram_mb: 12288,
            feature_level: 0xc200,
        };
        hw.decode.push(DecodeCaps { codec: Codec::Av1, max_width: 8192, max_height: 4352 });
        hw.logical_cpus = 16;
        hw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_names() {
        assert_eq!(Codec::from_name("avc1"), Some(Codec::H264));
        assert_eq!(Codec::from_name("HEVC"), Some(Codec::Hevc));
        assert_eq!(Codec::from_name("vp09"), Some(Codec::Vp9));
        assert_eq!(Codec::from_name("mpeg2"), None);
    }

    #[test]
    fn hw_decode_handles_portrait_and_missing_codec() {
        let hw = fixtures::hd620();
        assert!(hw.can_hw_decode(Codec::H264, 3840, 2160));
        assert!(hw.can_hw_decode(Codec::H264, 2160, 3840));
        assert!(!hw.can_hw_decode(Codec::H264, 7680, 4320));
        assert!(!hw.can_hw_decode(Codec::Av1, 1920, 1080));
    }
}
