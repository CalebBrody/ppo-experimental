//! Side conditions: adding them, and the entry hazards' switch-in effects.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::state::*;

impl Battle {
    /// Showdown `side.addSideCondition`. Fails if already active.
    pub(crate) fn add_side_condition(&mut self, s: usize, c: SideCond, source: MonRef) -> bool {
        if self.sides[s].conds.get(c) > 0 {
            return false;
        }
        let turns = match c {
            SideCond::Tailwind => self.cond_u8(c.id(), "duration", 4),
            SideCond::WideGuard | SideCond::QuickGuard => self.cond_u8(c.id(), "duration", 1),
            _ if self.has_item(source, "lightclay") => 8,
            _ => self.cond_u8(c.id(), "duration", 5),
        };
        *self.sides[s].conds.get_mut(c) = turns;
        self.sides[s].conds.stamp(c);
        if c.is_guard() {
            let who = self.ident(source);
            log_line!(self, "|-singleturn|{who}|{}", c.log_name());
        } else {
            let side = self.side_ident(s);
            log_line!(self, "|-sidestart|{side}|{}", c.log_name());
        }
        true
    }

    /// Stealth Rock on a side; fails if it is there already.
    pub(crate) fn add_stealth_rock(&mut self, side: usize) -> bool {
        let c = &mut self.sides[side].conds;
        if c.stealth_rock {
            return false;
        }
        c.stealth_rock = true;
        c.next_added += 1;
        c.stealth_rock_added = c.next_added;
        let si = self.side_ident(side);
        log_line!(self, "|-sidestart|{si}|move: Stealth Rock");
        true
    }

    /// Stealth Rock's `onSwitchIn`: 1/8 of max HP, doubled or halved per
    /// step of Rock-type effectiveness; Heavy-Duty Boots prevent it.
    pub(crate) fn stealth_rock_switch_in(&mut self, r: MonRef) {
        let boots = self
            .mon(r)
            .item
            .is_some_and(|i| self.dex.item(i).id == "heavydutyboots");
        if !self.sides[r.0].conds.stealth_rock || boots {
            return;
        }
        let rock = self.dex.ids.types.rock.unwrap_or(0);
        let ty = self.types_of(r);
        let steps: i32 = ty
            .iter()
            .map(|&t| self.dex.effectiveness_exp(rock, t))
            .sum();
        let steps = steps.clamp(-6, 6);
        let maxhp = self.mon(r).maxhp as u32;
        let amount = if steps >= 0 {
            (maxhp << steps) / 8
        } else {
            maxhp / (8 << (-steps))
        };
        self.apply_damage(r, amount.max(1), "|[from] Stealth Rock");
    }

    /// Add a layer of Toxic Spikes to a side (at most two).
    pub(crate) fn add_toxic_spikes(&mut self, side: usize) -> bool {
        if self.sides[side].conds.toxic_spikes >= 2 {
            return false;
        }
        if self.sides[side].conds.toxic_spikes == 0 {
            let c = &mut self.sides[side].conds;
            c.next_added += 1;
            c.toxic_spikes_added = c.next_added;
        }
        self.sides[side].conds.toxic_spikes += 1;
        let si = self.side_ident(side);
        log_line!(self, "|-sidestart|{si}|move: Toxic Spikes");
        true
    }

    /// Toxic Spikes' `onSwitchIn`: a grounded Poison type absorbs them; a
    /// Steel type is unaffected; others are poisoned (badly with two layers).
    pub(crate) fn toxic_spikes_switch_in(&mut self, r: MonRef) {
        let layers = self.sides[r.0].conds.toxic_spikes;
        if layers == 0 || !self.is_grounded(r) {
            return;
        }
        let is = |b: &Battle, name: &str| {
            b.dex
                .type_id(name)
                .map(|t| b.has_type_of(r, t))
                .unwrap_or(false)
        };
        if is(self, "Poison") {
            self.sides[r.0].conds.toxic_spikes = 0;
            let (si, ri) = (self.side_ident(r.0), self.ident(r));
            log_line!(self, "|-sideend|{si}|move: Toxic Spikes|[of] {ri}");
        } else if !is(self, "Steel")
            && !self
                .mon(r)
                .item
                .is_some_and(|i| self.dex.item(i).id == "heavydutyboots")
        {
            let st = if layers >= 2 {
                Status::Tox
            } else {
                Status::Psn
            };
            // Showdown's source is the foe side's first active Pokémon.
            let src = self.active(1 - r.0, 0).unwrap_or(r);
            self.try_set_status(r, st, src, crate::kinds::EffectRef::ToxicSpikes, false);
        }
    }

