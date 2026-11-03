//! Applying volatile conditions.

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, ItemKind, MoveKind};
use crate::state::*;

/// The volatiles added through `addVolatile` in this file.
pub(crate) fn is_volatile(id: &str) -> bool {
    matches!(
        id,
        "leechseed" | "aquaring" | "ingrain" | "saltcure" | "magnetrise"
    )
}

/// The crit-ratio volatiles Psych Up copies, in its order.
pub(crate) const CRIT_VOLATILES: [&str; 2] = ["dragoncheer", "focusenergy"];

impl Battle {
    /// Showdown `addVolatile` for the volatiles moves inflict: `Some(true)`
    /// when added, `Some(false)` when the volatile is already there (or the
    /// target has fainted, or its start fails), `None` (null) when an
    /// ability or terrain blocks it. Leech Seed, Aqua Ring, Ingrain, Salt
    /// Cure and Magnet Rise take `add_status_volatile`'s path (the
    /// `Immunity` event before TryAddVolatile).
    pub(crate) fn add_volatile(&mut self, t: MonRef, v: &str, status_move: bool) -> Option<bool> {
        if crate::conditions::volatiles::is_volatile(v) {
            return self.add_status_volatile(t, v, None);
        }
        if self.mon(t).hp == 0 {
            return Some(false);
        }
        if v == "sparklingaria" {
            return self.add_sparkling_aria(t);
        }
        // A volatile already there restarts (or fails) before TryAddVolatile.
        let vol = self.mon(t).vol;
        let present = match v {
            "imprison" => vol.imprison,
            "flinch" => vol.flinch,
            "healblock" => vol.heal_block > 0,
            "confusion" => vol.confusion > 0,
            "focusenergy" => vol.focus_energy,
            "dragoncheer" => vol.dragon_cheer.is_some(),
            "disable" => vol.disable.is_some(),
            "minimize" => vol.minimize,
            "charge" => vol.charge,
            "electrify" => vol.electrify,
            "gastroacid" => vol.gastro_acid,
            "noretreat" => vol.no_retreat,
            "octolock" => vol.octolock.is_some(),
            "powertrick" => vol.power_trick,
            "torment" => vol.torment,
            _ => false,
        };
        if !present && self.run_try_add_volatile(t, v, true) {
            return None;
        }
        let ti = self.ident(t);
        if self.ability_blocks_volatile(t, v) {
            if v == "confusion" && status_move {
                log_line!(self, "|-immune|{ti}|confusion|[from] ability: Own Tempo");
            }
            return None;
        }
        match v {
            "imprison" => {
                if self.mon(t).vol.imprison {
                    return Some(false);
                }
                self.mon_mut(t).vol.imprison = true;
                let ti = self.ident(t);
                log_line!(self, "|-start|{ti}|move: Imprison");
                Some(true)
            }
            "flinch" => {
                let had = self.mon(t).vol.flinch;
                // Focus Punch's volatile refuses it (TryAddVolatile).
                if self.focus_punch_on_try_add_volatile(t) {
                    return None;
                }
                self.mon_mut(t).vol.flinch = true;
                if !had {
                    self.stamp_volatile(t, ResidualVol::Flinch);
                }
                Some(!had)
            }
            // Psychic Noise's secondary, the only source in this format: two
            // turns (durationCallback); its onRestart does nothing.
            "healblock" => {
                if self.mon(t).vol.heal_block > 0 {
                    return None;
                }
                self.mon_mut(t).vol.heal_block = 2;
                self.stamp_volatile(t, ResidualVol::HealBlock);
                let ti = self.ident(t);
                log_line!(self, "|-start|{ti}|move: Heal Block");
                Some(true)
            }
            "confusion" => {
                if self.mon(t).vol.confusion > 0 {
                    return Some(false);
                }
                if self.is_grounded(t)
                    && !self.semi_invulnerable(t)
                    && self.field.terrain == Some(Terrain::Misty)
                {
                    if status_move {
                        let ti = self.ident(t);
                        log_line!(self, "|-activate|{ti}|move: Misty Terrain");
                    }
                    return None;
                }
                if self.safeguard_on_try_add_volatile(t, None, status_move) {
                    return None;
                }
                let turns = self
                    .rng
                    .random_range(if self.active_move.axe_kick { 3 } else { 2 }, 6)
                    as u8;
                self.mon_mut(t).vol.confusion = turns;
                let ti = self.ident(t);
                log_line!(self, "|-start|{ti}|confusion");
                Some(true)
            }
            "focusenergy" => {
                let vol = self.mon(t).vol;
                if vol.focus_energy || vol.dragon_cheer.is_some() {
                    return Some(false);
                }
                self.mon_mut(t).vol.focus_energy = true;
                log_line!(self, "|-start|{ti}|move: Focus Energy");
                Some(true)
            }
            "dragoncheer" => {
                let vol = self.mon(t).vol;
                if vol.dragon_cheer.is_some() || vol.focus_energy {
                    return Some(false);
                }
                let dragon = self
                    .dex
                    .type_id("Dragon")
                    .is_some_and(|d| self.has_type_of(t, d));
                self.mon_mut(t).vol.dragon_cheer = Some(dragon);
                log_line!(self, "|-start|{ti}|move: Dragon Cheer");
                Some(true)
            }
            "disable" => {
                let m = self.mon(t);
                if m.vol.disable.is_some() {
                    return Some(false);
                }
                // onStart: one turn less if the Pokémon still moves this turn.
                let duration = if self.has_queued_move(t) { 4 } else { 5 };
                let Some(lm) = m.last_move else {
                    return Some(false);
                };
                if m.moves.iter().any(|ms| ms.id == lm && ms.pp == 0) {
                    return Some(false);
                }
                self.stamp_volatile(t, ResidualVol::Disable);
                let v = &mut self.mon_mut(t).vol;
                v.disable = Some(lm);
                v.disable_duration = duration;
                let name = self.dex.mv(lm).name.clone();
                log_line!(self, "|-start|{ti}|Disable|{name}");
                Some(true)
            }
            // onRestart returns null.
            "minimize" if self.mon(t).vol.minimize => None,
            "minimize" => {
                self.mon_mut(t).vol.minimize = true;
                Some(true)
            }
            // onStart and onRestart both announce it; a restart succeeds.
            "charge" => {
                if !self.mon(t).vol.charge {
                    self.mon_mut(t).vol.charge = true;
                    self.stamp_volatile(t, ResidualVol::Charge);
                }
                log_line!(self, "|-start|{ti}|Charge");
                Some(true)
            }
            "electrify" => {
                if self.mon(t).vol.electrify {
                    return Some(false);
                }
                self.mon_mut(t).vol.electrify = true;
                self.stamp_volatile(t, ResidualVol::Electrify);
                log_line!(self, "|-singleturn|{ti}|move: Electrify");
                Some(true)
            }
            "noretreat" => {
                if self.mon(t).vol.no_retreat {
                    return Some(false);
                }
                self.mon_mut(t).vol.no_retreat = true;
                log_line!(self, "|-start|{ti}|move: No Retreat");
                Some(true)
            }
            // onStart swaps the stored Attack and Defense; onRestart removes
            // the volatile (`-end`, swapping them back) and the move succeeds.
            "powertrick" => {
                if self.mon(t).vol.power_trick {
                    log_line!(self, "|-end|{ti}|Power Trick");
                    self.swap_atk_def(t);
                    self.mon_mut(t).vol.power_trick = false;
                    return Some(true);
                }
                self.mon_mut(t).vol.power_trick = true;
                log_line!(self, "|-start|{ti}|Power Trick");
                self.swap_atk_def(t);
                Some(true)
            }
            // onStart: the start line has no `move:` prefix.
            "torment" => {
                if self.mon(t).vol.torment {
                    return Some(false);
                }
                self.mon_mut(t).vol.torment = true;
                log_line!(self, "|-start|{ti}|Torment");
                Some(true)
            }
            // onStart: `-endability`, then the ability's End (Illusion,
            // Flash Fire, Cloud Nine...), which runs while it still works.
            "gastroacid" => {
                if self.mon(t).vol.gastro_acid {
                    return Some(false);
                }
                log_line!(self, "|-endability|{ti}");
                if self.raw_ability_kind(t) == AbilityKind::Illusion {
                    self.end_illusion(t);
                }
                self.ability_on_end(t);
                self.mon_mut(t).vol.gastro_acid = true;
                Some(true)
            }
            _ => Some(false),
        }
    }

