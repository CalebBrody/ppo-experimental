//! Moves that call other moves: Copycat, Sleep Talk, Instruct and Metronome.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::ItemKind;
use crate::state::*;

/// A move called by another (`useMove` with the caller as `sourceEffect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Call {
    pub user: MonRef,
    pub id: MoveId,
    /// The caller: named in `[from] move:`, and it pays the Pressure PP.
    pub source: MoveId,
    /// The caller's `pranksterBoosted`, which the called move inherits.
    pub prankster: bool,
}

impl Battle {
    /// The Metronome item's `onStart`: `addVolatile('metronome')`.
    pub(crate) fn metronome_on_start(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 || self.mon(r).vol.metronome.is_some() {
            return;
        }
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        self.mon_mut(r).vol.metronome = Some(MetronomeVol::default());
    }

    /// The Metronome volatile's `onTryMove` (priority -2, after every other
    /// TryMove handler let the move through).
    pub(crate) fn metronome_on_try_move(&mut self, r: MonRef, mv: &Move) {
        let Some(mut v) = self.mon(r).vol.metronome else {
            return;
        };
        if self.item_kind(r) != Some(ItemKind::Metronome) {
            self.mon_mut(r).vol.metronome = None;
            return;
        }
        // A move that calls another (Copycat, Sleep Talk) leaves the count alone.
        if mv.fields.iter().any(|f| f == "callsMove") {
            return;
        }
        let same = v.last_move == Some(mv.idx);
        if same && self.mon(r).move_last_turn_result == Some(true) {
            v.num += 1;
        } else if self.mon(r).vol.two_turn.is_some() {
            v.num = if same { v.num + 1 } else { 1 };
        } else {
            v.num = 0;
        }
        v.last_move = Some(mv.idx);
        self.mon_mut(r).vol.metronome = Some(v);
    }

    /// The Metronome volatile's `onModifyDamage` modifier.
    pub(crate) fn metronome_on_modify_damage(&self, src: MonRef) -> Option<(u32, u32)> {
        let v = self.mon(src).vol.metronome?;
        Some((
            [4096, 4915, 5734, 6553, 7372, 8192][v.num.min(5) as usize],
            4096,
        ))
    }

    /// `clearActiveMove()` at an action's end: the active move becomes the
    /// battle's `lastMove`.
    pub(crate) fn clear_active_move(&mut self) {
        if let Some(m) = self.active_move.cur_move.take() {
            self.turn_state.last_move = Some(m);
            self.active_move.cur_user = None;
        }
    }

    /// The caller of `id` when `r` is using it as a called move.
    pub(crate) fn caller_of(&self, r: MonRef, id: MoveId) -> Option<MoveId> {
        self.active_move
            .call
            .filter(|c| c.user == r && c.id == id)
            .map(|c| c.source)
    }

    /// A called move's `pranksterBoosted`: the caller's.
    pub(crate) fn called_prankster_boosted(&self, r: MonRef, id: MoveId) -> Option<bool> {
        self.active_move
            .call
            .filter(|c| c.user == r && c.id == id)
            .map(|c| c.prankster)
    }

    /// Showdown `actions.useMove(id, user)` from a move's onHit: the target
    /// is the move's ModifyTarget or a random one, the called move keeps the
    /// caller's priority and Prankster boost, and its result becomes the
    /// user's `moveThisTurnResult`.
    pub(crate) fn call_move(&mut self, r: MonRef, id: MoveId, source: &Move) {
        let dex = self.dex.clone();
        let mv = dex.mv(id);
        let kind = dex.move_kind[id as usize];
        let target = if self.comeuppance_on_modify_target(r, kind).is_some() {
            None
        } else {
            self.get_random_target(r, mv.target)
        };
        let loc = target.map(|t| self.loc_of(r, t)).unwrap_or(0);
        let priority = self.move_priority(r, source);
        let prankster = self.prankster_boosted(r, source);
        let saved = self.priority_override.replace((r, id, priority));
        let outer = self.active_move.call.replace(Call {
            user: r,
            id,
            source: source.idx,
            prankster,
        });
        let result = self.use_move(r, id, target, loc, false);
        self.active_move.call = outer;
        self.priority_override = saved;
        self.active_move.called = Some((id, result, self.mon(r).fainted));
    }

