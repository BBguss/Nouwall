//! Hardware tier classification driven by `profiles.toml`.
//!
//! The default profile list ships inside the binary. Users and contributors can
//! extend it with `%APPDATA%\nouwall\profiles.toml`; entries there are checked
//! first, so they override the built-in ones.

use serde::{Deserialize, Serialize};

use crate::tr;

use crate::hardware::{GpuKind, HardwareInfo, PowerSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Low,
    Mid,
    High,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Low => "Low",
            Tier::Mid => "Mid",
            Tier::High => "High",
        }
    }

    pub fn down(self) -> Tier {
        match self {
            Tier::High => Tier::Mid,
            _ => Tier::Low,
        }
    }
}

/// What a tier is expected to run smoothly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    pub max_video_width: u32,
    pub max_video_height: u32,
    pub max_video_fps: u32,
    pub shader_fps: u32,
    /// Fraction of the monitor resolution shaders render at (upscaled on present).
    pub render_scale: f32,
    /// Highest `shader_cost` hint that runs comfortably.
    pub max_shader_cost: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budgets {
    pub low: Budget,
    pub mid: Budget,
    pub high: Budget,
}

impl Budgets {
    pub fn get(&self, tier: Tier) -> Budget {
        match tier {
            Tier::Low => self.low,
            Tier::Mid => self.mid,
            Tier::High => self.high,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuProfile {
    /// Case-insensitive substrings of the adapter name.
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    pub tier: Tier,
    #[serde(default)]
    pub note: String,
    /// Indonesian version of `note`; `note` is used when empty.
    #[serde(default)]
    pub note_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default, rename = "gpu")]
    pub gpus: Vec<GpuProfile>,
    pub budget: Budgets,
}

pub const DEFAULT_PROFILES: &str = include_str!("../profiles.toml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesOverride {
    #[serde(default, rename = "gpu")]
    pub gpus: Vec<GpuProfile>,
    #[serde(default)]
    pub budget: Option<Budgets>,
}

impl Profiles {
    pub fn builtin() -> Self {
        toml::from_str(DEFAULT_PROFILES).expect("built-in profiles.toml is valid")
    }

    /// Built-in profiles extended by a user file (user entries match first).
    pub fn with_override(text: &str) -> Result<Self, toml::de::Error> {
        let user: ProfilesOverride = toml::from_str(text)?;
        let mut p = Self::builtin();
        let mut gpus = user.gpus;
        gpus.append(&mut p.gpus);
        p.gpus = gpus;
        if let Some(b) = user.budget {
            p.budget = b;
        }
        Ok(p)
    }