    /// A move's `volatileStatus` that needs its user: partial trapping and
    /// Leech Seed (`sourceSlot`). `None`: not one of them.
    pub(crate) fn add_move_volatile_from_user(
        &mut self,
        t: MonRef,
        src: MonRef,
        mv: &Move,
        v: &str,
    ) -> Option<Option<bool>> {
        match v {
            "partiallytrapped" => Some(self.add_partial_trap(t, src, mv)),
            "leechseed" => Some(self.add_status_volatile(t, v, Some(src))),
            "stockpile" => Some(self.add_stockpile(t)),
            "smackdown" => Some(self.add_smack_down(t)),
            _ => None,
        }
    }

    /// `addVolatile` for these volatiles: none restarts; the status
    /// immunity check (`Immunity` event), `TryAddVolatile`, then the start
    /// message.
    pub(crate) fn add_status_volatile(
        &mut self,
        t: MonRef,
        v: &str,
        src: Option<MonRef>,
    ) -> Option<bool> {
        let vol = &self.mon(t).vol;
        let present = match v {
            "leechseed" => vol.leech_seed.is_some(),
            "aquaring" => vol.aqua_ring,
            "ingrain" => vol.ingrain,
            "magnetrise" => vol.magnet_rise > 0,
            _ => vol.salt_cure,
        };
        if self.mon(t).hp == 0 || present {
            return Some(false);
        }
        self.event_sort(Event::Immunity, t, None, &[]);
        self.try_add_volatile_sort(t);
        let ti = self.ident(t);
        let (rv, msg) = match v {
            "leechseed" => {
                // The source's slot (`getSlot()`); the target itself by default.
                self.mon_mut(t).vol.leech_seed = Some(src.unwrap_or(t));
                (ResidualVol::LeechSeed, "move: Leech Seed")
            }
            "aquaring" => {
                self.mon_mut(t).vol.aqua_ring = true;
                (ResidualVol::AquaRing, "Aqua Ring")
            }
            "ingrain" => {
                self.mon_mut(t).vol.ingrain = true;
                (ResidualVol::Ingrain, "move: Ingrain")
            }
            "magnetrise" => {
                self.mon_mut(t).vol.magnet_rise = self
                    .dex
                    .cond_num("magnetrise", "duration")
                    .map(|x| x as u8)
                    .unwrap_or(5);
                (ResidualVol::MagnetRise, "Magnet Rise")
            }
            _ => {
                self.mon_mut(t).vol.salt_cure = true;
                (ResidualVol::SaltCure, "Salt Cure")
            }
        };
        self.stamp_volatile(t, rv);
        log_line!(self, "|-start|{ti}|{msg}");
        Some(true)
    }

