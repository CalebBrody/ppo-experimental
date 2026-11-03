//! The `Update` event (`eachEvent('Update')`): the volatiles', abilities'
//! and items' `onUpdate` handlers.

use arrayvec::ArrayVec;

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

impl Battle {
    /// The volatiles' `Update` handlers (subOrder 2): Attract,
    /// Fling and Syrup Bomb. They run in insertion order, and equal ones draw.
    pub(crate) fn volatile_update(&mut self, r: MonRef) {
        use crate::abilities::{compare_mod, ModHandler};
        let m = self.mon(r);
        let v = &m.vol;
        let mut hs: ArrayVec<(u32, ResidualVol), 3> = ArrayVec::new();
        if v.attract.is_some() {
            hs.push((v.added[ResidualVol::Attract as usize], ResidualVol::Attract));
        }
        if v.fling {
            hs.push((v.added[ResidualVol::Fling as usize], ResidualVol::Fling));
        }
        if v.syrup_bomb.is_some() {
            hs.push((
                v.added[ResidualVol::SyrupBomb as usize],
                ResidualVol::SyrupBomb,
            ));
        }
        if hs.is_empty() {
            return;
        }
        let speed = m.speed;
        hs.sort_by_key(|h| h.0);
        let mut keyed: ArrayVec<(ModHandler, ResidualVol), 3> = hs
            .into_iter()
            .map(|(_, k)| (ModHandler::new(0, speed, 2), k))
            .collect();
        if keyed.len() > 1 {
            self.speed_sort_by(&mut keyed, |a, b| compare_mod(&a.0, &b.0));
        }
        for &(_, k) in keyed.iter() {
            match k {
                ResidualVol::Attract => self.volatiles_on_update(r),
                ResidualVol::Fling => self.fling_on_update(r),
                _ => self.syrup_bomb_on_update(r),
            }
        }
    }

    /// `addVolatile('syrupbomb', source)` from the move's secondary: nothing
    /// when already there; TryAddVolatile; the start line. Four turns.
    pub(crate) fn add_syrup_bomb(&mut self, t: MonRef, src: MonRef) {
        if self.mon(t).hp == 0 || self.mon(t).vol.syrup_bomb.is_some() {
            return;
        }
        if self.run_try_add_volatile(t, "syrupbomb", false) {
            return;
        }
        let v = &mut self.mon_mut(t).vol;
        v.syrup_bomb = Some(src);
        v.syrup_bomb_duration = 4;
        self.stamp_volatile(t, ResidualVol::SyrupBomb);
        let ti = self.ident(t);
        log_line!(self, "|-start|{ti}|Syrup Bomb");
    }

    /// The `syrupbomb` volatile's `onUpdate` ends it once its thrower has left
    /// the field.
    pub(crate) fn syrup_bomb_on_update(&mut self, r: MonRef) {
        if let Some(src) = self.mon(r).vol.syrup_bomb {
            if !self.mon(src).is_active || self.mon(src).fainted {
                let v = &mut self.mon_mut(r).vol;
                v.syrup_bomb = None;
                v.syrup_bomb_duration = 0;
                let ri = self.ident(r);
                log_line!(self, "|-end|{ri}|Syrup Bomb|[silent]");
            }
        }
    }

    /// Showdown `eachEvent('Update')`, on each active Pokémon in speed order:
    /// its volatiles' handlers (subOrder 2), then its ability's (7), then its
    /// item's (8), as in Showdown. A Pokémon at 0 HP that has not fainted yet
    /// still has the first two (a Wandering Spirit swap into Thermal
    /// Exchange).
    #[track_caller]
    pub(crate) fn run_update(&mut self) {
        // Usually no handler can act: then only the order's draws (and its
        // trace) matter, and without a speed tie it makes none.
        // One pass over the active Pokémon: whether a handler may act, and
        // whether two speeds tie.
        let mut speeds = [0i32; crate::state::MAX_ACTIVE];
        let (mut n, mut tie, mut acts) = (0, false, false);
        for (s, side) in self.sides.iter().enumerate() {
            for (slot, m) in side.mons.iter().enumerate().take(side.n_active) {
                if m.is_active && !m.fainted {
                    acts |= self.update_may_act((s, slot));
                    tie |= speeds[..n].contains(&m.speed);
                    if n < speeds.len() {
                        speeds[n] = m.speed;
                        n += 1;
                    }
                }
            }
        }
        if !acts {
            // The sort draws only between equal speeds.
            if tie || self.rng.trace.is_some() {
                self.each_event_order();
            }
            return;
        }
        for r in self.each_event_order() {
            self.volatile_update(r);
            self.ability_update(r);
            self.item_update(r);
        }
    }

