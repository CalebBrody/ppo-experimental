//! The action queue: building the turn's actions, Showdown's priority and speed
//! ordering, and inserting actions mid-turn.

use super::{ActiveList, Battle, MonRef, PerActive};
use crate::battle::Small;
use crate::dex::types::*;
use crate::state::*;

/// Whether `resolve_action_frac` resolves a new action or re-sorts a queued
/// one, and whether the stored speeds are current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Resolution {
    /// A new action: Showdown `resolveAction`, which records a move's
    /// `originalTarget`.
    New,
    /// A new action chosen for the turn, resolved right after
    /// `update_speed`: as `New`, but the stored speeds are current.
    Chosen,
    /// A queued action re-sorted by current speed, right after
    /// `update_speed`: only `getActionSpeed` runs, so the action keeps its
    /// `originalTarget`, and the stored speeds are current.
    Resort,
    /// `Resort` when no active Pokémon holds Unaware, which re-sorting
    /// cannot change: its `ModifyBoost` sorts have nothing to sort.
    ResortWithoutUnaware,
}

impl Resolution {
    /// `update_speed` has just stored every active Pokémon's action speed,
    /// and resolving actions changes nothing it reads (only targets are
    /// drawn and recorded).
    fn speeds_current(self) -> bool {
        self != Resolution::New
    }
}

