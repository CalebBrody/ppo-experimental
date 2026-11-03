//! Stat and damage modifiers: `ModifySpe`, `ModifyDef`, `BasePower` and
//! `WeatherModifyDamage`.

use crate::abilities::{compare_mod, ModHandler, SUB_ORDER_ABILITY, SUB_ORDER_ITEM};
use crate::battle::{Battle, MonRef};
use crate::dex::events::{Event, Role};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::MoveKind;
use crate::kinds::{AbilityKind, AbsorbEffect, ItemKind};
use crate::moves::damage::{self, Chain};
use crate::state::*;

impl Battle {
    /// Speed after `ModifySpe` handlers: Tailwind (chained), then paralysis
    /// (priority -101: finalize the chain, then halve).
    pub(crate) fn modify_speed(&self, r: MonRef, spe: u32) -> u32 {
        let m = self.mon(r);
        // A fainted Pokémon is no longer active, and runEvent finds no
        // handlers for it: no item, ability, Tailwind or paralysis.
        if m.fainted || !m.is_active {
            return spe;
        }
        let mut chain = Chain::ONE;
        if let Some((n, d)) = self.item_speed_mod(r) {
            chain = chain.then(n, d);
        }
        if let Some((n, d)) = self.ability_speed_mod(r) {
            chain = chain.then(n, d);
        }
        if self.sides[r.0].conds.tailwind > 0 {
            chain = chain.then(2, 1);
        }
        let mut v = chain.apply(spe);
        if m.status == Status::Par && !self.has_ability(r, AbilityKind::QuickFeet) {
            v = v * 50 / 100;
        }
        v
    }

