//! Showdown's fixed-point arithmetic and damage formula.
//!
//! Ported from `sim/battle.ts` (`modify`, `chainModify`, `randomizer`) and
//! `sim/battle-actions.ts` / `data/mods/champions/scripts.ts`
//! (`getDamage`, `modifyDamage`).

use crate::battle::Small;
use arrayvec::ArrayVec;

use crate::battle::{Battle, MonRef};
use crate::dex::events::Event;
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::AbilityKind;
use crate::kinds::MoveKind;
use crate::state::*;

/// Showdown's `trunc`: truncate to an unsigned 32-bit integer.
#[inline]
pub fn trunc(x: f64) -> u32 {
    if x <= 0.0 {
        0
    } else {
        (x as u64 & 0xffff_ffff) as u32
    }
}

/// `battle.modify(value, numerator, denominator)`: apply a ratio with
/// Showdown's 4096-based rounding (round half down).
#[inline]
pub fn modify(value: u32, numerator: u32, denominator: u32) -> u32 {
    let modifier = numerator as u64 * 4096 / denominator as u64;
    (((value as u64 * modifier) + 2048 - 1) / 4096) as u32
}

/// `modify` with a 4096-based modifier already computed (as from a chain).
#[inline]
pub fn modify_4096(value: u32, modifier: u32) -> u32 {
    (((value as u64 * modifier as u64) + 2048 - 1) / 4096) as u32
}

/// A chain of modifiers combined like `chainModify`: each step multiplies
/// the 4096-based running modifier and rounds to nearest.
#[derive(Clone, Copy, Debug)]
pub struct Chain(pub u32);

impl Chain {
    pub const ONE: Chain = Chain(4096);

    pub fn then(self, numerator: u32, denominator: u32) -> Chain {
        let next = numerator as u64 * 4096 / denominator as u64;
        Chain(((self.0 as u64 * next + 2048) >> 12) as u32)
    }

    pub fn apply(self, value: u32) -> u32 {
        if self.0 == 4096 {
            value
        } else {
            modify_4096(value, self.0)
        }
    }
}

/// Stat stage multipliers: `floor(stat * table[boost])` or `floor(stat / table[-boost])`.
pub fn apply_boost(stat: u32, boost: i8) -> u32 {
    let b = boost.clamp(-6, 6);
    // table = [1, 1.5, 2, 2.5, 3, 3.5, 4] as fractions of 2
    let num = 2 + b.unsigned_abs() as u32;
    if b >= 0 {
        stat * num / 2
    } else {
        stat * 2 / num
    }
}

/// Base damage before modifiers: int(int(int(2L/5+2) * P * A / D) / 50).
pub fn base_damage(level: u32, base_power: u32, attack: u32, defense: u32) -> u32 {
    let l = 2 * level / 5 + 2;
    let a = l as u64 * base_power as u64 * attack as u64;
    let b = a / defense.max(1) as u64;
    (b / 50) as u32
}

/// The chance of a critical hit at each stage (0 to 4; higher stages are 4).
pub const CRIT_ODDS: [(u32, u32); 5] = [(0, 1), (1, 24), (1, 8), (1, 2), (1, 1)];

/// Inputs to `modifyDamage` that the caller decides (Champions version).
#[derive(Clone, Copy, Debug)]
pub struct DamageMods {
    pub spread: bool,
    /// The second hit of a Parental Bond move (x1/4, unless spread).
    pub parental_bond: bool,
    /// Weather multiplier as a fraction (1/1 when no weather effect).
    pub weather: (u32, u32),
    pub crit: bool,
    /// Random roll in 0..16 (damage factor 100 - roll).
    pub roll: u32,
    /// STAB modifier as a fraction (3/2, or 2/1 with Adaptability), if any.
    pub stab: Option<(u32, u32)>,
    /// Type effectiveness exponent, clamped to -6..=6.
    pub type_mod: i32,
    pub burned_physical: bool,
    /// Final `ModifyDamage` chain (Life Orb, screens...).
    pub final_mod: Chain,
    /// A hit through Protect (Piercing Drill, Unseen Fist) does a quarter.
    pub bypass_protect: bool,
}

