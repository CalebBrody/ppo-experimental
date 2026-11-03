//! Berries and other consumed items: `eatItem` and `Eat`, `TryEatItem`, `EatItem`, Ripen, Cud Chew, Harvest, Pickup and Mental Herb.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

impl Battle {
    /// Showdown `eatItem`: `-enditem ... [eat]`, then the berry's `onEat`.
    pub(crate) fn eat_item(&mut self, r: MonRef) -> bool {
        self.eat_item_from(r, None)
    }

    /// `eatItem(false, source)`: `source` defaults to the target of the event
    /// being run (the Pokémon itself, except for a resist berry, which runs in
    /// the attacker's ModifyDamage).
    pub(crate) fn eat_item_from(&mut self, r: MonRef, source: Option<MonRef>) -> bool {
        self.eat_item_with(r, source, false)
    }

    /// `eatItem(true)` (Stuff Cheeks): the `TryEatItem` event is skipped.
    pub(crate) fn eat_item_forced(&mut self, r: MonRef) -> bool {
        self.eat_item_with(r, None, true)
    }

    /// `eatItem(true, source)` (Teatime): the eater's own heal names `source`.
    pub(crate) fn eat_item_forced_from(&mut self, r: MonRef, source: MonRef) -> bool {
        self.eat_item_with(r, Some(source), true)
    }

    fn eat_item_with(&mut self, r: MonRef, source: Option<MonRef>, force: bool) -> bool {
        let m = self.mon(r);
        if m.hp == 0 || !m.is_active || m.item.is_none() {
            return false;
        }
        // TryEatItem: Unnerve on the other side stops berries; a healing
        // berry's own onTryEatItem asks TryHeal, which Heal Block refuses.
        let kind = self.item_kind(r).unwrap_or(ItemKind::Unsupported);
        if !force {
            self.event_sort(Event::TryEatItem, r, None, &[]);
            if self.unnerved(r) || self.berserk_on_try_eat_item(r) {
                return false;
            }
            if matches!(kind, ItemKind::SitrusBerry | ItemKind::OranBerry)
                && self.mon(r).vol.heal_block > 0
            {
                return false;
            }
        }
        let ident = self.ident(r);
        let name = self.item_name(r);
        // `heal` names the source when it is another Pokémon (Teatime's user).
        let of = match source.filter(|&s| s != r) {
            Some(s) => format!("|[of] {}", self.ident(s)),
            None => String::new(),
        };
        // Ripen's TryEatItem (priority -1, after the berry's own).
        self.ripen_on_try_eat_item(r);
        log_line!(self, "|-enditem|{ident}|{name}|[eat]");
        self.run_eat(r, kind, &name, &of);
        // runEvent('EatItem'): Cheek Pouch, Cud Chew.
        if let Some(item) = self.mon(r).item {
            self.run_eat_item(r, item, source.or(Some(r)), true);
        }
        self.consume_item(r, true);
        true
    }

    /// A berry's `onEat` (Showdown `singleEvent('Eat')`). `name` is the
    /// berry's name and `of` the `[of]` naming another Pokémon as the
    /// heal's source.
    pub(crate) fn run_eat(&mut self, r: MonRef, kind: ItemKind, name: &str, of: &str) {
        match kind {
            ItemKind::SitrusBerry => {
                let amount = self.ripen_on_try_heal((self.mon(r).maxhp as u32 / 4).max(1), r);
                self.heal(r, amount, &format!("|[from] item: {name}{of}"));
            }
            ItemKind::OranBerry => {
                let amount = self.ripen_on_try_heal(10, r);
                self.heal(r, amount, &format!("|[from] item: {name}{of}"));
            }
            ItemKind::CureBerry(c) => {
                if c.cures(self.mon(r).status) {
                    self.cure_status(r);
                }
                if c.cures_confusion() && self.mon(r).vol.confusion > 0 {
                    self.mon_mut(r).vol.confusion = 0;
                    let ri = self.ident(r);
                    log_line!(self, "|-end|{ri}|confusion");
                }
            }
            // Leppa Berry restores 10 PP (20 with Ripen) to the first move
            // out of PP, else the first one short of it.
            ItemKind::LeppaBerry => {
                let moves = &self.mon(r).moves;
                let Some(i) = moves
                    .iter()
                    .position(|ms| ms.pp == 0)
                    .or_else(|| moves.iter().position(|ms| ms.pp < ms.max_pp))
                else {
                    return;
                };
                let added = if self.ability_id(r) == "ripen" {
                    20
                } else {
                    10
                };
                let ms = &mut self.mon_mut(r).moves[i];
                ms.pp = (ms.pp + added).min(ms.max_pp);
                let name = self.dex.mv(self.mon(r).moves[i].id).name.clone();
                let ri = self.ident(r);
                log_line!(self, "|-activate|{ri}|item: Leppa Berry|{name}|[consumed]");
            }
            _ => {}
        }
    }

