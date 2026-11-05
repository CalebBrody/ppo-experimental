use crate::{env::*, model::*};
use ninjask::{tracker::Tracker, Battle, Dex};
use std::sync::Arc;

#[test]
fn reward_per_turn_excludes_preview_and_combines_forced_choices() {
    let mut stats = TurnRewards::default();
    stats.record(1, false, false, 0.);
    assert_eq!(stats.turns, 0);
    stats.record(1, false, false, 0.125);
    assert_eq!(stats.turns, 0);
    stats.record(2, false, false, 0.125);
    assert_eq!(stats.turns, 1);
    assert_eq!(stats.reward, 0.25);
    stats.record(2, true, true, 1.25);
    assert_eq!(stats.turns, 2);
    assert_eq!(stats.reward, 1.5);
    stats.record(1, false, false, 0.);
    assert_eq!(stats.turns, 2);
    stats.record(1, false, true, 0.125);
    assert_eq!(stats.turns, 2);
    assert_eq!(stats.unfinished_reward, 0.125);
}

#[test]
fn upstream_collection_preserves_environment_order_and_terminal_resets() {
    use rl4burn::{Env, Space, Step, SyncVecEnv};
    struct Counter {
        step: usize,
        end: usize,
    }
    impl Env for Counter {
        type Observation = Vec<f32>;
        type Action = usize;
        fn reset(&mut self) -> Vec<f32> {
            self.step = 0;
            vec![0.]
        }
        fn step(&mut self, action: usize) -> Step<Vec<f32>> {
            self.step += 1;
            Step {
                observation: vec![self.step as f32],
                reward: action as f32,
                terminated: self.step == self.end,
                truncated: false,
            }
        }
        fn observation_space(&self) -> Space {
            Space::Box {
                low: vec![0.],
                high: vec![10.],
            }
        }
        fn action_space(&self) -> Space {
            Space::Discrete(3)
        }
    }
    let make = || {
        vec![
            Counter { step: 0, end: 2 },
            Counter { step: 0, end: 3 },
            Counter { step: 0, end: 4 },
        ]
    };
    let mut envs = SyncVecEnv::new(make());
    assert_eq!(envs.reset(), vec![vec![0.]; 3]);
    for turn in 1..=12 {
        for (i, step) in envs.step(vec![0, 1, 2]).iter().enumerate() {
            let period = i + 2;
            assert_eq!(step.observation, vec![(turn % period) as f32]);
            assert_eq!(step.reward, i as f32);
            assert_eq!(step.terminated, turn % period == 0);
            assert!(!step.truncated);
        }
    }
}

