use ninjask::state::{RequestState, SlotChoice};
use ninjask::{Battle, Dex, EnumerationMode, Outcome};
use std::collections::HashMap;
use std::sync::Arc;

fn battle() -> Battle {
    let dex = Arc::new(Dex::from_json(include_str!("fixtures/enumeration.json")).unwrap());
    let mut b = Battle::new(
        dex,
        [
            "Fast|||battlearmor|Tackle,Splash,U-turn,Double Hit|Hardy|||||50|",
            "Slow|||battlearmor|Splash|Hardy|||||50|",
        ],
        ["a", "b"],
        1,
    )
    .unwrap();
    b.choose_team(0, vec![0]);
    b.choose_team(1, vec![0]);
    b
}
fn distribution(out: &[Outcome]) -> HashMap<ninjask::BattleStateKey, f64> {
    out.iter()
        .map(|o| (o.battle.state_key(), o.probability))
        .collect()
}
#[test]
fn damage_modes_merge_rounding_and_preserve_input() {
    let b = battle();
    let before = format!("{b:?}");
    let precise = b
        .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::Precise)
        .unwrap();
    let almost = b
        .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::AlmostPrecise)
        .unwrap();
    assert!(precise.len() > 2 && precise.len() <= 16);
    assert_eq!(almost.len(), 2);
    assert!(almost.iter().all(|o| o.probability == 0.5));
    for out in [&precise, &almost] {
        assert!((out.iter().map(|o| o.probability).sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(out
            .iter()
            .all(|o| o.battle.turn == b.turn + 1 && o.battle.request == RequestState::Move));
    }
    let hp = |o: &Outcome| o.battle.sides[1].mons[0].hp;
    assert_eq!(precise.iter().map(hp).min(), almost.iter().map(hp).min());
    assert_eq!(precise.iter().map(hp).max(), almost.iter().map(hp).max());
    assert_eq!(format!("{b:?}"), before);
    let mut reseeded = b.clone();
    reseeded.rng = ninjask::rng::Rng::new(999);
    assert_eq!(
        distribution(&precise),
        distribution(
            &reseeded
                .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::Precise)
                .unwrap()
        )
    );
}
#[test]
fn pending_choice_is_preserved_and_structured_matches_text() {
    let mut b = battle();
    b.choose(0, "move 1").unwrap();
    let before = b.state_key();
    let out = b
        .enumerate_outcomes(
            &[],
            &[SlotChoice::Move {
                slot: 0,
                target: 0,
                mega: false,
            }],
            EnumerationMode::Precise,
        )
        .unwrap();
    assert_eq!(
        distribution(&out),
        distribution(
            &b.enumerate_outcomes_text("", "move 1", EnumerationMode::Precise)
                .unwrap()
        )
    );
    assert_eq!(b.state_key(), before);
    assert!(b.sides[0].choice.is_some());
    assert!(b
        .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::Precise)
        .is_err());
}
#[test]
fn deterministic_and_terminal_states_have_unit_mass() {
    let mut b = battle();
    let out = b
        .enumerate_outcomes_text("move 2", "move 1", EnumerationMode::Precise)
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].probability, 1.0);
    b.sides[1].mons[0].hp = 1;
    let out = b
        .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::Precise)
        .unwrap();
    assert!(out.iter().all(|o| o.battle.ended));
    let end = &out[0].battle;
    let again = end
        .enumerate_outcomes(&[], &[], EnumerationMode::Precise)
        .unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].probability, 1.0);
    assert_eq!(again[0].battle.state_key(), end.state_key());
}
#[test]
fn invalid_choices_are_errors() {
    let b = battle();
    assert!(b
        .enumerate_outcomes(&[], &[], EnumerationMode::Precise)
        .is_err());
    assert!(b
        .enumerate_outcomes_text("move 99", "move 1", EnumerationMode::Precise)
        .is_err());
}

