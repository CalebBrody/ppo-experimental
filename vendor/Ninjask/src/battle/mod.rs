//! The battle: its state, the turn loop and Showdown's `runAction`. Choices and
//! requests are in choices.rs, the action queue in queue.rs, switching and
//! fainting in switching.rs, the log in log.rs. Moves are in moves/, conditions
//! in conditions/ and events in events/.
//!
//! Each function names the Showdown code it ports. Pokémon are addressed by
//! (side, index into `side.mons`); index == position for active Pokémon.

mod enumerate;
mod state_key;
pub use state_key::BattleStateKey;

mod bookkeeping;
mod choices;
pub mod log;
mod queries;
mod queue;
mod switching;

pub use queries::Grounded;

use std::collections::VecDeque;
use std::sync::Arc;

use arrayvec::ArrayVec;

use crate::dex::team::{champions_max_pp, champions_stats, open_sheet, parse_packed, MonSet};
use crate::dex::types::*;
use crate::dex::Dex;
use crate::kinds::AbilityKind;
use crate::moves::damage;
use crate::rng::Rng;
use crate::state::*;
use log::LogEntry;
use queue::Resolution;

/// A reference to a Pokémon: (side, index in side.mons).
pub type MonRef = (usize, usize);

/// One value per active Pokémon at most, held inline.
pub type PerActive<T> = ArrayVec<T, MAX_ACTIVE>;
/// A list of active Pokémon.
pub type ActiveList = PerActive<MonRef>;
/// A short list (event handlers, the Pokémon a move reached) held inline,
/// spilling to the heap past `N`.
pub(crate) type Small<T, const N: usize = 4> = smallvec::SmallVec<[T; N]>;

/// Showdown's prefix for a choice refused on information the request hid.
pub const UNAVAILABLE: &str = "[Unavailable choice] ";

#[derive(Clone, Debug)]
pub struct Battle {
    // Core
    pub dex: Arc<Dex>,
    pub rng: Rng,
    pub sides: [Side; 2],
    pub field: Field,
    pub turn: u32,
    /// Showdown's `battle.queue`: actions run from the front.
    pub queue: VecDeque<Action>,
    pub mid_turn: bool,
    pub request: RequestState,
    /// Request counter, sent as `rqid`.
    pub rqid: u32,
    /// Which slots need a switch in the current switch request.
    force_switch: [[bool; 2]; 2],
    pub ended: bool,
    /// Winning side, or `None` for a tie (when `ended`).
    pub winner: Option<usize>,
    /// Side of the last Pokémon to faint (for simultaneous-loss tiebreaks).
    last_fainted_side: Option<usize>,
    faint_queue: Vec<MonRef>,

    // Log
    pub log: Vec<LogEntry>,
    pub log_enabled: bool,
    /// Index of the last `|move|` line (Showdown `lastMoveLine`), for `[still]` and retargeting.
    pub(crate) last_move_line: Option<usize>,

    // The move in use
    /// The hit being dealt by a multi-hit move (Showdown `move.hit`).
    pub(crate) move_hit: u8,
    /// The move being used still strikes its target and the target's ally
    /// (Showdown `move.smartTarget`, Dragon Darts). Redirection and Protect
    /// turn it off.
    pub(crate) smart_target: bool,
    /// Set just before a bounced move is used (Magic Bounce): the original
    /// move's priority, which the bounced one keeps (`priority_override`
    /// while it runs).
    pub(crate) bounce: Option<i8>,
    pub(crate) priority_override: Option<(MonRef, MoveId, i8)>,
    /// The Mold Breaker user whose move is the active move (Showdown
    /// `activeMove.ignoreAbility`), until the end of its action.
    pub(crate) ability_breaker: Option<MonRef>,
    /// Where the Pokémon using a move ended up after Ally Switch moved it.
    pub(crate) relocated: Option<(MonRef, MonRef)>,
    /// A read-only calculation is running on this battle (`crate::calc`):
    /// effects that would change the state or draw for an outcome (a resist
    /// berry eaten, Fickle Beam's roll) are evaluated without doing so.
    /// Always false in play.
    pub(crate) probe: bool,
    /// When set, every damaging hit's predicted distribution and actual
    /// damage are recorded (`crate::calc::DamageSample`), to test the
    /// calculation against played outcomes. Off in play.
    pub damage_samples: Option<Box<Vec<crate::calc::DamageSample>>>,
    /// Counter behind `Volatiles::added`.
    pub(crate) vol_stamp: u32,

    // Slot and side conditions, the move in use, and move bookkeeping
    /// Slot conditions (Wish, Healing Wish, Future Sight), by side and slot.
    pub(crate) slot_conds: [[crate::state::SlotConds; 2]; 2],
    pub(crate) side_extras: [crate::state::SideExtras; 2],
    pub(crate) active_move: crate::state::ActiveMove,
    pub(crate) turn_state: crate::state::TurnState,
}

