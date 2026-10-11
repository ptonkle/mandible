//! Match Settings and Player Settings: their shapes, defaults and the checks
//! that reject bad values by name (for example a Default Match Settings
//! value saved in browser storage by an older build).

use serde::{Deserialize, Serialize};

use crate::rules::Species;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    OneVOne,
    TwoVTwo,
    FreeForAll,
}

impl Format {
    pub fn seats(self) -> usize {
        match self {
            Format::OneVOne => 2,
            _ => 4,
        }
    }
    /// The team of each seat. In 2v2, seats 0 and 1 are allies and sit in
    /// neighbouring quarters of the map.
    pub fn team_of(self, seat: usize) -> u8 {
        match self {
            Format::OneVOne | Format::FreeForAll => seat as u8,
            Format::TwoVTwo => (seat / 2) as u8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    #[default]
    Normal,
    High,
}

impl Level {
    /// 80, 100 or 125 percent.
    pub fn percent(self) -> i32 {
        match self {
            Level::Low => 80,
            Level::Normal => 100,
            Level::High => 125,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GameSpeed {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl GameSpeed {
    /// The simulation runs at 20 ticks per second at Normal.
    pub fn ms_per_tick(self) -> i64 {
        match self {
            GameSpeed::Slow => 70,
            GameSpeed::Normal => 50,
            GameSpeed::Fast => 35,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Biome {
    #[default]
    Picnic,
    FlowerField,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Larva,
    #[default]
    Pupa,
    Imago,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MatchSettings {
    pub format: Format,
    pub map_size: Level,
    pub chamber_size: Level,
    pub tunnel_width: i32,
    pub resource_density: Level,
    pub starting_resources: Level,
    pub game_speed: GameSpeed,
    pub biome: Biome,
    pub events: bool,
    pub disabled: Vec<Species>,
    /// Every colony starts with the whole map Explored (not Visible).
    pub map_revealed: bool,
    /// Sandbox: human colonies have unlimited resources and start at Tier 3.
    pub sandbox: bool,
}

impl Default for MatchSettings {
    fn default() -> Self {
        MatchSettings {
            format: Format::OneVOne,
            map_size: Level::Normal,
            chamber_size: Level::Normal,
            tunnel_width: 12,
            resource_density: Level::Normal,
            starting_resources: Level::Normal,
            game_speed: GameSpeed::Normal,
            biome: Biome::Random,
            events: true,
            disabled: Vec::new(),
            map_revealed: false,
            sandbox: false,
        }
    }
}

pub const TUNNEL_MIN: i32 = 10;
pub const TUNNEL_MAX: i32 = 16;

/// Why a settings value cannot be used. The page shows the label-value form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    NotReadable(String),
    TunnelWidth(i32),
    Essential(Species),
}

impl SettingsError {
    pub fn message(&self) -> String {
        match self {
            SettingsError::NotReadable(d) => format!("Settings: not readable ({d})"),
            SettingsError::TunnelWidth(w) => {
                format!("Tunnel Width: {w} (allowed {TUNNEL_MIN}–{TUNNEL_MAX})")
            }
            SettingsError::Essential(s) => format!("Disabled: {} cannot be disabled", s.key()),
        }
    }
}

impl MatchSettings {
    pub fn check(&self) -> Result<(), SettingsError> {
        if !(TUNNEL_MIN..=TUNNEL_MAX).contains(&self.tunnel_width) {
            return Err(SettingsError::TunnelWidth(self.tunnel_width));
        }
        for s in &self.disabled {
            if !can_disable(*s) {
                return Err(SettingsError::Essential(*s));
            }
        }
        Ok(())
    }

    pub fn parse(json: &str) -> Result<MatchSettings, SettingsError> {
        let s: MatchSettings =
            serde_json::from_str(json).map_err(|e| SettingsError::NotReadable(e.to_string()))?;
        s.check()?;
        Ok(s)
    }

    pub fn is_disabled(&self, s: Species) -> bool {
        self.disabled.contains(&s)
    }
}

/// Harvester and Carpenter Ants keep the economy and building working, so
/// they can never be disabled.
pub fn can_disable(s: Species) -> bool {
    !matches!(s, Species::Harvester | Species::Carpenter)
}

/// Who sits in a seat for a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "difficulty")]
pub enum SeatKind {
    Human,
    Ai(Difficulty),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_round_trip() {
        let d = MatchSettings::default();
        assert!(d.check().is_ok());
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(MatchSettings::parse(&json).unwrap(), d);
    }

    #[test]
    fn settings_out_of_range_are_rejected_by_name() {
        let e = MatchSettings::parse(r#"{"tunnel_width": 4}"#).unwrap_err();
        assert_eq!(e, SettingsError::TunnelWidth(4));
        assert!(e.message().starts_with("Tunnel Width: 4"));
        let e = MatchSettings::parse(r#"{"disabled": ["harvester"]}"#).unwrap_err();
        assert_eq!(e, SettingsError::Essential(Species::Harvester));
        assert!(matches!(
            MatchSettings::parse(r#"{"nonsense": 1}"#),
            Err(SettingsError::NotReadable(_))
        ));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s = MatchSettings::parse(r#"{"format": "free_for_all"}"#).unwrap();
        assert_eq!(s.format, Format::FreeForAll);
        assert_eq!(s.tunnel_width, 12);
    }

    #[test]
    fn two_v_two_pairs_neighbouring_seats() {
        let f = Format::TwoVTwo;
        assert_eq!(f.team_of(0), f.team_of(1));
        assert_eq!(f.team_of(2), f.team_of(3));
        assert_ne!(f.team_of(0), f.team_of(2));
    }
}

#[cfg(test)]
mod default_tests {
    use super::*;

    #[test]
    fn the_default_biome_is_random() {
        assert_eq!(MatchSettings::default().biome, Biome::Random);
    }
}