    /// `addVolatile('smackdown')`: grounds a Flying-type or Levitate holder,
    /// or one that flies, bounces or floats on Magnet Rise. An iron ball,
    /// Ingrain or Gravity already hold it down. Fly and Bounce end, with the
    /// queued second turn.
    pub(crate) fn add_smack_down(&mut self, t: MonRef) -> Option<bool> {
        use crate::kinds::ChargeKind as C;
        let flying = |b: &Battle| matches!(b.charging_kind(t), Some(C::Fly | C::Bounce));
        if self.mon(t).hp == 0 {
            return Some(false);
        }
        if self.mon(t).vol.smack_down {
            // onRestart: only a flier is brought down again.
            if !flying(self) {
                return Some(false);
            }
            self.smack_down_cancel_charge(t);
            let ti = self.ident(t);
            log_line!(self, "|-start|{ti}|Smack Down");
            return Some(true);
        }
        self.event_sort(Event::Immunity, t, None, &[]);
        self.try_add_volatile_sort(t);
        let mut applies = self
            .dex
            .type_id("Flying")
            .is_some_and(|f| self.has_type_of(t, f))
            || (!self.mon(t).revived
                && !self.ignoring_ability(t)
                && !self.transform_ignores_ability(t)
                && matches!(
                    self.raw_ability_kind(t),
                    AbilityKind::Levitate | AbilityKind::Eelevate
                ));
        // `hasAbility` ignores Mold Breaker (only `runEvent` handlers honor
        // it), so a Mold Breaker ally's Smack Down still grounds Levitate.
        if self.has_item(t, "ironball") || self.mon(t).vol.ingrain || self.field.gravity > 0 {
            applies = false;
        }
        if flying(self) {
            applies = true;
            self.smack_down_cancel_charge(t);
        }
        if self.mon(t).vol.magnet_rise > 0 {
            applies = true;
            self.mon_mut(t).vol.magnet_rise = 0;
        }
        if !applies {
            return Some(false);
        }
        self.mon_mut(t).vol.smack_down = true;
        let ti = self.ident(t);
        log_line!(self, "|-start|{ti}|Smack Down");
        Some(true)
    }

