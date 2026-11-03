//! Weather: setting, clearing, the effective weather and the `Weather` event.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::AbilityKind;
use crate::state::*;

impl Battle {
    /// Showdown `field.effectiveWeather()` (no weather-suppressing abilities yet).
    pub fn effective_weather(&self) -> Option<Weather> {
        if self.suppressing_weather() {
            return None;
        }
        self.field.weather
    }

    /// Showdown `field.setWeather`. `ability` names the ability that set it.
    pub(crate) fn set_weather(
        &mut self,
        w: Weather,
        source: MonRef,
        ability: Option<&str>,
    ) -> bool {
        if self.field.weather == Some(w) {
            return false;
        }
        let turns = if self.has_item(source, w.rock()) {
            8
        } else {
            self.cond_u8(w.id(), "duration", 5)
        };
        self.field.weather = Some(w);
        self.field.weather_turns = turns;
        match ability {
            Some(name) => {
                let who = self.ident(source);
                log_line!(
                    self,
                    "|-weather|{}|[from] ability: {name}|[of] {who}",
                    w.name()
                );
            }
            None => log_line!(self, "|-weather|{}", w.name()),
        }
        // eachEvent('WeatherChange'): Forecast reacts, and the sort draws on ties.
        self.weather_change_event();
        true
    }

    pub(crate) fn clear_weather(&mut self) {
        if self.field.weather.take().is_some() {
            self.field.weather_turns = 0;
            self.add("|-weather|none");
            self.weather_change_event();
        }
    }

