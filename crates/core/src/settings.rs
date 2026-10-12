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
    /// Leaf litter, twigs and pine needles on dark loam (TRIAGE row 44).
    ForestFloor,
    /// Pebbles, reeds and puddles by a stream (TRIAGE row 44).
    Riverbank,
    Random,
}

impl Biome {
    /// Every biome a map can have (Random excluded).
    pub const ALL: [Biome; 4] = [
        Biome::Picnic,
        Biome::FlowerField,
        Biome::ForestFloor,
        Biome::Riverbank,
    ];

    /// Random becomes one of the four, chosen by the seed, the same on every
    /// peer; any other biome stays itself.
    pub fn resolve(self, seed: u64) -> Biome {
        match self {
            Biome::Random => Biome::ALL[(seed % Biome::ALL.len() as u64) as usize],
            b => b,
        }
    }
}

/// The shape of a generated map (TRIAGE row 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MapType {
    /// Chambers joined by tunnels.
    Tunnels,
    /// Open ground with blobs of rock.
    Open,
    /// Open ground crossed by thin, connected rock walls.
    Maze,
    /// One of the three, chosen by the seed at match start (TRIAGE row 61).
    #[default]
    Random,
}

impl MapType {
    /// Every Map Type a map can have (Random excluded).
    pub const ALL: [MapType; 3] = [MapType::Tunnels, MapType::Open, MapType::Maze];

    /// Random becomes one of the three, chosen by the seed, the same on
    /// every peer; any other Map Type stays itself.
    pub fn resolve(self, seed: u64) -> MapType {
        match self {
            MapType::Random => MapType::ALL[(seed % MapType::ALL.len() as u64) as usize],
            t => t,
        }
    }
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
    pub map_type: MapType,
    /// Each seat's team (0-based), set in Play vs AI or the Lobby (TRIAGE
    /// row 80). Empty, or shorter than the seats, means the format's
    /// default for the seats it leaves out. Not saved with Default Match
    /// Settings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<u8>,
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
            map_type: MapType::Random,
            teams: Vec::new(),
        }
    }
}

pub const TUNNEL_MIN: i32 = 10;

/// The most teams a match can have: one per seat.
pub const TEAM_COUNT: u8 = 4;
pub const TUNNEL_MAX: i32 = 16;

/// Why a settings value cannot be used. The page shows the label-value form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    NotReadable(String),
    TunnelWidth(i32),
    Essential(Species),
    /// A team outside Team 1–4, or more teams listed than seats.
    Team(u8),
}

impl SettingsError {
    pub fn message(&self) -> String {
        match self {
            SettingsError::NotReadable(d) => format!("Settings: not readable ({d})"),
            SettingsError::TunnelWidth(w) => {
                format!("Tunnel Width: {w} (allowed {TUNNEL_MIN}–{TUNNEL_MAX})")
            }
            SettingsError::Essential(s) => format!("Disabled: {} cannot be disabled", s.key()),
            SettingsError::Team(t) => {
                format!("Team: {} (allowed 1–{TEAM_COUNT})", *t as i32 + 1)
            }
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
        if let Some(t) = self.teams.iter().find(|t| **t >= TEAM_COUNT) {
            return Err(SettingsError::Team(*t));
        }
        if self.teams.len() > TEAM_COUNT as usize {
            return Err(SettingsError::Team(self.teams.len() as u8 - 1));
        }
        Ok(())
    }

    /// The team of a seat: the one chosen for it, else the format's default.
    pub fn team_of(&self, seat: usize) -> u8 {
        self.teams
            .get(seat)
            .copied()
            .unwrap_or_else(|| self.format.team_of(seat))
    }

    /// Every seat's team, the format's default where none is chosen.
    pub fn seat_teams(&self) -> Vec<u8> {
        (0..self.format.seats()).map(|i| self.team_of(i)).collect()
    }

    /// Sets one seat's team; the other seats keep theirs (their defaults
    /// written out). False if the seat or team is out of range.
    pub fn set_team(&mut self, seat: usize, team: u8) -> bool {
        if seat >= self.format.seats() || team >= TEAM_COUNT {
            return false;
        }
        let mut teams = self.seat_teams();
        teams[seat] = team;
        self.teams = teams;
        true
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

/// How many teams have a colony among the seats that are played. A match
/// starts only with at least two (TRIAGE row 81).
pub fn team_count<I: IntoIterator<Item = u8>>(played_teams: I) -> usize {
    let mut seen: Vec<u8> = played_teams.into_iter().collect();
    seen.sort();
    seen.dedup();
    seen.len()
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
