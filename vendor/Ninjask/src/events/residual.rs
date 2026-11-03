//! The end-of-turn residual phase (`fieldEvent('Residual')`): collecting the
//! handlers and running them in order.

use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::EffectRef;
use crate::kinds::{AbilityKind, ItemKind};
use crate::state::*;

/// `comparePriority`'s stand-in for a missing order.
const NO_ORDER: u64 = 4294967296;

/// One residual handler (Showdown `EventListener`) with its sort keys.
#[derive(Clone, Copy, Debug)]
struct Handler {
    kind: HandlerKind,
    order: u64,
    speed: i32,
    sub_order: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandlerKind {
    Weather,
    GrassyHeal(MonRef),
    Status(MonRef, Status),
    SideCondition(usize, SideCond),
    TrickRoom,
    Terrain,
    /// A volatile with only a duration: one handler each, as in Showdown,
    /// so that equal ones tie (and are shuffled) the same way.
    Volatile(MonRef, DurationVolatile),
    Leftovers(MonRef),
    WhiteHerb(MonRef),
    SpeedBoost(MonRef),
    ThroatChop(MonRef),
    Taunt(MonRef),
    Encore(MonRef),
    Disable(MonRef),
    Yawn(MonRef),
    Roost(MonRef),
    /// Perish Song: the count drops each turn; at zero the Pokémon faints.
    Perish(MonRef),
    /// Heal Block: a duration only (no onResidual).
    HealBlock(MonRef),
    /// `partiallytrapped` (onResidualOrder 13).
    PartialTrap(MonRef),
    /// Hydration (order 5, subOrder 3).
    Hydration(MonRef),
    /// Wish's slot condition (order 4, subOrder 3), on the Pokémon in it.
    Wish(MonRef),
    /// Leech Seed (order 8).
    LeechSeed(MonRef),
    /// Aqua Ring (order 6) and Ingrain (order 7): heal 1/16.
    AquaRing(MonRef),
    Ingrain(MonRef),
    /// Salt Cure (order 13).
    SaltCure(MonRef),
    /// Moody (order 28, subOrder 2).
    Moody(MonRef),
    /// Healer (order 5, subOrder 3).
    Healer(MonRef),
    /// Gravity's duration (onFieldResidualOrder 27, subOrder 2).
    Gravity,
    /// Safeguard's duration on a side (onSideResidualOrder 26, subOrder 3).
    Safeguard(usize),
    /// Chilly Reception's one-turn volatile (duration only).
    ChillyReception(MonRef),
    /// Cud Chew (order 28, subOrder 2).
    CudChew(MonRef),
    /// Magnet Rise's duration (onResidualOrder 18).
    MagnetRise(MonRef),
    /// Harvest (order 28, subOrder 2).
    Harvest(MonRef),
    /// Shed Skin (order 5, subOrder 3) and Pickup (order 28, subOrder 2).
    ShedSkin(MonRef),
    Pickup(MonRef),
    /// Magic Room's duration (onFieldResidualOrder 27, subOrder 6).
    MagicRoom,
    /// Wonder Room's duration (onFieldResidualOrder 27, subOrder 5).
    WonderRoom,
    /// Uproar (onResidualOrder 28, subOrder 1).
    Uproar(MonRef),
    /// Future Sight's slot condition (onResidualOrder 3, subOrder 3), on the
    /// Pokémon in the slot.
    FutureMove(MonRef),
    /// A volatile with a residual handler or a duration: Counter, Mirror
    /// Coat, Gem, Beak Blast, Focus Punch, Syrup Bomb, Endure, Ally Switch,
    /// Curse, locked moves, Electrify, Lock-On and Octolock.
    ResidualVolatile(MonRef, ResidualVol),
    /// The Fairy Lock pseudo-weather.
    FairyLock,
    /// Hunger Switch (order 29, subOrder 2).
    HungerSwitch(MonRef),
    /// Opportunist (order 29, subOrder 7).
    Opportunist(MonRef),
}

/// Volatiles that are in the residual phase only for their duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurationVolatile {
    Flinch,
    Protect,
    Stall,
    HelpingHand,
    FollowMe,
    RagePowder,
    MustRecharge,
    TwoTurnMove,
    Charging,
}

/// Showdown `comparePriority` for handlers (all residual priorities are 0).
fn compare_handlers(a: &Handler, b: &Handler) -> i64 {
    let d = a.order as i64 - b.order as i64;
    if d != 0 {
        return d;
    }
    let d = b.speed as i64 - a.speed as i64;
    if d != 0 {
        return d;
    }
    a.sub_order as i64 - b.sub_order as i64
}

