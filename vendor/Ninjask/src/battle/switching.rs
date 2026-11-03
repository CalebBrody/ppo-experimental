//! Switching in and out, dragging, reviving, fainting and the win check.

use super::{Battle, MonRef};
use crate::battle::Small;
use crate::dex::types::*;
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

#[derive(Clone, Copy, Debug)]
enum SwitchInHandler {
    Ability,
    Item,
    WhiteHerb,
    /// Toxic Spikes on the switcher's side (a side condition: subOrder 4).
    ToxicSpikes,
    StealthRock,
    /// Slot conditions (Healing Wish).
    HealingWish,
    /// Hazards (Spikes, Sticky Web).
    Hazard(crate::state::AHazard),
    /// Opportunist's `onAnySwitchIn` (priority -3).
    Opportunist,
}

impl Battle {
    /// Showdown `runSwitch` and `fieldEvent('SwitchIn')`: switch-in abilities
    /// of the Pokémon that just came in, fastest first.
    pub(crate) fn run_switch(&mut self, switchers: &[MonRef]) {
        // speedOrder: all active Pokémon (fainted too) by speed, ties shuffled.
        let mut all: Small<MonRef> = Small::new();
        for s in 0..2 {
            for slot in 0..self.sides[s].n_active {
                if let Some(r) = self.active(s, slot) {
                    all.push(r);
                }
            }
        }
        let speeds: Small<(MonRef, i32)> = all.iter().map(|&r| (r, self.mon(r).speed)).collect();
        if self.rng.trace.is_some() {
            let list: Vec<String> = speeds
                .iter()
                .map(|&(r, sp)| {
                    format!(
                        "{}{}:{}/{sp}",
                        self.sides[r.0].id(),
                        r.1,
                        self.dex.species(self.mon(r).species).id
                    )
                })
                .collect();
            if let Some(t) = &mut self.rng.trace {
                t.push(format!("# pokemon {}", list.join(",")));
            }
        }
        let mut order = speeds.clone();
        self.speed_sort_by(&mut order, |a, b| b.1 as i64 - a.1 as i64);
        let rank = |r: MonRef| order.iter().position(|&(x, _)| x == r).unwrap_or(0) as i64;
        // Handlers: (holder, kind, priority, speed key, subOrder). The speed key
        // is the Pokémon's speed minus a fraction from its rank, so ties are
        // broken by the speed order drawn above.
        // The last key is the effect order: a side's hazards tie otherwise,
        // and run in the order they were created.
        let mut handlers: Small<(MonRef, SwitchInHandler, i32, i64, u32, u16)> = Small::new();
        self.turn_state.sorting = true;
        for &r in switchers {
            let key = self.mon(r).speed as i64 * 4 - rank(r);
            let conds = self.sides[r.0].conds;
            if conds.toxic_spikes > 0 {
                handlers.push((
                    r,
                    SwitchInHandler::ToxicSpikes,
                    0,
                    key,
                    4,
                    conds.toxic_spikes_added,
                ));
            }
            if conds.stealth_rock {
                handlers.push((
                    r,
                    SwitchInHandler::StealthRock,
                    0,
                    key,
                    4,
                    conds.stealth_rock_added,
                ));
            }
            for (h, added) in self.entry_hazards(r) {
                handlers.push((r, SwitchInHandler::Hazard(h), 0, key, 4, added));
            }
            if let Some(priority) = self.ability_switch_in_priority(r) {
                handlers.push((r, SwitchInHandler::Ability, priority, key, 7, 0));
            }
            if let Some(priority) = self.item_switch_in_priority(r) {
                handlers.push((r, SwitchInHandler::Item, priority, key, 8, 0));
            }
            if self.slot_reacts_to_switch_in(r) {
                handlers.push((r, SwitchInHandler::HealingWish, 0, key, 3, 0));
            }
        }
        // White Herb's onAnySwitchIn (priority -2) on every active holder.
        for &r in &all {
            if self.item_kind(r) == Some(ItemKind::WhiteHerb) {
                let key = self.mon(r).speed as i64 * 4 - rank(r);
                handlers.push((r, SwitchInHandler::WhiteHerb, -2, key, 8, 0));
            }
            if self.raw_ability_kind(r) == AbilityKind::Opportunist {
                let key = self.mon(r).speed as i64 * 4 - rank(r);
                handlers.push((r, SwitchInHandler::Opportunist, -3, key, 7, 0));
            }
        }
        self.turn_state.sorting = false;
        self.speed_sort_by(&mut handlers, |a, b| {
            let d = b.2 as i64 - a.2 as i64;
            if d != 0 {
                return d;
            }
            let d = b.3 - a.3;
            if d != 0 {
                return d;
            }
            let d = a.4 as i64 - b.4 as i64;
            if d != 0 {
                return d;
            }
            a.5 as i64 - b.5 as i64
        });
        for &(r, kind, _, _, _, _) in handlers.iter() {
            if self.mon(r).fainted || !self.mon(r).is_active {
                continue;
            }
            match kind {
                // Imposter's handler is `onSwitchIn`, not `onStart`.
                SwitchInHandler::Ability if self.has_ability(r, AbilityKind::Imposter) => {
                    self.imposter_switch_in(r)
                }
                SwitchInHandler::Ability => {
                    self.air_lock_on_switch_in(r);
                    self.run_start(r)
                }
                SwitchInHandler::Item => self.item_on_start(r),
                SwitchInHandler::WhiteHerb => self.white_herb_check(r),
                SwitchInHandler::Opportunist => self.opportunist_release(r),
                SwitchInHandler::ToxicSpikes => self.toxic_spikes_switch_in(r),
                SwitchInHandler::StealthRock => self.stealth_rock_switch_in(r),
                SwitchInHandler::HealingWish => self.healing_wish_on_switch_in(r),
                SwitchInHandler::Hazard(h) => self.hazard_on_switch_in(r, h),
            }
            self.faint_messages();
            if self.ended {
                return;
            }
        }
    }

