//! Field and side moves: screens, hazards, Defog, Court Change, Heal Bell and Tidy Up.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, MoveKind};
use crate::state::*;

/// One of a side's conditions that Court Change swaps, for its insertion order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Swapped {
    Cond(crate::state::SideCond),
    ToxicSpikes,
    StealthRock,
    Spikes,
    StickyWeb,
    Safeguard,
}

/// Side and field moves handled in this file.
pub(crate) fn field_move_kind(k: MoveKind) -> bool {
    matches!(
        k,
        MoveKind::Safeguard
            | MoveKind::Spikes
            | MoveKind::ToxicSpikes
            | MoveKind::StickyWeb
            | MoveKind::CourtChange
            | MoveKind::ChillyReception
            | MoveKind::HealBell
            | MoveKind::MagicRoom
            | MoveKind::WonderRoom
            | MoveKind::Teatime
    )
}

impl Battle {
    /// A side or field move (`sideCondition`, `onHitField`):
    /// whether it worked.
    pub(crate) fn run_field_move(&mut self, r: MonRef, kind: MoveKind) -> bool {
        match kind {
            MoveKind::Safeguard => self.add_safeguard(r.0),
            MoveKind::Spikes => self.add_spikes(1 - r.0),
            MoveKind::ToxicSpikes => self.add_toxic_spikes(1 - r.0),
            MoveKind::StickyWeb => self.add_sticky_web(1 - r.0),
            MoveKind::CourtChange => self.court_change(r),
            MoveKind::HealBell => self.heal_bell(r),
            MoveKind::MagicRoom => self.use_magic_room(r),
            MoveKind::WonderRoom => self.use_wonder_room(r),
            MoveKind::Teatime => self.teatime(r),
            // moveHit: the weather, then selfSwitch counts as doing something
            // whenever the user can switch.
            MoveKind::ChillyReception => {
                let set = self.set_weather(Weather::Snow, r, None);
                let can_switch = self.can_switch_side(r.0);
                if let Some(id) = self
                    .dex
                    .move_id("chillyreception")
                    .filter(|_| set || can_switch)
                {
                    self.set_self_switch(r, id);
                }
                set || can_switch
            }
            _ => true,
        }
    }

    /// `addSideCondition('safeguard')`: five turns; fails when already up.
    pub(crate) fn add_safeguard(&mut self, s: usize) -> bool {
        if self.side_extras[s].safeguard > 0 {
            return false;
        }
        self.side_extras[s].safeguard = self
            .dex
            .cond_num("safeguard", "duration")
            .map(|x| x as u8)
            .unwrap_or(5);
        let si = self.side_ident(s);
        log_line!(self, "|-sidestart|{si}|Safeguard");
        true
    }

    /// `addSideCondition('spikes')`: up to three layers (`onSideRestart`).
    pub(crate) fn add_spikes(&mut self, s: usize) -> bool {
        let a = self.side_extras[s];
        if a.spikes >= 3 {
            return false;
        }
        if a.spikes == 0 {
            let c = &mut self.sides[s].conds;
            c.next_added += 1;
            self.side_extras[s].spikes_added = c.next_added;
        }
        self.side_extras[s].spikes += 1;
        let si = self.side_ident(s);
        log_line!(self, "|-sidestart|{si}|Spikes");
        true
    }

    /// `addSideCondition('stickyweb')`: fails when already there.
    pub(crate) fn add_sticky_web(&mut self, s: usize) -> bool {
        if self.side_extras[s].sticky_web {
            return false;
        }
        let c = &mut self.sides[s].conds;
        c.next_added += 1;
        self.side_extras[s].sticky_web_added = c.next_added;
        self.side_extras[s].sticky_web = true;
        let si = self.side_ident(s);
        log_line!(self, "|-sidestart|{si}|move: Sticky Web");
        true
    }

