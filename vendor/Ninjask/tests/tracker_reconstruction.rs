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
fn own_request_preserves_mega_build_parameters() {
    let mut t = Tracker::new(dex(), 0);
    t.feed_request(&serde_json::json!({"side":{"pokemon":[{
        "ident":"p1: Tyranitar","details":"Tyranitar, L50, M","condition":"202/202","active":true,
        "stats":{"atk":198,"def":131,"spa":103,"spd":120,"spe":93},
        "nature":"Adamant","statPoints":[27,26,1,0,0,12],"moves":["rockslide","protect"],
        "ability":"sandstream","baseAbility":"sandstream","item":"tyranitarite"
    }]}}));
    let w = world(&t);
    let m = &w.sides[0].mons[0];
    let mega = t.dex().species_id("Tyranitar-Mega").unwrap();
    let stats = ninjask::dex::team::champions_stats(
        &t.dex().species(mega).base_stats,
        &m.points,
        ninjask::dex::Nature {
            plus: m.nature_plus,
            minus: m.nature_minus,
        },
    );
    assert_eq!(&stats[1..], &[231, 171, 103, 140, 103]);
    assert_eq!(m.stats, [202, 198, 131, 103, 120, 93]);
}
#[test]
fn engine_and_request_agree_on_own_metadata_without_exposing_foe_builds() {
    let d = dex();
    let teams = include_str!("fixtures/regmc_teams.txt")
        .lines()
        .collect::<Vec<_>>();
    let mut b = ninjask::Battle::new(d.clone(), [teams[0], teams[1]], ["p1", "p2"], 7).unwrap();
    b.choose_team(0, vec![0, 1, 2, 3]);
    b.choose_team(1, vec![0, 1, 2, 3]);
    for side in 0..2 {
        let mut engine = Tracker::new(d.clone(), side);
        let mut request = engine.clone();
        for l in b.log_lines(ninjask::battle::log::Viewer::Side(side), 0) {
            engine.feed(&l);
            request.feed(&l);
        }
        engine.feed_engine(&b);
        request.feed_request(&b.request_json(side).unwrap());
        for (a, c) in engine.sides[side]
            .mons
            .iter()
            .zip(&request.sides[side].mons)
        {
            assert_eq!(a.stat_points, c.stat_points);
            assert_eq!(a.nature, c.nature);
        }
        assert!(engine.sides[1 - side]
            .mons
            .iter()
            .all(|m| m.stat_points.is_none()));
        let w = world(&engine);
        for (a, c) in w.sides[side].mons.iter().zip(&b.sides[side].mons) {
            assert_eq!(a.points, c.points);
            assert_eq!(a.nature_plus, c.nature_plus);
            assert_eq!(a.nature_minus, c.nature_minus);
        }
    }
}
#[test]
fn charge_keeps_committed_target_and_exact_move_not_first_known_charge() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|switch|p2b: Charizard|Charizard, L50|100/100",
            "|move|p2b: Charizard|Solar Beam|p1a: Basculegion",
            "|move|p2b: Charizard|Electro Shot|p1b: Gengar",
            "|-prepare|p2b: Charizard|Electro Shot",
            "|upkeep",
            "|turn|2",
        ],
    );
    let w = world(&t);
    let m = &w.sides[1].mons[1];
    assert_eq!(m.vol.charging, t.dex().move_id("Electro Shot"));
    assert_eq!(m.vol.charging_target_loc, 2);
    assert_eq!(m.vol.two_turn_duration, 1);
    assert_eq!(m.vol.charging_duration, 0);
}
#[test]
fn charge_accepts_explicit_target_and_ally_target_and_clears_on_fire() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Solar Beam|p1a: Basculegion",
            "|-prepare|p2a: Sneasler|Solar Beam|p2b: Gengar",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].vol.charging_target_loc, -2);
    t.feed("|move|p2a: Sneasler|Solar Beam|p2b: Gengar|[from] lockedmove");
    assert_eq!(world(&t).sides[1].mons[0].vol.charging, None);
}
#[test]
fn instant_charge_and_switch_clear_charge_history() {
    let mut t = view();
    for ending in [
        "|-anim|p2a: Sneasler|Electro Shot|p1a: Basculegion",
        "|switch|p2a: Gengar|Gengar, L50|100/100",
    ] {
        feed(
            &mut t,
            &[
                "|switch|p2a: Sneasler|Sneasler, L50|100/100",
                "|move|p2a: Sneasler|Electro Shot|p1a: Basculegion",
                "|-prepare|p2a: Sneasler|Electro Shot",
                ending,
            ],
        );
        assert!(t.sides[1].mons.iter().all(|m| m.charging.is_none()));
    }
}
#[test]
fn phantom_force_charge_duration_counts_residual() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Phantom Force|p1a: Basculegion",
            "|-prepare|p2a: Sneasler|Phantom Force",
            "|upkeep",
            "|turn|2",
        ],
    );
    let w = world(&t);
    assert_eq!(w.sides[1].mons[0].vol.charging_duration, 1);
}
#[test]
fn protean_and_libero_activation_survive_later_type_change_but_not_switch() {
    for (ability, field) in [("Protean", true), ("Libero", false)] {
        let mut t = view();
        t.feed(&format!("|-ability|p2a: Sneasler|{ability}"));
        t.feed(&format!(
            "|-start|p2a: Sneasler|typechange|Fire|[from] ability: {ability}"
        ));
        t.feed("|-start|p2a: Sneasler|typechange|Water|[from] move: Soak");
        let w = world(&t);
        let a = &w.sides[1].mons[0].ability_state;
        assert_eq!(if field { a.protean } else { a.libero }, Some(true));
        feed(
            &mut t,
            &[
                "|switch|p2a: Gengar|Gengar, L50|100/100",
                "|switch|p2a: Sneasler|Sneasler, L50|100/100",
            ],
        );
        assert!(
            !t.sides[1]
                .mons
                .iter()
                .find(|m| m.name == "Sneasler")
                .unwrap()
                .type_change_used
        );
    }
}
#[test]
fn ability_replacement_resets_type_change_use() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-ability|p2a: Sneasler|Protean",
            "|-start|p2a: Sneasler|typechange|Fire|[from] ability: Protean",
            "|-ability|p2a: Sneasler|Libero|Protean|[from] move: Role Play|[of] p1a: Basculegion",
        ],
    );
    assert!(!t.sides[1].mons[0].type_change_used);
}
#[test]
fn illusion_copy_preserves_activation_and_charge_target() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|poke|p2|Zoroark, L50|",
            "|-start|p2a: Sneasler|typechange|Fire|[from] ability: Protean",
            "|move|p2a: Sneasler|Solar Beam|p1b: Gengar",
            "|-prepare|p2a: Sneasler|Solar Beam",
            "|replace|p2a: Zoroark|Zoroark, L50|100/100",
        ],
    );
    let m = t.sides[1].mons.iter().find(|m| m.slot == Some(0)).unwrap();
    assert!(m.type_change_used);
    assert_eq!(m.charging.unwrap().1, 2);
}
#[test]
fn active_known_or_assumed_unnerve_has_started() {
    let mut t = view();
    t.feed("|-ability|p2a: Sneasler|Unnerve");
    assert_eq!(
        world(&t).sides[1].mons[0].ability_state.unnerved,
        Some(true)
    );
    let t = view();
    assert_eq!(
        assumed(&t, "Unnerve", "Sitrus Berry").sides[1].mons[0]
            .ability_state
            .unnerved,
        Some(true)
    );
}
#[test]
fn weather_uses_setter_not_other_side_or_bench_item() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|switch|p2b: Pelipper|Pelipper, L50|100/100",
            "|-item|p2b: Pelipper|Damp Rock",
            "|-item|p1a: Basculegion|Leftovers",
            "|-weather|RainDance|[from] ability: Drizzle|[of] p1a: Basculegion",
        ],
    );
    assert_eq!(world(&t).field.weather_turns, 5);
    t.feed("|switch|p2b: Gengar|Gengar, L50|100/100");
    assert_eq!(world(&t).field.weather_turns, 5);
}
#[test]
fn weather_extension_remembers_item_at_onset_after_item_loss_and_switch() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Damp Rock",
            "|-weather|RainDance|[from] ability: Drizzle|[of] p2a: Sneasler",
            "|-enditem|p2a: Sneasler|Damp Rock|[from] move: Knock Off",
            "|switch|p2a: Gengar|Gengar, L50|100/100",
            "|upkeep",
            "|turn|2",
        ],
    );
    assert_eq!(world(&t).field.weather_turns, 7);
}
#[test]
fn unknown_weather_item_uses_only_setter_assumption() {
    let mut t = view();
    t.feed("|-weather|RainDance|[from] ability: Drizzle|[of] p2a: Sneasler");
    assert_eq!(assumed(&t, "Drizzle", "Damp Rock").field.weather_turns, 8);
    t.feed("|-weather|RainDance|[from] ability: Drizzle|[of] p1a: Basculegion");
    assert_eq!(assumed(&t, "Drizzle", "Damp Rock").field.weather_turns, 5);
}
#[test]
fn manual_weather_and_terrain_use_move_user_and_clear_source_on_end() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-item|p1a: Basculegion|Damp Rock",
            "|move|p1a: Basculegion|Rain Dance|p1a: Basculegion",
            "|-weather|RainDance",
        ],
    );
    assert_eq!(world(&t).field.weather_turns, 8);
    feed(
        &mut t,
        &[
            "|-item|p2a: Sneasler|Terrain Extender",
            "|-fieldstart|move: Grassy Terrain|[from] ability: Grassy Surge|[of] p1a: Basculegion",
        ],
    );
    assert_eq!(world(&t).field.terrain_turns, 5);
    feed(
        &mut t,
        &["|-fieldstart|move: Grassy Terrain|[from] ability: Grassy Surge|[of] p2a: Sneasler"],
    );
    assert_eq!(world(&t).field.terrain_turns, 8);
    feed(
        &mut t,
        &["|-weather|none", "|-fieldend|move: Grassy Terrain"],
    );
    assert!(t.field.weather_source.is_none());
    assert!(t.field.terrain_source.is_none());
}