    /// Showdown `dragIn`: a random healthy benched Pokémon replaces the one
    /// in `slot`.
    pub(super) fn drag_in(&mut self, s: usize, slot: usize) {
        let side = &self.sides[s];
        let bench: Vec<usize> = (side.n_active..side.mons.len())
            .filter(|&i| !side.mons[i].fainted)
            .collect();
        if bench.is_empty() || self.mon((s, slot)).hp == 0 {
            return;
        }
        let pick = bench[self.rng.random(bench.len() as u32) as usize];
        self.switch_in(s, pick, slot, true, None);
    }

    /// Showdown `switchIn`: put `mons[index]` into `slot`. A drag (Roar)
    /// skips BeforeSwitchOut and runs the switch-in effects at once.
    /// Revival Blessing's action (Showdown `runAction` 'revivalblessing'):
    /// the teammate comes back at half HP. One that fainted in its slot this
    /// turn switches back in last (`queue.addChoice` appends an instaswitch);
    /// until then it stands there without being active.
    pub(super) fn revive(&mut self, s: usize, slot: usize, target: usize) {
        self.sides[s].pokemon_left += 1;
        let on_field = target < self.sides[s].n_active;
        if on_field {
            let kind = ActionKind::Switch {
                side: s,
                mon: target,
                target,
                insta: true,
                from: None,
            };
            let a = self.resolve_action(kind);
            self.queue.push_back(a);
        }
        let m = &mut self.sides[s].mons[target];
        m.fainted = false;
        m.faint_queued = false;
        m.status = Status::None;
        m.hp = (m.maxhp / 2).max(1);
        m.revived = on_field;
        let ident = self.ident((s, target));
        self.add_health(
            format_args!("|-heal|{ident}"),
            (s, target),
            "|[from] move: Revival Blessing",
        );
        self.sides[s].revival[slot] = false;
    }

