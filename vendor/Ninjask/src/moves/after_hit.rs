//! After a hit: secondary effects, self drops, recoil, self-switching and
//! `AfterMoveSecondary`.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::{Move, Secondary};
use crate::kinds::AbilityKind;
use crate::kinds::ItemKind;
use crate::kinds::{EffectRef, MoveKind};
use crate::state::*;

/// One `AfterMoveSecondary` handler on a hit target.
#[derive(Clone, Copy, Debug)]
enum SecondaryHandler {
    /// The freeze status.
    Thaw,
    EjectButton,
    Pickpocket,
    RedCard,
    Berserk,
}

/// `comparePriority` for (holder, handler, priority, speed, subOrder).
fn compare_secondary_handlers(
    a: &(MonRef, SecondaryHandler, i32, i32, i32),
    b: &(MonRef, SecondaryHandler, i32, i32, i32),
) -> i64 {
    let d = (b.2 - a.2) as i64;
    if d != 0 {
        return d;
    }
    let d = (b.3 - a.3) as i64;
    if d != 0 {
        return d;
    }
    (a.4 - b.4) as i64
}

impl Battle {
    /// `applyRecoilDamage`, then EmergencyExit for the user.
    pub(super) fn recoil(&mut self, r: MonRef, mv: &Move, kind: MoveKind, dealt: u32) {
        if self.mind_blown_recoil(r, kind) {
            return;
        }
        let hp_before = self.mon(r).hp;
        if kind == MoveKind::Struggle {
            // A quarter of max HP as direct damage: Rock Head doesn't stop it.
            let amount = ((self.mon(r).maxhp as f64 / 4.0).round() as u32).max(1);
            self.direct_damage(r, amount, "|[from] recoil");
            self.emergency_exit(r, hp_before);
        } else if let Some((n, d)) = mv.recoil {
            if self.blocks_recoil(r) {
                // Rock Head stops it inside the Damage event, which still sorts.
                if self.mon(r).hp > 0 {
                    self.damage_event_sort(r);
                }
            } else {
                let amount = (((dealt as f64) * n as f64 / d as f64) + 0.5)
                    .floor()
                    .max(1.0) as u32;
                self.apply_damage(r, amount, "|[from] Recoil");
            }
            self.emergency_exit(r, hp_before);
        }
    }

    /// A volatile a move puts on its user (`self.volatileStatus`). Unless
    /// already there, `addVolatile` runs TryAddVolatile first, whose sort
    /// draws (Sweet Veil on the user and an ally).
    pub(super) fn add_self_volatile(&mut self, r: MonRef, v: &str) {
        if self.mon(r).hp == 0 {
            return;
        }
        let ident = self.ident(r);
        match v {
            "mustrecharge" if self.mon(r).vol.must_recharge == 0 => {
                self.try_add_volatile_sort(r);
                self.mon_mut(r).vol.must_recharge = 2;
                self.stamp_volatile(r, ResidualVol::MustRecharge);
                log_line!(self, "|-mustrecharge|{ident}");
            }
            "roost" if !self.mon(r).vol.roost => {
                self.try_add_volatile_sort(r);
                self.mon_mut(r).vol.roost = true;
                self.stamp_volatile(r, ResidualVol::Roost);
                log_line!(self, "|-singleturn|{ident}|move: Roost");
            }
            "uproar" => self.add_uproar(r),
            "glaiverush" if !self.mon(r).vol.glaive_rush => {
                self.try_add_volatile_sort(r);
                self.mon_mut(r).vol.glaive_rush = true;
                log_line!(self, "|-singlemove|{ident}|Glaive Rush|[silent]");
            }
            _ => self.add_locked_move_volatile(r, v),
        }
    }

    /// The user of a self-switching move will switch out after the move.
    pub(crate) fn set_self_switch(&mut self, r: MonRef, move_id: MoveId) {
        if self.mon(r).hp > 0 && self.can_switch_side(r.0) {
            let m = self.mon_mut(r);
            m.switch_flag = true;
            m.switch_from_move = Some(move_id);
        }
    }

