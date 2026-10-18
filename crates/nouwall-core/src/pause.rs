//! When a wallpaper on one monitor should stop rendering.

use serde::{Deserialize, Serialize};

use crate::config::GeneralSettings;
use crate::hardware::PowerSource;

/// What covers a monitor from the foreground window's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coverage {
    #[default]
    None,
    Maximized,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PauseInputs {
    pub coverage: Coverage,
    pub power: Option<PowerSource>,
    pub session_locked: bool,
    pub display_off: bool,
    /// User paused from the tray.
    pub user_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    User,
    Locked,
    DisplayOff,
    Fullscreen,
    Maximized,
    Battery,
}

pub fn decide(inputs: &PauseInputs, settings: &GeneralSettings) -> Option<PauseReason> {
    if inputs.user_paused {
        return Some(PauseReason::User);
    }
    if inputs.session_locked {
        return Some(PauseReason::Locked);
    }
    if inputs.display_off {
        return Some(PauseReason::DisplayOff);
    }
    match inputs.coverage {
        Coverage::Fullscreen if settings.pause_on_fullscreen => return Some(PauseReason::Fullscreen),
        Coverage::Maximized if settings.pause_on_maximized => return Some(PauseReason::Maximized),
        _ => {}
    }
    if settings.pause_on_battery && inputs.power == Some(PowerSource::Battery) {
        return Some(PauseReason::Battery);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_pauses_by_default() {
        let s = GeneralSettings::default();
        let i = PauseInputs { coverage: Coverage::Fullscreen, ..Default::default() };
        assert_eq!(decide(&i, &s), Some(PauseReason::Fullscreen));
    }

    #[test]
    fn maximized_runs_by_default() {
        let s = GeneralSettings::default();
        let i = PauseInputs { coverage: Coverage::Maximized, ..Default::default() };
        assert_eq!(decide(&i, &s), None);
        let s = GeneralSettings { pause_on_maximized: true, ..s };
        assert_eq!(decide(&i, &s), Some(PauseReason::Maximized));
    }

    #[test]
    fn battery_only_when_enabled() {
        let i = PauseInputs { power: Some(PowerSource::Battery), ..Default::default() };
        assert_eq!(decide(&i, &GeneralSettings::default()), None);
        let s = GeneralSettings { pause_on_battery: true, ..Default::default() };
        assert_eq!(decide(&i, &s), Some(PauseReason::Battery));
    }

    #[test]
    fn user_and_lock_take_priority() {
        let s = GeneralSettings::default();
        let i = PauseInputs { coverage: Coverage::Fullscreen, session_locked: true, ..Default::default() };
        assert_eq!(decide(&i, &s), Some(PauseReason::Locked));
        let i = PauseInputs { user_paused: true, ..i };
        assert_eq!(decide(&i, &s), Some(PauseReason::User));
    }
}
