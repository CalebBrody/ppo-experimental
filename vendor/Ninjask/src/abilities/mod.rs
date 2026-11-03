//! Abilities: the stat-stage pipeline (Showdown `boost` with `ChangeBoost`,
//! `TryBoost`, `AfterEachBoost`, `AfterBoost`), and ability handlers at the
//! events where Showdown runs them. Callers in moves/, conditions/, events/ and
//! items/ invoke these at the matching points.

mod copy;
mod forms;
mod illusion;

use crate::battle::Small;
use crate::battle::{ActiveList, Battle, MonRef};
use crate::dex::events::{Event, Role};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::MoveKind;
use crate::kinds::{AbilityKind, AbsorbEffect, EffectRef, ItemKind};
use crate::state::MAX_ACTIVE;
use crate::state::{Terrain, Weather};
use arrayvec::ArrayVec;

/// The most handlers one `event_sort` collects: the target's and the
/// source's ability and item, two side handlers (ally or foe, and any) per
/// active Pokémon's ability and item, and an extra volatile.
const MAX_SORTED_HANDLERS: usize = 2 + 2 + 2 * 2 * MAX_ACTIVE + 1;
/// `resolvePriority`'s subOrder for an ability's and an item's handler.
pub(crate) const SUB_ORDER_ABILITY: u32 = 7;
pub(crate) const SUB_ORDER_ITEM: u32 = 8;

/// An event handler's sort keys in Showdown's `comparePriority` (no order).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModHandler {
    pub priority: i32,
    pub speed: i32,
    pub sub_order: u32,
}

impl ModHandler {
    pub fn new(priority: i32, speed: i32, sub_order: u32) -> ModHandler {
        ModHandler {
            priority,
            speed,
            sub_order,
        }
    }
}

