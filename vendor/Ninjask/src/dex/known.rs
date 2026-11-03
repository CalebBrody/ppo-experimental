//! Ids of the moves, types and abilities the mechanics refer to by name,
//! looked up once at load instead of by name during battle. Each is `None`
//! when the format's data lacks it, as the name lookup would return.

use crate::dex::types::{AbilityId, MoveId, TypeId};
use crate::dex::Dex;

/// Declares a table of named ids and how to resolve it from the dex.
macro_rules! id_table {
    ($(#[$doc:meta])* $table:ident: $id:ty, $lookup:ident { $($field:ident = $name:literal,)* }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default)]
        pub struct $table {
            $(pub $field: Option<$id>,)*
        }

        impl $table {
            fn resolve(dex: &Dex) -> $table {
                $table {
                    $($field: dex.$lookup($name),)*
                }
            }
        }
    };
}

id_table! {
    /// Moves named by the mechanics.
    MoveIds: MoveId, move_id {
        baneful_bunker = "banefulbunker",
        counter = "counter",
        curse = "curse",
        fell_stinger = "fellstinger",
        future_sight = "futuresight",
        last_resort = "lastresort",
        mirror_coat = "mirrorcoat",
        octolock = "octolock",
        round = "round",
        sticky_web = "stickyweb",
        struggle = "struggle",
        teatime = "teatime",
        uproar = "uproar",
    }
}

id_table! {
    /// Types named by the mechanics.
    TypeIds: TypeId, type_id {
        electric = "Electric",
        fire = "Fire",
        flying = "Flying",
        ground = "Ground",
        normal = "Normal",
        poison = "Poison",
        psychic = "Psychic",
        rock = "Rock",
        stellar = "Stellar",
        // Showdown's `???` type: typeless.
        typeless = "???",
        water = "Water",
    }
}

id_table! {
    /// Abilities named by the mechanics.
    AbilityIds: AbilityId, ability_id {
        insomnia = "insomnia",
        mummy = "mummy",
        poison_touch = "poisontouch",
        simple = "simple",
    }
}

/// Every id the mechanics name.
#[derive(Clone, Debug, Default)]
pub struct KnownIds {
    pub moves: MoveIds,
    pub types: TypeIds,
    pub abilities: AbilityIds,
}

impl KnownIds {
    pub(crate) fn resolve(dex: &Dex) -> KnownIds {
        KnownIds {
            moves: MoveIds::resolve(dex),
            types: TypeIds::resolve(dex),
            abilities: AbilityIds::resolve(dex),
        }
    }
}
