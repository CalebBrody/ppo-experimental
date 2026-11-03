//! `DamagingHit` (with the user's `SourceDamagingHit`): the handlers of the
//! targets a move damaged, and the user's Poison Touch.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, EffectRef, ItemKind};
use crate::state::*;

/// One `DamagingHit` handler (sorted by order, then target, left to right).
#[derive(Clone, Copy, Debug)]
pub(crate) enum HitHandler {
    Thaw(MonRef),
    ContactDamage(MonRef),
    ContactStatus(MonRef, Status),
    HitBoost(MonRef),
    SpicySpray(MonRef),
    Rattled(MonRef),
    RockyHelmet(MonRef),
    AirBalloon(MonRef),
    PoisonTouch(MonRef),
    CursedBody(MonRef),
    ToxicDebris(MonRef),
    /// Seed Sower sets Grassy Terrain; Sand Spit sets a sandstorm.
    SeedSower(MonRef),
    SandSpit(MonRef),
    /// Counter's and Mirror Coat's volatile `onDamagingHit`.
    Counter(MonRef),
    CuteCharm(MonRef),
    Mummy(MonRef),
    WanderingSpirit(MonRef),
    Illusion(MonRef),
    /// Electromorphosis, Aftermath and Innards Out (order 1), Effect Spore and Gooey.
    Electromorphosis(MonRef),
    Aftermath(MonRef),
    InnardsOut(MonRef),
    EffectSpore(MonRef),
    Gooey(MonRef),
}

