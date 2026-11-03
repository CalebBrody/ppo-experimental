//! Abilities that take things from others: Mummy, Pickpocket and Magician.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::AbilityKind;

impl Battle {
    /// Mummy's onDamagingHit: a contact attacker (unless its ability can't
    /// be suppressed or is Mummy) takes Mummy as its ability.
    pub(crate) fn mummy_on_damaging_hit(&mut self, src: MonRef, t: MonRef, contact: bool) {
        let a = self.dex.ability(self.mon(src).ability);
        if a.cant_suppress || a.id == "mummy" || !contact {
            return;
        }
        let mummy = self.dex.ids.abilities.mummy.expect("mummy");
        self.set_ability_from(src, mummy, Some(t), "ability: Mummy");
    }

    /// Pickpocket: hit by a contact move while holding nothing, the holder
    /// takes the attacker's item (`takeItem`, then `setItem`).
    pub(crate) fn pickpocket(&mut self, src: MonRef, t: MonRef, mv: &Move) {
        if src == t || !mv.has(MoveFlag::Contact) || self.mon(t).hp == 0 || !self.mon(t).is_active {
            return;
        }
        let m = self.mon(t);
        if m.item.is_some() || m.switch_flag || m.force_switch_flag {
            return;
        }
        // `source.switchFlag === true`: Eject Button's, not a U-turn's.
        let s = self.mon(src);
        if s.switch_flag && s.switch_from_move.is_none() {
            return;
        }
        let Some(item) = s.item else { return };
        // `source.takeItem(target)`: Sticky Hold, or Unburden (an ability)
        // takes note before a Mega Stone refuses.
        if self.sticky_hold_on_take_item(src, Some(t), false) {
            return;
        }
        self.after_use_item(src);
        if !self.item_removable(src) {
            return;
        }
        self.mon_mut(src).item = None;
        self.mon_mut(t).item = Some(item);
        // setItem: the item's Start (Air Balloon, seeds), then the messages.
        self.item_on_start(t);
        let (si, ti) = (self.ident(src), self.ident(t));
        let name = self.dex.item(item).name.clone();
        log_line!(
            self,
            "|-enditem|{si}|{name}|[silent]|[from] ability: Pickpocket|[of] {si}"
        );
        log_line!(
            self,
            "|-item|{ti}|{name}|[from] ability: Pickpocket|[of] {si}"
        );
    }

    /// Magician (`onAfterMoveSecondarySelf`): an item-less user of a damaging
    /// move takes the item of the first hit target, by speed, that gives one
    /// up (`takeItem`, then `setItem`). Whether it took one: the user's item
    /// handlers for the event were collected before it held anything.
    pub(crate) fn magician_on_after_move_secondary_self(
        &mut self,
        r: MonRef,
        hit: &[MonRef],
        mv: &Move,
    ) -> bool {
        use crate::abilities::{compare_mod, ModHandler};
        if !self.has_ability(r, AbilityKind::Magician)
            || mv.category == Category::Status
            || mv.id == "fling"
        {
            return false;
        }
        let m = self.mon(r);
        // `source.switchFlag === true`: Eject Button's, not a U-turn's.
        if (m.switch_flag && m.switch_from_move.is_none()) || m.item.is_some() || m.vol.gem {
            return false;
        }
        let mut targets: Small<(MonRef, ModHandler)> = hit
            .iter()
            .map(|&t| (t, ModHandler::new(0, self.mon(t).speed, 0)))
            .collect();
        if targets.len() > 1 {
            self.speed_sort_by(&mut targets, |a, b| compare_mod(&a.1, &b.1));
        }
        for (t, _) in targets {
            if t == r {
                continue;
            }
            let Some(item) = self.mon(t).item else {
                continue;
            };
            // TakeItem: Unburden (an ability) takes note before a Mega Stone
            // refuses; not for a target that fainted already (no longer
            // active, its ability is ignored; its item's handler still runs).
            if !self.mon(t).fainted && self.sticky_hold_on_take_item(t, Some(r), false) {
                continue;
            }
            if !self.mon(t).fainted {
                self.after_use_item(t);
            }
            if !self.item_removable(t) {
                continue;
            }
            // setItem fails on a user that fainted: the target keeps the item.
            if self.mon(r).hp == 0 || !self.mon(r).is_active {
                continue;
            }
            self.mon_mut(t).item = None;
            self.mon_mut(r).item = Some(item);
            // setItem: the item's Start (Air Balloon, seeds), then the message.
            self.item_on_start(r);
            let (ri, ti) = (self.ident(r), self.ident(t));
            let name = self.dex.item(item).name.clone();
            log_line!(
                self,
                "|-item|{ri}|{name}|[from] ability: Magician|[of] {ti}"
            );
            return true;
        }
        false
    }
}
