//! The hit steps of `trySpreadMoveHit`: `TryHit`, accuracy, immunity, Protect and
//! Substitute, and the effects a move applies to each target.

use crate::battle::Grounded;
use crate::battle::Small;
use crate::battle::{ActiveList, Battle, MonRef, PerActive};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, ItemKind};
use crate::kinds::{EffectRef, MoveKind, ProtectKind};
use crate::state::*;

/// Result of the `TryHit` step for one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TryHit {
    Hit,
    /// Blocked without failing (`NOT_FAIL`: Protect, Wide Guard, Quick Guard).
    Blocked,
    /// Blocked as a failure of the move (Psychic Terrain returns null).
    Failed,
}

/// Showdown's `didSomething` for a move's effects on one target, in the
/// precedence `combineResults` gives them: true over false over null over
/// undefined (nothing tried, which counts as success).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Did {
    Nothing,
    Null,
    Fail,
    Done,
}

/// Why a Pokémon is immune to a move's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeImmunity {
    /// Levitate (or Eelevate) against a Ground move.
    Levitate,
    /// Off the ground otherwise (Flying, Air Balloon, Magnet Rise...).
    Airborne,
    /// Its types.
    Type,
}

/// A move's accuracy before stages and modifiers.
#[derive(Clone, Copy, Debug)]
enum Accuracy {
    /// It can't miss.
    Sure,
    /// A one-hit KO's: no stages or modifiers.
    Fixed(u32),
    /// Subject to accuracy and evasion stages and modifiers.
    Stages(u32),
}

impl Battle {
    /// `TryHit` handlers on the target, in priority order: Psychic Terrain (4), Protect (3).
    /// TryHit at priority 4 (Wide Guard, Quick Guard, Psychic Terrain) or 3
    /// (Protect and its variants). Showdown runs each priority for every
    /// target before the next, so `stage` picks one.
    pub(super) fn try_hit(&mut self, src: MonRef, t: MonRef, mv: &Move, stage: u8) -> TryHit {
        let ti = self.ident(t);
        let protectable = mv.has(MoveFlag::Protect);
        if stage == 3 {
            if let Some(kind) = self.mon(t).vol.protect {
                if protectable && (mv.category != Category::Status || kind.blocks_status()) {
                    // checkMoveBypassesProtect: Piercing Drill, Unseen Fist.
                    if self.move_bypasses_protect(src, t, mv) {
                        return TryHit::Hit;
                    }
                    // A smart-targeting move is blocked quietly, and strikes
                    // the other target twice.
                    if self.smart_target {
                        self.smart_target = false;
                    } else {
                        log_line!(self, "|-activate|{ti}|move: Protect");
                    }
                    if mv.has(MoveFlag::Contact) {
                        self.protect_contact_effect(src, t, kind);
                    }
                    return TryHit::Blocked;
                }
            }
            return TryHit::Hit;
        }
        let conds = self.sides[t.0].conds;
        let spread_move = matches!(
            self.move_target_type(src, mv),
            Target::AllAdjacent | Target::AllAdjacentFoes
        );
        if conds.wide_guard > 0
            && spread_move
            && protectable
            && !self.move_bypasses_protect(src, t, mv)
        {
            log_line!(self, "|-activate|{ti}|move: Wide Guard");
            return TryHit::Blocked;
        }
        if conds.quick_guard > 0
            && self.move_priority(src, mv) > 0
            && protectable
            && !self.move_bypasses_protect(src, t, mv)
        {
            log_line!(self, "|-activate|{ti}|move: Quick Guard");
            return TryHit::Blocked;
        }
        if self.field.terrain == Some(Terrain::Psychic)
            && self.move_priority(src, mv) > 0
            && mv.target != Target::SelfTarget
            && t.0 != src.0
            && self.is_grounded(t)
        {
            let ti = self.ident(t);
            log_line!(self, "|-activate|{ti}|move: Psychic Terrain");
            return TryHit::Failed;
        }
        TryHit::Hit
    }

    /// What a Protect variant does to a Pokémon that touched it.
    fn protect_contact_effect(&mut self, src: MonRef, t: MonRef, kind: ProtectKind) {
        let move_ref = |b: &Battle, id: &str| EffectRef::Move(b.dex.move_id(id).unwrap_or(0));
        match kind {
            ProtectKind::SpikyShield => {
                let amount = (self.mon(src).maxhp as u32 / 8).max(1);
                let ti = self.ident(t);
                self.apply_damage(src, amount, &format!("|[from] Spiky Shield|[of] {ti}"));
            }
            ProtectKind::BanefulBunker => {
                let effect = move_ref(self, "banefulbunker");
                self.try_set_status(src, Status::Psn, t, effect, false);
            }
            ProtectKind::KingsShield | ProtectKind::Obstruct => {
                let mut b = [0i8; 7];
                let (stat, n, id) = if kind == ProtectKind::KingsShield {
                    (Boost::Atk, -1, "kingsshield")
                } else {
                    (Boost::Def, -2, "obstruct")
                };
                b[stat as usize] = n;
                let effect = move_ref(self, id);
                self.boost(src, &b, Some(t), effect, false, false);
            }
            ProtectKind::Protect => {}
        }
    }

    /// Add or restart the `stall` volatile (Protect's consecutive-use counter).
    pub(crate) fn add_stall(&mut self, r: MonRef) {
        if self.mon(r).vol.stall_counter == 0 {
            self.stamp_volatile(r, ResidualVol::Stall);
        }
        let m = self.mon_mut(r);
        if m.vol.stall_counter == 0 {
            m.vol.stall_counter = 3;
        } else if m.vol.stall_counter < 729 {
            m.vol.stall_counter *= 3;
        }
        m.vol.stall_duration = 2;
    }