#[test]
fn pivot_stops_with_queue_intact_then_resumes() {
    let dex = Arc::new(Dex::from_json(include_str!("fixtures/enumeration.json")).unwrap());
    let team = "Fast|||battlearmor|U-turn|Hardy|||||50|]Slow|||battlearmor|Splash|Hardy|||||50|";
    let mut b = Battle::new(
        dex,
        [team, "Slow|||battlearmor|Splash|Hardy|||||50|"],
        ["a", "b"],
        1,
    )
    .unwrap();
    b.choose_team(0, vec![0, 1]);
    b.choose_team(1, vec![0]);
    let out = b
        .enumerate_outcomes_text("move 1", "move 1", EnumerationMode::AlmostPrecise)
        .unwrap();
    assert!(!out.is_empty());
    for o in out {
        assert_eq!(o.battle.request, RequestState::Switch);
        assert_eq!(o.battle.turn, b.turn);
        assert!(o.battle.mid_turn);
        assert!(!o.battle.queue.is_empty());
        assert!(o.battle.side_needs_choice(0));
        assert!(!o.battle.side_needs_choice(1));
        let next = o
            .battle
            .enumerate_outcomes_text("switch 2", "", EnumerationMode::AlmostPrecise)
            .unwrap();
        assert!(next.iter().all(|x| x.battle.turn == b.turn + 1));
    }
}

#[test]
fn multihit_uses_independent_damage_rolls() {
    let b = battle();
    let precise = b
        .enumerate_outcomes_text("move 4", "move 1", EnumerationMode::Precise)
        .unwrap();
    let almost = b
        .enumerate_outcomes_text("move 4", "move 1", EnumerationMode::AlmostPrecise)
        .unwrap();
    assert!(precise.len() > almost.len());
    assert!(almost.len() <= 4);
    assert!((almost.iter().map(|o| o.probability).sum::<f64>() - 1.0).abs() < 1e-12);
}

/// Rolls affect surviving HP, but never whether this hit wins the battle.
#[test]
fn crit_matters_but_damage_roll_does_not_change_ko() {
    let dex = Arc::new(Dex::from_json(include_str!("fixtures/enumeration.json")).unwrap());
    let mut b = Battle::new(
        dex,
        [
            "Fast|||noability|Tackle|Hardy|||||50|",
            "Slow|||noability|Splash|Hardy|||||50|",
        ],
        ["a", "b"],
        1,
    )
    .unwrap();
    b.choose_team(0, vec![0]);
    b.choose_team(1, vec![0]);
    b.sides[1].mons[0].hp = 30;
    let calc = b.damage_calc((0, 0), b.dex.move_id("tackle").unwrap(), (1, 0));
    let rolls = &calc.hits[0];
    assert_eq!(
        (
            *rolls.normal.iter().min().unwrap(),
            *rolls.normal.iter().max().unwrap()
        ),
        (24, 28)
    );
    assert_eq!(
        (
            *rolls.crit.iter().min().unwrap(),
            *rolls.crit.iter().max().unwrap()
        ),
        (34, 42)
    );
    assert!(rolls.normal.iter().all(|&damage| damage < 30));
    assert!(rolls.crit.iter().all(|&damage| damage >= 30));
    println!("Scenario: Fast uses Tackle; Slow has 30 HP and uses Splash; no abilities/items.");
    println!("Normal damage: 24..28 -> always survives.");
    println!("Critical damage: 34..42 -> always KO.");
    // ceil(2^32 / 24) raw draws satisfy the engine's chance(1, 24).
    let expected_ko = 178_956_971.0 / 4_294_967_296.0;
    let mut distributions = Vec::new();
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let out = b.enumerate_outcomes_text("move 1", "move 1", mode).unwrap();
        let mut ko = 0.0;
        let mut survives = 0.0;
        for o in &out {
            let target = &o.battle.sides[1].mons[0];
            if target.hp == 0 {
                assert!(target.fainted && o.battle.ended);
                assert_eq!(o.battle.winner, Some(0));
                ko += o.probability;
            } else {
                assert!(!target.fainted && !o.battle.ended);
                assert!((2..=6).contains(&target.hp));
                survives += o.probability;
            }
        }
        assert!((ko - expected_ko).abs() < 1e-12);
        assert!((survives - (1.0 - expected_ko)).abs() < 1e-12);
        println!(
            "{mode:?}: KO = {ko:.12}, survives = {survives:.12}, total = {:.12}",
            ko + survives
        );
        distributions.push((ko, survives));
    }
    assert!((distributions[0].0 - distributions[1].0).abs() < 1e-12);
    assert!((distributions[0].1 - distributions[1].1).abs() < 1e-12);
    println!(
        "PASS: Precise and AlmostPrecise match for KO/survival probabilities (tolerance 1e-12)."
    );
}