/// A team prepared for battle: its Pokémon as built from the sets, before
/// anything random (an unset gender is drawn when a battle starts), and its
/// open team sheet. Preparing a pool of teams once makes building each
/// battle from them (`Battle::from_teams`) a copy.
#[derive(Clone, Debug)]
pub struct Team {
    mons: Vec<Mon>,
    sheet: String,
}

impl Team {
    pub fn new(dex: &Dex, sets: &[MonSet]) -> Team {
        Team {
            mons: sets
                .iter()
                .enumerate()
                .map(|(p, s)| build_mon(dex, s, p))
                .collect(),
            sheet: open_sheet(dex, sets),
        }
    }
}

pub(crate) fn build_mon(dex: &Dex, set: &MonSet, position: usize) -> Mon {
    let sp = dex.species(set.species);
    let nature = dex.nature(&set.nature);
    let stats = champions_stats(&sp.base_stats, &set.points, nature);
    let moves = set
        .moves
        .iter()
        .map(|&id| {
            let max_pp = champions_max_pp(dex.mv(id).pp, dex.mv(id).no_pp_boosts);
            MoveSlot {
                id,
                pp: max_pp,
                max_pp,
                disabled: false,
                used: false,
            }
        })
        .collect();
    Mon {
        // Showdown's Pokemon constructor: no nickname, or the species name,
        // means the base species ("Indeedee-F" is called "Indeedee").
        name: if set.nickname.is_empty() || set.nickname == sp.name {
            sp.base_species.as_str().into()
        } else {
            set.nickname.as_str().into()
        },
        species: set.species,
        base_species: set.species,
        types: sp.types,
        level: set.level,
        gender: set.gender.clone(),
        shiny: set.shiny,
        revived: false,
        hurt_this_turn: None,
        nature_name: crate::dex::team::nature_name(&set.nature),
        nature_plus: nature.plus,
        nature_minus: nature.minus,
        points: set.points,
        stats,
        hp: stats[0],
        maxhp: stats[0],
        status: Status::None,
        status_time: 0,
        status_start_time: 0,
        status_state: Status::None,
        toxic_stage: 0,
        boosts: [0; 7],
        item: set.item,
        last_item: None,
        used_item_this_turn: false,
        ate_berry: false,
        syrup_triggered: false,
        ability: set.ability,
        base_ability: set.ability,
        ability_state: AbilityState::default(),
        ability_order: 0,
        moves,
        vol: Volatiles::default(),
        last_move: None,
        last_move_target_loc: 0,
        move_this_turn_result: None,
        move_last_turn_result: None,
        position,
        is_active: false,
        fainted: false,
        faint_queued: false,
        switch_flag: false,
        force_switch_flag: false,
        switch_from_move: None,
        skip_before_switch_out: false,
        mega_evolved: false,
        trapped: Trapped::No,
        maybe_trapped: false,
        times_attacked: 0,
        active_turns: 0,
        moved_this_turn: false,
        active_move_actions: 0,
        // The constructor's clearVolatile -> setSpecies: the Speed stat.
        speed: stats[Stat::Spe as usize] as i32,
        illusion: None,
        hero_message_displayed: false,
        added_type: NO_TYPE,
        transformed: None,
        attacked_by: Vec::new(),
        stats_raised_this_turn: false,
        stats_lowered_this_turn: false,
        set_species: set.species,
    }
}

impl Battle {
    /// Create a battle at team preview. Teams are Showdown packed strings.
    pub fn new(
        dex: Arc<Dex>,
        teams: [&str; 2],
        names: [&str; 2],
        seed: u64,
    ) -> Result<Battle, String> {
        Battle::new_with_rng(dex, teams, names, Rng::new(seed))
    }

    /// `new` with a given generator (reference mode: Showdown's), which
    /// the team building already draws from.
    pub fn new_with_rng(
        dex: Arc<Dex>,
        teams: [&str; 2],
        names: [&str; 2],
        rng: Rng,
    ) -> Result<Battle, String> {
        let a = parse_packed(&dex, teams[0])?;
        let b = parse_packed(&dex, teams[1])?;
        Ok(Battle::from_sets_rng(dex, [&a, &b], names, rng))
    }

    /// Create a battle at team preview from parsed teams.
    pub fn from_sets(dex: Arc<Dex>, teams: [&[MonSet]; 2], names: [&str; 2], seed: u64) -> Battle {
        Battle::from_sets_rng(dex, teams, names, Rng::new(seed))
    }

