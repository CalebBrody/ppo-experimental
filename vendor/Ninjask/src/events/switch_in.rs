//! `SwitchIn`, `Start`, `End` and `SwitchOut` events of abilities, and the
//! switch-in priority of their handlers.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::{AbilityKind, AbsorbEffect, EffectRef};

impl Battle {
    /// Priority of an ability's switch-in (`onStart`) handler, if it has one.
    pub(crate) fn ability_switch_in_priority(&self, r: MonRef) -> Option<i32> {
        match self.ability_kind(r) {
            AbilityKind::Intimidate
            | AbilityKind::SetWeather(_)
            | AbilityKind::SetTerrain(_)
            | AbilityKind::Aura(_)
            | AbilityKind::Pressure
            | AbilityKind::Trace
            | AbilityKind::Imposter => Some(0),
            AbilityKind::Unnerve => Some(1),
            AbilityKind::Hospitality => Some(-2),
            AbilityKind::MoldBreaker | AbilityKind::ZeroToHero => Some(0),
            AbilityKind::SuppressWeather
            | AbilityKind::CuriousMedicine
            | AbilityKind::Gluttony
            | AbilityKind::ScreenCleaner => Some(0),
            AbilityKind::Forecast => Some(-2),
            AbilityKind::Frisk
            | AbilityKind::Anticipation
            | AbilityKind::Forewarn
            | AbilityKind::SuperSweetSyrup => Some(0),
            // Klutz's onStart (the item's End) goes before every item's.
            AbilityKind::Klutz => Some(1),
            AbilityKind::Mimicry => Some(-1),
            AbilityKind::SupremeOverlord => Some(0),
            _ => None,
        }
    }

    /// An ability's `onStart` (run as `onSwitchIn` in Gen 5+, and after Mega Evolution).
    pub(crate) fn run_start(&mut self, r: MonRef) {
        // singleEvent('Start') skips a suppressed ability (Gastro Acid).
        if self.ignoring_ability(r) {
            return;
        }
        let ability = self.mon(r).ability;
        let dex = self.dex.clone();
        let name = dex.ability(ability).name.as_str();
        let ident = self.ident(r);
        match self.dex.ability_kind[ability as usize] {
            AbilityKind::Intimidate => {
                let targets = self.foes_of(r);
                if !targets.is_empty() {
                    log_line!(self, "|-ability|{ident}|Intimidate|boost");
                }
                for t in targets {
                    // A substitute makes the foe immune.
                    if self.mon(t).vol.substitute > 0 {
                        let ti = self.ident(t);
                        log_line!(self, "|-immune|{ti}");
                        continue;
                    }
                    let mut b = [0i8; 7];
                    b[Boost::Atk as usize] = -1;
                    self.boost(t, &b, Some(r), EffectRef::Ability(ability), true, false);
                }
            }
            AbilityKind::SetWeather(w) => {
                self.set_weather(w, r, Some(name));
            }
            AbilityKind::SetTerrain(t) => {
                self.set_terrain(t, r, Some(name));
            }
            AbilityKind::Aura(_) => {
                // Fairy Aura and Dark Aura's onStart return early while a
                // move that ignores abilities is active (Roar from a Mold
                // Breaker user drags the holder in), breakable or not.
                if !self.suppressing_ability(r) {
                    log_line!(self, "|-ability|{ident}|{name}");
                }
            }
            AbilityKind::Pressure => {
                log_line!(self, "|-ability|{ident}|{name}");
            }
            AbilityKind::Unnerve => {
                if self.mon(r).ability_state.unnerved != Some(true) {
                    log_line!(self, "|-ability|{ident}|{name}");
                    self.mon_mut(r).ability_state.unnerved = Some(true);
                }
            }
            AbilityKind::Trace => {
                // Seek an ability to copy, unless an adjacent foe has No Ability.
                // `foe.ability === 'noability'`: the raw id, so a foe under
                // Gastro Acid doesn't count.
                let blocked = self
                    .foes_of(r)
                    .into_iter()
                    .any(|f| self.dex.ability(self.mon(f).ability).id == "noability");
                self.mon_mut(r).ability_state.seek = Some(!blocked);
                if !blocked {
                    self.trace_update(r);
                }
            }
            AbilityKind::Hospitality => {
                for ally in self.allies_of(r) {
                    let amount = (self.mon(ally).maxhp as u32 / 4).max(1);
                    self.heal(
                        ally,
                        amount,
                        &format!("|[from] ability: Hospitality|[of] {ident}"),
                    );
                }
            }
            // The rest go by the effective ability (`ability_kind`: none
            // while suppressed, e.g. a breakable one under Mold Breaker).
            _ => match self.ability_kind(r) {
                AbilityKind::MoldBreaker => {
                    let ident = self.ident(r);
                    log_line!(self, "|-ability|{ident}|Mold Breaker");
                }
                AbilityKind::ZeroToHero => {
                    let sp = self.dex.species(self.mon(r).species);
                    if sp.base_species == "Palafin"
                        && sp.name == "Palafin-Hero"
                        && !self.mon(r).hero_message_displayed
                    {
                        let ident = self.ident(r);
                        log_line!(self, "|-activate|{ident}|ability: Zero to Hero");
                        self.mon_mut(r).hero_message_displayed = true;
                    }
                }
                AbilityKind::SuppressWeather => {
                    self.mon_mut(r).ability_state.ending = Some(false);
                    self.weather_change_event();
                }
                AbilityKind::CuriousMedicine => {
                    for ally in self.allies_of(r) {
                        self.mon_mut(ally).boosts = [0; 7];
                        let (ai, ri) = (self.ident(ally), self.ident(r));
                        log_line!(
                            self,
                            "|-clearboost|{ai}|[from] ability: Curious Medicine|[of] {ri}"
                        );
                    }
                }
                AbilityKind::Gluttony => self.mon_mut(r).ability_state.gluttony = Some(true),
                AbilityKind::ScreenCleaner => self.screen_cleaner(r),
                // singleEvent('WeatherChange') on itself.
                AbilityKind::Forecast => self.forecast(r),
                AbilityKind::SuperSweetSyrup => self.super_sweet_syrup(r),
                AbilityKind::Mimicry => self.mimicry_terrain_change(r),
                AbilityKind::Anticipation => self.anticipation(r),
                AbilityKind::Forewarn => self.forewarn(r),
                AbilityKind::Frisk => {
                    let ri = self.ident(r);
                    for f in self.foes_of(r) {
                        if let Some(item) = self.mon(f).item {
                            let fi = self.ident(f);
                            let name = self.dex.item(item).name.clone();
                            log_line!(self, "|-item|{fi}|{name}|[from] ability: Frisk|[of] {ri}");
                        }
                    }
                }
                AbilityKind::SupremeOverlord
                    // Counts the fallen allies.
                    if self.sides[r.0].total_fainted > 0 => {
                        let fallen = self.sides[r.0].total_fainted.min(5);
                        let ri = self.ident(r);
                        log_line!(self, "|-activate|{ri}|ability: Supreme Overlord");
                        log_line!(self, "|-start|{ri}|fallen{fallen}|[silent]");
                        self.mon_mut(r).ability_state.fallen = Some(fallen);
                    }
                _ => {}
            },
        }
    }

