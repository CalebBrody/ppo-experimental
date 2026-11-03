//! Slot conditions: Wish, Healing Wish and Future Sight.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::ItemKind;

impl Battle {
    /// Wish's residual: the turn after it was made, it ends and heals the
    /// Pokémon in the slot (unless fainted) by half the wisher's max HP.
    pub(crate) fn wish_residual(&mut self, r: MonRef) -> Option<bool> {
        let wish = self.slot_conds[r.0][r.1].wish.clone()?;
        if (self.turn.wrapping_sub(1) & 255) <= wish.starting_turn {
            return Some(false);
        }
        self.slot_conds[r.0][r.1].wish = None;
        if !self.mon(r).fainted && self.mon(r).is_active {
            let amount = (wish.hp as u32).max(1);
            self.heal(
                r,
                amount,
                &format!("|[from] move: Wish|[wisher] {}", wish.wisher),
            );
        }
        Some(false)
    }

    /// The `futuremove` residual: from its ending turn on, it is removed
    /// and its `onEnd` makes the hit.
    pub(crate) fn future_move_residual(&mut self, r: MonRef) -> Option<bool> {
        let f = self.slot_conds[r.0][r.1].future?;
        if (self.turn.wrapping_sub(1) & 255) < f.ending_turn {
            return Some(false);
        }
        self.future_move_end(r, f.source);
        self.slot_conds[r.0][r.1].future = None;
        Some(false)
    }

    /// `futuremove`'s `onEnd`: Future Sight hits the Pokémon now in the
    /// slot, unless it fainted or is the user. The hit is a fresh move
    /// (`trySpreadMoveHit`): no PP, no `|move|` line, no Parental Bond or
    /// King's Rock, and a user that has left the field has lost its ability
    /// and item. Dark types are immune.
    pub(crate) fn future_move_end(&mut self, t: MonRef, src: MonRef) {
        if self.mon(t).fainted || t == src {
            return;
        }
        let ti = self.ident(t);
        log_line!(self, "|-end|{ti}|move: Future Sight");
        let Some(id) = self.dex.ids.moves.future_sight else {
            return;
        };
        let dex = self.dex.clone();
        let mv = dex.mv(id);
        let move_type = self.dex.ids.types.psychic.unwrap_or(mv.type_);
        // A user off the field has no ability or item (`ignoringAbility`).
        let on_field = self.mon(src).is_active && !self.mon(src).fainted && !self.mon(src).revived;
        let was_revived = self.mon(src).revived;
        if !on_field {
            self.mon_mut(src).revived = true;
        }
        self.ability_breaker = None;
        self.active_move.pbond = false;
        self.smart_target = false;
        self.active_move.used_type = Some((id, move_type));
        self.future_move_hit(src, t, mv, id, move_type);
        if !on_field {
            self.mon_mut(src).revived = was_revived;
        }
        // Life Orb's AfterMoveSecondarySelf, run on the item itself: it
        // costs the user HP whether or not the hit landed.
        if on_field
            && self.mon(src).hp > 0
            && self.item_kind(src) == Some(ItemKind::LifeOrb)
            && !self.mon(src).force_switch_flag
        {
            let amount = (self.mon(src).maxhp as u32 / 10).max(1);
            self.apply_damage(src, amount, "|[from] item: Life Orb");
        }
        self.check_win();
    }

    /// The hit steps for one target: invulnerability, TryHit abilities, type
    /// immunity, accuracy, then the damage.
    pub(crate) fn future_move_hit(
        &mut self,
        src: MonRef,
        t: MonRef,
        mv: &Move,
        id: crate::dex::types::MoveId,
        move_type: TypeId,
    ) {
        let (si, ti) = (self.ident(src), self.ident(t));
        // The misses go on the last move line, whichever move that was.
        let miss = |b: &mut Battle| {
            if let Some(line) = b.last_move_line {
                b.attr_move_line(line, "[miss]");
            }
        };
        if self.invulnerable(src, t, &mv.id) {
            miss(self);
            log_line!(self, "|-miss|{si}|{ti}");
            return;
        }
        if t != src && self.ability_try_hit(src, t, mv, move_type) {
            return;
        }
        if !self.ignores_type_immunity(src, move_type)
            && t != src
            && !self.run_immunity(t, move_type, true)
        {
            return;
        }
        self.event_sort(Event::ModifyAccuracy, t, Some(src), &[]);
        if !self.accuracy_hits(src, t, mv) {
            miss(self);
            log_line!(self, "|-miss|{si}|{ti}");
            return;
        }
        let kind = self.dex.move_kind[id as usize];
        self.damaging_move_hit(src, &[t], mv, kind, move_type, false, false, false);
    }

    /// Whether a slot condition reacts to this Pokémon switching in.
    pub(crate) fn slot_reacts_to_switch_in(&self, r: MonRef) -> bool {
        self.slot_conds[r.0][r.1].healing_wish.is_some()
    }

    /// `runEvent('Swap')` after `swapPosition` (Ally Switch): the slot's
    /// Healing Wish acts on the Pokémon moved into it.
    pub(crate) fn run_swap(&mut self, s: usize, slot: usize) {
        if self.active(s, slot).is_none() || self.slot_conds[s][slot].healing_wish.is_none() {
            return;
        }
        self.healing_wish_on_swap((s, slot));
        if self.slot_conds[s][slot].healing_wish.is_some() {
            self.slot_conds[s][slot].healing_wish_detached = true;
        }
    }

    /// Healing Wish (`onSwap`): a hurt or statused Pokémon coming in is
    /// fully healed and cured, and the wish is used up.
    pub(crate) fn healing_wish_on_swap(&mut self, r: MonRef) {
        let m = self.mon(r);
        if self.slot_conds[r.0][r.1].healing_wish.is_none() || m.fainted {
            return;
        }
        if m.hp >= m.maxhp && m.status == Status::None {
            return;
        }
        let m = self.mon_mut(r);
        m.hp = m.maxhp;
        m.status = Status::None;
        m.status_state = Status::None;
        m.status_time = 0;
        m.status_start_time = 0;
        m.toxic_stage = 0;
        let ri = self.ident(r);
        self.add_health(format_args!("|-heal|{ri}"), r, "|[from] move: Healing Wish");
        self.slot_conds[r.0][r.1].healing_wish = None;
        self.slot_conds[r.0][r.1].healing_wish_detached = false;
    }

    /// Healing Wish's `onSwitchIn` (`fieldEvent`): its `onSwap`, unless a
    /// swap's `runEvent` detached its state (it still joins the sort).
    pub(crate) fn healing_wish_on_switch_in(&mut self, r: MonRef) {
        if !self.slot_conds[r.0][r.1].healing_wish_detached {
            self.healing_wish_on_swap(r);
        }
    }
}
