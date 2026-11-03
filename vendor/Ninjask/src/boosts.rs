//! Stat boosts: `TryBoost`, Opportunist and boost messages.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, EffectRef, MoveKind};

impl Battle {
    /// ChangeBoost: Simple doubles every change.
    pub(crate) fn simple_on_change_boost(&self, t: MonRef, b: &mut [i8; 7]) {
        if self.has_ability(t, AbilityKind::Simple) {
            for x in b.iter_mut() {
                *x = x.saturating_mul(2);
            }
        }
    }

    /// A boost's message when the effect has its own (`boost`'s switch):
    /// Belly Drum sets Attack. Returns whether it was written.
    pub(crate) fn belly_drum_boost_message(&mut self, t: MonRef, effect: EffectRef) -> bool {
        match effect {
            EffectRef::Move(m) if self.dex.move_kind[m as usize] == MoveKind::BellyDrum => {
                let ti = self.ident(t);
                let atk = self.mon(t).boosts[Boost::Atk as usize];
                log_line!(self, "|-setboost|{ti}|atk|{atk}|[from] move: Belly Drum");
                true
            }
            _ => false,
        }
    }

    /// Opportunist's `onFoeAfterBoost`, from the `AfterBoost` event on `t`:
    /// each foe's holder keeps the positive boosts (in the order Showdown first
    /// saw each stat), unless Opportunist or Mirror Herb caused them. The
    /// holder's table exists, empty, after any such event.
    pub(crate) fn opportunist_on_foe_after_boost(
        &mut self,
        t: MonRef,
        b: &Boosts,
        present: &[bool; 7],
        order: &[u8; 7],
        effect: EffectRef,
    ) {
        let skip = match effect {
            EffectRef::Ability(a) => self.dex.ability(a).id == "opportunist",
            EffectRef::Item(i) => self.dex.item(i).id == "mirrorherb",
            _ => false,
        };
        if skip {
            return;
        }
        let foe = 1 - t.0;
        for slot in 0..self.sides[foe].n_active {
            let Some(h) = self.active(foe, slot) else {
                continue;
            };
            // `foes()` lists the Pokémon with HP left (a faint is announced later).
            if self.mon(h).fainted
                || self.mon(h).hp == 0
                || !self.has_ability(h, AbilityKind::Opportunist)
            {
                continue;
            }
            let mut mb = self.mon(h).ability_state.opp_boosts.unwrap_or_default();
            for &i in order {
                let i = i as usize;
                if present[i] && b[i] > 0 {
                    if mb.vals[i] == 0 {
                        let listed = mb.vals.iter().filter(|&&v| v != 0).count();
                        let mut ord = mb.order.to_vec();
                        let pos = ord.iter().position(|&x| x as usize == i).unwrap_or(0);
                        ord.remove(pos);
                        ord.insert(listed, i as u8);
                        mb.order.copy_from_slice(&ord);
                    }
                    mb.vals[i] += b[i];
                }
            }
            self.mon_mut(h).ability_state.opp_boosts = Some(mb);
        }
    }

    /// Opportunist's `onAnySwitchIn`, `onAnyAfterMove`, `onAnyAfterMega` and
    /// `onResidual`: boost by what the foes raised, once.
    pub(crate) fn opportunist_release(&mut self, h: MonRef) {
        if self.mon(h).fainted
            || !self.mon(h).is_active
            || !self.has_ability(h, AbilityKind::Opportunist)
        {
            return;
        }
        let Some(mb) = self.mon(h).ability_state.opp_boosts else {
            return;
        };
        let a = self.mon(h).ability;
        self.boost_listed(h, &mb, None, EffectRef::Ability(a), false, false);
        self.mon_mut(h).ability_state.opp_boosts = None;
    }

    /// `onTryBoost` of Big Pecks (Defense) and Keen Eye / Illuminate
    /// (accuracy): another's drop is removed, announced unless the effect is
    /// a move with secondaries (or Octolock, for Big Pecks).
    pub(crate) fn run_try_boost(
        &mut self,
        t: MonRef,
        b: &Boosts,
        present: &mut [bool; 7],
        effect: EffectRef,
        quiet: bool,
    ) {
        let (stat, octolock) = match self.ability_kind(t) {
            AbilityKind::BigPecks => (
                Boost::Def,
                matches!(effect, EffectRef::Move(m) if self.dex.mv(m).id == "octolock"),
            ),
            _ => (Boost::Accuracy, false),
        };
        let i = stat as usize;
        if present[i] && b[i] < 0 {
            present[i] = false;
            if !quiet && !octolock {
                let ti = self.ident(t);
                let name = self.ability_name(t);
                log_line!(
                    self,
                    "|-fail|{ti}|unboost|{}|[from] ability: {name}|[of] {ti}",
                    stat.id()
                );
            }
        }
    }

    /// A boost's message when the effect has its own (`boost`'s switch):
    /// Anger Point sets Attack. Returns whether it was written.
    pub(crate) fn anger_point_boost_message(&mut self, t: MonRef, effect: EffectRef) -> bool {
        match effect {
            EffectRef::Ability(a) if self.dex.ability(a).id == "angerpoint" => {
                let ti = self.ident(t);
                let atk = self.mon(t).boosts[Boost::Atk as usize];
                log_line!(
                    self,
                    "|-setboost|{ti}|atk|{atk}|[from] ability: Anger Point"
                );
                true
            }
            _ => false,
        }
    }

    /// Whether the user's move ignores evasion: its own flag, or Keen Eye's
    /// or Illuminate's ModifyMove.
    pub(crate) fn ignores_evasion(&self, r: MonRef, mv: &Move) -> bool {
        mv.ignore_evasion || self.active_move.ignore_evasion == Some(r)
    }

    /// Corrosion (`setStatus`): the source poisons regardless of the
    /// target's type.
    pub(crate) fn corrosion_poisons(&self, src: MonRef, st: Status) -> bool {
        matches!(st, Status::Psn | Status::Tox) && self.has_ability(src, AbilityKind::Corrosion)
    }
}