    /// An ability's `onEnd` when it is replaced (Mega Evolution, Skill Swap,
    /// Transform) or switched out: Flash Fire and Unburden drop their
    /// volatiles, Air Lock and Cloud Nine stop suppressing the weather.
    pub(crate) fn ability_on_end(&mut self, r: MonRef) {
        // singleEvent('End'): Mold Breaker doesn't suppress it.
        match self.raw_ability_kind(r) {
            AbilityKind::Absorb {
                effect: AbsorbEffect::FlashFire,
                ..
            } if self.mon(r).vol.flash_fire => {
                self.mon_mut(r).vol.flash_fire = false;
                let ident = self.ident(r);
                log_line!(self, "|-end|{ident}|ability: Flash Fire|[silent]");
            }
            AbilityKind::Unburden => self.mon_mut(r).vol.unburden = false,
            AbilityKind::Unnerve => self.mon_mut(r).ability_state.unnerved = Some(false),
            AbilityKind::SupremeOverlord => self.supreme_overlord_on_end(r),
            AbilityKind::SuppressWeather => self.air_lock_on_end(r),
            _ => {}
        }
    }

    /// `SwitchOut` (Champions versions): Regenerator, Natural Cure, Zero to Hero.
    pub(crate) fn ability_switch_out(&mut self, r: MonRef) {
        let ident = self.ident(r);
        match self.ability_kind(r) {
            AbilityKind::Regenerator => {
                let m = self.mon(r);
                let amount = m.maxhp as u32 / 3;
                if m.hp > 0 && m.hp < m.maxhp && amount > 0 {
                    let healed = amount.min((m.maxhp - m.hp) as u32);
                    self.mon_mut(r).hp += healed as u16;
                    self.add_health(
                        format_args!("|-heal|{ident}"),
                        r,
                        "|[from] ability: Regenerator|[silent]",
                    );
                }
            }
            AbilityKind::NaturalCure => {
                let st = self.mon(r).status;
                if st != Status::None && st != Status::Fnt {
                    log_line!(
                        self,
                        "|-curestatus|{ident}|{}|[from] ability: Natural Cure|[silent]",
                        st.id()
                    );
                    let m = self.mon_mut(r);
                    m.status = Status::None;
                    m.status_state = Status::None;
                    m.status_time = 0;
                    m.toxic_stage = 0;
                }
            }
            // Zero to Hero turns Palafin into Palafin-Hero for good
            // (`formeChange(..., isPermanent)`, whose `setAbility` restarts
            // the ability's state).
            AbilityKind::ZeroToHero => {
                let sp = self.dex.species(self.mon(r).species);
                if sp.base_species == "Palafin" && sp.name != "Palafin-Hero" {
                    self.forme_change(r, "Palafin-Hero", true, Some("Zero to Hero"));
                    self.reset_ability_state(r);
                    self.mon_mut(r).hero_message_displayed = false;
                }
            }
            _ => {}
        }
    }

