//! Held items: using and eating them (Showdown `useItem` / `eatItem`) and
//! their handlers at the events where Showdown runs them. Callers in
//! moves/, conditions/ and events/ invoke these at the matching points.

mod berries;
mod fling;
mod transfer;

use crate::abilities::{compare_mod, ModHandler};
use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, EffectRef, ItemKind};
use crate::moves::damage::{self, Chain};
use crate::state::{SideCond, Terrain};

/// One `ModifyDamage` handler, sorted like Showdown's handlers.
#[derive(Clone, Copy, Debug)]
enum DamageModHandler {
    /// The user's Sniper.
    Sniper,
    LifeOrb,
    ExpertBelt,
    /// Friend Guard on this Pokémon (`onAnyModifyDamage`).
    FriendGuard(MonRef),
    /// The target's Glaive Rush volatile.
    GlaiveRush,
    /// The target's Minimize volatile (`onSourceModifyDamage`).
    Minimize,
    /// The target's ability: Multiscale, Filter, Aura Guard.
    TargetAbility,
    /// The target's Fly, Dig or Dive volatile (`onSourceModifyDamage`).
    SemiInvulnerable,
    ResistBerry,
    /// Reflect, Light Screen or Aurora Veil on this side (`onAnyModifyDamage`).
    Screen(usize, SideCond),
    /// The user's Metronome volatile.
    Metronome,
}

impl Battle {
    #[inline(always)]
    pub fn item_kind(&self, r: MonRef) -> Option<ItemKind> {
        let item = self.mon(r).item?;
        // Not active (revived in its old slot): its item is ignored (`ignoringItem`).
        if self.mon(r).revived || self.ignoring_item(r) {
            return None;
        }
        Some(self.dex.item_kind[item as usize])
    }

    fn item_name(&self, r: MonRef) -> String {
        self.mon(r)
            .item
            .map(|i| self.dex.item(i).name.clone())
            .unwrap_or_default()
    }

    pub(crate) fn consume_item(&mut self, r: MonRef, berry: bool) {
        let m = self.mon_mut(r);
        m.last_item = m.item.take();
        m.used_item_this_turn = true;
        if berry {
            m.ate_berry = true;
        }
        self.run_after_use_item(r);
    }

    /// Showdown `useItem` for items that aren't eaten: their `boosts`, then `onUse`.
    pub(crate) fn use_item(&mut self, r: MonRef) -> bool {
        self.use_item_from(r, None)
    }

    /// `useItem(source)`: Red Card's message names the Pokémon it sends back.
    pub(crate) fn use_item_from(&mut self, r: MonRef, source: Option<MonRef>) -> bool {
        let m = self.mon(r);
        if m.hp == 0 || !m.is_active || m.item.is_none() {
            return false;
        }
        let kind = self.item_kind(r).unwrap_or(ItemKind::Unsupported);
        let ident = self.ident(r);
        let name = self.item_name(r);
        match (kind, source) {
            (ItemKind::RedCard, Some(src)) => {
                let si = self.ident(src);
                log_line!(self, "|-enditem|{ident}|{name}|[of] {si}");
            }
            _ => log_line!(self, "|-enditem|{ident}|{name}"),
        }
        match kind {
            ItemKind::Seed(t) => {
                let mut b = [0i8; 7];
                let stat = match t {
                    Terrain::Grassy | Terrain::Electric => Boost::Def,
                    Terrain::Psychic | Terrain::Misty => Boost::Spd,
                };
                b[stat as usize] = 1;
                let item = self.mon(r).item.unwrap();
                self.boost(r, &b, Some(r), EffectRef::Item(item), false, false);
            }
            ItemKind::WhiteHerb => {
                for b in self.mon_mut(r).boosts.iter_mut() {
                    if *b < 0 {
                        *b = 0;
                    }
                }
                log_line!(self, "|-clearnegativeboost|{ident}|[silent]");
            }
            _ => {}
        }
        self.consume_item(r, false);
        true
    }

    // ------------------------------------------------------------ switch-in and field changes

    /// Whether an item has a switch-in (`onStart`) effect, and its priority.
    pub(crate) fn item_switch_in_priority(&self, r: MonRef) -> Option<i32> {
        match self.item_kind(r)? {
            ItemKind::Seed(_) => Some(-1),
            ItemKind::AirBalloon | ItemKind::Metronome => Some(0),
            _ => None,
        }
    }

