//! `cargo run --release --example bench_outcomes`
//! Synthetic scenarios from the enumeration regression fixture, not a workload forecast.
use ninjask::state::SlotChoice;
use ninjask::{Battle, Dex, EnumerationMode};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

fn measure(
    b: &Battle,
    choices: &[Vec<SlotChoice>; 2],
    mode: EnumerationMode,
    repetitions: usize,
) -> f64 {
    let start = Instant::now();
    for _ in 0..repetitions {
        black_box(
            b.enumerate_outcomes(black_box(&choices[0]), black_box(&choices[1]), mode)
                .unwrap(),
        );
    }
    start.elapsed().as_secs_f64() * 1e6 / repetitions as f64
}

fn main() {
    let dex = Arc::new(Dex::from_json(include_str!("../tests/fixtures/enumeration.json")).unwrap());
    let mut b = Battle::new(
        dex,
        [
            "Fast|||battlearmor|Tackle,Splash,Double Hit|Hardy|||||50|",
            "Slow|||battlearmor|Tackle,Splash,Double Hit|Hardy|||||50|",
        ],
        ["a", "b"],
        1,
    )
    .unwrap();
    b.log_enabled = false;
    b.choose_team(0, vec![0]);
    b.choose_team(1, vec![0]);
    println!("Synthetic singles; Battle Armor; logging off; setup excluded; seven alternating batches, median microseconds/call.");
    println!("scenario,precise_us,almost_us,speedup,precise_states,almost_states,precise_reps,almost_reps");
    for (name, slots) in [
        ("no damage", [1, 1]),
        ("one hit", [0, 1]),
        ("two-hit move", [2, 1]),
        ("both attack", [0, 0]),
    ] {
        let choices = std::array::from_fn(|side| {
            b.joint_choices(side).into_iter().find(|c| matches!(c.as_slice(), [SlotChoice::Move {slot, ..}] if *slot == slots[side])).unwrap()
        });
        let modes = [EnumerationMode::Precise, EnumerationMode::AlmostPrecise];
        let counts: Vec<_> = modes
            .iter()
            .map(|&mode| {
                let out = b
                    .enumerate_outcomes(&choices[0], &choices[1], mode)
                    .unwrap();
                assert!((out.iter().map(|o| o.probability).sum::<f64>() - 1.0).abs() < 1e-12);
                out.len()
            })
            .collect();
        let reps: Vec<_> = modes
            .iter()
            .map(|&mode| {
                let us = measure(&b, &choices, mode, 3);
                (30_000.0 / us).clamp(1.0, 10_000.0) as usize
            })
            .collect();
        let mut timings = [Vec::new(), Vec::new()];
        for round in 0..7 {
            for offset in 0..2 {
                let i = (round + offset) % 2;
                timings[i].push(measure(&b, &choices, modes[i], reps[i]));
            }
        }
        for times in &mut timings {
            times.sort_by(f64::total_cmp);
        }
        let precise = timings[0][3];
        let almost = timings[1][3];
        println!(
            "{name},{precise:.2},{almost:.2},{:.2},{},{},{},{}",
            precise / almost,
            counts[0],
            counts[1],
            reps[0],
            reps[1]
        );
    }
}
