//! Small shared enums and ids.

use std::fmt;

pub type SpeciesId = u16;
pub type MoveId = u16;
pub type ItemId = u16;
pub type AbilityId = u16;
pub type TypeId = u8;

/// Index of the "no type" slot for single-typed Pokémon.
pub const NO_TYPE: TypeId = u8::MAX;

/// Stats in Showdown order. `Hp` is never boosted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stat {
    Hp = 0,
    Atk = 1,
    Def = 2,
    Spa = 3,
    Spd = 4,
    Spe = 5,
}

impl Stat {
    pub const ALL: [Stat; 6] = [
        Stat::Hp,
        Stat::Atk,
        Stat::Def,
        Stat::Spa,
        Stat::Spd,
        Stat::Spe,
    ];

    pub fn from_id(s: &str) -> Option<Stat> {
        Some(match s {
            "hp" => Stat::Hp,
            "atk" => Stat::Atk,
            "def" => Stat::Def,
            "spa" => Stat::Spa,
            "spd" => Stat::Spd,
            "spe" => Stat::Spe,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        ["hp", "atk", "def", "spa", "spd", "spe"][self as usize]
    }
}

/// Boostable stats: atk, def, spa, spd, spe, accuracy, evasion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boost {
    Atk = 0,
    Def = 1,
    Spa = 2,
    Spd = 3,
    Spe = 4,
    Accuracy = 5,
    Evasion = 6,
}

impl Boost {
    pub const ALL: [Boost; 7] = [
        Boost::Atk,
        Boost::Def,
        Boost::Spa,
        Boost::Spd,
        Boost::Spe,
        Boost::Accuracy,
        Boost::Evasion,
    ];