    /// An item's `onStart` at switch-in.
    pub(crate) fn item_on_start(&mut self, r: MonRef) {
        match self.item_kind(r) {
            Some(ItemKind::Seed(t)) => {
                if self.field.terrain == Some(t) {
                    self.use_item(r);
                }
            }
            // Not announced under Gravity.
            Some(ItemKind::AirBalloon) if self.field.gravity == 0 => {
                let ident = self.ident(r);
                log_line!(self, "|-item|{ident}|Air Balloon");
            }
            Some(ItemKind::Metronome) => self.metronome_on_start(r),
            // White Herb's onStart: used at once on a lowered stat (Trick
            // hands it over before announcing it).
            Some(ItemKind::WhiteHerb) => self.white_herb_check(r),
            // A Choice item's onStart removes an old Choice lock.
            Some(k) if k.is_choice() => self.mon_mut(r).vol.choice_lock = None,
            _ => {}
        }
    }

    /// White Herb's check (its `onStart`, run from `onAnySwitchIn`,
    /// `onAnyAfterMove`, `onAnyAfterMega` and `onResidual`).
    pub(crate) fn white_herb_check(&mut self, r: MonRef) {
        if self.item_kind(r) == Some(ItemKind::WhiteHerb)
            && self.mon(r).boosts.iter().any(|&b| b < 0)
        {
            self.use_item(r);
        }
    }