    pub fn from_sets_rng(
        dex: Arc<Dex>,
        teams: [&[MonSet]; 2],
        names: [&str; 2],
        rng: Rng,
    ) -> Battle {
        let teams = teams.map(|sets| Team::new(&dex, sets));
        Battle::from_team_parts(dex, teams, names, rng)
    }

    /// Create a battle at team preview from prepared teams: the same
    /// battle as `from_sets` with the teams' sets, without building
    /// their Pokémon and sheets again.
    pub fn from_teams(dex: Arc<Dex>, teams: [&Team; 2], names: [&str; 2], seed: u64) -> Battle {
        Battle::from_teams_rng(dex, teams, names, Rng::new(seed))
    }

    pub fn from_teams_rng(dex: Arc<Dex>, teams: [&Team; 2], names: [&str; 2], rng: Rng) -> Battle {
        Battle::from_team_parts(dex, teams.map(Team::clone), names, rng)
    }

    fn from_team_parts(dex: Arc<Dex>, teams: [Team; 2], names: [&str; 2], mut rng: Rng) -> Battle {
        let mut sides = Vec::new();
        for (i, team) in teams.into_iter().enumerate() {
            let Team { mut mons, sheet } = team;
            // Showdown's Pokemon constructor: the set's gender, else the
            // species' fixed one, else a random draw (side by side, in team order).
            for m in mons.iter_mut() {
                if m.gender != "M" && m.gender != "F" && m.gender != "N" {
                    let fixed = dex.species(m.species).gender.clone();
                    m.gender = if fixed.is_empty() {
                        rng.sample(&["M", "F"]).to_string()
                    } else {
                        fixed
                    };
                }
                if m.gender == "N" {
                    m.gender.clear();
                }
            }
            let n = mons.len();
            sides.push(Side {
                index: i,
                name: names[i].to_string(),
                mons,
                n_active: if dex.format.doubles { 2 } else { 1 },
                pokemon_left: n,
                mega_used: false,
                total_fainted: 0,
                conds: SideConditions::default(),
                choice: None,
                team_preview_choice: None,
                reveals: Vec::new(),
                reveals_rqid: 0,
                revival: [false; 2],
                sheet,
            });
        }
        let sides: [Side; 2] = [sides.remove(0), sides.remove(0)];
        let mut b = Battle {
            dex,
            rng,
            sides,
            field: Field::default(),
            turn: 0,
            // Showdown's start() queues the 'start' action before team preview.
            queue: VecDeque::from([Action {
                kind: ActionKind::Start,
                order: 2,
                priority: 0,
                speed: 1,
                frac: 0,
            }]),
            mid_turn: false,
            request: RequestState::TeamPreview,
            ended: false,
            winner: None,
            log: Vec::new(),
            log_enabled: true,
            faint_queue: Vec::new(),
            last_fainted_side: None,
            force_switch: [[false; 2]; 2],
            rqid: 1,
            last_move_line: None,
            vol_stamp: 0,
            move_hit: 0,
            smart_target: false,
            bounce: None,
            priority_override: None,
            ability_breaker: None,
            slot_conds: Default::default(),
            side_extras: Default::default(),
            active_move: Default::default(),
            turn_state: Default::default(),
            relocated: None,
            probe: false,
            damage_samples: None,
        };
        b.add(format!(
            "|gametype|{}",
            if b.dex.format.doubles {
                "doubles"
            } else {
                "singles"
            }
        ));
        for s in 0..2 {
            b.add(format!("|player|{}|{}||", b.sides[s].id(), b.sides[s].name));
        }
        b.add(format!("|tier|{}", b.dex.format.name));
        b.add("|clearpoke");
        for s in 0..2 {
            for i in 0..b.sides[s].mons.len() {
                let mut line = String::with_capacity(48);
                line.push_str("|poke|");
                line.push_str(b.sides[s].id());
                line.push('|');
                b.write_details(s, i, &mut line);
                line.push('|');
                b.add(line);
            }
        }
        let size = b
            .dex
            .format
            .picked_team_size
            .unwrap_or(b.sides[0].mons.len());
        b.add(format!("|teampreview|{size}"));
        b
    }

