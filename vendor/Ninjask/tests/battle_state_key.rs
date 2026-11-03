use ninjask::state::{Action, ActionKind, SlotChoice};
use ninjask::{Battle, Dex};
use std::{collections::HashSet, sync::Arc};
fn battle() -> Battle {
    Battle::skeleton(
        Arc::new(Dex::from_json(include_str!("fixtures/enumeration.json")).unwrap()),
        ["a", "b"],
        1,
    )
}
#[test]
fn key_preserves_pending_choices_queue_and_field() {
    let b = battle();
    let key = b.state_key();
    let mut keys = HashSet::new();
    keys.insert(key.clone());
    assert!(keys.contains(&b.clone().state_key()));
    let mut c = b.clone();
    c.sides[0].choice = Some(vec![SlotChoice::Pass]);
    assert_ne!(key, c.state_key());
    let mut c = b.clone();
    c.queue.push_back(Action {
        kind: ActionKind::Residual,
        order: 1,
        priority: 0,
        speed: 0,
        frac: 0,
    });
    assert_ne!(key, c.state_key());
    let mut c = b.clone();
    c.turn += 1;
    assert_ne!(key, c.state_key());
    let mut c = b.clone();
    c.field.weather_turns = 3;
    assert_ne!(key, c.state_key());
    assert_ne!(key, battle().state_key());
}
#[test]
fn key_ignores_rng_and_logging() {
    let b = battle();
    let mut c = b.clone();
    c.rng.next_u32();
    c.log_enabled = false;
    assert_eq!(b.state_key(), c.state_key());
}
