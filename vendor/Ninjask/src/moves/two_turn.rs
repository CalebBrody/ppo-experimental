//! Two-turn moves: locked moves, charge turns and semi-invulnerability.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::kinds::EffectRef;
use crate::kinds::MoveKind;
use crate::state::*;

/// A move a Pokémon must use this turn (Showdown `getLockedMove`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockedMove {
    /// `mustrecharge`, after Hyper Beam and other recharge moves.
    Recharge,
    /// The strike of a two-turn move.
    Move(MoveId),
}

impl Battle {
    /// Showdown `getLockedMove`: a move the Pokémon must use this turn.
    pub fn locked_move(&self, r: MonRef) -> Option<LockedMove> {
        let v = &self.mon(r).vol;
        if v.must_recharge > 0 {
            Some(LockedMove::Recharge)
        } else {
            v.two_turn
                .or(v.locked_move)
                .or_else(|| self.uproar_lock(r))
                .map(LockedMove::Move)
        }
    }

    /// The move slot and target location a locked Pokémon's action uses
    /// (Showdown `chooseMove`: the charging move's own target location, else
    /// `lastMoveTargetLoc`).
    pub(crate) fn locked_action(&self, r: MonRef, lock: LockedMove) -> (usize, i8) {
        let m = self.mon(r);
        match lock {
            LockedMove::Recharge => (RECHARGE, m.last_move_target_loc),
            LockedMove::Move(id) => {
                let slot = m
                    .moves
                    .iter()
                    .position(|ms| ms.id == id)
                    .unwrap_or(EXTERNAL);
                let loc = match m.vol.charging {
                    Some(c) if c == id && m.vol.charging_target_loc != 0 => {
                        m.vol.charging_target_loc
                    }
                    _ => m.last_move_target_loc,
                };
                (slot, loc)
            }
        }
    }

    /// End `twoturnmove` (and, through its onEnd, the charging move's volatile).
    pub(crate) fn end_two_turn(&mut self, r: MonRef) {
        let v = &mut self.mon_mut(r).vol;
        v.two_turn = None;
        v.two_turn_duration = 0;
        v.charging = None;
        v.charging_target_loc = 0;
        v.charging_duration = 0;
    }

    /// Magic Bounce: the holder uses the move on its user (`useMove`, with
    /// the original move's priority but no Prankster boost); its result
    /// becomes the holder's `moveThisTurnResult`.
    pub(super) fn bounce_move(&mut self, src: MonRef, holder: MonRef, move_id: MoveId) {
        let priority = self.move_priority(src, self.dex.mv(move_id));
        self.bounce = Some(priority);
        self.priority_override = Some((holder, move_id, priority));
        let loc = self.loc_of(holder, src);
        let result = self.use_move(holder, move_id, Some(src), loc, false);
        self.priority_override = None;
        self.mon_mut(holder).move_this_turn_result = result;
    }

    /// `runEvent('Invulnerability')`: a Pokémon charging Phantom Force can't
    /// be hit, unless the user or the target has No Guard (its handler goes
    /// first and returns 0).
    pub(crate) fn invulnerable(&self, r: MonRef, t: MonRef, move_id: &str) -> bool {
        use crate::kinds::AbilityKind;
        self.semi_invulnerable(t)
            && !self.charging_move_lets_through(t, move_id)
            && !self.has_ability(r, AbilityKind::NoGuard)
            && !self.has_ability(t, AbilityKind::NoGuard)
            && !self.lock_on_source_invulnerability(r, t)
    }

    /// `isSemiInvulnerable`: charging Phantom Force. The terrains leave such
    /// a Pokémon alone.
    pub(crate) fn semi_invulnerable(&self, t: MonRef) -> bool {
        let Some(id) = self.mon(t).vol.charging else {
            return false;
        };
        matches!(self.dex.move_kind[id as usize], MoveKind::Charge(ck) if ck.invulnerable())
    }

    /// Whether the move has a `priorityChargeCallback` (Chilly Reception).
    pub(crate) fn has_chilly_reception_charge(&self, id: MoveId) -> bool {
        self.dex.move_kind[id as usize] == MoveKind::ChillyReception
    }

    /// The `priorityChargeMove` action: `addVolatile('chillyreception')`.
    pub(crate) fn chilly_reception_priority_charge(&mut self, r: MonRef, _id: MoveId) {
        if self.mon(r).hp == 0 || self.mon(r).vol.chilly_reception {
            return;
        }
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        self.mon_mut(r).vol.chilly_reception = true;
        self.stamp_volatile(r, ResidualVol::ChillyReception);
    }