impl Battle {
    pub(super) fn commit(&mut self) {
        match self.request {
            RequestState::TeamPreview => {
                // Showdown queues a `team` action per chosen Pokémon (priority
                // -index, the Pokémon's speed) and sorts the queue: equal
                // speeds at the same index are shuffled. The order itself only
                // rebuilds each side's team, but the draws must match.
                let mut team_actions: Small<(i64, i64), 12> = Small::new();
                for s in 0..2 {
                    let order = self.sides[s].team_preview_choice.as_ref().unwrap();
                    for (index, &pos) in order.iter().enumerate() {
                        team_actions.push((-(index as i64), self.action_speed((s, pos)) as i64));
                    }
                }
                self.speed_sort_by(&mut team_actions, |a, b| {
                    (b.0 - a.0).signum() * 2 + (b.1 - a.1).signum()
                });
                for s in 0..2 {
                    let order = self.sides[s].team_preview_choice.take().unwrap();
                    // Move the chosen Pokémon out rather than cloning them (a
                    // repeated index, which the fast path allows, is a copy).
                    let mut old: Vec<Option<Mon>> = std::mem::take(&mut self.sides[s].mons)
                        .into_iter()
                        .map(Some)
                        .collect();
                    let mut mons: Vec<Mon> = Vec::with_capacity(order.len());
                    for &i in &order {
                        let m = match old[i].take() {
                            Some(m) => m,
                            None => mons[order.iter().position(|&j| j == i).unwrap()].clone(),
                        };
                        mons.push(m);
                    }
                    for (p, m) in mons.iter_mut().enumerate() {
                        m.position = p;
                    }
                    self.sides[s].pokemon_left = mons.len();
                    self.sides[s].mons = mons;
                }
                self.queue.clear();
                self.queue.push_back(Action {
                    kind: ActionKind::Start,
                    order: 2,
                    priority: 0,
                    speed: 1,
                    frac: 0,
                });
                self.mid_turn = false;
                self.request = RequestState::Move;
                self.run();
            }
            RequestState::Move | RequestState::Switch => {
                let insta = self.request == RequestState::Switch;
                self.update_speed();
                // The queue is empty while the choices resolve (Cud Chew
                // reads it when Custap Berry is eaten); its buffer is kept.
                let old_queue = std::mem::take(&mut self.queue);
                // At most four actions per slot: charge, Mega Evolution,
                // before-turn, move.
                let mut new_actions: arrayvec::ArrayVec<Action, { 4 * MAX_ACTIVE }> =
                    arrayvec::ArrayVec::new();
                for s in 0..2 {
                    if let Some(choice) = self.sides[s].choice.take() {
                        for (slot, c) in choice.into_iter().enumerate() {
                            let Some(r) = self.active(s, slot) else {
                                continue;
                            };
                            match c {
                                SlotChoice::Pass => {}
                                SlotChoice::Switch { index } => {
                                    let revive = insta && self.sides[s].revival[slot];
                                    let m = self.mon_mut(r);
                                    m.switch_flag = false;
                                    let from = m.switch_from_move.take();
                                    let kind = if revive {
                                        ActionKind::Revive {
                                            side: s,
                                            mon: r.1,
                                            target: index,
                                        }
                                    } else {
                                        ActionKind::Switch {
                                            side: s,
                                            mon: r.1,
                                            target: index,
                                            insta,
                                            from,
                                        }
                                    };
                                    new_actions.push(self.resolve_chosen_action(kind));
                                }
                                SlotChoice::Move {
                                    slot: ms,
                                    target,
                                    mega,
                                } => {
                                    // A locked Pokémon uses its locked move (Showdown `chooseMove`).
                                    let (ms, target, mega, lock_id) = match self.locked_move(r) {
                                        Some(lock) => {
                                            let (slot, loc) = self.locked_action(r, lock);
                                            let id = match lock {
                                                crate::moves::LockedMove::Move(id) => id,
                                                _ => 0,
                                            };
                                            (slot, loc, false, id)
                                        }
                                        None => (ms, target, mega, 0),
                                    };
                                    // resolveAction resolves the before-turn callback first, then
                                    // the charge callback (each draws a random target), and queues
                                    // them as [charge, mega, before-turn, move].
                                    let id = if ms == EXTERNAL {
                                        lock_id
                                    } else if ms < RECHARGE {
                                        self.mon(r).moves[ms].id
                                    } else {
                                        0
                                    };
                                    let before = (ms < RECHARGE
                                        && self.has_before_turn_callback(id))
                                    .then(|| {
                                        self.resolve_chosen_action(ActionKind::BeforeTurnMove {
                                            side: s,
                                            mon: r.1,
                                            id,
                                            target,
                                        })
                                    });
                                    if ms < RECHARGE && self.priority_charge_move(id) {
                                        new_actions.push(self.resolve_chosen_action(
                                            ActionKind::PriorityCharge {
                                                side: s,
                                                mon: r.1,
                                                id,
                                            },
                                        ));
                                    }
                                    if mega {
                                        new_actions.push(self.resolve_chosen_action(
                                            ActionKind::MegaEvo { side: s, mon: r.1 },
                                        ));
                                    }
                                    new_actions.extend(before);
                                    let kind = ActionKind::Move {
                                        side: s,
                                        mon: r.1,
                                        slot: ms,
                                        target,
                                        id,
                                    };
                                    new_actions.push(self.resolve_chosen_action(kind));
                                }
                            }
                        }
                    }
                }
                self.force_switch = [[false; 2]; 2];
                // commitChoices sorts only the new choices and appends the
                // rest of the turn; turnLoop then inserts beforeTurn and
                // appends residual at the start of a turn.
                self.speed_sort(&mut new_actions);
                // The new actions go before the rest of the queue.
                self.queue = old_queue;
                for &a in new_actions.iter().rev() {
                    self.queue.push_front(a);
                }
                if self.request == RequestState::Move && !self.mid_turn {
                    self.turn_state.round_actions.clear();
                    self.insert_action(Action {
                        kind: ActionKind::BeforeTurn,
                        order: 4,
                        priority: 0,
                        speed: 1,
                        frac: 0,
                    });
                    self.queue.push_back(Action {
                        kind: ActionKind::Residual,
                        order: 300,
                        priority: 0,
                        speed: 1,
                        frac: 0,
                    });
                    self.mid_turn = true;
                }
                self.request = RequestState::Move;
                self.run();
            }
            RequestState::Ended => {}
        }
    }

    /// Showdown `resolveAction` + `getActionSpeed`: fill in order, priority
    /// and speed. A move chosen without a target (spread moves) gets a random
    /// one now; `getActionSpeed` then looks its target up again, which for
    /// spread moves draws again (also each time the queue is re-sorted).
    pub(crate) fn resolve_action(&mut self, kind: ActionKind) -> Action {
        self.resolve_action_frac(kind, None, Resolution::New)
    }

