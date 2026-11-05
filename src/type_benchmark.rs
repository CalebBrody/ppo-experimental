//! Preserved pre-change encoder, used only by the explicit CPU microbenchmark.
use crate::model::*;
use ninjask::tracker::{ItemView, Tracker};

pub fn run(
    dex: std::sync::Arc<ninjask::Dex>,
    teams: std::sync::Arc<Vec<ninjask::battle::Team>>,
    out: &std::path::Path,
    iterations: usize,
) -> anyhow::Result<()> {
    let mut fixtures = Vec::new();
    for i in 0..16 {
        let b = ninjask::Battle::from_teams(
            dex.clone(),
            [&teams[i % teams.len()], &teams[(i + 1) % teams.len()]],
            ["p1", "p2"],
            i as u64,
        );
        let mut trackers = [Tracker::new(dex.clone(), 0), Tracker::new(dex.clone(), 1)];
        crate::env::sync(&b, &mut trackers, &mut 0);
        let mut t = trackers[0].clone();
        if i % 2 == 0 {
            t.sides[0].mons[0].types = Some(vec![dex.type_id("Water").unwrap()]);
        }
        if i % 3 == 0 {
            t.sides[1].mons[0].added_type = dex.type_id("Grass");
        }
        fixtures.push(t);
    }
    let measure = |encode: fn(&Tracker) -> Vec<f32>| {
        let timer = std::time::Instant::now();
        for _ in 0..iterations {
            for t in &fixtures {
                std::hint::black_box(encode(std::hint::black_box(t)));
            }
        }
        timer.elapsed().as_secs_f64()
    };
    for t in &fixtures {
        std::hint::black_box(legacy_encode_state(t));
        std::hint::black_box(crate::env::encode_state(t));
    }
    let old_a = measure(legacy_encode_state);
    let new_a = measure(crate::env::encode_state);
    let new_b = measure(crate::env::encode_state);
    let old_b = measure(legacy_encode_state);
    let calls = (iterations * fixtures.len() * 2) as f64;
    let old = (old_a + old_b) / calls;
    let new = (new_a + new_b) / calls;
    let result = serde_json::json!({"old_seconds_per_encode":old,"new_seconds_per_encode":new,
        "extra_microseconds_per_encode":(new-old)*1e6,"encoder_change_percent":100.*(new/old-1.),
        "calls_per_encoder":calls,"features":FEATURES,"state_width":STATE,"action_width":ACTION_FEATURES,
        "method":"ABBA release-mode CPU benchmark, identical preview-sized tracker fixtures with synthetic visible type overrides/additions. No games played, inference or training. Other CPU/system noise remains."});
    std::fs::write(
        out.join("type-encoding-benchmark.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn hash(out: &mut [f32], name: &str) {
    let h = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    out[h as usize % out.len()] += 1.;
}

fn legacy_encode_state(t: &Tracker) -> Vec<f32> {
    let mut out = vec![0.; STATE];
    for relative in 0..2 {
        let side = if relative == 0 { t.me } else { 1 - t.me };
        for (i, m) in t.sides[side].mons.iter().take(6).enumerate() {
            let f = &mut out[(relative * 6 + i) * FEATURES..(relative * 6 + i + 1) * FEATURES];
            f[0] = 1.;
            f[1] = relative as f32;
            f[8] = m.hp as f32 / m.maxhp.max(1) as f32;
            f[9] = m.fainted as u8 as f32;
            f[10] = m.seen as u8 as f32;
            if let Some(slot) = m.slot {
                f[11 + slot] = 1.;
            }
            hash(&mut f[13..21], m.status.id());
            for (j, v) in m.boosts.iter().enumerate() {
                f[21 + j] = *v as f32 / 6.;
            }
            if let Some(id) = m.species {
                let sp = t.dex().species(id);
                hash(&mut f[28..44], &sp.id);
                for j in 0..6 {
                    f[44 + j] = sp.base_stats[j] as f32 / 255.;
                }
                for ty in sp.types {
                    if ty < 18 {
                        f[50 + ty as usize] = 1.;
                    }
                }
            }
            if let Some(id) = m.ability {
                hash(&mut f[68..76], &t.dex().ability(id).id);
            }
            match m.item {
                ItemView::Known(id) => hash(&mut f[76..84], &t.dex().item(id).id),
                ItemView::None => f[84] = 1.,
                _ => {}
            }
            for (j, mv) in m.moves.iter().take(4).enumerate() {
                let data = t.dex().mv(mv.id);
                let start = 85 + j * 8;
                hash(&mut f[start..start + 4], &data.id);
                f[start + 4] = data.base_power as f32 / 250.;
                f[start + 5] = data.accuracy.unwrap_or(100) as f32 / 100.;
                f[start + 6] = mv.pp.unwrap_or(0) as f32 / 64.;
                f[start + 7] = mv.pp.is_some() as u8 as f32;
                let extra = 128 + j * 24;
                if data.type_ < 18 {
                    f[extra + data.type_ as usize] = 1.;
                }
                f[extra + 18 + data.category as usize] = 1.;
                f[extra + 21] = data.priority as f32 / 7.;
                f[extra + 22] = mv.uses as f32 / 64.;
                f[extra + 23] = 1.;
            }
            for v in &m.volatiles {
                hash(&mut f[117..128], v);
            }
            if let Some(stats) = m.stats {
                for (j, stat) in stats.iter().enumerate() {
                    f[224 + j] = *stat as f32 / 500.;
                }
                f[229] = 1.;
            }
            f[230] = m.mega as u8 as f32;
            if let Some(bounds) = t.speed_bounds(side, i) {
                f[231] = 1.;
                f[232] = bounds.min as f32 / 500.;
                f[233] = bounds.max as f32 / 500.;
                f[234] = (bounds.max - bounds.min) as f32 / 500.;
                f[235] = (bounds.min == bounds.max) as u8 as f32;
            }
        }
    }
    let f = &mut out[12 * FEATURES..];
    f[0] = t.turn as f32 / 150.;
    if let Some((w, start)) = t.field.weather {
        hash(&mut f[1..9], w.id());
        f[9] = t.turn.saturating_sub(start) as f32 / 8.;
    }
    if let Some((w, start)) = t.field.terrain {
        hash(&mut f[10..18], w.id());
        f[18] = t.turn.saturating_sub(start) as f32 / 8.;
    }
    f[19] = t.field.trick_room.is_some() as u8 as f32;
    for rel in 0..2 {
        let side = if rel == 0 { t.me } else { 1 - t.me };
        for (cond, _) in &t.sides[side].conditions {
            hash(&mut f[20 + rel * 16..36 + rel * 16], cond.id());
        }
    }
    out
}