    /// Whether the move has any `priorityChargeCallback`.
    pub(crate) fn priority_charge_move(&self, id: MoveId) -> bool {
        self.has_chilly_reception_charge(id) || self.has_priority_charge_callback(id)
    }

    /// Whether the move has a `priorityChargeCallback` handled here.
    pub(crate) fn has_priority_charge_callback(&self, id: MoveId) -> bool {
        matches!(
            self.dex.move_kind[id as usize],
            MoveKind::BeakBlast | MoveKind::FocusPunch
        )
    }

    /// The `priorityChargeCallback` of Beak Blast and Focus Punch:
    /// `addVolatile('beakblast')` or `addVolatile('focuspunch')`.
    pub(crate) fn priority_charge_callback(&mut self, r: MonRef, id: MoveId) {
        let focus = self.dex.move_kind[id as usize] == MoveKind::FocusPunch;
        let have = if focus {
            self.mon(r).vol.focus_punch.is_some()
        } else {
            self.mon(r).vol.beak_blast
        };
        if self.mon(r).hp == 0 || have {
            return;
        }
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        let ri = self.ident(r);
        if focus {
            self.mon_mut(r).vol.focus_punch = Some(false);
            self.stamp_volatile(r, ResidualVol::FocusPunch);
            log_line!(self, "|-singleturn|{ri}|move: Focus Punch");
        } else {
            self.mon_mut(r).vol.beak_blast = true;
            self.stamp_volatile(r, ResidualVol::BeakBlast);
            log_line!(self, "|-singleturn|{ri}|move: Beak Blast");
        }
    }

    /// Whether the move has a `beforeTurnCallback`.
    pub(crate) fn has_before_turn_callback(&self, id: MoveId) -> bool {
        matches!(
            self.dex.move_kind[id as usize],
            MoveKind::Counter | MoveKind::MirrorCoat
        )
    }

    /// Counter's `beforeTurnCallback`: `addVolatile('counter')`.
    pub(crate) fn run_before_turn(&mut self, r: MonRef, id: MoveId) {
        let mirror = self.dex.move_kind[id as usize] == MoveKind::MirrorCoat;
        let have = if mirror {
            self.mon(r).vol.mirror_coat
        } else {
            self.mon(r).vol.counter
        };
        if self.mon(r).hp == 0 || have.is_some() {
            return;
        }
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        if mirror {
            self.mon_mut(r).vol.mirror_coat = Some(CounterVol::default());
            self.stamp_volatile(r, ResidualVol::MirrorCoat);
        } else {
            self.mon_mut(r).vol.counter = Some(CounterVol::default());
            self.stamp_volatile(r, ResidualVol::Counter);
        }
    }

    /// Focus Punch's `beforeMoveCallback`: after a hit this turn it cannot go.
    pub(crate) fn focus_punch_before_move_callback(&mut self, r: MonRef, id: MoveId) -> bool {
        if self.dex.move_kind[id as usize] != MoveKind::FocusPunch
            || self.mon(r).vol.focus_punch != Some(true)
        {
            return false;
        }
        let ri = self.ident(r);
        log_line!(self, "|cant|{ri}|Focus Punch|Focus Punch");
        true
    }

    /// The move's own `PrepareHit`: false fails the move.
    pub(crate) fn move_on_prepare_hit(&mut self, r: MonRef, kind: MoveKind) -> bool {
        match kind {
            // Sleep Talk's onTry: the user must be asleep.
            MoveKind::SleepTalk => self.mon(r).status == Status::Slp,
            // addVolatile('allyswitch'): restarting it rolls like Protect's stall.
            MoveKind::AllySwitch => {
                let counter = self.mon(r).vol.ally_switch_counter;
                if counter > 0 {
                    if !self.rng.chance(1, counter as u32) {
                        let v = &mut self.mon_mut(r).vol;
                        v.ally_switch_counter = 0;
                        v.ally_switch_duration = 0;
                        return false;
                    }
                    let v = &mut self.mon_mut(r).vol;
                    if v.ally_switch_counter < 729 {
                        v.ally_switch_counter *= 3;
                    }
                    v.ally_switch_duration = 2;
                } else {
                    self.try_add_volatile_sort(r);
                    self.stamp_volatile(r, ResidualVol::AllySwitch);
                    let v = &mut self.mon_mut(r).vol;
                    v.ally_switch_counter = 3;
                    v.ally_switch_duration = 2;
                }
                true
            }
            // onPrepareHit: `!removeVolatile('destinybond')`.
            MoveKind::DestinyBond => !std::mem::take(&mut self.mon_mut(r).vol.destiny_bond),
            // onTry: fails with No Retreat already in effect.
            MoveKind::NoRetreat => !self.mon(r).vol.no_retreat,
            // onTry: `source.getItem().isBerry` (Champions drops its onDisableMove).
            MoveKind::StuffCheeks => self.mon(r).item.is_some_and(|i| self.dex.item(i).is_berry),
            _ => true,
        }
    }