    pub(super) fn switch_in(
        &mut self,
        s: usize,
        index: usize,
        slot: usize,
        drag: bool,
        from: Option<MoveId>,
    ) {
        let side = &self.sides[s];
        let old_active = if slot < side.mons.len() && side.mons[slot].is_active {
            Some(slot)
        } else {
            None
        };
        if let Some(o) = old_active {
            self.partial_trap_source_leaves((s, o));
            if self.sides[s].mons[o].hp > 0 {
                // BeforeSwitchOut and eachEvent('Update') (not for a drag, nor
                // when the switch request ran BeforeSwitchOut), then SwitchOut.
                if !drag && !self.sides[s].mons[o].skip_before_switch_out {
                    self.run_update();
                }
                self.sides[s].mons[o].skip_before_switch_out = false;
                self.ability_switch_out((s, o));
                // The ability's End (Flash Fire drops its boost).
                self.turn_state.called_back = true;
                self.ability_on_end((s, o));
                self.turn_state.called_back = false;
            }
            // After BeforeSwitchOut's Update: the Pokémon is no longer active.
            self.attract_source_leaves((s, o));
        }
        if let Some(o) = old_active.filter(|&o| self.sides[s].mons[o].hp > 0) {
            self.clear_transform((s, o));
            self.revert_forme((s, o));
        }
        // Baton Pass (`copyvolatile`): what the newcomer takes over.
        let mut carried = old_active.and_then(|o| self.copy_volatile((s, o), from));
        if let Some(o) = old_active.filter(|&o| self.sides[s].mons[o].hp > 0) {
            self.clear_volatile_links((s, o));
        }
        let side = &mut self.sides[s];
        if let Some(o) = old_active {
            if side.mons[o].hp > 0 {
                // clearVolatile: volatiles, boosts, the ability and the last
                // move (its target location stays); setSpecies makes the
                // stored speed the Speed stat.
                let types = self.dex.species(side.mons[o].species).types;
                let m = &mut side.mons[o];
                m.vol = Volatiles::default();
                m.boosts = [0; 7];
                m.types = types;
                m.added_type = NO_TYPE;
                m.times_attacked = 0;
                m.hurt_this_turn = None;
                m.speed = m.stats[Stat::Spe as usize] as i32;
                m.ability = m.base_ability;
                m.last_move = None;
                m.move_this_turn_result = None;
                m.move_last_turn_result = None;
                // And the switch flags: a U-turn user Red Card dragged out
                // keeps no pending switch.
                m.switch_flag = false;
                m.switch_from_move = None;
                m.force_switch_flag = false;
                // Cancel its queued actions.
                self.queue.retain(|a| match a.kind {
                    ActionKind::Move { side: ss, mon, .. }
                    | ActionKind::MegaEvo { side: ss, mon } => !(ss == s && mon == o),
                    _ => true,
                });
            }
        }
        // The new abilityState's effectOrder.
        self.vol_stamp += 1;
        let ability_order = self.vol_stamp;
        if let Some(o) = old_active {
            self.switch_out_cleanup((s, o));
        }
        if index != slot {
            self.positions_swapped(s, index, slot, &mut carried);
        }
        let side = &mut self.sides[s];
        if let Some(o) = old_active {
            side.mons[o].is_active = false;
            side.mons[o].used_item_this_turn = false;
            if side.mons[o].fainted {
                side.mons[o].status = Status::None;
            }
        }
        if index != slot {
            side.mons.swap(index, slot);
            let (a, b) = (index, slot);
            side.mons[a].position = a;
            side.mons[b].position = b;
        }
        let m = &mut side.mons[slot];
        m.is_active = true;
        m.revived = false;
        m.active_turns = 0;
        m.active_move_actions = 0;
        for ms in m.moves.iter_mut() {
            ms.used = false;
        }
        m.ability_state = AbilityState::default();
        m.ability_order = ability_order;
        m.switch_flag = false;
        m.moved_this_turn = false;
        if m.status == Status::Tox {
            m.toxic_stage = 0;
        }
        if let Some((vol, boosts)) = carried {
            m.vol = vol;
            m.boosts = boosts;
        }
        let who = (s, slot);
        self.gastro_acid_on_copy(who);
        // BeforeSwitchIn: Illusion picks its disguise.
        self.illusion_before_switch_in(who);
        if self.log_enabled {
            let details = self.switch_details(who);
            let ident = self.ident(who);
            let verb = if drag { "drag" } else { "switch" };
            let source = from
                .map(|m| format!("|[from] {}", self.dex.mv(m).name))
                .unwrap_or_default();
            self.add_health(format_args!("|{verb}|{ident}|{details}"), who, &source);
        }
        if drag {
            // Gen 5+: a dragged-in Pokémon's switch-in effects run at once,
            // with its stored speed as it was (the Speed stat).
            self.run_switch(&[who]);
        } else {
            // insertChoice updates the Pokémon's speed before queueing runSwitch.
            let sp = self.action_speed_draw(who);
            self.mon_mut(who).speed = sp;
            let ra = self.resolve_action(ActionKind::RunSwitch { side: s, mon: slot });
            self.insert_action(ra);
        }
    }

    /// Queue a Pokémon at 0 HP for `faint_messages` (Showdown `faint`, which
    /// also drops a pending switch: a U-turn user knocked out by Rough Skin
    /// doesn't ask for a replacement mid-turn).
    pub(crate) fn queue_faint(&mut self, r: MonRef) {
        if !self.mon(r).faint_queued {
            let m = self.mon_mut(r);
            m.faint_queued = true;
            m.switch_flag = false;
            m.switch_from_move = None;
            self.faint_queue.push(r);
        }
    }

