//! Battle-wide field state: weather, terrain and pseudo-weather.

use crate::battle::MonRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weather {
    Rain,
    Sun,
    Sand,
    Snow,
}

impl Weather {
    pub fn id(self) -> &'static str {
        match self {
            Weather::Rain => "raindance",
            Weather::Sun => "sunnyday",
            Weather::Sand => "sandstorm",
            Weather::Snow => "snowscape",
        }
    }
    /// Showdown's display name in `-weather` lines.
    pub fn name(self) -> &'static str {
        match self {
            Weather::Rain => "RainDance",
            Weather::Sun => "SunnyDay",
            Weather::Sand => "Sandstorm",
            Weather::Snow => "Snowscape",
        }
    }
    pub fn from_id(s: &str) -> Option<Weather> {
        Some(match s {
            "raindance" => Weather::Rain,
            "sunnyday" => Weather::Sun,
            "sandstorm" => Weather::Sand,
            "snowscape" | "snow" => Weather::Snow,
            _ => return None,
        })
    }
    /// The item that extends this weather to 8 turns.
    pub fn rock(self) -> &'static str {
        match self {
            Weather::Rain => "damprock",
            Weather::Sun => "heatrock",
            Weather::Sand => "smoothrock",
            Weather::Snow => "icyrock",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Terrain {
    Grassy,
    Psychic,
    Electric,
    Misty,
}

impl Terrain {
    pub fn id(self) -> &'static str {
        match self {
            Terrain::Grassy => "grassyterrain",
            Terrain::Psychic => "psychicterrain",
            Terrain::Electric => "electricterrain",
            Terrain::Misty => "mistyterrain",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Terrain::Grassy => "Grassy Terrain",
            Terrain::Psychic => "Psychic Terrain",
            Terrain::Electric => "Electric Terrain",
            Terrain::Misty => "Misty Terrain",
        }
    }
    pub fn from_id(s: &str) -> Option<Terrain> {
        Some(match s {
            "grassyterrain" => Terrain::Grassy,
            "psychicterrain" => Terrain::Psychic,
            "electricterrain" => Terrain::Electric,
            "mistyterrain" => Terrain::Misty,
            _ => return None,
        })
    }
    /// The type whose moves this terrain boosts by 5325/4096 for grounded users.
    pub fn boosted_type(self) -> &'static str {
        match self {
            Terrain::Grassy => "Grass",
            Terrain::Psychic => "Psychic",
            Terrain::Electric => "Electric",
            Terrain::Misty => "",
        }
    }
}

/// Field state: weather, terrain and the pseudo-weathers with remaining turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Field {
    pub weather: Option<crate::state::Weather>,
    pub weather_turns: u8,
    pub terrain: Option<crate::state::Terrain>,
    pub terrain_turns: u8,
    pub trick_room: u8,
    /// Gravity (a pseudo-weather): turns left, 0 = none.
    pub gravity: u8,
    /// Magic Room (a pseudo-weather): turns left, 0 = none, and the Pokémon
    /// that set it (`effectState.source`).
    pub magic_room: u8,
    pub magic_room_source: Option<MonRef>,
    /// Wonder Room (a pseudo-weather): turns left, 0 = none.
    pub wonder_room: u8,
    /// Wonder Room's effect state has a target once one of its callbacks has
    /// run (`runEvent` sets it); until then its handler sorts with sub-order 2
    /// instead of the field's 5.
    pub wonder_room_armed: bool,
    /// Fairy Lock (a pseudo-weather): turns left, 0 = none.
    pub fairy_lock: u8,
}
