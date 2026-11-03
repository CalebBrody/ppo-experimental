//! Forme changes: Disguise, Stance Change, Hunger Switch, Zen Mode and species changes.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::AbilityKind;

impl Battle {
    /// Showdown `setSpecies`: the species, its types and stats (HP aside),
    /// and the stored speed becomes the new Speed stat.
    pub(crate) fn set_species(&mut self, r: MonRef, species: SpeciesId) {
        let sp = self.dex.species(species).clone();
        let m = self.mon_mut(r);
        let nature = crate::dex::Nature {
            plus: m.nature_plus,
            minus: m.nature_minus,
        };
        let stats = crate::dex::team::champions_stats(&sp.base_stats, &m.points, nature);
        m.species = species;
        m.types = sp.types;
        m.added_type = NO_TYPE;
        for st in [Stat::Atk, Stat::Def, Stat::Spa, Stat::Spd, Stat::Spe] {
            m.stats[st as usize] = stats[st as usize];
        }
        m.speed = m.stats[Stat::Spe as usize] as i32;
    }

    /// `clearVolatile` -> `setSpecies(baseSpecies)` on switching out or
    /// fainting: a temporary forme (Aegislash-Blade) reverts, silently.
    pub(crate) fn revert_forme(&mut self, r: MonRef) {
        let m = self.mon(r);
        if m.species != m.base_species {
            self.set_species(r, m.base_species);
        }
    }

    /// Champions `formeChange`. A temporary change shows `-formechange`,
    /// naming the ability when the change runs in the ability's own event
    /// (`ability`, Showdown's `this.effect`); a permanent one becomes the
    /// base species and shows `detailschange`.
    pub(crate) fn forme_change(
        &mut self,
        r: MonRef,
        name: &str,
        permanent: bool,
        ability: Option<&str>,
    ) {
        let Some(sp) = self.dex.species_id(name) else {
            return;
        };
        self.set_species(r, sp);
        let ident = self.ident(r);
        if permanent {
            self.mon_mut(r).base_species = sp;
            let details = self.details(r.0, r.1);
            log_line!(self, "|detailschange|{ident}|{details}");
        } else {
            match ability {
                Some(a) => log_line!(self, "|-formechange|{ident}|{name}||[from] ability: {a}"),
                None => log_line!(self, "|-formechange|{ident}|{name}|"),
            }
        }
    }

    /// Stance Change: Aegislash takes its Blade forme for an attack and its
    /// Shield forme for King's Shield.
    pub(crate) fn stance_change(&mut self, r: MonRef, mv: &Move) {
        if self.dex.species(self.mon(r).species).base_species != "Aegislash"
            || self.mon(r).transformed.is_some()
        {
            return;
        }
        if mv.category == Category::Status && mv.id != "kingsshield" {
            return;
        }
        let target = if mv.id == "kingsshield" {
            "Aegislash"
        } else {
            "Aegislash-Blade"
        };
        if self.dex.species(self.mon(r).species).name != target {
            // A runEvent handler: Showdown's `this.effect` is not the ability.
            self.forme_change(r, target, false, None);
        }
    }

    /// Hunger Switch's `onResidual` (order 29): Morpeko and Morpeko-Hangry
    /// trade formes.
    pub(crate) fn hunger_switch(&mut self, r: MonRef) {
        // The handler was sorted while Gastro Acid could not hide it; the
        // run skips a suppressed ability.
        if !self.has_ability(r, AbilityKind::HungerSwitch) {
            return;
        }
        let sp = self.dex.species(self.mon(r).species);
        if sp.base_species != "Morpeko" {
            return;
        }
        let target = if sp.name == "Morpeko" {
            "Morpeko-Hangry"
        } else {
            "Morpeko"
        };
        self.forme_change(r, target, false, None);
    }

    /// Disguise's handlers act while the Pokémon is Mimikyu in disguise.
    pub(crate) fn disguised(&self, t: MonRef) -> bool {
        self.has_ability(t, AbilityKind::Disguise)
            && self.dex.species(self.mon(t).species).id == "mimikyu"
    }

    /// Disguise's `onEffectiveness` and `onCriticalHit`: a hit on the
    /// disguise (not on a substitute) is neutral and can't be critical.
    pub(crate) fn disguise_shields(&self, src: MonRef, t: MonRef, mv: &Move) -> bool {
        mv.category != Category::Status && self.disguised(t) && !self.hits_substitute(src, t, mv)
    }

    /// Disguise's `onDamage` (priority 1) for damage from a move (confusion
    /// included): the disguise takes it, and the Damage event stops there.
    /// The damage line still shows, at full HP.
    pub(crate) fn disguise_damage(&mut self, t: MonRef, from: &str) -> bool {
        if !self.disguised(t) {
            return false;
        }
        let ti = self.ident(t);
        log_line!(self, "|-activate|{ti}|ability: Disguise");
        self.mon_mut(t).ability_state.busted = Some(true);
        self.add_health(format_args!("|-damage|{ti}"), t, from);
        true
    }
}
