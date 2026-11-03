//! Multi-hit moves: `multiaccuracy`.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, ItemKind, MoveKind};
use crate::state::*;

impl Battle {
    /// A later hit of a `multiaccuracy` move (Champions `hitStepMoveHitLoop`):
    /// the base accuracy scaled by the user's accuracy and the target's
    /// evasion stages (a table, not the fractions of `hitStepAccuracy`), then
    /// ModifyAccuracy and the Accuracy event. The roll compares with the
    /// unrounded chance.
    pub(super) fn multiaccuracy_hits(&mut self, r: MonRef, t: MonRef, mv: &Move) -> bool {
        let Some(base) = mv.accuracy else { return true };
        self.multiaccuracy_events(r, t, mv);
        const TABLE: [f64; 7] = [1.0, 4.0 / 3.0, 5.0 / 3.0, 2.0, 7.0 / 3.0, 8.0 / 3.0, 3.0];
        let mut acc = base as f64;
        let ab = self.accuracy_stage(r, t).clamp(-6, 6);
        acc = if ab > 0 {
            acc * TABLE[ab as usize]
        } else {
            acc / TABLE[(-ab) as usize]
        };
        if !self.ignores_evasion(r, mv) {
            let eb = self.evasion_stage(r, t).clamp(-6, 6);
            acc = if eb > 0 {
                acc / TABLE[eb as usize]
            } else {
                acc * TABLE[(-eb) as usize]
            };
        }
        // ModifyAccuracy: a chained modifier applies to the unrounded value
        // and truncates (Showdown `modify`).
        let chain = self.accuracy_chain(r, t, mv);
        if chain.0 != 4096 {
            acc = (((acc * chain.0 as f64).trunc() + 2047.0) / 4096.0).trunc();
        }
        if self.accuracy_event(r, t, mv) {
            return true;
        }
        (self.rng.random(100) as f64) < acc
    }

    /// The move's hit count after ModifyMove: Skill Link takes the most.
    pub(crate) fn hit_count(&self, r: MonRef, mv: &Move) -> Option<(u8, u8)> {
        match mv.multihit {
            Some((_, hi)) if self.has_ability(r, AbilityKind::SkillLink) => Some((hi, hi)),
            m => m,
        }
    }

    /// Whether each hit after the first rolls accuracy (`multiaccuracy`);
    /// Skill Link deletes it.
    pub(crate) fn has_multiaccuracy(&self, r: MonRef, kind: MoveKind) -> bool {
        matches!(kind, MoveKind::TripleAxel | MoveKind::PopulationBomb)
            && !self.has_ability(r, AbilityKind::SkillLink)
    }

    /// The events of a later `multiaccuracy` hit's accuracy, in Showdown's
    /// order: ModifyBoost on the user and on the target, then ModifyAccuracy.
    pub(crate) fn multiaccuracy_events(&mut self, r: MonRef, t: MonRef, mv: &Move) {
        self.event_sort(Event::ModifyBoost, r, None, &[]);
        if !self.ignores_evasion(r, mv) {
            self.event_sort(Event::ModifyBoost, t, None, &[]);
        }
        self.event_sort(Event::ModifyAccuracy, t, Some(r), &[]);
    }

    /// Parental Bond's `onPrepareHit`: a damaging move that is not already
    /// multi-hit, spread, charging or a future move hits twice.
    pub(crate) fn parental_bond_on_prepare_hit(
        &mut self,
        r: MonRef,
        mv: &Move,
        spread: bool,
    ) -> Option<Move> {
        self.active_move.pbond = false;
        if !self.has_ability(r, AbilityKind::ParentalBond)
            || mv.category == Category::Status
            || mv.multihit.is_some()
            || spread
            || mv.has(MoveFlag::NoParentalBond)
            || mv.has(MoveFlag::Charge)
            || mv.has(MoveFlag::FutureMove)
        {
            return None;
        }
        self.active_move.pbond = true;
        Some(Move {
            multihit: Some((2, 2)),
            ..mv.clone()
        })
    }

    /// The user's item's `onSourceTryPrimaryHit` (before Substitute's):
    /// a gem of the move's type is used up (even by a user at 0 HP after
    /// Explosion), then the user gets the `gem` volatile.
    pub(crate) fn run_source_try_primary_hit(
        &mut self,
        r: MonRef,
        t: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) {
        if t == r || mv.category == Category::Status || mv.has(MoveFlag::PledgeCombo) {
            return;
        }
        let Some(ItemKind::Gem(gem_type)) = self.item_kind(r) else {
            return;
        };
        if gem_type != move_type || !self.mon(r).is_active {
            return;
        }
        let ident = self.ident(r);
        let name = self
            .mon(r)
            .item
            .map(|i| self.dex.item(i).name.clone())
            .unwrap_or_default();
        log_line!(
            self,
            "|-enditem|{ident}|{name}|[from] gem|[move] {}",
            mv.name
        );
        self.consume_item(r, false);
        // addVolatile('gem'): `affectsFainted`, so a user at 0 HP gets it too.
        if !self.mon(r).vol.gem {
            self.try_add_volatile_sort(r);
            self.stamp_volatile(r, ResidualVol::Gem);
            self.mon_mut(r).vol.gem = true;
        }
    }

    /// Beat Up's `onModifyMove`: one hit per healthy team member (and the
    /// user), each with the base power its set species' Attack gives.
    pub(crate) fn beat_up_on_modify_move(&mut self, r: MonRef, mv: &Move) -> Move {
        let bps: Vec<u32> = self.sides[r.0]
            .mons
            .iter()
            .enumerate()
            .filter(|(i, m)| *i == r.1 || (!m.fainted && m.status == Status::None))
            .map(|(_, m)| {
                5 + self.dex.species(m.set_species).base_stats[Stat::Atk as usize] as u32 / 10
            })
            .collect();
        let n = bps.len() as u8;
        self.active_move.beat_up = bps;
        Move {
            multihit: Some((n, n)),
            ..mv.clone()
        }
    }
}