#[test]
fn own_request_restores_known_choice_target_but_does_not_invent_foe_target() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p1a: Basculegion|Solar Beam||[still]",
            "|-prepare|p1a: Basculegion|Solar Beam",
            "|move|p2a: Sneasler|Solar Beam||[still]",
            "|-prepare|p2a: Sneasler|Solar Beam",
            "|upkeep",
            "|turn|2",
        ],
    );
    t.feed_request(&serde_json::json!({"side":{"pokemon":[{
        "ident":"p1: Basculegion","details":"Basculegion, L50","condition":"199/199",
        "active":true,"moves":["solarbeam"],"chargingTarget":2
    }]}}));
    let w = world(&t);
    assert_eq!(w.sides[0].mons[0].vol.charging_target_loc, 2);
    assert_eq!(w.sides[1].mons[0].vol.charging_target_loc, 0);
}
#[test]
fn mega_and_transform_reset_once_per_ability_type_change() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-start|p2a: Sneasler|typechange|Fire|[from] ability: Protean",
            "|detailschange|p2a: Sneasler|Gengar-Mega, L50",
        ],
    );
    assert!(!t.sides[1].mons[0].type_change_used);
    feed(
        &mut t,
        &[
            "|-start|p2a: Sneasler|typechange|Fire|[from] ability: Protean",
            "|-transform|p2a: Sneasler|p1a: Basculegion",
        ],
    );
    assert!(!t.sides[1].mons[0].type_change_used);
}