    /// A berry's `onEat` for `r`, which need not hold it (Bug Bite, Pluck,
    /// Fling).
    pub(crate) fn berry_on_eat(&mut self, r: MonRef, item: ItemId) {
        let name = self.dex.item(item).name.clone();
        self.run_eat(r, self.dex.item_kind[item as usize], &name, "");
    }

    /// Berserk's `onTryEatItem`: a healing berry waits while
    /// `checkedBerserk` is false.
    pub(crate) fn berserk_on_try_eat_item(&self, r: MonRef) -> bool {
        const HEALING: [&str; 9] = [
            "aguavberry",
            "enigmaberry",
            "figyberry",
            "iapapaberry",
            "magoberry",
            "sitrusberry",
            "wikiberry",
            "oranberry",
            "berryjuice",
        ];
        let Some(item) = self.mon(r).item else {
            return false;
        };
        self.has_ability(r, AbilityKind::Berserk)
            && self.mon(r).ability_state.checked_berserk == Some(false)
            && HEALING.contains(&self.dex.item(item).id.as_str())
    }

    /// Mental Herb, once used: removes Attract, Taunt, Encore, Torment,
    /// Disable and Heal Block, in that order, each announcing its end.
    pub(crate) fn mental_herb(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 {
            return;
        }
        self.mental_herb_on_use(r);
        let ri = self.ident(r);
        if self.mon(r).vol.taunt > 0 {
            self.mon_mut(r).vol.taunt = 0;
            log_line!(self, "|-end|{ri}|move: Taunt");
        }
        if self.mon(r).vol.encore.is_some() {
            let v = &mut self.mon_mut(r).vol;
            v.encore = None;
            v.encore_duration = 0;
            log_line!(self, "|-end|{ri}|Encore");
        }
        self.mental_herb_cures_torment(r);
        if self.mon(r).vol.disable.is_some() {
            let v = &mut self.mon_mut(r).vol;
            v.disable = None;
            v.disable_duration = 0;
            log_line!(self, "|-end|{ri}|Disable");
        }
        if self.mon(r).vol.heal_block > 0 {
            self.mon_mut(r).vol.heal_block = 0;
            log_line!(self, "|-end|{ri}|move: Heal Block");
        }
    }

    /// `runEvent('EatItem', r, source, effect, item)`: Cheek Pouch heals a
    /// third of its max HP; Cud Chew remembers the berry (not one Bug Bite or
    /// Pluck ate: `cud`). `source` is the event's source, named in the heal.
    pub(crate) fn run_eat_item(
        &mut self,
        r: MonRef,
        item: ItemId,
        source: Option<MonRef>,
        cud: bool,
    ) {
        match self.ability_kind(r) {
            AbilityKind::CheekPouch => {
                let amount = (self.mon(r).maxhp as u32 / 3).max(1);
                let of = match source {
                    Some(s) if s != r => format!("|[of] {}", self.ident(s)),
                    _ => String::new(),
                };
                self.heal(r, amount, &format!("|[from] ability: Cheek Pouch{of}"));
            }
            // Ripen's onEatItem: a resist berry weakens the next hit twice.
            AbilityKind::Ripen => {
                let weaken = matches!(self.dex.item_kind[item as usize], ItemKind::ResistBerry(_));
                self.mon_mut(r).ability_state.berry_weaken = Some(weaken);
            }
            AbilityKind::CudChew if cud && self.dex.item(item).is_berry => {
                // Eaten with nothing left in the queue (a residual): one turn less.
                let counter = if self.queue.is_empty() { 1 } else { 2 };
                let a = &mut self.mon_mut(r).ability_state;
                a.cud_berry = Some(item);
                a.cud_counter = Some(counter);
            }
            _ => {}
        }
    }

    /// Ripen's `onTryHeal` for a berry's heal: twice the amount.
    pub(crate) fn ripen_on_try_heal(&self, amount: u32, r: MonRef) -> u32 {
        if self.has_ability(r, AbilityKind::Ripen) {
            amount * 2
        } else {
            amount
        }
    }

