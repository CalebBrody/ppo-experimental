//! Structural solver state. Logs, PRNG position and diagnostics are excluded.
use super::*;
use std::hash::{Hash, Hasher};

/// Keys are scoped to a shared Dex allocation, retained to prevent address reuse.
#[derive(Clone, Debug)]
struct DexIdentity(Arc<Dex>);
impl PartialEq for DexIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for DexIdentity {}
impl Hash for DexIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SlotKey {
    wish: Option<(u64, u32, usize, String)>,
    healing_wish: Option<usize>,
    healing_wish_detached: bool,
    future: Option<FutureState>,
}
impl From<&SlotConds> for SlotKey {
    fn from(s: &SlotConds) -> Self {
        Self {
            wish: s.wish.as_ref().map(|w| {
                (
                    if w.hp == 0.0 { 0 } else { w.hp.to_bits() },
                    w.starting_turn,
                    w.source_slot,
                    w.wisher.clone(),
                )
            }),
            healing_wish: s.healing_wish,
            healing_wish_detached: s.healing_wish_detached,
            future: s.future,
        }
    }
}

/// Full simulator state, not a player's information-set key.
/// Equal keys share a Dex allocation. Independently loaded dexes stay distinct.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BattleStateKey {
    dex: DexIdentity,
    slot_conds: [[SlotKey; 2]; 2],
    sides: [Side; 2],
    field: Field,
    turn: u32,
    queue: VecDeque<Action>,
    mid_turn: bool,
    request: RequestState,
    rqid: u32,
    force_switch: [[bool; 2]; 2],
    ended: bool,
    winner: Option<usize>,
    last_fainted_side: Option<usize>,
    faint_queue: Vec<MonRef>,
    move_hit: u8,
    smart_target: bool,
    bounce: Option<i8>,
    priority_override: Option<(MonRef, MoveId, i8)>,
    ability_breaker: Option<MonRef>,
    relocated: Option<(MonRef, MonRef)>,
    probe: bool,
    vol_stamp: u32,
    side_extras: [crate::state::SideExtras; 2],
    active_move: crate::state::ActiveMove,
    turn_state: crate::state::TurnState,
}

impl From<&Battle> for BattleStateKey {
    fn from(b: &Battle) -> Self {
        Self {
            dex: DexIdentity(b.dex.clone()),
            slot_conds: std::array::from_fn(|s| {
                std::array::from_fn(|i| SlotKey::from(&b.slot_conds[s][i]))
            }),
            sides: b.sides.clone(),
            field: b.field,
            turn: b.turn,
            queue: b.queue.clone(),
            mid_turn: b.mid_turn,
            request: b.request,
            rqid: b.rqid,
            force_switch: b.force_switch,
            ended: b.ended,
            winner: b.winner,
            last_fainted_side: b.last_fainted_side,
            faint_queue: b.faint_queue.clone(),
            move_hit: b.move_hit,
            smart_target: b.smart_target,
            bounce: b.bounce,
            priority_override: b.priority_override,
            ability_breaker: b.ability_breaker,
            relocated: b.relocated,
            probe: b.probe,
            vol_stamp: b.vol_stamp,
            side_extras: b.side_extras,
            active_move: b.active_move.clone(),
            turn_state: b.turn_state.clone(),
        }
    }
}

impl Battle {
    pub fn state_key(&self) -> BattleStateKey {
        self.into()
    }
}