    /// `queue.cancelMove` and `removeVolatile('twoturnmove')` of a Pokémon
    /// brought down in mid-air.
    pub(crate) fn smack_down_cancel_charge(&mut self, t: MonRef) {
        if let Some(i) = self.queue.iter().position(|a| {
            matches!(a.kind, crate::state::ActionKind::Move { side, mon, .. } if (side, mon) == t)
        }) {
            self.queue.remove(i);
        }
        self.end_two_turn(t);
    }

    /// `addVolatile('partiallytrapped', source, move)`: no restart; the
    /// status immunity check (`Immunity` event), `TryAddVolatile`, then a
    /// duration of 5 or 6 turns and the start message. Binding Band on the
    /// user makes the residual damage 1/6.
    pub(crate) fn add_partial_trap(&mut self, t: MonRef, src: MonRef, mv: &Move) -> Option<bool> {
        if self.mon(t).hp == 0 || self.mon(t).vol.partial_trap > 0 {
            return Some(false);
        }
        self.event_sort(Event::Immunity, t, None, &[]);
        self.try_add_volatile_sort(t);
        let duration = self.rng.random_range(5, 7) as u8;
        let divisor = if self.item_kind(src) == Some(ItemKind::BindingBand) {
            6
        } else {
            8
        };
        self.stamp_volatile(t, ResidualVol::PartialTrap);
        let v = &mut self.mon_mut(t).vol;
        v.partial_trap = duration;
        v.partial_trap_divisor = divisor;
        v.partial_trap_move = Some(mv.idx);
        v.partial_trap_source = Some(src);
        let (ti, si) = (self.ident(t), self.ident(src));
        log_line!(self, "|-activate|{ti}|move: {}|[of] {si}", mv.name);
        Some(true)
    }

    /// Substitute's start frees its user: the volatile goes silently.
    pub(crate) fn end_partial_trap_silently(&mut self, r: MonRef) {
        if self.mon(r).vol.partial_trap == 0 {
            return;
        }
        let name = self
            .mon(r)
            .vol
            .partial_trap_move
            .map(|id| self.dex.mv(id).name.clone())
            .unwrap_or_default();
        self.clear_partial_trap(r);
        let ti = self.ident(r);
        log_line!(self, "|-end|{ti}|{name}|[partiallytrapped]|[silent]");
    }

    pub(crate) fn clear_partial_trap(&mut self, r: MonRef) {
        let v = &mut self.mon_mut(r).vol;
        v.partial_trap = 0;
        v.partial_trap_divisor = 0;
        v.partial_trap_move = None;
        v.partial_trap_source = None;
    }

    /// `addVolatile('uproar')` from the move's `self` effect: three turns.
    /// Already there, nothing restarts.
    pub(crate) fn add_uproar(&mut self, r: MonRef) {
        if self.mon(r).hp == 0 || self.mon(r).vol.uproar > 0 {
            return;
        }
        self.event_sort(Event::Immunity, r, None, &[]);
        self.try_add_volatile_sort(r);
        self.mon_mut(r).vol.uproar = self
            .dex
            .cond_num("uproar", "duration")
            .map(|x| x as u8)
            .unwrap_or(3);
        self.stamp_volatile(r, ResidualVol::Uproar);
        let ri = self.ident(r);
        log_line!(self, "|-start|{ri}|Uproar");
    }

