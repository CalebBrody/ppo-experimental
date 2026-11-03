//! The registry that maps Showdown ids to the engine's hand-ported moves,
//! abilities and items.

mod abilities;
mod items;
mod moves;

pub use abilities::*;
pub use items::*;
pub use moves::*;

/// The effect behind a boost, damage or status: Showdown's `effect`, which
/// decides messages and which handlers react.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRef {
    Move(crate::dex::types::MoveId),
    Ability(crate::dex::types::AbilityId),
    Item(crate::dex::types::ItemId),
    /// Conditions (weather, statuses, side conditions...).
    Other,
    /// Toxic Spikes poisoning a Pokémon that switches in (Synchronize ignores it).
    ToxicSpikes,
    /// Yawn's drowsiness ending in sleep: the terrains announce blocking it.
    Yawn,
}
