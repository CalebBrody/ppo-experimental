//! Items with hand-ported behaviour: the `ItemKind` registry.

use crate::dex::types::*;
use crate::state::Terrain;

/// What a status-curing berry cures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CureBerry {
    Par,
    Slp,
    /// Poison and bad poison.
    Psn,
    Brn,
    Frz,
    Confusion,
    /// Lum Berry: any status and confusion.
    Lum,
}

impl CureBerry {
    pub fn cures(self, st: crate::dex::types::Status) -> bool {
        use crate::dex::types::Status;
        match self {
            CureBerry::Par => st == Status::Par,
            CureBerry::Slp => st == Status::Slp,
            CureBerry::Psn => st == Status::Psn || st == Status::Tox,
            CureBerry::Brn => st == Status::Brn,
            CureBerry::Frz => st == Status::Frz,
            CureBerry::Confusion => false,
            CureBerry::Lum => st != Status::None && st != Status::Fnt,
        }
    }
    pub fn cures_confusion(self) -> bool {
        matches!(self, CureBerry::Confusion | CureBerry::Lum)
    }
}

/// Items with hand-ported behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// Mega Stones: only enable Mega Evolution.
    MegaStone,
    /// Weather rocks, Light Clay, Terrain Extender: only extend durations.
    DurationExtender,
    /// Base power x4915/4096 for moves of one type (Charcoal, Mystic Water...).
    TypeBoost(TypeId),
    /// Base power x4505/4096 for physical (Muscle Band) or special (Wise Glasses) moves.
    CategoryBoost(Category),
    /// Halves a super-effective hit of one type, once (Chilan Berry: any Normal hit).
    ResistBerry(TypeId),
    CureBerry(CureBerry),
    SitrusBerry,
    OranBerry,
    Leftovers,
    ShellBell,
    BigRoot,
    LifeOrb,
    ExpertBelt,
    FocusSash,
    FocusBand,
    /// Terrain seeds: +1 Def (Grassy, Electric) or SpD (Psychic, Misty) in their terrain.
    Seed(Terrain),
    WhiteHerb,
    ChoiceScarf,
    RockyHelmet,
    AirBalloon,
    ScopeLens,
    Leek,
    LightBall,
    KingsRock,
    BrightPowder,
    WideLens,
    ZoomLens,
    EjectButton,
    /// Partial trapping from its holder does 1/6 instead of 1/8.
    BindingBand,
    /// Used up to end Attract, Taunt, Encore, Torment, Disable and Heal Block.
    MentalHerb,
    /// Eaten to restore 10 PP to a move out of PP.
    LeppaBerry,
    /// Grounds the holder, halves its Speed; Ground moves hit it neutrally
    /// if it is Flying-type.
    IronBall,
    /// Gems: used up to power up the holder's first move of their type.
    Gem(TypeId),
    /// 20% to move first within its priority bracket.
    QuickClaw,
    /// Metronome: x1.2 per consecutive use of a move, up to x2.
    Metronome,
    /// Forces out the attacker that hit the holder.
    RedCard,
    /// Lets the holder switch out whatever traps it.
    ShedShell,
    Unsupported,
}

/// Type-boosting items and the type they boost.
const TYPE_BOOST_ITEMS: &[(&str, &str)] = &[
    ("blackbelt", "Fighting"),
    ("blackglasses", "Dark"),
    ("charcoal", "Fire"),
    ("dragonfang", "Dragon"),
    ("fairyfeather", "Fairy"),
    ("hardstone", "Rock"),
    ("magnet", "Electric"),
    ("metalcoat", "Steel"),
    ("miracleseed", "Grass"),
    ("mysticwater", "Water"),
    ("nevermeltice", "Ice"),
    ("poisonbarb", "Poison"),
    ("sharpbeak", "Flying"),
    ("silkscarf", "Normal"),
    ("silverpowder", "Bug"),
    ("softsand", "Ground"),
    ("spelltag", "Ghost"),
    ("twistedspoon", "Psychic"),
];

/// Damage-halving berries and the type they resist.
const RESIST_BERRIES: &[(&str, &str)] = &[
    ("babiriberry", "Steel"),
    ("chartiberry", "Rock"),
    ("chilanberry", "Normal"),
    ("chopleberry", "Fighting"),
    ("cobaberry", "Flying"),
    ("colburberry", "Dark"),
    ("habanberry", "Dragon"),
    ("kasibberry", "Ghost"),
    ("kebiaberry", "Poison"),
    ("occaberry", "Fire"),
    ("passhoberry", "Water"),
    ("payapaberry", "Psychic"),
    ("rindoberry", "Grass"),
    ("roseliberry", "Fairy"),
    ("shucaberry", "Ground"),
    ("tangaberry", "Bug"),
    ("wacanberry", "Electric"),
    ("yacheberry", "Ice"),
];

