//! The README's Rust quick start: one game between random players.
//! `cargo run --release --example quickstart -- <format.json> <teams.txt>`

use std::error::Error;
use std::sync::Arc;

use ninjask::battle::log::Viewer;
use ninjask::rng::Rng;
use ninjask::{Battle, Dex};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dex = Arc::new(Dex::from_json(&std::fs::read_to_string(&args[1])?)?);
    let pool = std::fs::read_to_string(&args[2])?; // one packed team per line
    let teams: Vec<&str> = pool.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut battle = Battle::new(dex, [teams[0], teams[1]], ["Alice", "Bob"], 1)?;
    let mut rng = Rng::new(1);
    while !battle.ended && battle.turn < 150 {
        for side in 0..2 {
            if battle.side_needs_choice(side) {
                // Any choice in Showdown's syntax works, e.g. "move 1 1, switch 3".
                let choice = battle.random_choice(side, &mut rng);
                battle.choose(side, &choice)?;
            }
        }
    }
    println!(
        "winner: {:?} after {} turns",
        battle.winner_name(),
        battle.turn
    );
    for line in battle.log_lines(Viewer::Spectator, 0).iter().take(12) {
        println!("{line}");
    }
    Ok(())
}
