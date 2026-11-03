//! Questions about Pokémon: types, grounding, items and condition counters.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::{AbilityKind, ItemKind};

/// Showdown `isGrounded`: true, false, or null (Levitate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grounded {
    Yes,
    Airborne,
    Levitate,
}

impl Battle {
    /// Showdown `getTypes()`: the types of `base_types_of`, then the type
    /// Forest's Curse or Trick-or-Treat added (`addedType`, `NO_TYPE` without one).
    pub(crate) fn types_of(&self, r: MonRef) -> [TypeId; 3] {
        let [a, b] = self.base_types_of(r);
        [a, b, self.mon(r).added_type]
    }

    /// Showdown `getTypes(true)`: Roost removes the Flying type for the turn,
    /// and a Pokémon left without a type is Normal.
    pub(crate) fn base_types_of(&self, r: MonRef) -> [TypeId; 2] {
        let m = self.mon(r);
        if !m.vol.roost {
            return m.types;
        }
        let flying = self.dex.ids.types.flying;
        let left: Vec<TypeId> = m
            .types
            .iter()
            .copied()
            .filter(|&t| t != NO_TYPE && Some(t) != flying)
            .collect();
        match left.as_slice() {
            [] => [self.dex.ids.types.normal.unwrap_or(m.types[0]), NO_TYPE],
            [a] => [*a, NO_TYPE],
            [a, b, ..] => [*a, *b],
        }
    }

    pub(crate) fn has_type_of(&self, r: MonRef, t: TypeId) -> bool {
        t != NO_TYPE && self.types_of(r).contains(&t)
    }

    pub fn grounded(&self, r: MonRef) -> Grounded {
        let m = self.mon(r);
        if self.forced_grounded(r) || self.item_kind(r) == Some(ItemKind::IronBall) {
            return Grounded::Yes;
        }
        if let Some(t) = self.dex.ids.types.flying {
            if self.has_type_of(r, t) {
                return Grounded::Airborne;
            }
        }
        if matches!(
            self.dex.ability_kind[m.ability as usize],
            AbilityKind::Levitate | AbilityKind::Eelevate
        ) && !m.revived
            && !self.ability_suppressed(r)
            && !self.ignoring_ability(r)
        {
            return Grounded::Levitate;
        }
        // Magnet Rise: after Levitate, before the Air Balloon.
        if m.vol.magnet_rise > 0 {
            return Grounded::Airborne;
        }
        if self.item_kind(r) == Some(ItemKind::AirBalloon) {
            return Grounded::Airborne;
        }
        Grounded::Yes
    }

    pub fn is_grounded(&self, r: MonRef) -> bool {
        self.grounded(r) == Grounded::Yes
    }

    pub(crate) fn has_item(&self, r: MonRef, id: &str) -> bool {
        self.mon(r)
            .item
            .map(|i| self.dex.item(i).id == id)
            .unwrap_or(false)
            && !self.ignoring_item(r)
    }

    pub(crate) fn cond_u8(&self, id: &str, field: &str, default: u8) -> u8 {
        self.dex
            .cond_num(id, field)
            .map(|x| x as u8)
            .unwrap_or(default)
    }

    pub(crate) fn cond_u32(&self, id: &str, field: &str, default: u32) -> u32 {
        self.dex
            .cond_num(id, field)
            .map(|x| x as u32)
            .unwrap_or(default)
    }

    pub(crate) fn side_ident(&self, s: usize) -> String {
        format!("{}: {}", self.sides[s].id(), self.sides[s].name)
    }
}