#[test]
fn flinch_paralysis_and_confusion_preserve_previous_action_failure() {
    for event in [
        "|cant|p2a: Sneasler|flinch",
        "|cant|p2a: Sneasler|par",
        "|-damage|p2a: Sneasler|90/100|[from] confusion",
    ] {
        let mut t = view();
        feed(&mut t, &[event, "|upkeep", "|turn|2"]);
        assert_eq!(
            world(&t).sides[1].mons[0].move_last_turn_result,
            Some(false)
        );
        feed(
            &mut t,
            &[
                "|move|p2a: Sneasler|Protect|p2a: Sneasler",
                "|-singleturn|p2a: Sneasler|Protect",
                "|upkeep",
                "|turn|3",
            ],
        );
        assert_eq!(world(&t).sides[1].mons[0].move_last_turn_result, Some(true));
    }
}
#[test]
fn spread_partial_success_overrides_a_miss_but_full_failure_does_not() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Rock Slide|p1a: Basculegion|[spread] p1a,p1b",
            "|-miss|p2a: Sneasler|p1a: Basculegion",
            "|-damage|p1b: Gengar|50/100",
            "|upkeep",
            "|turn|2",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].move_last_turn_result, Some(true));
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Close Combat|p1b: Gengar",
            "|-immune|p1b: Gengar",
            "|upkeep",
            "|turn|3",
        ],
    );
    assert_eq!(
        world(&t).sides[1].mons[0].move_last_turn_result,
        Some(false)
    );
}
#[test]
fn failed_protect_and_switch_reset_action_result() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Protect||[still]",
            "|-fail|p2a: Sneasler",
            "|upkeep",
            "|turn|2",
        ],
    );
    assert_eq!(
        world(&t).sides[1].mons[0].move_last_turn_result,
        Some(false)
    );
    feed(
        &mut t,
        &[
            "|switch|p2a: Gengar|Gengar, L50|100/100",
            "|switch|p2a: Sneasler|Sneasler, L50|100/100",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].move_last_turn_result, None);
}

