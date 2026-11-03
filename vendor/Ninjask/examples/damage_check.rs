//! The damage calculation against played outcomes:
//! `cargo run --release --example damage_check -- <dex.json> <teams.txt> [games] [seed]`.
//!
//! Plays random games, records every damaging hit with the distribution
//! the read-only calculation predicts for it (`ninjask::calc`), and
//! checks that each damage dealt is one of the predicted values, and that
//! critical hits, KOs and rolls come as often as predicted.

use std::sync::Arc;

use ninjask::calc::sample_games;
use ninjask::dex::team::parse_packed;
use ninjask::Dex;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dex = Arc::new(Dex::from_json(&std::fs::read_to_string(&args[1]).unwrap()).unwrap());
    let teams: Vec<_> = std::fs::read_to_string(&args[2])
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse_packed(&dex, l).unwrap())
        .collect();
    let games: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(2000);
    let seed: u64 = args.get(4).and_then(|x| x.parse().ok()).unwrap_or(1);
    let samples = sample_games(&dex, &teams, games, seed);
    let r = ninjask::calc::check_samples(&samples);
    println!("{r}");
    if std::env::var("DEBUG_WRONG").is_ok() {
        debug_wrong(&dex, &samples);
    }
}

#[allow(dead_code)]
fn debug_wrong(dex: &Dex, samples: &[ninjask::calc::DamageSample]) {
    for s in samples {
        if let (Some((sp, full)), Some(p), Some(after)) = (s.full, s.full_ko, s.hp_after) {
            if sp == s.spread
                && full == s.rolls
                && (p <= 1e-12 || p >= 1.0 - 1e-12)
                && ((after == 0) != (p > 0.5))
            {
                println!(
                    "wrong: {} p {p} hp {} dmg {} after {after}",
                    dex.mv(s.move_id).id,
                    s.target_hp,
                    s.damage
                );
            }
        }
    }
}