    /// Showdown `faintMessages` + `checkWin`.
    pub(crate) fn faint_messages(&mut self) {
        self.faint_messages_check(true);
    }

    /// Showdown `faintMessages(lastFirst, forceCheck, checkWin)`: with
    /// `check_win` false (after a move's hits, while its user still stands)
    /// the win waits until recoil and Life Orb have been applied.
    pub(crate) fn faint_messages_check(&mut self, check_win: bool) {
        // Nothing queued: no faint, no win to check, no AfterFaint.
        if self.ended || self.faint_queue.is_empty() {
            return;
        }
        let mut queue = std::mem::take(&mut self.faint_queue);
        let mut any = false;
        let mut check_win = check_win;
        let mut i = 0;
        while i < queue.len() {
            let r = queue[i];
            i += 1;
            if self.mon(r).fainted {
                continue;
            }
            let ident = self.ident(r);
            log_line!(self, "|faint|{ident}");
            let side = &mut self.sides[r.0];
            if side.pokemon_left > 0 {
                side.pokemon_left -= 1;
            }
            if side.total_fainted < 100 {
                side.total_fainted += 1;
            }
            // runEvent('Faint'): Destiny Bond. A faint it queues is handled
            // in this loop, and then the win is checked.
            self.destiny_bond_on_faint(r);
            if !self.faint_queue.is_empty() {
                queue.append(&mut self.faint_queue);
                check_win = true;
            }
            // The ability's End runs here too, but a Pokémon at 0 HP can't
            // lose a volatile (removeVolatile), so Flash Fire ends silently.
            // Unnerve's onEnd still marks its state.
            if self.has_ability(r, crate::kinds::AbilityKind::Unnerve) {
                self.mon_mut(r).ability_state.unnerved = Some(false);
            }
            // Faint: Illusion's onFaint drops the disguise (after the line).
            self.mon_mut(r).illusion = None;
            // The ability's End, then the forme reverts (Showdown faintMessages).
            self.air_lock_on_end(r);
            self.supreme_overlord_on_end(r);
            self.clear_transform(r);
            self.revert_forme(r);
            self.clear_volatile_links(r);
            let types = self.dex.species(self.mon(r).species).types;
            let m = self.mon_mut(r);
            m.fainted = true;
            m.types = types;
            m.added_type = NO_TYPE;
            m.times_attacked = 0;
            m.hurt_this_turn = None;
            m.faint_queued = false;
            // clearVolatile -> setSpecies: the stored speed becomes the Speed
            // stat, and the types the species' (after Soak).
            m.speed = m.stats[Stat::Spe as usize] as i32;
            m.vol = Volatiles::default();
            m.boosts = [0; 7];
            m.ability = m.base_ability;
            m.last_move = None;
            m.move_this_turn_result = None;
            m.move_last_turn_result = None;
            self.clear_volatile_on_faint(r);
            self.last_fainted_side = Some(r.0);
            any = true;
        }
        if any && check_win {
            self.check_win();
        }
        self.run_after_faint(&queue);
        // Keep the buffer for the next faints.
        if self.faint_queue.is_empty() {
            queue.clear();
            self.faint_queue = queue;
        }
    }

    pub(crate) fn check_win(&mut self) {
        if self.ended {
            return;
        }
        let left = [self.sides[0].pokemon_left, self.sides[1].pokemon_left];
        if left[0] == 0 && left[1] == 0 {
            // Gen 5+: the side whose Pokémon fainted last wins.
            self.win(self.last_fainted_side);
        } else if left[1] == 0 {
            self.win(Some(0));
        } else if left[0] == 0 {
            self.win(Some(1));
        }
    }

    fn win(&mut self, side: Option<usize>) {
        // The queue keeps the actions that will not run, as Showdown's does.
        self.ended = true;
        self.winner = side;
        self.request = RequestState::Ended;
        match side {
            Some(s) => {
                let name = self.sides[s].name.clone();
                log_line!(self, "|win|{name}");
            }
            None => self.add("|tie"),
        }
    }

    /// A Pokémon leaves its slot (switched out, dragged out or replaced after
    /// fainting): partial traps it set lose their source.
    pub(crate) fn partial_trap_source_leaves(&mut self, r: MonRef) {
        for s in 0..2 {
            for m in self.sides[s].mons.iter_mut() {
                if m.vol.partial_trap_source == Some(r) {
                    m.vol.partial_trap_source = None;
                }
            }
        }
    }