#[test]
fn two_observed_sleep_actions_force_next_action_to_wake_even_after_switch() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|-status|p2a: Sneasler|slp",
            "|cant|p2a: Sneasler|slp",
            "|upkeep",
            "|turn|2",
            "|cant|p2a: Sneasler|slp",
            "|upkeep",
            "|turn|3",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].status_time, 1);
    feed(
        &mut t,
        &[
            "|switch|p2a: Gengar|Gengar, L50|100/100",
            "|switch|p2a: Sneasler|Sneasler, L50|100/100 slp",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].status_time, 1);
    feed(
        &mut t,
        &[
            "|-curestatus|p2a: Sneasler|slp",
            "|-status|p2a: Sneasler|slp",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].status_time, 2);
}
#[test]
fn rest_sleep_is_exact_and_early_bird_uses_observed_decrement() {
    let mut t = view();
    t.feed("|-status|p2a: Sneasler|slp|[from] move: Rest");
    assert_eq!(world(&t).sides[1].mons[0].status_time, 3);
    t.feed("|-ability|p2a: Sneasler|Early Bird");
    t.feed("|cant|p2a: Sneasler|slp");
    assert_eq!(world(&t).sides[1].mons[0].status_time, 1);
}
#[test]
fn frozen_counter_does_not_start_at_zero_and_counts_observed_actions() {
    let mut t = view();
    t.feed("|-status|p2a: Sneasler|frz");
    assert_eq!(world(&t).sides[1].mons[0].status_time, 3);
    t.feed("|cant|p2a: Sneasler|frz");
    t.feed("|cant|p2a: Sneasler|frz");
    assert_eq!(world(&t).sides[1].mons[0].status_time, 1);
}

#[test]
fn protected_hit_is_null_not_failure_and_heal_pulse_full_hp_is_success() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Stomping Tantrum|p1a: Basculegion",
            "|-activate|p1a: Basculegion|move: Protect",
            "|upkeep",
            "|turn|2",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].move_last_turn_result, None);
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Heal Pulse|p1a: Basculegion",
            "|-fail|p1a: Basculegion|heal",
            "|upkeep",
            "|turn|3",
        ],
    );
    assert_eq!(world(&t).sides[1].mons[0].move_last_turn_result, Some(true));
}

#[test]
fn locked_request_slot_zero_resolves_real_move_and_committed_target() {
    let mut t = view();
    feed(
        &mut t,
        &[
            "|move|p2a: Sneasler|Dragon Pulse|p1a: Basculegion",
            "|move|p2a: Sneasler|Electro Shot|p1b: Gengar",
            "|-prepare|p2a: Sneasler|Electro Shot",
        ],
    );
    let mut w = world(&t);
    assert_ne!(
        w.sides[1].mons[0].moves[0].id,
        t.dex().move_id("Electro Shot").unwrap()
    );
    assert_eq!(w.choice_move_id((1, 0), 0), t.dex().move_id("Electro Shot"));
    assert_eq!(w.choice_target_loc((1, 0), 0), 2);
    w.sides[1].mons[0].vol.must_recharge = 1;
    assert_eq!(w.choice_move_id((1, 0), 0), None);
    w.sides[1].mons[0].vol = Default::default();
    assert_eq!(w.choice_move_id((1, 0), 0), t.dex().move_id("Dragon Pulse"));
    assert_eq!(
        w.choice_move_id((1, 0), ninjask::state::STRUGGLE),
        t.dex().move_id("Struggle")
    );
}
