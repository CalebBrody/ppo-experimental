//! `TrapPokemon` and `MaybeTrapPokemon`: trapping effects and their handlers.

use crate::abilities::{compare_mod, ModHandler};
use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

impl Battle {
    /// The end-of-turn trap pass (Showdown `endTurn`): `TrapPokemon`, then,
    /// unless the type is immune to trapping (Ghost), `MaybeTrapPokemon`.
    /// An adjacent foe's Shadow Tag traps the Pokémon in secret and marks it
    /// maybe trapped, unless it has Shadow Tag too. A fainted Pokémon has no
    /// handlers. Each sort draws for equal handlers (`draw` false: an
    /// imported state, recomputed without the generator).
    ///
    /// The handlers are collected in `findEventHandlers`' order: the
    /// Pokémon's own, then its foes' (`onFoeTrapPokemon`), then the field's.
    /// Shed Shell's and Run Away's handlers (priority -10) are not sorted:
    /// they sort last on their own, so they never tie.
    pub(crate) fn run_trap_pokemon(&mut self, r: MonRef, draw: bool) {
        let m = self.mon_mut(r);
        m.trapped = Trapped::No;
        m.maybe_trapped = false;
        if self.mon(r).fainted {
            return;
        }
        let mut tags: Small<ModHandler> = self
            .foes_of(r)
            .into_iter()
            .filter(|&f| self.has_ability(f, AbilityKind::ShadowTag))
            .map(|f| ModHandler::new(0, self.mon(f).speed, 7))
            .collect();
        // The Pokémon's own volatiles (subOrder 2): partial trapping,
        // Ingrain, `trapped`, No Retreat and Octolock. Their keys are all
        // equal, so their order doesn't change the sort's draws.
        let m = self.mon(r);
        let v = &m.vol;
        let own = [
            v.partial_trap > 0,
            v.ingrain,
            v.trapped.is_some(),
            v.no_retreat,
            v.octolock.is_some(),
        ];
        let mut all: Small<ModHandler> = smallvec::smallvec![ModHandler::new(0, m.speed, 2); own.iter().filter(|&&on| on).count()];
        all.extend(tags.iter().copied());
        // Fairy Lock, a pseudo-weather: no speed, subOrder 2.
        if self.field.fairy_lock > 0 {
            all.push(ModHandler::new(0, 0, 2));
        }
        if draw && all.len() > 1 {
            self.speed_sort_by(&mut all, compare_mod);
        }
        let ty = self.types_of(r);
        let immune = ty
            .iter()
            .any(|&t| self.dex.type_immune_to_effect(t, "trapped"));
        let own_tag = self.has_ability(r, AbilityKind::ShadowTag);
        if !tags.is_empty() && !own_tag && !immune {
            self.mon_mut(r).trapped = Trapped::Hidden;
        }
        // The effects that trap (`tryTrap()`, not hidden).
        let v = &self.mon(r).vol;
        let traps = (v.partial_trap > 0
            && v.partial_trap_source.is_some_and(|s| self.source_active(s)))
            || v.ingrain
            || v.trapped.is_some()
            || v.no_retreat
            || v.octolock.is_some_and(|s| self.mon(s).is_active)
            || self.field.fairy_lock > 0;
        if !immune && traps {
            self.mon_mut(r).trapped = Trapped::Yes;
        }
        if !immune {
            if draw && tags.len() > 1 {
                self.speed_sort_by(&mut tags, compare_mod);
            }
            if !tags.is_empty() && !own_tag {
                self.mon_mut(r).maybe_trapped = true;
            }
        }
        // Shed Shell and Run Away (Champions): `onTrapPokemon` and
        // `onMaybeTrapPokemon` (priority -10) clear `trapped` and
        // `maybeTrapped` after everything but the Commander volatiles (not in
        // the engine) set them.
        if self.item_kind(r) == Some(ItemKind::ShedShell)
            || self.has_ability(r, AbilityKind::RunAway)
        {
            let m = self.mon_mut(r);
            m.trapped = Trapped::No;
            m.maybe_trapped = false;
        }
    }

    /// `addVolatile('trapped', source, move, 'trapper')`: fails for a
    /// fainted target or source, one already trapped and a Ghost (`trapped`
    /// immunity, silently). Then TryAddVolatile, the start, and the source's
    /// linked `trapper` (with its own TryAddVolatile when it is new).
    pub(crate) fn add_trapped(&mut self, t: MonRef, src: MonRef) -> bool {
        if self.mon(t).hp == 0 || self.mon(src).hp == 0 || self.mon(t).vol.trapped.is_some() {
            return false;
        }
        if self
            .types_of(t)
            .iter()
            .any(|&x| self.dex.type_immune_to_effect(x, "trapped"))
        {
            return false;
        }
        self.try_add_volatile_sort(t);
        self.mon_mut(t).vol.trapped = Some(src);
        let ti = self.ident(t);
        log_line!(self, "|-activate|{ti}|trapped");
        if self.mon(src).vol.trapper == 0 {
            self.try_add_volatile_sort(src);
        }
        self.mon_mut(src).vol.trapper |= Self::trap_bit(t);
        true
    }

    /// The bit for `r` in a `trapper` set.
    pub(crate) fn trap_bit(r: MonRef) -> u16 {
        1 << (r.0 * 8 + r.1)
    }

    /// `removeLinkedVolatiles` from `clearVolatile`: a trapped Pokémon
    /// leaves its trapper's set (the set ends when empty), and a trapper's
    /// targets lose `trapped`. Both end silently.
    pub(crate) fn unlink_traps(&mut self, r: MonRef) {
        let v = self.mon(r).vol;
        if let Some(src) = v.trapped {
            self.mon_mut(src).vol.trapper &= !Self::trap_bit(r);
        }
        for s in 0..2 {
            for i in 0..self.sides[s].mons.len() {
                if v.trapper & Self::trap_bit((s, i)) != 0
                    && self.sides[s].mons[i].vol.trapped == Some(r)
                {
                    self.sides[s].mons[i].vol.trapped = None;
                }
            }
        }
        let v = &mut self.mon_mut(r).vol;
        v.trapped = None;
        v.trapper = 0;
    }

    /// `isGrounded`: Gravity, then Ingrain roots the Pokémon (after Gravity, before
    /// everything else).
    pub(crate) fn forced_grounded(&self, r: MonRef) -> bool {
        self.field.gravity > 0 || self.mon(r).vol.ingrain || self.mon(r).vol.smack_down
    }
}
