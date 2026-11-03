use crate::{env::*, model::*};
use ninjask::{tracker::Tracker, Battle, Dex};
use std::sync::Arc;

fn battle() -> Battle {
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
                    (s, cs[rng.random(cs.len() as u32) as usize].clone())
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
