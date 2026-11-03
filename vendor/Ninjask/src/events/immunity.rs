//! Immunity and blocking events: `TryImmunity` (the move's own checks on
//! each target, hit step 3 of `trySpreadMoveHit`), `TryAddVolatile` and
//! `SetStatus`.

use crate::abilities::{compare_mod, ModHandler, SUB_ORDER_ABILITY, SUB_ORDER_ITEM};
use crate::battle::Small;
use crate::battle::{Battle, MonRef};
use crate::dex::events::{Event, Role};
use crate::dex::types::*;
use crate::dex::Move;
use crate::kinds::{AbilityKind, EffectRef, MoveKind};
use crate::state::*;

/// One `SetStatus` handler (see `Battle::set_status_order`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StatusHandler {
    /// The target's own ability's `onSetStatus` (Insomnia, Leaf Guard...).
    Own,
    /// An `onAllySetStatus` ability on the target's side, itself included
    /// (Sweet Veil, Flower Veil).
    Ally(MonRef),
    /// An uproarer's `onAnySetStatus`.
    Uproar(MonRef),
    Safeguard,
    Terrain,
    /// A handler the sort counts that stops nothing here.
    Other,
}

impl Battle {
    /// Whether the move has an `onTryImmunity` handled by `run_try_immunity`.
    pub(crate) fn has_try_immunity(&self, mv: &Move) -> bool {
        matches!(
            self.dex.move_kind[mv.idx as usize],
            MoveKind::LeechSeed
                | MoveKind::WorrySeed
                | MoveKind::Attract
                | MoveKind::Octolock
                | MoveKind::Trick
                | MoveKind::Endeavor
        )
    }

    /// The move's own `onTryImmunity` on target `t` of user `src`: true when
    /// the target is immune (`-immune`). Leech Seed on Grass types, Worry
    /// Seed on Truant or Insomnia, Attract on the same gender or none,
    /// Trick on Sticky Hold, Octolock on Ghost types, Endeavor on a target
    /// with no more HP than the user.
    pub(crate) fn run_try_immunity(&self, t: MonRef, src: MonRef, mv: &Move) -> bool {
        match self.dex.move_kind[mv.idx as usize] {
            MoveKind::LeechSeed => self
                .dex
                .type_id("Grass")
                .is_some_and(|g| self.has_type_of(t, g)),
            MoveKind::WorrySeed => matches!(
                self.dex.ability(self.mon(t).ability).id.as_str(),
                "truant" | "insomnia"
            ),
            MoveKind::Attract => self.attract_immune(t, src),
            // Trick and Switcheroo: `!target.hasAbility('stickyhold')`, which
            // Mold Breaker doesn't bypass.
            MoveKind::Trick => {
                let m = self.mon(t);
                self.dex.ability(m.ability).id == "stickyhold"
                    && !m.revived
                    && !self.transform_ignores_ability(t)
                    && !self.ignoring_ability(t)
            }
            // `getImmunity('trapped', target)`: Ghost types.
            MoveKind::Octolock => self
                .types_of(t)
                .iter()
                .any(|&x| self.dex.type_immune_to_effect(x, "trapped")),
            MoveKind::Endeavor => self.mon(src).hp >= self.mon(t).hp,
            _ => false,
        }
    }

    /// `runEvent('TryAddVolatile')` for a volatile not yet present: every
    /// handler joins the sort, then Aroma Veil (on the target or an ally)
    /// blocks Attract, Disable, Encore, Heal Block, Taunt and Torment,
    /// announcing it when a move adds them. True when blocked (null).
    pub(crate) fn run_try_add_volatile(&mut self, t: MonRef, v: &str, from_move: bool) -> bool {
        let holders = self.try_add_volatile_sort(t);
        if !matches!(
            v,
            "attract" | "disable" | "encore" | "healblock" | "taunt" | "torment"
        ) {
            return false;
        }
        let Some(h) = holders
            .into_iter()
            .find(|&h| h.0 == t.0 && self.has_ability(h, AbilityKind::AromaVeil))
        else {
            return false;
        };
        if from_move {
            let (ti, hi) = (self.ident(t), self.ident(h));
            log_line!(self, "|-block|{ti}|ability: Aroma Veil|[of] {hi}");
        }
        true
    }

