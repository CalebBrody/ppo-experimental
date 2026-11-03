//! Behaviour fingerprint: `cargo run --release --example fingerprint -- <dex.json> <teams.txt> [games] [per-game-output]`.
//!
//! Plays seeded random-policy games and hashes everything observable: the log
//! for each viewer, every request, and every PRNG draw (value, range and the
//! queue/speed-sort notes) with the call sites left out, so moving code does
//! not change the result. Two builds that print the same numbers behave
//! identically. The optional fourth argument writes one hash per game, to find
//! the first game that differs.

use std::io::Write;
use std::sync::Arc;

use ninjask::battle::log::Viewer;
use ninjask::rng::Rng;
use ninjask::state::RequestState;
use ninjask::{Battle, Dex};

/// FNV-1a over 64 bits, fed length-prefixed strings so boundaries matter.
#[derive(Clone, Copy)]
struct Hash(u64);

impl Hash {
    fn new() -> Hash {
        Hash(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ x as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn text(&mut self, s: &str) {
        self.bytes(&(s.len() as u64).to_le_bytes());
        self.bytes(s.as_bytes());
    }
}

/// A trace entry without its trailing " @ file:line" call site.
fn strip_site(line: &str) -> &str {
    match line.find(" @ ") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dex = Arc::new(Dex::from_json(&std::fs::read_to_string(&args[1]).unwrap()).unwrap());
    let teams: Vec<String> = std::fs::read_to_string(&args[2])
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    let games: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(2000);
    let mut per_game = args.get(4).map(|p| std::fs::File::create(p).unwrap());

    let (mut logs, mut requests, mut draws, mut all) =
        (Hash::new(), Hash::new(), Hash::new(), Hash::new());
    let (mut lines, mut n_requests, mut n_draws, mut turns) = (0usize, 0usize, 0usize, 0u64);
    for g in 0..games {
        let t1 = &teams[g % teams.len()];
        let t2 = &teams[(g * 7 + 3) % teams.len()];
        let mut rng = Rng::new(g as u64);
        rng.trace = Some(Vec::new());
        let mut b =
            Battle::new_with_rng(dex.clone(), [t1, t2], ["Player 1", "Player 2"], rng).unwrap();
        let mut policy = Rng::new(1_000_003 + g as u64);
        let (mut game_logs, mut game_requests, mut game_draws) =
            (Hash::new(), Hash::new(), Hash::new());
        let note_requests = |b: &Battle, h: &mut Hash, n: &mut usize| {
            for s in 0..2 {
                if let Some(r) = b.request_json(s) {
                    h.text(&r.to_string());
                    *n += 1;
                }
            }
        };
        note_requests(&b, &mut game_requests, &mut n_requests);
        for s in 0..2 {
            let n = b.sides[s].mons.len();
            let mut order: Vec<usize> = (0..n).collect();
            policy.shuffle(&mut order, 0, n);
            b.choose_team(s, order);
            note_requests(&b, &mut game_requests, &mut n_requests);
        }
        while !b.ended && b.turn < 150 {
            for s in 0..2 {
                if b.request == RequestState::Ended || !b.side_needs_choice(s) {
                    continue;
                }
                let c = b.sample_joint_choice(s, &mut policy);
                b.choose_structured(s, c);
                note_requests(&b, &mut game_requests, &mut n_requests);
            }
        }
        for viewer in [
            Viewer::Omniscient,
            Viewer::Side(0),
            Viewer::Side(1),
            Viewer::Spectator,
        ] {
            for line in b.log_lines(viewer, 0) {
                game_logs.text(&line);
                lines += 1;
            }
            game_logs.text("--");
        }
        game_logs.text(&format!("end {} {:?}", b.turn, b.winner_name()));
        for line in b.rng.trace.take().unwrap_or_default() {
            game_draws.text(strip_site(&line));
            n_draws += 1;
        }
        turns += b.turn as u64;
        for (h, x) in [
            (&mut logs, game_logs),
            (&mut requests, game_requests),
            (&mut draws, game_draws),
        ] {
            h.bytes(&x.0.to_le_bytes());
        }
        let game = [game_logs.0, game_requests.0, game_draws.0];
        for x in game {
            all.bytes(&x.to_le_bytes());
        }
        if let Some(f) = per_game.as_mut() {
            writeln!(f, "{g} {:016x} {:016x} {:016x}", game[0], game[1], game[2]).unwrap();
        }
    }
    println!("games {games} turns {turns} log_lines {lines} requests {n_requests} draws {n_draws}");
    println!("logs {:016x}", logs.0);
    println!("requests {:016x}", requests.0);
    println!("draws {:016x}", draws.0);
    println!("fingerprint {:016x}", all.0);
}
