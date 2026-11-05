// Frozen baseline feature encoder for cross-architecture evaluation only.
use crate::baseline_model::*;
use crate::env::Choice;
use burn::prelude::*;
use ninjask::{
    state::SlotChoice,
    tracker::{ItemView, Tracker},
    Battle,
};
use rand::rngs::SmallRng;
use rl4burn::{ActionDist, MaskedActorCritic};
fn hash(out: &mut [f32], name: &str) {
    let h = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    out[h as usize % out.len()] += 1.;
}

/// Reads only Tracker and static dex data. Never reads the opposing hidden set.
pub fn encode_state(t: &Tracker) -> Vec<f32> {
    let mut out = vec![0.; STATE];
    for relative in 0..2 {
        let side = if relative == 0 { t.me } else { 1 - t.me };
        for (i, m) in t.sides[side].mons.iter().take(6).enumerate() {
            let f = &mut out[(relative * 6 + i) * FEATURES..(relative * 6 + i + 1) * FEATURES];
            f[0] = 1.;
            f[1] = relative as f32;
            f[2 + i] = 1.;
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
            if let Some(pos) = m.team_pos {
                if pos < 6 {
                    f[231 + pos] = 1.;
                }
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

pub fn observe(b: &Battle, t: &Tracker, cs: &[Choice]) -> Vec<f32> {
    assert!(
        !cs.is_empty() && cs.len() <= ACTIONS,
        "legal action capacity exceeded: {}",
        cs.len()
    );
    let mut out = encode_state(t);
    out.resize(OBS, 0.);
    for (j, c) in cs.iter().enumerate() {
        let f = &mut out[STATE + j * ACTION_FEATURES..STATE + (j + 1) * ACTION_FEATURES];
        match c {
            Choice::Preview(order) => {
                f[31] = 1.;
                for (slot, &index) in order.iter().enumerate() {
                    f[slot * 6 + index] = 1.;
                }
            }
            Choice::Turn(slots) => {
                for (k, c) in slots.iter().enumerate() {
                    let f = &mut f[k * 16..(k + 1) * 16];
                    match c {
                        SlotChoice::Pass => f[0] = 1.,
                        SlotChoice::Switch { index } => {
                            f[1] = 1.;
                            f[2 + index] = 1.;
                        }
                        SlotChoice::Move { slot, target, mega } => {
                            f[8] = 1.;
                            f[9] = *target as f32 / 2.;
                            f[10] = *mega as u8 as f32;
                            if let Some(mv) = b.sides[t.me].mons[k].moves.get(*slot) {
                                let m = b.dex.mv(mv.id);
                                hash(&mut f[0..8], &m.id);
                                f[11] = m.base_power as f32 / 250.;
                                f[12] = m.accuracy.unwrap_or(100) as f32 / 100.;
                                f[13] = m.priority as f32 / 7.;
                                f[14] = *slot as f32 / 4.;
                            } else {
                                f[15] = 1.;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn sample<B: Backend>(
    model: &Model<B>,
    b: &Battle,
    t: &Tracker,
    cs: &[Choice],
    device: &B::Device,
    rng: &mut SmallRng,
) -> usize {
    let obs = Tensor::from_data(TensorData::new(observe(b, t, cs), [1, OBS]), device);
    let (logits, _) = model.forward(obs);
    let mut mask = vec![0.; ACTIONS];
    mask[..cs.len()].fill(1.);
    let mask = Tensor::from_data(TensorData::new(mask, [1, ACTIONS]), device);
    ActionDist::Discrete(ACTIONS).sample(&logits, Some(&mask), None, rng)[0][0] as usize
}