    /// Switch-in only, before the ability's start: Air Lock and Cloud Nine
    /// announce themselves (their `onSwitchIn`), which a Skill Swap or a
    /// Mega Evolution doesn't.
    pub(crate) fn air_lock_on_switch_in(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::SuppressWeather) {
            let (ri, name) = (self.ident(r), self.ability_name(r));
            log_line!(self, "|-ability|{ri}|{name}");
        }
    }

    /// An ability's end (`onEnd`): when replaced, switched out, or fainting.
    pub(crate) fn air_lock_on_end(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::SuppressWeather) {
            self.mon_mut(r).ability_state.ending = Some(true);
            self.weather_change_event();
        }
    }

    /// Supreme Overlord's `onEnd`: silent, and not when the Pokémon is being
    /// switched out.
    pub(crate) fn supreme_overlord_on_end(&mut self, r: MonRef) {
        if self.raw_ability_kind(r) != AbilityKind::SupremeOverlord || self.turn_state.called_back {
            return;
        }
        if let Some(f) = self.mon(r).ability_state.fallen.filter(|&f| f > 0) {
            let ri = self.ident(r);
            log_line!(self, "|-end|{ri}|fallen{f}|[silent]");
        }
    }

    /// Anticipation: shudders once if a foe knows a damaging move that is
    /// super effective on it (by the dex's type chart alone, no abilities)
    /// or an OHKO move.
    pub(crate) fn anticipation(&mut self, r: MonRef) {
        let types = self.types_of(r);
        for f in self.foes_of(r) {
            for slot in &self.mon(f).moves {
                let mv = self.dex.mv(slot.id);
                if mv.category == Category::Status {
                    continue;
                }
                let t = mv.type_;
                let immune = types.iter().any(|&d| self.dex.is_immune(t, d));
                let exp: i32 = types
                    .iter()
                    .map(|&d| self.dex.effectiveness_exp(t, d))
                    .sum();
                if (!immune && exp > 0) || mv.ohko.is_some() {
                    let ri = self.ident(r);
                    log_line!(self, "|-ability|{ri}|Anticipation");
                    return;
                }
            }
        }
    }

    /// Forewarn: the foes' moves with the highest power (OHKO 150; Counter,
    /// Metal Burst, Mirror Coat 120; other damaging moves without one 80);
    /// one is drawn and announced.
    pub(crate) fn forewarn(&mut self, r: MonRef) {
        let mut warn: Small<(MoveId, MonRef)> = Small::new();
        let mut warn_bp = 1;
        for f in self.foes_of(r) {
            for slot in &self.mon(f).moves {
                let mv = self.dex.mv(slot.id);
                let mut bp = mv.base_power as u32;
                if mv.ohko.is_some() {
                    bp = 150;
                }
                if matches!(mv.id.as_str(), "counter" | "metalburst" | "mirrorcoat") {
                    bp = 120;
                }
                if bp == 1 || (bp == 0 && mv.category != Category::Status) {
                    bp = 80;
                }
                if bp > warn_bp {
                    warn = smallvec::smallvec![(slot.id, f)];
                    warn_bp = bp;
                } else if bp == warn_bp {
                    warn.push((slot.id, f));
                }
            }
        }
        if warn.is_empty() {
            return;
        }
        let (m, f) = self.rng.sample(&warn);
        let (ri, fi) = (self.ident(r), self.ident(f));
        let name = self.dex.mv(m).name.clone();
        log_line!(self, "|-activate|{ri}|ability: Forewarn|{name}|[of] {fi}");
    }

    /// Plus and Minus: another living active Pokémon on its side has Plus
    /// or Minus (`allies()` excludes the Pokémon itself).
    pub(crate) fn plus_minus_ally(&self, r: MonRef) -> bool {
        (0..self.sides[r.0].n_active)
            .filter_map(|sl| self.active(r.0, sl))
            .filter(|&a| a != r && self.mon(a).hp > 0)
            .any(|a| matches!(self.ability_id(a), "plus" | "minus"))
    }
}