    /// `DragOut` (after Guard Dog and Suction Cups, priority 1): Ingrain
    /// holds the Pokémon in place.
    pub(crate) fn ingrain_on_drag_out(&mut self, t: MonRef) -> bool {
        if !self.mon(t).vol.ingrain {
            return false;
        }
        let ti = self.ident(t);
        log_line!(self, "|-activate|{ti}|move: Ingrain");
        true
    }

    /// A position swap (a switch, Ally Switch) moves Pokémon in the team
    /// list: the references kept here follow them.
    pub(crate) fn follow_swap_field_refs(
        &mut self,
        fix: impl Fn(Option<MonRef>) -> Option<MonRef>,
    ) {
        self.field.magic_room_source = fix(self.field.magic_room_source);
        for side in self.slot_conds.iter_mut() {
            for slot in side.iter_mut() {
                if let Some(f) = slot.future.as_mut() {
                    f.source = fix(Some(f.source)).unwrap_or(f.source);
                }
            }
        }
    }

    /// `isAdjacent` between two active Pokémon (doubles).
    pub(crate) fn is_adjacent(&self, a: MonRef, b: MonRef) -> bool {
        a != b && (a.0 != b.0 || a.1.abs_diff(b.1) == 1)
    }

    /// The source Pokémon is still on the field (`source.isActive`).
    pub(crate) fn source_active(&self, s: MonRef) -> bool {
        let m = self.mon(s);
        m.is_active && !m.fainted
    }

    /// The Pokémon leaves its slot: an Attract it caused ends at the next
    /// Update (its source is no longer active).
    pub(crate) fn attract_source_leaves(&mut self, r: MonRef) {
        for side in self.sides.iter_mut() {
            for m in side.mons.iter_mut() {
                if m.vol.attract == Some(r) {
                    m.vol.attract_source_gone = true;
                }
            }
        }
    }

