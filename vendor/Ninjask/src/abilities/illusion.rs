//! Illusion.

use crate::battle::{Battle, MonRef};
use crate::kinds::AbilityKind;

impl Battle {
    /// Illusion's `onBeforeSwitchIn`: the disguise is the last unfainted
    /// Pokémon after this one in the side's order (an active ally to its
    /// right included), or none.
    pub(crate) fn illusion_before_switch_in(&mut self, r: MonRef) {
        if !self.has_ability(r, AbilityKind::Illusion) {
            return;
        }
        let side = &self.sides[r.0];
        let pos = self.mon(r).position;
        let disguise = (pos + 1..side.mons.len())
            .rev()
            .find(|&i| !side.mons[i].fainted)
            .map(|i| side.mons[i].name.to_string());
        self.mon_mut(r).illusion = disguise;
    }

    /// The details a switch line shows (`getFullDetails`): a disguise's
    /// species, gender and shininess, at the Pokémon's own level.
    pub(crate) fn switch_details(&self, r: MonRef) -> String {
        let m = self.mon(r);
        let Some(i) = m
            .illusion
            .as_ref()
            .and_then(|n| self.sides[r.0].mons.iter().position(|x| *x.name == **n))
        else {
            return self.details(r.0, r.1);
        };
        let d = &self.sides[r.0].mons[i];
        let mut out = self.dex.species(d.species).name.clone();
        if m.level != 100 {
            out.push_str(&format!(", L{}", m.level));
        }
        if d.gender == "M" || d.gender == "F" {
            out.push_str(&format!(", {}", d.gender));
        }
        if d.shiny {
            out.push_str(", shiny");
        }
        out
    }

    /// Illusion's `onEnd` (from `onDamagingHit`): the disguise drops, even
    /// at 0 HP.
    pub(crate) fn end_illusion(&mut self, r: MonRef) {
        if self.mon_mut(r).illusion.take().is_none() {
            return;
        }
        let ident = self.ident(r);
        let m = self.mon(r);
        let mut details = self.dex.species(m.species).name.clone();
        if m.level != 100 {
            details.push_str(&format!(", L{}", m.level));
        }
        if m.gender == "M" || m.gender == "F" {
            details.push_str(&format!(", {}", m.gender));
        }
        if m.shiny {
            details.push_str(", shiny");
        }
        log_line!(self, "|replace|{ident}|{details}");
        log_line!(self, "|-end|{ident}|Illusion");
    }
}