    /// Showdown's `runEvent('BasePower', source, target, move)`, with the
    /// move's own handler first. Every handler present joins the sort,
    /// whether or not it changes this move, since ties draw: the move; the
    /// user's volatiles (Helping Hand), ability and item; Ally and Any
    /// handlers on the user's side, then on its foes (Fairy Aura); the
    /// terrain. Each then applies in sorted order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_base_power(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        kind: crate::kinds::MoveKind,
        move_type: TypeId,
        type_changed: bool,
        base_power: u32,
    ) -> Chain {
        use crate::dex::Handlers;
        #[derive(Clone, Copy)]
        enum H {
            Move,
            HelpingHand,
            /// Charge (a volatile, priority 9).
            Charge,
            /// A gem's volatile (priority 14).
            Gem,
            Ability,
            Item,
            /// `onAllyBasePower` / `onFoeBasePower` (no supported ability uses one).
            Other(MonRef),
            /// `onAnyBasePower` on this Pokémon: Fairy Aura, Dark Aura.
            Any(MonRef),
            /// The target's `onSourceBasePower` (Dry Skin).
            Source,
            /// The target's Bounce volatile (`onSourceBasePower`).
            SourceVolatile,
            Terrain,
        }
        let key = |priority: Option<f64>, speed: i32, sub_order: u32| {
            ModHandler::new(priority.unwrap_or(0.0) as i32, speed, sub_order)
        };
        let dex = self.dex.clone();
        let mut hs: arrayvec::ArrayVec<(H, ModHandler), 24> = arrayvec::ArrayVec::new();
        let speed = self.mon(src).speed;
        if mv.has_callback("onBasePower") {
            hs.push((H::Move, key(mv.num("onBasePowerPriority"), speed, 0)));
        }
        if self.mon(src).vol.helping_hand > 0 {
            hs.push((
                H::HelpingHand,
                key(dex.cond_num("helpinghand", "onBasePowerPriority"), speed, 2),
            ));
        }
        if self.mon(src).vol.charge {
            hs.push((
                H::Charge,
                key(dex.cond_num("charge", "onBasePowerPriority"), speed, 2),
            ));
        }
        if self.mon(src).vol.gem {
            hs.push((
                H::Gem,
                key(dex.cond_num("gem", "onBasePowerPriority"), speed, 2),
            ));
        }
        let event = Event::BasePower;
        let ability_handlers = |r: MonRef| &dex.ability_handlers[self.mon(r).ability as usize];
        if let Some(p) = ability_handlers(src).priority(event, Role::Own) {
            hs.push((H::Ability, ModHandler::new(p, speed, SUB_ORDER_ABILITY)));
        }
        if let Some(item) = self.mon(src).item {
            if let Some(p) = dex.item_handlers[item as usize].priority(event, Role::Own) {
                hs.push((H::Item, ModHandler::new(p, speed, SUB_ORDER_ITEM)));
            }
        }
        for s in [src.0, 1 - src.0] {
            let side_role = if s == src.0 { Role::Ally } else { Role::Foe };
            for slot in 0..self.sides[s].n_active {
                let Some(r) = self.active(s, slot).filter(|&r| self.mon(r).hp > 0) else {
                    continue;
                };
                let sp = self.mon(r).speed;
                if let Some(p) = ability_handlers(r).priority(event, side_role) {
                    hs.push((H::Other(r), ModHandler::new(p, sp, SUB_ORDER_ABILITY)));
                }
                if let Some(p) = ability_handlers(r).priority(event, Role::Any) {
                    hs.push((H::Any(r), ModHandler::new(p, sp, SUB_ORDER_ABILITY)));
                }
            }
        }
        if let Some(p) = ability_handlers(tgt).priority(event, Role::Source) {
            hs.push((
                H::Source,
                ModHandler::new(p, self.mon(tgt).speed, SUB_ORDER_ABILITY),
            ));
        }
        if self
            .mon(tgt)
            .vol
            .charging
            .is_some_and(|id| dex.mv(id).id == "bounce")
        {
            hs.push((H::SourceVolatile, key(None, self.mon(tgt).speed, 2)));
        }
        if let Some(t) = self.field.terrain {
            // Terrains are their own effect type, which has no default subOrder.
            hs.push((
                H::Terrain,
                key(dex.cond_num(t.id(), "onBasePowerPriority"), 0, 0),
            ));
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        }
        let type_name = dex.type_names[move_type as usize].as_str();
        let mut chain = Chain::ONE;
        let mut aura_booster = None;
        for &(h, _) in hs.iter() {
            let m = match h {
                H::Move => self.move_base_power_mod(src, tgt, kind),
                H::HelpingHand => {
                    let hh = self.mon(src).vol.helping_hand as u32;
                    Some((3u32.pow(hh), 2u32.pow(hh)))
                }
                // Charge: Electric moves deal double.
                H::Charge => (type_name == "Electric").then_some((2, 1)),
                H::Gem => Some((5325, 4096)),
                H::Ability => {
                    self.ability_base_power_mod(src, tgt, mv, move_type, type_changed, base_power)
                }
                H::Item => self.item_base_power_mod(src, mv, move_type),
                // Steely Spirit boosts its side's Steel moves, its own included.
                H::Other(r) => match self.ability_kind(r) {
                    AbilityKind::SteelySpirit if r.0 == src.0 && type_name == "Steel" => {
                        Some((3, 2))
                    }
                    _ => None,
                },
                // The first Aura holder in sorted order boosts the move.
                H::Any(r) => {
                    let boosts = tgt != src
                        && mv.category != Category::Status
                        && self.has_ability(r, AbilityKind::Aura(move_type))
                        && aura_booster.is_none();
                    if boosts {
                        aura_booster = Some(r);
                    }
                    boosts.then_some((5448, 4096))
                }
                // Dry Skin takes 1.25x from Fire.
                H::Source => match self.ability_kind(tgt) {
                    AbilityKind::DrySkin if type_name == "Fire" => Some((5120, 4096)),
                    _ => None,
                },
                H::SourceVolatile => self
                    .semi_invulnerable_doubles(tgt, &mv.id, true)
                    .then_some((2, 1)),
                H::Terrain => match self.field.terrain {
                    Some(Terrain::Grassy) => {
                        if matches!(mv.id.as_str(), "earthquake" | "bulldoze" | "magnitude")
                            && self.is_grounded(tgt)
                            && !self.semi_invulnerable(tgt)
                        {
                            Some((1, 2))
                        } else {
                            (type_name == "Grass" && self.is_grounded(src)).then_some((5325, 4096))
                        }
                    }
                    Some(t @ (Terrain::Psychic | Terrain::Electric)) => {
                        // Not an attacker in the semi-invulnerable turn of Bounce (a
                        // Future Sight that lands meanwhile).
                        (type_name == t.boosted_type()
                            && self.is_grounded(src)
                            && !self.semi_invulnerable(src))
                        .then_some((5325, 4096))
                    }
                    Some(Terrain::Misty) => (type_name == "Dragon"
                        && self.is_grounded(tgt)
                        && !self.semi_invulnerable(tgt))
                    .then_some((1, 2)),
                    None => None,
                },
            };
            if let Some((n, d)) = m {
                chain = chain.then(n, d);
            }
        }
        chain
    }

    /// The user's ability's `onBasePower` for this move, when it changes it:
    /// Technician, -ate abilities, Tough Claws and the other flag boosts,
    /// Punk Rock, Reckless, Supreme Overlord, Sand Force, Analytic (priority
    /// 21) when no other active Pokémon still has a move queued, Rivalry.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ability_base_power_mod(
        &self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        move_type: TypeId,
        type_changed: bool,
        base_power: u32,
    ) -> Option<(u32, u32)> {
        match self.ability_kind(src) {
            AbilityKind::Technician if base_power <= 60 => Some((3, 2)),
            AbilityKind::SheerForce if mv.sheer_force || mv.has_sheer_force_boost => {
                Some((5325, 4096))
            }
            AbilityKind::TypeChange(_) if type_changed => Some((4915, 4096)),
            AbilityKind::FlagBoost => {
                let (flag, num, den) = match self.ability_id(src) {
                    "toughclaws" => ("contact", 5325, 4096),
                    "ironfist" => ("punch", 4915, 4096),
                    "strongjaw" => ("bite", 3, 2),
                    "megalauncher" => ("pulse", 3, 2),
                    _ => ("slicing", 3, 2),
                };
                mv.has_flag(flag).then_some((num, den))
            }
            AbilityKind::PunkRock if mv.has(MoveFlag::Sound) => Some((5325, 4096)),
            // recoil or hasCrashDamage
            AbilityKind::Reckless
                if mv.recoil.is_some()
                    || self.dex.move_kind[mv.idx as usize] == MoveKind::Crash =>
            {
                Some((4915, 4096))
            }
            AbilityKind::SupremeOverlord => {
                let fallen = self.mon(src).ability_state.fallen? as usize;
                Some(([4096, 4506, 4915, 5325, 5734, 6144][fallen.min(5)], 4096))
            }
            AbilityKind::SandForce if self.effective_weather() == Some(Weather::Sand) => matches!(
                self.dex.type_names[move_type as usize].as_str(),
                "Rock" | "Ground" | "Steel"
            )
            .then_some((5325, 4096)),
            AbilityKind::Analytic => {
                let waiting = self.all_active().into_iter().any(|t| {
                    t != src && !self.mon(t).fainted && self.queued_move_slot(t).is_some()
                });
                (!waiting).then_some((5325, 4096))
            }
            // Rivalry (priority 24): both genders known, x1.25 if alike, else x0.75.
            AbilityKind::Rivalry => {
                let (a, d) = (&self.mon(src).gender, &self.mon(tgt).gender);
                if a.is_empty() || d.is_empty() {
                    None
                } else if a == d {
                    Some((5, 4))
                } else {
                    Some((3, 4))
                }
            }
            _ => None,
        }
    }

    /// `ModifyCritRatio`: the item (Scope Lens, Leek), the ability (Super
    /// Luck), the volatiles (Focus Energy, Dragon Cheer), then Merciless (5
    /// against a poisoned target). Showdown sorts the volatiles first and the
    /// item last; the stage is clamped to 4 after the event, so applying
    /// Merciless last gives the same result.
    pub(crate) fn run_modify_crit_ratio(&self, src: MonRef, tgt: MonRef, ratio: u8) -> u8 {
        let mut ratio = ratio;
        match self.item_kind(src) {
            Some(ItemKind::ScopeLens) => ratio += 1,
            Some(ItemKind::Leek)
                if matches!(
                    to_id(&self.dex.species(self.mon(src).base_species).base_species).as_str(),
                    "farfetchd" | "sirfetchd"
                ) =>
            {
                ratio += 2
            }
            _ => {}
        }
        if self.has_ability(src, AbilityKind::SuperLuck) {
            ratio += 1;
        }
        let v = &self.mon(src).vol;
        if v.focus_energy {
            ratio += 2;
        }
        if let Some(dragon) = v.dragon_cheer {
            ratio += if dragon { 2 } else { 1 };
        }
        if self.has_ability(src, AbilityKind::Merciless)
            && matches!(self.mon(tgt).status, Status::Psn | Status::Tox)
        {
            ratio = 5;
        }
        ratio
    }

    /// `ModifyAtk` / `ModifySpA` on the attacker and `SourceModifyAtk` /
    /// `SourceModifySpA` on the defender. Returns the attack stat.
    pub(crate) fn modify_attack(
        &self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        move_type: TypeId,
        attack: u32,
    ) -> u32 {
        let physical = mv.category == Category::Physical;
        let m = self.mon(src);
        let type_name = self.dex.type_names[move_type as usize].as_str();
        let mut value = attack;
        let mut chain = Chain::ONE;
        match self.ability_kind(src) {
            AbilityKind::HugePower if physical => chain = chain.then(2, 1),
            // Hustle modifies the stat directly rather than chaining.
            AbilityKind::Hustle if physical => value = crate::moves::damage::modify(value, 3, 2),
            AbilityKind::Guts if physical && m.status != Status::None => chain = chain.then(3, 2),
            AbilityKind::Pinch(t) if t == move_type && m.hp as u32 * 3 <= m.maxhp as u32 => {
                chain = chain.then(3, 2)
            }
            AbilityKind::FireMane if type_name == "Fire" => chain = chain.then(3, 2),
            AbilityKind::WaterBubble if type_name == "Water" => chain = chain.then(2, 1),
            AbilityKind::SolarPower
                if !physical && self.effective_weather() == Some(Weather::Sun) =>
            {
                chain = chain.then(3, 2)
            }
            AbilityKind::PlusMinus if !physical && self.plus_minus_ally(src) => {
                chain = chain.then(3, 2)
            }
            _ => {}
        }
        if m.vol.flash_fire
            && type_name == "Fire"
            && matches!(
                self.ability_kind(src),
                AbilityKind::Absorb {
                    effect: AbsorbEffect::FlashFire,
                    ..
                }
            )
        {
            chain = chain.then(3, 2);
        }
        if self.has_ability(tgt, AbilityKind::ThickFat) && matches!(type_name, "Fire" | "Ice") {
            chain = chain.then(1, 2);
        }
        if self.has_ability(tgt, AbilityKind::WaterBubble) && type_name == "Fire" {
            chain = chain.then(1, 2);
        }
        // The user's Stakeout; the target's Heatproof and Purifying Salt
        // (`onSourceModifyAtk`).
        if self.has_ability(src, AbilityKind::Stakeout) && self.mon(tgt).active_turns == 0 {
            chain = chain.then(2, 1);
        }
        if self.has_ability(tgt, AbilityKind::Heatproof) && type_name == "Fire" {
            chain = chain.then(1, 2);
        }
        if self.has_ability(tgt, AbilityKind::PurifyingSalt) && type_name == "Ghost" {
            chain = chain.then(1, 2);
        }
        if let Some((n, d)) = self.item_attack_mod(src) {
            chain = chain.then(n, d);
        }
        chain.apply(value)
    }

    /// `ModifySpe` from abilities and the Unburden volatile: Swift Swim and
    /// the other weather abilities, Unburden, Quick Feet, Surge Surfer.
    #[inline(always)]
    pub(crate) fn ability_speed_mod(&self, r: MonRef) -> Option<(u32, u32)> {
        let m = self.mon(r);
        // The raw ability first: most have no Speed handler, and then
        // whether it is suppressed does not matter.
        let modifier = match self.raw_ability_kind(r) {
            AbilityKind::WeatherSpeed(w) if self.effective_weather() == Some(w) => (2, 1),
            AbilityKind::Unburden if m.vol.unburden && m.item.is_none() => (2, 1),
            AbilityKind::QuickFeet if m.status != Status::None => (3, 2),
            AbilityKind::SurgeSurfer if self.field.terrain == Some(Terrain::Electric) => (2, 1),
            _ => return None,
        };
        (!self.ability_ignored(r)).then_some(modifier)
    }

    /// `ModifyDef` / `ModifySpD` on the defender: Snowscape and Sandstorm
    /// (priority 10, applied directly before any chained modifier), then Fur
    /// Coat, Grass Pelt and Marvel Scale.
    pub(crate) fn modify_defense(&self, tgt: MonRef, stat: Stat, defense: u32) -> u32 {
        let defense = match (self.move_weather(), stat) {
            (Some(Weather::Sand), Stat::Spd)
                if self
                    .dex
                    .type_id("Rock")
                    .map(|t| self.has_type_of(tgt, t))
                    .unwrap_or(false) =>
            {
                damage::modify(defense, 3, 2)
            }
            (Some(Weather::Snow), Stat::Def)
                if self
                    .dex
                    .type_id("Ice")
                    .map(|t| self.has_type_of(tgt, t))
                    .unwrap_or(false) =>
            {
                damage::modify(defense, 3, 2)
            }
            _ => defense,
        };
        // Then the chained modifiers: Fur Coat, Grass Pelt, Marvel Scale.
        if stat != Stat::Def {
            return defense;
        }
        let m = match self.ability_kind(tgt) {
            AbilityKind::FurCoat => Some((2, 1)),
            AbilityKind::GrassPelt if self.field.terrain == Some(Terrain::Grassy) => Some((3, 2)),
            AbilityKind::MarvelScale if self.mon(tgt).status != Status::None => Some((3, 2)),
            _ => None,
        };
        m.map_or(defense, |(n, d)| Chain::ONE.then(n, d).apply(defense))
    }

    /// `WeatherModifyDamage`: rain and sun boost or weaken Water and Fire moves.
    pub(crate) fn weather_damage_mod(&self, move_type: TypeId) -> (u32, u32) {
        if let Some(m) = self.mega_sol_weather_modify_damage(move_type) {
            return m;
        }
        let type_name = self.dex.type_names[move_type as usize].as_str();
        match (self.effective_weather(), type_name) {
            (Some(Weather::Rain), "Water") | (Some(Weather::Sun), "Fire") => (3, 2),
            (Some(Weather::Rain), "Fire") | (Some(Weather::Sun), "Water") => (1, 2),
            _ => (1, 1),
        }
    }

    /// Gravity's `ModifyAccuracy` (priority 0, so first): x6840/4096.
    pub(crate) fn gravity_on_modify_accuracy(&self) -> Option<(u32, u32)> {
        (self.field.gravity > 0).then_some((6840, 4096))
    }

    /// Spit Up's `basePowerCallback`: 100 per layer.
    pub(crate) fn spit_up_base_power_callback(&self, r: MonRef) -> u32 {
        100 * self.mon(r).vol.stockpile as u32
    }

    /// The events a `basePowerCallback` runs, before the critical-hit roll:
    /// `getStat('spe')` runs ModifyBoost then ModifySpe, on the user, then
    /// on the target.
    pub(crate) fn base_power_callback_events(&mut self, src: MonRef, tgt: MonRef, kind: MoveKind) {
        if matches!(kind, MoveKind::ElectroBall | MoveKind::GyroBall) {
            for r in [src, tgt] {
                self.event_sort(Event::ModifyBoost, r, None, &[]);
                self.event_sort(Event::ModifySpe, r, None, &[]);
            }
        }
    }

    /// `getWeight` in hectograms: ModifyWeight (Heavy Metal doubles, Light
    /// Metal halves, truncated), at least 1.
    pub(crate) fn get_weight(&self, r: MonRef) -> u32 {
        let w = (self.dex.species(self.mon(r).species).weight_kg * 10.0).round() as u32;
        let w = match self.ability_id(r) {
            "heavymetal" => w * 2,
            "lightmetal" => w / 2,
            _ => w,
        };
        w.max(1)
    }

    /// The user's accuracy stage against `t`, after Unaware.
    pub(crate) fn accuracy_stage(&self, r: MonRef, t: MonRef) -> i8 {
        if r != t && self.unaware_on_any_modify_boost(t) {
            0
        } else {
            self.mon(r).boosts[Boost::Accuracy as usize]
        }
    }

    /// The target's evasion stage against `r`, after Unaware.
    pub(crate) fn evasion_stage(&self, r: MonRef, t: MonRef) -> i8 {
        if r != t && self.unaware_on_any_modify_boost(r) {
            0
        } else {
            self.mon(t).boosts[Boost::Evasion as usize]
        }
    }

    /// A confusion self-hit's Attack and Defense stages: two `calculateStat`
    /// ModifyBoost sorts. Confusion makes the Pokémon its own `activeTarget`
    /// first, so Unaware never applies.
    pub(crate) fn confusion_stages(&mut self, r: MonRef) -> (i8, i8) {
        self.event_sort(Event::ModifyBoost, r, None, &[]);
        self.event_sort(Event::ModifyBoost, r, None, &[]);
        let b = self.mon(r).boosts;
        (b[Boost::Atk as usize], b[Boost::Def as usize])
    }

    /// Unaware (`onAnyModifyBoost`) on the target zeroes the user's
    /// offensive and accuracy stages; on the user, the target's defensive
    /// and evasion stages. The holder's own stages stay.
    pub(crate) fn unaware_on_any_modify_boost(&self, r: MonRef) -> bool {
        self.has_ability(r, AbilityKind::Unaware)
    }

    /// The attack and defense stages `getDamage` uses, after Unaware.
    pub(crate) fn damage_stages_after_unaware(
        &self,
        src: MonRef,
        tgt: MonRef,
        atk: i8,
        def: i8,
    ) -> (i8, i8) {
        if src == tgt {
            return (atk, def);
        }
        (
            if self.unaware_on_any_modify_boost(tgt) {
                0
            } else {
                atk
            },
            if self.unaware_on_any_modify_boost(src) {
                0
            } else {
                def
            },
        )
    }

    /// Shell Side Arm's `onModifyMove`: Physical with contact when the
    /// physical estimate beats the special one (a tie flips a coin).
    pub(crate) fn shell_side_arm_on_modify_move(
        &mut self,
        r: MonRef,
        target: Option<MonRef>,
        mv: &Move,
    ) -> Option<Move> {
        let t = target?;
        // Four getStat calls, each running ModifyBoost.
        self.event_sort(Event::ModifyBoost, r, None, &[]);
        self.event_sort(Event::ModifyBoost, r, None, &[]);
        self.event_sort(Event::ModifyBoost, t, None, &[]);
        self.event_sort(Event::ModifyBoost, t, None, &[]);
        let stat = |b: &Battle, who: MonRef, st: Stat, aware: bool| {
            let m = b.mon(who);
            // getStat(unmodified) under Wonder Room keeps the stored stat but
            // takes the boosts of the other defense.
            let boost = if aware {
                m.boosts[Boost::of_stat(b.wonder_room_stat(st)) as usize]
            } else {
                0
            };
            crate::moves::damage::apply_boost(m.stats[st as usize] as u32, boost)
        };
        // Unaware on the target ignores the user's stages, and on the user the target's.
        let user_aware = !(t != r && self.unaware_on_any_modify_boost(t));
        let target_aware = !(t != r && self.unaware_on_any_modify_boost(r));
        let (atk, spa) = (
            stat(self, r, Stat::Atk, user_aware),
            stat(self, r, Stat::Spa, user_aware),
        );
        let (def, spd) = (
            stat(self, t, Stat::Def, target_aware),
            stat(self, t, Stat::Spd, target_aware),
        );
        let level = self.mon(r).level as u32;
        let base = (2 * level / 5 + 2) * 90;
        let physical = base * atk / def / 50;
        let special = base * spa / spd / 50;
        if physical > special || (physical == special && self.rng.chance(1, 2)) {
            let mut m = mv.clone();
            m.category = Category::Physical;
            m.flags.insert(MoveFlag::Contact);
            Some(m)
        } else {
            None
        }
    }

    /// Shell Side Arm's `onPrepareHit`: the animation names the category.
    pub(crate) fn shell_side_arm_on_prepare_hit(
        &mut self,
        r: MonRef,
        first: Option<MonRef>,
        mv: &Move,
        line: usize,
    ) {
        if first.is_some_and(|t| t.0 != r.0) {
            let cat = if mv.category == Category::Physical {
                "Physical"
            } else {
                "Special"
            };
            self.attr_move_line(line, &format!("[anim] Shell Side Arm {cat}"));
        }
    }

    /// Shell Side Arm's `onHit` and `onAfterSubDamage`: a hint of the category.
    pub(crate) fn shell_side_arm_on_hit(&mut self, r: MonRef, t: MonRef, mv: &Move) {
        if t.0 != r.0 {
            let cat = if mv.category == Category::Physical {
                "Physical"
            } else {
                "Special"
            };
            self.add(format!("|-hint|{cat} Shell Side Arm"));
        }
    }

    /// That handler's modifier for this hit.
    pub(crate) fn source_modify_damage(
        &self,
        tgt: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) -> Option<(u32, u32)> {
        match self.ability_kind(tgt) {
            AbilityKind::PunkRock if mv.has(MoveFlag::Sound) => Some((1, 2)),
            AbilityKind::Fluffy => {
                let fire = self.dex.type_names[move_type as usize] == "Fire";
                match (fire, mv.has(MoveFlag::Contact)) {
                    (true, false) => Some((2, 1)),
                    (false, true) => Some((1, 2)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether the target's ability has an `onSourceModifyDamage` handler
    /// among the handlers here (it joins the ModifyDamage sort).
    pub(crate) fn has_source_modify_damage(&self, tgt: MonRef) -> bool {
        matches!(
            self.raw_ability_kind(tgt),
            AbilityKind::PunkRock | AbilityKind::Fluffy
        )
    }
}
