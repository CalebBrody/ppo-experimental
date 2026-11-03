//! Per-turn bookkeeping: what hit whom, who was hurt and which stats changed this turn.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::kinds::{AbilityKind, EffectRef};
use crate::state::*;

impl Battle {
    /// `endTurn`: after the first turn, every active Pokémon's per-turn flags reset.
    pub(crate) fn reset_turn_flags(&mut self, r: MonRef) {
        // Hits taken stop counting as this turn's; those from a source that
        // is no longer on the field are forgotten.
        let mut attacked = std::mem::take(&mut self.mon_mut(r).attacked_by);
        attacked.retain_mut(|a| {
            let active = self.attacker_is_active(a);
            if active {
                a.this_turn = false;
            }
            active
        });
        self.mon_mut(r).attacked_by = attacked;
        if self.turn != 1 {
            let m = self.mon_mut(r);
            m.stats_raised_this_turn = false;
            m.stats_lowered_this_turn = false;
        }
    }

    /// `switchIn` for the Pokémon leaving: `clearVolatile` (if it still
    /// stands) clears `hurtThisTurn`; the stat flags reset either way.
    pub(crate) fn switch_out_cleanup(&mut self, r: MonRef) {
        let stands = self.mon(r).hp > 0;
        let m = self.mon_mut(r);
        if stands {
            m.attacked_by.clear();
        }
        m.stats_raised_this_turn = false;
        m.stats_lowered_this_turn = false;
    }

    /// Fainting runs `clearVolatile`, which clears `hurtThisTurn` only.
    pub(crate) fn clear_volatile_on_faint(&mut self, r: MonRef) {
        self.mon_mut(r).hurt_this_turn = None;
        self.mon_mut(r).attacked_by.clear();
    }

    /// `spreadDamage` after HP was lost: `hurtThisTurn`.
    pub(crate) fn set_hurt_this_turn(&mut self, t: MonRef, dealt: u32) {
        if dealt > 0 {
            let hp = self.mon(t).hp;
            self.mon_mut(t).hurt_this_turn = Some(hp);
        }
    }

    /// The end of `boost`: a successful boost whose table (after capping and
    /// TryBoost) raises a stat marks `statsRaisedThisTurn`; one that lowers
    /// a stat, `statsLoweredThisTurn`.
    pub(crate) fn mark_stat_changes_this_turn(
        &mut self,
        t: MonRef,
        b: &Boosts,
        present: &[bool; 7],
        success: bool,
    ) {
        if success && (0..7).any(|i| present[i] && b[i] > 0) {
            self.mon_mut(t).stats_raised_this_turn = true;
        }
        if success && (0..7).any(|i| present[i] && b[i] < 0) {
            self.mon_mut(t).stats_lowered_this_turn = true;
        }
    }

    /// The end of `faintMessages` (unless the battle was won):
    /// `runEvent('AfterFaint')` for the last entry of the queue, with the
    /// queue's length. Moxie raises its holder's Attack by that length
    /// when the faint came from a move.
    pub(crate) fn run_after_faint(&mut self, queue: &[MonRef]) {
        let Some(&last) = queue.last() else { return };
        let source = self
            .turn_state
            .move_kills
            .iter()
            .find(|&&(t, _)| t == last)
            .map(|&(_, s)| s);
        self.turn_state
            .move_kills
            .retain(|(t, _)| !queue.contains(t));
        if self.ended {
            return;
        }
        self.event_sort(Event::AfterFaint, last, source, &[]);
        if let Some(s) = source {
            if self.has_ability(s, AbilityKind::Moxie) {
                let mut b = [0i8; 7];
                b[Boost::Atk as usize] = queue.len().min(6) as i8;
                let a = self.mon(s).ability;
                self.boost(s, &b, Some(s), EffectRef::Ability(a), false, false);
            } else if self.has_ability(s, AbilityKind::Eelevate) {
                self.eelevate_on_source_after_faint(s, queue.len());
            }
        }
    }

    /// The attack's source as a Pokémon, by its side and name.
    pub(crate) fn attacker_mon(&self, a: &Attacker) -> Option<MonRef> {
        let s = a.side as usize;
        self.sides[s]
            .mons
            .iter()
            .position(|m| m.name == a.name)
            .map(|i| (s, i))
    }

    /// `source.isActive` for an attack's source.
    pub(crate) fn attacker_is_active(&self, a: &Attacker) -> bool {
        self.attacker_mon(a).is_some_and(|r| {
            let m = self.mon(r);
            m.is_active && !m.fainted && !m.revived
        })
    }

    /// `getLastDamagedBy(true)`: the last hit from a Pokémon of the other side.
    pub(crate) fn last_damaged_by(&self, r: MonRef) -> Option<&Attacker> {
        self.mon(r)
            .attacked_by
            .iter()
            .rev()
            .find(|a| a.side as usize != r.0)
    }

    /// `gotAttacked` for a target that took a hit's damage.
    pub(crate) fn got_attacked(&mut self, t: MonRef, src: MonRef, mv: MoveId, damage: u32) {
        let a = Attacker {
            side: src.0 as u8,
            name: self.mon(src).name.clone(),
            damage,
            mv,
            this_turn: true,
            slot: (src.0 as u8, self.mon(src).position as u8),
        };
        self.mon_mut(t).attacked_by.push(a);
    }

    /// A move's damage left the target at 0 HP: its faint comes from the move.
    pub(crate) fn note_move_damage(&mut self, t: MonRef, src: MonRef) {
        if self.mon(t).hp == 0 && !self.turn_state.move_kills.iter().any(|&(x, _)| x == t) {
            self.turn_state.move_kills.push((t, src));
        }
    }
}