    /// `resolve_action` for an action chosen for the turn, right after
    /// `update_speed`.
    fn resolve_chosen_action(&mut self, kind: ActionKind) -> Action {
        self.resolve_action_frac(kind, None, Resolution::Chosen)
    }

    /// `resolve_action`; `frac` is the fractional priority already resolved
    /// (the queue's re-sort only runs `getActionSpeed`, which reuses it).
    pub(super) fn resolve_action_frac(
        &mut self,
        kind: ActionKind,
        frac: Option<i32>,
        resolution: Resolution,
    ) -> Action {
        let note_target = !matches!(
            resolution,
            Resolution::Resort | Resolution::ResortWithoutUnaware
        );
        let order = action_order(&kind);
        let mut kind = kind;
        let mut action_frac = 0;
        let (priority, speed) = match kind {
            ActionKind::Move {
                side,
                mon,
                slot,
                target,
                id,
            } => {
                let r = (side, mon);
                // resolveAction's runEvent('FractionalPriority') comes first.
                action_frac = match frac {
                    Some(f) => f,
                    None => {
                        let status =
                            slot < RECHARGE && self.dex.mv(id).category == Category::Status;
                        self.run_fractional_priority(r, status)
                    }
                };
                let pr = if slot >= RECHARGE {
                    // Neither is a moveset move. Struggle targets randomNormal;
                    // `recharge` isn't in the dex, so it has no target type.
                    // Both get a random foe in resolveAction when the location
                    // is 0, and again from getActionSpeed's getTarget.
                    if target == 0 {
                        if let Some(t) = self.get_random_target(r, Target::RandomNormal) {
                            kind = ActionKind::Move {
                                side,
                                mon,
                                slot,
                                target: self.loc_of(r, t),
                                id,
                            };
                        }
                    }
                    if let ActionKind::Move { target, .. } = kind {
                        if note_target {
                            self.note_original_target(r, target);
                        }
                    }
                    self.get_random_target(r, Target::RandomNormal);
                    0
                } else {
                    let dex = self.dex.clone();
                    let mv = dex.mv(id);
                    let mut loc = target;
                    // resolveAction's `action.move` (Champions: Curse from a
                    // non-Ghost targets the user).
                    let tt = self.move_target_at_choice(r, mv);
                    if loc == 0 {
                        if let Some(t) = self.get_random_target(r, tt) {
                            loc = self.loc_of(r, t);
                        }
                        kind = ActionKind::Move {
                            side,
                            mon,
                            slot,
                            target: loc,
                            id,
                        };
                    }
                    if note_target {
                        self.note_original_target(r, loc);
                    }
                    self.get_target(r, tt, loc, mv.smart_target, mv.has(MoveFlag::FutureMove));
                    self.move_priority(r, mv) as i32
                };
                (pr, self.resolved_action_speed(resolution, r))
            }
            ActionKind::Switch { side, mon, .. }
            | ActionKind::Revive { side, mon, .. }
            | ActionKind::MegaEvo { side, mon }
            | ActionKind::RunSwitch { side, mon } => {
                (0, self.resolved_action_speed(resolution, (side, mon)))
            }
            // The callback's move has a target, so a random one is drawn (a
            // field move's is the user: no draw).
            ActionKind::PriorityCharge { side, mon, id } => {
                let r = (side, mon);
                let tt = self.dex.mv(id).target;
                self.get_random_target(r, tt);
                (0, self.resolved_action_speed(resolution, r))
            }
            ActionKind::BeforeTurnMove {
                side,
                mon,
                id,
                target,
            } => {
                let r = (side, mon);
                if target == 0 {
                    let tt = self.dex.mv(id).target;
                    if let Some(t) = self.get_random_target(r, tt) {
                        kind = ActionKind::BeforeTurnMove {
                            side,
                            mon,
                            id,
                            target: self.loc_of(r, t),
                        };
                    }
                }
                (0, self.resolved_action_speed(resolution, r))
            }
            _ => (0, 1),
        };
        Action {
            kind,
            order,
            priority,
            speed,
            frac: action_frac,
        }
    }

