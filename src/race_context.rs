//! Champions Meeting **race context** that is announced for a race but does not
//! (currently) feed the stat-recommendation math: weather, season, and time of
//! day. These are pure data: the UI lets the player record what the CM
//! announcement shows, the summary line echoes them back, and they persist with
//! the build profile — but [`crate::cm_model`] ignores them. Only the course
//! (distance/surface) and the [`crate::cm_model::GroundCondition`] drive scoring.
//!
//! Discriminants follow the game / uma-sim convention so the icon-file basenames
//! line up (`utx_ico_weather_0{n-1}`, `utx_txt_season_0{n-1}`,
//! `utx_ico_timezone_0{icon}`). The icon assets are embedded by
//! [`crate::ui::icons`].

use serde::{Deserialize, Serialize};

/// Race weather. Order matches the game's `utx_ico_weather_0{0..3}` icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Weather {
    #[default]
    Sunny,
    Cloudy,
    Rainy,
    Snowy,
}

impl Weather {
    /// All weathers, in game order, for the icon picker.
    pub const ALL: [Weather; 4] = [Weather::Sunny, Weather::Cloudy, Weather::Rainy, Weather::Snowy];

    /// Short English label (tooltip / summary).
    pub fn label(self) -> &'static str {
        match self {
            Weather::Sunny => "Sunny",
            Weather::Cloudy => "Cloudy",
            Weather::Rainy => "Rainy",
            Weather::Snowy => "Snowy",
        }
    }

    /// Embedded icon basename (no extension): `utx_ico_weather_0{0..3}`.
    pub fn icon_name(self) -> &'static str {
        match self {
            Weather::Sunny => "utx_ico_weather_00",
            Weather::Cloudy => "utx_ico_weather_01",
            Weather::Rainy => "utx_ico_weather_02",
            Weather::Snowy => "utx_ico_weather_03",
        }
    }
}

/// Race season. Order matches the game's `utx_txt_season_0{0..3}` icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Season {
    #[default]
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    /// All seasons, in game order, for the icon picker.
    pub const ALL: [Season; 4] = [Season::Spring, Season::Summer, Season::Autumn, Season::Winter];

    /// Short English label (tooltip / summary).
    pub fn label(self) -> &'static str {
        match self {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        }
    }

    /// Embedded icon basename (no extension): `utx_txt_season_0{0..3}`.
    pub fn icon_name(self) -> &'static str {
        match self {
            Season::Spring => "utx_txt_season_00",
            Season::Summer => "utx_txt_season_01",
            Season::Autumn => "utx_txt_season_02",
            Season::Winter => "utx_txt_season_03",
        }
    }
}

/// Time of day shown by the CM announcement. The game exposes morning/noon/
/// evening/night, but only three icons exist (`utx_ico_timezone_0{0..2}` =
/// noon/evening/night), so we model those three — matching uma-sim's picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TimeOfDay {
    #[default]
    Noon,
    Evening,
    Night,
}

impl TimeOfDay {
    /// All times, in game order, for the icon picker.
    pub const ALL: [TimeOfDay; 3] = [TimeOfDay::Noon, TimeOfDay::Evening, TimeOfDay::Night];

    /// Short English label (tooltip / summary).
    pub fn label(self) -> &'static str {
        match self {
            TimeOfDay::Noon => "Noon",
            TimeOfDay::Evening => "Evening",
            TimeOfDay::Night => "Night",
        }
    }

    /// Embedded icon basename (no extension): `utx_ico_timezone_0{0..2}`.
    pub fn icon_name(self) -> &'static str {
        match self {
            TimeOfDay::Noon => "utx_ico_timezone_00",
            TimeOfDay::Evening => "utx_ico_timezone_01",
            TimeOfDay::Night => "utx_ico_timezone_02",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_first_option() {
        assert_eq!(Weather::default(), Weather::Sunny);
        assert_eq!(Season::default(), Season::Spring);
        assert_eq!(TimeOfDay::default(), TimeOfDay::Noon);
    }

    #[test]
    fn icon_names_match_embedded_assets() {
        // Each icon_name must correspond to a shipped PNG basename.
        for w in Weather::ALL {
            assert!(w.icon_name().starts_with("utx_ico_weather_0"));
        }
        for s in Season::ALL {
            assert!(s.icon_name().starts_with("utx_txt_season_0"));
        }
        for t in TimeOfDay::ALL {
            assert!(t.icon_name().starts_with("utx_ico_timezone_0"));
        }
    }

    #[test]
    fn labels_are_non_empty_and_serde_round_trips() {
        for w in Weather::ALL {
            assert!(!w.label().is_empty());
            let j = serde_json::to_string(&w).expect("weather serializes");
            assert_eq!(serde_json::from_str::<Weather>(&j).expect("weather deserializes"), w);
        }
    }
}