    /// AfterMoveSecondary on the hit targets. Their handlers run in
    /// Showdown's speed order, ties shuffled: of two Eject Buttons hit by one
    /// spread move, the faster holder's activates. Per target they are
    /// collected as `findPokemonEventHandlers` does (status, ability, item),
    /// except Eject Button, which comes right after the status: its priority
    /// (2) is the only nonzero one, so it never ties with the others.
    pub(super) fn run_after_move_secondary(&mut self, src: MonRef, targets: &[MonRef], mv: &Move) {
        let mut handlers: Small<(MonRef, SecondaryHandler, i32, i32, i32)> = Small::new();
        for &t in targets {
            let speed = self.mon(t).speed;
            if self.mon(t).status == Status::Frz {
                handlers.push((t, SecondaryHandler::Thaw, 0, speed, 0));
            }
            if self.item_kind(t) == Some(ItemKind::EjectButton) && !mv.has(MoveFlag::FutureMove) {
                handlers.push((t, SecondaryHandler::EjectButton, 2, speed, 8));
            }
            if self.has_ability(t, AbilityKind::Pickpocket) {
                handlers.push((t, SecondaryHandler::Pickpocket, 0, speed, 7));
            }
            if self.item_kind(t) == Some(ItemKind::RedCard) {
                handlers.push((t, SecondaryHandler::RedCard, 0, speed, 8));
            }
            if self.has_ability(t, AbilityKind::Berserk) {
                handlers.push((t, SecondaryHandler::Berserk, 0, speed, 7));
            }
        }
        self.speed_sort_by(&mut handlers, compare_secondary_handlers);
        for &(t, h, ..) in handlers.iter() {
            match h {
                // The freeze status thaws on a thawsTarget move.
                SecondaryHandler::Thaw => {
                    if mv.thaws_target && self.mon(t).status == Status::Frz {
                        self.cure_status(t);
                    }
                }
                SecondaryHandler::EjectButton => self.eject_button(src, t),
                SecondaryHandler::Pickpocket => self.pickpocket(src, t, mv),
                SecondaryHandler::RedCard => self.red_card(src, t, mv),
                SecondaryHandler::Berserk => self.berserk_on_after_move_secondary(src, t),
            }
        }
    }

    /// `afterMoveSecondaryEvent` after a status move: of the handlers, only
    /// Berserk's acts on a status move (it marks the berries ready).
    pub(crate) fn run_after_move_secondary_status(
        &mut self,
        src: MonRef,
        targets: &[MonRef],
        mv: &Move,
    ) {
        self.begin_damaging_move(mv, false);
        let mut hs: Small<_> = Small::new();
        for &t in targets
            .iter()
            .filter(|&&t| !self.hits_substitute(src, t, mv))
        {
            if self.has_ability(t, AbilityKind::Berserk) {
                hs.push((t, SecondaryHandler::Berserk, 0, self.mon(t).speed, 7));
            }
        }
        self.speed_sort_by(&mut hs, compare_secondary_handlers);
        for &(t, ..) in hs.iter() {
            self.berserk_on_after_move_secondary(src, t);
        }
    }

    /// Berserk's `onAfterMoveSecondary`: +1 Special Attack when the move
    /// took the holder from above half its max HP to half or less.
    pub(crate) fn berserk_on_after_move_secondary(&mut self, src: MonRef, t: MonRef) {
        // A fainted holder is no longer active: its ability has no handlers.
        if !self.has_ability(t, AbilityKind::Berserk) || self.mon(t).hp == 0 {
            return;
        }
        self.mon_mut(t).ability_state.checked_berserk = Some(true);
        if src == t || self.active_move.total_damage == 0 {
            return;
        }
        let damage = if self.active_move.multihit && !self.active_move.smart {
            self.active_move.total_damage
        } else {
            self.active_move
                .hit_damage
                .iter()
                .find(|(x, _)| *x == t)
                .map_or(0, |e| e.1)
        };
        let (hp, maxhp) = (self.mon(t).hp as u32, self.mon(t).maxhp as u32);
        if hp * 2 <= maxhp && (hp + damage) * 2 > maxhp {
            let mut b = [0i8; 7];
            b[Boost::Spa as usize] = 1;
            let a = self.mon(t).ability;
            self.boost(t, &b, Some(t), EffectRef::Ability(a), false, false);
        }
    }