    /// The move Uproar locks its user into.
    pub(crate) fn uproar_lock(&self, r: MonRef) -> Option<MoveId> {
        if self.mon(r).vol.uproar > 0 {
            self.dex.ids.moves.uproar
        } else {
            None
        }
    }

    /// `DisableMove`: Gravity disables moves with the `gravity` flag.
    pub(crate) fn gravity_on_disable_move(&self, mv: &Move) -> bool {
        self.field.gravity > 0 && mv.has(MoveFlag::Gravity)
    }

    /// The move's `sleepUsable`: it goes on after the sleep check says the
    /// Pokémon can't move.
    pub(crate) fn sleep_usable(&self, id: MoveId) -> bool {
        matches!(
            self.dex.move_kind[id as usize],
            MoveKind::Snore | MoveKind::SleepTalk
        )
    }

    /// Early Bird: sleep counts down twice per turn.
    pub(crate) fn early_bird_sleep_decrement(&self, r: MonRef) -> u8 {
        if self.has_ability(r, AbilityKind::EarlyBird) {
            2
        } else {
            1
        }
    }

    /// Baton Pass leaves `noCopy` volatiles behind: Salt Cure, Stockpile.
    pub(crate) fn remove_no_copy_volatiles(v: &mut crate::state::Volatiles) {
        v.salt_cure = false;
        v.smack_down = false;
        v.stockpile = 0;
        v.stockpile_def = 0;
        v.stockpile_spd = 0;
    }

    /// `addVolatile('stockpile')`: a new one goes through the immunity check
    /// and `TryAddVolatile`; an existing one restarts. Each layer raises
    /// Defense and Special Defense by 1 and remembers which of them moved.
    pub(crate) fn add_stockpile(&mut self, t: MonRef) -> Option<bool> {
        if self.mon(t).hp == 0 {
            return Some(false);
        }
        let layers = self.mon(t).vol.stockpile;
        if layers >= 3 {
            return Some(false);
        }
        if layers == 0 {
            self.event_sort(Event::Immunity, t, None, &[]);
            self.try_add_volatile_sort(t);
            let v = &mut self.mon_mut(t).vol;
            v.stockpile_def = 0;
            v.stockpile_spd = 0;
        }
        self.mon_mut(t).vol.stockpile = layers + 1;
        let ti = self.ident(t);
        log_line!(self, "|-start|{ti}|stockpile{}", layers + 1);
        let before = self.mon(t).boosts;
        let mut b = [0i8; 7];
        b[Boost::Def as usize] = 1;
        b[Boost::Spd as usize] = 1;
        self.boost(t, &b, Some(t), crate::kinds::EffectRef::Other, false, false);
        let after = self.mon(t).boosts;
        let v = &mut self.mon_mut(t).vol;
        if before[Boost::Def as usize] != after[Boost::Def as usize] {
            v.stockpile_def -= 1;
        }
        if before[Boost::Spd as usize] != after[Boost::Spd as usize] {
            v.stockpile_spd -= 1;
        }
        Some(true)
    }

    /// `removeVolatile('stockpile')`: its end takes back what the layers
    /// changed, then announces it.
    pub(crate) fn end_stockpile(&mut self, t: MonRef) {
        let v = self.mon(t).vol;
        if self.mon(t).hp == 0 || v.stockpile == 0 {
            return;
        }
        let m = self.mon_mut(t);
        m.vol.stockpile = 0;
        m.vol.stockpile_def = 0;
        m.vol.stockpile_spd = 0;
        if v.stockpile_def != 0 || v.stockpile_spd != 0 {
            // Only the stats that changed, Defense first.
            let mut b = [0i8; 7];
            b[Boost::Def as usize] = v.stockpile_def;
            b[Boost::Spd as usize] = v.stockpile_spd;
            self.boost(t, &b, Some(t), crate::kinds::EffectRef::Other, false, false);
        }
        let ti = self.ident(t);
        log_line!(self, "|-end|{ti}|Stockpile");
    }