    /// Defog's `onHit`: the target's evasion falls (not behind a
    /// substitute); screens, Safeguard and hazards leave the target's side
    /// and hazards the user's; the terrain ends. Only the evasion drop and
    /// the hazards count as success.
    pub(crate) fn defog(&mut self, r: MonRef, t: MonRef, mv: &Move) -> bool {
        use crate::state::SideCond;
        let mut success = false;
        if self.mon(t).vol.substitute == 0 || self.infiltrates(r) {
            let mut b = [0i8; 7];
            b[Boost::Evasion as usize] = -1;
            success = self.boost(
                t,
                &b,
                Some(r),
                crate::kinds::EffectRef::Move(mv.idx),
                false,
                false,
            );
        }
        let ri = self.ident(r);
        for c in [
            SideCond::Reflect,
            SideCond::LightScreen,
            SideCond::AuroraVeil,
        ] {
            if self.sides[t.0].conds.get(c) > 0 {
                *self.sides[t.0].conds.get_mut(c) = 0;
                let si = self.side_ident(t.0);
                log_line!(self, "|-sideend|{si}|{}", c.log_name());
            }
        }
        if self.side_extras[t.0].safeguard > 0 {
            self.side_extras[t.0].safeguard = 0;
            let si = self.side_ident(t.0);
            log_line!(self, "|-sideend|{si}|Safeguard");
        }
        for s in [t.0, r.0] {
            let si = self.side_ident(s);
            for name in ["Spikes", "Toxic Spikes", "Stealth Rock", "Sticky Web"] {
                let had = match name {
                    "Spikes" => std::mem::take(&mut self.side_extras[s].spikes) > 0,
                    "Toxic Spikes" => std::mem::take(&mut self.sides[s].conds.toxic_spikes) > 0,
                    "Stealth Rock" => std::mem::take(&mut self.sides[s].conds.stealth_rock),
                    _ => std::mem::take(&mut self.side_extras[s].sticky_web),
                };
                if had {
                    log_line!(self, "|-sideend|{si}|{name}|[from] move: Defog|[of] {ri}");
                    success = true;
                }
            }
        }
        self.clear_terrain();
        success
    }

    /// Court Change's `onHitField`: the two sides trade their screens,
    /// Tailwind, Safeguard and hazards, which go after the conditions the
    /// receiving side keeps, in the order the giving side added them.
    pub(crate) fn court_change(&mut self, r: MonRef) -> bool {
        use crate::state::SideCond;
        let held = |b: &Battle, s: usize| -> Vec<(Swapped, u16)> {
            let c = &b.sides[s].conds;
            let a = &b.side_extras[s];
            let mut v: Vec<(Swapped, u16)> = [
                SideCond::Tailwind,
                SideCond::Reflect,
                SideCond::LightScreen,
                SideCond::AuroraVeil,
            ]
            .into_iter()
            .filter(|&x| c.get(x) > 0)
            .map(|x| (Swapped::Cond(x), c.added[x as usize]))
            .collect();
            if c.toxic_spikes > 0 {
                v.push((Swapped::ToxicSpikes, c.toxic_spikes_added));
            }
            if c.stealth_rock {
                v.push((Swapped::StealthRock, c.stealth_rock_added));
            }
            if a.spikes > 0 {
                v.push((Swapped::Spikes, a.spikes_added));
            }
            if a.sticky_web {
                v.push((Swapped::StickyWeb, a.sticky_web_added));
            }
            // Safeguard has no stamp: no handler of it ever ties another.
            if a.safeguard > 0 {
                v.push((Swapped::Safeguard, 0));
            }
            v.sort_by_key(|&(_, added)| added);
            v
        };
        let moving = [held(self, 0), held(self, 1)];
        if moving[0].is_empty() && moving[1].is_empty() {
            return false;
        }
        let old = [self.sides[0].conds, self.sides[1].conds];
        let old_a = self.side_extras;
        for s in 0..2 {
            let from = 1 - s;
            let c = &mut self.sides[s].conds;
            for x in [
                SideCond::Tailwind,
                SideCond::Reflect,
                SideCond::LightScreen,
                SideCond::AuroraVeil,
            ] {
                *c.get_mut(x) = old[from].get(x);
            }
            c.toxic_spikes = old[from].toxic_spikes;
            c.stealth_rock = old[from].stealth_rock;
            let a = &mut self.side_extras[s];
            a.safeguard = old_a[from].safeguard;
            a.spikes = old_a[from].spikes;
            a.sticky_web = old_a[from].sticky_web;
            for &(x, _) in &moving[from] {
                c.next_added += 1;
                let n = c.next_added;
                match x {
                    Swapped::Cond(sc) => c.added[sc as usize] = n,
                    Swapped::ToxicSpikes => c.toxic_spikes_added = n,
                    Swapped::StealthRock => c.stealth_rock_added = n,
                    Swapped::Spikes => a.spikes_added = n,
                    Swapped::StickyWeb => a.sticky_web_added = n,
                    Swapped::Safeguard => {}
                }
            }
        }
        self.add("|-swapsideconditions");
        let ri = self.ident(r);
        log_line!(self, "|-activate|{ri}|move: Court Change");
        true
    }