    /// Eject Button (Champions): the holder switches out after being hit.
    fn eject_button(&mut self, src: MonRef, t: MonRef) {
        if t == src
            || self.mon(t).hp == 0
            || self.item_kind(t) != Some(crate::kinds::ItemKind::EjectButton)
        {
            return;
        }
        // Not when it's already being forced out (Circle Throw, Dragon Tail).
        if !self.can_switch_side(t.0) || self.mon(t).force_switch_flag {
            return;
        }
        if self
            .all_active()
            .iter()
            .any(|&a| self.mon(a).switch_flag && self.mon(a).switch_from_move.is_none())
        {
            return;
        }
        // The flag is set before the item is used (Symbiosis checks it).
        let (old_flag, old_from) = (self.mon(t).switch_flag, self.mon(t).switch_from_move);
        let m = self.mon_mut(t);
        m.switch_flag = true;
        m.switch_from_move = None;
        if !self.use_item(t) {
            let m = self.mon_mut(t);
            m.switch_flag = old_flag;
            m.switch_from_move = old_from;
        }
    }

    /// Emergency Exit and Wimp Out (Champions): switch out after falling to
    /// half HP or less from above half.
    pub(crate) fn emergency_exit(&mut self, t: MonRef, hp_before: u16) {
        let m = self.mon(t);
        if !self.has_ability(t, crate::kinds::AbilityKind::EmergencyExit) || m.hp == 0 {
            return;
        }
        let (hp, before, maxhp) = (m.hp as u32, hp_before as u32, m.maxhp as u32);
        if hp * 2 > maxhp || before * 2 <= maxhp {
            return;
        }
        // (The Champions version leaves other pending switches alone.)
        if !self.can_switch_side(t.0) || m.switch_flag || m.force_switch_flag {
            return;
        }
        let m = self.mon_mut(t);
        m.switch_flag = true;
        m.switch_from_move = None;
        let ti = self.ident(t);
        let name = self.dex.ability(self.mon(t).ability).name.clone();
        log_line!(self, "|-activate|{ti}|ability: {name}");
    }

    /// Whether the target's item can be taken (Mega Stones stay with their species).
    pub(crate) fn item_removable(&self, t: MonRef) -> bool {
        self.mon(t)
            .item
            .map(|i| self.item_receivable(t, i))
            .unwrap_or(false)
    }

    /// Mega Stones' onTakeItem: a stone that maps from or to the Pokémon's
    /// base species (the Mega after Mega Evolution) can neither leave it nor
    /// be given to it, even when the base species is a forme (Floettite:
    /// Floette-Eternal).
    pub(crate) fn item_receivable(&self, r: MonRef, item: ItemId) -> bool {
        let name = &self.dex.species(self.mon(r).base_species).name;
        !self
            .dex
            .item(item)
            .mega_stone
            .iter()
            .any(|(from, to)| from == name || to == name)
    }

    /// Knock Off's `onAfterHit`: the target loses its item.
    pub(super) fn knock_off(&mut self, src: MonRef, t: MonRef) {
        if self.mon(t).item.is_none() {
            return;
        }
        // Sticky Hold's onTakeItem: Knock Off never takes the item.
        if self.sticky_hold_on_take_item(t, Some(src), true) {
            return;
        }
        // runEvent('TakeItem'): Unburden's handler (an ability's, so before
        // the item's) adds its volatile even when a Mega Stone then refuses.
        self.after_use_item(t);
        if !self.item_removable(t) {
            // takeItem's TakeItem event: Unburden (an ability, before the
            // Mega Stone's refusal) takes note anyway.
            if self.mon(t).item.is_some() {
                self.after_use_item(t);
            }
            return;
        }
        let item = self.mon(t).item.unwrap();
        let name = self.dex.item(item).name.clone();
        self.mon_mut(t).item = None;
        let ti = self.ident(t);
        let si = self.ident(src);
        log_line!(
            self,
            "|-enditem|{ti}|{name}|[from] move: Knock Off|[of] {si}"
        );
    }

    /// Psychic Fangs and Brick Break remove screens from the target's side.
    pub(crate) fn break_screens(&mut self, side: usize) {
        for c in [
            SideCond::Reflect,
            SideCond::LightScreen,
            SideCond::AuroraVeil,
        ] {
            if self.sides[side].conds.get(c) > 0 {
                *self.sides[side].conds.get_mut(c) = 0;
                let si = format!("{}: {}", self.sides[side].id(), self.sides[side].name);
                log_line!(self, "|-sideend|{si}|{}", c.log_name());
            }
        }
    }