    /// removeVolatile('charge') (nothing at 0 HP): its end is announced
    /// silently.
    pub(crate) fn end_charge(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 {
            return;
        }
        self.mon_mut(r).vol.charge = false;
        let ri = self.ident(r);
        log_line!(self, "|-end|{ri}|Charge|[silent]");
    }

    /// The type a move takes after the user's other `ModifyType` handlers:
    /// Electrify (priority -2, last) makes it Electric. Struggle never
    /// gets here.
    pub(crate) fn run_modify_type(&self, r: MonRef, t: TypeId) -> TypeId {
        if self.mon(r).vol.electrify {
            return self.dex.ids.types.electric.unwrap_or(t);
        }
        t
    }

    /// removeVolatile('lockedmove'): its onEnd confuses the Pokémon when
    /// the rampage ran its course.
    pub(crate) fn end_locked_move(&mut self, r: MonRef) {
        let true_duration = self.mon(r).vol.locked_true_duration;
        let v = &mut self.mon_mut(r).vol;
        v.locked_move = None;
        v.locked_duration = 0;
        v.locked_true_duration = 0;
        if true_duration <= 1 {
            self.fatigue_confusion(r);
        }
    }

    /// The semi-invulnerable move the Pokémon is charging, when it is one of
    /// Fly, Bounce, Dig or Dive (Phantom Force has no source handlers).
    pub(crate) fn charging_kind(&self, t: MonRef) -> Option<crate::kinds::ChargeKind> {
        let id = self.mon(t).vol.charging?;
        match self.dex.move_kind[id as usize] {
            crate::kinds::MoveKind::Charge(ck) if ck.invulnerable() => Some(ck),
            _ => None,
        }
    }

    /// The charging move's `onInvulnerability` lets this move through.
    pub(crate) fn charging_move_lets_through(&self, t: MonRef, move_id: &str) -> bool {
        self.charging_kind(t)
            .is_some_and(|ck| ck.lets_through(move_id))
    }

    /// The target holds an `onSourceModifyDamage` volatile: Fly, Dig, Dive.
    pub(crate) fn semi_invulnerable_source_modify_damage(&self, t: MonRef) -> bool {
        use crate::kinds::ChargeKind as C;
        matches!(self.charging_kind(t), Some(C::Fly | C::Dig | C::Dive))
    }

    /// Whether the volatile doubles this move: `base_power` asks for the
    /// `onSourceBasePower` of Bounce, otherwise the `onSourceModifyDamage`
    /// of the others.
    pub(crate) fn semi_invulnerable_doubles(
        &self,
        t: MonRef,
        move_id: &str,
        base_power: bool,
    ) -> bool {
        use crate::kinds::ChargeKind as C;
        match self.charging_kind(t) {
            Some(ck @ C::Bounce) => base_power && ck.doubles(move_id),
            Some(ck @ (C::Fly | C::Dig | C::Dive)) => !base_power && ck.doubles(move_id),
            _ => false,
        }
    }

    /// Dig and Dive make their user immune to sandstorm damage (`onImmunity`).
    pub(crate) fn immune_to_sandstorm_while_digging(&self, t: MonRef) -> bool {
        use crate::kinds::ChargeKind as C;
        matches!(self.charging_kind(t), Some(C::Dig | C::Dive))
    }

    /// Eelevate's `onSourceAfterFaint`: the best stored stat (ties keep the
    /// first of Atk, Def, SpA, SpD, Spe) rises by the number that fainted.
    pub(crate) fn eelevate_on_source_after_faint(&mut self, s: MonRef, length: usize) {
        let stats = self.mon(s).stats;
        let mut best = 1;
        for i in 2..6 {
            if stats[i] > stats[best] {
                best = i;
            }
        }
        let mut b = [0i8; 7];
        b[best - 1] = length.min(6) as i8;
        let a = self.mon(s).ability;
        self.boost(s, &b, Some(s), EffectRef::Ability(a), false, false);
    }
}
