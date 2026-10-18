//! `config.toml`: user settings and which wallpaper runs on which monitor.

use serde::{Deserialize, Serialize};

use crate::tier::Tier;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Upper frame rate for shader wallpapers; the tier budget may lower it further.
    pub fps_cap: u32,
    pub pause_on_fullscreen: bool,
    pub pause_on_maximized: bool,
    pub pause_on_battery: bool,
    /// Halve the frame rate on battery when not pausing.
    pub battery_saver: bool,
    pub mute: bool,
    pub volume: f32,
    pub autostart: bool,
    /// Let the governor lower fps / render scale when frames run late.
    pub auto_downgrade: bool,
    /// Skip warnings for this hardware and always apply.
    pub suppress_warnings: bool,
    pub tier_override: Option<Tier>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            fps_cap: 60,
            pause_on_fullscreen: true,
            pause_on_maximized: false,
            pause_on_battery: false,
            battery_saver: true,
            mute: true,
            volume: 0.5,
            autostart: false,
            auto_downgrade: true,
            suppress_warnings: false,
            tier_override: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", untagged)]
pub enum MonitorTarget {
    /// `"all"`
    All(AllMonitors),
    /// Monitor index as enumerated at startup (0 = first).
    Index(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AllMonitors {
    All,
}

impl MonitorTarget {
    pub const ALL: MonitorTarget = MonitorTarget::All(AllMonitors::All);

    pub fn matches(&self, index: usize) -> bool {
        match self {
            MonitorTarget::All(_) => true,
            MonitorTarget::Index(i) => *i == index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub monitor: MonitorTarget,
    /// Library id (folder name).
    pub wallpaper: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralSettings,
    #[serde(rename = "assign")]
    pub assignments: Vec<Assignment>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config serializes")
    }

    /// Wallpaper for a monitor: a specific index wins over `"all"`.
    pub fn wallpaper_for(&self, index: usize) -> Option<&str> {
        self.assignments
            .iter()
            .find(|a| a.monitor == MonitorTarget::Index(index))
            .or_else(|| self.assignments.iter().find(|a| a.monitor == MonitorTarget::ALL))
            .map(|a| a.wallpaper.as_str())
    }

    /// Assign a wallpaper. Assigning to all monitors clears per-monitor entries.
    pub fn assign(&mut self, monitor: MonitorTarget, wallpaper: impl Into<String>) {
        if monitor == MonitorTarget::ALL {
            self.assignments.clear();
        } else {
            self.assignments.retain(|a| a.monitor != monitor);
        }
        self.assignments.push(Assignment { monitor, wallpaper: wallpaper.into() });
    }

    /// Drop every assignment that points at `wallpaper` (used when it is deleted).
    pub fn unassign_wallpaper(&mut self, wallpaper: &str) {
        self.assignments.retain(|a| a.wallpaper != wallpaper);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let c = Config::parse("").unwrap();
        assert_eq!(c, Config::default());
        assert!(c.general.pause_on_fullscreen);
    }

    #[test]
    fn specific_monitor_overrides_all() {
        let mut c = Config::default();
        c.assign(MonitorTarget::ALL, "waves");
        c.assign(MonitorTarget::Index(1), "ocean");
        assert_eq!(c.wallpaper_for(0), Some("waves"));
        assert_eq!(c.wallpaper_for(1), Some("ocean"));
        c.assign(MonitorTarget::ALL, "aurora");
        assert_eq!(c.wallpaper_for(1), Some("aurora"));
    }

    #[test]
    fn toml_roundtrip_with_all_and_index() {
        let mut c = Config::default();
        c.general.tier_override = Some(Tier::Low);
        c.assign(MonitorTarget::ALL, "waves");
        c.assign(MonitorTarget::Index(2), "ocean");
        let text = c.to_toml();
        assert!(text.contains("monitor = \"all\""), "{text}");
        assert!(text.contains("monitor = 2"), "{text}");
        assert_eq!(Config::parse(&text).unwrap(), c);
    }

    #[test]
    fn partial_general_section_keeps_defaults() {
        let c = Config::parse("[general]\nfps_cap = 30\n").unwrap();
        assert_eq!(c.general.fps_cap, 30);
        assert!(c.general.mute);
    }

    #[test]
    fn unassign_removes_entries() {
        let mut c = Config::default();
        c.assign(MonitorTarget::Index(0), "a");
        c.assign(MonitorTarget::Index(1), "b");
        c.unassign_wallpaper("a");
        assert_eq!(c.wallpaper_for(0), None);
        assert_eq!(c.wallpaper_for(1), Some("b"));
    }
}