    /// `runEvent('Faint')` for `r`: Destiny Bond takes down the foe whose
    /// move knocked it out (`faint()`: queued, unless it is already).
    pub(crate) fn destiny_bond_on_faint(&mut self, r: MonRef) {
        // The ability's End follows, and End isn't suppressed: Gastro Acid
        // (cleared with the volatiles anyway) mustn't hide the ability from it.
        self.mon_mut(r).vol.gastro_acid = false;
        // Receiver's onAllyFaint joins Destiny Bond's onFaint in the sort.
        let receivers: Small<MonRef> = self
            .allies_of(r)
            .into_iter()
            .filter(|&a| self.has_ability(a, AbilityKind::Receiver))
            .collect();
        if !receivers.is_empty() {
            use crate::abilities::{compare_mod, ModHandler};
            let mut hs: Small<(ModHandler, Option<MonRef>)> = receivers
                .iter()
                .map(|&a| (ModHandler::new(0, self.mon(a).speed, 7), Some(a)))
                .collect();
            if self.mon(r).vol.destiny_bond {
                hs.push((ModHandler::new(0, self.mon(r).speed, 2), None));
            }
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.0, &b.0));
            for &(_, holder) in hs.iter() {
                match holder {
                    Some(h) => self.receiver(h, r),
                    None => self.destiny_bond_faint(r),
                }
            }
            return;
        }
        self.destiny_bond_faint(r);
    }

    /// Receiver's `onAllyFaint`: take the fainted ally's ability, unless it
    /// has `noreceiver` or is no ability.
    pub(crate) fn receiver(&mut self, holder: MonRef, fainted: MonRef) {
        if self.mon(holder).hp == 0 {
            return;
        }
        let ability = self.mon(fainted).ability;
        let a = self.dex.ability(ability);
        if a.id == "noability" || a.flags.iter().any(|f| f == "noreceiver") {
            return;
        }
        self.set_ability(holder, ability, fainted, "ability: Receiver");
    }

    /// Destiny Bond's `onFaint`.
    pub(crate) fn destiny_bond_faint(&mut self, r: MonRef) {
        if !self.mon(r).vol.destiny_bond {
            return;
        }
        let Some(src) = self
            .turn_state
            .move_kills
            .iter()
            .find(|&&(t, _)| t == r)
            .map(|&(_, s)| s)
        else {
            return;
        };
        if src.0 == r.0 {
            return;
        }
        let ri = self.ident(r);
        log_line!(self, "|-activate|{ri}|move: Destiny Bond");
        let m = self.mon(src);
        if m.fainted || m.faint_queued {
            return;
        }
        self.mon_mut(src).hp = 0;
        self.queue_faint(src);
    }

    /// Showdown `swapPosition`: the Pokémon trades places with whatever is
    /// in `new` (doubles). Queued actions follow their Pokémon.
    pub(crate) fn swap_position(&mut self, r: MonRef, new: usize, attrs: &str) {
        let (s, old) = r;
        let ri = self.ident(r);
        log_line!(self, "|swap|{ri}|{new}{attrs}");
        self.relocated = Some((r, (s, new)));
        let side = &mut self.sides[s];
        side.mons.swap(old, new);
        side.mons[old].position = old;
        side.mons[new].position = new;
        let flip = |m: usize| {
            if m == old {
                new
            } else if m == new {
                old
            } else {
                m
            }
        };
        self.swap_refs(s, flip);
        // References to the swapped Pokémon follow them (partial trapping's user).
        for side in self.sides.iter_mut() {
            for m in side.mons.iter_mut() {
                if let Some((ts, ti)) = m.vol.partial_trap_source {
                    if ts == s {
                        m.vol.partial_trap_source = Some((ts, flip(ti)));
                    }
                }
            }
        }
        for a in self.queue.iter_mut() {
            match &mut a.kind {
                ActionKind::Move { side, mon, .. }
                | ActionKind::MegaEvo { side, mon }
                | ActionKind::Switch { side, mon, .. }
                | ActionKind::RunSwitch { side, mon }
                    if *side == s =>
                {
                    *mon = flip(*mon);
                }
                _ => {}
            }
        }
        // runEvent('Swap') on the Pokémon now in the old slot, then on the
        // mover: a Healing Wish waiting in either slot heals it.
        self.run_swap(s, old);
        self.run_swap(s, new);
    }

    /// Ally Switch: references to the two swapped Pokémon follow them.
    pub(crate) fn swap_refs(&mut self, s: usize, flip: impl Fn(usize) -> usize) {
        let fix =
            |x: Option<MonRef>| x.map(|(ts, ti)| if ts == s { (ts, flip(ti)) } else { (ts, ti) });
        for side in self.sides.iter_mut() {
            for m in side.mons.iter_mut() {
                m.vol.attract = fix(m.vol.attract);
                m.vol.trapped = fix(m.vol.trapped);
                m.vol.lock_on = fix(m.vol.lock_on);
                m.vol.octolock = fix(m.vol.octolock);
                let mut set = m.vol.trapper & !(0xff << (s * 8));
                for i in 0..8 {
                    if m.vol.trapper & Self::trap_bit((s, i)) != 0 {
                        set |= Self::trap_bit((s, flip(i)));
                    }
                }
                m.vol.trapper = set;
            }
        }
        // Magic Room's setter and Future Sight's user.
        self.follow_swap_field_refs(fix);
        self.follow_swap_volatile_refs(fix);
    }

    /// The Pokémon `r` after a swap during its move (Ally Switch).
    pub(crate) fn relocated_mon(&mut self, r: MonRef) -> MonRef {
        match self.relocated.take() {
            Some((from, to)) if from == r => to,
            _ => r,
        }
    }

    /// A switch swaps the newcomer's and the old active Pokémon's positions:
    /// Lock-On's target (a Pokémon, not a slot) follows its Pokémon.
    pub(crate) fn positions_swapped(
        &mut self,
        s: usize,
        a: usize,
        b: usize,
        carried: &mut Option<(Volatiles, [i8; 7])>,
    ) {
        let flip = |i: usize| {
            if i == a {
                b
            } else if i == b {
                a
            } else {
                i
            }
        };
        // Octolock's holder, and what Baton Pass carries over.
        let fix =
            |x: Option<MonRef>| x.map(|(ts, ti)| if ts == s { (ts, flip(ti)) } else { (ts, ti) });
        if let Some((v, _)) = carried {
            v.octolock = fix(v.octolock);
        }
        // A Mold Breaker user dragged out (Red Card) stays the move's user.
        self.ability_breaker =
            self.ability_breaker
                .map(|(ts, ti)| if ts == s { (ts, flip(ti)) } else { (ts, ti) });
        self.follow_swap_field_refs(fix);
        self.follow_swap_volatile_refs(fix);
        for side in self.sides.iter_mut() {
            for m in side.mons.iter_mut() {
                m.vol.lock_on = m
                    .vol
                    .lock_on
                    .map(|(ts, ti)| if ts == s { (ts, flip(ti)) } else { (ts, ti) });
                m.vol.octolock = fix(m.vol.octolock);
            }
        }
    }
}