    pub fn from_id(s: &str) -> Option<Boost> {
        Some(match s {
            "atk" => Boost::Atk,
            "def" => Boost::Def,
            "spa" => Boost::Spa,
            "spd" => Boost::Spd,
            "spe" => Boost::Spe,
            "accuracy" => Boost::Accuracy,
            "evasion" => Boost::Evasion,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        ["atk", "def", "spa", "spd", "spe", "accuracy", "evasion"][self as usize]
    }

    pub fn of_stat(stat: Stat) -> Boost {
        match stat {
            Stat::Atk => Boost::Atk,
            Stat::Def => Boost::Def,
            Stat::Spa => Boost::Spa,
            Stat::Spd => Boost::Spd,
            Stat::Spe => Boost::Spe,
            Stat::Hp => panic!("hp has no boost"),
        }
    }
}

/// A set of boost changes, as in a move's `boosts` or `self.boosts`.
pub type Boosts = [i8; 7];

/// Stat indices in `Boost` order.
pub const BOOST_ORDER: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];

/// Boosts from move data with the order the data lists them in, which is
/// the order Showdown's `boost` applies and announces them (Shell Smash
/// lowers Defense first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MoveBoosts {
    pub vals: Boosts,
    /// Stat indices: the listed ones in the data's order, then the rest.
    pub order: [u8; 7],
}

impl Default for MoveBoosts {
    fn default() -> MoveBoosts {
        MoveBoosts {
            vals: [0; 7],
            order: BOOST_ORDER,
        }
    }
}

impl std::ops::Deref for MoveBoosts {
    type Target = Boosts;
    fn deref(&self) -> &Boosts {
        &self.vals
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Status {
    #[default]
    None,
    Brn,
    Par,
    Psn,
    Tox,
    Slp,
    Frz,
    /// Set on fainted active Pokémon at the end of a turn (Showdown `checkFainted`).
    Fnt,
}

impl Status {
    pub fn from_id(s: &str) -> Option<Status> {
        Some(match s {
            "" => Status::None,
            "brn" => Status::Brn,
            "par" => Status::Par,
            "psn" => Status::Psn,
            "tox" => Status::Tox,
            "slp" => Status::Slp,
            "frz" => Status::Frz,
            "fnt" => Status::Fnt,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Status::Fnt => "fnt",
            Status::None => "",
            Status::Brn => "brn",
            Status::Par => "par",
            Status::Psn => "psn",
            Status::Tox => "tox",
            Status::Slp => "slp",
            Status::Frz => "frz",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Physical,
    Special,
    Status,
}

/// Showdown move target types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Normal,
    Any,
    AdjacentAlly,
    AdjacentAllyOrSelf,
    AdjacentFoe,
    AllAdjacent,
    AllAdjacentFoes,
    Allies,
    AllySide,
    AllyTeam,
    All,
    FoeSide,
    RandomNormal,
    Scripted,
    SelfTarget,
}

impl Target {
    pub fn from_id(s: &str) -> Option<Target> {
        Some(match s {
            "normal" => Target::Normal,
            "any" => Target::Any,
            "adjacentAlly" => Target::AdjacentAlly,
            "adjacentAllyOrSelf" => Target::AdjacentAllyOrSelf,
            "adjacentFoe" => Target::AdjacentFoe,
            "allAdjacent" => Target::AllAdjacent,
            "allAdjacentFoes" => Target::AllAdjacentFoes,
            "allies" => Target::Allies,
            "allySide" => Target::AllySide,
            "allyTeam" => Target::AllyTeam,
            "all" => Target::All,
            "foeSide" => Target::FoeSide,
            "randomNormal" => Target::RandomNormal,
            "scripted" => Target::Scripted,
            "self" => Target::SelfTarget,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Target::Normal => "normal",
            Target::Any => "any",
            Target::AdjacentAlly => "adjacentAlly",
            Target::AdjacentAllyOrSelf => "adjacentAllyOrSelf",
            Target::AdjacentFoe => "adjacentFoe",
            Target::AllAdjacent => "allAdjacent",
            Target::AllAdjacentFoes => "allAdjacentFoes",
            Target::Allies => "allies",
            Target::AllySide => "allySide",
            Target::AllyTeam => "allyTeam",
            Target::All => "all",
            Target::FoeSide => "foeSide",
            Target::RandomNormal => "randomNormal",
            Target::Scripted => "scripted",
            Target::SelfTarget => "self",
        }
    }

    /// Whether choosing this move in doubles requires a target location.
    pub fn needs_choice(self) -> bool {
        matches!(
            self,
            Target::Normal
                | Target::Any
                | Target::AdjacentAlly
                | Target::AdjacentAllyOrSelf
                | Target::AdjacentFoe
        )
    }
}

/// A Showdown move flag (`move.flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveFlag {
    AllyAnim,
    Bite,
    Bullet,
    BypassSub,
    CantUseTwice,
    Charge,
    Contact,
    Dance,
    Defrost,
    Distance,
    FailCopycat,
    FailEncore,
    FailInstruct,
    FailMeFirst,
    FailMimic,
    FutureMove,
    Gravity,
    Heal,
    Metronome,
    Minimize,
    Mirror,
    MustPressure,
    NoAssist,
    NonSky,
    NoParentalBond,
    NoSketch,
    NoSleepTalk,
    PledgeCombo,
    Powder,
    Protect,
    Pulse,
    Punch,
    Recharge,
    Reflectable,
    Slicing,
    Snatch,
    Sound,
    Wind,
}

impl MoveFlag {
    pub const ALL: [MoveFlag; 38] = [
        MoveFlag::AllyAnim,
        MoveFlag::Bite,
        MoveFlag::Bullet,
        MoveFlag::BypassSub,
        MoveFlag::CantUseTwice,
        MoveFlag::Charge,
        MoveFlag::Contact,
        MoveFlag::Dance,
        MoveFlag::Defrost,
        MoveFlag::Distance,
        MoveFlag::FailCopycat,
        MoveFlag::FailEncore,
        MoveFlag::FailInstruct,
        MoveFlag::FailMeFirst,
        MoveFlag::FailMimic,
        MoveFlag::FutureMove,
        MoveFlag::Gravity,
        MoveFlag::Heal,
        MoveFlag::Metronome,
        MoveFlag::Minimize,
        MoveFlag::Mirror,
        MoveFlag::MustPressure,
        MoveFlag::NoAssist,
        MoveFlag::NonSky,
        MoveFlag::NoParentalBond,
        MoveFlag::NoSketch,
        MoveFlag::NoSleepTalk,
        MoveFlag::PledgeCombo,
        MoveFlag::Powder,
        MoveFlag::Protect,
        MoveFlag::Pulse,
        MoveFlag::Punch,
        MoveFlag::Recharge,
        MoveFlag::Reflectable,
        MoveFlag::Slicing,
        MoveFlag::Snatch,
        MoveFlag::Sound,
        MoveFlag::Wind,
    ];

    /// The flag's id in Showdown's data.
    pub fn id(self) -> &'static str {
        match self {
            MoveFlag::AllyAnim => "allyanim",
            MoveFlag::Bite => "bite",
            MoveFlag::Bullet => "bullet",
            MoveFlag::BypassSub => "bypasssub",
            MoveFlag::CantUseTwice => "cantusetwice",
            MoveFlag::Charge => "charge",
            MoveFlag::Contact => "contact",
            MoveFlag::Dance => "dance",
            MoveFlag::Defrost => "defrost",
            MoveFlag::Distance => "distance",
            MoveFlag::FailCopycat => "failcopycat",
            MoveFlag::FailEncore => "failencore",
            MoveFlag::FailInstruct => "failinstruct",
            MoveFlag::FailMeFirst => "failmefirst",
            MoveFlag::FailMimic => "failmimic",
            MoveFlag::FutureMove => "futuremove",
            MoveFlag::Gravity => "gravity",
            MoveFlag::Heal => "heal",
            MoveFlag::Metronome => "metronome",
            MoveFlag::Minimize => "minimize",
            MoveFlag::Mirror => "mirror",
            MoveFlag::MustPressure => "mustpressure",
            MoveFlag::NoAssist => "noassist",
            MoveFlag::NonSky => "nonsky",
            MoveFlag::NoParentalBond => "noparentalbond",
            MoveFlag::NoSketch => "nosketch",
            MoveFlag::NoSleepTalk => "nosleeptalk",
            MoveFlag::PledgeCombo => "pledgecombo",
            MoveFlag::Powder => "powder",
            MoveFlag::Protect => "protect",
            MoveFlag::Pulse => "pulse",
            MoveFlag::Punch => "punch",
            MoveFlag::Recharge => "recharge",
            MoveFlag::Reflectable => "reflectable",
            MoveFlag::Slicing => "slicing",
            MoveFlag::Snatch => "snatch",
            MoveFlag::Sound => "sound",
            MoveFlag::Wind => "wind",
        }
    }

    pub fn from_id(id: &str) -> Option<MoveFlag> {
        MoveFlag::ALL.into_iter().find(|f| f.id() == id)
    }
}

/// A move's flags, one bit per [`MoveFlag`]: testing one is a mask, not a
/// search through names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveFlags(u64);

impl MoveFlags {
    pub const EMPTY: MoveFlags = MoveFlags(0);

    #[inline]
    pub fn contains(self, flag: MoveFlag) -> bool {
        self.0 & (1 << flag as u32) != 0
    }

    pub fn insert(&mut self, flag: MoveFlag) {
        self.0 |= 1 << flag as u32;
    }

    pub fn remove(&mut self, flag: MoveFlag) {
        self.0 &= !(1 << flag as u32);
    }

    /// The flags set, in [`MoveFlag::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = MoveFlag> {
        MoveFlag::ALL.into_iter().filter(move |&f| self.contains(f))
    }
}

impl FromIterator<MoveFlag> for MoveFlags {
    fn from_iter<I: IntoIterator<Item = MoveFlag>>(flags: I) -> MoveFlags {
        let mut set = MoveFlags::EMPTY;
        for f in flags {
            set.insert(f);
        }
        set
    }
}

/// Support level of a move, item or ability in this engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    /// Fully described by data fields the engine implements.
    Generic,
    /// Has hand-written code in the engine, ported from Showdown.
    Implemented,
    /// Not simulated correctly; teams using it are unsupported.
    Unsupported(String),
}

impl fmt::Display for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Support::Generic => write!(f, "generic"),
            Support::Implemented => write!(f, "implemented"),
            Support::Unsupported(r) => write!(f, "unsupported ({r})"),
        }
    }
}

/// Showdown's toID.
pub fn to_id(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod move_flag_tests {
    use super::*;

    #[test]
    fn move_flags_fit_and_round_trip() {
        assert!(MoveFlag::ALL.len() <= u64::BITS as usize);
        for (i, f) in MoveFlag::ALL.into_iter().enumerate() {
            assert_eq!(f as usize, i);
            assert_eq!(MoveFlag::from_id(f.id()), Some(f));
        }
        let mut set: MoveFlags = [MoveFlag::Contact, MoveFlag::Sound].into_iter().collect();
        assert!(set.contains(MoveFlag::Contact) && !set.contains(MoveFlag::Punch));
        set.remove(MoveFlag::Contact);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![MoveFlag::Sound]);
    }
}
