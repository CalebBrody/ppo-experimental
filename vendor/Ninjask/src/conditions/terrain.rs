//! Terrain: setting and clearing.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::AbilityKind;
use crate::state::*;

impl Battle {
    /// Showdown `field.setTerrain`.
    pub(crate) fn set_terrain(
        &mut self,
        t: Terrain,
        source: MonRef,
        ability: Option<&str>,
    ) -> bool {
        if self.field.terrain == Some(t) {
            return false;
        }
        let turns = if self.has_item(source, "terrainextender") {
            8
        } else {
            self.cond_u8(t.id(), "duration", 5)
        };
        self.field.terrain = Some(t);
        self.field.terrain_turns = turns;
        match ability {
            Some(name) => {
                let who = self.ident(source);
                log_line!(
                    self,
                    "|-fieldstart|move: {}|[from] ability: {name}|[of] {who}",
                    t.name()
                );
            }
            None => log_line!(self, "|-fieldstart|move: {}", t.name()),
        }
        self.terrain_change();
        true
    }

    pub(crate) fn clear_terrain(&mut self) {
        if let Some(t) = self.field.terrain.take() {
            self.field.terrain_turns = 0;
            // Showdown's Misty Terrain end message has no "move:" prefix.
            if t == Terrain::Misty {
                self.add("|-fieldend|Misty Terrain");
            } else {
                log_line!(self, "|-fieldend|move: {}", t.name());
            }
            self.terrain_change();
        }
    }

    /// Mimicry's `onTerrainChange` (also its `onStart`): the holder takes the
    /// terrain's type, or its species' types without one.
    pub(crate) fn mimicry_terrain_change(&mut self, r: MonRef) {
        let ty = |b: &Battle, n: &str| b.dex.type_id(n).unwrap_or(NO_TYPE);
        let types: [TypeId; 2] = match self.field.terrain {
            Some(Terrain::Electric) => [ty(self, "Electric"), NO_TYPE],
            Some(Terrain::Grassy) => [ty(self, "Grass"), NO_TYPE],
            Some(Terrain::Misty) => [ty(self, "Fairy"), NO_TYPE],
            Some(Terrain::Psychic) => [ty(self, "Psychic"), NO_TYPE],
            None => self.dex.species(self.mon(r).base_species).types,
        };
        // setType: Arceus and Silvally keep their types.
        if self.types_of(r) == [types[0], types[1], NO_TYPE]
            || matches!(
                self.dex.species(self.mon(r).species).base_species.as_str(),
                "Arceus" | "Silvally"
            )
        {
            return;
        }
        self.set_type(r, types);
        let ri = self.ident(r);
        if self.field.terrain.is_some() || self.mon(r).transformed.is_some() {
            let list: Vec<&str> = types
                .iter()
                .filter(|&&t| t != NO_TYPE)
                .map(|&t| self.dex.type_names[t as usize].as_str())
                .collect();
            log_line!(
                self,
                "|-start|{ri}|typechange|{}|[from] ability: Mimicry",
                list.join("/")
            );
            if self.field.terrain.is_none() {
                self.add(
                    "|-hint|Transform Mimicry changes you to your original un-transformed types.",
                );
            }
        } else {
            log_line!(self, "|-activate|{ri}|ability: Mimicry");
            log_line!(self, "|-end|{ri}|typechange|[silent]");
        }
    }

    /// `TerrainChange` on one Pokémon: Mimicry.
    pub(crate) fn mimicry_on_terrain_change(&mut self, r: MonRef) {
        if self.mon(r).hp > 0 && self.has_ability(r, AbilityKind::Mimicry) {
            self.mimicry_terrain_change(r);
        }
    }
}