    /// `addVolatile('attract', source)`: nothing when already infatuated or
    /// for Oblivious (`Immunity`); then TryAddVolatile, and the start fails
    /// unless one is male and the other female. Cute Charm's start names it.
    pub(crate) fn add_attract(&mut self, t: MonRef, src: MonRef, cute_charm: bool) -> Option<bool> {
        if self.mon(t).hp == 0 || self.mon(t).vol.attract.is_some() {
            return Some(false);
        }
        if self.has_ability(t, AbilityKind::IntimidateImmune) && self.ability_id(t) == "oblivious" {
            return Some(false);
        }
        if self.run_try_add_volatile(t, "attract", !cute_charm) {
            return None;
        }
        if !matches!(
            (self.mon(t).gender.as_str(), self.mon(src).gender.as_str()),
            ("M", "F") | ("F", "M")
        ) {
            return Some(false);
        }
        let v = &mut self.mon_mut(t).vol;
        v.attract = Some(src);
        v.attract_source_gone = false;
        self.stamp_volatile(t, ResidualVol::Attract);
        let ti = self.ident(t);
        if cute_charm {
            let si = self.ident(src);
            log_line!(
                self,
                "|-start|{ti}|Attract|[from] ability: Cute Charm|[of] {si}"
            );
        } else {
            log_line!(self, "|-start|{ti}|Attract");
        }
        Some(true)
    }

    /// Attract's onTryImmunity: the user and the target must be male and
    /// female.
    pub(crate) fn attract_immune(&self, t: MonRef, src: MonRef) -> bool {
        !matches!(
            (self.mon(t).gender.as_str(), self.mon(src).gender.as_str()),
            ("M", "F") | ("F", "M")
        )
    }

    /// Attract's `onEnd` (silent).
    pub(crate) fn end_attract(&mut self, r: MonRef) {
        let v = &mut self.mon_mut(r).vol;
        v.attract = None;
        v.attract_source_gone = false;
        let ri = self.ident(r);
        log_line!(self, "|-end|{ri}|Attract|[silent]");
    }

    /// Attract's onBeforeMove (priority 2): true when the Pokémon is
    /// immobilized by love (half the time).
    pub(crate) fn attract_on_before_move(&mut self, r: MonRef) -> bool {
        let Some(src) = self.mon(r).vol.attract else {
            return false;
        };
        let (ri, si) = (self.ident(r), self.ident(src));
        log_line!(self, "|-activate|{ri}|move: Attract|[of] {si}");
        if self.rng.chance(1, 2) {
            log_line!(self, "|cant|{ri}|Attract");
            return true;
        }
        false
    }

    /// This module's volatile `Update` handlers (before the ability's):
    /// Attract ends once its source has left the field.
    pub(crate) fn volatiles_on_update(&mut self, r: MonRef) {
        let v = self.mon(r).vol;
        let Some(src) = v.attract else { return };
        let s = self.mon(src);
        // removeVolatile does nothing at 0 HP (a recoil KO before its faint).
        if (v.attract_source_gone || !s.is_active || s.fainted) && self.mon(r).hp > 0 {
            self.end_attract(r);
        }
    }

    /// `addVolatile('octolock', source)`: fails for a fainted target or one
    /// held already; TryAddVolatile, then the start names the holder.
    pub(crate) fn add_octolock(&mut self, t: MonRef, src: MonRef) -> Option<bool> {
        if self.mon(t).hp == 0 || self.mon(t).vol.octolock.is_some() {
            return Some(false);
        }
        if self.run_try_add_volatile(t, "octolock", true) {
            return None;
        }
        self.mon_mut(t).vol.octolock = Some(src);
        self.stamp_volatile(t, ResidualVol::Octolock);
        let (ti, si) = (self.ident(t), self.ident(src));
        log_line!(self, "|-start|{ti}|move: Octolock|[of] {si}");
        Some(true)
    }

    /// Lock-On's onHit: the user's `lockon` volatile (TryAddVolatile first),
    /// linked to the target, announced by the move.
    pub(crate) fn lock_on(&mut self, r: MonRef, t: MonRef) {
        if self.mon(r).hp > 0 {
            self.try_add_volatile_sort(r);
            self.stamp_volatile(r, ResidualVol::LockOn);
            let v = &mut self.mon_mut(r).vol;
            v.lock_on_duration = 2;
            v.lock_on = Some(t);
        }
        let (ri, ti) = (self.ident(r), self.ident(t));
        log_line!(self, "|-activate|{ri}|move: Lock-On|[of] {ti}");
    }