    fn match_gpu(&self, name: &str) -> Option<&GpuProfile> {
        let name = name.to_ascii_lowercase();
        self.gpus
            .iter()
            .find(|p| p.patterns.iter().any(|pat| name.contains(&pat.to_ascii_lowercase())))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierReport {
    /// Tier in effect (after override).
    pub tier: Tier,
    /// Tier the detection arrived at before any override.
    pub detected: Tier,
    pub overridden: bool,
    pub reasons: Vec<String>,
    pub budget: Budget,
}

pub fn classify(hw: &HardwareInfo, profiles: &Profiles, tier_override: Option<Tier>) -> TierReport {
    let mut reasons = Vec::new();
    let gpu = &hw.gpu;

    let mut tier = if let Some(p) = profiles.match_gpu(&gpu.name) {
        reasons.push(tr!("GPU \"{}\" matches the {} profile", "GPU \"{}\" cocok dengan profil {}", gpu.name, p.tier.label()));
        let note = if crate::i18n::current() == crate::i18n::Lang::Id && !p.note_id.is_empty() { &p.note_id } else { &p.note };
        if !note.is_empty() {
            reasons.push(note.clone());
        }
        p.tier
    } else {
        match gpu.kind {
            GpuKind::Software => {
                reasons.push(tr!("No hardware GPU (software rendering)", "Tidak ada GPU hardware (render software)"));
                Tier::Low
            }
            GpuKind::Discrete if gpu.dedicated_vram_mb >= 4096 => {
                reasons.push(tr!("Discrete GPU with {} MB VRAM", "GPU diskrit dengan VRAM {} MB", gpu.dedicated_vram_mb));
                Tier::High
            }
            GpuKind::Discrete => {
                reasons.push(tr!("Discrete GPU with little VRAM ({} MB)", "GPU diskrit dengan VRAM kecil ({} MB)", gpu.dedicated_vram_mb));
                Tier::Mid
            }
            GpuKind::Integrated => {
                reasons.push(tr!("Unknown integrated GPU \"{}\", assuming Low", "GPU terintegrasi \"{}\" tidak dikenal, diasumsikan Low", gpu.name));
                Tier::Low
            }
        }
    };

    if hw.logical_cpus > 0 && hw.logical_cpus < 4 && tier != Tier::Low {
        reasons.push(tr!("Only {} CPU threads, tier lowered", "Hanya {} thread CPU, tier diturunkan", hw.logical_cpus));
        tier = tier.down();
    }
    if hw.ram_mb > 0 && hw.ram_mb < 4096 && tier != Tier::Low {
        reasons.push(tr!("{} MB RAM, tier lowered", "RAM {} MB, tier diturunkan", hw.ram_mb));
        tier = tier.down();
    }

    let detected = tier;
    let (tier, overridden) = match tier_override {
        Some(t) if t != detected => {
            reasons.push(tr!("Tier manually set to {}", "Tier ditimpa manual menjadi {}", t.label()));
            (t, true)
        }
        _ => (detected, false),
    };

    TierReport { tier, detected, overridden, reasons, budget: profiles.budget.get(tier) }
}

/// Shader frame rate after applying the user cap, the tier budget and power state.
pub fn effective_shader_fps(report: &TierReport, fps_cap: u32, power: PowerSource, battery_saver: bool) -> u32 {
    let mut fps = report.budget.shader_fps.min(fps_cap.max(1));
    if power == PowerSource::Battery && battery_saver {
        fps = (fps / 2).max(10);
    }
    fps.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::fixtures;

    #[test]
    fn builtin_profiles_parse() {
        let p = Profiles::builtin();
        assert!(!p.gpus.is_empty());
        assert!(p.budget.low.max_video_height <= p.budget.high.max_video_height);
    }

    #[test]
    fn hd620_is_low() {
        let r = classify(&fixtures::hd620(), &Profiles::builtin(), None);
        assert_eq!(r.tier, Tier::Low);
        assert_eq!(r.budget.max_video_height, 1080);
    }

    #[test]
    fn rtx_is_high() {
        let r = classify(&fixtures::rtx(), &Profiles::builtin(), None);
        assert_eq!(r.tier, Tier::High);
    }

    #[test]
    fn iris_xe_is_mid() {
        let mut hw = fixtures::hd620();
        hw.gpu.name = "Intel(R) Iris(R) Xe Graphics".into();
        assert_eq!(classify(&hw, &Profiles::builtin(), None).tier, Tier::Mid);
    }

    #[test]
    fn weak_cpu_downgrades() {
        let mut hw = fixtures::rtx();
        hw.logical_cpus = 2;
        assert_eq!(classify(&hw, &Profiles::builtin(), None).tier, Tier::Mid);
    }

    #[test]
    fn override_wins_but_detected_is_kept() {
        let r = classify(&fixtures::hd620(), &Profiles::builtin(), Some(Tier::High));
        assert_eq!((r.tier, r.detected, r.overridden), (Tier::High, Tier::Low, true));
    }

    #[test]
    fn user_profiles_match_first() {
        let p = Profiles::with_override(
            "[[gpu]]\nmatch = [\"HD Graphics 620\"]\ntier = \"mid\"\nnote = \"tested ok\"\n",
        )
        .unwrap();
        assert_eq!(classify(&fixtures::hd620(), &p, None).tier, Tier::Mid);
    }

    #[test]
    fn battery_halves_fps() {
        let r = classify(&fixtures::rtx(), &Profiles::builtin(), None);
        assert_eq!(effective_shader_fps(&r, 60, PowerSource::Ac, true), 60);
        assert_eq!(effective_shader_fps(&r, 60, PowerSource::Battery, true), 30);
        assert_eq!(effective_shader_fps(&r, 60, PowerSource::Battery, false), 60);
        assert_eq!(effective_shader_fps(&r, 24, PowerSource::Ac, true), 24);
    }
}
