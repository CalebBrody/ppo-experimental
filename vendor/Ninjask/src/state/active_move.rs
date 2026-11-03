//! The move in use: its hit data, and the turn-scoped move bookkeeping.

use crate::battle::{ActiveList, MonRef};
use crate::dex::types::*;

/// A Pokémon identified by its side and name, as a queued action refers to it.
pub type SideName = (usize, std::sync::Arc<str>);

/// What the battle keeps about the move in use: the Showdown `activeMove` and its
/// hit data.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActiveMove {
    /// Showdown `activeMove` (its id) while a move action runs: what
    /// `clearActiveMove` makes the battle's `lastMove` unless a failure cleared it.
    pub cur_move: Option<MoveId>,
    /// Showdown `activePokemon`, set and cleared with `cur_move` (Mega Sol).
    pub cur_user: Option<MonRef>,
    /// The move last used and its type after ModifyType (Showdown's
    /// `activeMove`), for the user's `AfterMove` handlers.
    pub used_type: Option<(MoveId, TypeId)>,
    /// The move another move is calling (`actions.useMove` from Copycat or
    /// Sleep Talk), while it runs.
    pub call: Option<crate::moves::Call>,
    /// The called move, its result and whether the user had fainted, once
    /// it ran: `runMove` goes on with it as the active move.
    pub called: Option<(MoveId, Option<bool>, bool)>,
    /// The Pokémon whose move is in use (from its `Try` step): the source
    /// of the volatiles its move adds, for Safeguard.
    pub mover: Option<MonRef>,
    /// Targets of the status move being used whose effect returned
    /// `NOT_FAIL` (a heal at full HP): the move counts as done, but
    /// spreadMoveHit drops them before `afterMoveSecondaryEvent`.
    pub not_fail: Vec<MonRef>,
    /// A field move's `onHitField` returned `NOT_FAIL` (Teatime with no
    /// berry): its message is written, and its result is neither.
    pub field_not_fail: bool,
    /// The move being used ignores redirection (`move.tracksTarget`).
    pub tracks_target: bool,
    /// Axe Kick's confusion is being added: it lasts at least 3 turns.
    pub axe_kick: bool,
    /// The move in use got Parental Bond's second hit (`multihitType`).
    pub pbond: bool,
    /// The move in use has `sourceEffect` Round: doubled power.
    pub round_doubled: bool,
    /// Beat Up's base powers, one per hit (`move.allies`).
    pub beat_up: Vec<u32>,
    /// `move.hitTargets` of the move in use (Sparkling Aria's AfterMove).
    pub hit_targets: Option<ActiveList>,
    /// The damage each target took in the hit being resolved, for DamagingHit
    /// handlers (Counter) and `gotAttacked`.
    pub hit_dealt: Vec<(MonRef, u32)>,
    /// Targets whose Protect a Piercing Drill or Unseen Fist went through
    /// (`getMoveHitData(move).bypassProtect`): the hit does a quarter.
    pub bypass: Vec<MonRef>,
    /// The item Fling threw, and who threw it (the `onHit` it installed).
    pub fling: Option<(ItemId, MonRef)>,
    /// Targets the move in use last hit critically (`getMoveHitData(move).crit`).
    pub crit: Vec<MonRef>,
    /// Each target's damage from the move's last hit on it (`moveDamage`,
    /// which `gotAttacked` records as `lastAttackedBy.damage`).
    pub hit_damage: Vec<(MonRef, u32)>,
    /// `move.totalDamage`.
    pub total_damage: u32,
    /// The move in use has `multihit`, and `smartTarget` is on.
    pub multihit: bool,
    pub smart: bool,
    /// The Keen Eye or Illuminate user whose move is in use
    /// (`move.ignoreEvasion` from their ModifyMove).
    pub ignore_evasion: Option<MonRef>,
    /// Synchronize's stand-in effect is passing a status back: it has no
    /// `name` or `effectType`, so Flower Veil blocks it without a message.
    pub synchronizing: bool,
}

/// Move bookkeeping that outlives one move: the last move used, faint-queue
/// kills, queued Rounds and targets, and event-sort flags.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TurnState {
    /// Showdown `battle.lastMove` (Copycat).
    pub last_move: Option<MoveId>,
    /// Faint-queue entries caused by a move's damage, with the move's user
    /// (Showdown `faintData.source` when `faintData.effect` is a move).
    pub move_kills: Vec<(MonRef, MonRef)>,
    /// Pokémon whose queued Round was moved up by another Round
    /// (`action.sourceEffect`); cleared each turn.
    pub round_actions: Vec<MonRef>,
    /// The next move used is such a Round (consumed by `use_move`).
    pub round_source: bool,
    /// Each queued move's `originalTarget` (resolveAction's `getAtLoc`), by
    /// the user's side and name: the target's side and name, if one stood there.
    pub original_targets: Vec<(SideName, Option<SideName>)>,
    /// Collecting the handlers of a sorted event (residual, switch-in):
    /// Showdown sorts the handlers of a suppressed ability (Gastro Acid) or
    /// item (Klutz) too, and skips them when run.
    pub sorting: bool,
    /// `beingCalledBack`: the ability's End at a switch-out says nothing.
    pub called_back: bool,
}