    /// Focus Punch's `onTryAddVolatile`: a flinch is refused.
    pub(crate) fn focus_punch_on_try_add_volatile(&self, t: MonRef) -> bool {
        self.mon(t).vol.focus_punch.is_some()
    }

    /// `TryAddVolatile` ability handlers: Inner Focus (flinch), Own Tempo (confusion).
    pub(crate) fn ability_blocks_volatile(&self, t: MonRef, v: &str) -> bool {
        matches!(
            (self.ability_id(t), v),
            ("innerfocus", "flinch") | ("owntempo", "confusion")
        )
    }

    /// `runEvent('SetStatus', t, src)`'s handlers in Showdown's order:
    /// `findEventHandlers` lists the target's `onSetStatus`, then for each
    /// Pokémon on its side (itself included) `onAllySetStatus` and
    /// `onAnySetStatus` (Uproar's volatile before the ability), for each foe
    /// `onFoeSetStatus` and `onAnySetStatus`, the source's
    /// `onSourceSetStatus`, Safeguard (side condition) and the terrain; then
    /// `speedSort` orders them, equal ones drawing. The first that stops
    /// the status wins, so two Sweet Veil holders, or Insomnia next to a
    /// faster ally's Sweet Veil, act by Speed.
    pub(crate) fn set_status_order(&mut self, t: MonRef, src: MonRef) -> Small<StatusHandler> {
        let mut hs: Small<(ModHandler, StatusHandler)> = Small::new();
        let push = |hs: &mut Small<(ModHandler, StatusHandler)>,
                    b: &Battle,
                    h: MonRef,
                    role: Role,
                    kind: StatusHandler| {
            let m = b.mon(h);
            let event = Event::SetStatus;
            if let Some(p) = b.dex.ability_handlers[m.ability as usize].priority(event, role) {
                hs.push((ModHandler::new(p, m.speed, SUB_ORDER_ABILITY), kind));
            }
            if let Some(item) = m.item {
                if let Some(p) = b.dex.item_handlers[item as usize].priority(event, role) {
                    hs.push((
                        ModHandler::new(p, m.speed, SUB_ORDER_ITEM),
                        StatusHandler::Other,
                    ));
                }
            }
        };
        push(&mut hs, self, t, Role::Own, StatusHandler::Own);
        for s in [t.0, 1 - t.0] {
            for slot in 0..self.sides[s].n_active {
                let Some(h) = self.active(s, slot).filter(|&h| self.mon(h).hp > 0) else {
                    continue;
                };
                if s == t.0 {
                    push(&mut hs, self, h, Role::Ally, StatusHandler::Ally(h));
                } else {
                    push(&mut hs, self, h, Role::Foe, StatusHandler::Other);
                }
                if self.mon(h).vol.uproar > 0 {
                    hs.push((
                        ModHandler::new(0, self.mon(h).speed, 2),
                        StatusHandler::Uproar(h),
                    ));
                }
                push(&mut hs, self, h, Role::Any, StatusHandler::Other);
            }
        }
        push(&mut hs, self, src, Role::Source, StatusHandler::Other);
        if self.side_extras[t.0].safeguard > 0 {
            hs.push((ModHandler::new(0, 0, 4), StatusHandler::Safeguard));
        }
        // The terrain's condition has effectType Terrain, which has no entry
        // in resolvePriority's effectTypeOrder: subOrder 0, ahead of
        // Safeguard (4) and the abilities (7).
        if matches!(self.field.terrain, Some(Terrain::Misty | Terrain::Electric)) {
            hs.push((ModHandler::new(0, 0, 0), StatusHandler::Terrain));
        }
        if hs.len() > 1 {
            self.speed_sort_by(&mut hs, |a, b| compare_mod(&a.0, &b.0));
        }
        hs.into_iter().map(|(_, k)| k).collect()
    }