    /// A secondary effect on its target: boosts, status, volatile; then self boosts.
    pub(super) fn apply_secondary(
        &mut self,
        src: MonRef,
        t: MonRef,
        sec: &Secondary,
        effect: EffectRef,
    ) {
        if let EffectRef::Move(m) = effect {
            if self.secondary_on_hit(src, t, m) || self.volatile_secondary_on_hit(src, t, m) {
                return;
            }
            if self.dex.move_kind[m as usize] == MoveKind::ThroatChop {
                if self.mon(t).hp > 0 && self.mon(t).vol.throat_chop == 0 {
                    self.mon_mut(t).vol.throat_chop = 2;
                    self.stamp_volatile(t, ResidualVol::ThroatChop);
                    let ti = self.ident(t);
                    log_line!(self, "|-start|{ti}|Throat Chop|[silent]");
                }
                return;
            }
            if self.dex.move_kind[m as usize] == MoveKind::DireClaw {
                // Its onHit calls trySetStatus without an effect, so the
                // status comes from the secondary, not the move: sleep is
                // announced without `[from] move: Dire Claw`.
                let st = self.rng.sample(&[Status::Psn, Status::Par, Status::Slp]);
                self.try_set_status(t, st, src, EffectRef::Other, false);
                return;
            }
        }
        if let Some(b) = sec.boosts {
            if !self.mon(t).fainted {
                self.boost_listed(t, &b, Some(src), effect, true, false);
            }
        }
        if let Some(st) = sec.status {
            self.try_set_status(t, st, src, effect, false);
        }
        if let Some(v) = &sec.volatile {
            self.add_volatile(t, v, false);
        }
        if let Some(b) = sec.self_boosts {
            self.boost_listed(src, &b, Some(src), effect, true, true);
        }
    }

    /// The `Hit` event on a target: Beak Blast burns a contact attacker.
    pub(crate) fn beak_blast_on_hit(&mut self, src: MonRef, t: MonRef, mv: &Move) {
        if self.mon(t).vol.beak_blast && mv.has(MoveFlag::Contact) {
            self.try_set_status(src, Status::Brn, t, EffectRef::Other, false);
        }
        // Focus Punch's volatile: any damaging hit loses the focus.
        if self.mon(t).vol.focus_punch.is_some() && mv.category != Category::Status {
            self.mon_mut(t).vol.focus_punch = Some(true);
        }
    }

    /// `applyRecoilDamage` for `mindBlownRecoil` (Steel Beam): half the
    /// user's max HP, rounded, then EmergencyExit. Whether it handled it.
    pub(crate) fn mind_blown_recoil(&mut self, r: MonRef, kind: MoveKind) -> bool {
        if kind != MoveKind::SteelBeam {
            return false;
        }
        let hp_before = self.mon(r).hp;
        let amount = (self.mon(r).maxhp as u32).div_ceil(2);
        self.apply_damage(r, amount, "|[from] steelbeam");
        self.emergency_exit(r, hp_before);
        true
    }