impl Battle {
    /// Showdown `fieldEvent('Residual')` for the effects the engine models.
    /// One Pokémon's `onResidual` handlers in Showdown's collection order:
    /// status, volatiles, ability, item, then Grassy Terrain's heal.
    fn mon_residual_handlers(&self, r: MonRef, handlers: &mut Small<Handler, 16>) {
        let order = |b: &Battle, id: &str, field: &str, default: u32| -> u64 {
            b.dex
                .cond_num(id, field)
                .map(|x| x as u64)
                .unwrap_or(default as u64)
        };
        let m = self.mon(r);
        let speed = m.speed;
        if matches!(m.status, Status::Psn | Status::Tox | Status::Brn) {
            let id = m.status.id();
            let default = if m.status == Status::Brn { 10 } else { 9 };
            handlers.push(Handler {
                kind: HandlerKind::Status(r, m.status),
                order: order(self, id, "onResidualOrder", default),
                speed,
                sub_order: 0,
            });
        }
        // The volatiles, in the order they were added.
        let mut vols: Small<(u32, Handler)> = Small::new();
        let mut add = |v: ResidualVol, kind: HandlerKind, order: u64| {
            vols.push((
                m.vol.added[v as usize],
                Handler {
                    kind,
                    order,
                    speed,
                    sub_order: 2,
                },
            ));
        };
        if m.vol.throat_chop > 0 {
            add(ResidualVol::ThroatChop, HandlerKind::ThroatChop(r), 22);
        }
        if m.vol.taunt > 0 {
            add(ResidualVol::Taunt, HandlerKind::Taunt(r), 15);
        }
        if m.vol.encore.is_some() {
            add(
                ResidualVol::Encore,
                HandlerKind::Encore(r),
                order(self, "encore", "onResidualOrder", 16),
            );
        }
        if m.vol.disable.is_some() {
            add(
                ResidualVol::Disable,
                HandlerKind::Disable(r),
                order(self, "disable", "onResidualOrder", 17),
            );
        }
        if m.vol.yawn > 0 {
            add(
                ResidualVol::Yawn,
                HandlerKind::Yawn(r),
                order(self, "yawn", "onResidualOrder", 23),
            );
        }
        if m.vol.roost {
            add(
                ResidualVol::Roost,
                HandlerKind::Roost(r),
                order(self, "roost", "onResidualOrder", 25),
            );
        }
        if m.vol.perish > 0 {
            add(
                ResidualVol::Perish,
                HandlerKind::Perish(r),
                order(self, "perishsong", "onResidualOrder", 24),
            );
        }
        if m.vol.heal_block > 0 {
            add(
                ResidualVol::HealBlock,
                HandlerKind::HealBlock(r),
                order(self, "healblock", "onResidualOrder", 20),
            );
        }
        let present = [
            (m.vol.flinch, DurationVolatile::Flinch, ResidualVol::Flinch),
            (
                m.vol.protect.is_some(),
                DurationVolatile::Protect,
                ResidualVol::Protect,
            ),
            (
                m.vol.stall_duration > 0,
                DurationVolatile::Stall,
                ResidualVol::Stall,
            ),
            (
                m.vol.helping_hand > 0,
                DurationVolatile::HelpingHand,
                ResidualVol::HelpingHand,
            ),
            (
                m.vol.follow_me,
                DurationVolatile::FollowMe,
                ResidualVol::FollowMe,
            ),
            (
                m.vol.rage_powder,
                DurationVolatile::RagePowder,
                ResidualVol::RagePowder,
            ),
            (
                m.vol.must_recharge > 0,
                DurationVolatile::MustRecharge,
                ResidualVol::MustRecharge,
            ),
            (
                m.vol.two_turn.is_some(),
                DurationVolatile::TwoTurnMove,
                ResidualVol::TwoTurnMove,
            ),
            (
                m.vol.charging_duration > 0,
                DurationVolatile::Charging,
                ResidualVol::Charging,
            ),
        ];
        for (on, v, rv) in present {
            if on {
                add(rv, HandlerKind::Volatile(r, v), NO_ORDER);
            }
        }
        // Volatiles with their own handler kind, then those run by
        // `volatile_residual`, each with its `onResidualOrder` (`NO_ORDER`:
        // a duration only).
        let v = &m.vol;
        let vol = |rv: ResidualVol| HandlerKind::ResidualVolatile(r, rv);
        let with_handler = [
            (
                v.partial_trap > 0,
                ResidualVol::PartialTrap,
                HandlerKind::PartialTrap(r),
                13,
            ),
            (
                v.leech_seed.is_some(),
                ResidualVol::LeechSeed,
                HandlerKind::LeechSeed(r),
                8,
            ),
            (
                v.aqua_ring,
                ResidualVol::AquaRing,
                HandlerKind::AquaRing(r),
                6,
            ),
            (v.ingrain, ResidualVol::Ingrain, HandlerKind::Ingrain(r), 7),
            (
                v.salt_cure,
                ResidualVol::SaltCure,
                HandlerKind::SaltCure(r),
                13,
            ),
            (
                v.magnet_rise > 0,
                ResidualVol::MagnetRise,
                HandlerKind::MagnetRise(r),
                18,
            ),
            (
                v.uproar > 0,
                ResidualVol::Uproar,
                HandlerKind::Uproar(r),
                28,
            ),
            (
                v.chilly_reception,
                ResidualVol::ChillyReception,
                HandlerKind::ChillyReception(r),
                NO_ORDER,
            ),
            (v.gem, ResidualVol::Gem, vol(ResidualVol::Gem), NO_ORDER),
            (
                v.counter.is_some(),
                ResidualVol::Counter,
                vol(ResidualVol::Counter),
                NO_ORDER,
            ),
            (
                v.mirror_coat.is_some(),
                ResidualVol::MirrorCoat,
                vol(ResidualVol::MirrorCoat),
                NO_ORDER,
            ),
            (
                v.beak_blast,
                ResidualVol::BeakBlast,
                vol(ResidualVol::BeakBlast),
                NO_ORDER,
            ),
            (
                v.focus_punch.is_some(),
                ResidualVol::FocusPunch,
                vol(ResidualVol::FocusPunch),
                NO_ORDER,
            ),
            (
                v.syrup_bomb.is_some(),
                ResidualVol::SyrupBomb,
                vol(ResidualVol::SyrupBomb),
                14,
            ),
            (
                v.endure,
                ResidualVol::Endure,
                vol(ResidualVol::Endure),
                NO_ORDER,
            ),
            (
                v.ally_switch_duration > 0,
                ResidualVol::AllySwitch,
                vol(ResidualVol::AllySwitch),
                NO_ORDER,
            ),
            (v.curse, ResidualVol::Curse, vol(ResidualVol::Curse), 12),
            (
                v.locked_move.is_some(),
                ResidualVol::LockedMove,
                vol(ResidualVol::LockedMove),
                NO_ORDER,
            ),
            (
                v.electrify,
                ResidualVol::Electrify,
                vol(ResidualVol::Electrify),
                NO_ORDER,
            ),
            (
                v.lock_on_duration > 0,
                ResidualVol::LockOn,
                vol(ResidualVol::LockOn),
                NO_ORDER,
            ),
            (
                v.octolock.is_some(),
                ResidualVol::Octolock,
                vol(ResidualVol::Octolock),
                14,
            ),
        ];
        for (on, rv, kind, order) in with_handler {
            if on {
                add(rv, kind, order);
            }
        }
        // Uproar's `onResidualSubOrder` is 1 (volatiles default to 2).
        for (_, h) in vols.iter_mut() {
            if matches!(h.kind, HandlerKind::Uproar(_)) {
                h.sub_order = 1;
            }
        }
        vols.sort_by_key(|(added, _)| *added);
        handlers.extend(vols.into_iter().map(|(_, h)| h));
        // The ability: (handler, onResidualOrder, onResidualSubOrder).
        // Opportunist's handler goes by the raw ability (it checks the
        // ability itself when it runs).
        let ability = match self.ability_kind(r) {
            AbilityKind::SpeedBoost => Some((HandlerKind::SpeedBoost(r), 28, 2)),
            AbilityKind::HungerSwitch => Some((HandlerKind::HungerSwitch(r), 29, 2)),
            AbilityKind::Hydration => Some((HandlerKind::Hydration(r), 5, 3)),
            AbilityKind::Moody => Some((HandlerKind::Moody(r), 28, 2)),
            AbilityKind::Healer => Some((HandlerKind::Healer(r), 5, 3)),
            AbilityKind::CudChew => Some((HandlerKind::CudChew(r), 28, 2)),
            AbilityKind::Harvest => Some((HandlerKind::Harvest(r), 28, 2)),
            AbilityKind::ShedSkin => Some((HandlerKind::ShedSkin(r), 5, 3)),
            AbilityKind::Pickup => Some((HandlerKind::Pickup(r), 28, 2)),
            _ if self.raw_ability_kind(r) == AbilityKind::Opportunist => {
                Some((HandlerKind::Opportunist(r), 29, 7))
            }
            _ => None,
        };
        // The item.
        let item = match self.item_kind(r) {
            Some(ItemKind::Leftovers) => Some((HandlerKind::Leftovers(r), 5, 4)),
            Some(ItemKind::WhiteHerb) => Some((HandlerKind::WhiteHerb(r), 29, 8)),
            _ => None,
        };
        // The slot conditions of the Pokémon's slot: Future Sight, Wish.
        let slot = &self.slot_conds[r.0][r.1];
        let future = slot
            .future
            .is_some()
            .then_some((HandlerKind::FutureMove(r), 3, 3));
        let wish = slot.wish.is_some().then_some((HandlerKind::Wish(r), 4, 3));
        for (kind, order, sub_order) in [ability, item, future, wish].into_iter().flatten() {
            handlers.push(Handler {
                kind,
                order,
                speed,
                sub_order,
            });
        }
        if self.field.terrain == Some(Terrain::Grassy) {
            handlers.push(Handler {
                kind: HandlerKind::GrassyHeal(r),
                order: order(self, "grassyterrain", "onResidualOrder", 5),
                speed,
                sub_order: self.cond_u32("grassyterrain", "onResidualSubOrder", 2),
            });
        }
    }

