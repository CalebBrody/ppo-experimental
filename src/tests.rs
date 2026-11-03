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