    /// Showdown `runImmunity(move, message)` for a move type.
    pub(crate) fn run_immunity(&mut self, t: MonRef, move_type: TypeId, message: bool) -> bool {
        let Some(immunity) = self.type_immunity(t, move_type) else {
            return true;
        };
        if message {
            let ti = self.ident(t);
            match immunity {
                TypeImmunity::Levitate => {
                    let name = if self.raw_ability_kind(t) == crate::kinds::AbilityKind::Eelevate {
                        "Eelevate"
                    } else {
                        "Levitate"
                    };
                    log_line!(self, "|-immune|{ti}|[from] ability: {name}");
                }
                TypeImmunity::Airborne | TypeImmunity::Type => log_line!(self, "|-immune|{ti}"),
            }
        }
        false
    }

    /// Why `t` is immune to moves of `move_type` (`runImmunity`), if it is:
    /// a Ground move against a Pokémon off the ground, or its types.
    pub(crate) fn type_immunity(&self, t: MonRef, move_type: TypeId) -> Option<TypeImmunity> {
        if Some(move_type) == self.dex.ids.types.ground {
            return match self.grounded(t) {
                Grounded::Yes => None,
                Grounded::Levitate => Some(TypeImmunity::Levitate),
                Grounded::Airborne => Some(TypeImmunity::Airborne),
            };
        }
        let ty = self.types_of(t);
        ty.iter()
            .any(|&x| self.dex.is_immune(move_type, x))
            .then_some(TypeImmunity::Type)
    }

    /// Showdown `hitStepAccuracy` for one target: the event sorts' draws,
    /// then the roll against [`Battle::move_accuracy`]'s value.
    pub(crate) fn accuracy_hits(&mut self, r: MonRef, t: MonRef, mv: &Move) -> bool {
        let base = self.accuracy_base(r, t, mv);
        if let Accuracy::Stages(_) = base {
            // ModifyBoost on the user, then on the target for its evasion.
            self.event_sort(Event::ModifyBoost, r, None, &[]);
            if !self.ignores_evasion(r, mv) {
                self.event_sort(Event::ModifyBoost, t, None, &[]);
            }
        }
        let sure = self.accuracy_sure(r, mv) || self.accuracy_event(r, t, mv);
        match self.accuracy_value(r, t, mv, base, sure) {
            None => true,
            Some(a) => self.rng.chance(a, 100),
        }
    }

    /// The chance in percent that `r`'s move hits `t` (`None`: it can't
    /// miss), as `hitStepAccuracy` computes it; read-only.
    pub fn move_accuracy(&self, r: MonRef, t: MonRef, mv: &Move) -> Option<u32> {
        let base = self.accuracy_base(r, t, mv);
        let sure = self.accuracy_sure(r, mv) || self.accuracy_event_sure(r, t, mv);
        self.accuracy_value(r, t, mv, base, sure)
    }

    /// The move's accuracy before stages and modifiers.
    fn accuracy_base(&self, r: MonRef, t: MonRef, mv: &Move) -> Accuracy {
        // None: the move can't miss (`accuracy: true`).
        let mut acc = mv.accuracy.map(u32::from);
        // onModifyMove: Hurricane and Thunder in rain or sun, Blizzard in snow.
        match (self.dex.move_kind[mv.idx as usize], self.move_weather()) {
            (MoveKind::RainAccuracy, Some(Weather::Rain))
            | (MoveKind::SnowAccuracy, Some(Weather::Snow)) => acc = None,
            (MoveKind::RainAccuracy, Some(Weather::Sun)) => acc = acc.map(|_| 50),
            _ => {}
        }
        // A one-hit KO skips ModifyAccuracy and the stages: 30, or 20 for
        // Sheer Cold from a non-Ice user, plus the level difference.
        if let Some(immune_type) = mv.ohko {
            let base = match immune_type {
                Some(ty) if !self.has_type_of(r, ty) => 20,
                _ => 30,
            };
            return Accuracy::Fixed(base + (self.mon(r).level - self.mon(t).level) as u32);
        }
        match acc {
            None => Accuracy::Sure,
            Some(a) => Accuracy::Stages(a),
        }
    }

    /// Status moves on the user and Toxic from a Poison type always hit.
    fn accuracy_sure(&self, r: MonRef, mv: &Move) -> bool {
        let poison = self
            .dex
            .ids
            .types
            .poison
            .map(|p| self.has_type_of(r, p))
            .unwrap_or(false);
        (mv.target == Target::SelfTarget && mv.category == Category::Status)
            || (mv.id == "toxic" && poison)
    }

    /// The accuracy after the stages and the `ModifyAccuracy` modifiers
    /// (`None` when `sure` or the move can't miss).
    fn accuracy_value(
        &self,
        r: MonRef,
        t: MonRef,
        mv: &Move,
        base: Accuracy,
        sure: bool,
    ) -> Option<u32> {
        if sure {
            return None;
        }
        match base {
            Accuracy::Sure => None,
            Accuracy::Fixed(a) => Some(a),
            Accuracy::Stages(a0) => {
                let mut boost = self.accuracy_stage(r, t) as i32;
                if !self.ignores_evasion(r, mv) {
                    boost = (boost - self.evasion_stage(r, t) as i32).clamp(-6, 6);
                }
                let mut a = self.item_accuracy(r, t, mv, a0);
                if boost > 0 {
                    a = a * (3 + boost as u32) / 3;
                } else if boost < 0 {
                    a = a * 3 / (3 + (-boost) as u32);
                }
                Some(a)
            }
        }
    }