fn chance_battle(accuracy: u8, poison: bool, tied: bool) -> Battle {
    // Synthetic fixture variants isolate each chance mechanic.
    let mut data: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/enumeration.json")).unwrap();
    data["moves"][0]["accuracy"] = accuracy.into();
    if poison {
        data["moves"][0]["secondaries"] = serde_json::json!([{"chance": 30, "status": "psn"}]);
    }
    let dex = Arc::new(Dex::from_json(&data.to_string()).unwrap());
    let second = if tied {
        "Fast|||noability|Tackle,Splash|Hardy|||||50|"
    } else {
        "Slow|||noability|Tackle,Splash|Hardy|||||50|"
    };
    let mut b = Battle::new(
        dex,
        ["Fast|||noability|Tackle,Splash|Hardy|||||50|", second],
        ["a", "b"],
        7,
    )
    .unwrap();
    b.choose_team(0, vec![0]);
    b.choose_team(1, vec![0]);
    b
}

fn mass(out: &[Outcome], predicate: impl Fn(&Battle) -> bool) -> f64 {
    out.iter()
        .filter(|o| predicate(&o.battle))
        .map(|o| o.probability)
        .sum()
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual:.15}, expected={expected:.15}"
    );
}

#[test]
fn accuracy_and_crits_keep_their_joint_probabilities() {
    let mut b = chance_battle(75, false, false);
    b.sides[1].mons[0].hp = 30;
    let crit = 178_956_971.0 / 4_294_967_296.0;
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let out = b.enumerate_outcomes_text("move 1", "move 2", mode).unwrap();
        let miss = mass(&out, |b| b.sides[1].mons[0].hp == 30);
        let ko = mass(&out, |b| b.ended && b.winner == Some(0));
        let normal_hit = mass(&out, |b| (2..=6).contains(&b.sides[1].mons[0].hp));
        near(miss, 0.25);
        near(ko, 0.75 * crit);
        near(normal_hit, 0.75 * (1.0 - crit));
        near(miss + ko + normal_hit, 1.0);
        println!(
            "{mode:?} accuracy+crit: miss={miss:.12}, normal hit={normal_hit:.12}, KO={ko:.12}"
        );
    }
}

#[test]
fn lethal_speed_tie_splits_winner_probability_equally() {
    let mut b = chance_battle(100, false, true);
    for side in &mut b.sides {
        side.mons[0].hp = 1;
    }
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let out = b.enumerate_outcomes_text("move 1", "move 1", mode).unwrap();
        assert!(out.iter().all(|o| o.battle.ended));
        let p1 = mass(&out, |b| b.winner == Some(0));
        let p2 = mass(&out, |b| b.winner == Some(1));
        near(p1, 0.5);
        near(p2, 0.5);
        println!("{mode:?} speed tie: p1 wins={p1:.12}, p2 wins={p2:.12}");
    }
}

