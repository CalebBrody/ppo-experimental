//! Choices and queued actions as plain data.

use crate::dex::types::*;

/// One slot's part of a choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotChoice {
    Pass,
    /// Move slot index (0-based), target location (0 = none), Mega Evolve.
    Move {
        slot: usize,
        target: i8,
        mega: bool,
    },
    /// Index (0-based) in the side's Pokémon order.
    Switch {
        index: usize,
    },
}

/// What the battle is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequestState {
    TeamPreview,
    Move,
    Switch,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    Start,
    BeforeTurn,
    MegaEvo {
        side: usize,
        mon: usize,
    },
    /// `id`: the chosen move (Showdown `action.move`), which the queue's
    /// re-sorts use even once the move slots changed (a transformed
    /// Pokémon fainting gets its own back); unused for Struggle and recharge.
    Move {
        side: usize,
        mon: usize,
        slot: usize,
        target: i8,
        id: MoveId,
    },
    /// `mon` is the Pokémon leaving (its position is the slot), `target` the one entering.
    /// `insta`: chosen in answer to a switch request (Showdown `instaswitch`, order 3).
    /// `from`: the move that made the Pokémon leave (U-turn), named in the switch line.
    Switch {
        side: usize,
        mon: usize,
        target: usize,
        insta: bool,
        from: Option<MoveId>,
    },
    RunSwitch {
        side: usize,
        mon: usize,
    },
    /// Revival Blessing's choice: `mon` (its slot) revives `target`.
    Revive {
        side: usize,
        mon: usize,
        target: usize,
    },
    Residual,
    /// `priorityChargeMove` (order 107): runs a move's `priorityChargeCallback`
    /// before any move of the turn (Chilly Reception).
    PriorityCharge {
        side: usize,
        mon: usize,
        id: MoveId,
    },
    /// `beforeTurnMove` (order 5): runs a move's `beforeTurnCallback` before the
    /// turn's moves (Counter). `target` is the location the action was made with.
    BeforeTurnMove {
        side: usize,
        mon: usize,
        id: MoveId,
        target: i8,
    },
}

/// An action in Showdown's queue, with its sort keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Action {
    pub kind: ActionKind,
    pub order: u32,
    pub priority: i32,
    pub speed: i32,
    /// Showdown `fractionalPriority` in tenths (Quick Claw: 1), added to the
    /// priority; resolved once, when the action is made.
    pub frac: i32,
}
