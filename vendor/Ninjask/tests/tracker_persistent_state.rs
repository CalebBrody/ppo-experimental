//! Public-history regression tests. These use real protocol messages and validate
//! the reconstructed engine state, rather than reading private battle counters.
mod common;
use ninjask::{
    tracker::Tracker,
    world::{Assumptions, SetAssumption, World},
    Dex,
};
use std::sync::{Arc, OnceLock};

fn dex() -> Arc<Dex> {
    static DEX: OnceLock<Arc<Dex>> = OnceLock::new();
    DEX.get_or_init(|| {
        Arc::new(
            Dex::from_json(
                &common::format_export("gen9championsvgc2026regmc")
                    .expect("format export required; set NINJASK_DATA"),
            )
            .unwrap(),
        )
    })
    .clone()
}
fn view() -> Tracker {
    let mut t = Tracker::new(dex(), 0);
    feed(
        &mut t,
        &[
            "|switch|p1a: Basculegion|Basculegion, L50|199/199",
            "|switch|p2a: Sneasler|Sneasler, L50|100/100",
            "|turn|1",
        ],
    );
    t
}
fn feed(t: &mut Tracker, lines: &[&str]) {
    for l in lines {
        t.feed(l);
    }
}
fn world(t: &Tracker) -> ninjask::Battle {
    World::from_view(t, &Assumptions::default()).unwrap()
}
fn assumed(t: &Tracker, ability: &str, item: &str) -> ninjask::Battle {
    let d = t.dex();
    World::from_view(
        t,
        &Assumptions {
            foe: vec![Some(SetAssumption {
                ability: d.ability_id(ability),
                item: Some(d.item_id(item)),
                moves: vec![
                    d.move_id("Close Combat").unwrap(),
                    d.move_id("Dire Claw").unwrap(),
                ],
                ..Default::default()
            })],
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn toxic_loss_192_counts_damage_once_and_restores_sixty_not_twenty_four() {
    let mut t = view();
    t.feed("|-status|p1a: Basculegion|tox");
    for (turn, hp) in [(10, 187), (11, 163), (12, 127), (13, 79)] {
        t.feed(&format!("|turn|{turn}"));
        t.feed(&format!(
            "|-damage|p1a: Basculegion|{hp}/199 tox|[from] psn"
        ));
        let before = world(&t).sides[0].mons[0].toxic_stage;
        t.feed("|upkeep");
        assert_eq!(world(&t).sides[0].mons[0].toxic_stage, before);
    }
    t.feed("|-sethp|p1a: Basculegion|67/199 tox");
    t.feed("|turn|14");
    let w = world(&t);
    let m = &w.sides[0].mons[0];
    assert_eq!(m.toxic_stage, 4);
    assert_eq!((m.maxhp / 16) * (m.toxic_stage as u16 + 1), 60);
    assert_eq!(m.hp, 67);
}
#[test]
fn toxic_switch_cure_reapplication_cap_and_hidden_ticks() {
    let mut t = view();
    t.feed("|-status|p1a: Basculegion|tox");
    for turn in 1..=20 {
        t.feed(&format!("|turn|{turn}"));
        t.feed("|upkeep");
    }
    assert_eq!(world(&t).sides[0].mons[0].toxic_stage, 15);
    feed(
        &mut t,
        &[
            "|switch|p1a: Gengar|Gengar, L50|150/150",
            "|switch|p1a: Basculegion|Basculegion, L50|67/199 tox",
        ],
    );
    assert_eq!(world(&t).sides[0].mons[0].toxic_stage, 0);
    feed(
        &mut t,
        &[
            "|turn|21",
            "|-damage|p1a: Basculegion|55/199 tox|[from] psn",
        ],
    );
    assert_eq!(world(&t).sides[0].mons[0].toxic_stage, 1);
    feed(
        &mut t,
        &[
            "|-curestatus|p1a: Basculegion|tox",
            "|-status|p1a: Basculegion|tox",
        ],
    );
    assert_eq!(world(&t).sides[0].mons[0].toxic_stage, 0);
}
#[test]
fn unburden_consumption_doubles_speed_and_item_gain_temporarily_masks_it() {
    let mut t = view();
    t.feed("|-ability|p2a: Sneasler|Unburden");
    let before = world(&t).effective_speed((1, 0));
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Psychic Seed",
            "|-enditem|p2a: Sneasler|Psychic Seed",
        ],
    );
    assert_eq!(world(&t).effective_speed((1, 0)), 2 * before);
    t.feed("|-item|p2a: Sneasler|Leftovers|[from] move: Trick");
    assert!(world(&t).sides[1].mons[0].vol.unburden);
    assert_eq!(world(&t).effective_speed((1, 0)), before);
    t.feed("|-enditem|p2a: Sneasler|Leftovers|[from] move: Knock Off");
    assert_eq!(world(&t).effective_speed((1, 0)), 2 * before);
}
#[test]
fn unburden_does_not_reactivate_on_itemless_reentry_or_ability_gain() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Unburden",
            "|-enditem|p2a: Sneasler|White Herb",
            "|switch|p2a: Gengar|Gengar, L50|100/100",
            "|switch|p2a: Sneasler|Sneasler, L50|100/100",
        ],
    );
    assert!(!world(&t).sides[1].mons[0].vol.unburden);
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|White Herb|[from] move: Recycle",
            "|-enditem|p2a: Sneasler|White Herb",
        ],
    );
    assert!(world(&t).sides[1].mons[0].vol.unburden);
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Insomnia|Unburden|[from] move: Worry Seed",
            "|-ability|p2a: Sneasler|Unburden|Insomnia|[from] move: Entrainment",
        ],
    );
    assert!(!world(&t).sides[1].mons[0].vol.unburden);
}
#[test]
fn unburden_unknown_ability_is_conditional_and_suppression_prevents_activation() {
    let mut t = view();
    t.feed("|-enditem|p2a: Sneasler|Psychic Seed");
    assert!(assumed(&t, "Unburden", "").sides[1].mons[0].vol.unburden);
    assert!(
        !assumed(&t, "Poison Touch", "").sides[1].mons[0]
            .vol
            .unburden
    );
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Poison Touch",
            "|-enditem|p2a: Sneasler|Psychic Seed",
            "|-ability|p2a: Sneasler|Unburden|Poison Touch|[from] move: Entrainment",
        ],
    );
    assert!(!world(&t).sides[1].mons[0].vol.unburden);
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Unburden",
            "|-endability|p2a: Sneasler",
            "|-enditem|p2a: Sneasler|Psychic Seed",
            "|-end|p2a: Sneasler|Gastro Acid",
        ],
    );
    assert!(!world(&t).sides[1].mons[0].vol.unburden);
}
#[test]
fn item_theft_without_enditem_records_loss() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Unburden",
            "|-item|p1a: Basculegion|Leftovers|[from] move: Covet|[of] p2a: Sneasler",
        ],
    );
    assert!(world(&t).sides[1].mons[0].vol.unburden);
}
#[test]
fn choice_lock_restricts_assumed_responses_and_survives_struggle() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Choice Scarf",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
            "|move|p2a: Sneasler|Struggle|p1a: Basculegion",
        ],
    );
    let w = assumed(&t, "Poison Touch", "Choice Scarf");
    assert_eq!(
        w.sides[1].mons[0].vol.choice_lock,
        t.dex().move_id("Close Combat")
    );
    let i = w.sides[1].mons[0]
        .moves
        .iter()
        .position(|m| Some(m.id) == t.dex().move_id("Dire Claw"))
        .unwrap();
    assert!(w.move_disabled((1, 0), i));
}
#[test]
fn choice_acquisition_not_revelation_resets_the_move_identity() {
    let mut t = view();
    t.feed("|move|p2a: Sneasler|Close Combat|p1a: Basculegion");
    t.feed("|-item|p2a: Sneasler|Choice Scarf|[from] ability: Frisk|[of] p1a: Basculegion");
    assert_eq!(
        assumed(&t, "Poison Touch", "Choice Scarf").sides[1].mons[0]
            .vol
            .choice_lock,
        t.dex().move_id("Close Combat")
    );
    t.feed("|-item|p2a: Sneasler|Choice Scarf|[from] move: Trick");
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
    t.feed("|move|p2a: Sneasler|Dire Claw|p1a: Basculegion");
    assert_eq!(
        world(&t).sides[1].mons[0].vol.choice_lock,
        t.dex().move_id("Dire Claw")
    );
    t.feed("|-enditem|p2a: Sneasler|Choice Scarf|[from] move: Knock Off");
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
}
#[test]
fn choice_called_move_cant_and_switch_do_not_replace_the_lock() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Choice Scarf",
            "|move|p2a: Sneasler|Sleep Talk|p2a: Sneasler",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion|[from] move: Sleep Talk",
            "|cant|p2a: Sneasler|par",
        ],
    );
    assert_eq!(
        world(&t).sides[1].mons[0].vol.choice_lock,
        t.dex().move_id("Sleep Talk")
    );
    feed(
        &mut t,
        &[
            "|switch|p2a: Gengar|Gengar, L50|100/100",
            "|switch|p2a: Sneasler|Sneasler, L50|100/100",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
}
#[test]
fn choice_magic_room_preserves_old_lock_but_does_not_start_one() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Choice Scarf",
            "|-fieldstart|move: Magic Room",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
    feed(
        &mut t,
        &[
            "|-fieldend|move: Magic Room",
            "|move|p2a: Sneasler|Dire Claw|p1a: Basculegion",
            "|-fieldstart|move: Magic Room",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
            "|-fieldend|move: Magic Room",
        ],
    );
    assert_eq!(
        world(&t).sides[1].mons[0].vol.choice_lock,
        t.dex().move_id("Dire Claw")
    );
}
#[test]
fn illusion_replacement_preserves_all_three_fields() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|poke|p2|Zoroark, L50|",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
            "|-status|p2a: Sneasler|tox",
            "|-damage|p2a: Sneasler|94/100 tox|[from] psn",
            "|-enditem|p2a: Sneasler|White Herb",
            "|replace|p2a: Zoroark|Zoroark, L50|94/100 tox",
        ],
    );
    let m = t.sides[1].mons.iter().find(|m| m.slot == Some(0)).unwrap();
    assert_eq!(m.toxic_stage, 1);
    assert!(m.unburden_active);
    assert_eq!(m.choice_lock, None);
    // Item loss correctly cleared the first lock; a later acquisition starts a new one.
    feed(
        &mut t,
        &[
            "|-item|p2a: Zoroark|Choice Scarf|[from] move: Trick",
            "|move|p2a: Zoroark|Dark Pulse|p1a: Basculegion",
        ],
    );
    assert_eq!(
        world(&t).sides[1].mons[0].vol.choice_lock,
        t.dex().move_id("Dark Pulse")
    );
}