    /// `getActionSpeed`'s speed for an action of `r`: `action_speed_draw`,
    /// or with current stored speeds the one `update_speed` stored for a
    /// Pokémon on the field (the Unaware sort still runs for its draws).
    #[track_caller]
    fn resolved_action_speed(&mut self, resolution: Resolution, r: MonRef) -> i32 {
        let m = self.mon(r);
        if resolution == Resolution::ResortWithoutUnaware {
            // `sort_unaware_handlers` would sort nothing.
            if m.is_active && !m.fainted {
                debug_assert_eq!(m.speed, self.action_speed(r), "stale stored speed");
                return m.speed;
            }
            return self.action_speed(r);
        }
        if !(resolution.speeds_current() && m.is_active && !m.fainted) {
            return self.action_speed_draw(r);
        }
        let stored = m.speed;
        self.sort_unaware_handlers();
        debug_assert_eq!(stored, self.action_speed(r), "stale stored speed");
        stored
    }

    /// Showdown `speedSort` for actions.
    #[track_caller]
    pub(super) fn speed_sort(&mut self, list: &mut [Action]) {
        let show = |b: &Battle, list: &[Action]| {
            list.iter()
                .map(|a| {
                    let p = if a.frac == 0 {
                        a.priority.to_string()
                    } else {
                        ((a.priority * 10 + a.frac) as f64 / 10.0).to_string()
                    };
                    format!("{}/{}/{}/{}", b.action_label(a), a.order, p, a.speed)
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let before = if self.rng.trace.is_some() {
            show(self, list)
        } else {
            String::new()
        };
        self.speed_sort_by(list, compare_priority);
        if self.rng.trace.is_some() {
            let after = show(self, list);
            if let Some(t) = &mut self.rng.trace {
                t.push(format!("# queue {before} -> {after}"));
            }
        }
    }

    /// An action as the trace shows it: kind and Pokémon ("move:p1a").
    fn action_label(&self, a: &Action) -> String {
        let who = |s: usize, m: usize| format!("{}{}", self.sides[s].id(), m);
        match a.kind {
            ActionKind::Move { side, mon, .. } => format!("move:{}", who(side, mon)),
            ActionKind::Switch {
                side, mon, insta, ..
            } => format!(
                "{}:{}",
                if insta { "instaswitch" } else { "switch" },
                who(side, mon)
            ),
            ActionKind::MegaEvo { side, mon } => format!("megaEvo:{}", who(side, mon)),
            ActionKind::RunSwitch { side, mon } => format!("runSwitch:{}", who(side, mon)),
            ActionKind::Start => "start:".into(),
            ActionKind::BeforeTurn => "beforeTurn:".into(),
            ActionKind::Residual => "residual:".into(),
            ActionKind::Revive { side, mon, .. } => format!("revivalblessing:{}", who(side, mon)),
            ActionKind::PriorityCharge { side, mon, .. } => {
                format!("priorityChargeMove:{}", who(side, mon))
            }
            ActionKind::BeforeTurnMove { side, mon, .. } => {
                format!("beforeTurnMove:{}", who(side, mon))
            }
        }
    }

    /// Showdown `speedSort`: selection sort by `cmp` (negative when the first
    /// argument goes first), ties shuffled with the PRNG.
    #[track_caller]
    pub(crate) fn speed_sort_by<T>(&mut self, list: &mut [T], cmp: impl Fn(&T, &T) -> i64) {
        if let Some(t) = &mut self.rng.trace {
            let at = std::panic::Location::caller();
            t.push(format!(
                "# sort {} @ {}:{}",
                list.len(),
                at.file(),
                at.line()
            ));
        }
        if list.len() < 2 {
            return;
        }
        let mut sorted = 0;
        while sorted + 1 < list.len() {
            // The first of the best remaining elements.
            let mut best = sorted;
            for i in sorted + 1..list.len() {
                if cmp(&list[best], &list[i]) > 0 {
                    best = i;
                }
            }
            // Move it and every element equal to it, in list order, to the
            // front of the unsorted part. No element before `best` equals
            // it, and it lands at `sorted` before any other is compared.
            list.swap(sorted, best);
            let mut ties = 1;
            for i in best + 1..list.len() {
                if cmp(&list[sorted], &list[i]) == 0 {
                    list.swap(sorted + ties, i);
                    ties += 1;
                }
            }
            if ties > 1 {
                self.rng.shuffle(list, sorted, sorted + ties);
            }
            sorted += ties;
        }
    }

    /// Active Pokémon in Showdown `eachEvent` order: by speed, ties shuffled.
    #[track_caller]
    pub(crate) fn each_event_order(&mut self) -> ActiveList {
        // The sort moves (speed, index into `actives`) pairs: small to swap.
        let mut actives = ActiveList::new();
        let mut keys: PerActive<(i32, u8)> = PerActive::new();
        for (s, side) in self.sides.iter().enumerate() {
            for (slot, m) in side.mons.iter().enumerate().take(side.n_active) {
                if m.is_active && !m.fainted {
                    keys.push((m.speed, actives.len() as u8));
                    actives.push((s, slot));
                }
            }
        }
        self.speed_sort_by(&mut keys, |a, b| b.0 as i64 - a.0 as i64);
        keys.iter().map(|&(_, i)| actives[i as usize]).collect()
    }

    /// `each_event_order` when no handler will run: only its draws and its
    /// trace entry matter. The sort draws only between equal speeds, so
    /// without a tie (and without a trace) it is skipped.
    #[track_caller]
    pub(crate) fn each_event_order_draws(&mut self) {
        if self.rng.trace.is_some() || self.active_speed_tie() {
            self.each_event_order();
        }
    }

    /// Whether two of the Pokémon `each_event_order` sorts have the same speed.
    fn active_speed_tie(&self) -> bool {
        let mut speeds = [0i32; MAX_ACTIVE];
        let mut n = 0;
        for side in &self.sides {
            for m in side.mons.iter().take(side.n_active) {
                if m.is_active && !m.fainted {
                    if speeds[..n].contains(&m.speed) {
                        return true;
                    }
                    if n < MAX_ACTIVE {
                        speeds[n] = m.speed;
                        n += 1;
                    }
                }
            }
        }
        false
    }

    /// Showdown `insertChoice` for an already-resolved action.
    pub(super) fn insert_action(&mut self, action: Action) {
        self.insert_actions(&[action]);
    }

    /// `insertChoice`: the actions go in together, placed by the first.
    fn insert_actions(&mut self, actions: &[Action]) {
        let action = actions[0];
        let mut first = None;
        let mut last = None;
        for (i, cur) in self.queue.iter().enumerate() {
            let c = compare_priority(&action, cur);
            if c <= 0 && first.is_none() {
                first = Some(i);
            }
            if c < 0 {
                last = Some(i);
                break;
            }
        }
        match first {
            None => self.queue.extend(actions.iter().copied()),
            Some(f) => {
                let l = last.unwrap_or(self.queue.len());
                let index = if f == l {
                    f
                } else {
                    self.rng.random_range(f as u32, l as u32 + 1) as usize
                };
                for (k, &a) in actions.iter().enumerate() {
                    self.queue.insert(index + k, a);
                }
            }
        }
    }

    /// Showdown `queue.changeAction(pokemon, {choice: 'move', moveid})`
    /// (Encore): drop the Pokémon's queued actions, update its speed, and
    /// queue the move with a random target.
    pub(crate) fn change_action_to_move(&mut self, t: MonRef, slot: usize) {
        self.queue.retain(|a| match a.kind {
            ActionKind::Move { side, mon, .. } | ActionKind::MegaEvo { side, mon } => {
                (side, mon) != t
            }
            _ => true,
        });
        let sp = self.action_speed_draw(t);
        self.mon_mut(t).speed = sp;
        let mv = self.dex.mv(self.mon(t).moves[slot].id).clone();
        // resolveAction resolves the before-turn callback and the charge
        // callback first (each draws a random target), then the
        // FractionalPriority comes before the move's own random target.
        let before = self.has_before_turn_callback(mv.idx).then(|| {
            self.resolve_action(ActionKind::BeforeTurnMove {
                side: t.0,
                mon: t.1,
                id: mv.idx,
                target: 0,
            })
        });
        let charge = self.priority_charge_move(mv.idx).then(|| {
            self.resolve_action(ActionKind::PriorityCharge {
                side: t.0,
                mon: t.1,
                id: mv.idx,
            })
        });
        let frac = self.run_fractional_priority(t, mv.category == Category::Status);
        let target = self.random_target_loc(t, &mv);
        let action = self.resolve_action_frac(
            ActionKind::Move {
                side: t.0,
                mon: t.1,
                slot,
                target,
                id: mv.idx,
            },
            Some(frac),
            Resolution::New,
        );
        // insertChoice places the whole group by its first action.
        let group: Small<Action> = charge
            .into_iter()
            .chain(before)
            .chain(std::iter::once(action))
            .collect();
        if group.len() > 1 {
            self.insert_actions(&group);
        } else {
            self.insert_action(action);
        }
    }

    /// Showdown `getRandomTarget`, as a target location (doubles): the user
    /// for self and field moves, a random adjacent ally for ally moves, else
    /// a random foe.
    pub(crate) fn random_target_loc(&mut self, r: MonRef, mv: &crate::dex::Move) -> i8 {
        let tt = self.move_target_at_choice(r, mv);
        if let Some(t) = self.get_random_target(r, tt) {
            return self.loc_of(r, t);
        }
        match tt {
            Target::SelfTarget
            | Target::All
            | Target::AllySide
            | Target::AllyTeam
            | Target::AdjacentAllyOrSelf => -(r.1 as i8 + 1),
            Target::AdjacentAlly => {
                let allies = self.allies_of(r);
                if allies.is_empty() {
                    0
                } else {
                    let a = allies[self.rng.random(allies.len() as u32) as usize];
                    -(a.1 as i8 + 1)
                }
            }
            _ => {
                let foes = self.foes_of(r);
                if foes.is_empty() {
                    1
                } else {
                    let f = foes[self.rng.random(foes.len() as u32) as usize];
                    f.1 as i8 + 1
                }
            }
        }
    }

    /// resolveAction's `originalTarget` for a move action.
    pub(crate) fn note_original_target(&mut self, r: MonRef, loc: i8) {
        let target = self.at_loc(r, loc).map(|t| (t.0, self.mon(t).name.clone()));
        let user = (r.0, self.mon(r).name.clone());
        self.turn_state.original_targets.retain(|(u, _)| *u != user);
        self.turn_state.original_targets.push((user, target));
    }
}
fn action_order(kind: &ActionKind) -> u32 {
    match kind {
        ActionKind::Start => 2,
        ActionKind::BeforeTurn => 4,
        ActionKind::RunSwitch { .. } => 101,
        ActionKind::Switch { insta: true, .. } => 3,
        ActionKind::Switch { .. } => 103,
        ActionKind::MegaEvo { .. } => 104,
        ActionKind::Move { .. } => 200,
        ActionKind::Residual => 300,
        ActionKind::Revive { .. } => 6,
        ActionKind::PriorityCharge { .. } => 107,
        ActionKind::BeforeTurnMove { .. } => 5,
    }
}

/// Showdown's `comparePriority`: order asc, priority desc, speed desc.
/// Returns negative if `a` goes first, positive if `b` does, 0 on a tie.
fn compare_priority(a: &Action, b: &Action) -> i64 {
    let ao = if a.order == 0 {
        4294967296
    } else {
        a.order as i64
    };
    let bo = if b.order == 0 {
        4294967296
    } else {
        b.order as i64
    };
    let d = ao - bo;
    if d != 0 {
        return d;
    }
    let d = (b.priority as i64 * 10 + b.frac as i64) - (a.priority as i64 * 10 + a.frac as i64);
    if d != 0 {
        return d;
    }
    b.speed as i64 - a.speed as i64
}
