//! Warnings shown before a wallpaper is applied on a given machine.
//! Warnings never block: the UI offers "apply anyway" or "cancel".

use serde::{Deserialize, Serialize};

use crate::hardware::{GpuKind, HardwareInfo, MonitorInfo};
use crate::manifest::{Kind, Manifest};
use crate::tier::TierReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    NoHardwareDecode,
    UnknownCodec,
    ExceedsTier,
    HighFrameRate,
    Downscaled,
    HeavyShader,
    SoftwareGpu,
    RemoteContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub code: WarningCode,
    pub severity: Severity,
    pub title: String,
    pub message: String,
}

impl Warning {
    fn new(code: WarningCode, severity: Severity, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, severity, title: title.into(), message: message.into() }
    }
}

fn res_label(w: u32, h: u32) -> String {
    let short = w.min(h);
    match short {
        s if s >= 4320 => "8K".into(),
        s if s >= 2160 => "4K".into(),
        s if s >= 1440 => "1440p".into(),
        s if s >= 1080 => "1080p".into(),
        s if s >= 720 => "720p".into(),
        _ => format!("{w}x{h}"),
    }
}

/// Evaluate `manifest` against this machine. `monitor` is the target display
/// (use the largest one when applying to all monitors).
pub fn evaluate(manifest: &Manifest, hw: &HardwareInfo, report: &TierReport, monitor: &MonitorInfo) -> Vec<Warning> {
    let mut out = Vec::new();
    let budget = report.budget;
    let tier = report.tier.label();

    if hw.gpu.kind == GpuKind::Software {
        out.push(Warning::new(
            WarningCode::SoftwareGpu,
            Severity::Critical,
            "GPU tidak terdeteksi",
            "Windows memakai render software. Pasang driver GPU agar wallpaper animasi tidak membebani CPU.",
        ));
    }

    match manifest.kind {
        Kind::Video => {
            if let Some(v) = &manifest.video {
                let label = res_label(v.width, v.height);
                match v.codec {
                    Some(codec) if !hw.can_hw_decode(codec, v.width, v.height) => out.push(Warning::new(
                        WarningCode::NoHardwareDecode,
                        Severity::Critical,
                        format!("{} {label} tidak didukung decoder hardware", codec.label()),
                        format!(
                            "GPU \"{}\" tidak bisa men-decode video {} {}x{} secara hardware. Video akan di-decode CPU sehingga laptop panas dan baterai cepat habis. Gunakan H.264 atau resolusi lebih kecil.",
                            hw.gpu.name, codec.label(), v.width, v.height
                        ),
                    )),
                    None => out.push(Warning::new(
                        WarningCode::UnknownCodec,
                        Severity::Info,
                        "Codec video tidak diketahui",
                        "nouwall tidak bisa memastikan video ini didukung decoder hardware.",
                    )),
                    _ => {}
                }

                let (long, short) = (v.width.max(v.height), v.width.min(v.height));
                let (blong, bshort) = (budget.max_video_width, budget.max_video_height);
                if long > blong || short > bshort {
                    out.push(Warning::new(
                        WarningCode::ExceedsTier,
                        Severity::Warn,
                        format!("Perangkat Anda mungkin tidak mendukung {label} dengan lancar"),
                        format!(
                            "Perangkat ini masuk tier {tier} (maksimal disarankan {}). Video {label} bisa tersendat atau boros daya. Pilih versi {} bila tersedia.",
                            res_label(blong, bshort),
                            res_label(blong, bshort)
                        ),
                    ));
                }
                if v.fps > budget.max_video_fps {
                    out.push(Warning::new(
                        WarningCode::HighFrameRate,
                        Severity::Warn,
                        format!("Video {} fps melebihi tier {tier}", v.fps),
                        format!("Tier {tier} disarankan maksimal {} fps.", budget.max_video_fps),
                    ));
                }
                let (mlong, mshort) = (monitor.width.max(monitor.height), monitor.width.min(monitor.height));
                if long > mlong || short > mshort {
                    out.push(Warning::new(
                        WarningCode::Downscaled,
                        Severity::Info,
                        format!("Video {label} lebih besar dari layar"),
                        format!(
                            "Monitor {}x{} hanya menampilkan sebagian detail. Video yang sesuai resolusi layar lebih hemat daya.",
                            monitor.width, monitor.height
                        ),
                    ));
                }
            } else {
                out.push(Warning::new(
                    WarningCode::UnknownCodec,
                    Severity::Info,
                    "Info video belum dibaca",
                    "Resolusi dan codec video belum diketahui; impor ulang agar nouwall bisa memeriksanya.",
                ));
            }
        }
        Kind::Shader => {
            let cost = manifest.shader_cost.unwrap_or(2);
            if cost > budget.max_shader_cost {
                out.push(Warning::new(
                    WarningCode::HeavyShader,
                    Severity::Warn,
                    format!("Shader berat untuk tier {tier}"),
                    format!(
                        "Shader ini (beban {cost}/5) bisa membuat GPU bekerja keras. nouwall akan merender di {:.0}% resolusi dan menurunkan fps otomatis bila perlu.",
                        budget.render_scale * 100.0
                    ),
                ));
            }
        }
        Kind::Web => {
            if manifest.is_remote() {
                out.push(Warning::new(
                    WarningCode::RemoteContent,
                    Severity::Info,
                    "Wallpaper memuat halaman online",
                    "Halaman ini diambil dari internet setiap kali wallpaper dimulai dan bisa memakai kuota data.",
                ));
            }
        }
    }

    out.sort_by_key(|w| std::cmp::Reverse(w.severity));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{fixtures, Codec};
    use crate::manifest::VideoMeta;
    use crate::tier::{classify, Profiles};

    fn video(w: u32, h: u32, fps: u32, codec: Option<Codec>) -> Manifest {
        Manifest {
            title: "v".into(),
            kind: Kind::Video,
            entry: "v.mp4".into(),
            author: String::new(),
            description: String::new(),
            video: Some(VideoMeta { width: w, height: h, fps, codec }),
            shader_cost: None,
        }
    }

    fn codes(m: &Manifest, hw: &HardwareInfo) -> Vec<WarningCode> {
        let r = classify(hw, &Profiles::builtin(), None);
        evaluate(m, hw, &r, &hw.monitors[0]).into_iter().map(|w| w.code).collect()
    }

    #[test]
    fn hd620_warns_on_4k() {
        let hw = fixtures::hd620();
        let c = codes(&video(3840, 2160, 30, Some(Codec::H264)), &hw);
        assert!(c.contains(&WarningCode::ExceedsTier));
        assert!(c.contains(&WarningCode::Downscaled));
        assert!(!c.contains(&WarningCode::NoHardwareDecode));
    }

    #[test]
    fn hd620_has_no_av1_decode() {
        let hw = fixtures::hd620();
        let c = codes(&video(1920, 1080, 30, Some(Codec::Av1)), &hw);
        assert_eq!(c, vec![WarningCode::NoHardwareDecode]);
    }

    #[test]
    fn hd620_1080p30_is_clean() {
        let hw = fixtures::hd620();
        assert!(codes(&video(1920, 1080, 30, Some(Codec::H264)), &hw).is_empty());
    }

    #[test]
    fn hd620_60fps_warns() {
        let hw = fixtures::hd620();
        assert_eq!(codes(&video(1920, 1080, 60, Some(Codec::H264)), &hw), vec![WarningCode::HighFrameRate]);
    }

    #[test]
    fn rtx_4k_only_downscale_info_on_1080p_monitor() {
        let hw = fixtures::rtx();
        assert_eq!(codes(&video(3840, 2160, 60, Some(Codec::Av1)), &hw), vec![WarningCode::Downscaled]);
    }

    #[test]
    fn critical_sorted_first() {
        let hw = fixtures::hd620();
        let r = classify(&hw, &Profiles::builtin(), None);
        let w = evaluate(&video(7680, 4320, 30, Some(Codec::Av1)), &hw, &r, &hw.monitors[0]);
        assert_eq!(w[0].severity, Severity::Critical);
        assert!(w[0].title.contains("8K"));
    }

    #[test]
    fn heavy_shader_on_low() {
        let hw = fixtures::hd620();
        let mut m = video(0, 0, 0, None);
        m.kind = Kind::Shader;
        m.video = None;
        m.shader_cost = Some(4);
        assert_eq!(codes(&m, &hw), vec![WarningCode::HeavyShader]);
        m.shader_cost = Some(1);
        assert!(codes(&m, &hw).is_empty());
    }
}