    /// Ripen's `onTryEatItem`: announced whenever it eats a berry.
    pub(crate) fn ripen_on_try_eat_item(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::Ripen) {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|ability: Ripen");
        }
    }

    /// Ripen's `onTryHeal` for Leftovers (announced even at full HP, but not
    /// under Heal Block, whose `TryHeal` runs first).
    pub(crate) fn ripen_on_try_heal_leftovers(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::Ripen) && self.mon(r).vol.heal_block == 0 {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|ability: Ripen");
        }
    }

    /// Cud Chew's `onResidual`: after the counter runs out, the berry is
    /// eaten again (`Eat` and `EatItem`, with no checks).
    pub(crate) fn cud_chew(&mut self, r: MonRef) {
        let a = self.mon(r).ability_state;
        let (Some(item), Some(counter)) = (a.cud_berry, a.cud_counter) else {
            return;
        };
        if self.mon(r).hp == 0 {
            return;
        }
        let counter = counter - 1;
        self.mon_mut(r).ability_state.cud_counter = Some(counter);
        if counter > 0 {
            return;
        }
        let (ri, name) = (self.ident(r), self.dex.item(item).name.clone());
        log_line!(self, "|-activate|{ri}|ability: Cud Chew");
        log_line!(self, "|-enditem|{ri}|{name}|[eat]");
        self.berry_on_eat(r, item);
        self.run_eat_item(r, item, None, false);
        let m = self.mon_mut(r);
        m.ate_berry = true;
        m.ability_state.cud_berry = None;
        m.ability_state.cud_counter = None;
    }

    /// Pickup's `onResidual`: an adjacent Pokémon's item used this turn,
    /// picked at random, comes to a Pokémon holding none.
    pub(crate) fn pickup(&mut self, r: MonRef) {
        if self.mon(r).item.is_some() {
            return;
        }
        let targets: Small<MonRef> = self
            .all_active()
            .into_iter()
            .filter(|&t| {
                let m = self.mon(t);
                m.last_item.is_some() && m.used_item_this_turn && self.is_adjacent(r, t)
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        let t = self.rng.sample(&targets);
        let Some(item) = self.mon_mut(t).last_item.take() else {
            return;
        };
        let (ri, name) = (self.ident(r), self.dex.item(item).name.clone());
        log_line!(self, "|-item|{ri}|{name}|[from] ability: Pickup");
        self.mon_mut(r).item = Some(item);
        self.item_on_start(r);
    }

    pub(crate) fn harvest(&mut self, r: MonRef) {
        let sun = self.effective_weather() == Some(Weather::Sun);
        if !(sun || self.rng.chance(1, 2)) {
            return;
        }
        let m = self.mon(r);
        let Some(berry) = m.last_item.filter(|&i| self.dex.item(i).is_berry) else {
            return;
        };
        if m.hp == 0 || m.item.is_some() {
            return;
        }
        let m = self.mon_mut(r);
        m.item = Some(berry);
        m.last_item = None;
        let ri = self.ident(r);
        let name = self.dex.item(berry).name.clone();
        log_line!(self, "|-item|{ri}|{name}|[from] ability: Harvest");
    }

    /// `runEvent('AfterUseItem', r)`: the user's Unburden and Symbiosis on
    /// its side (`onAllyAfterUseItem`, itself included), by speed, ties drawn.
    pub(crate) fn run_after_use_item(&mut self, r: MonRef) {
        use crate::abilities::{compare_mod, ModHandler};
        let mut hs: Small<(Option<MonRef>, ModHandler)> = Small::new();
        if self.has_ability(r, AbilityKind::Unburden) {
            hs.push((None, ModHandler::new(0, self.mon(r).speed, 7)));
        }
        let mut side: Small<MonRef> = smallvec::smallvec![r];
        side.extend(self.allies_of(r));
        for h in side {
            if self.has_ability(h, AbilityKind::Symbiosis)
                && self.mon(h).hp > 0
                && !self.mon(h).fainted
            {
                hs.push((Some(h), ModHandler::new(0, self.mon(h).speed, 7)));
            }
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        }
        for &(h, _) in hs.iter() {
            match h {
                None => self.after_use_item(r),
                Some(h) => self.symbiosis(h, r),
            }
        }
    }

    /// Mental Herb's cure, first in its list: Attract ends (silently), and
    /// the herb names it when it was what set the herb off.
    pub(crate) fn mental_herb_on_use(&mut self, r: MonRef) {
        if self.mon(r).vol.attract.is_none() {
            return;
        }
        self.end_attract(r);
        let ri = self.ident(r);
        log_line!(self, "|-end|{ri}|move: Attract|[from] item: Mental Herb");
    }
}