    /// Heal Bell's `onHit`: the user's whole team (bench too) is cured. A
    /// Soundproof or Good as Gold ally on the field is immune, unless the
    /// user's move ignores abilities.
    pub(crate) fn heal_bell(&mut self, r: MonRef) -> bool {
        let ri = self.ident(r);
        log_line!(self, "|-activate|{ri}|move: Heal Bell");
        let mut success = false;
        for i in 0..self.sides[r.0].mons.len() {
            let ally = (r.0, i);
            if ally != r
                && !self.suppressing_ability(ally)
                && self.mon(ally).is_active
                && !self.mon(ally).revived
            {
                let name = match self.ability_kind(ally) {
                    AbilityKind::Soundproof => Some("Soundproof"),
                    AbilityKind::GoodAsGold => Some("Good as Gold"),
                    _ => None,
                };
                if let Some(name) = name {
                    let ai = self.ident(ally);
                    log_line!(self, "|-immune|{ai}|[from] ability: {name}");
                    continue;
                }
            }
            if self.mon(ally).hp > 0 && self.mon(ally).status != Status::None {
                self.cure_status(ally);
                success = true;
            }
        }
        success
    }

    /// Screen Cleaner's `onStart`: removes Reflect, Light Screen and Aurora
    /// Veil from its own side, then the foes' (one `-activate` for all).
    pub(crate) fn screen_cleaner(&mut self, r: MonRef) {
        use crate::state::SideCond;
        let mut activated = false;
        for c in [
            SideCond::Reflect,
            SideCond::LightScreen,
            SideCond::AuroraVeil,
        ] {
            for s in [r.0, 1 - r.0] {
                if self.sides[s].conds.get(c) > 0 {
                    if !activated {
                        let ri = self.ident(r);
                        log_line!(self, "|-activate|{ri}|ability: Screen Cleaner");
                        activated = true;
                    }
                    *self.sides[s].conds.get_mut(c) = 0;
                    let si = self.side_ident(s);
                    log_line!(self, "|-sideend|{si}|{}", c.log_name());
                }
            }
        }
    }

    /// Teatime's `onHitField`: every active Pokémon holding a berry eats it
    /// (forced). Pokémon the move misses (semi-invulnerable) or that block
    /// it (Good as Gold) are skipped.
    pub(crate) fn teatime(&mut self, r: MonRef) -> bool {
        let Some(id) = self.dex.ids.moves.teatime else {
            return true;
        };
        let mv = self.dex.mv(id).clone();
        let ri = self.ident(r);
        let mut targets = Vec::new();
        for p in self.all_active() {
            if self.invulnerable(r, p, &mv.id) {
                let pi = self.ident(p);
                log_line!(self, "|-miss|{ri}|{pi}");
            } else if !self.ability_try_hit(r, p, &mv, mv.type_)
                && self.mon(p).item.is_some_and(|i| self.dex.item(i).is_berry)
            {
                targets.push(p);
            }
        }
        log_line!(self, "|-fieldactivate|move: Teatime");
        if targets.is_empty() {
            // NOT_FAIL: no further failure line, and no result.
            self.active_move.field_not_fail = true;
            log_line!(self, "|-fail|{ri}|move: Teatime");
            if let Some(line) = self.last_move_line {
                self.attr_move_line(line, "[still]");
            }
            return true;
        }
        // The event's target is the first of the move's targets: the first
        // living Pokémon on the user's side (`alliesAndSelf`).
        let first = (0..self.sides[r.0].n_active)
            .filter_map(|sl| self.active(r.0, sl))
            .find(|&m| self.mon(m).hp > 0)
            .unwrap_or(r);
        for p in targets {
            self.eat_item_forced_from(p, first);
        }
        true
    }