    /// Showdown's `runEvent('Accuracy', target, source)`: the target's Glaive
    /// Rush volatile, then `onAnyAccuracy` on the target's side and on its
    /// foes (No Guard). Every handler joins the sort, so equal ones draw.
    /// Whether the move can no longer miss.
    pub(super) fn accuracy_event(&mut self, r: MonRef, t: MonRef, mv: &Move) -> bool {
        use crate::abilities::{compare_mod, ModHandler};
        let mut hs: Small<(Option<MonRef>, ModHandler)> = Small::new();
        // The target's volatiles in the order they were added (Minimize's
        // can't miss a `minimize` move).
        let minimized = self.mon(t).vol.minimize;
        if minimized {
            hs.push((None, ModHandler::new(0, self.mon(t).speed, 2)));
        }
        if self.mon(t).vol.glaive_rush {
            hs.push((None, ModHandler::new(0, self.mon(t).speed, 2)));
        }
        for s in [t.0, 1 - t.0] {
            for slot in 0..self.sides[s].n_active {
                let Some(h) = self.active(s, slot).filter(|&h| self.mon(h).hp > 0) else {
                    continue;
                };
                if self.has_ability(h, crate::kinds::AbilityKind::NoGuard) {
                    hs.push((Some(h), ModHandler::new(0, self.mon(h).speed, 7)));
                }
            }
        }
        // The user's onSourceAccuracy (Lock-On) comes last.
        if let Some(h) = self.lock_on_source_accuracy_handler(r) {
            hs.push((None, h));
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        }
        self.accuracy_event_sure(r, t, mv)
    }

    /// The `Accuracy` event's result, without its sort: Glaive Rush, a
    /// `minimize` move on a minimized target and Lock-On let the move hit,
    /// and so does No Guard on the user or the target.
    pub(crate) fn accuracy_event_sure(&self, r: MonRef, t: MonRef, mv: &Move) -> bool {
        let volatile = self.mon(t).vol.glaive_rush
            || (self.mon(t).vol.minimize && mv.has(MoveFlag::Minimize))
            || self.lock_on_source_invulnerability(r, t);
        let no_guard = |h: MonRef| {
            self.active(h.0, h.1) == Some(h)
                && h.1 < self.sides[h.0].n_active
                && self.mon(h).hp > 0
                && self.has_ability(h, crate::kinds::AbilityKind::NoGuard)
        };
        volatile || no_guard(r) || no_guard(t)
    }

    /// Showdown's `StallMove` event: the stall volatile's success roll.
    pub(crate) fn stall_move(&mut self, r: MonRef) -> bool {
        let counter = self.mon(r).vol.stall_counter;
        if counter == 0 {
            return true;
        }
        let ok = self.rng.chance(1, counter as u32);
        if !ok {
            let m = self.mon_mut(r);
            m.vol.stall_counter = 0;
            m.vol.stall_duration = 0;
        }
        ok
    }

    /// `spreadMoveHit` for a status move: `runMoveEffects` per target, then
    /// self boosts.
    /// Returns the targets the move did something to.
    /// Whether a substitute takes this move's hit on `t`: not the user's
    /// own, and not a `bypasssub` move or an Infiltrator's.
    pub(crate) fn hits_substitute(&self, src: MonRef, t: MonRef, mv: &Move) -> bool {
        t != src
            && self.mon(t).vol.substitute > 0
            && !mv.has(MoveFlag::BypassSub)
            && !self.infiltrates(src)
            && !self.ally_puff_on_try_hit(src, &[t], self.dex.move_kind[mv.idx as usize])
    }

    /// `runMoveEffects` for the move's own (not secondary) effects: boosts,
    /// heal, status, volatile status. Returns Showdown's `didSomething` for
    /// this target, combined over the parts (`combineResults`). A failed
    /// status or a heal at full HP stops there, already reported (null).
    pub(crate) fn primary_effects(
        &mut self,
        src: MonRef,
        t: MonRef,
        mv: &Move,
        status_move: bool,
    ) -> Did {
        let effect = self.move_ref(mv);
        let mut did = Did::Nothing;
        if let Some(b) = mv.boosts {
            if !self.mon(t).fainted {
                // `boost` returns null when no stat changed.
                let ok = self.boost_listed(t, &b, Some(src), effect, false, false);
                did = did.max(if ok { Did::Done } else { Did::Null });
            }
        }
        if let Some((n, d)) = mv.heal {
            let m = self.mon(t);
            if !m.fainted {
                if m.hp >= m.maxhp {
                    let ti = self.ident(t);
                    log_line!(self, "|-fail|{ti}|heal");
                    if let Some(line) = self.last_move_line {
                        self.attr_move_line(line, "[still]");
                    }
                    return Did::Null;
                }
                let amount = ((m.maxhp as f64 * n as f64 / d as f64) + 0.5).floor() as u32;
                if self.heal(t, amount, "") == 0 {
                    // TryHeal refused it (Heal Block): the move fails here.
                    let si = self.ident(src);
                    log_line!(self, "|-fail|{si}");
                    if let Some(line) = self.last_move_line {
                        self.attr_move_line(line, "[still]");
                    }
                    return Did::Null;
                }
                did = did.max(Did::Done);
            }
        }
        if let Some(st) = mv.status {
            if !self.try_set_status(t, st, src, effect, status_move) {
                return Did::Null;
            }
            did = did.max(Did::Done);
        }
        if let Some(v) = mv
            .volatile_status
            .as_ref()
            .filter(|_| !self.skip_volatile_status(src, mv))
        {
            let added = match self.add_move_volatile_from_user(t, src, mv, v) {
                Some(added) => added,
                None => match self.add_move_volatile_with_source(t, src, v) {
                    Some(added) => added,
                    None => self.add_volatile(t, v, status_move),
                },
            };
            did = did.max(match added {
                Some(true) => Did::Done,
                Some(false) => Did::Fail,
                None => Did::Null,
            });
        }
        if mv.force_switch {
            did = did.max(if self.can_switch_side(t.0) {
                Did::Done
            } else {
                Did::Fail
            });
        }
        did
    }