    /// Whether any of `r`'s `Update` handlers could act now: a superset of
    /// the conditions `volatile_update`, `ability_update` and `item_update`
    /// test. When it is false for every active Pokémon, none of them does
    /// anything, so none can make another's act either.
    fn update_may_act(&self, r: MonRef) -> bool {
        let m = self.mon(r);
        let v = &m.vol;
        if v.attract.is_some() || v.fling || v.syrup_bomb.is_some() {
            return true;
        }
        let ability = match self.raw_ability_kind(r) {
            AbilityKind::Disguise => true,
            AbilityKind::StatusImmune(_)
            | AbilityKind::ThermalExchange
            | AbilityKind::WaterBubble => m.status != Status::None,
            AbilityKind::Trace => m.ability_state.seek == Some(true),
            AbilityKind::IntimidateImmune => v.taunt > 0 || v.confusion > 0,
            _ => false,
        };
        if ability {
            return true;
        }
        if m.hp == 0 {
            return false;
        }
        match self.held_item_kind(r) {
            Some(ItemKind::SitrusBerry | ItemKind::OranBerry) => m.hp as u32 * 2 <= m.maxhp as u32,
            Some(ItemKind::CureBerry(_)) => m.status != Status::None || v.confusion > 0,
            Some(ItemKind::MentalHerb) => {
                v.attract.is_some()
                    || v.taunt > 0
                    || v.encore.is_some()
                    || v.torment
                    || v.disable.is_some()
                    || v.heal_block > 0
            }
            Some(ItemKind::LeppaBerry) => m.moves.iter().any(|ms| ms.pp == 0),
            _ => false,
        }
    }

    /// The abilities' `onUpdate`: Disguise, status-immune abilities cure,
    /// Trace, Oblivious, Own Tempo.
    pub(crate) fn ability_update(&mut self, r: MonRef) {
        // Only these abilities have an Update handler (Oblivious and Own
        // Tempo are IntimidateImmune). A suppressed ability's kind is None,
        // so when the raw ability is none of them, no handler below acts.
        if !matches!(
            self.raw_ability_kind(r),
            AbilityKind::Disguise
                | AbilityKind::StatusImmune(_)
                | AbilityKind::ThermalExchange
                | AbilityKind::WaterBubble
                | AbilityKind::Trace
                | AbilityKind::IntimidateImmune
        ) {
            return;
        }
        // A busted Disguise changes Mimikyu's forme for good and costs it
        // an eighth of its max HP.
        if self.disguised(r) && self.mon(r).ability_state.busted == Some(true) {
            self.forme_change(r, "Mimikyu-Busted", true, Some("Disguise"));
            let amount = (self.mon(r).maxhp as u32 / 8).max(1);
            self.apply_damage(r, amount, "|[from] pokemon: Mimikyu-Busted");
        }
        let st = self.mon(r).status;
        let cures = match self.ability_kind(r) {
            AbilityKind::StatusImmune(s) => st == s || (s == Status::Psn && st == Status::Tox),
            AbilityKind::ThermalExchange | AbilityKind::WaterBubble => st == Status::Brn,
            _ => false,
        };
        if cures {
            if self.log_enabled {
                let (ident, name) = (self.ident(r), self.ability_name(r));
                log_line!(self, "|-activate|{ident}|ability: {name}");
            }
            self.cure_status(r);
        }
        if self.has_ability(r, AbilityKind::Trace) && self.mon(r).ability_state.seek == Some(true) {
            self.trace_update(r);
        }
        // Oblivious ends an Attract, then a Taunt, that got through (Mold
        // Breaker's).
        if self.mon(r).vol.attract.is_some() && self.ability_id(r) == "oblivious" {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|ability: Oblivious");
            self.end_attract(r);
            log_line!(self, "|-end|{ri}|move: Attract|[from] ability: Oblivious");
        }
        if self.mon(r).vol.taunt > 0 && self.ability_id(r) == "oblivious" {
            let ident = self.ident(r);
            log_line!(self, "|-activate|{ident}|ability: Oblivious");
            self.mon_mut(r).vol.taunt = 0;
            log_line!(self, "|-end|{ident}|move: Taunt");
        }
        if self.mon(r).vol.confusion > 0 && self.ability_id(r) == "owntempo" {
            let ident = self.ident(r);
            log_line!(self, "|-activate|{ident}|ability: Own Tempo");
            self.mon_mut(r).vol.confusion = 0;
            log_line!(self, "|-end|{ident}|confusion");
        }
    }

    /// The items' `onUpdate`: healing and cure berries, Mental Herb and
    /// Leppa Berry. Items do nothing without HP.
    pub(crate) fn item_update(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 {
            return;
        }
        // Only these items have an Update handler; an ignored item's kind
        // is None, so when the held item is none of them, nothing acts.
        if !matches!(
            self.held_item_kind(r),
            Some(
                ItemKind::SitrusBerry
                    | ItemKind::OranBerry
                    | ItemKind::CureBerry(_)
                    | ItemKind::MentalHerb
                    | ItemKind::LeppaBerry
            )
        ) {
            return;
        }
        let m = self.mon(r);
        match self.item_kind(r) {
            Some(ItemKind::SitrusBerry) | Some(ItemKind::OranBerry) => {
                if m.hp as u32 * 2 <= m.maxhp as u32 {
                    self.eat_item(r);
                }
            }
            Some(ItemKind::CureBerry(c)) => {
                if c.cures(m.status) || (c.cures_confusion() && m.vol.confusion > 0) {
                    self.eat_item(r);
                }
            }
            Some(ItemKind::MentalHerb) => {
                let v = self.mon(r).vol;
                if (v.attract.is_some()
                    || v.taunt > 0
                    || v.encore.is_some()
                    || v.torment
                    || v.disable.is_some()
                    || v.heal_block > 0)
                    && self.use_item(r)
                {
                    self.mental_herb(r);
                }
            }
            Some(ItemKind::LeppaBerry) if self.mon(r).moves.iter().any(|ms| ms.pp == 0) => {
                self.eat_item(r);
            }
            _ => {}
        }
    }
}