    /// A battle with two empty sides, for importing states.
    pub fn skeleton(dex: Arc<Dex>, names: [&str; 2], seed: u64) -> Battle {
        let n_active = if dex.format.doubles { 2 } else { 1 };
        let side = |i: usize| Side {
            index: i,
            name: names[i].to_string(),
            mons: Vec::new(),
            n_active,
            pokemon_left: 0,
            mega_used: false,
            total_fainted: 0,
            conds: SideConditions::default(),
            choice: None,
            team_preview_choice: None,
            reveals: Vec::new(),
            reveals_rqid: 0,
            revival: [false; 2],
            sheet: String::new(),
        };
        Battle {
            dex,
            rng: Rng::new(seed),
            sides: [side(0), side(1)],
            field: Field::default(),
            turn: 0,
            queue: VecDeque::new(),
            mid_turn: false,
            request: RequestState::Move,
            ended: false,
            winner: None,
            log: Vec::new(),
            log_enabled: true,
            faint_queue: Vec::new(),
            last_fainted_side: None,
            force_switch: [[false; 2]; 2],
            rqid: 1,
            last_move_line: None,
            vol_stamp: 0,
            move_hit: 0,
            smart_target: false,
            bounce: None,
            priority_override: None,
            ability_breaker: None,
            slot_conds: Default::default(),
            side_extras: Default::default(),
            active_move: Default::default(),
            turn_state: Default::default(),
            relocated: None,
            probe: false,
            damage_samples: None,
        }
    }

    /// Record that a volatile with a residual handler was just added.
    /// A fresh `abilityState` (switch-in, an ability change), stamped.
    pub(crate) fn reset_ability_state(&mut self, r: MonRef) {
        self.vol_stamp += 1;
        let n = self.vol_stamp;
        let m = self.mon_mut(r);
        m.ability_state = AbilityState::default();
        m.ability_order = n;
    }

    pub(crate) fn stamp_volatile(&mut self, r: MonRef, v: ResidualVol) {
        self.vol_stamp += 1;
        let n = self.vol_stamp;
        self.mon_mut(r).vol.added[v as usize] = n;
    }

    pub(crate) fn set_force_switch(&mut self, s: usize, flags: &[bool]) {
        for (i, f) in flags.iter().enumerate().take(2) {
            self.force_switch[s][i] = *f;
        }
    }

    pub(crate) fn refresh_speeds(&mut self) {
        self.update_speed();
    }

    /// Whether `slot` of side `s` must switch (forceSwitch).
    pub fn force_switch_flag(&self, s: usize, slot: usize) -> bool {
        self.force_switch[s][slot]
    }

    pub(crate) fn target_ok(&self, s: usize, slot: usize, loc: i8, target: Target) -> bool {
        self.valid_target_loc(s, slot, loc, target)
    }

    /// Open team sheets: show both teams' sets (Showdown `showOpenTeamSheets`,
    /// called at team preview).
    pub fn show_team_sheets(&mut self) {
        for s in 0..2 {
            let line = format!("|showteam|{}|{}", self.sides[s].id(), self.sides[s].sheet);
            self.add(line);
        }
    }

    /// Fast path for self-play: submit an already-legal structured choice.
    pub fn choose_structured(&mut self, s: usize, choice: Vec<SlotChoice>) {
        self.sides[s].choice = Some(choice);
        if self.all_choices_done() {
            self.commit();
        }
    }

    /// Fast path for team preview: an order of team indices (0-based).
    pub fn choose_team(&mut self, s: usize, order: Vec<usize>) {
        let n = self.sides[s].mons.len();
        let want = self.dex.format.picked_team_size.unwrap_or(n).min(n);
        let mut o = order;
        o.truncate(want);
        self.sides[s].team_preview_choice = Some(o);
        if self.all_choices_done() {
            self.commit();
        }
    }

    pub fn mon(&self, r: MonRef) -> &Mon {
        &self.sides[r.0].mons[r.1]
    }

    pub(crate) fn mon_mut(&mut self, r: MonRef) -> &mut Mon {
        &mut self.sides[r.0].mons[r.1]
    }

    /// Active Pokémon on a side, by slot (None for an empty slot).
    pub fn active(&self, s: usize, slot: usize) -> Option<MonRef> {
        let side = &self.sides[s];
        if slot < side.mons.len() && side.mons[slot].is_active {
            Some((s, slot))
        } else {
            None
        }
    }

    /// Non-fainted active Pokémon, side 0 first.
    #[inline]
    pub fn all_active(&self) -> ActiveList {
        // A plain loop: this runs for nearly every event, and an iterator
        // chain collected into an ArrayVec costs several times as much.
        let mut out = ActiveList::new();
        for (s, side) in self.sides.iter().enumerate() {
            for (slot, m) in side.mons.iter().enumerate().take(side.n_active) {
                if m.is_active && !m.fainted {
                    out.push((s, slot));
                }
            }
        }
        out
    }