    /// `spreadMoveHit` for a damaging move over all hits: damage for every
    /// target, then `-damage` lines, effects, self drops, secondaries and
    /// `DamagingHit` reactions; then faints, hit count and recoil.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn damaging_move_hit(
        &mut self,
        r: MonRef,
        targets: &[MonRef],
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        type_changed: bool,
        spread: bool,
        smart: bool,
    ) -> u32 {
        let hits = match self.hit_count(r, mv) {
            None => 1,
            Some((2, 5)) => self
                .rng
                .sample(&[2u8, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5]),
            Some((lo, hi)) if lo == hi => lo,
            Some((lo, hi)) => self.rng.random_range(lo as u32, hi as u32 + 1) as u8,
        };
        let effect = self.move_ref(mv);
        self.begin_damaging_move(mv, smart);
        self.active_move.hit_targets = Some(targets.iter().copied().collect());
        let kings_rock = self.kings_rock_flinch(r, mv);
        let hp_before: PerActive<u16> = targets.iter().map(|&t| self.mon(t).hp).collect();
        let mut total_damage = 0u32;
        let mut self_dropped = false;
        let mut hit_count = 0;
        // The targets the last hit damaged (Showdown `moveDamage`).
        let mut last_damaged = ActiveList::new();
        let mut last_dealt: PerActive<(MonRef, u32)> = PerActive::new();
        // The targets a substitute shielded on the last hit (Showdown nulls them).
        let mut last_subbed = ActiveList::new();
        let mut accuracy_break = false;
        // Every target some hit reached (smart targeting counts each once).
        let mut reached_any = ActiveList::new();
        // A move that can be used asleep (Snore, or any move Sleep Talk called) goes on; any other
        // multi-hit move stops when its user falls asleep between hits (Effect Spore).
        let sleep_usable = self.sleep_usable(mv.idx)
            || self
                .active_move
                .call
                .is_some_and(|c| c.user == r && self.sleep_usable(c.source));
        for hit in 1..=hits {
            if hit > 1 && self.mon(r).status == Status::Slp && !sleep_usable {
                break;
            }
            if targets.iter().all(|&t| self.mon(t).hp == 0) {
                break;
            }
            self.move_hit = hit;
            // Smart targeting: hit n strikes target n.
            let hit_targets: &[MonRef] = if smart && targets.len() > 1 {
                &targets[hit as usize - 1..hit as usize]
            } else {
                targets
            };
            // Dragon Darts names its target on the move line, then each later
            // dart gets an `-anim` line, which takes the move line's place.
            if mv.smart_target {
                if let Some(&t) = hit_targets.first() {
                    if hit > 1 {
                        let (ri, ti) = (self.ident(r), self.ident(t));
                        self.last_move_line = Some(self.log.len());
                        log_line!(self, "|-anim|{ri}|{}|{ti}", mv.name);
                    } else if let Some(line) = self.last_move_line {
                        self.retarget_move_line(line, t);
                    }
                }
            }
            // multiaccuracy (Triple Axel): each hit after the first rolls
            // again, and a miss ends the move without a message.
            if self.has_multiaccuracy(r, kind)
                && hit > 1
                && !self.multiaccuracy_hits(r, targets[0], mv)
            {
                // `targetsCopy` was just reset to every target, so gotAttacked
                // below also reaches one a substitute shielded on the last hit.
                accuracy_break = true;
                break;
            }
            hit_count += 1;
            self.begin_hit(hit_targets);
            // getSpreadDamage, then spreadDamage.
            let reached: ActiveList = hit_targets
                .iter()
                .copied()
                .filter(|&t| self.mon(t).hp > 0)
                .collect();
            self.run_try_hit(hit_targets, &reached, kind);
            // TryPrimaryHit: a substitute takes the hit. The target then
            // drops out of the damage and the effects on it, but still counts
            // as reached for the user's own effects and the secondary rolls.
            let mut subbed = ActiveList::new();
            for &t in &reached {
                self.run_source_try_primary_hit(r, t, mv, move_type);
                if self.hits_substitute(r, t, mv) {
                    self.hit_substitute(r, t, mv, kind, move_type, type_changed, spread);
                    subbed.push(t);
                }
            }
            let live: ActiveList = reached
                .iter()
                .copied()
                .filter(|t| !subbed.contains(t))
                .collect();
            let damages: PerActive<u32> = live
                .iter()
                .map(|&t| self.move_damage(r, t, mv, kind, move_type, type_changed, spread))
                .collect();
            let mut damaged = ActiveList::new();
            let mut dealt_now: PerActive<(MonRef, u32)> = PerActive::new();
            for (&t, &dmg) in live.iter().zip(&damages) {
                let dealt = self.apply_move_damage(t, dmg, "");
                // Testing the calculation: the target's HP after this hit.
                if let Some(v) = self.damage_samples.as_mut() {
                    if let Some(x) = v.iter_mut().rev().find(|x| x.target == t) {
                        x.hp_after
                            .get_or_insert(self.sides[t.0].mons[t.1].hp as u32);
                    }
                }
                dealt_now.push((t, dealt));
                self.note_move_damage(t, r);
                self.note_hit_damage(t, dealt);
                total_damage += dealt;
                if let (Some((n, d)), true) = (mv.drain, dealt > 0) {
                    let amount = ((dealt as f64 * n as f64 / d as f64) + 0.5).floor() as u32;
                    if amount > 0 {
                        let ti = self.ident(t);
                        // Liquid Ooze takes the heal as `runEvent` hands it on:
                        // Big Root's chainModify only applies at the end.
                        if !self.liquid_ooze_on_source_try_heal(r, t, amount) {
                            let amount = self.drain_heal_mod(r, amount);
                            self.heal(r, amount, &format!("|[from] drain|[of] {ti}"));
                        }
                    }
                }
                damaged.push(t);
            }
            for &t in &damaged {
                self.primary_effects(r, t, mv, false);
                self.move_on_hit(r, t, mv, kind);
                // The move's onHit: Steel Roller ends the terrain.
                if kind == MoveKind::SteelRoller {
                    self.clear_terrain();
                }
                match kind {
                    MoveKind::Fling => self.fling_on_hit(t),
                    MoveKind::ShellSideArm => self.shell_side_arm_on_hit(r, t, mv),
                    _ => {}
                }
                // The Hit event's handlers by effect type: a volatile
                // (Beak Blast) before an ability (Anger Point).
                self.beak_blast_on_hit(r, t, mv);
                self.run_hit(t);
            }
            if mv.self_switch && !reached.is_empty() {
                self.set_self_switch(r, mv.idx);
            }
            // selfDrops: Double Shock's `self.onHit` turns Electric into "???",
            // Burn Up's Fire.
            if matches!(kind, MoveKind::DoubleShock | MoveKind::BurnUp)
                && !reached.is_empty()
                && self.mon(r).hp > 0
            {
                let (lost, name) = if kind == MoveKind::BurnUp {
                    ("Fire", "Burn Up")
                } else {
                    ("Electric", "Double Shock")
                };
                let lost = self.dex.type_id(lost);
                let typeless = self.dex.ids.types.typeless.unwrap_or(NO_TYPE);
                let types = self
                    .base_types_of(r)
                    .map(|t| if Some(t) == lost { typeless } else { t });
                self.set_type(r, types);
                let shown: Vec<&str> = self
                    .types_of(r)
                    .iter()
                    .filter(|&&t| t != NO_TYPE)
                    .map(|&t| self.dex.type_names[t as usize].as_str())
                    .collect();
                let (ri, list) = (self.ident(r), shown.join("/"));
                log_line!(self, "|-start|{ri}|typechange|{list}|[from] move: {name}");
            }
            if let (Some(b), false) = (mv.self_boosts, self_dropped) {
                let _ = self.rng.random(100);
                self.boost_listed(r, &b, Some(r), effect, false, true);
                if mv.multihit.is_none() {
                    self_dropped = true;
                }
            }
            // selfDrops: `self.volatileStatus` (Hyper Beam's mustrecharge).
            if let (Some(v), false) = (&mv.self_volatile, reached.is_empty()) {
                self.add_self_volatile(r, v);
            }
            // Secondaries roll for every target reached; behind a substitute
            // only their effect on the user applies.
            for &t in &reached {
                let sub = subbed.contains(&t);
                // ModifySecondaries (Shield Dust) filters before any roll; a
                // substitute made the target null, so its handlers don't run.
                let shielded = !sub && self.shield_dust_on_modify_secondaries(t);
                for sec in mv
                    .secondaries
                    .iter()
                    .filter(|s| !shielded || s.self_boosts.is_some())
                {
                    let roll = self.rng.random(100);
                    if sec.chance.map(|c| roll < c as u32).unwrap_or(true) {
                        if !sub {
                            self.apply_secondary(r, t, sec, effect);
                        } else if let Some(b) = sec.self_boosts {
                            self.boost_listed(r, &b, Some(r), effect, true, true);
                        }
                    }
                }
                if kings_rock && !shielded && self.rng.random(100) < 10 && !sub {
                    self.add_volatile(t, "flinch", false);
                }
            }
            if mv.force_switch {
                for &t in &damaged {
                    self.force_switch_target(r, t);
                }
            }
            let user_hp = self.mon(r).hp;
            self.active_move.hit_dealt.clear();
            self.active_move.hit_dealt.extend_from_slice(&dealt_now);
            self.run_damaging_hit(r, &damaged, mv, move_type);
            self.move_on_after_hit(r, &damaged, mv, kind);
            // AfterHit: Knock Off removes the target's item; Ice Spinner ends
            // the terrain.
            if kind == MoveKind::KnockOff {
                for &t in &damaged {
                    self.knock_off(r, t);
                }
            }
            if kind == MoveKind::IceSpinner {
                for _ in &damaged {
                    self.clear_terrain();
                }
            }
            // Stone Axe: Stealth Rock on the foe's side (silently if there).
            if kind == MoveKind::StoneAxe && !mv.sheer_force {
                for _ in &damaged {
                    if !self.sides[1 - r.0].conds.stealth_rock {
                        self.add_stealth_rock(1 - r.0);
                    }
                }
            }
            for _ in &damaged {
                self.ceaseless_edge_on_after_hit(r, kind, mv.sheer_force);
            }
            last_damaged.clone_from(&damaged);
            // moveDamage for gotAttacked: each hit replaces it, but a
            // smart-targeting move keeps one entry per target.
            if smart {
                for &(t, d) in &dealt_now {
                    match last_dealt.iter_mut().find(|e| e.0 == t) {
                        Some(e) => e.1 = d,
                        None => last_dealt.push((t, d)),
                    }
                }
            } else {
                last_dealt.clone_from(&dealt_now);
            }
            last_subbed = subbed;
            for &t in &reached {
                if !reached_any.contains(&t) {
                    reached_any.push(t);
                }
            }
            // EmergencyExit for a user that Rough Skin or Rocky Helmet took to
            // half HP or less.
            if !damaged.is_empty() {
                self.emergency_exit(r, user_hp);
            }
            self.run_update();
            if self.mon(r).hp == 0 && targets.len() == 1 {
                break;
            }
        }
        let user_down = self.mon(r).hp == 0;
        self.faint_messages_check(user_down);
        // gotAttacked: each target the move damaged counts every hit; with
        // smart targeting, each target reached counts once (a substitute's
        // too).
        let mut shielded_too = last_damaged.clone();
        if accuracy_break {
            shielded_too.extend(
                last_subbed
                    .iter()
                    .copied()
                    .filter(|t| !last_damaged.contains(t)),
            );
        }
        let attacked = if smart { &reached_any } else { &shielded_too };
        let count = if smart { 1 } else { hit_count };
        for &t in attacked {
            if t != r {
                let m = self.mon_mut(t);
                m.times_attacked = m.times_attacked.saturating_add(count);
                let dealt = last_dealt.iter().find(|e| e.0 == t).map_or(0, |e| e.1);
                self.got_attacked(t, r, mv.idx, dealt);
            }
        }
        // No hit count for a smart-targeting move.
        // Champions: no hit count for a Parental Bond move that landed once.
        if mv.multihit.is_some()
            && hit_count > 0
            && !mv.smart_target
            && !(self.active_move.pbond && self.move_hit == 1)
        {
            let first = self.ident(targets[0]);
            log_line!(self, "|-hitcount|{first}|{hit_count}");
        }
        if total_damage > 0 {
            self.recoil(r, mv, kind, total_damage);
        }
        if hit_count > 0 && mv.ohko.is_some() && self.mon(targets[0]).hp == 0 {
            self.add("|-ohko");
        }
        if hit_count > 0 {
            self.run_update();
            // Smart targeting shows the move every target.
            let shown: ActiveList = targets
                .iter()
                .copied()
                .filter(|t| smart || !last_subbed.contains(t))
                .collect();
            self.run_after_move_secondary(r, &shown, mv);
            // EmergencyExit for targets that fell to half HP or less. With
            // smart targeting, Showdown's damage list keeps only the second.
            for (i, &t) in targets.iter().enumerate() {
                if smart && i == 0 {
                    continue;
                }
                self.emergency_exit(t, hp_before[i]);
            }
        }
        total_damage
    }

    /// Substitute's `onTryPrimaryHit`: the substitute takes the damage (at
    /// most its HP) and breaks at 0. Recoil and drain follow from the
    /// damage it took, then `AfterSubDamage`: the move's own, then the
    /// target's Air Balloon.
    #[allow(clippy::too_many_arguments)]
    fn hit_substitute(
        &mut self,
        r: MonRef,
        t: MonRef,
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        type_changed: bool,
        spread: bool,
    ) {
        let dmg = self.move_damage(r, t, mv, kind, move_type, type_changed, spread);
        let sub = self.mon(t).vol.substitute as u32;
        let dmg = dmg.min(sub);
        self.mon_mut(t).vol.substitute = (sub - dmg) as u16;
        let ti = self.ident(t);
        if dmg == sub {
            if mv.ohko.is_some() {
                self.add("|-ohko");
            }
            log_line!(self, "|-end|{ti}|Substitute");
        } else {
            log_line!(self, "|-activate|{ti}|move: Substitute|[damage]");
        }
        if dmg > 0 {
            self.recoil(r, mv, kind, dmg);
        }
        if let Some((n, d)) = mv.drain {
            // Rounded up here, unlike a drain from the Pokémon's own HP.
            let amount = (dmg * n).div_ceil(d);
            if amount > 0 && !self.liquid_ooze_on_source_try_heal(r, t, amount) {
                let amount = self.drain_heal_mod(r, amount);
                self.heal(r, amount, &format!("|[from] drain|[of] {ti}"));
            }
        }
        match kind {
            MoveKind::ShellSideArm => self.shell_side_arm_on_hit(r, t, mv),
            MoveKind::IceSpinner if self.mon(r).hp > 0 => self.clear_terrain(),
            MoveKind::SteelRoller => self.clear_terrain(),
            MoveKind::StoneAxe if self.mon(r).hp > 0 && !mv.sheer_force => {
                if !self.sides[1 - r.0].conds.stealth_rock {
                    self.add_stealth_rock(1 - r.0);
                }
            }
            k if self.mon(r).hp > 0 => self.ceaseless_edge_on_after_hit(r, k, mv.sheer_force),
            _ => {}
        }
        if self.item_kind(t) == Some(crate::kinds::ItemKind::AirBalloon) {
            log_line!(self, "|-enditem|{ti}|Air Balloon");
            self.mon_mut(t).item = None;
            self.run_after_use_item(t);
        }
    }

    /// `TryHit` on the target: Dry Skin drinks Water moves.
    pub(crate) fn ability_on_try_hit(&mut self, src: MonRef, t: MonRef, move_type: TypeId) -> bool {
        if self.has_ability(t, AbilityKind::DrySkin) && Some(move_type) == self.dex.ids.types.water
        {
            let amount = (self.mon(t).maxhp as u32 / 4).max(1);
            let si = self.ident(src);
            if self.heal(t, amount, &format!("|[from] ability: Dry Skin|[of] {si}")) == 0 {
                let ti = self.ident(t);
                log_line!(self, "|-immune|{ti}|[from] ability: Dry Skin");
            }
            return true;
        }
        false
    }

    /// The move's own `Try` (before `PrepareHit`). `Some(result)` stops the
    /// move with that result, its messages written.
    pub(crate) fn move_on_try(
        &mut self,
        r: MonRef,
        kind: MoveKind,
        target: Option<MonRef>,
    ) -> Option<Option<bool>> {
        self.active_move.mover = Some(r);
        let ri = self.ident(r);
        match kind {
            MoveKind::Rest => {
                let m = self.mon(r);
                if m.status == Status::Slp {
                    self.fail_move(r);
                    return Some(Some(false));
                }
                if m.hp >= m.maxhp {
                    log_line!(self, "|-fail|{ri}|heal");
                    return Some(Some(false));
                }
                if matches!(self.ability_id(r), "insomnia" | "vitalspirit") {
                    let name = self.ability_name(r);
                    log_line!(self, "|-fail|{ri}|[from] ability: {name}|[of] {ri}");
                    return Some(Some(false));
                }
                None
            }
            // onTryHit on its user: fails without a Pokémon to switch in.
            MoveKind::HealingWish if !self.can_switch_side(r.0) => {
                self.fail_move(r);
                Some(Some(false))
            }
            // onTry: Stockpile stops at 3 layers; Spit Up and Swallow need one.
            MoveKind::Stockpile if self.mon(r).vol.stockpile >= 3 => {
                self.fail_move(r);
                Some(Some(false))
            }
            MoveKind::SpitUp | MoveKind::Swallow if self.mon(r).vol.stockpile == 0 => {
                self.fail_move(r);
                Some(Some(false))
            }
            // onTry: one Future Sight per slot; the hit comes two turns on.
            MoveKind::FutureSight => {
                let t = target?;
                if self.slot_conds[t.0][t.1].future.is_some() {
                    self.fail_move(r);
                    return Some(Some(false));
                }
                let ending_turn = self.turn.saturating_sub(1) + 2;
                self.slot_conds[t.0][t.1].future = Some(FutureState {
                    ending_turn,
                    source: r,
                });
                log_line!(self, "|-start|{ri}|move: Future Sight");
                // NOT_FAIL: the move counts as done.
                Some(Some(true))
            }
            // onTry: Snore needs a sleeping user.
            MoveKind::Snore if self.mon(r).status != Status::Slp => {
                self.fail_move(r);
                Some(Some(false))
            }
            // onTry: Ingrain or Smack Down keeps it down.
            MoveKind::MagnetRise if self.mon(r).vol.ingrain || self.mon(r).vol.smack_down => {
                self.fail_move(r);
                Some(Some(false))
            }
            _ => None,
        }
    }

    /// The move's own `TryHit` in `spreadMoveHit`, on the hit's targets:
    /// Raging Bull breaks the screens on the target's side, substitute or
    /// not; Poltergeist names the item; Uproar wakes every sleeping Pokémon
    /// on the field, the target's side first at each slot; Psychic Fangs
    /// and Brick Break break the screens of each target still standing
    /// (`reached`).
    pub(crate) fn run_try_hit(&mut self, targets: &[MonRef], reached: &[MonRef], kind: MoveKind) {
        if kind == MoveKind::ScreenBreak {
            for &t in reached {
                self.break_screens(t.0);
            }
            return;
        }
        let Some(&t) = targets.first() else { return };
        match kind {
            MoveKind::RagingBull => self.break_screens(t.0),
            MoveKind::Poltergeist => {
                if let Some(item) = self.mon(t).item {
                    let (ti, name) = (self.ident(t), self.dex.item(item).name.clone());
                    log_line!(self, "|-activate|{ti}|move: Poltergeist|{name}");
                }
            }
            MoveKind::Uproar => {
                for i in 0..self.sides[t.0].n_active.max(self.sides[1 - t.0].n_active) {
                    for s in [t.0, 1 - t.0] {
                        if let Some(a) = self.active(s, i) {
                            if self.mon(a).status == Status::Slp {
                                self.cure_status(a);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// `MoveFail` (the move reached `trySpreadMoveHit` and hit nothing):
    /// Steel Beam and High Jump Kick hurt the user, then EmergencyExit.
    pub(crate) fn run_move_fail(&mut self, r: MonRef, target: MonRef, mv: &Move, kind: MoveKind) {
        let hp_before = self.mon(r).hp;
        let maxhp = self.mon(r).maxhp as u32;
        match kind {
            // Math.round(maxhp / 2), unless the move is multi-hit.
            MoveKind::SteelBeam if mv.multihit.is_none() => {
                self.apply_damage(r, maxhp.div_ceil(2), "|[from] steelbeam");
            }
            // baseMaxhp / 2, floored by clampIntRange; the condition named
            // after the move doesn't exist, so the line shows its id.
            MoveKind::Crash => {
                self.apply_damage(r, (maxhp / 2).max(1), &format!("|[from] {}", mv.id));
            }
            _ => {}
        }
        if r != target && mv.category != Category::Status {
            self.emergency_exit(r, hp_before);
        }
    }

    /// Round's `onTry`: the first other queued Round moves to the front of
    /// the queue (`prioritizeAction`), to be used with doubled power.
    pub(crate) fn round_on_try(&mut self) {
        let round = self.dex.ids.moves.round;
        let at = self.queue.iter().position(|a| {
            matches!(a.kind, ActionKind::Move { slot, id, .. } if slot < crate::state::RECHARGE && Some(id) == round)
        });
        if let Some(mut a) = at.and_then(|i| self.queue.remove(i)) {
            a.order = 3;
            if let ActionKind::Move { side, mon, .. } = a.kind {
                self.turn_state.round_actions.push((side, mon));
            }
            self.queue.push_front(a);
        }
    }

    /// The `FractionalPriority` event when an action is made, in tenths: the
    /// handlers run by priority, each seeing the value so far. Quick Draw
    /// (-1) gives a damaging move 30% to go first; Quick Claw (-2) acts only
    /// while the value is not above 0.
    pub(crate) fn run_fractional_priority(&mut self, r: MonRef, status_move: bool) -> i32 {
        let mut frac = self.stall_on_fractional_priority(r);
        if self.has_ability(r, AbilityKind::QuickDraw) && !status_move && self.rng.chance(3, 10) {
            let ident = self.ident(r);
            log_line!(self, "|-activate|{ident}|ability: Quick Draw");
            frac = 1;
        }
        if self.item_kind(r) == Some(ItemKind::QuickClaw) && frac <= 0 && self.rng.chance(1, 5) {
            let ident = self.ident(r);
            log_line!(self, "|-activate|{ident}|item: Quick Claw");
            frac = 1;
        }
        frac
    }

    /// `checkMoveBypassesProtect`'s `HitProtect` event: a contact move of a
    /// Piercing Drill or Unseen Fist user goes through, marking the target.
    pub(crate) fn move_bypasses_protect(&mut self, src: MonRef, t: MonRef, mv: &Move) -> bool {
        if !self.has_ability(src, AbilityKind::BypassProtect) || !mv.has(MoveFlag::Contact) {
            return false;
        }
        if !self.active_move.bypass.contains(&t) {
            self.active_move.bypass.push(t);
        }
        true
    }

    /// `modifyDamage` after ModifyDamage: a hit through Protect names the
    /// ability and breaks the protection; it does a quarter.
    pub(crate) fn hit_protect_modify_damage(&mut self, src: MonRef, tgt: MonRef) -> bool {
        if !self.active_move.bypass.contains(&tgt) {
            return false;
        }
        let (si, ti) = (self.ident(src), self.ident(tgt));
        let name = self.ability_name(src);
        log_line!(self, "|-ability|{si}|{name}");
        log_line!(self, "|-zbroken|{ti}");
        true
    }

    /// Aura Wheel's `onTry`: only Morpeko can use it.
    pub(crate) fn aura_wheel_on_try(&mut self, r: MonRef, line: usize) -> bool {
        if self.dex.species(self.mon(r).species).base_species == "Morpeko" {
            return false;
        }
        self.attr_move_line(line, "[still]");
        let ident = self.ident(r);
        log_line!(self, "|-fail|{ident}|move: Aura Wheel");
        self.add(
            "|-hint|Only a Pokemon whose form is Morpeko or Morpeko-Hangry can use this move.",
        );
        true
    }

    /// The move's own TryHit (`spreadMoveHit`'s singleEvent), before a
    /// substitute: true when it failed (announced).
    pub(crate) fn move_on_try_hit(&mut self, r: MonRef, t: MonRef, kind: MoveKind) -> bool {
        let failed = match kind {
            MoveKind::Disable => self
                .mon(t)
                .last_move
                .is_none_or(|lm| self.dex.mv(lm).id == "struggle"),
            MoveKind::Curse => self.is_ghost(r) && self.mon(t).vol.curse,
            // Electrify fails on a target that has moved this turn.
            MoveKind::Electrify => !self.has_queued_move(t) && self.mon(t).active_turns > 0,
            MoveKind::GastroAcid => self.cant_suppress(t),
            MoveKind::LockOn => self.mon(r).vol.lock_on_duration > 0,
            // Simple Beam reads the raw ability (`target.ability`).
            MoveKind::SimpleBeam => {
                let a = self.dex.ability(self.mon(t).ability);
                self.cant_suppress(t) || a.id == "simple" || a.id == "truant"
            }
            MoveKind::Entrainment | MoveKind::RolePlay | MoveKind::WorrySeed => {
                let flag = |b: &Battle, who: MonRef, f: &str| {
                    b.dex
                        .ability(b.mon(who).ability)
                        .flags
                        .iter()
                        .any(|x| x == f)
                };
                let id = |b: &Battle, who: MonRef| b.dex.ability(b.mon(who).ability).id.clone();
                let tid = id(self, t);
                (kind != MoveKind::RolePlay && flag(self, t, "cantsuppress"))
                    || match kind {
                        MoveKind::Entrainment => {
                            t == r
                                || tid == id(self, r)
                                || tid == "truant"
                                || flag(self, r, "noentrain")
                        }
                        MoveKind::RolePlay => {
                            tid == id(self, r)
                                || flag(self, t, "failroleplay")
                                || flag(self, r, "cantsuppress")
                        }
                        _ => false,
                    }
            }
            _ => false,
        };
        if failed {
            self.fail_move(r);
        }
        failed
    }

    /// Hit step 0 (Invulnerability) is skipped: Toxic from a Poison type.
    pub(crate) fn skips_invulnerability(&self, r: MonRef, kind: MoveKind) -> bool {
        kind == MoveKind::Toxic
            && self
                .dex
                .type_id("Poison")
                .is_some_and(|p| self.has_type_of(r, p))
    }

    /// The start of `hitStepMoveHitLoop` for a damaging move: a new active
    /// move, with no hit data yet.
    pub(crate) fn begin_damaging_move(&mut self, mv: &Move, smart: bool) {
        let m = &mut self.active_move;
        m.crit.clear();
        m.hit_damage.clear();
        m.total_damage = 0;
        m.multihit = mv.multihit.is_some();
        m.smart = smart;
    }

    /// The start of one hit: its targets' `moveDamage` starts at 0 (a
    /// substitute's hit, or no damage).
    pub(crate) fn begin_hit(&mut self, targets: &[MonRef]) {
        for &t in targets {
            match self
                .active_move
                .hit_damage
                .iter_mut()
                .find(|(x, _)| *x == t)
            {
                Some(e) => e.1 = 0,
                None => self.active_move.hit_damage.push((t, 0)),
            }
        }
    }

    /// The damage a hit dealt to a target.
    pub(crate) fn note_hit_damage(&mut self, t: MonRef, dealt: u32) {
        match self
            .active_move
            .hit_damage
            .iter_mut()
            .find(|(x, _)| *x == t)
        {
            Some(e) => e.1 = dealt,
            None => self.active_move.hit_damage.push((t, dealt)),
        }
        self.active_move.total_damage += dealt;
    }

    /// `getDamage`'s `moveHit.crit` for this target.
    pub(crate) fn note_hit_crit(&mut self, t: MonRef, crit: bool) {
        self.active_move.crit.retain(|&x| x != t);
        if crit {
            self.active_move.crit.push(t);
        }
    }

    /// `runEvent('Hit')` on a target of a damaging move: Anger Point maxes
    /// Attack after a critical hit.
    pub(crate) fn run_hit(&mut self, t: MonRef) {
        if !self.has_ability(t, AbilityKind::AngerPoint)
            || self.mon(t).hp == 0
            || !self.active_move.crit.contains(&t)
        {
            return;
        }
        let mut b = [0i8; 7];
        b[Boost::Atk as usize] = 12;
        let a = self.mon(t).ability;
        self.boost(t, &b, Some(t), EffectRef::Ability(a), false, false);
    }

    /// `ModifySecondaries`: Shield Dust drops every secondary without a
    /// `self` part, before any chance roll.
    pub(crate) fn shield_dust_on_modify_secondaries(&self, t: MonRef) -> bool {
        self.has_ability(t, AbilityKind::ShieldDust)
    }
}