    /// After `useMove` in `runMove`: the active move and the user's
    /// `moveThisTurnResult`. The called move's `useMove` set the result, so
    /// the caller's keeps it, unless the user fainted since (the caller's
    /// hit loop: `clearVolatile` resets it, and the caller's result is set).
    pub(crate) fn called_move_result(
        &mut self,
        r: MonRef,
        id: MoveId,
        result: Option<bool>,
    ) -> (MoveId, Option<bool>) {
        match self.active_move.called.take() {
            Some((called, inner, fainted)) => (
                called,
                if self.mon(r).fainted && !fainted {
                    result
                } else {
                    inner
                },
            ),
            None => (id, result),
        }
    }

    /// Copycat's onHit: the battle's last move, unless it can't be copied
    /// (`failcopycat`); with none, it does nothing (and doesn't fail).
    pub(crate) fn copycat(&mut self, r: MonRef, mv: &Move) -> bool {
        let Some(last) = self.turn_state.last_move else {
            return true;
        };
        if self.dex.mv(last).has(MoveFlag::FailCopycat) {
            self.fail_move(r);
            return false;
        }
        self.call_move(r, last, mv);
        true
    }

    /// Sleep Talk's onHit: one of the user's moves at random, but not one
    /// with `nosleeptalk` or `charge`.
    pub(crate) fn sleep_talk(&mut self, r: MonRef, mv: &Move) -> bool {
        let dex = self.dex.clone();
        let moves: Vec<MoveId> = self
            .mon(r)
            .moves
            .iter()
            .map(|ms| ms.id)
            .filter(|&id| {
                !dex.mv(id).has(MoveFlag::NoSleepTalk) && !dex.mv(id).has(MoveFlag::Charge)
            })
            .collect();
        if moves.is_empty() {
            self.fail_move(r);
            return false;
        }
        let pick = self.rng.sample(&moves);
        self.call_move(r, pick, mv);
        true
    }

    /// Instruct's onHit: the target uses its last move again right after
    /// this action (`prioritizeAction` of `resolveAction(...)[0]`, order 3).
    /// The resolved list starts with the move's priority-charge or
    /// before-turn action when it has one, and only that one is queued.
    pub(crate) fn instruct(&mut self, r: MonRef, t: MonRef) -> bool {
        let dex = self.dex.clone();
        let m = self.mon(t);
        let Some(last) = m.last_move else {
            self.fail_move(r);
            return false;
        };
        let lm = dex.mv(last);
        let slot = m.moves.iter().position(|ms| ms.id == last);
        if lm.has(MoveFlag::FailInstruct)
            || lm.has(MoveFlag::Charge)
            || lm.has(MoveFlag::Recharge)
            || m.vol.beak_blast
            || m.vol.focus_punch.is_some()
            || slot.is_some_and(|s| m.moves[s].pp == 0)
        {
            self.fail_move(r);
            return false;
        }
        let loc = m.last_move_target_loc;
        let (ti, ri) = (self.ident(t), self.ident(r));
        log_line!(self, "|-singleturn|{ti}|move: Instruct|[of] {ri}");
        let (side, mon) = t;
        let before = self.has_before_turn_callback(last).then(|| {
            self.resolve_action(ActionKind::BeforeTurnMove {
                side,
                mon,
                id: last,
                target: loc,
            })
        });
        let charge = self.priority_charge_move(last).then(|| {
            self.resolve_action(ActionKind::PriorityCharge {
                side,
                mon,
                id: last,
            })
        });
        let slot = slot.unwrap_or(crate::state::EXTERNAL);
        let action = self.resolve_action(ActionKind::Move {
            side,
            mon,
            slot,
            target: loc,
            id: last,
        });
        let mut first = charge.or(before).unwrap_or(action);
        first.order = 3;
        self.queue.push_front(first);
        true
    }
}