#[test]
fn secondary_poison_probability_is_exact_in_both_modes() {
    use ninjask::dex::types::Status;
    let b = chance_battle(100, true, false);
    // ceil(0.30 * 2^32), matching random(100) < 30.
    let expected = 1_288_490_189.0 / 4_294_967_296.0;
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let out = b.enumerate_outcomes_text("move 1", "move 2", mode).unwrap();
        let poisoned = mass(&out, |b| b.sides[1].mons[0].status == Status::Psn);
        let healthy = mass(&out, |b| b.sides[1].mons[0].status == Status::None);
        near(poisoned, expected);
        near(healthy, 1.0 - expected);
        println!("{mode:?} secondary: poisoned={poisoned:.12}, healthy={healthy:.12}");
    }
}

#[test]
fn full_paralysis_branches_before_a_guaranteed_ko() {
    use ninjask::dex::types::Status;
    let mut b = chance_battle(100, false, false);
    b.sides[0].mons[0].status = Status::Par;
    b.sides[0].mons[0].status_state = Status::Par;
    b.sides[1].mons[0].hp = 1;
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let out = b.enumerate_outcomes_text("move 1", "move 2", mode).unwrap();
        let blocked = mass(&out, |b| !b.ended && b.sides[1].mons[0].hp == 1);
        let ko = mass(&out, |b| b.ended && b.winner == Some(0));
        near(blocked, 1.0 / 8.0);
        near(ko, 7.0 / 8.0);
        println!("{mode:?} paralysis: blocked={blocked:.12}, KO={ko:.12}");
    }
}

#[test]
fn pending_choice_and_reseeding_preserve_accuracy_crit_distribution() {
    let mut b = chance_battle(75, false, false);
    b.sides[1].mons[0].hp = 30;
    for mode in [EnumerationMode::Precise, EnumerationMode::AlmostPrecise] {
        let expected = distribution(&b.enumerate_outcomes_text("move 1", "move 2", mode).unwrap());
        let mut pending = b.clone();
        pending.choose(0, "move 1").unwrap();
        let before = pending.state_key();
        pending.rng = ninjask::rng::Rng::from_showdown_seed("sodium,1234").unwrap();
        let choice = [SlotChoice::Move {
            slot: 1,
            target: 0,
            mega: false,
        }];
        let actual = distribution(&pending.enumerate_outcomes(&[], &choice, mode).unwrap());
        assert_eq!(actual, expected);
        assert_eq!(pending.state_key(), before);
        assert!(pending.sides[0].choice.is_some());
    }
}