    /// Whether `f` holds for a non-fainted active Pokémon (those
    /// `all_active` lists), without building the list.
    #[inline]
    pub(crate) fn any_active(&self, f: impl Fn(&Battle, MonRef) -> bool) -> bool {
        for (s, side) in self.sides.iter().enumerate() {
            for (slot, m) in side.mons.iter().enumerate().take(side.n_active) {
                if m.is_active && !m.fainted && f(self, (s, slot)) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn can_switch_side(&self, s: usize) -> bool {
        self.can_switch(s)
    }

    fn can_switch(&self, s: usize) -> bool {
        let side = &self.sides[s];
        side.mons
            .iter()
            .enumerate()
            .any(|(i, m)| i >= side.n_active && !m.fainted)
    }

    /// Showdown `getStat(stat, unboosted=false, unmodified=false)` for battle stats.
    pub fn get_stat(&self, r: MonRef, stat: Stat, boosted: bool, modified: bool) -> u32 {
        let m = self.mon(r);
        let mut v = m.stats[stat as usize] as u32;
        if boosted {
            v = damage::apply_boost(v, m.boosts[Boost::of_stat(stat) as usize]);
        }
        if modified && stat == Stat::Spe {
            v = self.modify_speed(r, v);
        }
        if stat == Stat::Spe && v > 10000 {
            v = 10000;
        }
        v
    }

    /// Champions `getActionSpeed`: speed, negated under Trick Room (no underflow).
    pub fn action_speed(&self, r: MonRef) -> i32 {
        let speed = self.get_stat(r, Stat::Spe, true, true) as i32;
        if self.field.trick_room > 0 {
            -speed
        } else {
            speed
        }
    }

    /// `getActionSpeed` as Showdown runs it: `getStat('spe')` fires the
    /// `ModifyBoost` event, whose `onAnyModifyBoost` handlers (Unaware, one per
    /// active holder, at the holder's stored speed) join a sort that draws
    /// when two of them tie. Only an active Pokémon has handlers.
    #[track_caller]
    pub fn action_speed_draw(&mut self, r: MonRef) -> i32 {
        if self.mon(r).is_active {
            self.sort_unaware_handlers();
        }
        self.action_speed(r)
    }

    /// The stored speeds of the active holders of an `onAnyModifyBoost`
    /// handler (Unaware), counted as an event sort counts them: a
    /// suppressed one still sorts.
    #[inline]
    fn unaware_handler_speeds(&mut self) -> PerActive<i32> {
        // Almost always nobody holds Unaware: answer that without building
        // the lists.
        if !self.any_unaware_holder() {
            return PerActive::new();
        }
        self.unaware_holder_speeds()
    }

    /// Whether an active Pokémon has Unaware (suppressed or not).
    #[inline]
    fn any_unaware_holder(&self) -> bool {
        self.any_active(|b, r| {
            matches!(
                b.dex.ability_kind[b.mon(r).ability as usize],
                AbilityKind::Unaware
            )
        })
    }

    /// `unaware_handler_speeds` once a holder is on the field.
    #[cold]
    #[inline(never)]
    fn unaware_holder_speeds(&mut self) -> PerActive<i32> {
        let mut holders: PerActive<(MonRef, i32)> = PerActive::new();
        for (s, side) in self.sides.iter().enumerate() {
            for (slot, m) in side.mons.iter().enumerate().take(side.n_active) {
                let unaware = matches!(
                    self.dex.ability_kind[m.ability as usize],
                    AbilityKind::Unaware
                );
                if unaware && m.is_active && !m.fainted {
                    holders.push(((s, slot), m.speed));
                }
            }
        }
        let mut speeds = PerActive::new();
        if !holders.is_empty() {
            self.turn_state.sorting = true;
            for (h, speed) in holders {
                if !self.ability_ignored(h) {
                    speeds.push(speed);
                }
            }
            self.turn_state.sorting = false;
        }
        speeds
    }

    /// The `ModifyBoost` sort over the Unaware holders, at their stored
    /// speeds: only its draws matter.
    #[track_caller]
    pub(super) fn sort_unaware_handlers(&mut self) {
        // Fewer than two holders: no sort, so no draw and no trace entry.
        if !self.any_unaware_holder() {
            return;
        }
        let mut speeds = self.unaware_handler_speeds();
        if speeds.len() > 1 {
            self.speed_sort_by(&mut speeds, |a, b| (*b - *a) as i64);
        }
    }

    /// Showdown `updateSpeed`: each active Pokémon's stored speed, in order,
    /// each through `action_speed_draw`. Who holds Unaware cannot change
    /// while it runs, so with fewer than two holders no sort can draw and
    /// the holders are not looked up again.
    fn update_speed(&mut self) {
        let sorts = self.unaware_handler_speeds().len() > 1;
        for r in self.all_active() {
            if sorts {
                self.sort_unaware_handlers();
            }
            let sp = self.action_speed(r);
            self.mon_mut(r).speed = sp;
        }
    }

    /// Showdown `turnLoop`: run queued actions until a request or the end.
    fn run(&mut self) {
        self.add("|");
        while let Some(action) = self.queue.pop_front() {
            let stop = self.run_action(action);
            if stop || self.ended {
                return;
            }
        }
        self.end_turn();
        self.mid_turn = false;
        self.queue.clear();
    }

    /// Showdown `runAction`. Returns true when a request interrupts the turn.
    fn run_action(&mut self, action: Action) -> bool {
        // HP of the active Pokémon before the residual phase (for Emergency Exit).
        let mut residual_hp: PerActive<(MonRef, u16)> = PerActive::new();
        // HP of the first Pokémon of a runSwitch before its entry (hazards).
        let mut switch_hp: Option<(MonRef, u16)> = None;
        match action.kind {
            ActionKind::Start => {
                for s in 0..2 {
                    let n = self.sides[s].mons.len();
                    log_line!(self, "|teamsize|{}|{}", self.sides[s].id(), n);
                }
                self.add("|start");
                for s in 0..2 {
                    for slot in 0..self.sides[s].n_active.min(self.sides[s].mons.len()) {
                        self.switch_in(s, slot, slot, false, None);
                    }
                }
                self.mid_turn = true;
            }
            // eachEvent('BeforeTurn'): no handler acts here, but the sort draws.
            ActionKind::BeforeTurn => {
                self.each_event_order_draws();
            }
            ActionKind::MegaEvo { side, mon } => self.run_mega_evo((side, mon)),
            ActionKind::PriorityCharge { side, mon, id } => {
                let m = self.mon((side, mon));
                if !m.is_active || m.fainted || m.revived || m.position != mon {
                    return false;
                }
                if self.has_priority_charge_callback(id) {
                    self.priority_charge_callback((side, mon), id);
                } else {
                    self.chilly_reception_priority_charge((side, mon), id);
                }
            }
            ActionKind::BeforeTurnMove {
                side,
                mon,
                id,
                target,
            } => {
                let r = (side, mon);
                let m = self.mon(r);
                if !m.is_active || m.fainted || m.revived || m.position != mon {
                    return false;
                }
                let tt = self.dex.mv(id).target;
                let future = self.dex.mv(id).has(MoveFlag::FutureMove);
                if self.get_target(r, tt, target, false, future).is_none() {
                    return false;
                }
                self.run_before_turn(r, id);
            }
            ActionKind::Move {
                side,
                mon,
                slot,
                target,
                id,
            } => {
                let r = (side, mon);
                // A Round that another Round moved up carries its sourceEffect.
                if let Some(i) = self.turn_state.round_actions.iter().position(|&x| x == r) {
                    self.turn_state.round_actions.remove(i);
                    self.turn_state.round_source = true;
                }
                let m = self.mon(r);
                if !m.is_active || m.fainted || m.revived || m.position != mon {
                    self.turn_state.round_source = false;
                    // The Pokémon left the field or fainted: Showdown's runAction
                    // returns at once, without the end-of-action steps.
                    return false;
                }
                self.run_move(r, slot, target, id);
                self.turn_state.round_source = false;
            }
            ActionKind::Switch {
                side,
                mon,
                target,
                from,
                ..
            } => {
                // `mon` was the index of the leaving Pokémon at choice time,
                // which is its slot; switch whatever occupies that slot.
                let slot = mon;
                let m = &self.sides[side].mons[target];
                if (!m.is_active || m.revived) && !m.fainted {
                    self.switch_in(side, target, slot, false, from);
                }
            }
            ActionKind::Revive { side, mon, target } => self.revive(side, mon, target),
            ActionKind::RunSwitch { side, mon } => {
                switch_hp = Some(((side, mon), self.mon((side, mon)).hp));
                // Batch consecutive runSwitch actions (Showdown `runSwitch`).
                let mut switchers: Small<MonRef> = smallvec::smallvec![(side, mon)];
                while let Some(ActionKind::RunSwitch { side, mon }) =
                    self.queue.front().map(|a| a.kind)
                {
                    self.queue.pop_front();
                    switchers.push((side, mon));
                }
                self.run_switch(&switchers);
            }
            ActionKind::Residual => {
                self.add("|");
                // clearActiveMove(true).
                self.active_move.cur_move = None;
                self.active_move.cur_user = None;
                self.update_speed();
                residual_hp = self
                    .all_active()
                    .into_iter()
                    .map(|r| (r, self.mon(r).hp))
                    .collect();
                self.run_residual();
                if !self.ended {
                    self.add("|upkeep");
                }
            }
        }

        // Phazing (Roar, Dragon Tail): drag in a random teammate.
        for s in 0..2 {
            for slot in 0..self.sides[s].n_active {
                let Some(r) = self.active(s, slot) else {
                    continue;
                };
                if self.mon(r).force_switch_flag {
                    self.mon_mut(r).force_switch_flag = false;
                    if self.mon(r).hp > 0 {
                        self.drag_in(s, slot);
                    }
                }
            }
        }
        // clearActiveMove.
        self.ability_breaker = None;
        self.clear_active_move();

        self.faint_messages();
        if self.ended {
            return true;
        }
        if self.queue.is_empty() {
            self.check_fainted();
        } else if matches!(self.queue[0].kind, ActionKind::Switch { insta: true, .. }) {
            // More replacements to bring in first (Showdown returns early).
            return false;
        }
        // Gen 5+: eachEvent('Update') after every action but the start; then
        // Emergency Exit for damage taken in the residual phase.
        if !matches!(action.kind, ActionKind::Start) {
            self.run_update();
            for (r, hp) in residual_hp {
                self.emergency_exit(r, hp);
            }
        }
        // Then for the runSwitch action's Pokémon (only the first of a
        // batch): hazard damage on entry.
        if let Some((r, hp)) = switch_hp {
            self.emergency_exit(r, hp);
        }

        // Switch requests (fainted Pokémon to replace).
        let mut any = false;
        for s in 0..2 {
            let mut flags = [false; 2];
            let mut need = false;
            // `slot` indexes `flags` and names the active slot at once.
            #[allow(clippy::needless_range_loop)]
            for slot in 0..self.sides[s].n_active {
                if let Some(r) = self.active(s, slot) {
                    if self.mon(r).switch_flag {
                        flags[slot] = true;
                        need = true;
                    }
                }
            }
            if need && !self.can_switch(s) {
                // Revival Blessing's fake switch stays (`reviveSwitch`).
                let mut revive = false;
                #[allow(clippy::needless_range_loop)]
                for slot in 0..self.sides[s].n_active {
                    if let Some(r) = self.active(s, slot) {
                        if self.sides[s].revival[slot] {
                            revive = true;
                            continue;
                        }
                        self.mon_mut(r).switch_flag = false;
                        flags[slot] = false;
                    }
                }
                if !revive {
                    need = false;
                    flags = [false; 2];
                }
            } else if need {
                // BeforeSwitchOut runs now for a Pokémon leaving on its own
                // (U-turn, Eject Button...), and not again when it switches.
                // No supported effect handles it.
                for slot in 0..self.sides[s].n_active {
                    if let Some(r) = self.active(s, slot) {
                        // Not for Revival Blessing's fake switch.
                        let revival = self.sides[s].revival[slot];
                        let m = self.mon_mut(r);
                        if m.hp > 0 && m.switch_flag && !revival {
                            m.skip_before_switch_out = true;
                        }
                    }
                }
            }
            self.force_switch[s] = flags;
            any |= need;
        }
        if any {
            self.request = RequestState::Switch;
            self.rqid += 1;
            return true;
        }

        // Gen 8+: re-sort remaining moves by current speed.
        if matches!(
            self.queue.front().map(|a| a.kind),
            Some(ActionKind::Move { .. })
        ) {
            self.update_speed();
            let mut rest = std::mem::take(&mut self.queue);
            // Resolving queued actions changes no ability and no active
            // Pokémon: whether anyone holds Unaware holds for the loop.
            let resolution = if self.any_unaware_holder() {
                Resolution::Resort
            } else {
                Resolution::ResortWithoutUnaware
            };
            for a in rest.iter_mut() {
                let resolved = self.resolve_action_frac(a.kind, Some(a.frac), resolution);
                a.priority = resolved.priority;
                a.speed = resolved.speed;
            }
            self.speed_sort(rest.make_contiguous());
            self.queue = rest;
        }
        false
    }

    /// Showdown `checkFainted`: fainted actives must be replaced.
    fn check_fainted(&mut self) {
        for s in 0..2 {
            for slot in 0..self.sides[s].n_active {
                if let Some(r) = self.active(s, slot) {
                    if self.mon(r).fainted {
                        let m = self.mon_mut(r);
                        m.status = Status::Fnt;
                        m.switch_flag = true;
                    }
                }
            }
        }
    }

    /// Showdown `endTurn` (the parts that matter so far) and a move request.
    fn end_turn(&mut self) {
        self.turn += 1;
        let turn = self.turn;
        for s in 0..2 {
            for slot in 0..self.sides[s].n_active {
                if let Some(r) = self.active(s, slot) {
                    let m = self.mon_mut(r);
                    m.moved_this_turn = false;
                    m.hurt_this_turn = None;
                    // nextTurn skips this at turn 1.
                    if turn != 1 {
                        m.used_item_this_turn = false;
                    }
                    m.move_last_turn_result = m.move_this_turn_result.take();
                    self.disable_move_event(r);
                    // choicelock's onDisableMove ends the lock once the Pokémon
                    // has no Choice item or no longer has the move.
                    if let Some(lock) = self.mon(r).vol.choice_lock {
                        let choice = self.held_item_kind(r).is_some_and(|k| k.is_choice());
                        if !choice || !self.mon(r).moves.iter().any(|ms| ms.id == lock) {
                            self.mon_mut(r).vol.choice_lock = None;
                        }
                    }
                    self.store_disabled_moves(r);
                    self.sync_apparent_type(r);
                    self.run_trap_pokemon(r, true);
                    self.reset_turn_flags(r);
                    if !self.mon(r).fainted {
                        self.mon_mut(r).active_turns += 1;
                    }
                }
            }
        }
        log_line!(self, "|turn|{}", self.turn);
        self.request = RequestState::Move;
        self.rqid += 1;
    }

    /// `runEvent('DisableMove', pokemon)` at the end of the turn. The
    /// engine works out disabled moves when asked (`move_disabled`), but the
    /// handlers still sort: the Pokémon's volatiles (Choice lock, Disable,
    /// Encore, Taunt, Throat Chop, Heal Block, Torment) and its foes' Imprison. Equal
    /// ones draw.
    fn disable_move_event(&mut self, r: MonRef) {
        use crate::abilities::{compare_mod, ModHandler};
        let m = self.mon(r);
        if m.fainted || m.hp == 0 {
            return;
        }
        let own = [
            m.vol.choice_lock.is_some(),
            m.vol.disable.is_some(),
            m.vol.encore.is_some(),
            m.vol.taunt > 0,
            m.vol.throat_chop > 0,
            m.vol.heal_block > 0,
            m.vol.torment,
        ];
        let mut keys: Small<ModHandler> = own
            .iter()
            .filter(|&&on| on)
            .map(|_| ModHandler::new(0, m.speed, 2))
            .collect();
        for f in self.foes_of(r) {
            if self.mon(f).vol.imprison {
                keys.push(ModHandler::new(0, self.mon(f).speed, 2));
            }
        }
        if keys.len() > 1 {
            self.speed_sort_by(&mut keys, compare_mod);
        }
    }

    /// Showdown `runMegaEvo` + Champions `formeChange` (permanent).
    fn run_mega_evo(&mut self, r: MonRef) {
        if self.sides[r.0].mega_used {
            return;
        }
        let Some(mega) = self.can_mega_evo(r) else {
            return;
        };
        let item_name = self
            .mon(r)
            .item
            .map(|i| self.dex.item(i).name.clone())
            .unwrap_or_default();
        let dex = self.dex.clone();
        let sp = dex.species(mega);
        {
            let m = self.mon_mut(r);
            // formeChange(isPermanent): the Mega becomes the base species.
            m.species = mega;
            m.base_species = mega;
            m.types = sp.types;
            m.added_type = NO_TYPE;
            let nature = crate::dex::Nature {
                plus: m.nature_plus,
                minus: m.nature_minus,
            };
            let stats = champions_stats(&sp.base_stats, &m.points, nature);
            // HP doesn't change on Mega Evolution.
            for st in [Stat::Atk, Stat::Def, Stat::Spa, Stat::Spd, Stat::Spe] {
                m.stats[st as usize] = stats[st as usize];
            }
            m.mega_evolved = true;
            // Mega Evolution counts as an action (Truant).
            m.move_this_turn_result = Some(true);
            // setSpecies resets the stored speed to the new Speed stat.
            m.speed = m.stats[Stat::Spe as usize] as i32;
        }
        self.sides[r.0].mega_used = true;
        let ident = self.ident(r);
        let details = self.details(r.0, r.1);
        log_line!(self, "|detailschange|{ident}|{details}");
        // The species the opponent sees: the Mega's base species ("Floette").
        let apparent = self.dex.species(self.mon(r).species).base_species.clone();
        log_line!(self, "|-mega|{ident}|{apparent}|{item_name}");
        // formeChange -> setAbility, after the messages: the old ability's
        // End (Flash Fire), then the new one's Start (Intimidate, Drought...).
        if let Some((_, ab)) = sp.abilities.first() {
            if let Some(a) = self.dex.ability_id(ab) {
                self.ability_on_end(r);
                let m = self.mon_mut(r);
                m.ability = a;
                m.base_ability = a;
                self.reset_ability_state(r);
            }
        }
        self.run_start(r);
        // AfterMega: White Herb's onAnyAfterMega.
        self.white_herbs_any();
    }

    /// Winner as a player name, like Showdown's `battle.winner`.
    pub fn winner_name(&self) -> Option<&str> {
        self.winner.map(|s| self.sides[s].name.as_str())
    }
}