impl Battle {
    /// `DamagingHit` (with the user's `SourceDamagingHit`) for the targets that
    /// took damage, ordered left to right by `onDamagingHitOrder`, then target:
    /// Rough Skin (1), Rocky Helmet (2), then the rest in collection order
    /// (status, ability, item; then the user's Poison Touch).
    pub(crate) fn run_damaging_hit(
        &mut self,
        src: MonRef,
        damaged: &[MonRef],
        mv: &Move,
        move_type: TypeId,
    ) {
        let fire = self.dex.ids.types.fire == Some(move_type);
        let mut handlers: Small<(u64, usize, HitHandler)> = Small::new();
        for (i, &t) in damaged.iter().enumerate() {
            if fire && mv.category != Category::Status && self.mon(t).status == Status::Frz {
                handlers.push((u64::MAX, i, HitHandler::Thaw(t)));
            }
            // A volatile's handler (Counter) comes before the ability's and item's.
            if self.mon(t).vol.counter.is_some() || self.mon(t).vol.mirror_coat.is_some() {
                handlers.push((u64::MAX, i, HitHandler::Counter(t)));
            }
            match self.ability_kind(t) {
                AbilityKind::ContactDamage => handlers.push((1, i, HitHandler::ContactDamage(t))),
                AbilityKind::ContactStatus(st) => {
                    handlers.push((u64::MAX, i, HitHandler::ContactStatus(t, st)))
                }
                AbilityKind::HitBoost | AbilityKind::ThermalExchange => {
                    handlers.push((u64::MAX, i, HitHandler::HitBoost(t)))
                }
                AbilityKind::SpicySpray => handlers.push((u64::MAX, i, HitHandler::SpicySpray(t))),
                AbilityKind::Rattled => handlers.push((u64::MAX, i, HitHandler::Rattled(t))),
                AbilityKind::CursedBody => handlers.push((u64::MAX, i, HitHandler::CursedBody(t))),
                AbilityKind::ToxicDebris => {
                    handlers.push((u64::MAX, i, HitHandler::ToxicDebris(t)))
                }
                AbilityKind::SeedSower => handlers.push((u64::MAX, i, HitHandler::SeedSower(t))),
                AbilityKind::SandSpit => handlers.push((u64::MAX, i, HitHandler::SandSpit(t))),
                AbilityKind::Illusion => handlers.push((u64::MAX, i, HitHandler::Illusion(t))),
                AbilityKind::CuteCharm => handlers.push((u64::MAX, i, HitHandler::CuteCharm(t))),
                AbilityKind::Mummy => handlers.push((u64::MAX, i, HitHandler::Mummy(t))),
                AbilityKind::WanderingSpirit => {
                    handlers.push((u64::MAX, i, HitHandler::WanderingSpirit(t)))
                }
                AbilityKind::Electromorphosis => {
                    handlers.push((1, i, HitHandler::Electromorphosis(t)))
                }
                AbilityKind::Aftermath => handlers.push((1, i, HitHandler::Aftermath(t))),
                AbilityKind::InnardsOut => handlers.push((1, i, HitHandler::InnardsOut(t))),
                AbilityKind::EffectSpore => {
                    handlers.push((u64::MAX, i, HitHandler::EffectSpore(t)))
                }
                AbilityKind::Gooey => handlers.push((u64::MAX, i, HitHandler::Gooey(t))),
                _ => {}
            }
            match self.item_kind(t) {
                Some(ItemKind::RockyHelmet) => handlers.push((2, i, HitHandler::RockyHelmet(t))),
                Some(ItemKind::AirBalloon) => {
                    handlers.push((u64::MAX, i, HitHandler::AirBalloon(t)))
                }
                _ => {}
            }
            if self.has_ability(src, AbilityKind::PoisonTouch) {
                handlers.push((u64::MAX, i, HitHandler::PoisonTouch(t)));
            }
        }
        handlers.sort_by_key(|&(order, index, _)| (order, index));
        let contact = mv.has(MoveFlag::Contact);
        let dex = self.dex.clone();
        let type_name = dex.type_names[move_type as usize].as_str();
        for &(_, _, h) in handlers.iter() {
            match h {
                HitHandler::Thaw(t) => {
                    if self.mon(t).status == Status::Frz {
                        self.cure_status(t);
                    }
                }
                HitHandler::ContactDamage(t) => {
                    if contact {
                        let amount = (self.mon(src).maxhp as u32 / 8).max(1);
                        let ti = self.ident(t);
                        let name = self.ability_name(t);
                        self.apply_damage(
                            src,
                            amount,
                            &format!("|[from] ability: {name}|[of] {ti}"),
                        );
                    }
                }
                HitHandler::ContactStatus(t, st) => {
                    if contact && self.rng.chance(3, 10) {
                        let a = self.mon(t).ability;
                        self.try_set_status_from(src, st, t, EffectRef::Ability(a));
                    }
                }
                HitHandler::SpicySpray(t) => {
                    let a = self.mon(t).ability;
                    self.try_set_status_from(src, Status::Brn, t, EffectRef::Ability(a));
                }
                HitHandler::HitBoost(t) => self.hit_boost(src, t, mv, type_name),
                HitHandler::Rattled(t) => {
                    if matches!(type_name, "Dark" | "Bug" | "Ghost") {
                        let mut b = [0i8; 7];
                        b[Boost::Spe as usize] = 1;
                        let a = self.mon(t).ability;
                        self.boost(t, &b, Some(src), EffectRef::Ability(a), false, false);
                    }
                }
                HitHandler::RockyHelmet(t) => {
                    if contact && self.item_kind(t) == Some(ItemKind::RockyHelmet) {
                        let amount = (self.mon(src).maxhp as u32 / 6).max(1);
                        let ti = self.ident(t);
                        self.apply_damage(
                            src,
                            amount,
                            &format!("|[from] item: Rocky Helmet|[of] {ti}"),
                        );
                    }
                }
                HitHandler::AirBalloon(t) => {
                    if self.item_kind(t) == Some(ItemKind::AirBalloon) {
                        let ti = self.ident(t);
                        log_line!(self, "|-enditem|{ti}|Air Balloon");
                        self.mon_mut(t).item = None;
                        self.run_after_use_item(t);
                    }
                }
                HitHandler::PoisonTouch(t) => {
                    // Despite not being a secondary, Shield Dust blocks it.
                    if !self.shield_dust_on_modify_secondaries(t)
                        && contact
                        && self.rng.chance(3, 10)
                    {
                        // The handler was collected before a Wandering Spirit swap.
                        let a = self.dex.ids.abilities.poison_touch.expect("poisontouch");
                        self.try_set_status_from(t, Status::Psn, src, EffectRef::Ability(a));
                    }
                }
                HitHandler::ToxicDebris(t) => {
                    // Toxic Spikes on the attacker's side (its foes' side when an
                    // ally hit the holder), up to two layers.
                    let side = if src.0 == t.0 { 1 - src.0 } else { src.0 };
                    if mv.category == Category::Physical && self.sides[side].conds.toxic_spikes < 2
                    {
                        let ti = self.ident(t);
                        log_line!(self, "|-activate|{ti}|ability: Toxic Debris");
                        self.add_toxic_spikes(side);
                    }
                }
                HitHandler::SeedSower(t) => {
                    let name = self.ability_name(t);
                    self.set_terrain(Terrain::Grassy, t, Some(&name));
                }
                HitHandler::SandSpit(t) => {
                    let name = self.ability_name(t);
                    self.set_weather(Weather::Sand, t, Some(&name));
                }
                HitHandler::Counter(t) => self.counter_on_damaging_hit(src, t, mv),
                HitHandler::Illusion(t) => self.end_illusion(t),
                HitHandler::CuteCharm(t) => self.cute_charm_on_damaging_hit(src, t, contact),
                HitHandler::Mummy(t) => self.mummy_on_damaging_hit(src, t, contact),
                HitHandler::WanderingSpirit(t) => {
                    if contact {
                        self.skill_swap_quiet(src, t);
                    }
                }
                // Electromorphosis (order 1) adds `charge`, whose start names
                // the move that hit.
                HitHandler::Electromorphosis(t) => {
                    if self.mon(t).hp > 0 {
                        if !self.mon(t).vol.charge {
                            self.try_add_volatile_sort(t);
                            self.mon_mut(t).vol.charge = true;
                            self.stamp_volatile(t, ResidualVol::Charge);
                        }
                        let ti = self.ident(t);
                        log_line!(
                            self,
                            "|-start|{ti}|Charge|{}|[from] ability: Electromorphosis",
                            mv.name
                        );
                    }
                }
                // Effect Spore: checkMoveMakesContact, then
                // runStatusImmunity('powder'): Grass types, Overcoat (Safety
                // Goggles are not modelled).
                HitHandler::EffectSpore(t) => {
                    if contact && !self.mon(src).fainted {
                        let types = self.types_of(src);
                        let immune = types
                            .iter()
                            .any(|&t| self.dex.type_immune_to_effect(t, "powder"))
                            || self.has_ability(src, AbilityKind::Overcoat);
                        if !immune {
                            let r = self.rng.random(100);
                            let st = if r < 11 {
                                Some(Status::Slp)
                            } else if r < 21 {
                                Some(Status::Par)
                            } else if r < 30 {
                                Some(Status::Psn)
                            } else {
                                None
                            };
                            if let Some(st) = st {
                                let a = self.mon(t).ability;
                                self.try_set_status_from(src, st, t, EffectRef::Ability(a));
                            }
                        }
                    }
                }
                // Gooey: `checkMoveMakesContact`, then -1 Speed on the attacker,
                // announced as a secondary effect of the ability.
                HitHandler::Gooey(t) => {
                    if contact {
                        let ti = self.ident(t);
                        log_line!(self, "|-ability|{ti}|Gooey");
                        let mut b = [0i8; 7];
                        b[Boost::Spe as usize] = -1;
                        let a = self.mon(t).ability;
                        self.boost(src, &b, Some(t), EffectRef::Ability(a), true, false);
                    }
                }
                // Innards Out (order 1): the fainting hit's damage, plus the damage
                // of the move's earlier hits (`move.totalDamage`, unless smart
                // targeting), is dealt back to the attacker.
                HitHandler::InnardsOut(t) => {
                    if self.mon(t).hp == 0 {
                        let this_hit: u32 = self.active_move.hit_damage.iter().map(|e| e.1).sum();
                        let dealt = self
                            .active_move
                            .hit_damage
                            .iter()
                            .find(|(x, _)| *x == t)
                            .map_or(0, |e| e.1);
                        let prior = if self.active_move.smart {
                            0
                        } else {
                            self.active_move.total_damage.saturating_sub(this_hit)
                        };
                        let ti = self.ident(t);
                        self.apply_damage(
                            src,
                            dealt + prior,
                            &format!("|[from] ability: Innards Out|[of] {ti}"),
                        );
                    }
                }
                // Aftermath (order 1) hurts a contact attacker that fainted
                // its holder. Damp (`onAnyDamage`) on any active Pokémon stops
                // the damage, silently.
                HitHandler::Aftermath(t) => {
                    if self.mon(t).hp == 0 && contact {
                        let damp = self
                            .all_active()
                            .into_iter()
                            .any(|p| self.mon(p).hp > 0 && self.has_ability(p, AbilityKind::Damp));
                        if !damp {
                            let amount = (self.mon(src).maxhp as u32 / 4).max(1);
                            let ti = self.ident(t);
                            self.apply_damage(
                                src,
                                amount,
                                &format!("|[from] ability: Aftermath|[of] {ti}"),
                            );
                        }
                    }
                }
                HitHandler::CursedBody(t) => {
                    // 30% to Disable the attacker's move, unless it already has a
                    // Disable or the move is Struggle.
                    if self.mon(src).vol.disable.is_none()
                        && mv.id != "struggle"
                        && !mv.has(MoveFlag::FutureMove)
                        && self.rng.chance(3, 10)
                    {
                        self.add_disable(src, t);
                    }
                }
            }
        }
    }

    /// Stamina, Weak Armor, Justified, Water Compaction, Steam Engine, Thermal Exchange.
    fn hit_boost(&mut self, src: MonRef, t: MonRef, mv: &Move, type_name: &str) {
        let mut b = [0i8; 7];
        let mut source = Some(src);
        match self.ability_id(t) {
            "stamina" => b[Boost::Def as usize] = 1,
            "weakarmor" if mv.category == Category::Physical => {
                b[Boost::Def as usize] = -1;
                b[Boost::Spe as usize] = 2;
                source = Some(t);
            }
            "justified" if type_name == "Dark" => b[Boost::Atk as usize] = 1,
            "watercompaction" if type_name == "Water" => b[Boost::Def as usize] = 2,
            "steamengine" if matches!(type_name, "Water" | "Fire") => b[Boost::Spe as usize] = 6,
            "thermalexchange" if type_name == "Fire" => b[Boost::Atk as usize] = 1,
            _ => return,
        }
        let a = self.mon(t).ability;
        self.boost(t, &b, source, EffectRef::Ability(a), false, false);
    }
}