    /// The side's hazards for a Pokémon switching in, with
    /// their creation stamps.
    pub(crate) fn entry_hazards(&self, r: MonRef) -> Small<(AHazard, u16)> {
        let a = self.side_extras[r.0];
        let mut out = Small::new();
        if a.spikes > 0 {
            out.push((AHazard::Spikes, a.spikes_added));
        }
        if a.sticky_web {
            out.push((AHazard::StickyWeb, a.sticky_web_added));
        }
        out
    }

    /// Spikes' and Sticky Web's `onSwitchIn`: nothing for a Pokémon in the
    /// air or in Heavy-Duty Boots. Spikes take 1/8, 1/6 or 1/4 of max HP by
    /// layers; Sticky Web lowers Speed, its source the foe side's first
    /// active Pokémon.
    pub(crate) fn hazard_on_switch_in(&mut self, r: MonRef, h: AHazard) {
        let boots = self
            .mon(r)
            .item
            .is_some_and(|i| self.dex.item(i).id == "heavydutyboots");
        if !self.is_grounded(r) || boots {
            return;
        }
        match h {
            AHazard::Spikes => {
                let layers = self.side_extras[r.0].spikes as u32;
                if layers == 0 {
                    return;
                }
                let amount = [0, 3, 4, 6][layers as usize] * self.mon(r).maxhp as u32 / 24;
                self.apply_damage(r, amount.max(1), "|[from] Spikes");
            }
            AHazard::StickyWeb => {
                if !self.side_extras[r.0].sticky_web {
                    return;
                }
                let ri = self.ident(r);
                log_line!(self, "|-activate|{ri}|move: Sticky Web");
                let mut b = [0i8; 7];
                b[Boost::Spe as usize] = -1;
                let src = self.active(1 - r.0, 0);
                let effect = match self.dex.ids.moves.sticky_web {
                    Some(m) => crate::kinds::EffectRef::Move(m),
                    None => crate::kinds::EffectRef::Other,
                };
                self.boost(r, &b, src, effect, false, false);
            }
        }
    }

    /// Whether Safeguard on the target's side stops something `src` does to
    /// it: not its own doing, and not an Infiltrator's move against a foe.
    pub(crate) fn safeguard_applies(&self, t: MonRef, src: MonRef, from_move: bool) -> bool {
        self.side_extras[t.0].safeguard > 0
            && t != src
            && !(from_move && self.infiltrates(src) && src.0 != t.0)
    }

    /// Safeguard's `onSetStatus` (after the abilities, before the terrain):
    /// `null` for a status from another Pokémon, Yawn's sleep aside;
    /// announced against Synchronize and a status move.
    pub(crate) fn safeguard_on_set_status(
        &mut self,
        t: MonRef,
        src: MonRef,
        effect: crate::kinds::EffectRef,
        status_move: bool,
    ) -> bool {
        use crate::kinds::EffectRef;
        if effect == EffectRef::Yawn
            || !self.safeguard_applies(t, src, matches!(effect, EffectRef::Move(_)))
        {
            return false;
        }
        // Baneful Bunker's poison is cast with the move itself as the effect
        // (a Move without secondaries), so it announces too.
        let bunker =
            matches!(effect, EffectRef::Move(m) if self.dex.ids.moves.baneful_bunker == Some(m));
        if status_move || bunker {
            let ti = self.ident(t);
            log_line!(self, "|-activate|{ti}|move: Safeguard");
        }
        true
    }

    /// Safeguard's `onTryAddVolatile` for confusion and Yawn from another
    /// Pokémon's move (the one in use, unless `src` is given): `null`,
    /// announced against a status move.
    pub(crate) fn safeguard_on_try_add_volatile(
        &mut self,
        t: MonRef,
        src: Option<MonRef>,
        status_move: bool,
    ) -> bool {
        let Some(src) = src.or(self.active_move.mover) else {
            return false;
        };
        if !self.safeguard_applies(t, src, true) {
            return false;
        }
        if status_move {
            let ti = self.ident(t);
            log_line!(self, "|-activate|{ti}|move: Safeguard");
        }
        true
    }
}