/// `modifyDamage` from `data/mods/champions/scripts.ts`.
pub fn modify_damage(base: u32, m: &DamageMods) -> u32 {
    let mut d = base + 2;
    if m.spread {
        d = modify(d, 3, 4);
    } else if m.parental_bond {
        d = modify(d, 1, 4);
    }
    if m.weather != (1, 1) {
        d = modify(d, m.weather.0, m.weather.1);
    }
    if m.crit {
        d = trunc(d as f64 * 1.5);
    }
    // randomizer: trunc(trunc(d * (100 - random(16))) / 100)
    d = d * (100 - m.roll) / 100;
    if let Some((n, den)) = m.stab {
        d = modify(d, n, den);
    }
    if m.type_mod > 0 {
        for _ in 0..m.type_mod {
            d *= 2;
        }
    } else if m.type_mod < 0 {
        for _ in 0..(-m.type_mod) {
            d /= 2;
        }
    }
    if m.burned_physical {
        d = modify(d, 1, 2);
    }
    d = m.final_mod.apply(d);
    if m.bypass_protect {
        d = modify(d, 1, 4);
    }
    if d == 0 {
        return 1;
    }
    d & 0xffff
}

impl Battle {
    /// `getDamage` for one target of a damaging move.
    // Each argument is a distinct input of Showdown's `getDamage`; a struct used
    // by this one caller would only rename them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn move_damage(
        &mut self,
        r: MonRef,
        t: MonRef,
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        type_changed: bool,
        spread: bool,
    ) -> u32 {
        if kind == MoveKind::FinalGambit && mv.ohko.is_none() && mv.fixed_damage.is_none() {
            return self.final_gambit_damage_callback(r);
        }
        if let Some(d) = self.fixed_damage(r, t, mv, kind) {
            return d;
        }
        // Testing the read-only calculation: its distribution for this very
        // hit, from a copy of the state before any draw.
        let predicted = self.damage_samples.is_some().then(|| {
            let samples = self.damage_samples.take();
            let mut copy = self.clone();
            self.damage_samples = samples;
            let hp = self.mon(t).hp as u32;
            let (crit_chance, rolls) =
                copy.hit_rolls(r, t, mv, kind, move_type, type_changed, spread);
            // The full calculation rebuilds the move: first hits only.
            let full = (self.move_hit <= 1 && !self.active_move.pbond).then(|| {
                let c = copy.calc(r, mv.idx, t);
                let ko = (c.hits.len() == 1 && c.accuracy > 0.0).then(|| c.ko / c.accuracy);
                (c.spread, c.hits.first().copied().unwrap_or_default(), ko)
            });
            (crit_chance, rolls, hp, full)
        });
        let d = self.get_damage(r, t, mv, kind, move_type, type_changed, spread);
        if let Some((crit_chance, rolls, hp, full)) = predicted {
            let crit = self.active_move.crit.contains(&t);
            if let Some(v) = self.damage_samples.as_mut() {
                v.push(crate::calc::DamageSample {
                    attacker: r,
                    target: t,
                    move_id: mv.idx,
                    hit: self.move_hit,
                    spread,
                    crit_chance,
                    rolls,
                    target_hp: hp,
                    damage: d,
                    crit,
                    full: full.map(|(s, h, _)| (s, h)),
                    full_ko: full.and_then(|(_, _, k)| k),
                    hp_after: None,
                });
            }
        }
        d
    }

    /// The damage of a move that doesn't use the formula, as `getDamage`
    /// returns it: a one-hit KO's (the target's max HP), fixed damage (Night
    /// Shade, Seismic Toss), half the target's HP (Super Fang), the user's
    /// HP (Final Gambit), or a `damageCallback` (Counter, Endeavor...).
    pub(crate) fn fixed_damage(
        &self,
        r: MonRef,
        t: MonRef,
        mv: &Move,
        kind: MoveKind,
    ) -> Option<u32> {
        match (mv.fixed_damage, kind) {
            // getDamage returns the target's max HP before any roll.
            _ if mv.ohko.is_some() => Some(self.mon(t).maxhp as u32),
            (Some(fixed), _) => Some(fixed.unwrap_or(self.mon(r).level as u32)),
            (None, MoveKind::HalfHp) => Some((self.mon(t).hp as u32 / 2).max(1)),
            (None, MoveKind::FinalGambit) => Some(self.mon(r).hp as u32),
            (None, k) => self.damage_callback(r, t, k),
        }
    }

    /// The move's base power after `basePowerCallback` and `onModifyMove`
    /// (Weather Ball, Terrain Pulse, weight and HP based moves...).
    fn base_power_callback(&self, src: MonRef, tgt: MonRef, mv: &Move, kind: MoveKind) -> u32 {
        let bp = mv.base_power as u32;
        let weight = |r: MonRef| self.get_weight(r);
        let speed = |r: MonRef| self.get_stat(r, Stat::Spe, true, true);
        match kind {
            MoveKind::WeatherBall if self.move_weather().is_some() => bp * 2,
            MoveKind::TerrainPulse if self.field.terrain.is_some() && self.is_grounded(src) => {
                bp * 2
            }
            MoveKind::WeightPower => match weight(tgt) {
                w if w >= 2000 => 120,
                w if w >= 1000 => 100,
                w if w >= 500 => 80,
                w if w >= 250 => 60,
                w if w >= 100 => 40,
                _ => 20,
            },
            MoveKind::WeightRatioPower => {
                let (user, target) = (weight(src), weight(tgt));
                match user {
                    u if u >= target * 5 => 120,
                    u if u >= target * 4 => 100,
                    u if u >= target * 3 => 80,
                    u if u >= target * 2 => 60,
                    _ => 40,
                }
            }
            MoveKind::LastRespects => 50 + 50 * self.sides[src.0].total_fainted as u32,
            MoveKind::HpPower => {
                let m = self.mon(src);
                bp * m.hp as u32 / m.maxhp as u32
            }
            MoveKind::Acrobatics if self.mon(src).item.is_none() => bp * 2,
            MoveKind::FailurePower if self.mon(src).move_last_turn_result == Some(false) => bp * 2,
            // basePowerCallback: 20 times the hit.
            MoveKind::TripleAxel => 20 * self.move_hit.max(1) as u32,
            MoveKind::HardPress => {
                let t = self.mon(tgt);
                let x = t.hp as u64 * 4096 / t.maxhp.max(1) as u64;
                let y = (100 * (100 * x) + 2047) / 4096;
                ((y / 100) as u32).max(1)
            }
            MoveKind::PowerTrip => {
                let up: u32 = self
                    .mon(src)
                    .boosts
                    .iter()
                    .filter(|&&b| b > 0)
                    .map(|&b| b as u32)
                    .sum();
                bp + 20 * up
            }
            MoveKind::RageFist => (50 + 50 * self.mon(src).times_attacked as u32).min(350),
            MoveKind::SpitUp => self.spit_up_base_power_callback(src),
            // basePowerCallback of the speed, HP, status and turn-order moves.
            MoveKind::ElectroBall => {
                let t = speed(tgt);
                let ratio = speed(src).checked_div(t).unwrap_or(0);
                [40, 60, 80, 120, 150][ratio.min(4) as usize]
            }
            MoveKind::GyroBall => (25 * speed(tgt))
                .checked_div(speed(src))
                .map_or(1, |q| (q + 1).min(150)),
            MoveKind::LowHpPower => {
                let m = self.mon(src);
                match (m.hp as u32 * 48 / m.maxhp as u32).max(1) {
                    r if r < 2 => 200,
                    r if r < 5 => 150,
                    r if r < 10 => 100,
                    r if r < 17 => 80,
                    r if r < 33 => 40,
                    _ => 20,
                }
            }
            // newlySwitched (in since the last end of turn) or willMove.
            MoveKind::Payback if self.mon(tgt).active_turns == 0 || self.has_queued_move(tgt) => bp,
            MoveKind::Payback => bp * 2,
            MoveKind::WaterShuriken
                if self.dex.species(self.mon(src).species).name == "Greninja-Ash"
                    && self.ability_id(src) == "battlebond" =>
            {
                bp + 5
            }
            MoveKind::RisingVoltage
                if self.field.terrain == Some(Terrain::Electric) && self.is_grounded(tgt) =>
            {
                bp * 2
            }
            MoveKind::Assurance if self.mon(tgt).hurt_this_turn.is_some_and(|h| h > 0) => bp * 2,
            MoveKind::StatusPower if self.mon(tgt).status != Status::None => bp * 2,
            // basePowerCallback shifts the next ally off `move.allies`.
            MoveKind::BeatUp => self
                .active_move
                .beat_up
                .get((self.move_hit as usize).max(1) - 1)
                .copied()
                .unwrap_or(bp),
            MoveKind::Round if self.active_move.round_doubled => bp * 2,
            // The target hit the user this turn.
            MoveKind::Avalanche
                if self.mon(src).attacked_by.iter().any(|a| {
                    a.damage > 0
                        && a.this_turn
                        && a.side as usize == tgt.0
                        && a.name == self.mon(tgt).name
                }) =>
            {
                bp * 2
            }
            _ => bp,
        }
    }

    /// The move's own `onBasePower` (priority 0): Expanding Force, Knock Off,
    /// Solar Beam, Fickle Beam, Facade and the others.
    pub(crate) fn move_base_power_mod(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        kind: MoveKind,
    ) -> Option<(u32, u32)> {
        match kind {
            MoveKind::ExpandingForce
                if self.field.terrain == Some(Terrain::Psychic) && self.is_grounded(src) =>
            {
                Some((3, 2))
            }
            MoveKind::KnockOff if self.item_removable(tgt) => Some((3, 2)),
            MoveKind::Charge(crate::kinds::ChargeKind::Solar)
                if matches!(
                    self.move_weather(),
                    Some(Weather::Rain | Weather::Sand | Weather::Snow)
                ) =>
            {
                Some((1, 2))
            }
            // Grav Apple under Gravity.
            MoveKind::GravApple => (self.field.gravity > 0).then_some((3, 2)),
            // Fickle Beam: randomChance(3, 10), then its `-activate` and an
            // `[anim]` on the move line.
            // A read-only calculation takes the usual power (see `crate::calc`).
            MoveKind::FickleBeam if self.probe => None,
            MoveKind::FickleBeam => {
                if !self.rng.chance(3, 10) {
                    return None;
                }
                if let Some(line) = self.last_move_line {
                    self.attr_move_line(line, "[anim] Fickle Beam All Out");
                }
                let ident = self.ident(src);
                log_line!(self, "|-activate|{ident}|move: Fickle Beam");
                Some((2, 1))
            }
            MoveKind::PoisonPower => {
                matches!(self.mon(tgt).status, Status::Psn | Status::Tox).then_some((2, 1))
            }
            MoveKind::Facade => {
                (!matches!(self.mon(src).status, Status::None | Status::Slp)).then_some((2, 1))
            }
            MoveKind::LashOut => self.mon(src).stats_lowered_this_turn.then_some((2, 1)),
            MoveKind::MistyExplosion => (self.field.terrain == Some(Terrain::Misty)
                && self.is_grounded(src))
            .then_some((3, 2)),
            _ => None,
        }
    }

    /// Showdown `getDamage` + Champions `modifyDamage`: the critical-hit
    /// roll, then the damage before the random roll ([`Battle::damage_base`]),
    /// the roll, and the modifiers after it ([`Battle::damage_mods`]). The
    /// read-only calculation (`crate::calc`) runs the same two phases for
    /// each roll and both crit outcomes.
    #[allow(clippy::too_many_arguments)]
    fn get_damage(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        type_changed: bool,
        spread: bool,
    ) -> u32 {
        self.base_power_callback_events(src, tgt, kind);
        let ratio = self.run_modify_crit_ratio(src, tgt, mv.crit_ratio);
        // A certain crit by stage still rolls: randomChance(1, 1) draws.
        let mut crit = mv.will_crit
            || match ratio.clamp(0, 4) {
                0 => false,
                r => {
                    let (n, d) = CRIT_ODDS[r as usize];
                    self.rng.chance(n, d)
                }
            };
        if crit && self.crit_prevented(src, tgt, mv) {
            crit = false;
        }
        self.note_hit_crit(tgt, crit);
        let base = self.damage_base(src, tgt, mv, kind, move_type, type_changed, crit);
        let roll = self.rng.random_damage(16);
        let mods = self.damage_mods(src, tgt, mv, kind, move_type, crit, spread, roll);
        modify_damage(base, &mods)
    }

    /// The target's Battle Armor (or the like) or an intact Disguise stops a
    /// critical hit.
    pub(crate) fn crit_prevented(&self, src: MonRef, tgt: MonRef, mv: &Move) -> bool {
        self.crit_blocked(tgt) || self.disguise_shields(src, tgt, mv)
    }

    /// `getDamage` up to the random roll: the base power after its
    /// callbacks and the `BasePower` event, the attack and defense after
    /// boosts and modifiers, and the base damage.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn damage_base(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        type_changed: bool,
        crit: bool,
    ) -> u32 {
        let bp = self.base_power_callback(src, tgt, mv, kind).max(1);
        let bp = self
            .run_base_power(src, tgt, mv, kind, move_type, type_changed, bp)
            .apply(bp)
            .max(1);

        let physical = mv.category == Category::Physical;
        // Body Press attacks with Def, Psyshock hits Def, Foul Play uses the target's Atk.
        let atk_stat =
            mv.override_offensive_stat
                .unwrap_or(if physical { Stat::Atk } else { Stat::Spa });
        let def_stat =
            mv.override_defensive_stat
                .unwrap_or(if physical { Stat::Def } else { Stat::Spd });
        // calculateStat's ModifyBoost on the user, then on the target.
        self.event_sort(Event::ModifyBoost, src, None, &[]);
        self.event_sort(Event::ModifyBoost, tgt, None, &[]);
        // runEvent('ModifyAtk' / 'ModifySpA', user, target), then
        // ('ModifyDef' / 'ModifySpD', target, user): their sorts' draws
        // (Flash Fire's volatile joins the first).
        {
            use crate::abilities::ModHandler;
            let (event, priority_key) = if physical {
                (Event::ModifyAtk, "onModifyAtkPriority")
            } else {
                (Event::ModifySpA, "onModifySpAPriority")
            };
            let mut extra = ArrayVec::<ModHandler, 1>::new();
            if self.mon(src).vol.flash_fire {
                let p = self.dex.cond_num("flashfire", priority_key).unwrap_or(0.0) as i32;
                extra.push(ModHandler::new(p, self.mon(src).speed, 2));
            }
            self.event_sort(event, src, Some(tgt), &extra);
            let event = if def_stat == Stat::Def {
                Event::ModifyDef
            } else {
                Event::ModifySpD
            };
            self.event_sort(event, tgt, Some(src), &[]);
        }
        let a = if mv.offensive_pokemon_is_target {
            self.mon(tgt)
        } else {
            self.mon(src)
        };
        let d = self.mon(tgt);
        let mut atk_boost = a.boosts[Boost::of_stat(atk_stat) as usize];
        let mut def_boost = d.boosts[Boost::of_stat(def_stat) as usize];
        if crit {
            atk_boost = atk_boost.max(0);
            def_boost = def_boost.min(0);
        }
        if mv.ignore_defensive {
            def_boost = 0;
        }
        let (atk_boost, def_boost) =
            self.damage_stages_after_unaware(src, tgt, atk_boost, def_boost);
        let level = self.mon(src).level as u32;
        let attack = apply_boost(
            a.stats[self.wonder_room_stat(atk_stat) as usize] as u32,
            atk_boost,
        );
        let attack = self.modify_attack(src, tgt, mv, move_type, attack);
        let defense = apply_boost(
            d.stats[self.wonder_room_stat(def_stat) as usize] as u32,
            def_boost,
        );
        let defense = self.modify_defense(tgt, def_stat, defense);
        base_damage(level, bp, attack, defense)
    }

    /// `getDamage` after the random roll: the type effectiveness (and its
    /// message), the critical hit's message, and `modifyDamage`'s inputs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn damage_mods(
        &mut self,
        src: MonRef,
        tgt: MonRef,
        mv: &Move,
        kind: MoveKind,
        move_type: TypeId,
        crit: bool,
        spread: bool,
        roll: u32,
    ) -> DamageMods {
        let physical = mv.category == Category::Physical;
        let type_mod = {
            let t = self.types_of(tgt);
            let water = self.dex.ids.types.water;
            let eff = |ty: TypeId| {
                if kind == MoveKind::FreezeDry && Some(ty) == water {
                    1
                } else if kind == MoveKind::FlyingPress {
                    // onEffectiveness: Flying's effectiveness is added.
                    let flying = self
                        .dex
                        .ids
                        .types
                        .flying
                        .map_or(0, |f| self.dex.effectiveness_exp(f, ty));
                    self.dex.effectiveness_exp(move_type, ty) + flying
                } else {
                    self.dex.effectiveness_exp(move_type, ty)
                }
            };
            t.iter()
                .filter(|&&x| x != NO_TYPE)
                .map(|&x| eff(x))
                .sum::<i32>()
                .clamp(-6, 6)
        };
        let type_mod = self.effectiveness_handlers(tgt, move_type, type_mod);
        let type_mod = if self.disguise_shields(src, tgt, mv) {
            0
        } else {
            type_mod
        };
        if self.log_enabled {
            let tid = self.ident(tgt);
            // Champions announces 4x and 0.25x with a count.
            if type_mod > 0 {
                log_line!(self, "|-supereffective|{tid}|{}", type_mod.min(2));
            } else if type_mod < 0 {
                log_line!(self, "|-resisted|{tid}|{}", (-type_mod).min(2));
            }
            if crit {
                log_line!(self, "|-crit|{tid}");
            }
        }
        let guts = self.has_ability(src, crate::kinds::AbilityKind::Guts);
        let final_mod = self.modify_damage_chain(src, tgt, mv, crit, type_mod, move_type);
        let bypass_protect = self.hit_protect_modify_damage(src, tgt);
        DamageMods {
            bypass_protect,
            spread,
            parental_bond: self.active_move.pbond && self.move_hit > 1,
            weather: self.weather_damage_mod(move_type),
            crit,
            roll,
            stab: self.stab_modifier(src, move_type),
            type_mod,
            burned_physical: physical
                && self.mon(src).status == Status::Brn
                && !guts
                && kind != MoveKind::Facade,
            final_mod,
        }
    }

    /// `spreadDamage` for damage from a move (including confusion self-hits):
    /// the `Damage` event (Sturdy, then Focus Sash and Focus Band) can lower it first.
    pub(crate) fn apply_move_damage(&mut self, t: MonRef, amount: u32, from: &str) -> u32 {
        if self.mon(t).hp == 0 || amount == 0 || self.mon(t).revived {
            return 0;
        }
        self.damage_event_sort(t);
        // Damage event, by priority: Disguise (1), Endure (-10), Sturdy (-30),
        // then the item (Focus Sash -40).
        if self.disguise_damage(t, from) {
            return 0;
        }
        // Berserk's onDamage (Champions): damage from a single-hit move (a
        // confusion self-hit included) makes healing berries wait.
        if self.has_ability(t, AbilityKind::Berserk) {
            let multihit = from.is_empty() && self.active_move.multihit;
            self.mon_mut(t).ability_state.checked_berserk = Some(multihit);
        }
        // Endure (-10): the Pokémon holds on at 1 HP.
        let hp = self.mon(t).hp as u32;
        let amount = amount.max(1);
        let amount = if self.mon(t).vol.endure && amount >= hp {
            let ti = self.ident(t);
            log_line!(self, "|-activate|{ti}|move: Endure");
            hp - 1
        } else {
            amount
        };
        // A handler that returns 0 (Endure at 1 HP) ends the event.
        let amount = if amount == 0 {
            0
        } else {
            self.ability_damage_event(t, amount)
        };
        let amount = if amount == 0 {
            0
        } else {
            self.item_damage_event(t, amount)
        };
        if amount == 0 {
            // Focus Band at 1 HP: no HP lost, but the damage line still shows.
            let ident = self.ident(t);
            self.add_health(format_args!("|-damage|{ident}"), t, from);
            return 0;
        }
        let dealt = self.direct_damage(t, amount, from);
        self.set_hurt_this_turn(t, dealt);
        dealt
    }

    /// Showdown `spreadDamage` for one target, after a move's: the `Damage`
    /// event, then the damage. Returns HP actually removed.
    pub(crate) fn apply_damage(&mut self, t: MonRef, amount: u32, from: &str) -> u32 {
        // spreadDamage skips a Pokémon that isn't active (revived in its slot).
        if self.mon(t).hp == 0 || amount == 0 || self.mon(t).revived {
            return 0;
        }
        self.damage_event_sort(t);
        // Poison Heal's onDamage (priority 1): poison or toxic damage heals
        // an eighth of the max HP instead, and the damage is cancelled.
        if self.has_ability(t, AbilityKind::PoisonHeal) && from == "|[from] psn" {
            let amount = (self.mon(t).maxhp as u32 / 8).max(1);
            self.heal(t, amount, "|[from] ability: Poison Heal");
            return 0;
        }
        // Gluttony's onDamage: any damage sets its state (a hazard can
        // knock the Pokémon out before its onStart runs).
        if self.has_ability(t, crate::kinds::AbilityKind::Gluttony) {
            self.mon_mut(t).ability_state.gluttony = Some(true);
        }
        if self.magic_guard_blocks(t, from) {
            return 0;
        }
        // Berserk's onDamage: other damage leaves the berries free.
        if self.has_ability(t, AbilityKind::Berserk) {
            self.mon_mut(t).ability_state.checked_berserk = Some(true);
        }
        // Focus Band rolls for any damage, though only a move's can be survived.
        if self.item_kind(t) == Some(crate::kinds::ItemKind::FocusBand) {
            let _ = self.rng.chance(1, 10);
        }
        let dealt = self.direct_damage(t, amount, from);
        self.set_hurt_this_turn(t, dealt);
        dealt
    }

    /// Magic Guard's `onDamage`: no damage but a move's. Damage from an
    /// ability is announced on the ability's holder (`[of]`).
    pub(crate) fn magic_guard_blocks(&mut self, t: MonRef, from: &str) -> bool {
        if !self.has_ability(t, AbilityKind::MagicGuard) {
            return false;
        }
        if let Some(rest) = from.split("[from] ability: ").nth(1) {
            let name = rest.split('|').next().unwrap_or("");
            if let Some(of) = from.split("[of] ").nth(1) {
                let of = of.split('|').next().unwrap_or("");
                log_line!(self, "|-activate|{of}|ability: {name}");
            }
        }
        true
    }

    /// The `Damage` event's handler sort (`runEvent('Damage')`): the
    /// target's Disguise (1), Poison Heal (1), Sturdy (-30), Rock Head,
    /// Gluttony, Magic Guard, Berserk and Heatproof (0), Endure (-10) and
    /// Focus Sash or Band (-40), and
    /// every active Pokémon's Damp (`onAnyDamage`, 0). Equal ones draw; their
    /// effects are applied where they act.
    pub(super) fn damage_event_sort(&mut self, t: MonRef) {
        use crate::abilities::{compare_mod, ModHandler};
        use crate::kinds::{AbilityKind, ItemKind};
        let speed = self.mon(t).speed;
        let mut hs: Small<ModHandler> = Small::new();
        // The ability's handler (one at most): by the raw ability, except
        // Poison Heal's.
        match self.raw_ability_kind(t) {
            AbilityKind::Sturdy => hs.push(ModHandler::new(-30, speed, 7)),
            AbilityKind::RockHead
            | AbilityKind::Gluttony
            | AbilityKind::MagicGuard
            | AbilityKind::Berserk
            | AbilityKind::Heatproof => hs.push(ModHandler::new(0, speed, 7)),
            AbilityKind::Disguise => hs.push(ModHandler::new(1, speed, 7)),
            _ if self.has_ability(t, AbilityKind::PoisonHeal) => {
                hs.push(ModHandler::new(1, speed, 7))
            }
            _ => {}
        }
        if matches!(
            self.item_kind(t),
            Some(ItemKind::FocusSash | ItemKind::FocusBand)
        ) {
            hs.push(ModHandler::new(-40, speed, 8));
        }
        if self.mon(t).vol.endure {
            hs.push(ModHandler::new(-10, speed, 2));
        }
        for p in self.all_active() {
            if self.mon(p).hp > 0 && self.has_ability(p, AbilityKind::Damp) {
                hs.push(ModHandler::new(0, self.mon(p).speed, 7));
            }
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, compare_mod);
        }
    }

    /// Showdown `directDamage` (Clangorous Soul's cost): no `Damage` event.
    pub(crate) fn direct_damage(&mut self, t: MonRef, amount: u32, from: &str) -> u32 {
        let m = self.mon_mut(t);
        if m.hp == 0 || amount == 0 {
            return 0;
        }
        let dealt = amount.max(1).min(m.hp as u32);
        m.hp -= dealt as u16;
        let fainted = m.hp == 0;
        let ident = self.ident(t);
        self.add_health(format_args!("|-damage|{ident}"), t, from);
        if fainted {
            self.queue_faint(t);
        }
        dealt
    }

    /// Showdown `heal`. Returns HP restored.
    /// Showdown `battle.heal`: the `TryHeal` event (Heal Block stops it),
    /// then the heal.
    pub(crate) fn heal(&mut self, t: MonRef, amount: u32, from: &str) -> u32 {
        let m = self.mon_mut(t);
        if m.hp == 0 || m.hp >= m.maxhp || amount == 0 || m.vol.heal_block > 0 || m.revived {
            return 0;
        }
        let healed = amount.max(1).min((m.maxhp - m.hp) as u32);
        m.hp += healed as u16;
        let ident = self.ident(t);
        self.add_health(format_args!("|-heal|{ident}"), t, from);
        healed
    }

    /// Burn damage after Heatproof's `onDamage` (halved, floored, at least 1).
    pub(crate) fn burn_modify_damage(&self, r: MonRef, amount: u32) -> u32 {
        if self.has_ability(r, AbilityKind::Heatproof) {
            (amount / 2).max(1)
        } else {
            amount
        }
    }

    /// The target's `Effectiveness` handlers: a Flying-type Iron Ball holder
    /// takes Ground moves neutrally (each of its types returns 0).
    pub(crate) fn effectiveness_handlers(
        &self,
        tgt: MonRef,
        move_type: TypeId,
        type_mod: i32,
    ) -> i32 {
        let ground = self.dex.ids.types.ground == Some(move_type);
        let flying = self
            .dex
            .type_id("Flying")
            .is_some_and(|f| self.has_type_of(tgt, f));
        if ground && flying && self.item_kind(tgt) == Some(crate::kinds::ItemKind::IronBall) {
            0
        } else {
            type_mod
        }
    }

    /// The move's own `onModifyType`: Raging Bull takes Paldean Tauros's type.
    pub(crate) fn move_on_modify_type(&self, r: MonRef, kind: MoveKind, t: TypeId) -> TypeId {
        // Aura Wheel's onModifyType follows the form.
        if kind == MoveKind::AuraWheel {
            let name = if self.dex.species(self.mon(r).species).name == "Morpeko-Hangry" {
                "Dark"
            } else {
                "Electric"
            };
            return self.dex.type_id(name).unwrap_or(t);
        }
        if kind != MoveKind::RagingBull {
            return t;
        }
        let name = match self.dex.species(self.mon(r).species).name.as_str() {
            "Tauros-Paldea-Combat" => "Fighting",
            "Tauros-Paldea-Blaze" => "Fire",
            "Tauros-Paldea-Aqua" => "Water",
            _ => return t,
        };
        self.dex.type_id(name).unwrap_or(t)
    }

    /// A `damageCallback`: the damage the move deals instead of the formula.
    pub(crate) fn damage_callback(&self, r: MonRef, t: MonRef, kind: MoveKind) -> Option<u32> {
        match kind {
            MoveKind::Counter => Some(self.mon(r).vol.counter.map_or(0, |c| c.damage.max(1))),
            MoveKind::MirrorCoat => {
                Some(self.mon(r).vol.mirror_coat.map_or(0, |c| c.damage.max(1)))
            }
            MoveKind::Comeuppance | MoveKind::MetalBurst => Some(
                self.last_damaged_by(r)
                    .map_or(0, |a| (a.damage * 3 / 2).max(1)),
            ),
            MoveKind::Endeavor => {
                Some((self.mon(t).hp as u32).saturating_sub(self.mon(r).hp as u32))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modify_rounds_half_down() {
        // 0.75 * 10 = 7.5 -> 7 ; 1.5 * 3 = 4.5 -> 4
        assert_eq!(modify(10, 3, 4), 7);
        assert_eq!(modify(3, 3, 2), 4);
        assert_eq!(modify(100, 1, 2), 50);
    }

    #[test]
    fn boost_table() {
        assert_eq!(apply_boost(100, 1), 150);
        assert_eq!(apply_boost(100, -1), 66);
        assert_eq!(apply_boost(101, 6), 404);
        assert_eq!(apply_boost(101, -6), 25);
    }

    #[test]
    fn chain_matches_chain_modify() {
        // 1.5 then 1.3 in 4096ths: (6144 * 5324 + 2048) >> 12 = 7986
        assert_eq!(Chain::ONE.then(3, 2).then(13, 10).0, 7986);
    }
}
