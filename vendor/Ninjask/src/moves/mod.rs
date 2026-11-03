//! Moves: Showdown's `runMove`, `useMove`, `trySpreadMoveHit` and the hit steps,
//! `spreadMoveHit`, damage, and the effects moves apply. Ported special cases
//! are called where Showdown runs the matching events.

mod after_hit;
mod called;
pub mod damage;
mod field_moves;
mod hit;
mod multihit;
mod status_moves;
mod targeting;
mod two_turn;
mod use_move;

pub use called::Call;
pub use two_turn::LockedMove;