    /// A secondary effect's own `onHit`. Whether it replaced the default.
    pub(crate) fn secondary_on_hit(&mut self, src: MonRef, t: MonRef, m: MoveId) -> bool {
        match self.dex.move_kind[m as usize] {
            MoveKind::SyrupBomb => {
                self.add_syrup_bomb(t, src);
                true
            }
            MoveKind::AlluringVoice => {
                if self.mon(t).stats_raised_this_turn {
                    self.add_volatile(t, "confusion", false);
                }
                true
            }
            // Axe Kick's confusion (its only secondary) lasts 3 to 5 turns.
            MoveKind::Crash if self.dex.mv(m).id == "axekick" => {
                self.active_move.axe_kick = true;
                self.add_volatile(t, "confusion", false);
                self.active_move.axe_kick = false;
                true
            }
            MoveKind::TriAttack => {
                // trySetStatus without an effect, like Dire Claw's.
                let st = self.rng.sample(&[Status::Brn, Status::Par, Status::Frz]);
                self.try_set_status(t, st, src, EffectRef::Other, false);
                true
            }
            MoveKind::BurningJealousy => {
                if self.mon(t).stats_raised_this_turn {
                    self.try_set_status(t, Status::Brn, src, EffectRef::Move(m), false);
                }
                true
            }
            // Eerie Spell: the target's last move loses 3 PP (`deductPP`).
            MoveKind::EerieSpell => {
                if self.mon(t).hp > 0 {
                    if let Some(lm) = self.mon(t).last_move {
                        if let Some(ms) = self.mon_mut(t).moves.iter_mut().find(|ms| ms.id == lm) {
                            ms.used = true;
                            let amount = ms.pp.min(3);
                            ms.pp -= amount;
                            if amount > 0 {
                                let (ti, name) = (self.ident(t), self.dex.mv(lm).name.clone());
                                log_line!(
                                    self,
                                    "|-activate|{ti}|move: Eerie Spell|{name}|{amount}"
                                );
                            }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// The move's own `AfterMoveSecondarySelf` (before the items'), on the
    /// move's last chosen target: Fell Stinger's +3 Atk when it is down.
    pub(crate) fn run_after_move_secondary_self(
        &mut self,
        r: MonRef,
        target: MonRef,
        kind: MoveKind,
    ) {
        if kind == MoveKind::FellStinger && (self.mon(target).fainted || self.mon(target).hp == 0) {
            let mut b = [0i8; 7];
            b[Boost::Atk as usize] = 3;
            let id = self.dex.ids.moves.fell_stinger.unwrap_or(0);
            self.boost(r, &b, Some(r), EffectRef::Move(id), false, false);
        }
    }

    /// `useMoveInner` after TryMove: a `selfdestruct: 'always'` move makes
    /// its user faint (`faint(pokemon, pokemon, move)`) before it hits,
    /// whether or not anything is left to hit.
    pub(crate) fn self_destruct(&mut self, r: MonRef, kind: MoveKind) {
        if !matches!(kind, MoveKind::Explode | MoveKind::MistyExplosion) {
            return;
        }
        let m = self.mon(r);
        if m.fainted || m.faint_queued {
            return;
        }
        self.mon_mut(r).hp = 0;
        self.queue_faint(r);
        self.turn_state.move_kills.push((r, r));
    }

    /// Final Gambit's `damageCallback`: the damage is the user's HP, and
    /// the user faints (`faint()`, with no source).
    pub(crate) fn final_gambit_damage_callback(&mut self, r: MonRef) -> u32 {
        let hp = self.mon(r).hp as u32;
        if !self.mon(r).fainted && !self.mon(r).faint_queued {
            self.mon_mut(r).hp = 0;
            self.queue_faint(r);
        }
        hp
    }

    /// Counter's `onDamagingHit`: a physical hit from the other side sets the
    /// slot and double the damage.
    pub(crate) fn counter_on_damaging_hit(&mut self, src: MonRef, t: MonRef, mv: &Move) {
        if src.0 == t.0 {
            return;
        }
        let dealt = self
            .active_move
            .hit_dealt
            .iter()
            .find(|&&(x, _)| x == t)
            .map_or(0, |&(_, d)| d);
        let slot = Some((src.0 as u8, self.mon(src).position as u8));
        let vol = Some(CounterVol {
            slot,
            damage: 2 * dealt,
        });
        if self.mon(t).vol.counter.is_some() && mv.category == Category::Physical {
            self.mon_mut(t).vol.counter = vol;
        }
        if self.mon(t).vol.mirror_coat.is_some() && mv.category == Category::Special {
            self.mon_mut(t).vol.mirror_coat = vol;
        }
    }

    /// `addVolatile('sparklingaria')` from the secondary.
    pub(crate) fn add_sparkling_aria(&mut self, t: MonRef) -> Option<bool> {
        if self.mon(t).vol.sparkling_aria {
            return Some(false);
        }
        self.event_sort(Event::Immunity, t, None, &[]);
        self.try_add_volatile_sort(t);
        self.mon_mut(t).vol.sparkling_aria = true;
        Some(true)
    }

    /// Sparkling Aria's `onAfterMove`: its targets lose a burn.
    pub(crate) fn sparkling_aria_on_after_move(&mut self, r: MonRef) {
        let targets = self.active_move.hit_targets.take();
        let sheer = self.has_ability(r, AbilityKind::SheerForce);
        let Some(targets) = targets.filter(|_| !self.mon(r).fainted && !sheer) else {
            for a in self.all_active() {
                self.mon_mut(a).vol.sparkling_aria = false;
            }
            return;
        };
        let n = targets.len();
        for t in targets {
            if t != r && self.mon(t).is_active {
                let had = std::mem::take(&mut self.mon_mut(t).vol.sparkling_aria);
                if (had || n > 1) && self.mon(t).status == Status::Brn {
                    self.cure_status(t);
                }
            }
        }
    }

    /// The targets a status move hit that reach `afterMoveSecondaryEvent`:
    /// not those whose effect returned `NOT_FAIL`.
    pub(crate) fn after_move_secondary_targets(&mut self, hit: &[MonRef]) -> Small<MonRef> {
        let not_fail = std::mem::take(&mut self.active_move.not_fail);
        hit.iter()
            .copied()
            .filter(|t| !not_fail.contains(t))
            .collect()
    }

    /// A status move's effect on `t` returned `NOT_FAIL`.
    pub(crate) fn note_not_fail(&mut self, t: MonRef) {
        self.active_move.not_fail.push(t);
    }

    /// The move's own onHit on a target it damaged: Bug Bite and Pluck eat
    /// the target's berry; Jaw Lock traps both.
    pub(crate) fn move_on_hit(&mut self, r: MonRef, t: MonRef, mv: &Move, kind: MoveKind) {
        if kind == MoveKind::JawLock {
            // The user is trapped by the target, then the target by the user.
            self.add_trapped(r, t);
            self.add_trapped(t, r);
            return;
        }
        if kind == MoveKind::ClearSmog {
            self.mon_mut(t).boosts = [0; 7];
            let ti = self.ident(t);
            log_line!(self, "|-clearboost|{ti}");
            return;
        }
        if kind != MoveKind::StealEat || self.mon(r).hp == 0 {
            return;
        }
        let Some(item) = self.mon(t).item else { return };
        if !self.dex.item(item).is_berry || !self.item_receivable(t, item) {
            return;
        }
        // takeItem(source): Sticky Hold, or TakeItem (Unburden takes note), then
        // the berry is gone.
        if self.sticky_hold_on_take_item(t, Some(r), false) {
            return;
        }
        self.mon_mut(t).item = None;
        self.after_use_item(t);
        let (ti, ri) = (self.ident(t), self.ident(r));
        let name = self.dex.item(item).name.clone();
        log_line!(
            self,
            "|-enditem|{ti}|{name}|[from] stealeat|[move] {}|[of] {ri}",
            mv.name
        );
        // singleEvent('Eat') on the user: suppressed while it ignores items (Magic Room).
        if !self.ignoring_item(r) {
            self.berry_on_eat(r, item);
        }
        // runEvent('EatItem', user, user, move): Cheek Pouch, but not Cud Chew.
        self.run_eat_item(r, item, Some(r), false);
        self.mon_mut(r).ate_berry = true;
    }

    /// The move's own `onAfterHit` on the targets it damaged, while the user
    /// stands: Thief and Covet.
    pub(crate) fn move_on_after_hit(
        &mut self,
        r: MonRef,
        damaged: &[MonRef],
        mv: &Move,
        kind: MoveKind,
    ) {
        if kind != MoveKind::Thief || self.mon(r).hp == 0 {
            return;
        }
        for &t in damaged {
            self.thief(r, t, mv);
        }
    }

    /// AfterMove for the user `r`, as in Showdown's `useMove`: the move's own
    /// `onAfterMove` (`singleEvent`), then `runEvent('AfterMove')`.
    ///
    /// The move's own: Spit Up uses up the stockpile, whatever the move did;
    /// Beak Blast's volatile ends; Sparkling Aria's targets lose a burn.
    ///
    /// `runEvent('AfterMove')`: the user's volatiles' handlers
    /// (subOrder 2) in the order they were added, lockedmove's and Charge's;
    /// then every White Herb's onAnyAfterMove (an item, 8), the user's side
    /// first. By speed, equal ones drawn.
    pub(crate) fn run_after_move(&mut self, r: MonRef, move_id: MoveId) {
        match self.dex.move_kind[move_id as usize] {
            MoveKind::SpitUp => self.end_stockpile(r),
            MoveKind::BeakBlast if self.mon(r).hp > 0 => self.mon_mut(r).vol.beak_blast = false,
            MoveKind::SparklingAria => self.sparkling_aria_on_after_move(r),
            _ => {}
        }
        use crate::abilities::{compare_mod, ModHandler};
        use crate::kinds::ItemKind;
        #[derive(Clone, Copy)]
        enum H {
            LockedMove,
            Charge,
            WhiteHerb(MonRef),
            /// Opportunist's `onAnyAfterMove` (an ability, 7).
            Opportunist(MonRef),
        }
        let v = self.mon(r).vol;
        let speed = self.mon(r).speed;
        let mut vols: Small<(u32, H)> = Small::new();
        if v.locked_move.is_some() {
            vols.push((v.added[ResidualVol::LockedMove as usize], H::LockedMove));
        }
        if v.charge {
            vols.push((v.added[ResidualVol::Charge as usize], H::Charge));
        }
        vols.sort_by_key(|&(n, _)| n);
        let mut hs: Small<(H, ModHandler)> = vols
            .into_iter()
            .map(|(_, h)| (h, ModHandler::new(0, speed, 2)))
            .collect();
        let mut holders = self.all_active();
        holders.sort_by_key(|h| h.0 != r.0);
        for h in holders {
            if self.raw_ability_kind(h) == AbilityKind::Opportunist {
                hs.push((H::Opportunist(h), ModHandler::new(0, self.mon(h).speed, 7)));
            }
            if self.item_kind(h) == Some(ItemKind::WhiteHerb) {
                hs.push((H::WhiteHerb(h), ModHandler::new(0, self.mon(h).speed, 8)));
            }
        }
        if hs.is_empty() {
            return;
        }
        self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        for &(h, _) in hs.iter() {
            match h {
                H::WhiteHerb(h) => self.white_herb_check(h),
                H::Opportunist(h) => self.opportunist_release(h),
                H::LockedMove => {
                    let v = self.mon(r).vol;
                    if v.locked_move.is_some() && v.locked_duration == 1 && self.mon(r).hp > 0 {
                        self.end_locked_move(r);
                    }
                }
                H::Charge => {
                    let Some((m, t)) = self.active_move.used_type else {
                        continue;
                    };
                    if self.mon(r).vol.charge
                        && self.dex.type_names[t as usize] == "Electric"
                        && self.dex.mv(m).id != "charge"
                    {
                        self.end_charge(r);
                    }
                }
            }
        }
    }

    /// `addVolatile('confusion')` from lockedmove's onEnd: `[fatigue]`, and
    /// Own Tempo and Misty Terrain stop it silently.
    pub(crate) fn fatigue_confusion(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 || self.mon(r).vol.confusion > 0 {
            return;
        }
        self.try_add_volatile_sort(r);
        if self.ability_blocks_volatile(r, "confusion") {
            return;
        }
        if self.is_grounded(r)
            && !self.semi_invulnerable(r)
            && self.field.terrain == Some(Terrain::Misty)
        {
            return;
        }
        let turns = self.rng.random_range(2, 6) as u8;
        self.mon_mut(r).vol.confusion = turns;
        let ri = self.ident(r);
        log_line!(self, "|-start|{ri}|confusion|[fatigue]");
    }

    /// Liquid Ooze's `onSourceTryHeal` for a drain or Leech Seed heal that
    /// `healer` takes from `source` (the heal as given, before Big Root's chainModify applies): the heal
    /// becomes damage to the healer. Heal Block's `onTryHeal` (a volatile,
    /// subOrder 2) sorts with it at the same priority and stops the event
    /// if it comes first. True when the ooze took the heal.
    pub(crate) fn liquid_ooze_on_source_try_heal(
        &mut self,
        healer: MonRef,
        source: MonRef,
        amount: u32,
    ) -> bool {
        if self.mon(source).fainted
            || self.mon(healer).hp == 0
            || !self.has_ability(source, AbilityKind::LiquidOoze)
        {
            return false;
        }
        if self.mon(healer).vol.heal_block > 0 {
            use crate::abilities::{compare_mod, ModHandler};
            let mut hs = [
                (true, ModHandler::new(0, self.mon(healer).speed, 2)),
                (false, ModHandler::new(0, self.mon(source).speed, 7)),
            ];
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
            if hs[0].0 {
                return false;
            }
        }
        let si = self.ident(source);
        self.apply_damage(
            healer,
            amount.max(1),
            &format!("|[from] ability: Liquid Ooze|[of] {si}"),
        );
        true
    }
}