    /// Tidy Up's `onHit`: substitutes and hazards go, then +1 Atk and Spe.
    /// It fails only if nothing was cleared and neither stat could rise.
    pub(crate) fn tidy_up(&mut self, r: MonRef, mv: &Move) -> bool {
        let mut success = false;
        for a in self.all_active() {
            if self.mon(a).vol.substitute > 0 {
                self.mon_mut(a).vol.substitute = 0;
                let ai = self.ident(a);
                log_line!(self, "|-end|{ai}|Substitute");
                success = true;
            }
        }
        for s in [r.0, 1 - r.0] {
            let si = self.side_ident(s);
            for name in ["Spikes", "Toxic Spikes", "Stealth Rock", "Sticky Web"] {
                let had = match name {
                    "Spikes" => std::mem::take(&mut self.side_extras[s].spikes) > 0,
                    "Toxic Spikes" => std::mem::take(&mut self.sides[s].conds.toxic_spikes) > 0,
                    "Stealth Rock" => std::mem::take(&mut self.sides[s].conds.stealth_rock),
                    _ => std::mem::take(&mut self.side_extras[s].sticky_web),
                };
                if had {
                    log_line!(self, "|-sideend|{si}|{name}");
                    success = true;
                }
            }
        }
        if success {
            let ri = self.ident(r);
            log_line!(self, "|-activate|{ri}|move: Tidy Up");
        }
        let mut b = [0i8; 7];
        b[Boost::Atk as usize] = 1;
        b[Boost::Spe as usize] = 1;
        let effect = crate::kinds::EffectRef::Move(mv.idx);
        let boosted = self.boost(r, &b, Some(r), effect, false, true);
        if boosted || success {
            true
        } else {
            self.fail_move(r);
            false
        }
    }

    /// Mortal Spin's `onAfterHit`: frees the user of Leech Seed, hazards on
    /// its side and partial trapping.
    pub(crate) fn mortal_spin_clear(&mut self, r: MonRef, name: &str) {
        let ri = self.ident(r);
        if self.mon_mut(r).vol.leech_seed.take().is_some() {
            log_line!(self, "|-end|{ri}|Leech Seed|[from] move: {name}|[of] {ri}");
        }
        let s = r.0;
        let si = self.side_ident(s);
        for cond in ["Spikes", "Toxic Spikes", "Stealth Rock", "Sticky Web"] {
            let had = match cond {
                "Spikes" => std::mem::take(&mut self.side_extras[s].spikes) > 0,
                "Toxic Spikes" => std::mem::take(&mut self.sides[s].conds.toxic_spikes) > 0,
                "Stealth Rock" => std::mem::take(&mut self.sides[s].conds.stealth_rock),
                _ => std::mem::take(&mut self.side_extras[s].sticky_web),
            };
            if had {
                log_line!(self, "|-sideend|{si}|{cond}|[from] move: {name}|[of] {ri}");
            }
        }
        if self.mon(r).vol.partial_trap > 0 {
            let trap = self
                .mon(r)
                .vol
                .partial_trap_move
                .map(|id| self.dex.mv(id).name.clone())
                .unwrap_or_default();
            self.clear_partial_trap(r);
            log_line!(self, "|-end|{ri}|{trap}|[partiallytrapped]");
        }
    }

    /// Ceaseless Edge's `onAfterHit` (and `onAfterSubDamage`, while its
    /// user stands): Spikes on the foe's side, silently at three layers.
    pub(crate) fn ceaseless_edge_on_after_hit(
        &mut self,
        r: MonRef,
        kind: MoveKind,
        sheer_force: bool,
    ) {
        if kind == MoveKind::CeaselessEdge && !sheer_force {
            self.add_spikes(1 - r.0);
        }
        if matches!(kind, MoveKind::MortalSpin | MoveKind::RapidSpin)
            && !sheer_force
            && self.mon(r).hp > 0
        {
            self.mortal_spin_clear(
                r,
                if kind == MoveKind::MortalSpin {
                    "Mortal Spin"
                } else {
                    "Rapid Spin"
                },
            );
        }
    }
}