    pub(crate) fn run_residual(&mut self) {
        let mut handlers: Small<Handler, 16> = Small::new();
        self.turn_state.sorting = true;
        let order = |b: &Battle, id: &str, field: &str, default: u32| -> u64 {
            b.dex
                .cond_num(id, field)
                .map(|x| x as u64)
                .unwrap_or(default as u64)
        };
        // Showdown's fieldEvent('Residual') collects, in this order: the
        // field's own handlers (pseudo-weather, weather, terrain); then per
        // side, its side conditions and each Pokémon in its slot (fainted
        // ones too, skipped when run): status, volatiles, ability, item,
        // then the field's handlers for it (Grassy Terrain). The sort's ties
        // depend on this order.
        //
        // Checked against `fieldEvent` and `findPokemonEventHandlers`
        // (status, volatiles, ability, item, species, slot conditions):
        // Showdown takes pseudo-weather and side conditions in insertion
        // order, where this pushes them in a fixed order (Trick Room,
        // Gravity, Magic Room, Wonder Room, Fairy Lock; Safeguard after the
        // other side conditions). Their orders and subOrders differ, so no
        // two of them tie within a side and the sort's draws are the same.
        if self.field.trick_room > 0 {
            handlers.push(Handler {
                kind: HandlerKind::TrickRoom,
                order: order(self, "trickroom", "onFieldResidualOrder", 27),
                speed: 0,
                sub_order: self.cond_u32("trickroom", "onFieldResidualSubOrder", 1),
            });
        }
        // Gravity (27, 2), Magic Room (27, 6), Wonder Room (27, 5), and Fairy
        // Lock (a duration only; a pseudo-weather's state has no target, so
        // subOrder 2).
        let pseudo_weather = [
            (self.field.gravity > 0, HandlerKind::Gravity, 27, 2),
            (self.field.magic_room > 0, HandlerKind::MagicRoom, 27, 6),
            (self.field.wonder_room > 0, HandlerKind::WonderRoom, 27, 5),
            (
                self.field.fairy_lock > 0,
                HandlerKind::FairyLock,
                NO_ORDER,
                2,
            ),
        ];
        for (on, kind, order, sub_order) in pseudo_weather {
            if on {
                handlers.push(Handler {
                    kind,
                    order,
                    speed: 0,
                    sub_order,
                });
            }
        }
        if let Some(w) = self.field.weather {
            handlers.push(Handler {
                kind: HandlerKind::Weather,
                order: order(self, w.id(), "onFieldResidualOrder", 1),
                speed: 0,
                sub_order: 5,
            });
        }
        if let Some(t) = self.field.terrain {
            handlers.push(Handler {
                kind: HandlerKind::Terrain,
                order: order(self, t.id(), "onFieldResidualOrder", 27),
                speed: 0,
                sub_order: self.cond_u32(t.id(), "onFieldResidualSubOrder", 7),
            });
        }
        for s in 0..2 {
            for c in self.sides[s].conds.in_order() {
                handlers.push(Handler {
                    kind: HandlerKind::SideCondition(s, c),
                    order: if c.is_guard() {
                        NO_ORDER
                    } else {
                        order(self, c.id(), "onSideResidualOrder", 26)
                    },
                    speed: 0,
                    sub_order: self.cond_u32(
                        c.id(),
                        "onSideResidualSubOrder",
                        c.residual_sub_order(),
                    ),
                });
            }
            // Safeguard (onSideResidualOrder 26, subOrder 3).
            if self.side_extras[s].safeguard > 0 {
                handlers.push(Handler {
                    kind: HandlerKind::Safeguard(s),
                    order: 26,
                    speed: 0,
                    sub_order: 3,
                });
            }
            for sl in 0..self.sides[s].n_active {
                let Some(r) = self.active(s, sl) else {
                    continue;
                };
                self.mon_residual_handlers(r, &mut handlers);
            }
        }
        self.turn_state.sorting = false;
        self.speed_sort_by(&mut handlers, compare_handlers);

        for &h in handlers.iter() {
            match h.kind {
                HandlerKind::GrassyHeal(r)
                | HandlerKind::Status(r, _)
                | HandlerKind::Volatile(r, _)
                | HandlerKind::Leftovers(r)
                | HandlerKind::WhiteHerb(r)
                | HandlerKind::SpeedBoost(r)
                | HandlerKind::ThroatChop(r)
                | HandlerKind::Taunt(r)
                | HandlerKind::Encore(r)
                | HandlerKind::Disable(r)
                | HandlerKind::Yawn(r)
                | HandlerKind::Roost(r)
                | HandlerKind::Perish(r)
                | HandlerKind::HealBlock(r)
                | HandlerKind::HungerSwitch(r)
                | HandlerKind::Opportunist(r)
                | HandlerKind::ResidualVolatile(r, _)
                    if (self.mon(r).fainted || !self.mon(r).is_active) =>
                {
                    continue;
                }
                _ => {}
            }
            // A handler whose effect's duration runs out ends it, and Showdown
            // moves on without the faint check that follows a callback
            // (`continue`): a Perish Song faint waits for the next handler
            // that runs, or for the end of the residual action.
            if self.residual_handler(h.kind) != Some(false) {
                continue;
            }
            self.faint_messages();
            if self.ended {
                return;
            }
        }
    }