/// `comparePriority` without orders: priority desc, speed desc, subOrder asc.
pub(crate) fn compare_mod(a: &ModHandler, b: &ModHandler) -> i64 {
    let d = b.priority as i64 - a.priority as i64;
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
    #[inline]
    pub fn ability_kind(&self, r: MonRef) -> AbilityKind {
        if self.ability_ignored(r) {
            return AbilityKind::None;
        }
        self.dex.ability_kind[self.mon(r).ability as usize]
    }

    /// Whether the Pokémon's effective ability is of `kind` (not
    /// `AbilityKind::None`): `ability_kind(r) == kind`, reading the raw
    /// ability first so a mismatch skips the suppression checks.
    #[inline]
    pub(crate) fn has_ability(&self, r: MonRef, kind: AbilityKind) -> bool {
        debug_assert!(kind != AbilityKind::None);
        let raw = &self.dex.ability_kind[self.mon(r).ability as usize];
        // The variant check first: it inlines, unlike the full comparison.
        std::mem::discriminant(raw) == std::mem::discriminant(&kind)
            && *raw == kind
            && !self.ability_ignored(r)
    }

    #[inline]
    pub(crate) fn ability_id(&self, r: MonRef) -> &str {
        if self.ability_ignored(r) {
            return "noability";
        }
        &self.dex.ability(self.mon(r).ability).id
    }

    /// Whether the Pokémon's ability does nothing now: not active (revived
    /// in its old slot, `ignoringAbility`), suppressed by Mold Breaker,
    /// copied by Transform with `notransform`, or under Gastro Acid.
    #[inline]
    pub(crate) fn ability_ignored(&self, r: MonRef) -> bool {
        let m = self.mon(r);
        if m.revived {
            return true;
        }
        // Each remaining cause needs a battle or Pokémon state that is
        // usually absent: test those before reading the ability's flags.
        let breaker = self.ability_breaker.is_some();
        let transformed = m.transformed.is_some();
        let gastro_acid = m.vol.gastro_acid && !self.turn_state.sorting;
        if !(breaker || transformed || gastro_acid) {
            return false;
        }
        let a = self.dex.ability(m.ability);
        (breaker && a.breakable && self.ability_breaker_acts_on(r))
            || (transformed && a.no_transform)
            || (gastro_acid && !a.cant_suppress)
    }

    pub(crate) fn ability_name(&self, r: MonRef) -> String {
        self.dex.ability(self.mon(r).ability).name.clone()
    }

    fn effect_has_secondaries(&self, effect: EffectRef) -> bool {
        match effect {
            EffectRef::Move(m) => !self.dex.mv(m).secondaries.is_empty(),
            _ => false,
        }
    }

    fn is_intimidate(&self, effect: EffectRef) -> bool {
        matches!(effect, EffectRef::Ability(a) if self.dex.ability_kind[a as usize] == AbilityKind::Intimidate)
    }

    fn is_mirror_armor(&self, effect: EffectRef) -> bool {
        matches!(effect, EffectRef::Ability(a) if self.dex.ability_kind[a as usize] == AbilityKind::MirrorArmor)
    }

    /// Trace's Update: copy a random adjacent foe's ability, unless none can
    /// be traced (`notrace`, No Ability); then keep seeking.
    pub(crate) fn trace_update(&mut self, r: MonRef) {
        let candidates: Small<MonRef> = self
            .foes_of(r)
            .into_iter()
            .filter(|&f| {
                let a = self.dex.ability(self.mon(f).ability);
                a.id != "noability" && !a.flags.iter().any(|x| x == "notrace")
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let target = candidates[self.rng.random(candidates.len() as u32) as usize];
        let ability = self.mon(target).ability;
        self.set_ability(r, ability, target, "ability: Trace");
    }

    /// Showdown `setAbility` (not from a forme change): the old ability ends,
    /// the new one is announced and starts.
    pub(crate) fn set_ability(
        &mut self,
        r: MonRef,
        ability: AbilityId,
        source: MonRef,
        from: &str,
    ) -> bool {
        self.set_ability_from(r, ability, Some(source), from)
    }

    /// `set_ability` with an optional source: without one, the message has
    /// no `[of]`.
    pub(crate) fn set_ability_from(
        &mut self,
        r: MonRef,
        ability: AbilityId,
        source: Option<MonRef>,
        from: &str,
    ) -> bool {
        if self.mon(r).hp == 0 {
            return false;
        }
        let cant_suppress = |b: &Battle, a: AbilityId| b.dex.ability(a).cant_suppress;
        let old = self.mon(r).ability;
        if cant_suppress(self, ability) || cant_suppress(self, old) {
            return false;
        }
        // Illusion's onEnd (not on switch-out, where `beingCalledBack` skips it).
        if self.has_ability(r, AbilityKind::Illusion) {
            self.end_illusion(r);
        }
        self.ability_on_end(r);
        self.mon_mut(r).ability = ability;
        self.reset_ability_state(r);
        let ri = self.ident(r);
        let of = source
            .map(|s| format!("|[of] {}", self.ident(s)))
            .unwrap_or_default();
        let new_name = self.dex.ability(ability).name.clone();
        let old_name = self.dex.ability(old).name.clone();
        if from == "ability: Mummy" {
            // `-activate|HOLDER|ability: Mummy|ATTACKER|[ability] OLD`.
            let holder = source.map(|s| self.ident(s)).unwrap_or_default();
            log_line!(
                self,
                "|-activate|{holder}|ability: Mummy|{ri}|[ability] {old_name}"
            );
        } else {
            log_line!(
                self,
                "|-ability|{ri}|{new_name}|{old_name}|[from] {from}{of}"
            );
        }
        self.run_start(r);
        true
    }

    /// Non-fainted active allies of `r` (not including `r`).
    pub(crate) fn allies_of(&self, r: MonRef) -> ActiveList {
        (0..self.sides[r.0].n_active)
            .filter_map(|slot| self.active(r.0, slot))
            .filter(|&x| x != r && !self.mon(x).fainted && self.mon(x).hp > 0)
            .collect()
    }

    /// Non-fainted active foes of `r`.
    pub(crate) fn foes_of(&self, r: MonRef) -> ActiveList {
        let foe = 1 - r.0;
        (0..self.sides[foe].n_active)
            .filter_map(|slot| self.active(foe, slot))
            .filter(|&x| !self.mon(x).fainted && self.mon(x).hp > 0)
            .collect()
    }

    // ------------------------------------------------------------ stat stages

    /// Showdown `boost`. `source` caused it; `effect` decides the messages and
    /// which handlers react; `is_secondary` / `is_self` as in Showdown.
    pub(crate) fn boost(
        &mut self,
        t: MonRef,
        boosts: &Boosts,
        source: Option<MonRef>,
        effect: EffectRef,
        is_secondary: bool,
        is_self: bool,
    ) -> bool {
        self.boost_in_order(
            t,
            boosts,
            &BOOST_ORDER,
            source,
            effect,
            is_secondary,
            is_self,
        )
    }

    /// `boost` for boosts from move data, in the order the data lists them.
    pub(crate) fn boost_listed(
        &mut self,
        t: MonRef,
        boosts: &MoveBoosts,
        source: Option<MonRef>,
        effect: EffectRef,
        is_secondary: bool,
        is_self: bool,
    ) -> bool {
        self.boost_in_order(
            t,
            &boosts.vals,
            &boosts.order,
            source,
            effect,
            is_secondary,
            is_self,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn boost_in_order(
        &mut self,
        t: MonRef,
        boosts: &Boosts,
        order: &[u8; 7],
        source: Option<MonRef>,
        effect: EffectRef,
        is_secondary: bool,
        is_self: bool,
    ) -> bool {
        if self.mon(t).hp == 0 || !self.mon(t).is_active || self.mon(t).revived {
            return false;
        }
        if self.sides[1 - t.0].pokemon_left == 0 {
            return false;
        }
        // runEvent('ChangeBoost') and, after capping, ('TryBoost'): every
        // handler joins each sort (equal ones draw) before any acts.
        self.event_sort(Event::ChangeBoost, t, source, &[]);
        let mut b = *boosts;
        // ChangeBoost: Contrary.
        if self.has_ability(t, AbilityKind::Contrary) {
            for x in b.iter_mut() {
                *x = -*x;
            }
        }
        self.simple_on_change_boost(t, &mut b);
        // getCappedBoost (entries capped to 0 still produce messages).
        let mut present = [false; 7];
        let cur = self.mon(t).boosts;
        for i in 0..7 {
            if b[i] != 0 {
                present[i] = true;
                b[i] = (cur[i] + b[i]).clamp(-6, 6) - cur[i];
            }
        }
        self.event_sort(Event::TryBoost, t, source, &[]);
        self.try_boost(t, &mut b, &mut present, source, effect);

        let ident = self.ident(t);
        let mut announced = is_secondary;
        let mut success = false;
        for &i in order {
            let i = i as usize;
            let stat = Boost::ALL[i];
            if !present[i] {
                continue;
            }
            let delta = b[i];
            let before = self.mon(t).boosts[i];
            let after = (before + delta).clamp(-6, 6);
            let by = after - before;
            self.mon_mut(t).boosts[i] = after;
            let msg = if delta < 0 || after == -6 {
                "-unboost"
            } else {
                "-boost"
            };
            if by != 0 {
                success = true;
                match effect {
                    _ if self.belly_drum_boost_message(t, effect) => {}
                    _ if self.anger_point_boost_message(t, effect) => {}
                    EffectRef::Item(it) => {
                        let name = self.dex.item(it).name.clone();
                        log_line!(
                            self,
                            "|{msg}|{ident}|{}|{}|[from] item: {name}",
                            stat.id(),
                            by.abs()
                        );
                    }
                    EffectRef::Ability(a) => {
                        if !announced {
                            let name = self.dex.ability(a).name.clone();
                            log_line!(self, "|-ability|{ident}|{name}|boost");
                            announced = true;
                        }
                        log_line!(self, "|{msg}|{ident}|{}|{}", stat.id(), by.abs());
                    }
                    _ => log_line!(self, "|{msg}|{ident}|{}|{}", stat.id(), by.abs()),
                }
                self.event_sort(Event::AfterEachBoost, t, source, &[]);
                self.after_each_boost(t, by, source);
            } else if matches!(effect, EffectRef::Ability(_)) {
                if is_secondary || is_self {
                    log_line!(self, "|{msg}|{ident}|{}|0", stat.id());
                }
            } else if !is_secondary && !is_self {
                log_line!(self, "|{msg}|{ident}|{}|0", stat.id());
            }
        }
        self.event_sort(Event::AfterBoost, t, source, &[]);
        self.opportunist_on_foe_after_boost(t, &b, &present, order, effect);
        // AfterBoost: Rattled reacts to Intimidate.
        if self.has_ability(t, AbilityKind::Rattled)
            && self.is_intimidate(effect)
            && present[Boost::Atk as usize]
        {
            let mut spe = [0i8; 7];
            spe[Boost::Spe as usize] = 1;
            let a = self.mon(t).ability;
            self.boost(t, &spe, source, EffectRef::Ability(a), false, false);
        }
        self.mark_stat_changes_this_turn(t, &b, &present, success);
        success
    }

    /// `TryBoost` handlers: the target's ability, then an ally's Flower Veil.
    fn try_boost(
        &mut self,
        t: MonRef,
        b: &mut Boosts,
        present: &mut [bool; 7],
        source: Option<MonRef>,
        effect: EffectRef,
    ) {
        let ident = self.ident(t);
        let by_other = source != Some(t);
        let quiet = self.effect_has_secondaries(effect);
        let dex = self.dex.clone();
        let name = dex.ability(self.mon(t).ability).name.as_str();
        match self.ability_kind(t) {
            AbilityKind::ClearBody if by_other => {
                let mut blocked = false;
                for i in 0..7 {
                    if present[i] && b[i] < 0 {
                        present[i] = false;
                        blocked = true;
                    }
                }
                if blocked && !quiet {
                    log_line!(
                        self,
                        "|-fail|{ident}|unboost|[from] ability: {name}|[of] {ident}"
                    );
                }
            }
            AbilityKind::HyperCutter if by_other => {
                let i = Boost::Atk as usize;
                if present[i] && b[i] < 0 {
                    present[i] = false;
                    if !quiet {
                        log_line!(
                            self,
                            "|-fail|{ident}|unboost|atk|[from] ability: {name}|[of] {ident}"
                        );
                    }
                }
            }
            AbilityKind::BigPecks | AbilityKind::KeenEye if by_other => {
                self.run_try_boost(t, b, present, effect, quiet)
            }
            AbilityKind::IntimidateImmune => {
                let i = Boost::Atk as usize;
                if self.is_intimidate(effect) && present[i] && b[i] != 0 {
                    present[i] = false;
                    log_line!(
                        self,
                        "|-fail|{ident}|unboost|atk|[from] ability: {name}|[of] {ident}"
                    );
                }
            }
            AbilityKind::MirrorArmor if by_other && !self.is_mirror_armor(effect) => {
                // Each drop (unless already at -6) bounces back at its source.
                if let Some(src) = source {
                    let a = self.mon(t).ability;
                    for i in 0..7 {
                        if !present[i] || b[i] >= 0 || self.mon(t).boosts[i] == -6 {
                            continue;
                        }
                        present[i] = false;
                        if self.mon(src).hp > 0 {
                            let mut back = [0i8; 7];
                            back[i] = b[i];
                            log_line!(self, "|-ability|{ident}|Mirror Armor");
                            self.boost(src, &back, Some(t), EffectRef::Ability(a), true, false);
                        }
                    }
                }
            }
            AbilityKind::GuardDog => {
                let i = Boost::Atk as usize;
                if self.is_intimidate(effect) && present[i] && b[i] != 0 {
                    present[i] = false;
                    let mut up = [0i8; 7];
                    up[i] = 1;
                    let a = self.mon(t).ability;
                    self.boost(t, &up, Some(t), EffectRef::Ability(a), false, true);
                }
            }
            _ => {}
        }
        // Flower Veil on the target or its ally protects Grass types from others.
        let grass = self
            .dex
            .type_id("Grass")
            .map(|g| self.has_type_of(t, g))
            .unwrap_or(false);
        if grass && by_other {
            let mut holders: Small<MonRef> = smallvec::smallvec![t];
            holders.extend(self.allies_of(t));
            if let Some(&holder) = holders
                .iter()
                .find(|&&h| self.has_ability(h, AbilityKind::FlowerVeil))
            {
                let mut blocked = false;
                for i in 0..7 {
                    if present[i] && b[i] < 0 {
                        present[i] = false;
                        blocked = true;
                    }
                }
                if blocked && !quiet {
                    let hi = self.ident(holder);
                    log_line!(self, "|-block|{ident}|ability: Flower Veil|[of] {hi}");
                }
            }
        }
    }

    /// `AfterEachBoost`: Defiant and Competitive answer a foe's stat drop.
    fn after_each_boost(&mut self, t: MonRef, by: i8, source: Option<MonRef>) {
        if by >= 0 {
            return;
        }
        let Some(src) = source else { return };
        if src.0 == t.0 {
            return;
        }
        if let AbilityKind::StatDropReaction(stat) = self.ability_kind(t) {
            let mut up = [0i8; 7];
            up[stat as usize] = 2;
            let a = self.mon(t).ability;
            self.boost(t, &up, Some(t), EffectRef::Ability(a), false, true);
        }
    }

    /// `Immunity` to weather damage from abilities.
    pub(crate) fn ability_weather_immune(&self, r: MonRef, w: Weather) -> bool {
        w == Weather::Sand
            && matches!(
                self.ability_id(r),
                "sandrush" | "overcoat" | "sandveil" | "sandforce"
            )
    }

    // ------------------------------------------------------------ priority, type, targets

    /// The move's priority after `ModifyPriority` (Prankster, Gale Wings).
    pub(crate) fn move_priority(&self, r: MonRef, mv: &Move) -> i8 {
        if let Some((h, id, p)) = self.priority_override {
            if h == r && id == mv.idx {
                return p;
            }
        }
        let m = self.mon(r);
        // The move's own onModifyPriority: Grassy Glide.
        if self.dex.move_kind[mv.idx as usize] == crate::kinds::MoveKind::GrassyGlide
            && self.field.terrain == Some(crate::state::Terrain::Grassy)
            && self.is_grounded(r)
        {
            return mv.priority + 1;
        }
        // The ModifyPriority event finds no handlers on a fainted Pokémon,
        // whose move stays queued until its turn comes.
        if m.fainted || !m.is_active {
            return mv.priority;
        }
        // The raw ability first: most have no priority handler, and then
        // whether it is suppressed does not matter.
        match self.raw_ability_kind(r) {
            AbilityKind::Prankster
                if mv.category == Category::Status && !self.ability_ignored(r) =>
            {
                mv.priority + 1
            }
            AbilityKind::GaleWings
                if self.dex.type_names[mv.type_ as usize] == "Flying"
                    && m.hp == m.maxhp
                    && !self.ability_ignored(r) =>
            {
                mv.priority + 1
            }
            _ => mv.priority,
        }
    }

    /// Whether a status move gets Prankster's boost (Dark types ignore it).
    pub(crate) fn prankster_boosted(&self, r: MonRef, mv: &Move) -> bool {
        if let Some(boosted) = self.called_prankster_boosted(r, mv.idx) {
            return boosted;
        }
        self.has_ability(r, AbilityKind::Prankster) && mv.category == Category::Status
    }

    /// `ModifyType` from abilities: -ate abilities and Liquid Voice. Returns the
    /// new type and whether an -ate ability changed it (for its power boost).
    pub(crate) fn ability_move_type(
        &self,
        r: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) -> (TypeId, bool) {
        match self.ability_kind(r) {
            AbilityKind::TypeChange(t)
                if self.dex.type_names[move_type as usize] == "Normal"
                    && !matches!(
                        mv.id.as_str(),
                        "judgment"
                            | "multiattack"
                            | "naturalgift"
                            | "revelationdance"
                            | "technoblast"
                            | "terrainpulse"
                            | "weatherball"
                    ) =>
            {
                (t, true)
            }
            AbilityKind::LiquidVoice if mv.has(MoveFlag::Sound) => {
                (self.dex.ids.types.water.unwrap_or(move_type), false)
            }
            _ => (move_type, false),
        }
    }

    /// The sort of `runEvent(event, target, source)` over ability and item
    /// handlers, from their callbacks in the data: `on{event}` on the
    /// target, `onSource{event}` on the source, `onAlly{event}` on the
    /// target's side (itself included), `onFoe{event}` on its foes and
    /// `onAny{event}` everywhere, plus `extra` (volatiles, conditions). Only
    /// the order's draws matter: equal handlers shuffle, and the engine
    /// applies their effects where it models them (chained modifiers give
    /// the same result in any order).
    pub(crate) fn event_sort(
        &mut self,
        event: Event,
        target: MonRef,
        source: Option<MonRef>,
        extra: &[ModHandler],
    ) {
        // Only the sort's draws matter, and a read-only calculation makes none.
        if self.probe {
            return;
        }
        // Without two handlers there is no sort, so no draw: count, as an
        // upper bound, the effects below with a handler for the event in
        // any role.
        if extra.len() + self.event_handler_bound(event, target, source) < 2 {
            return;
        }
        let mut hs: ArrayVec<ModHandler, MAX_SORTED_HANDLERS> = ArrayVec::new();
        hs.extend(extra.iter().copied());
        self.push_event_handlers(&mut hs, event, Role::Own, target);
        if let Some(src) = source {
            self.push_event_handlers(&mut hs, event, Role::Source, src);
        }
        for s in [target.0, 1 - target.0] {
            let side_role = if s == target.0 { Role::Ally } else { Role::Foe };
            for slot in 0..self.sides[s].n_active {
                let Some(h) = self.active(s, slot).filter(|&h| self.mon(h).hp > 0) else {
                    continue;
                };
                self.push_side_event_handlers(&mut hs, event, side_role, h);
            }
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, compare_mod);
        }
    }

    /// An upper bound on the ability and item handlers `event_sort`
    /// collects: those of the target, the source and every active Pokémon
    /// with a handler for `event` in any role.
    #[inline]
    fn event_handler_bound(&self, event: Event, target: MonRef, source: Option<MonRef>) -> usize {
        let count = |m: &crate::state::Mon| -> usize {
            let a = self.dex.ability_handlers[m.ability as usize].has_event(event) as usize;
            let i = m
                .item
                .is_some_and(|i| self.dex.item_handlers[i as usize].has_event(event))
                as usize;
            a + i
        };
        let mut n = count(self.mon(target));
        if let Some(src) = source {
            n += count(self.mon(src));
        }
        for side in &self.sides {
            let k = side.n_active.min(side.mons.len());
            for m in &side.mons[..k] {
                n += count(m);
            }
        }
        n
    }

    /// Adds `h`'s ability and item handlers for `event` in `role` to an
    /// event's sort (abilities sort ahead of items at equal priority and speed).
    #[inline(always)]
    fn push_event_handlers<const N: usize>(
        &self,
        hs: &mut ArrayVec<ModHandler, N>,
        event: Event,
        role: Role,
        h: MonRef,
    ) {
        let m = self.mon(h);
        if let Some(p) = self.dex.ability_handlers[m.ability as usize].priority(event, role) {
            hs.push(ModHandler::new(p, m.speed, SUB_ORDER_ABILITY));
        }
        if let Some(item) = m.item {
            if let Some(p) = self.dex.item_handlers[item as usize].priority(event, role) {
                hs.push(ModHandler::new(p, m.speed, SUB_ORDER_ITEM));
            }
        }
    }

    /// `push_event_handlers` for a Pokémon on the field: its `role` (ally
    /// or foe) handlers, then its `onAny` ones, ability before item in each.
    #[inline(always)]
    fn push_side_event_handlers<const N: usize>(
        &self,
        hs: &mut ArrayVec<ModHandler, N>,
        event: Event,
        role: Role,
        h: MonRef,
    ) {
        let m = self.mon(h);
        let ability = &self.dex.ability_handlers[m.ability as usize];
        let item = m.item.map(|i| &self.dex.item_handlers[i as usize]);
        for role in [role, Role::Any] {
            if let Some(p) = ability.priority(event, role) {
                hs.push(ModHandler::new(p, m.speed, SUB_ORDER_ABILITY));
            }
            if let Some(p) = item.and_then(|item| item.priority(event, role)) {
                hs.push(ModHandler::new(p, m.speed, SUB_ORDER_ITEM));
            }
        }
    }

    /// `runEvent('TryAddVolatile', t)`: every handler joins the sort before
    /// any acts (their effects are applied where the engine adds the
    /// volatile), so equal ones draw: the target's ability, its side's
    /// `onAllyTryAddVolatile` (itself included) and the foes'
    /// `onFoeTryAddVolatile`, every `onAnyTryAddVolatile`, and Misty or
    /// Electric Terrain.
    /// Returns the handlers' holders in the sorted order.
    pub(crate) fn try_add_volatile_sort(&mut self, t: MonRef) -> Small<MonRef> {
        let event = Event::TryAddVolatile;
        let mut hs: Small<(ModHandler, Option<MonRef>)> = Small::new();
        let handlers = |h: MonRef| &self.dex.ability_handlers[self.mon(h).ability as usize];
        if let Some(p) = handlers(t).priority(event, Role::Own) {
            hs.push((
                ModHandler::new(p, self.mon(t).speed, SUB_ORDER_ABILITY),
                Some(t),
            ));
        }
        for s in [t.0, 1 - t.0] {
            let side_role = if s == t.0 { Role::Ally } else { Role::Foe };
            for slot in 0..self.sides[s].n_active {
                let Some(h) = self.active(s, slot).filter(|&h| self.mon(h).hp > 0) else {
                    continue;
                };
                for role in [side_role, Role::Any] {
                    if let Some(p) = handlers(h).priority(event, role) {
                        hs.push((
                            ModHandler::new(p, self.mon(h).speed, SUB_ORDER_ABILITY),
                            Some(h),
                        ));
                    }
                }
            }
        }
        if matches!(self.field.terrain, Some(Terrain::Misty | Terrain::Electric)) {
            hs.push((ModHandler::new(0, 0, 0), None));
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.0, &b.0));
        }
        hs.into_iter().filter_map(|(_, h)| h).collect()
    }

    /// `RedirectTarget`: Lightning Rod and Storm Drain draw single-target moves
    /// of their type. Returns the new target, announcing the redirection.
    pub(crate) fn redirect_target(
        &mut self,
        src: MonRef,
        target: MonRef,
        move_type: TypeId,
    ) -> MonRef {
        // Follow Me and Rage Powder (priority 1); Rage Powder doesn't draw
        // Pokémon immune to powder.
        let powder_immune = {
            let ty = self.types_of(src);
            ty.iter()
                .any(|&t| self.dex.type_immune_to_effect(t, "powder"))
                || self.has_ability(src, AbilityKind::Overcoat)
        };
        let mut centers: Small<(MonRef, i32)> = self
            .foes_of(src)
            .into_iter()
            .filter(|&h| {
                self.mon(h).vol.follow_me || (self.mon(h).vol.rage_powder && !powder_immune)
            })
            .map(|h| (h, self.mon(h).speed))
            .collect();
        if !centers.is_empty() {
            // compareRedirectOrder: speed, then the older ability state.
            centers.sort_by_key(|&(h, s)| (-(s as i64), self.mon(h).ability_order));
            self.smart_target = false;
            return centers[0].0;
        }
        let mut holders: Small<(MonRef, i32)> = self
            .all_active()
            .into_iter()
            .filter(|&h| h != src)
            .filter(|&h| matches!(self.ability_kind(h), AbilityKind::Absorb { type_, redirect: true, .. } if type_ == move_type))
            .map(|h| (h, self.mon(h).speed))
            .collect();
        if holders.is_empty() {
            // Counter's redirection comes last (priority -1).
            return self.counter_on_redirect_target(src, target);
        }
        holders.sort_by_key(|&(h, speed)| (-(speed as i64), self.mon(h).ability_order));
        self.smart_target = false;
        let holder = holders[0].0;
        if holder != target {
            let hi = self.ident(holder);
            let name = self.ability_name(holder);
            log_line!(self, "|-activate|{hi}|ability: {name}");
        }
        holder
    }

    /// Showdown's `runEvent('TryMove', user, target, move)`: `onAnyTryMove`
    /// on the user's side (Damp), then `onFoeTryMove` and `onAnyTryMove` on
    /// its foes (Armor Tail, Queenly Majesty, Damp). Every handler present
    /// joins the sort, so equal ones draw; the first that blocks stops the
    /// move. Returns whether it was blocked.
    pub(crate) fn run_try_move(
        &mut self,
        src: MonRef,
        target: MonRef,
        mv: &Move,
        line: usize,
    ) -> bool {
        let mut hs: Small<(Option<MonRef>, ModHandler)> = Small::new();
        for s in [src.0, 1 - src.0] {
            for slot in 0..self.sides[s].n_active {
                let Some(h) = self.active(s, slot).filter(|&h| self.mon(h).hp > 0) else {
                    continue;
                };
                let a = &self.dex.ability_handlers[self.mon(h).ability as usize];
                // No ability's onFoeTryMove or onAnyTryMove sets a priority.
                let key = ModHandler::new(0, self.mon(h).speed, SUB_ORDER_ABILITY);
                if s != src.0 && a.has(Event::TryMove, Role::Foe) {
                    hs.push((Some(h), key));
                }
                if a.has(Event::TryMove, Role::Any) {
                    hs.push((Some(h), key));
                }
            }
        }
        if self.mon(src).vol.metronome.is_some() {
            // The Metronome volatile's handler (priority -2) joins the sort.
            hs.push((None, ModHandler::new(-2, self.mon(src).speed, 2)));
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.1, &b.1));
        }
        let priority = self.move_priority(src, mv);
        let unblockable = matches!(mv.target, Target::FoeSide)
            || (mv.target == Target::All
                && !matches!(mv.id.as_str(), "perishsong" | "flowershield" | "rototiller"));
        for &(h, _) in hs.iter() {
            let Some(h) = h else {
                self.metronome_on_try_move(src, mv);
                continue;
            };
            let blocks = match self.ability_kind(h) {
                AbilityKind::PriorityBlock => {
                    !unblockable && priority > 0 && (target.0 == h.0 || mv.target == Target::All)
                }
                AbilityKind::Damp => matches!(
                    mv.id.as_str(),
                    "explosion" | "mindblown" | "mistyexplosion" | "selfdestruct"
                ),
                _ => false,
            };
            if blocks {
                self.attr_move_line(line, "[still]");
                let hi = self.ident(h);
                let si = self.ident(src);
                let name = self.ability_name(h);
                log_line!(self, "|cant|{hi}|ability: {name}|{}|[of] {si}", mv.name);
                return true;
            }
        }
        false
    }

    /// Extra PP that Pressure takes from a move aimed at its holder.
    pub(crate) fn pressure_pp(&self, src: MonRef, mv: &Move, targets: &[MonRef]) -> u8 {
        // `mustpressure` moves (Imprison...) count every foe, whatever they target.
        let pressured: ActiveList = match mv.target {
            _ if mv.has(MoveFlag::MustPressure) => self.foes_of(src),
            Target::FoeSide => ActiveList::new(),
            Target::All => self.foes_of(src),
            Target::AllySide | Target::AllyTeam => ActiveList::new(),
            _ => targets.iter().copied().collect(),
        };
        pressured
            .iter()
            .filter(|&&t| t.0 != src.0 && self.has_ability(t, AbilityKind::Pressure))
            .count() as u8
    }

    /// `TryHit` ability handlers on a target (after Psychic Terrain and Protect).
    /// Returns true when the move is stopped for this target.
    pub(crate) fn ability_try_hit(
        &mut self,
        src: MonRef,
        t: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) -> bool {
        if t == src || !self.ability_try_hit_blocks(src, t, mv, move_type) {
            return false;
        }
        let ti = self.ident(t);
        let name = self.ability_name(t);
        match self.ability_kind(t) {
            AbilityKind::GoodAsGold if mv.category == Category::Status => {
                log_line!(self, "|-immune|{ti}|[from] ability: Good as Gold");
                true
            }
            AbilityKind::Absorb { type_, effect, .. } if type_ == move_type => {
                let a = self.mon(t).ability;
                let done = match effect {
                    AbsorbEffect::Boost(stat, n) => {
                        let mut b = [0i8; 7];
                        b[stat as usize] = n;
                        self.boost(t, &b, Some(src), EffectRef::Ability(a), false, false)
                    }
                    AbsorbEffect::Heal => {
                        let amount = (self.mon(t).maxhp as u32 / 4).max(1);
                        let si = self.ident(src);
                        self.heal(t, amount, &format!("|[from] ability: {name}|[of] {si}")) > 0
                    }
                    AbsorbEffect::FlashFire => {
                        if self.mon(t).vol.flash_fire {
                            false
                        } else {
                            self.mon_mut(t).vol.flash_fire = true;
                            log_line!(self, "|-start|{ti}|ability: Flash Fire");
                            true
                        }
                    }
                };
                if !done {
                    log_line!(self, "|-immune|{ti}|[from] ability: {name}");
                }
                true
            }
            AbilityKind::Sturdy if mv.ohko.is_some() => {
                log_line!(self, "|-immune|{ti}|[from] ability: Sturdy");
                true
            }
            AbilityKind::Soundproof if mv.has(MoveFlag::Sound) => {
                log_line!(self, "|-immune|{ti}|[from] ability: Soundproof");
                true
            }
            AbilityKind::Bulletproof if mv.has(MoveFlag::Bullet) => {
                log_line!(self, "|-immune|{ti}|[from] ability: Bulletproof");
                true
            }
            AbilityKind::Overcoat if mv.has(MoveFlag::Powder) => {
                log_line!(self, "|-immune|{ti}|[from] ability: Overcoat");
                true
            }
            AbilityKind::IntimidateImmune
                if self.ability_id(t) == "oblivious"
                    && matches!(
                        self.dex.move_kind[mv.idx as usize],
                        crate::kinds::MoveKind::Taunt | crate::kinds::MoveKind::Attract
                    ) =>
            {
                log_line!(self, "|-immune|{ti}|[from] ability: Oblivious");
                true
            }
            AbilityKind::Telepathy if t.0 == src.0 && mv.category != Category::Status => {
                log_line!(self, "|-activate|{ti}|ability: Telepathy");
                true
            }
            _ => self.ability_on_try_hit(src, t, move_type),
        }
    }

    /// Whether a `TryHit` ability handler of `t` stops `src`'s move
    /// ([`Battle::ability_try_hit`]'s cases, without their effects): Good as
    /// Gold, the absorbing abilities (Volt Absorb, Flash Fire, Sap Sipper...),
    /// Sturdy against a one-hit KO, Soundproof, Bulletproof, Overcoat,
    /// Oblivious, Telepathy and Dry Skin.
    pub(crate) fn ability_try_hit_blocks(
        &self,
        src: MonRef,
        t: MonRef,
        mv: &Move,
        move_type: TypeId,
    ) -> bool {
        if t == src {
            return false;
        }
        match self.ability_kind(t) {
            AbilityKind::GoodAsGold => mv.category == Category::Status,
            AbilityKind::Absorb { type_, .. } if type_ == move_type => true,
            AbilityKind::Sturdy if mv.ohko.is_some() => true,
            AbilityKind::Soundproof if mv.has(MoveFlag::Sound) => true,
            AbilityKind::Bulletproof if mv.has(MoveFlag::Bullet) => true,
            AbilityKind::Overcoat if mv.has(MoveFlag::Powder) => true,
            AbilityKind::IntimidateImmune
                if self.ability_id(t) == "oblivious"
                    && matches!(
                        self.dex.move_kind[mv.idx as usize],
                        crate::kinds::MoveKind::Taunt | crate::kinds::MoveKind::Attract
                    ) =>
            {
                true
            }
            AbilityKind::Telepathy if t.0 == src.0 && mv.category != Category::Status => true,
            _ => {
                self.has_ability(t, AbilityKind::DrySkin)
                    && Some(move_type) == self.dex.ids.types.water
            }
        }
    }

    /// Whether type immunity is ignored for this move (Scrappy on Normal and
    /// Fighting moves).
    pub(crate) fn ignores_type_immunity(&self, src: MonRef, move_type: TypeId) -> bool {
        self.ability_id(src) == "scrappy"
            && matches!(
                self.dex.type_names[move_type as usize].as_str(),
                "Normal" | "Fighting"
            )
    }

    // ------------------------------------------------------------ modifiers

    /// `ModifySTAB`: Adaptability.
    pub(crate) fn stab_modifier(&self, src: MonRef, move_type: TypeId) -> Option<(u32, u32)> {
        if !self.has_type_of(src, move_type) {
            return None;
        }
        if self.has_ability(src, AbilityKind::Adaptability) {
            Some((2, 1))
        } else {
            Some((3, 2))
        }
    }

    /// Whether screens are bypassed (Infiltrator).
    pub(crate) fn infiltrates(&self, src: MonRef) -> bool {
        self.has_ability(src, AbilityKind::Infiltrator)
    }

    /// `CriticalHit`: Battle Armor and Shell Armor prevent critical hits.
    pub(crate) fn crit_blocked(&self, tgt: MonRef) -> bool {
        self.has_ability(tgt, AbilityKind::CritImmune)
    }

    /// `SourceModifyAccuracy` from the user's ability (priority -1): Compound
    /// Eyes, Hustle on physical moves.
    pub(crate) fn ability_accuracy_mod(&self, src: MonRef, mv: &Move) -> Option<(u32, u32)> {
        match self.ability_kind(src) {
            AbilityKind::CompoundEyes => Some((5325, 4096)),
            AbilityKind::Hustle if mv.category == Category::Physical => Some((3277, 4096)),
            _ => None,
        }
    }

    /// The `Damage` event from abilities for damage from a move: Sturdy.
    pub(crate) fn ability_damage_event(&mut self, t: MonRef, amount: u32) -> u32 {
        let (hp, maxhp) = (self.mon(t).hp, self.mon(t).maxhp);
        if self.has_ability(t, AbilityKind::Sturdy) && hp == maxhp && amount >= hp as u32 {
            let ti = self.ident(t);
            log_line!(self, "|-ability|{ti}|Sturdy");
            return hp as u32 - 1;
        }
        amount
    }

    /// Rock Head: no recoil.
    pub(crate) fn blocks_recoil(&self, r: MonRef) -> bool {
        self.has_ability(r, AbilityKind::RockHead)
    }

    /// `AfterUseItem`: Unburden.
    pub(crate) fn after_use_item(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::Unburden) && !self.mon(r).vol.unburden {
            self.try_add_volatile_sort(r);
            self.mon_mut(r).vol.unburden = true;
        }
    }

    /// Unnerve on the field stops this Pokémon from eating berries.
    pub(crate) fn unnerved(&self, r: MonRef) -> bool {
        // Only once its start has run (`effectState.unnerved`): a berry
        // eaten in the Update after a switch, before runSwitch, gets through.
        self.foes_of(r).iter().any(|&f| {
            self.has_ability(f, AbilityKind::Unnerve)
                && self.mon(f).ability_state.unnerved == Some(true)
        })
    }

    // ------------------------------------------------------------ residual

    /// Speed Boost's residual handler (order 28, subOrder 2).
    pub(crate) fn speed_boost(&mut self, r: MonRef) {
        if self.has_ability(r, AbilityKind::SpeedBoost) && self.mon(r).active_turns > 0 {
            let mut b = [0i8; 7];
            b[Boost::Spe as usize] = 1;
            let a = self.mon(r).ability;
            self.boost(r, &b, None, EffectRef::Ability(a), false, false);
        }
    }

    /// `suppressingAbility(target)`: the move in use ignores abilities
    /// (Mold Breaker) and its user is not the target.
    pub(crate) fn suppressing_ability(&self, target: MonRef) -> bool {
        self.ability_breaker
            .is_some_and(|u| u != target && self.mon(u).is_active && !self.mon(u).fainted)
            && !self.has_item(target, "abilityshield")
    }

    /// The ability can't be suppressed (`cantsuppress`).
    #[inline]
    pub(crate) fn cant_suppress(&self, r: MonRef) -> bool {
        self.dex.ability(self.mon(r).ability).cant_suppress
    }

    /// Showdown `ignoringAbility` from Gastro Acid (none of this format's
    /// abilities is Neutralizing Gas). Handler collection for a sort keeps it.
    #[inline]
    pub(crate) fn ignoring_ability(&self, r: MonRef) -> bool {
        !self.turn_state.sorting && self.mon(r).vol.gastro_acid && !self.cant_suppress(r)
    }

    /// Showdown `ignoringItem` from Klutz (no item here has `ignoreKlutz`).
    #[inline]
    pub(crate) fn ignoring_item(&self, r: MonRef) -> bool {
        // Magic Room and Klutz (`ignoringItem`).
        !self.turn_state.sorting
            && (self.field.magic_room > 0 || self.has_ability(r, AbilityKind::Klutz))
    }

    /// The held item's kind, Klutz or not (`getItem()`): the Choice lock
    /// reads it, and only stops enforcing itself while the item is ignored.
    pub(crate) fn held_item_kind(&self, r: MonRef) -> Option<ItemKind> {
        if self.mon(r).revived {
            return None;
        }
        self.mon(r).item.map(|i| self.dex.item_kind[i as usize])
    }

    /// Sticky Hold's `onTakeItem`, run when `taker` (`takeItem(source)`; none
    /// for a bare `takeItem()`) removes the item of `holder`: another
    /// Pokémon's move or ability can't, and Knock Off never can. Announces
    /// itself; true when the item stays.
    pub(crate) fn sticky_hold_on_take_item(
        &mut self,
        holder: MonRef,
        taker: Option<MonRef>,
        knock_off: bool,
    ) -> bool {
        let m = self.mon(holder);
        if !self.has_ability(holder, AbilityKind::StickyHold)
            || m.hp == 0
            || !(taker.is_some_and(|x| x != holder) || knock_off)
            || m.item.is_some_and(|i| self.dex.item(i).id == "stickybarb")
        {
            return false;
        }
        let hi = self.ident(holder);
        log_line!(self, "|-activate|{hi}|ability: Sticky Hold");
        true
    }

    /// Magnetic Flux's onHitSide: +1 Def and SpD to each Pokémon on the
    /// user's side with Plus or Minus; false (the move fails) without one.
    pub(crate) fn magnetic_flux_on_hit_side(&mut self, r: MonRef, mv: &Move) -> bool {
        let targets: Small<MonRef> = (0..self.sides[r.0].n_active)
            .filter_map(|sl| self.active(r.0, sl))
            .filter(|&a| self.mon(a).hp > 0 && !self.mon(a).fainted)
            .filter(|&a| matches!(self.ability_id(a), "plus" | "minus"))
            .collect();
        let mut did = false;
        for t in targets {
            let mut b = [0i8; 7];
            b[Boost::Def as usize] = 1;
            b[Boost::Spd as usize] = 1;
            did = self.boost(t, &b, Some(r), EffectRef::Move(mv.idx), false, true) || did;
        }
        did
    }

    /// Pollen Puff aimed at an ally: its onTryHit turns the move into a heal
    /// (no damage, `infiltrates`), which runs like a status move.
    pub(crate) fn ally_puff_on_try_hit(
        &self,
        src: MonRef,
        targets: &[MonRef],
        kind: MoveKind,
    ) -> bool {
        kind == MoveKind::PollenPuff && targets.first().is_some_and(|&t| t.0 == src.0 && t != src)
    }

    /// The turn's start (`endTurn`'s loop): an apparentType that differs
    /// from the real types is brought up to date with a silent `typechange`.
    pub(crate) fn sync_apparent_type(&mut self, r: MonRef) {
        let Some((apparent, real)) = self.mon_mut(r).vol.apparent_types.take() else {
            return;
        };
        if self.mon(r).types != real {
            return;
        }
        let names = |b: &Battle, t: [TypeId; 2]| {
            t.iter()
                .filter(|&&x| x != NO_TYPE)
                .map(|&x| b.dex.type_names[x as usize].clone())
                .collect::<Vec<_>>()
                .join("/")
        };
        let now = names(self, self.base_types_of(r));
        if now != names(self, apparent) {
            let ri = self.ident(r);
            log_line!(self, "|-start|{ri}|typechange|{now}|[silent]");
            // The typechange message removes the added type, so put it back.
            let added = self.mon(r).added_type;
            if added != NO_TYPE {
                let name = self.dex.type_names[added as usize].clone();
                log_line!(self, "|-start|{ri}|typeadd|{name}|[silent]");
            }
        }
    }

    /// Cute Charm's onDamagingHit: 30% to infatuate an attacker that made
    /// contact.
    pub(crate) fn cute_charm_on_damaging_hit(&mut self, src: MonRef, t: MonRef, contact: bool) {
        if contact && self.rng.chance(3, 10) {
            self.add_attract(src, t, true);
        }
    }

    /// Showdown `suppressingAbility`: while a Mold Breaker user's move is the
    /// active move (from its ModifyMove to the end of the action), every
    /// other Pokémon's `breakable` ability is skipped. Handlers still join
    /// the event sorts; only their effects are skipped.
    pub(crate) fn ability_suppressed(&self, r: MonRef) -> bool {
        self.dex.ability(self.mon(r).ability).breakable && self.ability_breaker_acts_on(r)
    }

    /// Whether a Mold Breaker user's move is running against `r` (another,
    /// active Pokémon), so `r`'s breakable ability does nothing.
    #[inline]
    fn ability_breaker_acts_on(&self, r: MonRef) -> bool {
        self.ability_breaker.is_some_and(|u| {
            let breaker = self.mon(u);
            u != r && breaker.is_active && !breaker.fainted
        })
    }

    /// The ability's kind whether or not Mold Breaker suppresses it, for
    /// event sorts (Showdown sorts suppressed handlers too).
    pub(crate) fn raw_ability_kind(&self, r: MonRef) -> AbilityKind {
        self.dex.ability_kind[self.mon(r).ability as usize]
    }
}
