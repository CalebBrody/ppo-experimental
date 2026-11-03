//! Random-policy throughput: `cargo run --release --example bench -- <dex.json> <teams.txt> [games] [--log]`.
//! Logging is off unless `--log` is given (Showdown always logs; see refsim/bench_showdown.js).

use std::sync::Arc;
use std::time::Instant;

use ninjask::battle::Team;
use ninjask::dex::team::parse_packed;
use ninjask::rng::Rng;
use ninjask::state::RequestState;
use ninjask::{Battle, Dex};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dex = Arc::new(Dex::from_json(&std::fs::read_to_string(&args[1]).unwrap()).unwrap());
    // Each team is parsed and prepared once; a game builds its battle from them.
    let teams: Vec<Team> = std::fs::read_to_string(&args[2])
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Team::new(&dex, &parse_packed(&dex, l).unwrap()))
        .collect();
    let games: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(20000);
    let log = args.iter().any(|a| a == "--log");
    let mut rng = Rng::new(7);
    let start = Instant::now();
    let mut turns = 0u64;
    let mut decisions = 0u64;
    for g in 0..games {
        let t1 = &teams[g % teams.len()];
        let t2 = &teams[(g * 7 + 3) % teams.len()];
        let mut b = Battle::from_teams(dex.clone(), [t1, t2], ["Player 1", "Player 2"], g as u64);
        b.log_enabled = log;
        for s in 0..2 {
            let n = b.sides[s].mons.len();
            let mut order: Vec<usize> = (0..n).collect();
            rng.shuffle(&mut order, 0, n);
            b.choose_team(s, order);
        }
        while !b.ended && b.turn < 150 {
            for s in 0..2 {
                if b.request == RequestState::Ended || !b.side_needs_choice(s) {
                    continue;
                }
                let c = b.sample_joint_choice(s, &mut rng);
                b.choose_structured(s, c);
                decisions += 1;
            }
        }
        turns += b.turn as u64;
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{games} games, {turns} turns, {decisions} decisions in {secs:.2}s: {:.0} games/s, {:.0} turns/s (one thread)",
        games as f64 / secs,
        turns as f64 / secs
    );
}