    /// Run one residual handler. `None`: its effect is gone (skipped);
    /// `Some(true)`: its duration ran out and ended it; `Some(false)`: it ran,
    /// and the faint check follows.
    fn residual_handler(&mut self, kind: HandlerKind) -> Option<bool> {
        Some(match kind {
            HandlerKind::Weather => {
                let w = self.field.weather?;
                if self.field.weather_turns > 0 {
                    self.field.weather_turns -= 1;
                    if self.field.weather_turns == 0 {
                        self.clear_weather();
                        return None;
                    }
                }
                log_line!(self, "|-weather|{}|[upkeep]", w.name());
                let weather_event = match w {
                    Weather::Rain | Weather::Sun => true,
                    Weather::Sand | Weather::Snow => self.effective_weather() == Some(w),
                };
                if weather_event {
                    self.run_weather(w);
                }
                if weather_event {
                    self.run_update();
                }
                false
            }
            HandlerKind::GrassyHeal(r) => {
                if self.field.terrain != Some(Terrain::Grassy) {
                    return None;
                }
                if self.is_grounded(r) && !self.semi_invulnerable(r) {
                    let amount = (self.mon(r).maxhp as u32 / 16).max(1);
                    self.heal(r, amount, "|[from] Grassy Terrain");
                }
                false
            }
            HandlerKind::Status(r, st) => {
                if self.mon(r).status != st {
                    return None;
                }
                let maxhp = self.mon(r).maxhp as u32;
                match st {
                    Status::Psn => {
                        self.apply_damage(r, (maxhp / 8).max(1), "|[from] psn");
                    }
                    Status::Tox => {
                        let m = self.mon_mut(r);
                        if m.toxic_stage < 15 {
                            m.toxic_stage += 1;
                        }
                        let d = (maxhp / 16).max(1) * m.toxic_stage as u32;
                        self.apply_damage(r, d, "|[from] psn");
                    }
                    Status::Brn => {
                        self.apply_damage(
                            r,
                            self.burn_modify_damage(r, (maxhp / 16).max(1)),
                            "|[from] brn",
                        );
                    }
                    _ => {}
                }
                false
            }
            HandlerKind::SideCondition(s, c) => {
                let d = self.sides[s].conds.get_mut(c);
                if *d == 0 {
                    return None;
                }
                *d -= 1;
                if *d == 0 && !c.is_guard() {
                    let side = self.side_ident(s);
                    log_line!(self, "|-sideend|{side}|{}", c.log_name());
                }
                self.sides[s].conds.get(c) == 0
            }
            HandlerKind::TrickRoom => {
                if self.field.trick_room == 0 {
                    return None;
                }
                self.field.trick_room -= 1;
                if self.field.trick_room == 0 {
                    self.add("|-fieldend|move: Trick Room");
                }
                self.field.trick_room == 0
            }
            HandlerKind::Terrain => {
                if self.field.terrain.is_none() || self.field.terrain_turns == 0 {
                    return None;
                }
                self.field.terrain_turns -= 1;
                if self.field.terrain_turns == 0 {
                    self.clear_terrain();
                    true
                } else {
                    false
                }
            }
            HandlerKind::Leftovers(r) => {
                if self.item_kind(r) == Some(ItemKind::Leftovers) {
                    let amount = (self.mon(r).maxhp as u32 / 16).max(1);
                    self.ripen_on_try_heal_leftovers(r);
                    self.heal(r, amount, "|[from] item: Leftovers");
                }
                false
            }
            HandlerKind::WhiteHerb(r) => {
                self.white_herb_check(r);
                false
            }
            HandlerKind::SpeedBoost(r) => {
                self.speed_boost(r);
                false
            }
            HandlerKind::HungerSwitch(r) => {
                self.hunger_switch(r);
                false
            }
            HandlerKind::Opportunist(r) => {
                self.opportunist_release(r);
                false
            }
            HandlerKind::ThroatChop(r) => {
                let m = self.mon_mut(r);
                m.vol.throat_chop = m.vol.throat_chop.saturating_sub(1);
                if m.vol.throat_chop == 0 {
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|Throat Chop|[silent]");
                }
                self.mon(r).vol.throat_chop == 0
            }
            HandlerKind::Taunt(r) => {
                let m = self.mon_mut(r);
                m.vol.taunt = m.vol.taunt.saturating_sub(1);
                if m.vol.taunt == 0 {
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|move: Taunt");
                }
                self.mon(r).vol.taunt == 0
            }
            HandlerKind::Encore(r) => {
                let e = self.mon(r).vol.encore?;
                // The duration first; then onResidual ends it early when the
                // encored move is out of PP.
                let m = self.mon_mut(r);
                m.vol.encore_duration = m.vol.encore_duration.saturating_sub(1);
                let expired = m.vol.encore_duration == 0;
                let out_of_pp = !m.moves.iter().any(|ms| ms.id == e && ms.pp > 0);
                if expired || out_of_pp {
                    m.vol.encore = None;
                    m.vol.encore_duration = 0;
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|Encore");
                }
                expired
            }
            HandlerKind::Disable(r) => {
                self.mon(r).vol.disable?;
                let m = self.mon_mut(r);
                m.vol.disable_duration = m.vol.disable_duration.saturating_sub(1);
                if m.vol.disable_duration == 0 {
                    m.vol.disable = None;
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|Disable");
                }
                self.mon(r).vol.disable.is_none()
            }
            HandlerKind::Yawn(r) => {
                if self.mon(r).vol.yawn == 0 {
                    return None;
                }
                let m = self.mon_mut(r);
                m.vol.yawn -= 1;
                if m.vol.yawn == 0 {
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|move: Yawn|[silent]");
                    self.try_set_status(r, Status::Slp, r, crate::kinds::EffectRef::Yawn, false);
                }
                self.mon(r).vol.yawn == 0
            }
            HandlerKind::Roost(r) => {
                self.mon_mut(r).vol.roost = false;
                true
            }
            HandlerKind::Perish(r) => {
                if self.mon(r).vol.perish == 0 {
                    return None;
                }
                let m = self.mon_mut(r);
                m.vol.perish -= 1;
                let left = m.vol.perish;
                let ident = self.ident(r);
                log_line!(self, "|-start|{ident}|perish{left}");
                if left == 0 {
                    // onEnd: faint().
                    self.mon_mut(r).hp = 0;
                    self.queue_faint(r);
                }
                left == 0
            }
            HandlerKind::ResidualVolatile(r, v) => self.volatile_residual(r, v)?,
            HandlerKind::FairyLock => {
                // Its duration runs down; it ends silently.
                if self.field.fairy_lock == 0 {
                    return None;
                }
                self.field.fairy_lock -= 1;
                self.field.fairy_lock == 0
            }
            HandlerKind::HealBlock(r) => {
                if self.mon(r).vol.heal_block == 0 {
                    return None;
                }
                let m = self.mon_mut(r);
                m.vol.heal_block -= 1;
                if m.vol.heal_block == 0 {
                    let ident = self.ident(r);
                    log_line!(self, "|-end|{ident}|move: Heal Block");
                }
                self.mon(r).vol.heal_block == 0
            }
            HandlerKind::Volatile(r, v) => {
                let m = self.mon_mut(r);
                match v {
                    // One-turn volatiles end.
                    DurationVolatile::Flinch => m.vol.flinch = false,
                    DurationVolatile::Protect => m.vol.protect = None,
                    DurationVolatile::HelpingHand => m.vol.helping_hand = 0,
                    DurationVolatile::FollowMe => m.vol.follow_me = false,
                    DurationVolatile::RagePowder => m.vol.rage_powder = false,
                    DurationVolatile::Stall => {
                        m.vol.stall_duration -= 1;
                        if m.vol.stall_duration == 0 {
                            m.vol.stall_counter = 0;
                        }
                    }
                    DurationVolatile::MustRecharge => {
                        m.vol.must_recharge = m.vol.must_recharge.saturating_sub(1)
                    }
                    // Its end also removes the charging move's volatile.
                    DurationVolatile::TwoTurnMove => {
                        m.vol.two_turn_duration = m.vol.two_turn_duration.saturating_sub(1);
                        if m.vol.two_turn_duration == 0 {
                            m.vol.two_turn = None;
                            m.vol.charging = None;
                            m.vol.charging_target_loc = 0;
                            m.vol.charging_duration = 0;
                        }
                    }
                    // Removed already when twoturnmove ended first.
                    DurationVolatile::Charging => {
                        m.vol.charging_duration = m.vol.charging_duration.saturating_sub(1);
                        if m.vol.charging_duration == 0 {
                            m.vol.charging = None;
                            m.vol.charging_target_loc = 0;
                        }
                    }
                }
                let m = self.mon(r);
                match v {
                    DurationVolatile::Stall => m.vol.stall_duration == 0,
                    DurationVolatile::MustRecharge => m.vol.must_recharge == 0,
                    DurationVolatile::TwoTurnMove => m.vol.two_turn.is_none(),
                    DurationVolatile::Charging => m.vol.charging.is_none(),
                    _ => true,
                }
            }
            HandlerKind::PartialTrap(r) => self.partial_trap_residual(r)?,
            HandlerKind::Wish(r) => self.wish_residual(r)?,
            HandlerKind::FutureMove(r) => self.future_move_residual(r)?,
            HandlerKind::LeechSeed(r) => self.leech_seed_residual(r)?,
            HandlerKind::AquaRing(r) | HandlerKind::Ingrain(r) => {
                let m = self.mon(r);
                let (on, name) = match kind {
                    HandlerKind::AquaRing(_) => (m.vol.aqua_ring, "Aqua Ring"),
                    _ => (m.vol.ingrain, "Ingrain"),
                };
                if m.fainted || !m.is_active || !on {
                    return None;
                }
                // TryHeal: Big Root.
                let amount = self.drain_heal_mod(r, (m.maxhp as u32 / 16).max(1));
                self.heal(r, amount, &format!("|[from] {name}"));
                false
            }
            HandlerKind::SaltCure(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !m.vol.salt_cure {
                    return None;
                }
                // Champions: 1/8 to Water and Steel types, 1/16 to the rest.
                let hit_hard = ["Water", "Steel"]
                    .iter()
                    .any(|t| self.dex.type_id(t).is_some_and(|t| self.has_type_of(r, t)));
                let amount = (self.mon(r).maxhp as u32 / if hit_hard { 8 } else { 16 }).max(1);
                self.apply_damage(r, amount, "|[from] Salt Cure");
                false
            }
            HandlerKind::Moody(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::Moody) {
                    return None;
                }
                self.moody(r);
                false
            }
            HandlerKind::Healer(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::Healer) {
                    return None;
                }
                // Champions: a 1 in 2 chance for each statused adjacent ally.
                for ally in self.allies_of(r) {
                    if self.mon(ally).status != Status::None && self.rng.chance(1, 2) {
                        let ri = self.ident(r);
                        log_line!(self, "|-activate|{ri}|ability: Healer");
                        self.cure_status(ally);
                    }
                }
                false
            }
            HandlerKind::MagnetRise(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || m.vol.magnet_rise == 0 {
                    return None;
                }
                self.mon_mut(r).vol.magnet_rise -= 1;
                if self.mon(r).vol.magnet_rise == 0 {
                    let ti = self.ident(r);
                    log_line!(self, "|-end|{ti}|Magnet Rise");
                    return Some(true);
                }
                false
            }
            HandlerKind::Harvest(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::Harvest) {
                    return None;
                }
                self.harvest(r);
                false
            }
            HandlerKind::ShedSkin(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::ShedSkin) {
                    return None;
                }
                if m.hp > 0 && m.status != Status::None && self.rng.chance(33, 100) {
                    let ri = self.ident(r);
                    log_line!(self, "|-activate|{ri}|ability: Shed Skin");
                    self.cure_status(r);
                }
                false
            }
            HandlerKind::Pickup(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::Pickup) {
                    return None;
                }
                self.pickup(r);
                false
            }
            HandlerKind::MagicRoom => {
                if self.field.magic_room == 0 {
                    return None;
                }
                self.field.magic_room -= 1;
                if self.field.magic_room == 0 {
                    self.end_magic_room();
                    return Some(true);
                }
                false
            }
            HandlerKind::WonderRoom => {
                if self.field.wonder_room == 0 {
                    return None;
                }
                self.field.wonder_room -= 1;
                if self.field.wonder_room == 0 {
                    log_line!(self, "|-fieldend|move: Wonder Room");
                    return Some(true);
                }
                false
            }
            HandlerKind::Uproar(r) => self.uproar_residual(r)?,
            HandlerKind::Gravity => {
                if self.field.gravity == 0 {
                    return None;
                }
                self.field.gravity -= 1;
                if self.field.gravity == 0 {
                    self.add("|-fieldend|move: Gravity");
                    return Some(true);
                }
                false
            }
            HandlerKind::Safeguard(s) => {
                let a = &mut self.side_extras[s];
                if a.safeguard == 0 {
                    return None;
                }
                a.safeguard -= 1;
                if a.safeguard == 0 {
                    let si = self.side_ident(s);
                    log_line!(self, "|-sideend|{si}|Safeguard");
                    return Some(true);
                }
                false
            }
            HandlerKind::ChillyReception(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !m.vol.chilly_reception {
                    return None;
                }
                // duration 1 runs out: removeVolatile, with nothing to say.
                self.mon_mut(r).vol.chilly_reception = false;
                true
            }
            HandlerKind::CudChew(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::CudChew) {
                    return None;
                }
                self.cud_chew(r);
                false
            }
            HandlerKind::Hydration(r) => {
                let m = self.mon(r);
                if m.fainted || !m.is_active || !self.has_ability(r, AbilityKind::Hydration) {
                    return None;
                }
                if m.status != Status::None && self.effective_weather() == Some(Weather::Rain) {
                    let ri = self.ident(r);
                    log_line!(self, "|-activate|{ri}|ability: Hydration");
                    self.cure_status(r);
                }
                false
            }
        })
    }

    /// Leech Seed's residual: whoever stands in the seeder's slot, unless
    /// fainted, drinks 1/8 of the Pokémon's max HP (Big Root applies; the
    /// heal is silent).
    pub(crate) fn leech_seed_residual(&mut self, r: MonRef) -> Option<bool> {
        let m = self.mon(r);
        if m.fainted || !m.is_active {
            return None;
        }
        let (s, slot) = m.vol.leech_seed?;
        let Some(target) = self.active(s, slot) else {
            return Some(false);
        };
        if self.mon(target).fainted || self.mon(target).hp == 0 {
            return Some(false);
        }
        let amount = (m.maxhp as u32 / 8).max(1);
        let ti = self.ident(target);
        // `source !== target` gates the [of] (the seed's slot now holds the victim).
        let of = if target == r {
            String::new()
        } else {
            format!("|[of] {ti}")
        };
        let dealt = self.apply_damage(r, amount, &format!("|[from] Leech Seed{of}"));
        if dealt > 0 && !self.liquid_ooze_on_source_try_heal(target, r, dealt) {
            let amount = self.drain_heal_mod(target, dealt);
            self.heal(target, amount, "|[silent]");
        }
        Some(false)
    }

    /// The duration runs down first (its end announces it); then, if the
    /// user has left, fainted or only just come in (`!activeTurns`), the
    /// volatile goes silently; otherwise 1/8 (1/6) of max HP.
    pub(crate) fn partial_trap_residual(&mut self, r: MonRef) -> Option<bool> {
        let m = self.mon(r);
        if m.fainted || !m.is_active || m.vol.partial_trap == 0 {
            return None;
        }
        let name = m
            .vol
            .partial_trap_move
            .map(|id| self.dex.mv(id).name.clone())
            .unwrap_or_default();
        let ti = self.ident(r);
        self.mon_mut(r).vol.partial_trap -= 1;
        if self.mon(r).vol.partial_trap == 0 {
            self.clear_partial_trap(r);
            log_line!(self, "|-end|{ti}|{name}|[partiallytrapped]");
            return Some(true);
        }
        let gone = match self.mon(r).vol.partial_trap_source {
            None => true,
            Some(s) => {
                let sm = self.mon(s);
                !sm.is_active || sm.fainted || sm.hp == 0 || sm.active_turns == 0
            }
        };
        if gone {
            self.end_partial_trap_silently(r);
            return Some(false);
        }
        let m = self.mon(r);
        let amount = (m.maxhp as u32 / m.vol.partial_trap_divisor.max(1) as u32).max(1);
        self.apply_damage(
            r,
            amount,
            &format!("|[from] move: {name}|[partiallytrapped]"),
        );
        Some(false)
    }

    /// Uproar's residual: the duration counts down and ends it; otherwise
    /// Throat Chop ends it, or a Struggle drops it silently; else upkeep.
    pub(crate) fn uproar_residual(&mut self, r: MonRef) -> Option<bool> {
        let m = self.mon(r);
        if m.fainted || !m.is_active || m.vol.uproar == 0 {
            return None;
        }
        let ri = self.ident(r);
        self.mon_mut(r).vol.uproar -= 1;
        // removeVolatile does nothing for a Pokémon at 0 HP (a Perish Song
        // faint not yet announced): no end message.
        let alive = self.mon(r).hp > 0;
        if self.mon(r).vol.uproar == 0 {
            if alive {
                log_line!(self, "|-end|{ri}|Uproar");
            }
            return Some(true);
        }
        if self.mon(r).vol.throat_chop > 0 {
            self.mon_mut(r).vol.uproar = 0;
            if alive {
                log_line!(self, "|-end|{ri}|Uproar");
            }
            return Some(false);
        }
        if self.mon(r).last_move.is_some() && self.mon(r).last_move == self.dex.ids.moves.struggle {
            self.mon_mut(r).vol.uproar = 0;
            return Some(false);
        }
        log_line!(self, "|-start|{ri}|Uproar|[upkeep]");
        Some(false)
    }

    /// Moody: +2 to a random stat below +6 (not accuracy or evasion), then
    /// -1 to another above -6, applied in that order.
    pub(crate) fn moody(&mut self, r: MonRef) {
        let cur = self.mon(r).boosts;
        let mut b = crate::dex::types::MoveBoosts::default();
        let mut n = 0;
        let up: Vec<usize> = (0..5).filter(|&i| cur[i] < 6).collect();
        let plus = if up.is_empty() {
            None
        } else {
            Some(self.rng.sample(&up))
        };
        if let Some(i) = plus {
            b.vals[i] = 2;
            b.order[n] = i as u8;
            n += 1;
        }
        let down: Vec<usize> = (0..5).filter(|&i| cur[i] > -6 && Some(i) != plus).collect();
        if !down.is_empty() {
            let i = self.rng.sample(&down);
            b.vals[i] = -1;
            b.order[n] = i as u8;
            n += 1;
        }
        // The rest of the order, unused.
        let mut k = n;
        for i in 0..7u8 {
            if !b.order[..n].contains(&i) {
                b.order[k] = i;
                k += 1;
            }
        }
        let a = self.mon(r).ability;
        self.boost_listed(
            r,
            &b,
            Some(r),
            crate::kinds::EffectRef::Ability(a),
            false,
            false,
        );
    }

    /// The residual handler of a volatile (`HandlerKind::ResidualVolatile`):
    /// whether it ended (`None`: it was gone already).
    pub(crate) fn volatile_residual(&mut self, r: MonRef, v: ResidualVol) -> Option<bool> {
        match v {
            ResidualVol::Gem if self.mon(r).vol.gem => {
                self.mon_mut(r).vol.gem = false;
                Some(true)
            }
            // One-turn volatiles: the duration runs out, with nothing to say.
            ResidualVol::Counter if self.mon(r).vol.counter.is_some() => {
                self.mon_mut(r).vol.counter = None;
                Some(true)
            }
            ResidualVol::MirrorCoat if self.mon(r).vol.mirror_coat.is_some() => {
                self.mon_mut(r).vol.mirror_coat = None;
                Some(true)
            }
            ResidualVol::BeakBlast if self.mon(r).vol.beak_blast => {
                self.mon_mut(r).vol.beak_blast = false;
                Some(true)
            }
            ResidualVol::FocusPunch if self.mon(r).vol.focus_punch.is_some() => {
                self.mon_mut(r).vol.focus_punch = None;
                Some(true)
            }
            // Syrup Bomb: four turns; the last one ends it (silently), the
            // others lower the holder's Speed, crediting the thrower.
            ResidualVol::SyrupBomb if self.mon(r).vol.syrup_bomb.is_some() => {
                let v = &mut self.mon_mut(r).vol;
                v.syrup_bomb_duration -= 1;
                if v.syrup_bomb_duration == 0 {
                    v.syrup_bomb = None;
                    let ri = self.ident(r);
                    log_line!(self, "|-end|{ri}|Syrup Bomb|[silent]");
                    return Some(true);
                }
                let src = v.syrup_bomb;
                let mut b = [0i8; 7];
                b[Boost::Spe as usize] = -1;
                self.boost(r, &b, src, EffectRef::Other, false, false);
                Some(false)
            }
            ResidualVol::Endure => {
                if !self.mon(r).vol.endure {
                    return None;
                }
                self.mon_mut(r).vol.endure = false;
                Some(true)
            }
            ResidualVol::AllySwitch => {
                let v = &mut self.mon_mut(r).vol;
                if v.ally_switch_duration == 0 {
                    return None;
                }
                v.ally_switch_duration -= 1;
                if v.ally_switch_duration == 0 {
                    v.ally_switch_counter = 0;
                }
                Some(v.ally_switch_duration == 0)
            }
            ResidualVol::Curse => {
                if !self.mon(r).vol.curse {
                    return None;
                }
                let amount = (self.mon(r).maxhp as u32 / 4).max(1);
                self.apply_damage(r, amount, "|[from] Curse");
                Some(false)
            }
            ResidualVol::LockedMove => {
                let v = self.mon(r).vol;
                v.locked_move?;
                // The duration runs out (its end), else onResidual: sleep
                // drops it without the end; the true duration counts down.
                if v.locked_duration <= 1 {
                    self.end_locked_move(r);
                    return Some(true);
                }
                let asleep = self.mon(r).status == Status::Slp;
                let v = &mut self.mon_mut(r).vol;
                v.locked_duration -= 1;
                if asleep {
                    v.locked_move = None;
                    v.locked_duration = 0;
                    v.locked_true_duration = 0;
                } else {
                    v.locked_true_duration = v.locked_true_duration.saturating_sub(1);
                }
                Some(false)
            }
            ResidualVol::Electrify => {
                if !self.mon(r).vol.electrify {
                    return None;
                }
                self.mon_mut(r).vol.electrify = false;
                Some(true)
            }
            ResidualVol::LockOn => {
                let v = &mut self.mon_mut(r).vol;
                if v.lock_on_duration == 0 {
                    return None;
                }
                v.lock_on_duration -= 1;
                if v.lock_on_duration == 0 {
                    v.lock_on = None;
                }
                Some(v.lock_on_duration == 0)
            }
            ResidualVol::Octolock => {
                let src = self.mon(r).vol.octolock?;
                let s = self.mon(src);
                if !s.is_active || s.hp == 0 || s.active_turns == 0 {
                    self.mon_mut(r).vol.octolock = None;
                    let ti = self.ident(r);
                    log_line!(self, "|-end|{ti}|Octolock|[partiallytrapped]|[silent]");
                    return Some(false);
                }
                let mut b = [0i8; 7];
                b[Boost::Def as usize] = -1;
                b[Boost::Spd as usize] = -1;
                let octolock = self.dex.ids.moves.octolock.expect("octolock");
                self.boost(r, &b, Some(src), EffectRef::Move(octolock), false, false);
                Some(false)
            }
            _ => None,
        }
    }
}