    /// `eachEvent('Weather')`: actives in speed order. For each, the
    /// weather's own `onWeather` (sandstorm: 1/16 damage unless immune by
    /// type or ability), then the Pokémon's ability (Rain Dish, Solar Power, Ice Body, Dry
    /// Skin).
    pub(crate) fn run_weather(&mut self, w: Weather) {
        let mut actives: Small<(MonRef, i32)> = self
            .all_active()
            .into_iter()
            .map(|r| (r, self.mon(r).speed))
            .collect();
        self.speed_sort_by(&mut actives, |a, b| b.1 as i64 - a.1 as i64);
        // Air Lock and Cloud Nine: every `Weather` handler is skipped.
        if self.suppressing_weather() {
            return;
        }
        for (r, _) in actives {
            if self.mon(r).hp == 0 {
                continue;
            }
            let maxhp = self.mon(r).maxhp as u32;
            if w == Weather::Sand {
                let types = self.types_of(r);
                let immune = types
                    .iter()
                    .any(|&t| self.dex.type_immune_to_effect(t, "sandstorm"))
                    || self.ability_weather_immune(r, Weather::Sand)
                    || self.immune_to_sandstorm_while_digging(r);
                if !immune {
                    self.apply_damage(r, (maxhp / 16).max(1), "|[from] Sandstorm");
                }
            }
            if self.mon(r).hp == 0 {
                continue;
            }
            match self.ability_kind(r) {
                AbilityKind::RainDish if w == Weather::Rain => {
                    self.heal(r, (maxhp / 16).max(1), "|[from] ability: Rain Dish");
                }
                AbilityKind::SolarPower if w == Weather::Sun => {
                    let ri = self.ident(r);
                    self.apply_damage(
                        r,
                        (maxhp / 8).max(1),
                        &format!("|[from] ability: Solar Power|[of] {ri}"),
                    );
                }
                AbilityKind::IceBody if w == Weather::Snow => {
                    self.heal(r, (maxhp / 16).max(1), "|[from] ability: Ice Body");
                }
                AbilityKind::DrySkin if self.effective_weather() == Some(w) => match w {
                    Weather::Rain => {
                        self.heal(r, (maxhp / 8).max(1), "|[from] ability: Dry Skin");
                    }
                    Weather::Sun => {
                        let ri = self.ident(r);
                        self.apply_damage(
                            r,
                            (maxhp / 8).max(1),
                            &format!("|[from] ability: Dry Skin|[of] {ri}"),
                        );
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// Showdown `field.suppressingWeather()`: an active, unfainted Air Lock or
    /// Cloud Nine that hasn't ended.
    pub(crate) fn suppressing_weather(&self) -> bool {
        (0..2).any(|s| {
            (0..self.sides[s].n_active).any(|slot| {
                self.active(s, slot).is_some_and(|r| {
                    let m = self.mon(r);
                    !m.fainted
                        && self.has_ability(r, AbilityKind::SuppressWeather)
                        && m.ability_state.ending != Some(true)
                })
            })
        })
    }

    /// `eachEvent('WeatherChange')`: every active Pokémon, fastest first, ties
    /// drawn; Forecast reacts.
    pub(crate) fn weather_change_event(&mut self) {
        for r in self.each_event_order() {
            self.forecast(r);
        }
    }

    /// Forecast's `onWeatherChange`: Castform takes the form of the weather.
    pub(crate) fn forecast(&mut self, r: MonRef) {
        if !self.has_ability(r, AbilityKind::Forecast) {
            return;
        }
        let m = self.mon(r);
        if self.dex.species(m.base_species).base_species != "Castform"
            || m.transformed.is_some()
            || !m.is_active
        {
            return;
        }
        let name = match self.effective_weather() {
            Some(Weather::Sun) => "Castform-Sunny",
            Some(Weather::Rain) => "Castform-Rainy",
            Some(Weather::Snow) => "Castform-Snowy",
            _ => "Castform",
        };
        if self.dex.species(m.species).name == name {
            return;
        }
        let Some(sp) = self.dex.species_id(name) else {
            return;
        };
        self.set_species(r, sp);
        let ident = self.ident(r);
        log_line!(
            self,
            "|-formechange|{ident}|{name}|[msg]|[from] ability: Forecast"
        );
    }

    /// Whether the active Pokémon (Showdown `activePokemon`, the move's
    /// user) has Mega Sol: `Pokemon.effectiveWeather()` from a move's or a
    /// weather's handler is then sun, for any Pokémon.
    pub(crate) fn mega_sol_active(&self) -> bool {
        self.active_move.cur_user.is_some_and(|u| {
            self.dex.ability(self.mon(u).ability).id == "megasol"
                && !self.ignoring_ability(u)
                && !self.mon(u).revived
        })
    }

    /// `Pokemon.effectiveWeather()` as a move's or a weather's handler sees it.
    pub(crate) fn move_weather(&self) -> Option<Weather> {
        if self.mega_sol_active() {
            Some(Weather::Sun)
        } else {
            self.effective_weather()
        }
    }

    /// `effectiveWeather(undefined, true)`: Mega Sol announces the sun it
    /// makes up (Solar Beam, Solar Blade, Synthesis, Morning Sun, Moonlight).
    pub(crate) fn mega_sol_message(&mut self, r: MonRef) {
        if self.mega_sol_active() && self.effective_weather() != Some(Weather::Sun) {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|ability: Mega Sol");
        }
    }

    /// A charge move's weather check in its onTryMove: Solar Beam and Solar
    /// Blade's announces Mega Sol, Electro Shot's doesn't.
    pub(crate) fn charge_move_weather(
        &mut self,
        r: MonRef,
        ck: crate::kinds::ChargeKind,
    ) -> Option<Weather> {
        if ck.instant_in() == Some(Weather::Sun) {
            self.mega_sol_message(r);
        }
        self.move_weather()
    }

    /// WeatherModifyDamage with a Mega Sol attacker: `priorityEvent` exits
    /// after the first handler that returns a value. Mega Sol (priority 1)
    /// runs sun's handler itself (x1.5 Fire, x0.5 Water) and returns the
    /// damage, so the real weather's handler never runs: real sun doesn't
    /// stack, and rain does nothing.
    pub(crate) fn mega_sol_weather_modify_damage(&self, move_type: TypeId) -> Option<(u32, u32)> {
        if !self.mega_sol_active()
            || self.ability_kind(self.active_move.cur_user?) != AbilityKind::MegaSol
        {
            return None;
        }
        Some(match self.dex.type_names[move_type as usize].as_str() {
            "Fire" => (3, 2),
            "Water" => (1, 2),
            _ => (1, 1),
        })
    }
}