/// Compare event probabilities, not full HP states: endpoint approximation is
/// valid for the first four scenarios but not for the damage-threshold case.
#[test]
fn sampled_randomness_matches_enumerated_probabilities() {
    use ninjask::dex::types::Status;
    use ninjask::rng::Rng;
    const SAMPLES: usize = 100_000;
    const SIGMAS: f64 = 5.0;
    struct Scenario {
        name: &'static str,
        battle: Battle,
        choices: [&'static str; 2],
        mode: EnumerationMode,
        labels: &'static [&'static str],
        classify: fn(&Battle) -> usize,
    }
    let mut accuracy = chance_battle(75, false, false);
    accuracy.sides[1].mons[0].hp = 30;
    let mut tie = chance_battle(100, false, true);
    for side in &mut tie.sides {
        side.mons[0].hp = 1;
    }
    let poison = chance_battle(100, true, false);
    let mut paralysis = chance_battle(100, false, false);
    paralysis.sides[0].mons[0].status = Status::Par;
    paralysis.sides[0].mons[0].status_state = Status::Par;
    paralysis.sides[1].mons[0].hp = 1;
    let mut threshold = chance_battle(100, false, false);
    threshold.sides[1].mons[0].hp = 27;
    let scenarios = [
        Scenario {
            name: "accuracy+crit",
            battle: accuracy,
            choices: ["move 1", "move 2"],
            mode: EnumerationMode::AlmostPrecise,
            labels: &["miss", "normal hit", "critical KO"],
            classify: |b| {
                if b.ended {
                    2
                } else if b.sides[1].mons[0].hp == 30 {
                    0
                } else {
                    1
                }
            },
        },
        Scenario {
            name: "speed tie",
            battle: tie,
            choices: ["move 1", "move 1"],
            mode: EnumerationMode::AlmostPrecise,
            labels: &["p1 wins", "p2 wins"],
            classify: |b| {
                assert!(b.ended);
                b.winner.unwrap()
            },
        },
        Scenario {
            name: "secondary",
            battle: poison,
            choices: ["move 1", "move 2"],
            mode: EnumerationMode::AlmostPrecise,
            labels: &["poisoned", "healthy"],
            classify: |b| usize::from(b.sides[1].mons[0].status != Status::Psn),
        },
        Scenario {
            name: "paralysis",
            battle: paralysis,
            choices: ["move 1", "move 2"],
            mode: EnumerationMode::AlmostPrecise,
            labels: &["blocked", "KO"],
            classify: |b| usize::from(b.ended),
        },
        Scenario {
            name: "damage threshold (27 HP)",
            battle: threshold,
            choices: ["move 1", "move 2"],
            mode: EnumerationMode::Precise,
            labels: &["KO", "survives"],
            classify: |b| usize::from(!b.ended),
        },
    ];
    println!("Fixed seeds; {SAMPLES} samples per scenario per generator; tolerance = {SIGMAS} binomial standard errors + 1/sample.");
    for mut scenario in scenarios {
        scenario.battle.log_enabled = false;
        let out = scenario
            .battle
            .enumerate_outcomes_text(scenario.choices[0], scenario.choices[1], scenario.mode)
            .unwrap();
        let mut expected = vec![0.0; scenario.labels.len()];
        for o in &out {
            expected[(scenario.classify)(&o.battle)] += o.probability;
        }
        near(expected.iter().sum(), 1.0);
        if scenario.mode == EnumerationMode::Precise {
            let almost = scenario
                .battle
                .enumerate_outcomes_text(
                    scenario.choices[0],
                    scenario.choices[1],
                    EnumerationMode::AlmostPrecise,
                )
                .unwrap();
            let approximate_ko = mass(&almost, |b| b.ended);
            let crit = 178_956_971.0 / 4_294_967_296.0;
            // Six of sixteen normal rolls KO; all critical rolls KO.
            near(expected[0], crit + (1.0 - crit) * 6.0 / 16.0);
            assert!((approximate_ko - expected[0]).abs() > 0.1);
            println!(
                "Damage threshold requires Precise: KO={:.9}; AlmostPrecise KO={approximate_ko:.9}",
                expected[0]
            );
        }
        for generator in ["xoshiro", "sodium"] {
            let mut counts = vec![0usize; expected.len()];
            for trial in 0..SAMPLES {
                let seed = 0x517c_c1b7_2722_0a95u64.wrapping_add(trial as u64);
                let mut b = scenario.battle.clone();
                b.rng = if generator == "xoshiro" {
                    Rng::new(seed)
                } else {
                    Rng::from_showdown_seed(&format!("sodium,{seed:016x}")).unwrap()
                };
                b.choose(0, scenario.choices[0]).unwrap();
                b.choose(1, scenario.choices[1]).unwrap();
                counts[(scenario.classify)(&b)] += 1;
            }
            for (i, &probability) in expected.iter().enumerate() {
                let observed = counts[i] as f64 / SAMPLES as f64;
                let tolerance = SIGMAS
                    * (probability * (1.0 - probability) / SAMPLES as f64).sqrt()
                    + 1.0 / SAMPLES as f64;
                let difference = (observed - probability).abs();
                println!("{generator} | {} | {:?} | {}: calculated={probability:.6}, sampled={observed:.6}, tolerance=+/-{tolerance:.6}", scenario.name, scenario.mode, scenario.labels[i]);
                assert!(difference <= tolerance,
                    "{} / {generator} / {}: observed {observed}, expected {probability}, tolerance {tolerance}", scenario.name, scenario.labels[i]);
            }
        }
    }
}
