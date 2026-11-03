//! Fling.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

impl Battle {
    /// Fling's `onPrepareHit`: the held item's power and effect. `None` when
    /// the move fails; else the move as thrown.
    pub(crate) fn fling_on_prepare_hit(&mut self, r: MonRef, mv: &Move) -> Option<Move> {
        if self.ignoring_item(r) || !self.mon(r).is_active {
            return None;
        }
        let item = self.mon(r).item?;
        if !self.item_receivable(r, item) {
            return None;
        }
        let it = self.dex.item(item).clone();
        if it.fling_power == 0 {
            return None;
        }
        let mut m = mv.clone();
        m.base_power = it.fling_power;
        self.active_move.fling = None;
        let sec = |chance: Option<u8>, status: Option<Status>, volatile: Option<&str>| {
            crate::dex::Secondary {
                chance,
                status,
                volatile: volatile.map(|v| v.to_string()),
                boosts: None,
                self_boosts: None,
            }
        };
        if it.is_berry {
            if self.has_ability(r, AbilityKind::CudChew) {
                self.run_eat_item(r, item, Some(r), true);
            }
            self.active_move.fling = Some((item, r));
        } else if matches!(it.id.as_str(), "mentalherb" | "whiteherb") {
            self.active_move.fling = Some((item, r));
        } else if it.id == "lightball" {
            m.secondaries.push(sec(None, Some(Status::Par), None));
        } else if it.id == "poisonbarb" {
            m.secondaries.push(sec(None, Some(Status::Psn), None));
        } else if it.id == "kingsrock" {
            // ModifyMove ran first: King's Rock added its own 10% flinch.
            m.secondaries.push(sec(Some(10), None, Some("flinch")));
            m.secondaries.push(sec(None, None, Some("flinch")));
        }
        // addVolatile('fling')
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        self.mon_mut(r).vol.fling = true;
        self.stamp_volatile(r, ResidualVol::Fling);
        Some(m)
    }

    /// Fling's `onHit` on a target: a berry is eaten (`Eat`, then `EatItem`),
    /// a Mental or White Herb works on it.
    pub(crate) fn fling_on_hit(&mut self, t: MonRef) {
        let Some((item, src)) = self.active_move.fling else {
            return;
        };
        let it = self.dex.item(item).clone();
        if it.is_berry {
            if !self.ignoring_item(t) {
                self.fling_on_eat(t, src, item);
            }
            self.run_eat_item(t, item, Some(src), true);
            self.mon_mut(t).ate_berry = true;
        } else if it.id == "mentalherb" {
            self.fling_mental_herb(t);
        } else if it.id == "whiteherb" {
            let ti = self.ident(t);
            if self.mon(t).boosts.iter().any(|&b| b < 0) {
                for b in self.mon_mut(t).boosts.iter_mut() {
                    if *b < 0 {
                        *b = 0;
                    }
                }
                log_line!(self, "|-clearnegativeboost|{ti}|[silent]");
            }
        }
    }

    /// A berry's `onEat` on the target, with the thrower as the event source.
    pub(crate) fn fling_on_eat(&mut self, t: MonRef, src: MonRef, item: ItemId) {
        let name = self.dex.item(item).name.clone();
        let of = format!("|[of] {}", self.ident(src));
        match self.dex.item_kind[item as usize] {
            ItemKind::SitrusBerry => {
                let amount = self.ripen_on_try_heal((self.mon(t).maxhp as u32 / 4).max(1), t);
                self.heal(t, amount, &format!("|[from] item: {name}{of}"));
            }
            ItemKind::OranBerry => {
                let amount = self.ripen_on_try_heal(10, t);
                self.heal(t, amount, &format!("|[from] item: {name}{of}"));
            }
            _ => self.berry_on_eat(t, item),
        }
    }

    /// Mental Herb's thrown effect: no `-enditem`, no hp check.
    pub(crate) fn fling_mental_herb(&mut self, t: MonRef) {
        let ti = self.ident(t);
        let v = self.mon(t).vol;
        if v.attract.is_none()
            && v.taunt == 0
            && v.encore.is_none()
            && !v.torment
            && v.disable.is_none()
            && v.heal_block == 0
        {
            return;
        }
        self.mental_herb_on_use(t);
        if self.mon(t).vol.taunt > 0 {
            self.mon_mut(t).vol.taunt = 0;
            log_line!(self, "|-end|{ti}|move: Taunt");
        }
        if self.mon(t).vol.encore.is_some() {
            let v = &mut self.mon_mut(t).vol;
            v.encore = None;
            v.encore_duration = 0;
            log_line!(self, "|-end|{ti}|Encore");
        }
        self.mental_herb_cures_torment(t);
        if self.mon(t).vol.disable.is_some() {
            let v = &mut self.mon_mut(t).vol;
            v.disable = None;
            v.disable_duration = 0;
            log_line!(self, "|-end|{ti}|Disable");
        }
        if self.mon(t).vol.heal_block > 0 {
            self.mon_mut(t).vol.heal_block = 0;
            log_line!(self, "|-end|{ti}|move: Heal Block");
        }
    }

    /// The `fling` volatile's `onUpdate`: the thrown item is gone.
    pub(crate) fn fling_on_update(&mut self, r: MonRef) {
        if !self.mon(r).vol.fling {
            return;
        }
        self.mon_mut(r).vol.fling = false;
        let Some(item) = self.mon(r).item else { return };
        let (ri, name) = (self.ident(r), self.dex.item(item).name.clone());
        log_line!(self, "|-enditem|{ri}|{name}|[from] move: Fling");
        self.consume_item(r, false);
    }
}