    /// Whether `r`'s Lock-On is on `t` (`onSourceInvulnerability`,
    /// `onSourceAccuracy`).
    pub(crate) fn lock_on_source_invulnerability(&self, r: MonRef, t: MonRef) -> bool {
        let v = &self.mon(r).vol;
        v.lock_on_duration > 0 && v.lock_on == Some(t)
    }

    /// The user's `onSourceAccuracy` handler (Lock-On), for the Accuracy
    /// event's sort: it is there whatever the target.
    pub(crate) fn lock_on_source_accuracy_handler(
        &self,
        r: MonRef,
    ) -> Option<crate::abilities::ModHandler> {
        (self.mon(r).vol.lock_on_duration > 0)
            .then(|| crate::abilities::ModHandler::new(0, self.mon(r).speed, 2))
    }

    /// `clearVolatile` (switching out, fainting): linked trapping ends, and
    /// `setSpecies` restores the stored stats Speed Swap and the splits
    /// changed.
    pub(crate) fn clear_volatile_links(&mut self, r: MonRef) {
        self.unlink_traps(r);
        if self.mon(r).vol.stats_changed {
            let st = self.stats_from_set(r);
            let m = self.mon_mut(r);
            m.stats[1..].copy_from_slice(&st[1..]);
            m.vol.stats_changed = false;
        }
    }

    /// Baton Pass (`selfSwitch: 'copyvolatile'`): the stat stages and the
    /// volatiles without `noCopy` the newcomer takes from `old`.
    pub(crate) fn copy_volatile(
        &self,
        old: MonRef,
        from: Option<MoveId>,
    ) -> Option<(Volatiles, [i8; 7])> {
        let from = from?;
        if self.mon(old).hp == 0 {
            return None;
        }
        // Shed Tail (`selfSwitch: 'shedtail'`): only the substitute, no stat stages.
        if self.dex.move_kind[from as usize] == MoveKind::ShedTail {
            let v = Volatiles {
                substitute: self.mon(old).vol.substitute,
                ..Volatiles::default()
            };
            return Some((v, [0; 7]));
        }
        if self.dex.move_kind[from as usize] != MoveKind::BatonPass {
            return None;
        }
        let m = self.mon(old);
        let mut v = m.vol;
        v.choice_lock = None;
        v.encore = None;
        v.encore_duration = 0;
        v.disable = None;
        v.disable_duration = 0;
        v.imprison = false;
        v.yawn = 0;
        v.glaive_rush = false;
        v.minimize = false;
        v.stats_changed = false;
        Self::remove_no_copy_volatiles(&mut v);
        v.destiny_bond = false;
        v.attract = None;
        v.attract_source_gone = false;
        v.trapped = None;
        v.trapper = 0;
        v.lock_on = None;
        v.lock_on_duration = 0;
        v.apparent_types = None;
        // `noCopy`: Torment, and Syrup Bomb.
        v.torment = false;
        v.syrup_bomb = None;
        v.syrup_bomb_duration = 0;
        Some((v, m.boosts))
    }

    /// Psych Up and Transform: the critical-hit volatiles are all removed
    /// (silently), then each one `theirs` has is added (`[silent]`).
    pub(crate) fn clear_crit_volatiles(&mut self, src: MonRef, theirs: &Volatiles) {
        {
            let v = &mut self.mon_mut(src).vol;
            v.focus_energy = false;
            v.dragon_cheer = None;
        }
        for id in CRIT_VOLATILES {
            let has = match id {
                "dragoncheer" => theirs.dragon_cheer.is_some(),
                _ => theirs.focus_energy,
            };
            if has {
                self.try_add_volatile_sort(src);
                let si = self.ident(src);
                if id == "dragoncheer" {
                    self.mon_mut(src).vol.dragon_cheer = theirs.dragon_cheer;
                    log_line!(self, "|-start|{si}|move: Dragon Cheer|[silent]");
                } else {
                    self.mon_mut(src).vol.focus_energy = true;
                    log_line!(self, "|-start|{si}|move: Focus Energy|[silent]");
                }
            }
        }
    }

