//! Items changing hands: Symbiosis and Red Card.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, ItemKind};

impl Battle {
    /// Symbiosis: `h` gives its item to its ally `r`, which just used its own.
    pub(crate) fn symbiosis(&mut self, h: MonRef, r: MonRef) {
        // `onAllyAfterUseItem` runs for the holder itself too: when an earlier
        // Symbiosis just handed it an item, it passes that on to itself.
        if self.mon(r).switch_flag {
            return;
        }
        let Some(item) = self.mon(h).item else { return };
        // takeItem on the holder (a Mega Stone stays), then setItem on the ally.
        if !self.item_receivable(h, item) {
            return;
        }
        if !self.item_receivable(r, item) || self.mon(r).hp == 0 || !self.mon(r).is_active {
            return;
        }
        self.mon_mut(h).item = None;
        self.mon_mut(r).item = Some(item);
        self.item_on_start(r);
        let (hi, ri) = (self.ident(h), self.ident(r));
        let name = self.dex.item(item).name.clone();
        log_line!(self, "|-activate|{hi}|ability: Symbiosis|{name}|[of] {ri}");
    }

    /// Red Card: the holder, hit by a damaging move, is used up and drags
    /// the attacker out, unless DragOut holds it (Guard Dog, Suction Cups,
    /// Ingrain). The card is spent either way.
    pub(crate) fn red_card(&mut self, src: MonRef, t: MonRef, mv: &Move) {
        if src == t
            || mv.category == Category::Status
            || self.mon(src).hp == 0
            || self.mon(t).hp == 0
        {
            return;
        }
        if self.item_kind(t) != Some(ItemKind::RedCard) {
            return;
        }
        if !self.mon(src).is_active
            || !self.can_switch_side(src.0)
            || self.mon(src).force_switch_flag
            || self.mon(t).force_switch_flag
        {
            return;
        }
        if !self.use_item_from(t, Some(src)) {
            return;
        }
        // runEvent('DragOut', source): the abilities (priority 1), then Ingrain.
        if matches!(
            self.ability_kind(src),
            AbilityKind::GuardDog | AbilityKind::SuctionCups
        ) {
            let si = self.ident(src);
            let name = self.ability_name(src);
            log_line!(self, "|-activate|{si}|ability: {name}");
            return;
        }
        if self.ingrain_on_drag_out(src) {
            return;
        }
        self.mon_mut(src).force_switch_flag = true;
    }
}