    /// `AfterMega`: White Herb's and Opportunist's `onAnyAfterMega` on every
    /// active holder, fastest first (an ability before an item).
    pub(crate) fn white_herbs_any(&mut self) {
        use crate::abilities::{compare_mod, ModHandler};
        let mut hs: Small<(MonRef, bool, ModHandler)> = Small::new();
        for r in self.all_active() {
            if self.raw_ability_kind(r) == crate::kinds::AbilityKind::Opportunist {
                hs.push((r, false, ModHandler::new(0, self.mon(r).speed, 7)));
            }
            if self.item_kind(r) == Some(ItemKind::WhiteHerb) {
                hs.push((r, true, ModHandler::new(0, self.mon(r).speed, 8)));
            }
        }
        if hs.is_empty() {
            return;
        }
        self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.2, &b.2));
        for &(r, item, _) in hs.iter() {
            if item {
                self.white_herb_check(r);
            } else {
                self.opportunist_release(r);
            }
        }
    }

    /// `eachEvent('TerrainChange')`: terrain seeds activate.
    pub(crate) fn terrain_change(&mut self) {
        let mut actives: Small<(MonRef, i32)> = self
            .all_active()
            .into_iter()
            .map(|r| (r, self.mon(r).speed))
            .collect();
        self.speed_sort_by(&mut actives, |a, b| b.1 as i64 - a.1 as i64);
        for (r, _) in actives {
            // The ability's handler (subOrder 7) comes before the item's (8).
            self.mimicry_on_terrain_change(r);
            if let Some(ItemKind::Seed(t)) = self.item_kind(r) {
                if self.field.terrain == Some(t) {
                    self.use_item(r);
                }
            }
        }
    }

    /// `AfterSetStatus`: Lum Berry cures at once.
    /// `AfterSetStatus`: Synchronize (priority 0) passes poison, paralysis
    /// and burn back to their source; then Lum Berry (-1) cures.
    pub(crate) fn after_set_status(
        &mut self,
        r: MonRef,
        st: Status,
        src: MonRef,
        effect: EffectRef,
    ) {
        let passes = matches!(st, Status::Psn | Status::Tox | Status::Par | Status::Brn);
        let synchronize = self.has_ability(r, crate::kinds::AbilityKind::Synchronize);
        if synchronize && src != r && passes && effect != EffectRef::ToxicSpikes {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|ability: Synchronize");
            // Messages as for a status move (Showdown's stand-in effect has `status`).
            self.active_move.synchronizing = true;
            self.try_set_status(src, st, r, EffectRef::Other, true);
            self.active_move.synchronizing = false;
        }
        if let Some(ItemKind::CureBerry(crate::kinds::CureBerry::Lum)) = self.item_kind(r) {
            self.eat_item(r);
        }
    }

    // ------------------------------------------------------------ stat and damage modifiers

    /// `BasePower` item handler (priority 15-16): the modifier, if any.
    pub(crate) fn item_base_power_mod(
        &self,
        src: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) -> Option<(u32, u32)> {
        match self.item_kind(src)? {
            ItemKind::TypeBoost(t) if t == move_type => Some((4915, 4096)),
            ItemKind::CategoryBoost(c) if c == mv.category => Some((4505, 4096)),
            _ => None,
        }
    }

    /// `ModifySpe` item handler: Choice Scarf.
    pub(crate) fn item_speed_mod(&self, r: MonRef) -> Option<(u32, u32)> {
        match self.item_kind(r)? {
            ItemKind::ChoiceScarf => Some((3, 2)),
            ItemKind::IronBall => Some((1, 2)),
            _ => None,
        }
    }

    /// `ModifyAtk` / `ModifySpA` item handlers (Light Ball).
    pub(crate) fn item_attack_mod(&self, src: MonRef) -> Option<(u32, u32)> {
        match self.item_kind(src)? {
            ItemKind::LightBall
                if self.dex.species(self.mon(src).base_species).base_species == "Pikachu" =>
            {
                Some((2, 1))
            }
            _ => None,
        }
    }

    /// `ModifyAccuracy` (target) and `SourceModifyAccuracy` (user) item handlers.
    pub(crate) fn item_accuracy(&self, src: MonRef, tgt: MonRef, mv: &Move, accuracy: u32) -> u32 {
        self.accuracy_chain(src, tgt, mv).apply(accuracy)
    }

    /// The `ModifyAccuracy` modifiers, chained.
    pub(crate) fn accuracy_chain(&self, src: MonRef, tgt: MonRef, mv: &Move) -> Chain {
        let mut chain = Chain::ONE;
        if let Some((n, d)) = self.gravity_on_modify_accuracy() {
            chain = chain.then(n, d);
        }
        if let Some((n, d)) = self.ability_accuracy_mod(src, mv) {
            chain = chain.then(n, d);
        }
        // Snow Cloak, Sand Veil (onModifyAccuracy, priority -1): x0.8 in their weather.
        if let crate::kinds::AbilityKind::WeatherEvasion(w) = self.ability_kind(tgt) {
            if self.effective_weather() == Some(w) {
                chain = chain.then(3277, 4096);
            }
        }
        // Tangled Feet (onModifyAccuracy, priority -1): x0.5 against a confused holder.
        if self.has_ability(tgt, crate::kinds::AbilityKind::TangledFeet)
            && self.mon(tgt).vol.confusion > 0
        {
            chain = chain.then(2048, 4096);
        }
        if self.item_kind(tgt) == Some(ItemKind::BrightPowder) {
            chain = chain.then(3686, 4096);
        }
        match self.item_kind(src) {
            Some(ItemKind::WideLens) => chain = chain.then(4505, 4096),
            Some(ItemKind::ZoomLens) if !self.has_queued_move(tgt) => {
                chain = chain.then(4915, 4096)
            }
            _ => {}
        }
        chain
    }

    /// Showdown `queue.willMove`: does this Pokémon still have a move queued?
    pub(crate) fn has_queued_move(&self, t: MonRef) -> bool {
        !self.mon(t).fainted
            && self.queue.iter().any(|a| matches!(a.kind, crate::state::ActionKind::Move { side, mon, .. } if (side, mon) == t))
    }

    /// Showdown's `runEvent('ModifyDamage', source, target)`. Every handler
    /// present joins the sort in Showdown's order, whether or not it changes
    /// this hit, since ties draw: the user's ability and item; Friend Guard on
    /// the user's side, then on its foes; the target's volatiles, ability and
    /// item; then screens, side by side, in the order they were set up. Each
    /// then applies in sorted order. Resist berries are eaten here.
    pub(crate) fn modify_damage_chain(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        crit: bool,
        type_mod: i32,
        move_type: TypeId,
    ) -> Chain {
        use DamageModHandler as H;
        let key = |speed: i32, sub_order: u32| ModHandler::new(0, speed, sub_order);
        let mut hs: arrayvec::ArrayVec<(H, ModHandler), 32> = arrayvec::ArrayVec::new();
        let speed = self.mon(src).speed;
        if self.mon(src).vol.metronome.is_some() {
            hs.push((H::Metronome, key(speed, 2)));
        }
        if self.has_ability(src, AbilityKind::Sniper) {
            hs.push((H::Sniper, key(speed, 7)));
        }
        match self.item_kind(src) {
            Some(ItemKind::LifeOrb) => hs.push((H::LifeOrb, key(speed, 8))),
            Some(ItemKind::ExpertBelt) => hs.push((H::ExpertBelt, key(speed, 8))),
            _ => {}
        }
        let mut around: arrayvec::ArrayVec<MonRef, 8> = arrayvec::ArrayVec::new();
        for s in [src.0, 1 - src.0] {
            around.extend(
                (0..self.sides[s].n_active)
                    .filter_map(|slot| self.active(s, slot))
                    .filter(|&r| self.mon(r).hp > 0),
            );
        }
        for r in around {
            if self.raw_ability_kind(r) == AbilityKind::FriendGuard {
                hs.push((H::FriendGuard(r), key(self.mon(r).speed, 7)));
            }
        }
        let speed = self.mon(tgt).speed;
        if self.mon(tgt).vol.minimize {
            hs.push((H::Minimize, key(speed, 2)));
        }
        if self.mon(tgt).vol.glaive_rush {
            hs.push((H::GlaiveRush, key(speed, 2)));
        }
        if self.semi_invulnerable_source_modify_damage(tgt) {
            hs.push((H::SemiInvulnerable, key(speed, 2)));
        }
        if matches!(
            self.raw_ability_kind(tgt),
            AbilityKind::Multiscale | AbilityKind::Filter | AbilityKind::AuraGuard
        ) || self.has_source_modify_damage(tgt)
        {
            hs.push((H::TargetAbility, key(speed, 7)));
        }
        if matches!(self.item_kind(tgt), Some(ItemKind::ResistBerry(_))) {
            hs.push((H::ResistBerry, key(speed, 8)));
        }
        for s in 0..2 {
            for c in self.sides[s].conds.in_order() {
                if matches!(
                    c,
                    SideCond::Reflect | SideCond::LightScreen | SideCond::AuroraVeil
                ) {
                    hs.push((H::Screen(s, c), key(0, 4)));
                }
            }
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        }
        let physical = mv.category == Category::Physical;
        let special = mv.category == Category::Special;
        let mut chain = Chain::ONE;
        for &(h, _) in hs.iter() {
            match h {
                H::Metronome => {
                    if let Some((n, d)) = self.metronome_on_modify_damage(src) {
                        chain = chain.then(n, d);
                    }
                }
                H::Sniper if crit => chain = chain.then(3, 2),
                H::LifeOrb => chain = chain.then(5324, 4096),
                H::ExpertBelt if type_mod > 0 => chain = chain.then(4915, 4096),
                H::FriendGuard(r)
                    if r != tgt
                        && r.0 == tgt.0
                        && !self.ability_suppressed(r)
                        && !self.ignoring_ability(r) =>
                {
                    chain = chain.then(3, 4)
                }
                H::GlaiveRush => chain = chain.then(2, 1),
                H::Minimize if mv.has(MoveFlag::Minimize) => chain = chain.then(2, 1),
                H::SemiInvulnerable if self.semi_invulnerable_doubles(tgt, &mv.id, false) => {
                    chain = chain.then(2, 1)
                }
                H::TargetAbility => {
                    let t = self.mon(tgt);
                    match self.ability_kind(tgt) {
                        AbilityKind::Multiscale if t.hp >= t.maxhp => chain = chain.then(1, 2),
                        AbilityKind::Filter if type_mod > 0 => chain = chain.then(3, 4),
                        AbilityKind::AuraGuard if mv.has(MoveFlag::Contact) => {
                            chain = chain.then(1, 2)
                        }
                        _ => {
                            if let Some((n, d)) = self.source_modify_damage(tgt, mv, move_type) {
                                chain = chain.then(n, d);
                            }
                        }
                    }
                }
                H::ResistBerry => {
                    let Some(ItemKind::ResistBerry(t)) = self.item_kind(tgt) else {
                        continue;
                    };
                    let normal = self.dex.type_names[t as usize] == "Normal";
                    // Not eaten when a substitute takes the hit.
                    if t == move_type
                        && (type_mod > 0 || normal)
                        && !self.hits_substitute(src, tgt, mv)
                    {
                        // A read-only calculation: the berry would be eaten
                        // (TryEatItem: Unnerve and Berserk stop it), and
                        // Ripen weakens the hit once more.
                        if self.probe {
                            if !self.unnerved(tgt) && !self.berserk_on_try_eat_item(tgt) {
                                chain = chain.then(1, 2);
                                if self.has_ability(tgt, AbilityKind::Ripen) {
                                    chain = chain.then(1, 2);
                                }
                            }
                            continue;
                        }
                        let name = self.item_name(tgt);
                        if self.eat_item_from(tgt, Some(src)) {
                            let ti = self.ident(tgt);
                            log_line!(self, "|-enditem|{ti}|{name}|[weaken]");
                            chain = chain.then(1, 2);
                        }
                    }
                }
                H::Screen(s, c) if s == tgt.0 && src != tgt && !crit && !self.infiltrates(src) => {
                    let conds = self.sides[s].conds;
                    let weakens = match c {
                        SideCond::Reflect => physical,
                        SideCond::LightScreen => special,
                        // Aurora Veil steps aside for the matching screen.
                        _ => {
                            !(physical && conds.reflect > 0) && !(special && conds.light_screen > 0)
                        }
                    };
                    if weakens {
                        chain = if self.sides[s].n_active > 1 {
                            chain.then(2732, 4096)
                        } else {
                            chain.then(1, 2)
                        };
                    }
                }
                _ => {}
            }
        }
        // Ripen's onSourceModifyDamage (priority -1, after the berry): the
        // resist berry it ate weakens the hit once more.
        if self.has_ability(tgt, AbilityKind::Ripen)
            && self.mon(tgt).ability_state.berry_weaken == Some(true)
        {
            self.mon_mut(tgt).ability_state.berry_weaken = Some(false);
            chain = chain.then(1, 2);
        }
        chain
    }

    /// The `Damage` event for damage from a move: Focus Sash and Focus Band.
    pub(crate) fn item_damage_event(&mut self, t: MonRef, amount: u32) -> u32 {
        let (hp, maxhp) = (self.mon(t).hp as u32, self.mon(t).maxhp as u32);
        match self.item_kind(t) {
            Some(ItemKind::FocusSash) => {
                if hp == maxhp && amount >= hp && self.use_item(t) {
                    return hp - 1;
                }
            }
            Some(ItemKind::FocusBand) if self.rng.chance(1, 10) && amount >= hp => {
                let ti = self.ident(t);
                log_line!(self, "|-activate|{ti}|item: Focus Band");
                return hp - 1;
            }
            _ => {}
        }
        amount
    }

    /// Big Root's `TryHeal` for draining moves.
    pub(crate) fn drain_heal_mod(&self, r: MonRef, amount: u32) -> u32 {
        if self.item_kind(r) == Some(ItemKind::BigRoot) {
            damage::modify(amount, 5324, 4096)
        } else {
            amount
        }
    }

    // ------------------------------------------------------------ after hits and moves

    /// `AfterMoveSecondarySelf`: Life Orb recoil, then Shell Bell (priority -1).
    pub(crate) fn after_move_secondary_self(
        &mut self,
        r: MonRef,
        last_target: MonRef,
        mv: &Move,
        total_damage: u32,
    ) {
        match self.item_kind(r) {
            Some(ItemKind::LifeOrb) => {
                // Not for a user Red Card is sending back (forceSwitchFlag).
                if r != last_target
                    && mv.category != Category::Status
                    && !self.mon(r).force_switch_flag
                {
                    let amount = (self.mon(r).maxhp as u32 / 10).max(1);
                    self.apply_damage(r, amount, "|[from] item: Life Orb");
                }
            }
            Some(ItemKind::ShellBell) if total_damage > 0 && !self.mon(r).force_switch_flag => {
                // battle.heal names the event's source, the move's target.
                let of = if last_target != r {
                    format!("|[of] {}", self.ident(last_target))
                } else {
                    String::new()
                };
                self.heal(
                    r,
                    (total_damage / 8).max(1),
                    &format!("|[from] item: Shell Bell{of}"),
                );
            }
            _ => {}
        }
    }

    /// Whether King's Rock adds a 10% flinch to this move.
    pub(crate) fn kings_rock_flinch(&self, src: MonRef, mv: &Move) -> bool {
        self.item_kind(src) == Some(ItemKind::KingsRock)
            && mv.category != Category::Status
            // A delayed hit never ran ModifyMove.
            && !mv.has(MoveFlag::FutureMove)
            && !mv.secondaries.iter().any(|s| s.volatile.as_deref() == Some("flinch"))
    }
}