    /// A move's `volatileStatus` this module adds with its source
    /// (`attract`); `None` for the others.
    pub(crate) fn add_move_volatile_with_source(
        &mut self,
        t: MonRef,
        src: MonRef,
        v: &str,
    ) -> Option<Option<bool>> {
        match v {
            "attract" => Some(self.add_attract(t, src, false)),
            "octolock" => Some(self.add_octolock(t, src)),
            _ => None,
        }
    }

    /// No Retreat's onTry deletes the move's `volatileStatus` for a
    /// Pokémon that is already `trapped`.
    pub(crate) fn skip_volatile_status(&self, src: MonRef, mv: &Move) -> bool {
        self.dex.move_kind[mv.idx as usize] == MoveKind::NoRetreat
            && self.mon(src).vol.trapped.is_some()
    }

    /// A move's `self.volatileStatus` from this module: `lockedmove`
    /// (restarted, or started for two or three turns).
    pub(crate) fn add_locked_move_volatile(&mut self, r: MonRef, v: &str) {
        if v != "lockedmove" {
            return;
        }
        let vol = &mut self.mon_mut(r).vol;
        if vol.locked_move.is_some() {
            // onRestart.
            if vol.locked_true_duration >= 2 {
                vol.locked_duration = 2;
            }
            return;
        }
        self.try_add_volatile_sort(r);
        self.stamp_volatile(r, ResidualVol::LockedMove);
        // onStart: effectState.move is the move that added it.
        let true_duration = self.rng.random_range(2, 4) as u8;
        // (`effect` is the active move: the called move for Copycat etc.)
        let mv = self
            .active_move
            .used_type
            .map(|(m, _)| m)
            .or(self.mon(r).last_move);
        let vol = &mut self.mon_mut(r).vol;
        vol.locked_move = mv;
        vol.locked_duration = 2;
        vol.locked_true_duration = true_duration;
    }

    /// Mental Herb's turn at Torment (between Encore and Disable): it ends.
    pub(crate) fn mental_herb_cures_torment(&mut self, r: MonRef) {
        if self.mon(r).vol.torment {
            self.mon_mut(r).vol.torment = false;
            let ri = self.ident(r);
            log_line!(self, "|-end|{ri}|Torment");
        }
    }

    /// Torment's `onDisableMove`: the last move used (not Struggle) can't be
    /// chosen again.
    pub(crate) fn torment_on_disable_move(&self, r: MonRef, id: MoveId) -> bool {
        let m = self.mon(r);
        m.vol.torment && m.last_move == Some(id) && self.dex.ids.moves.struggle != Some(id)
    }

    /// A secondary's own onHit from this module: Spirit Shackle traps the
    /// target while the user is on the field. True when it was handled.
    pub(crate) fn volatile_secondary_on_hit(&mut self, src: MonRef, t: MonRef, m: MoveId) -> bool {
        if self.dex.move_kind[m as usize] != MoveKind::SpiritShackle {
            return false;
        }
        if self.mon(src).is_active {
            self.add_trapped(t, src);
        }
        true
    }

    /// Gastro Acid's onCopy: Baton Pass hands it on unless the newcomer's
    /// ability can't be suppressed (removed silently).
    pub(crate) fn gastro_acid_on_copy(&mut self, r: MonRef) {
        // Power Trick's onCopy swaps the newcomer's stored stats.
        if self.mon(r).vol.power_trick {
            self.swap_atk_def(r);
        }
        if self.mon(r).vol.gastro_acid && self.cant_suppress(r) {
            self.mon_mut(r).vol.gastro_acid = false;
        }
    }

    /// Showdown `baseStoredStats`: the species' stats from the set.
    pub(crate) fn stats_from_set(&self, r: MonRef) -> [u16; 6] {
        let m = self.mon(r);
        if !m.vol.stats_changed {
            return m.stats;
        }
        let sp = self.dex.species(m.species);
        let nature = crate::dex::Nature {
            plus: m.nature_plus,
            minus: m.nature_minus,
        };
        let mut st = crate::dex::team::champions_stats(&sp.base_stats, &m.points, nature);
        st[0] = m.stats[0];
        st
    }
}