pub(crate) fn battle() -> Battle {
    let dex = Arc::new(
        Dex::from_json(
            &std::fs::read_to_string(
                "vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json",
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let text = std::fs::read_to_string("data/teams.txt").unwrap();
    let team = text
        .lines()
        .find(|l| ninjask::dex::support::team_problems(&dex, l).is_ok_and(|p| p.is_empty()))
        .unwrap();
    Battle::new(dex, [team, team], ["p1", "p2"], 37).unwrap()
}

#[test]
fn candidate_actions_carry_selected_move_types_for_both_actors() {
    use ninjask::state::SlotChoice;
    let mut b = reward_battle();
    b.sides[0].mons[0].moves[0].id = b.dex.move_id("direclaw").unwrap();
    b.sides[0].mons[0].moves[1].id = b.dex.move_id("highhorsepower").unwrap();
    b.sides[0].mons[1].moves[0].id = b.dex.move_id("scald").unwrap();
    let mut ts = [
        Tracker::new(b.dex.clone(), 0),
        Tracker::new(b.dex.clone(), 1),
    ];
    sync(&b, &mut ts, &mut 0);
    let mv = |slot| SlotChoice::Move {
        slot,
        target: 1,
        mega: false,
    };
    let cs = vec![
        Choice::Turn(vec![mv(0), mv(0)]),
        Choice::Turn(vec![mv(1), SlotChoice::Pass]),
        Choice::Turn(vec![SlotChoice::Switch { index: 2 }, SlotChoice::Pass]),
    ];
    let obs = observe(&b, &ts[0], &cs);
    let types = |candidate, actor| {
        let start =
            STATE + candidate * ACTION_FEATURES + ACTION_TYPE_OFFSET + actor * ACTION_TYPE_COUNT;
        obs[start..start + ACTION_TYPE_COUNT].to_vec()
    };
    let one = |index| {
        let mut flags = vec![0.; 20];
        flags[index] = 1.;
        flags
    };
    assert_eq!(types(0, 0), one(13)); // Dire Claw: Poison
    assert_eq!(types(0, 1), one(17)); // Scald: Water, independent of raw dex ID
    assert_eq!(types(1, 0), one(10)); // Selected alternative: Ground
    assert_eq!(types(1, 1), vec![0.; 20]); // Pass
    assert_eq!(types(2, 0), vec![0.; 20]); // Switch
    assert_eq!(types(2, 1), vec![0.; 20]);
    let preview = battle();
    sync(&preview, &mut ts, &mut 0);
    let cs = choices(&preview, 0);
    let obs = observe(&preview, &ts[0], &cs);
    assert_eq!(
        &obs[STATE + ACTION_TYPE_OFFSET..STATE + ACTION_DESCRIPTOR],
        &[0.; 40]
    );
    assert_eq!(obs[STATE + 31], 1.); // Preview flag does not overlap type bits
                                     // The network's action layer must actually accept the extended descriptor.
    #[cfg(feature = "cpu")]
    {
        use burn::module::Module;
        let model = Model::<burn::backend::NdArray>::new(&Default::default());
        assert_eq!(model.action_input_width(), 72);
        assert_eq!(model.num_params(), 145858);
    }
}

#[test]
fn explicit_types_include_water_and_replace_defaults_after_type_changes() {
    let b = battle();
    let mut t = Tracker::new(b.dex.clone(), 0);
    for line in [
        "|poke|p1|Pawmot, L50|",
        "|poke|p2|Milotic, L50|",
        "|switch|p1a: Pawmot|Pawmot, L50|100/100",
        "|switch|p2a: Milotic|Milotic, L50|100/100",
    ] {
        t.feed(line);
    }
    let bit = |name: &str| 236 + TYPE_FEATURE_NAMES.iter().position(|x| *x == name).unwrap();
    let water = TYPE_FEATURE_NAMES
        .iter()
        .position(|x| *x == "Water")
        .unwrap();
    let before = encode_state(&t);
    assert_eq!(before[bit("Electric")], 1.);
    assert_eq!(before[bit("Fighting")], 1.);
    let foe = 6 * FEATURES;
    assert_eq!(before[foe + bit("Water")], 1.);
    assert_eq!(before[foe + 50 + water], 1.);
    t.feed("|-start|p1a: Pawmot|typechange|???/Fighting|[from] move: Double Shock");
    let after = encode_state(&t);
    assert_eq!(after[bit("Electric")], 0.);
    assert_eq!(after[50 + 3], 0.);
    assert_eq!(after[bit("???")], 1.);
    assert_eq!(after[bit("Fighting")], 1.);
    t.feed("|-start|p2a: Milotic|typechange|Ghost");
    t.feed("|-start|p2a: Milotic|typeadd|Grass");
    let changed = encode_state(&t);
    assert_eq!(changed[foe + bit("Water")], 0.);
    assert_eq!(changed[foe + bit("Ghost")], 1.);
    assert_eq!(changed[foe + bit("Grass")], 1.);
}

#[test]
fn water_move_type_does_not_collide_with_move_category() {
    let b = battle();
    let mut t = Tracker::new(b.dex.clone(), 0);
    t.feed("|poke|p1|Milotic, L50|");
    t.feed("|switch|p1a: Milotic|Milotic, L50|100/100");
    t.feed("|move|p1a: Milotic|Scald|p2a: Target");
    let obs = encode_state(&t);
    let extra = 128;
    assert_eq!(obs[extra + 17], 1.); // Water is the 18th ordinary type, not raw dex ID 18.
    assert_eq!(
        obs[extra + 18 + b.dex.mv(b.dex.move_id("scald").unwrap()).category as usize],
        1.
    );
}

fn reward_battle() -> Battle {
    let mut b = battle();
    b.choose_team(0, vec![0, 1, 2, 3]);
    b.choose_team(1, vec![0, 1, 2, 3]);
    assert_eq!(b.sides[0].mons.len(), 4);
    for side in &mut b.sides {
        for m in &mut side.mons {
            m.maxhp = 100;
            m.hp = 100;
            m.fainted = false;
        }
    }
    b
}

#[test]
fn hp_reward_matches_scale_and_counts_bench_and_fainted_pokemon() {
    let mut b = reward_battle();
    assert_eq!(hp_advantage(&b, 0), 0.);
    for m in &mut b.sides[1].mons {
        m.hp = 0;
        m.fainted = true;
    }
    assert_eq!(battle_reward(&b, 0, false, 0.), 0.5);
    assert_eq!(battle_reward(&b, 1, false, 0.), -0.5);
    // One of four Pokemon at zero means 75% team health, not 100% of survivors.
    b.sides[0].mons[3].hp = 0;
    b.sides[0].mons[3].fainted = true;
    assert_eq!(hp_advantage(&b, 0), 0.375);
    // Equal weighting per Pokemon, irrespective of its maximum HP.
    b.sides[0].mons[2].maxhp = 200;
    b.sides[0].mons[2].hp = 100;
    assert_eq!(hp_advantage(&b, 0), 0.3125);
}

#[test]
fn hp_reward_pays_only_for_progress_and_reversals_cancel() {
    let mut b = reward_battle();
    for m in &mut b.sides[0].mons {
        m.hp = 80;
    }
    for m in &mut b.sides[1].mons {
        m.hp = 40;
    }
    let reward = battle_reward(&b, 0, false, 0.);
    assert!((reward - 0.2).abs() < 1e-6);
    let previous = hp_advantage(&b, 0);
    assert_eq!(battle_reward(&b, 0, false, previous), 0.);
    assert!((battle_reward(&b, 1, false, 0.) + 0.2).abs() < 1e-6);
    for m in &mut b.sides[1].mons {
        m.hp = 80;
    }
    assert!((reward + battle_reward(&b, 0, false, previous)).abs() < 1e-6);
}

#[test]
fn terminal_results_add_only_final_progress_and_never_reset_advantage() {
    let mut b = reward_battle();
    b.sides[1].mons[0].hp = 0;
    let previous = hp_advantage(&b, 0);
    assert!(previous > 0.);
    assert_eq!(battle_reward(&b, 0, true, previous), 0.); // Unchanged horizon draw.
    b.ended = true;
    b.winner = Some(0);
    // The final knockout improves the HP advantage from 0.125 to 0.5.
    for m in &mut b.sides[1].mons {
        m.hp = 0;
        m.fainted = true;
    }
    assert_eq!(battle_reward(&b, 0, true, previous), 1.375);
    assert_eq!(battle_reward(&b, 1, true, -previous), -1.375);
    b.winner = None;
    assert_eq!(battle_reward(&b, 0, true, 0.5), 0.);
    assert_eq!(battle_reward(&battle(), 0, false, 0.), 0.); // Preview.
}

#[test]
fn total_hp_reward_is_independent_of_idle_decisions_and_progress_path() {
    let mut b = reward_battle();
    let mut previous = hp_advantage(&b, 0);
    let mut total = 0.;
    for foe_hp in [80, 80, 40, 60, 0] {
        for m in &mut b.sides[1].mons {
            m.hp = foe_hp;
        }
        total += battle_reward(&b, 0, false, previous);
        previous = hp_advantage(&b, 0);
    }
    assert!((total - 0.5).abs() < 1e-6);
    b.ended = true;
    b.winner = Some(0);
    total += battle_reward(&b, 0, true, previous);
    assert!((total - 1.5).abs() < 1e-6);
}

#[test]
fn preview_has_all_ordered_four_of_six() {
    let b = battle();
    let cs = choices(&b, 0);
    assert_eq!(cs.len(), 360);
    let mut keys = std::collections::HashSet::new();
    for c in cs {
        if let Choice::Preview(order) = c {
            assert!(keys.insert(order));
        } else {
            panic!();
        }
    }
}

#[test]
fn hidden_opponent_spread_does_not_enter_observation() {
    let b = battle();
    let mut alternate = b.clone();
    alternate.sides[1].mons[0].stats[1] += 50;
    alternate.sides[1].mons[0].stats[4] += 50;
    alternate.sides[1].mons[0].ability = alternate.dex.ability_id("intimidate").unwrap();
    let get = |b: &Battle| {
        let mut t = [
            Tracker::new(b.dex.clone(), 0),
            Tracker::new(b.dex.clone(), 1),
        ];
        sync(b, &mut t, &mut 0);
        observe(b, &t[0], &choices(b, 0))
    };
    assert_eq!(get(&b), get(&alternate));
}

#[test]
fn speed_features_use_tracker_bounds_and_observations() {
    let b = battle();
    let mut t = Tracker::new(b.dex.clone(), 0);
    for line in [
        "|poke|p2|Gholdengo, L50|",
        "|switch|p1a: Arcanine|Arcanine-Hisui, L50|100/100",
        "|switch|p2a: Gholdengo|Gholdengo, L50|100/100",
        "|-ability|p1a: Arcanine|Rock Head",
        "|-item|p1a: Arcanine|Focus Sash",
        "|-ability|p2a: Gholdengo|Good as Gold",
        "|-item|p2a: Gholdengo|Life Orb",
        "|turn|1",
    ] {
        t.feed(line);
    }
    t.sides[0].mons[0].stats = Some([100, 100, 100, 100, 120]);
    let before = encode_state(&t);
    assert_eq!(before[231], 1.0);
    assert_eq!(before[232], 120.0 / 500.0);
    assert_eq!(before[235], 1.0);
    let foe = 6 * FEATURES;
    assert_eq!(before[foe + 233], 149.0 / 500.0);
    t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
    t.feed("|-damage|p2a: Gholdengo|80/100");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    let after = encode_state(&t);
    assert_eq!(after[foe + 233], 120.0 / 500.0);
    assert_eq!(after[foe + 232], 93.0 / 500.0);
    t.sides[1].mons[0].transformed = t.sides[1].mons[0].species;
    assert_eq!(&encode_state(&t)[foe + 231..foe + 236], &[0.; 5]);
}

#[cfg(feature = "cpu")]
#[test]
fn prepared_opponent_choice_is_not_submitted_early_and_is_used_by_step() {
    use rl4burn::Env;
    use std::sync::Mutex;
    let b = battle();
    let dex = b.dex.clone();
    let text = std::fs::read_to_string("data/teams.txt").unwrap();
    let packed = text
        .lines()
        .find(|s| ninjask::dex::support::team_problems(&dex, s).is_ok_and(|p| p.is_empty()))
        .unwrap();
    let sets = ninjask::dex::team::parse_packed(&dex, packed).unwrap();
    let team = ninjask::battle::Team::new(&dex, &sets);
    type B = burn::backend::NdArray;
    let device = Default::default();
    let league = Arc::new(Mutex::new(vec![Model::<B>::new(&device)]));
    let mut e = BattleEnv::new(
        dex,
        Arc::new(vec![team.clone(), team]),
        league,
        device,
        137,
        150,
    );
    e.reset();
    let foe = 1 - e.learner_side();
    let expected: Vec<_> = [5, 4, 3, 2]
        .iter()
        .map(|&i| e.battle.sides[foe].mons[i].name.to_string())
        .collect();
    let cursor = e.battle.log.len();
    e.prepare_opponent(Choice::Preview(vec![5, 4, 3, 2]));
    assert_eq!(e.battle.request, ninjask::state::RequestState::TeamPreview);
    assert_eq!(e.battle.log.len(), cursor);
    e.step(vec![0.]);
    let actual: Vec<_> = e.battle.sides[foe]
        .mons
        .iter()
        .map(|m| m.name.to_string())
        .collect();
    assert_eq!(actual, expected);
    // The queued action was consumed; preparing the next action is allowed.
    if e.battle.side_needs_choice(foe) {
        e.prepare_opponent(choices(&e.battle, foe)[0].clone());
    }
}

#[cfg(feature = "cpu")]
#[test]
fn compact_action_scoring_and_value_only_match_full_forward() {
    use burn::prelude::*;
    use rl4burn::MaskedActorCritic;
    type B = burn::backend::NdArray;
    let device = Default::default();
    let model = Model::<B>::new(&device);
    let b = reward_battle();
    let mut ts = [
        Tracker::new(b.dex.clone(), 0),
        Tracker::new(b.dex.clone(), 1),
    ];
    sync(&b, &mut ts, &mut 0);
    let cs = choices(&b, 0);
    let cs = &cs[..cs.len().min(8)];
    let full = observe(&b, &ts[0], cs);
    let compact = observe_padded(&b, &ts[0], cs, cs.len());
    let (a, va) = model.forward(Tensor::from_data(TensorData::new(full, [1, OBS]), &device));
    let (b, vb) = model.forward(Tensor::from_data(
        TensorData::new(compact, [1, STATE + cs.len() * ACTION_FEATURES]),
        &device,
    ));
    let vc = model.state_value(Tensor::from_data(
        TensorData::new(encode_state(&ts[0]), [1, STATE]),
        &device,
    ));
    let a = a.into_data().to_vec::<f32>().unwrap();
    let b = b.into_data().to_vec::<f32>().unwrap();
    assert!(a[..cs.len()]
        .iter()
        .zip(&b)
        .all(|(a, b)| (a - b).abs() < 1e-5));
    assert!((va.clone() - vb).abs().into_scalar() < 1e-5);
    assert!((va - vc).abs().into_scalar() < 1e-5);
}

#[cfg(feature = "cpu")]
#[test]
fn policy_and_value_are_invariant_to_entity_and_action_order() {
    use burn::prelude::*;
    use rl4burn::MaskedActorCritic;
    type B = burn::backend::NdArray;
    let device = Default::default();
    B::seed(&device, 37);
    let model = Model::<B>::new(&device);
    let mut b = battle();
    for phase in 0..2 {
        if phase == 1 {
            apply(&mut b, 0, Choice::Preview(vec![0, 1, 2, 3]));
            apply(&mut b, 1, Choice::Preview(vec![0, 1, 2, 3]));
        }
        let mut ts = [
            Tracker::new(b.dex.clone(), 0),
            Tracker::new(b.dex.clone(), 1),
        ];
        sync(&b, &mut ts, &mut 0);
        let cs = choices(&b, 0);
        let original = observe(&b, &ts[0], &cs);
        let mut shuffled = ts[0].clone();
        shuffled.sides[0].mons.rotate_left(2);
        shuffled.sides[1].mons.reverse();
        let mut reordered_actions = cs.clone();
        reordered_actions.reverse();
        let shuffled = observe(&b, &shuffled, &reordered_actions);
        let run =
            |x: Vec<f32>| model.forward(Tensor::from_data(TensorData::new(x, [1, OBS]), &device));
        let (a, va) = run(original);
        let (z, vz) = run(shuffled);
        let a = a.into_data().to_vec::<f32>().unwrap();
        let z = z.into_data().to_vec::<f32>().unwrap();
        let delta = (0..cs.len())
            .map(|i| (a[i] - z[cs.len() - 1 - i]).abs())
            .fold(0.0f32, f32::max);
        assert!(delta < 1e-5, "phase {phase}: logits differ by {delta}");
        let delta = (va - vz).abs().max().into_data().to_vec::<f32>().unwrap()[0];
        assert!(delta < 1e-5, "phase {phase}: value differs by {delta}");
    }
}

#[test]
fn legal_actions_and_observations_survive_full_games() {
    let mut rng = ninjask::rng::Rng::new(37);
    for _ in 0..8 {
        let mut b = battle();
        b.rng = ninjask::rng::Rng::new(rng.next_u64());
        let mut ts = [
            Tracker::new(b.dex.clone(), 0),
            Tracker::new(b.dex.clone(), 1),
        ];
        let mut cursor = 0;
        let mut decisions = 0;
        while !b.ended && b.turn < 150 && decisions < 512 {
            sync(&b, &mut ts, &mut cursor);
            let selected: Vec<_> = (0..2)
                .filter(|&s| b.side_needs_choice(s))
                .map(|s| {
                    let cs = choices(&b, s);
                    let obs = observe(&b, &ts[s], &cs);
                    assert_eq!(obs.len(), OBS);
                    assert!(obs.iter().all(|v| v.is_finite()));
                    (s, cs[rng.random(cs.len() as u32).resolve(&mut rng) as usize].clone())
                })
                .collect();
            assert!(!selected.is_empty());
            for (s, c) in selected {
                apply(&mut b, s, c);
            }
            decisions += 1;
        }
        assert!(b.ended || b.turn >= 150 || decisions == 512);
    }
}


#[test]
fn gradient_clipping_preserves_small_and_scales_large_gradients() {
    use burn::{prelude::*, optim::GradientsParams};
    use crate::{Inner, Train};
    let device=Default::default();
    for magnitude in [0.1_f32, 0.5, 2.0] {
        let layer=burn::nn::LinearConfig::new(1,1).with_bias(false).init::<Train>(&device);
        let input=Tensor::<Train,2>::from_data([[1.0]],&device);
        let loss=layer.forward(input).mul_scalar(magnitude).sum();
        let grads=GradientsParams::from_grads(loss.backward(),&layer);
        let mut clipped=rl4burn::clip_grad_norm(&layer,grads,0.5);
        let grad=clipped.remove::<Inner,2>(layer.weight.id.clone())
            .expect("clipping must retain sub-threshold gradients");
        let expected=magnitude*(0.5/(magnitude+1e-6)).min(1.0);
        assert!((grad.into_scalar()-expected).abs()<1e-6);
    }
}
