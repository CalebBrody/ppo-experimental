//! A battle built from prepared teams (`Battle::from_teams`) plays exactly
//! like one built from the sets. Needs the format export
//! (`data/derived/format/`, see `common`); skipped without it.

mod common;

use std::sync::Arc;

use ninjask::battle::log::Viewer;
use ninjask::battle::Team;
use ninjask::dex::team::{parse_packed, MonSet};
use ninjask::rng::Rng;
use ninjask::state::RequestState;
use ninjask::{Battle, Dex};

fn setup() -> Option<(Arc<Dex>, Vec<Vec<MonSet>>)> {
    let json = common::format_export("gen9championsvgc2026regmc")?;
    let root = env!("CARGO_MANIFEST_DIR");
    let dex = Arc::new(Dex::from_json(&json).ok()?);
    let teams = std::fs::read_to_string(format!("{root}/tests/fixtures/regmc_teams.txt"))
        .ok()?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse_packed(&dex, l).unwrap())
        .collect();
    Some((dex, teams))
}

/// Plays a seeded random game to the end and returns its full log.
fn play(mut b: Battle, seed: u64) -> Vec<String> {
    let mut policy = Rng::new(seed);
    b.show_team_sheets();
    for s in 0..2 {
        let n = b.sides[s].mons.len();
        let mut order: Vec<usize> = (0..n).collect();
        policy.shuffle(&mut order, 0, n);
        b.choose_team(s, order);
    }
    while !b.ended && b.turn < 100 {
        for s in 0..2 {
            if b.request == RequestState::Ended || !b.side_needs_choice(s) {
                continue;
            }
            let c = b.sample_joint_choice(s, &mut policy);
            b.choose_structured(s, c);
        }
    }
    b.log_lines(Viewer::Omniscient, 0)
}

#[test]
fn prepared_teams_play_like_their_sets() {
    let Some((dex, teams)) = setup() else { return };
    let prepared: Vec<Team> = teams.iter().map(|t| Team::new(&dex, t)).collect();
    for g in 0..40 {
        let (i, j) = (g % teams.len(), (g * 3 + 1) % teams.len());
        let names = ["Player 1", "Player 2"];
        let from_sets = Battle::from_sets(dex.clone(), [&teams[i], &teams[j]], names, g as u64);
        let from_teams =
            Battle::from_teams(dex.clone(), [&prepared[i], &prepared[j]], names, g as u64);
        assert_eq!(
            play(from_sets, g as u64),
            play(from_teams, g as u64),
            "game {g}"
        );
    }
}
