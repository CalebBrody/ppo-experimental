//! Transform (the move) and Imposter (the ability): Showdown's
//! `Pokemon.transformInto`, and its undoing in `clearVolatile`.

use crate::battle::{Battle, MonRef};
use crate::state::MoveSlot;

/// What a transformed Pokémon was: Showdown keeps its own move slots
/// (`baseMoveSlots`, with their PP and stored disabled flags) and stats
/// (`baseStoredStats`) while `moveSlots` and `storedStats` are copies.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Transformed {
    pub moves: Vec<MoveSlot>,
    pub stats: [u16; 6],
}

impl Battle {
    /// Showdown `transformInto`. Fails (false, no message) when the target
    /// has fainted or is behind a substitute, or either is already
    /// transformed or under an Illusion. Otherwise `setSpecies(target's species)` gives the user
    /// the species, its types and stats from its own set (HP untouched) and
    /// stores that Speed; then the target's types, stats, move slots (5 PP
    /// each at most, virtual), boosts and `timesAttacked` are copied. The
    /// message comes before the ability: `setAbility(..., isTransform)` ends
    /// the old one and starts the new one only if it differs.
    pub(crate) fn transform_into(&mut self, r: MonRef, t: MonRef, imposter: bool) -> bool {
        let (m, tm) = (self.mon(r), self.mon(t));
        if tm.fainted
            || m.illusion.is_some()
            || tm.illusion.is_some()
            || tm.vol.substitute > 0
            || tm.transformed.is_some()
            || m.transformed.is_some()
        {
            return false;
        }
        // baseStoredStats: not what Speed Swap or a split left.
        let backup = Transformed {
            moves: m.moves.clone(),
            stats: self.stats_from_set(r),
        };
        let species = tm.species;
        self.set_species(r, species);
        let tm = self.mon(t).clone();
        let moves: Vec<MoveSlot> = tm
            .moves
            .iter()
            .map(|ms| {
                let pp = self.dex.mv(ms.id).pp.min(5);
                MoveSlot {
                    id: ms.id,
                    pp,
                    max_pp: pp,
                    disabled: false,
                    used: false,
                }
            })
            .collect();
        let m = self.mon_mut(r);
        m.transformed = Some(Box::new(backup));
        m.types = tm.types;
        m.added_type = tm.added_type;
        // storedStats, not the stored speed: setSpecies set that already.
        m.stats[1..].copy_from_slice(&tm.stats[1..]);
        m.moves = moves;
        m.times_attacked = tm.times_attacked;
        m.boosts = tm.boosts;
        self.clear_crit_volatiles(r, &tm.vol);
        let (ri, ti) = (self.ident(r), self.ident(t));
        if imposter {
            log_line!(self, "|-transform|{ri}|{ti}|[from] ability: Imposter");
        } else {
            log_line!(self, "|-transform|{ri}|{ti}");
        }
        let old = self.mon(r).ability;
        self.ability_on_end(r);
        self.mon_mut(r).ability = tm.ability;
        self.reset_ability_state(r);
        if old != tm.ability && !self.transform_ignores_ability(r) {
            self.run_start(r);
        }
        true
    }

    /// Imposter's `onSwitchIn`: transform into the foe directly across.
    pub(crate) fn imposter_switch_in(&mut self, r: MonRef) {
        let foe = 1 - r.0;
        let n = self.sides[foe].n_active;
        let Some(slot) = (n - 1).checked_sub(self.mon(r).position) else {
            return;
        };
        if slot < self.sides[foe].mons.len() {
            self.transform_into(r, (foe, slot), true);
        }
    }

    /// `clearVolatile` on switching out or fainting: the Pokémon's own move
    /// slots come back, and `setSpecies(baseSpecies)` its species and stats.
    pub(crate) fn clear_transform(&mut self, r: MonRef) {
        let Some(t) = self.mon_mut(r).transformed.take() else {
            return;
        };
        self.mon_mut(r).moves = t.moves;
        let base = self.mon(r).base_species;
        self.set_species(r, base);
    }

    /// `ignoringAbility`: a `notransform` ability (Disguise) does nothing
    /// while transformed.
    #[inline]
    pub(crate) fn transform_ignores_ability(&self, r: MonRef) -> bool {
        let m = self.mon(r);
        m.transformed.is_some() && self.dex.ability(m.ability).no_transform
    }

    /// The stats a request shows (`baseStoredStats`): the Pokémon's own,
    /// also while transformed, and without Speed Swap and the splits.
    pub fn base_stored_stats(&self, r: MonRef) -> [u16; 6] {
        match &self.mon(r).transformed {
            Some(t) => t.stats,
            None => self.stats_from_set(r),
        }
    }
}