    /// Runs the sorted `SetStatus` handlers until one stops the status, and
    /// returns that one. `status_move`: the status is the move's own effect
    /// (Thunder Wave), which announces most blocks; `rest`: Rest's own
    /// `setStatus` (a move without `status` or secondaries).
    pub(crate) fn run_set_status(
        &mut self,
        t: MonRef,
        st: Status,
        src: MonRef,
        effect: EffectRef,
        status_move: bool,
        rest: bool,
    ) -> Option<StatusHandler> {
        let order = self.set_status_order(t, src);
        let ti = self.ident(t);
        for h in order {
            let blocked = match h {
                StatusHandler::Own => self.own_blocks_status(t, st, status_move),
                StatusHandler::Ally(holder) => match self.ability_kind(holder) {
                    // Flower Veil protects Grass types from others' status, Yawn's sleep aside.
                    AbilityKind::FlowerVeil => {
                        let grass = self
                            .dex
                            .type_id("Grass")
                            .is_some_and(|g| self.has_type_of(t, g));
                        let blocks = grass && src != t && effect != EffectRef::Yawn;
                        if blocks && status_move && !self.active_move.synchronizing {
                            let hi = self.ident(holder);
                            log_line!(self, "|-block|{ti}|ability: Flower Veil|[of] {hi}");
                        }
                        blocks
                    }
                    AbilityKind::SweetVeil if st == Status::Slp => {
                        let hi = self.ident(holder);
                        log_line!(self, "|-block|{ti}|ability: Sweet Veil|[of] {hi}");
                        true
                    }
                    _ => false,
                },
                StatusHandler::Uproar(holder) => {
                    let blocks = st == Status::Slp && self.mon(holder).vol.uproar > 0;
                    if blocks {
                        if t == holder {
                            log_line!(self, "|-fail|{ti}|slp|[from] Uproar|[msg]");
                        } else {
                            log_line!(self, "|-fail|{ti}|slp|[from] Uproar");
                        }
                    }
                    blocks
                }
                StatusHandler::Safeguard => {
                    self.safeguard_on_set_status(t, src, effect, status_move)
                }
                StatusHandler::Terrain => {
                    if !self.is_grounded(t) || self.semi_invulnerable(t) {
                        false
                    } else {
                        // The terrains' onSetStatus also announce Yawn's sleep;
                        // Electric Terrain announces any move without secondaries.
                        let announce = status_move || effect == EffectRef::Yawn;
                        match self.field.terrain {
                            Some(Terrain::Misty) => {
                                if announce {
                                    log_line!(self, "|-activate|{ti}|move: Misty Terrain");
                                }
                                true
                            }
                            Some(Terrain::Electric) if st == Status::Slp => {
                                if announce || rest {
                                    log_line!(self, "|-activate|{ti}|move: Electric Terrain");
                                }
                                true
                            }
                            _ => false,
                        }
                    }
                }
                StatusHandler::Other => false,
            };
            if blocked {
                return Some(h);
            }
        }
        None
    }

    /// The target's own `onSetStatus`: Purifying Salt, Insomnia and the
    /// other single-status immunities, Thermal Exchange and Water Bubble,
    /// Leaf Guard in sun. Returns true when blocked.
    fn own_blocks_status(&mut self, t: MonRef, st: Status, status_move: bool) -> bool {
        let blocked = match self.ability_kind(t) {
            AbilityKind::PurifyingSalt => true,
            AbilityKind::StatusImmune(s) => st == s || (s == Status::Psn && st == Status::Tox),
            AbilityKind::ThermalExchange | AbilityKind::WaterBubble => st == Status::Brn,
            AbilityKind::LeafGuard => self.effective_weather() == Some(Weather::Sun),
            _ => false,
        };
        if blocked && status_move {
            let ident = self.ident(t);
            let name = self.ability_name(t);
            log_line!(self, "|-immune|{ident}|[from] ability: {name}");
        }
        blocked
    }
}