impl ItemKind {
    /// `type_id` resolves a type name in the format's type chart.
    pub fn from_id(
        id: &str,
        is_mega_stone: bool,
        type_id: impl Fn(&str) -> Option<TypeId>,
    ) -> ItemKind {
        if is_mega_stone {
            return ItemKind::MegaStone;
        }
        if let Some((_, t)) = TYPE_BOOST_ITEMS.iter().find(|(i, _)| *i == id) {
            return type_id(t)
                .map(ItemKind::TypeBoost)
                .unwrap_or(ItemKind::Unsupported);
        }
        if let Some((_, t)) = RESIST_BERRIES.iter().find(|(i, _)| *i == id) {
            return type_id(t)
                .map(ItemKind::ResistBerry)
                .unwrap_or(ItemKind::Unsupported);
        }
        match id {
            "damprock" | "heatrock" | "smoothrock" | "icyrock" | "lightclay"
            | "terrainextender" => ItemKind::DurationExtender,
            "muscleband" => ItemKind::CategoryBoost(Category::Physical),
            "wiseglasses" => ItemKind::CategoryBoost(Category::Special),
            "cheriberry" => ItemKind::CureBerry(CureBerry::Par),
            "chestoberry" => ItemKind::CureBerry(CureBerry::Slp),
            "pechaberry" => ItemKind::CureBerry(CureBerry::Psn),
            "rawstberry" => ItemKind::CureBerry(CureBerry::Brn),
            "aspearberry" => ItemKind::CureBerry(CureBerry::Frz),
            "persimberry" => ItemKind::CureBerry(CureBerry::Confusion),
            "lumberry" => ItemKind::CureBerry(CureBerry::Lum),
            "sitrusberry" => ItemKind::SitrusBerry,
            "oranberry" => ItemKind::OranBerry,
            "leftovers" => ItemKind::Leftovers,
            "shellbell" => ItemKind::ShellBell,
            "bigroot" => ItemKind::BigRoot,
            "lifeorb" => ItemKind::LifeOrb,
            "expertbelt" => ItemKind::ExpertBelt,
            "focussash" => ItemKind::FocusSash,
            "focusband" => ItemKind::FocusBand,
            "grassyseed" => ItemKind::Seed(Terrain::Grassy),
            "electricseed" => ItemKind::Seed(Terrain::Electric),
            "psychicseed" => ItemKind::Seed(Terrain::Psychic),
            "mistyseed" => ItemKind::Seed(Terrain::Misty),
            "whiteherb" => ItemKind::WhiteHerb,
            "choicescarf" => ItemKind::ChoiceScarf,
            "rockyhelmet" => ItemKind::RockyHelmet,
            "airballoon" => ItemKind::AirBalloon,
            "scopelens" => ItemKind::ScopeLens,
            "leek" => ItemKind::Leek,
            "lightball" => ItemKind::LightBall,
            "kingsrock" => ItemKind::KingsRock,
            "brightpowder" => ItemKind::BrightPowder,
            "widelens" => ItemKind::WideLens,
            "zoomlens" => ItemKind::ZoomLens,
            "ejectbutton" => ItemKind::EjectButton,
            "bindingband" => ItemKind::BindingBand,
            "mentalherb" => ItemKind::MentalHerb,
            "leppaberry" => ItemKind::LeppaBerry,
            "ironball" => ItemKind::IronBall,
            "normalgem" => type_id("Normal")
                .map(ItemKind::Gem)
                .unwrap_or(ItemKind::Unsupported),
            "quickclaw" => ItemKind::QuickClaw,
            "metronome" => ItemKind::Metronome,
            "redcard" => ItemKind::RedCard,
            "shedshell" => ItemKind::ShedShell,
            _ => ItemKind::Unsupported,
        }
    }

    pub fn is_berry(self) -> bool {
        matches!(
            self,
            ItemKind::ResistBerry(_)
                | ItemKind::CureBerry(_)
                | ItemKind::SitrusBerry
                | ItemKind::OranBerry
        )
    }

    pub fn is_choice(self) -> bool {
        self == ItemKind::ChoiceScarf
    }
}