#[test]
fn public_history_matches_live_engine_through_seeded_battles() {
    use ninjask::{battle::log::Viewer, rng::Rng, state::RequestState, Battle};
    let d = dex();
    let packed = include_str!("fixtures/regmc_teams.txt")
        .lines()
        .collect::<Vec<_>>();
    let mut counts = [0usize; 3];
    for game in 0..32 {
        let mut b = Battle::new(
            d.clone(),
            [packed[game % 4], packed[(game / 4 + 1) % 4]],
            ["p1", "p2"],
            781000 + game as u64,
        )
        .unwrap();
        let mut ts = [Tracker::new(d.clone(), 0), Tracker::new(d.clone(), 1)];
        let mut cursor = 0;
        let mut rng = Rng::new(57 + game as u64);
        b.choose_team(0, vec![5, 3, 1, 0]);
        b.choose_team(1, vec![5, 3, 1, 0]);
        for _ in 0..200 {
            if b.ended {
                break;
            }
            for side in 0..2 {
                for line in b.log_lines(Viewer::Side(side), cursor) {
                    ts[side].feed(&line);
                }
                ts[side].feed_engine(&b);
                let w = world(&ts[side]);
                for m in b.sides[side]
                    .mons
                    .iter()
                    .filter(|m| m.is_active && !m.fainted)
                {
                    let got = w.sides[side]
                        .mons
                        .iter()
                        .find(|x| x.name == m.name)
                        .unwrap();
                    let ctx = format!(
                        "game {game} turn {} side {side} {} logs {:?}",
                        b.turn,
                        m.name,
                        b.log_lines(Viewer::Side(side), cursor)
                    );
                    assert_eq!(got.toxic_stage, m.toxic_stage, "toxic: {ctx}");
                    assert_eq!(got.vol.unburden, m.vol.unburden, "unburden: {ctx}");
                    assert_eq!(got.vol.choice_lock, m.vol.choice_lock, "choice: {ctx}");
                    counts[0] += usize::from(m.toxic_stage > 0);
                    counts[1] += usize::from(m.vol.unburden);
                    counts[2] += usize::from(m.vol.choice_lock.is_some());
                }
            }
            cursor = b.log.len();
            if b.turn >= 60 || b.request == RequestState::Ended {
                break;
            }
            for side in 0..2 {
                if b.side_needs_choice(side) {
                    let c = b.sample_joint_choice(side, &mut rng);
                    b.choose_structured(side, c);
                }
            }
        }
    }
    println!(
        "Live-engine active positions: Toxic {}, Unburden {}, Choice {}",
        counts[0], counts[1], counts[2]
    );
    assert!(
        counts.iter().all(|n| *n > 0),
        "all three histories must be exercised: {counts:?}"
    );
}

#[test]
fn illusion_replacement_keeps_choice_lock_distinct_from_last_move() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|poke|p2|Zoroark, L50|",
            "|-item|p2a: Sneasler|Choice Scarf",
            "|move|p2a: Sneasler|Dark Pulse|p1a: Basculegion",
            "|move|p2a: Sneasler|Struggle|p1a: Basculegion",
            "|replace|p2a: Zoroark|Zoroark, L50|100/100",
        ],
    );
    let w = world(&t);
    let m = &w.sides[1].mons[0];
    assert_eq!(m.vol.choice_lock, t.dex().move_id("Dark Pulse"));
    assert_eq!(m.last_move, t.dex().move_id("Struggle"));
}
#[test]
fn acquired_choice_does_not_lock_a_move_used_before_receiving_it() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Leftovers",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
            "|-item|p2a: Sneasler|Choice Scarf|[from] move: Trick",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
}
#[test]
fn klutz_prevents_new_choice_locks() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Klutz",
            "|-item|p2a: Sneasler|Choice Scarf",
            "|move|p2a: Sneasler|Close Combat|p1a: Basculegion",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].vol.choice_lock, None);
}
