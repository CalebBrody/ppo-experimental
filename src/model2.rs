//! Model2: public-view one-turn matrix search, regret matching, and distillation.
use crate::{
    env::{self, BattleEnv, Choice},
    model::*,
    online_audit, Args, Inner, Train,
};
use anyhow::{ensure, Context, Result};
use burn::{
    module::{AutodiffModule, Module},
    optim::{AdamConfig, GradientsParams, Optimizer},
    prelude::*,
    record::{BinFileRecorder, FullPrecisionSettings},
    tensor::activation::log_softmax,
};
use ninjask::{
    battle::{log::Viewer, Team},
    dex::team::parse_packed,
    state::RequestState,
    tracker::{ItemView, Tracker},
    world::{Assumptions, SetAssumption, World},
    Dex,
};
use rand::{rngs::SmallRng, seq::SliceRandom, RngExt, SeedableRng};
use rl4burn::{clip_grad_norm, Env, MaskedActorCritic};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

const N: usize = 8;
const WIDTH: usize = STATE + N * ACTION_FEATURES;

/// Average regret-matching strategies for a zero-sum rectangular matrix.
pub fn solve(m: &[Vec<f64>], iterations: usize) -> (Vec<f64>, Vec<f64>, f64, f64) {
    let (rows, cols) = (m.len(), m[0].len());
    let mut rp = vec![0.; rows];
    let mut rq = vec![0.; cols];
    let mut ap = vec![0.; rows];
    let mut aq = vec![0.; cols];
    let strategy = |r: &[f64]| {
        let sum: f64 = r.iter().map(|x| x.max(0.)).sum();
        r.iter()
            .map(|x| {
                if sum > 0. {
                    x.max(0.) / sum
                } else {
                    1. / r.len() as f64
                }
            })
            .collect::<Vec<_>>()
    };
    for _ in 0..iterations.max(1) {
        let p = strategy(&rp);
        let q = strategy(&rq);
        let u: Vec<f64> = m
            .iter()
            .map(|row| row.iter().zip(&q).map(|(x, q)| x * q).sum())
            .collect();
        let w: Vec<f64> = (0..cols)
            .map(|j| (0..rows).map(|i| p[i] * m[i][j]).sum())
            .collect();
        let v: f64 = p.iter().zip(&u).map(|(p, u)| p * u).sum();
        for i in 0..rows {
            rp[i] += u[i] - v;
            ap[i] += p[i];
        }
        for j in 0..cols {
            rq[j] += v - w[j];
            aq[j] += q[j];
        }
    }
    for x in &mut ap {
        *x /= iterations.max(1) as f64;
    }
    for x in &mut aq {
        *x /= iterations.max(1) as f64;
    }
    let v = (0..rows)
        .map(|i| (0..cols).map(|j| ap[i] * m[i][j] * aq[j]).sum::<f64>())
        .sum();
    let upper = m
        .iter()
        .map(|row| row.iter().zip(&aq).map(|(x, q)| x * q).sum::<f64>())
        .fold(f64::NEG_INFINITY, f64::max);
    let lower = (0..cols)
        .map(|j| (0..rows).map(|i| ap[i] * m[i][j]).sum::<f64>())
        .fold(f64::INFINITY, f64::min);
    (ap, aq, v, upper - lower)
}

fn draw(p: &[f64], rng: &mut SmallRng) -> usize {
    let mut r = rng.random::<f64>();
    for (i, p) in p.iter().enumerate() {
        r -= p;
        if r <= 0. {
            return i;
        }
    }
    p.len() - 1
}

fn scores(
    model: &Model<Inner>,
    b: &ninjask::Battle,
    t: &Tracker,
    cs: &[Choice],
    device: &<Inner as Backend>::Device,
) -> Vec<f32> {
    let obs = env::observe_padded(b, t, cs, cs.len());
    let (logits, _) = model.forward(Tensor::from_data(
        TensorData::new(obs, [1, STATE + cs.len() * ACTION_FEATURES]),
        device,
    ));
    logits.into_data().to_vec::<f32>().unwrap()
}

fn shortlist(scores: &[f32], eligible: &[usize], rng: &mut SmallRng) -> Vec<usize> {
    let mut order = eligible.to_vec();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    let split = order.len().min(N - 2);
    let mut rest = order.split_off(split);
    rest.shuffle(rng);
    order.extend(rest.into_iter().take(N - order.len()));
    order
}

pub(crate) type Catalog = HashMap<u16, Vec<SetAssumption>>;

#[derive(Clone, Debug, Default)]
struct MoveHistory {
    actions: u16,
    stall: u16,
    duration: u8,
}

fn public_history<'a>(lines: impl Iterator<Item = &'a str>) -> HashMap<String, MoveHistory> {
    let mut history = HashMap::<String, MoveHistory>::new();
    let mut slots = HashMap::<String, String>::new();
    let mut pending: Option<String> = None;
    let key = |who: &str| {
        who.split_once(": ")
            .filter(|_| who.starts_with("p1") || who.starts_with("p2"))
            .map(|(_, name)| format!("{}:{name}", &who[..2]))
    };
    let protects = |s: &str| {
        matches!(
            s.trim_start_matches("move: "),
            "Protect"
                | "Detect"
                | "Spiky Shield"
                | "King's Shield"
                | "Baneful Bunker"
                | "Obstruct"
                | "Silk Trap"
                | "Burning Bulwark"
                | "Endure"
                | "Wide Guard"
                | "Quick Guard"
        )
    };
    for line in lines {
        let p: Vec<_> = line.split('|').collect();
        let event = p.get(1).copied().unwrap_or("");
        let who = p.get(2).copied().unwrap_or("");
        let value = p.get(3).copied().unwrap_or("");
        match event {
            "switch" | "drag" => {
                if let Some(k) = key(who) {
                    history.insert(k.clone(), MoveHistory::default());
                    slots.insert(who[..3].to_string(), k);
                }
            }
            "replace" => {
                if let Some(k) = key(who) {
                    if let Some(old) = slots.insert(who[..3].to_string(), k.clone()) {
                        let h = history.remove(&old).unwrap_or_default();
                        history.insert(k, h);
                    }
                }
            }
            "move" | "cant" => {
                pending = None;
                if let Some(k) = key(who) {
                    history.entry(k.clone()).or_default().actions += 1;
                    if event == "move" && protects(value) {
                        pending = Some(k);
                    }
                }
            }
            "-singleturn" if protects(value) => {
                if let Some(k) = key(who) {
                    let h = history.entry(k).or_default();
                    h.stall = if h.stall == 0 {
                        3
                    } else {
                        (h.stall * 3).min(729)
                    };
                    h.duration = 2;
                    pending = None;
                }
            }
            "-sidestart" if protects(value) => {
                if let Some(k) = pending.take() {
                    let h = history.entry(k).or_default();
                    h.stall = if h.stall == 0 {
                        3
                    } else {
                        (h.stall * 3).min(729)
                    };
                    h.duration = 2;
                }
            }
            "-fail" if value.is_empty() || value.starts_with("move:") => {
                if let Some(k) = key(who) {
                    if pending.as_ref() == Some(&k) {
                        let h = history.entry(k).or_default();
                        h.stall = 0;
                        h.duration = 0;
                        pending = None;
                    }
                }
            }
            "-activate" if line.contains("[broken]") || value == "move: Feint" => {
                if let Some(k) = key(who) {
                    let h = history.entry(k).or_default();
                    h.stall = 0;
                    h.duration = 0;
                }
            }
            "upkeep" => {
                for h in history.values_mut() {
                    h.duration = h.duration.saturating_sub(1);
                    if h.duration == 0 {
                        h.stall = 0;
                    }
                }
            }
            _ => {}
        }
    }
    history
}

fn restore_public_history(world: &mut ninjask::Battle, b: &ninjask::Battle, side: usize) {
    let history = public_history(b.log.iter().map(|line| line.render(Viewer::Side(side))));
    for (s, side) in world.sides.iter_mut().enumerate() {
        for m in &mut side.mons {
            if let Some(h) = history.get(&format!("p{}:{}", s + 1, m.name)) {
                m.active_move_actions = h.actions;
                m.vol.stall_counter = h.stall;
                m.vol.stall_duration = h.duration;
            }
        }
    }
}
fn catalog(args: &Args, dex: &Dex) -> Result<Catalog> {
    catalog_path(&args.teams, dex)
}
pub(crate) fn catalog_path(path: &std::path::Path, dex: &Dex) -> Result<Catalog> {
    let mut out = Catalog::new();
    for text in std::fs::read_to_string(path)?.lines() {
        if !ninjask::dex::support::team_problems(dex, text).is_ok_and(|p| p.is_empty()) {
            continue;
        }
        if let Ok(sets) = parse_packed(dex, text) {
            for s in sets {
                out.entry(s.species).or_default().push(SetAssumption {
                    item: Some(s.item),
                    ability: Some(s.ability),
                    moves: s.moves,
                    nature: Some(s.nature),
                    points: Some(s.points),
                });
            }
        }
    }
    Ok(out)
}

fn assumed_world(view: &Tracker, bank: &Catalog, rng: &mut SmallRng) -> Result<ninjask::Battle> {
    let foe = 1 - view.me;
    let mut asm = Assumptions {
        seed: rng.random(),
        ..Default::default()
    };
    for mon in &view.sides[foe].mons {
        let set = mon
            .species
            .and_then(|sp| bank.get(&sp))
            .filter(|v| !v.is_empty())
            .map(|v| v[rng.random_range(0..v.len())].clone());
        asm.foe.push(set);
    }
    World::from_view(view, &asm).map_err(anyhow::Error::msg)
}

/// Conservative opponent perspective: no access to our private moves/items/stats.
fn opponent_view(view: &Tracker, world: &ninjask::Battle) -> Tracker {
    let mut t = view.clone();
    t.me = 1 - view.me;
    for m in &mut t.sides[view.me].mons {
        m.moves.clear();
        m.ability = None;
        m.item = ItemView::Unknown;
        m.stats = None;
        m.team_pos = None;
        m.hp = (100u32 * m.hp as u32 / m.maxhp.max(1) as u32) as u16;
        m.maxhp = 100;
    }
    t.feed_engine(world);
    t
}

struct Example {
    obs: Vec<f32>,
    policy: Vec<f32>,
    value: f32,
    mask: Vec<f32>,
}
#[derive(Clone)]
struct ValueExample {
    state: Vec<f32>,
    target: f32,
}

// Prefix is the cumulative reward BEFORE the action: subtraction therefore
// includes this action and all later rewards, including forced replacements.
fn complete_value_game(
    pending: &mut Vec<(ValueExample, f64)>,
    total_return: f64,
    ended: bool,
    return_weight: f32,
    replay: &mut VecDeque<ValueExample>,
    capacity: usize,
) -> Vec<ValueExample> {
    let mut fresh = Vec::new();
    for (mut example, prefix) in pending.drain(..) {
        if !ended { continue; } // Artificial cutoffs are not completed outcomes.
        let actual = (total_return - prefix) as f32;
        example.target = (1. - return_weight) * example.target + return_weight * actual;
        fresh.push(example.clone());
        replay.push_back(example);
        while replay.len() > capacity { replay.pop_front(); }
    }
    fresh
}

pub(crate) struct Search {
    example: Example,
    pub(crate) chosen: usize,
    gap: f64,
    pub(crate) cells: usize,
    pub(crate) evidence: serde_json::Value,
}

fn search(
    model: &Model<Inner>,
    e: &BattleEnv<Inner>,
    bank: &Catalog,
    args: &Args,
    device: &<Inner as Backend>::Device,
    rng: &mut SmallRng,
) -> Result<Option<Search>> {
    search_position(
        model,
        &e.battle,
        e.learner_view(),
        e.legal_choices(),
        bank,
        args,
        device,
        rng,
    )
}

pub(crate) fn search_position(
    model: &Model<Inner>,
    b: &ninjask::Battle,
    t: &Tracker,
    cs: &[Choice],
    bank: &Catalog,
    args: &Args,
    device: &<Inner as Backend>::Device,
    rng: &mut SmallRng,
) -> Result<Option<Search>> {
    search_position_mode(model, b, t, cs, bank, args, device, rng, false)
}

// Bounds are on unmodified Speed. Keep battle modifiers in the engine.
fn speed_bound_worlds(world: &ninjask::Battle, view: &Tracker) -> Vec<ninjask::Battle> {
    let foe = 1 - view.me;
    let mut variant = world.clone();
    let trick_room = world.field.trick_room > 0;
    let mut matched = HashSet::new();
    for (position, mon) in world.sides[foe].mons.iter().enumerate() {
        if mon.fainted { continue; }
        let found = view.sides[foe].mons.iter().enumerate().find(|(k, m)|
            !matched.contains(k) && m.name.as_str() == mon.name.as_ref());
        let Some((k, _)) = found else { continue; };
        matched.insert(k);
        let Some(bounds) = view.speed_bounds(foe, k) else { continue; };
        let speed = if trick_room { bounds.min } else { bounds.max };
        variant.sides[foe].mons[position].stats[5] = speed;
        variant.sides[foe].mons[position].speed = speed as i32;
        // Engine recomputes modified action speeds when choices resolve.
    }
    // One adverse-order assumption: fast foes normally, slow foes in Trick Room.
    // Own speeds remain known and unchanged. This is not an exhaustive value minimum.
    vec![variant]
}

fn expected_leaf_value(probabilities: &[f64], values: &[f32]) -> f64 {
    assert_eq!(probabilities.len(), values.len());
    probabilities.iter().zip(values).map(|(p, v)| p * v.clamp(-2., 2.) as f64).sum()
}

// Nature chooses a separate worst speed scenario for each action pair.
fn worst_speed_cells(totals: &[Vec<Vec<f64>>], immediate: &[Vec<Vec<f64>>])
    -> (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<Vec<usize>>) {
    let mut payoff = totals[0].clone();
    let mut rewards = immediate[0].clone();
    let mut chosen = vec![vec![0; payoff[0].len()]; payoff.len()];
    for i in 0..payoff.len() {
        for j in 0..payoff[i].len() {
            for k in 1..totals.len() {
                if totals[k][i][j] < payoff[i][j] {
                    payoff[i][j] = totals[k][i][j];
                    rewards[i][j] = immediate[k][i][j];
                    chosen[i][j] = k;
                }
            }
        }
    }
    (payoff, rewards, chosen)
}

mod chance_benchmark;

pub(crate) fn search_position_mode(
    model: &Model<Inner>, b: &ninjask::Battle, t: &Tracker, cs: &[Choice], bank: &Catalog,
    args: &Args, device: &<Inner as Backend>::Device, rng: &mut SmallRng, hp_only: bool,
) -> Result<Option<Search>> {
    if hp_only && args.eval_hp_sampled {
        ensure!(args.matrix_samples > 0, "Sampled HP search needs positive --matrix-samples");
        return chance_benchmark::sampled(model, b, t, cs, bank, args, device, rng, true);
    }
    if args.benchmark_chance_turn == 0 {
        return search_position_enumerated(model, b, t, cs, bank, args, device, rng, hp_only);
    }
    if b.turn != args.benchmark_chance_turn || hp_only {
        return chance_benchmark::sampled(model, b, t, cs, bank, args, device, rng, hp_only);
    }
    // Freeze the RNG state before world assumptions and shortlisting for both methods.
    let frozen = rng.clone();
    let mut warm_rng = frozen.clone();
    chance_benchmark::sampled(model, b, t, cs, bank, args, device, &mut warm_rng, false)?;
    let mut sampled_times = Vec::new();
    let mut sampled = None;
    for _ in 0..3 {
        let mut repeat_rng = frozen.clone();
        Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        let start = Instant::now();
        sampled = chance_benchmark::sampled(model, b, t, cs, bank, args, device, &mut repeat_rng, false)?;
        Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        sampled_times.push(start.elapsed().as_secs_f64());
    }
    let sampled = sampled.context("No sampled search at benchmark turn")?;
    std::fs::write(args.out.join("benchmark-progress.json"), serde_json::to_vec_pretty(&json!({
        "status":"enumerating","turn":b.turn,"sampled_seconds":sampled_times,
        "encoded_state":env::encode_state(t),"sampled":sampled.evidence}))?)?;
    eprintln!("Benchmark turn {}: sampled timings {:?}; beginning PreciceButSame", b.turn, sampled_times);
    let mut enum_rng = frozen.clone();
    Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
    let start = Instant::now();
    let enumerated = search_position_enumerated(model, b, t, cs, bank, args, device, &mut enum_rng, false)?
        .context("No enumerated search at benchmark turn")?;
    Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
    let enum_seconds = start.elapsed().as_secs_f64() - enumerated.evidence["benchmark_log_seconds"].as_f64().unwrap_or(0.);
    for key in ["state", "candidates", "opponent_candidates", "speed_bound_scenarios"] {
        ensure!(sampled.evidence[key] == enumerated.evidence[key], "Benchmark inputs differ: {key}");
    }
    sampled_times.sort_by(f64::total_cmp);
    let result = json!({"status":"completed","turn":b.turn,"side":t.me,
        "same_inputs_verified":true,"encoded_state":env::encode_state(t),
        "sampling_seconds":sampled_times,"sampling_median_seconds":sampled_times[1],
        "almost_precise_seconds":enum_seconds,"slowdown":enum_seconds/sampled_times[1],
        "sampling_successors":sampled.cells,"enumerated_successors":enumerated.cells,
        "sampled":sampled.evidence,"enumerated":enumerated.evidence,
        "timing_scope":"complete learner search: reconstruction, candidates, all speed scenarios, simulation/enumeration, neural leaves, solve and diagnostic evidence; excludes replay to turn and disk writes"});
    std::fs::write(args.out.join("benchmark.json"), serde_json::to_vec_pretty(&result)?)?;
    println!("Turn benchmark complete: sampled {:.6}s, PreciceButSame {:.6}s, {:.2}x", sampled_times[1], enum_seconds, enum_seconds/sampled_times[1]);
    std::process::exit(0);
}

fn search_position_enumerated(
    model: &Model<Inner>,
    b: &ninjask::Battle,
    t: &Tracker,
    cs: &[Choice],
    bank: &Catalog,
    args: &Args,
    device: &<Inner as Backend>::Device,
    rng: &mut SmallRng,
    hp_only: bool,
) -> Result<Option<Search>> {
    if b.request != RequestState::Move {
        return Ok(None);
    }
    let side = t.me;
    let mut world = assumed_world(t, bank, rng)?;
    restore_public_history(&mut world, b, side);
    let legal_world: HashSet<_> = env::choices(&world, side)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let eligible: Vec<_> = cs
        .iter()
        .enumerate()
        .filter(|(_, c)| legal_world.contains(&format!("{c:?}")))
        .map(|(i, _)| i)
        .collect();
    if eligible.is_empty() {
        return Ok(None);
    }
    let own_ids = if hp_only {
        let mut ids = eligible.clone(); ids.shuffle(rng); ids.truncate(N); ids
    } else { shortlist(&scores(model, b, t, cs, device), &eligible, rng) };
    let own: Vec<_> = own_ids.iter().map(|&i| cs[i].clone()).collect();
    let opp_view = opponent_view(t, &world);
    let opp_all = env::choices(&world, 1 - side);
    if opp_all.is_empty() {
        return Ok(None);
    }
    let opp_ids = if hp_only {
        let mut ids: Vec<_> = (0..opp_all.len()).collect(); ids.shuffle(rng); ids.truncate(N); ids
    } else { shortlist(
        &scores(model, &world, &opp_view, &opp_all, device),
        &(0..opp_all.len()).collect::<Vec<_>>(),
        rng,
    )
    };
    let opp: Vec<_> = opp_ids.iter().map(|&i| opp_all[i].clone()).collect();
    let speed_worlds = speed_bound_worlds(&world, t);
    let mut scenario_payoffs = vec![vec![vec![0.; opp.len()]; own.len()]; speed_worlds.len()];
    let mut scenario_immediate = scenario_payoffs.clone();
    let before = env::hp_advantage(&world, side);
    let detailed = (args.search_trace_turn > 0 && b.turn == args.search_trace_turn)
        || (args.inspect_decision > 0 && b.turn == 16
            && b.sides[side].mons.iter().filter(|m| !m.fainted).count() == 1);
    let mut cell_traces = vec![];
    let mut enumerated_outcomes = 0usize;
    let mut benchmark_log_seconds = 0.;
    // Pack leaves across cells to amortize GPU dispatch/upload/readback. Keep
    // each original cell chunk's weights together so reduction order is unchanged.
    let mut leaf_states = Vec::new();
    let mut leaf_cells = Vec::new();
    let mut probability_mass = vec![vec![vec![0.; opp.len()]; own.len()]; speed_worlds.len()];
    for (scenario, speed_world) in speed_worlds.iter().enumerate() {
        let mut origin = speed_world.clone();
        origin.log_enabled = true;
        origin.log.clear();
        for (i, a) in own.iter().enumerate() {
            for (j, b) in opp.iter().enumerate() {
                let (Choice::Turn(ours), Choice::Turn(theirs)) = (a, b) else {
                    anyhow::bail!("Matrix enumeration requires turn choices");
                };
                let (p1, p2) = if side == 0 { (ours, theirs) } else { (theirs, ours) };
                if args.benchmark_chance_turn > 0 {
                    let log_start = Instant::now();
                    eprintln!("Enumerating speed scenario {}, row {}, column {}", scenario, i, j);
                    benchmark_log_seconds += log_start.elapsed().as_secs_f64();
                }
                let outcomes = origin.enumerate_outcomes_limited(p1, p2, ninjask::EnumerationMode::PreciceButSame, Some(args.matrix_max_scenarios))
                    .context("Matrix outcome enumeration failed")?;
                let mass: f64 = outcomes.iter().map(|o| o.probability).sum();
                ensure!(outcomes.iter().all(|o| o.probability.is_finite() && o.probability >= 0.)
                    && mass > 0. && mass <= 1. + 1e-6, "Invalid enumerated probability mass: {mass}");
                if mass < 1. - 1e-9 {
                    eprintln!("chance_search_cutoff turn={} side={} speed_scenario={} row={} column={} max_scenarios={} total_probability={:.17}", origin.turn, side, scenario, i, j, args.matrix_max_scenarios, mass);
                }
                probability_mass[scenario][i][j] = mass;
                enumerated_outcomes += outcomes.len();
                // Preserve the original reduction groups even for caps above 64.
                for chunk in outcomes.chunks(64) {
                    let mut states = Vec::new();
                    let mut weights = Vec::new();
                    for outcome in chunk {
                        let w = &outcome.battle;
                        let probability = outcome.probability / mass;
                        let reward = if hp_only { 2. * env::hp_advantage(w, side) as f64 }
                            else { env::battle_reward(w, side, w.ended, before) as f64 };
                        scenario_immediate[scenario][i][j] += probability * reward;
                        scenario_payoffs[scenario][i][j] += probability * reward;
                        if detailed {
                            cell_traces.push(json!({"probability":probability,"speed_scenario":scenario,
                                "row":i,"column":j,"reward":reward,"ended":w.ended,
                                "request":format!("{:?}",w.request),"events":w.log_lines(Viewer::Side(side),0)}));
                        }
                        if !hp_only && !w.ended && w.turn < args.max_turns {
                            let mut next = t.clone();
                            for line in w.log_lines(Viewer::Side(side), 0) { next.feed(&line); }
                            next.feed_engine(w);
                            states.extend(env::encode_state(&next));
                            weights.push(probability);
                        }
                    }
                    if !weights.is_empty() {
                        leaf_states.extend(states);
                        leaf_cells.push((scenario, i, j, weights));
                    }
                }
            }
        }
    }
    // Terminal/cutoff leaves and HP-only search need no neural value calls.
    // GPU batches stay bounded at 64; only encoded inputs are retained on CPU.
    let mut leaf_values = Vec::with_capacity(leaf_states.len() / STATE);
    let mut leaf_value_calls = 0usize;
    for chunk in leaf_states.chunks(64 * STATE) {
        let values = model.state_value(Tensor::from_data(
            TensorData::new(chunk.to_vec(), [chunk.len() / STATE, STATE]), device,
        )).into_data().to_vec::<f32>().unwrap();
        leaf_values.extend(values);
        leaf_value_calls += 1;
    }
    let mut cursor = 0;
    for (scenario, i, j, weights) in leaf_cells {
        let end = cursor + weights.len();
        scenario_payoffs[scenario][i][j] +=
            expected_leaf_value(&weights, &leaf_values[cursor..end]);
        cursor = end;
    }
    debug_assert_eq!(cursor, leaf_values.len());
    let (mut payoff, immediate_payoff, worst_speed_scenario) =
        worst_speed_cells(&scenario_payoffs, &scenario_immediate);
    let bootstrap_payoff: Vec<Vec<f64>> = payoff
        .iter()
        .zip(&immediate_payoff)
        .map(|(row, immediate)| {
            row.iter()
                .zip(immediate)
                .map(|(total, reward)| total - reward)
                .collect()
        })
        .collect();
    for row in &mut payoff {
        for x in row {
            *x = x.clamp(-2., 2.);
        }
    }
    let (p, q, value, gap) = solve(&payoff, args.regret_iterations);
    let selected = draw(&p, rng);
    let mut policy = vec![0.; N];
    let mut mask = vec![-1e9; N];
    for i in 0..own.len() {
        policy[i] = p[i] as f32;
        mask[i] = 0.;
    }
    let mut evidence = json!({"turn":b.turn,"side":side,"state":crate::diagnosis::state(t),
        "candidates":own.iter().map(|a|crate::diagnosis::choice(b,t,a,cs)).collect::<Vec<_>>(),
        "payoff":payoff,"policy":p,"opponent_policy":q,"value":value,"duality_gap":gap,
        "immediate_payoff":immediate_payoff,"bootstrap_payoff":bootstrap_payoff,
        "opponent_candidates":opp.iter().map(|a|crate::diagnosis::choice(&world,&opp_view,a,&opp_all)).collect::<Vec<_>>(),
        "cell_traces":cell_traces,
        "speed_bound_scenarios":speed_worlds.iter().map(|w| w.sides[1-side].mons.iter()
            .map(|m| json!({"name":m.name.as_ref(),"speed_stat":m.stats[5]})).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "worst_speed_scenario":worst_speed_scenario,
        "speed_aggregation":"single adverse-speed scenario: opponent lower bounds in Trick Room, upper bounds otherwise",
        "chance_method":"enumerate_outcomes_precice_but_same","outcome_probability_mass":probability_mass,"max_scenarios":args.matrix_max_scenarios,"probability_normalization":"raw probability / explored mass",
        "enumerated_successor_states":enumerated_outcomes,"benchmark_log_seconds":benchmark_log_seconds,
        "leaf_value_states":leaf_values.len(),"leaf_value_calls":leaf_value_calls,"leaf_batch_size":64,
        "chosen_candidate":selected,"world_source":"Tracker plus catalog assumptions; no actual private opponent set"});
    if detailed {
        let summary = |battle: &ninjask::Battle| battle.sides.iter().map(|s| s.mons.iter().map(|m|
            json!({"name":m.name.as_ref(),"hp":m.hp,"stats":m.stats,"speed":m.speed,
                "active":m.is_active,"fainted":m.fainted,"disable":m.vol.disable,
                "stall_counter":m.vol.stall_counter})).collect::<Vec<_>>()).collect::<Vec<_>>();
        // Diagnostic output only: actual private stats never enter search/inference.
        evidence["diagnostic_reconstructed"] = json!(summary(&world));
        evidence["diagnostic_actual"] = json!(summary(b));
    }

    Ok(Some(Search {
        example: Example {
            obs: env::observe_padded(b, t, &own, N),
            policy,
            value: value as f32,
            mask,
        },
        chosen: own_ids[selected],
        gap,
        cells: enumerated_outcomes,
        evidence,
    }))
}

pub fn diagnose(
    args: &Args,
    dex: Arc<Dex>,
    teams: Arc<Vec<Team>>,
    model: Model<Inner>,
    device: &<Inner as Backend>::Device,
) -> Result<()> {
    let bank = catalog(args, &dex)?;
    let league = Arc::new(Mutex::new(vec![model.clone()]));
    let mut e = BattleEnv::new(
        dex,
        teams,
        league,
        device.clone(),
        args.seed,
        args.max_turns,
    );
    let mut own_rng = SmallRng::seed_from_u64(args.seed ^ 0xA137);
    let mut foe_rng = SmallRng::seed_from_u64(args.seed ^ 0xB237);
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(args.out.join("diagnostic-games.jsonl"))?;
    let start = Instant::now();
    for game in 0..args.diagnose_games {
        e.reset();
        let mut trace = vec![];
        loop {
            let side = e.learner_side();
            let from = e.battle.log.len();
            let result = search(&model, &e, &bank, args, device, &mut own_rng)?;
            let chosen = if let Some(s) = &result {
                s.chosen
            } else {
                env::sample(
                    &model,
                    &e.battle,
                    e.learner_view(),
                    e.legal_choices(),
                    device,
                    &mut own_rng,
                )
            };
            let selected = crate::diagnosis::choice(
                &e.battle,
                e.learner_view(),
                &e.legal_choices()[chosen],
                e.legal_choices(),
            );
            let history = public_history(
                e.battle
                    .log
                    .iter()
                    .map(|line| line.render(Viewer::Side(side))),
            );
            let counters: Vec<_> = e.learner_view().sides[side]
                .mons
                .iter()
                .filter(|m| m.slot.is_some())
                .map(|m| {
                    let h = history
                        .get(&format!("p{}:{}", side + 1, m.name))
                        .cloned()
                        .unwrap_or_default();
                    let real = &e.battle.sides[side].mons[m.slot.unwrap()];
                    json!({"name":m.name,"slot":m.slot,"stall":h.stall,"duration":h.duration,
                    "actual_stall":real.vol.stall_counter,"actions_since_entry":h.actions,
                    "stall_roll_success_probability":if h.stall==0 {1.}else{1./h.stall as f64}})
                })
                .collect();
            let mut d = json!({"decision":trace.len(),"turn":e.battle.turn,"request":format!("{:?}",e.battle.request),
                "state":crate::diagnosis::state(e.learner_view()),"selected":selected,"protect_history":counters});
            if args.diagnose_values {
                let v = model.state_value(Tensor::from_data(
                    TensorData::new(env::encode_state(e.learner_view()), [1, STATE]),
                    device,
                ));
                d["network_value"] = json!(v.into_data().to_vec::<f32>().unwrap()[0]);
                // Evaluation labels only; the network input above is Tracker-only.
                d["hp_potential"] = json!(env::hp_advantage(&e.battle, side));
            }
            if let Some(s) = &result {
                d["search"] = s.evidence.clone();
            }
            let foe = 1 - side;
            if e.battle.side_needs_choice(foe) {
                let cs = env::choices(&e.battle, foe);
                let s = search_position(
                    &model,
                    &e.battle,
                    e.side_view(foe),
                    &cs,
                    &bank,
                    args,
                    device,
                    &mut foe_rng,
                )?;
                let i = if let Some(s) = s {
                    s.chosen
                } else {
                    env::sample(
                        &model,
                        &e.battle,
                        e.side_view(foe),
                        &cs,
                        device,
                        &mut foe_rng,
                    )
                };
                e.prepare_opponent(cs[i].clone());
            }
            let step = e.step(vec![chosen as f32]);
            d["events"] = json!(e.battle.log_lines(Viewer::Side(side), from));
            trace.push(d);
            if step.terminated || step.truncated {
                break;
            }
        }
        let side = e.learner_side();
        let outcome = if e.battle.ended {
            e.battle.winner.map_or(2, |s| if s == side { 0 } else { 1 })
        } else {
            2
        };
        writeln!(
            output,
            "{}",
            json!({"game":game,"learner_side":side,"outcome":outcome,
                "terminal_hp_potential":env::hp_advantage(&e.battle,side),"decisions":trace})
        )?;
        output.flush()?;
    }
    std::fs::write(
        args.out.join("diagnostic-provenance.json"),
        serde_json::to_vec_pretty(&json!({
        "games":args.diagnose_games,"weights":args.weights,"seed":args.seed,"matrix_samples":null,"chance_method":"enumerate_outcomes_precice_but_same",
        "regret_iterations":args.regret_iterations,"both_sides_search":true,"weights_updated":false,
        "seconds":start.elapsed().as_secs_f64(),"method":"New sampled fixed-checkpoint diagnostic games, not original training records."}))?,
    )?;
    println!(
        "Diagnostic games complete: {} in {:.3}s; no weight updates",
        args.diagnose_games,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

pub fn train(
    args: &Args,
    dex: Arc<Dex>,
    teams: Arc<Vec<Team>>,
    mut model: Model<Train>,
    device: &<Train as Backend>::Device,
) -> Result<()> {
    ensure!(
        args.regret_iterations > 0,
        "Matrix samples/iterations must be positive"
    );
    let bank = catalog(args, &dex)?;
    let recorder = BinFileRecorder::<FullPrecisionSettings>::new();
    let mut metrics = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(args.out.join("metrics.jsonl"))
        .context("Choose a new Model2 output directory")?;
    std::fs::write(
        args.out.join("config.json"),
        serde_json::to_vec_pretty(args)?,
    )?;
    for name in ["initial", "hour-000"] {
        model
            .clone()
            .save_file(args.out.join(name), &recorder)
            .map_err(|e| anyhow::anyhow!("save: {e:?}"))?;
    }
    std::fs::write(
        args.out.join("hour-000.json"),
        serde_json::to_vec(&json!({"hour":0,"update":0,"seconds":0.,"games":0,"decisions":0}))?,
    )?;
    let mut frozen = vec![];
    for path in &args.league_weights {
        let m = Model::<Inner>::new(device)
            .load_file(path, &recorder, device)
            .map_err(|e| anyhow::anyhow!("restore league: {e:?}"))?;
        ensure!(
            m.action_input_width() == ACTION_DESCRIPTOR,
            "Incompatible frozen league checkpoint"
        );
        frozen.push(m);
    }
    if frozen.is_empty() {
        frozen.push(model.valid());
    }
    if frozen.len() > args.league_size {
        frozen.drain(..frozen.len() - args.league_size);
    }
    let league = Arc::new(Mutex::new(frozen));
    let audit = Arc::new(Mutex::new(online_audit::TrainingAudit::new(
        args.audit_window,
        args.audit_every,
    )));
    let mut envs: Vec<_> = (0..args.envs)
        .map(|i| {
            let mut e = BattleEnv::new(
                dex.clone(),
                teams.clone(),
                league.clone(),
                device.clone(),
                args.seed + i as u64 + 1,
                args.max_turns,
            );
            e.audit = Some(audit.clone());
            e.audit_every = args.audit_every;
            e.reset();
            e
        })
        .collect();
    let mut optim = AdamConfig::new().with_epsilon(1e-5).init();
    let mut rng = SmallRng::seed_from_u64(args.seed);
    let mut opponent_rng = SmallRng::seed_from_u64(args.seed ^ 0x5E_A2_C4);
    let mut witness = std::fs::File::create(args.out.join("mistake-examples.jsonl"))?;
    let mut witnesses = VecDeque::<serde_json::Value>::new();
    let mut game_ids: Vec<usize> = (0..args.envs).collect();
    let mut next_game_id = args.envs;
    let mut snapshots = std::fs::File::create(args.out.join("league-snapshots.jsonl"))?;
    ensure!((0.0..=1.0).contains(&args.value_return_weight), "value return weight must be in [0,1]");
    ensure!(args.value_replay_capacity > 0, "value replay capacity must be positive");
    let mut value_replay = VecDeque::<ValueExample>::new();
    let mut pending_returns = vec![Vec::<(ValueExample, f64)>::new(); args.envs];
    let mut replay_rng = SmallRng::seed_from_u64(args.seed ^ 0xAC_721);
    let mut returns = vec![0.; args.envs];
    let mut value_audit = crate::value_audit::ValueAudit::new(args.audit_window);
    let mut pending_values = vec![Vec::<crate::value_audit::Prediction>::new(); args.envs];
    let mut value_move_counts = vec![0usize; args.envs];
    let mut value_samples = std::fs::File::create(args.out.join("value-outcome-samples.jsonl"))?;
    let (mut decisions, mut games, mut last_hour, mut last_snapshot, mut snapshot_count) =
        (0usize, 0usize, 0u64, 0., 0usize);
    let (mut collect_total, mut optimize_total) = (0., 0.);
    let mut previous_outcomes = [0usize; 3];
    let mut previous_turns = 0u64;
    let mut previous_rewards = 0.;
    let start = Instant::now();
    println!("Model2: 8x8 joint-action matrix, PreciceButSame probability-weighted enumeration, {} regret iterations; matrix opponents: {}; policy distillation, no PPO",args.regret_iterations,args.matrix_opponents);
    for update in 1..=args.updates {
        let timer = Instant::now();
        let teacher = model.valid();
        let mut batch = vec![];
        let (mut solved, mut cell_count, mut gap_sum, mut excluded) = (0usize, 0usize, 0., 0usize);
        let (mut opponent_requests, mut opponent_solved, mut opponent_cells) =
            (0usize, 0usize, 0usize);
        let mut completed_returns = vec![];
        let mut fresh_values = Vec::new();
        let mut value_training_samples = 0usize;
        for _ in 0..args.steps {
            for (index, e) in envs.iter_mut().enumerate() {
                let sample_value = e.battle.request == RequestState::Move
                    && args.audit_every > 0
                    && value_move_counts[index] % args.audit_every == 0;
                if e.battle.request == RequestState::Move {
                    value_move_counts[index] += 1;
                }
                if sample_value {
                    let audit_timer = Instant::now();
                    let value = teacher
                        .state_value(Tensor::from_data(
                            TensorData::new(env::encode_state(e.learner_view()), [1, STATE]),
                            device,
                        ))
                        .into_data()
                        .to_vec::<f32>()
                        .unwrap()[0];
                    pending_values[index].push(crate::value_audit::Prediction {
                        value,
                        hp_difference: 2. * env::hp_advantage(&e.battle, e.learner_side()),
                    });
                    value_audit.seconds += audit_timer.elapsed().as_secs_f64();
                }
                let result = search(&teacher, e, &bank, args, device, &mut rng)?;
                if args.inspect_decision > 0 && decisions + 1 == args.inspect_decision {
                    let record = json!({"game":game_ids[index],"env":index,"update":update,"decision":decisions+1,
                        "turn":e.battle.turn,"tracker_debug":format!("{:?}",e.learner_view()),
                        "encoded_state":env::encode_state(e.learner_view()),
                        "search":result.as_ref().map(|s| &s.evidence),
                        "replay_wall_seconds":start.elapsed().as_secs_f64(),
                        "training_games_seconds_before_current_update":collect_total,
                        "model_training_seconds":optimize_total,
                        "current_update_collection_seconds":timer.elapsed().as_secs_f64()});
                    std::fs::write(
                        args.out.join("inspected-decision.json"),
                        serde_json::to_vec_pretty(&record)?,
                    )?;
                    teacher
                        .save_file(args.out.join("inspected-model"), &recorder)
                        .map_err(|e| anyhow::anyhow!("save inspected model: {e:?}"))?;
                    println!("Inspected learner decision {} at game {}, turn {}; diagnostic replay stopped",decisions+1,game_ids[index],e.battle.turn);
                    return Ok(());
                }
                let chosen = if let Some(s) = &result {
                    s.chosen
                } else {
                    excluded += 1;
                    env::sample(
                        &teacher,
                        &e.battle,
                        e.learner_view(),
                        e.legal_choices(),
                        device,
                        &mut rng,
                    )
                };
                // Both searches read the unchanged pre-action state. Each side
                // uses its own Tracker, frozen/live weights and independent RNG.
                let foe = 1 - e.learner_side();
                if args.matrix_opponents && e.battle.side_needs_choice(foe) {
                    let cs = env::choices(&e.battle, foe);
                    if e.battle.request == RequestState::Move {
                        opponent_requests += 1;
                    }
                    let s = search_position(
                        e.frozen_opponent(),
                        &e.battle,
                        e.side_view(foe),
                        &cs,
                        &bank,
                        args,
                        device,
                        &mut opponent_rng,
                    )?;
                    let i = if let Some(s) = s {
                        opponent_solved += 1;
                        opponent_cells += s.cells;
                        s.chosen
                    } else {
                        env::sample(
                            e.frozen_opponent(),
                            &e.battle,
                            e.side_view(foe),
                            &cs,
                            device,
                            &mut opponent_rng,
                        )
                    };
                    e.prepare_opponent(cs[i].clone());
                }
                let from = e.battle.log.len();
                let mut sample = online_audit::Sample::prepare(
                    &e.battle,
                    e.learner_view(),
                    &e.legal_choices()[chosen],
                    e.legal_choices(),
                );
                let return_prefix = returns[index];
                let step = e.step(vec![chosen as f32]);
                decisions += 1;
                returns[index] += step.reward as f64;
                sample.finish(&e.battle, e.learner_side(), from);
                if let Some(s) = result {
                    if sample.counts.0[3] > 0 || sample.counts.0[7] > 0 || sample.counts.0[9] > 0 {
                        witnesses.push_back(json!({"update":update,"decision":decisions,
                            "game":game_ids[index],"env":index,"game_outcome":null,"search":s.evidence,
                            "counts":sample.counts.0,"events":e.battle.log_lines(Viewer::Side(e.learner_side()),from)}));
                        if witnesses.len() > 128 {
                            witnesses.pop_front();
                        }
                    }
                    solved += 1;
                    cell_count += s.cells;
                    gap_sum += s.gap;
                    pending_returns[index].push((ValueExample {
                        state: s.example.obs[..STATE].to_vec(),
                        target: s.example.value,
                    }, return_prefix));
                    batch.push(s.example);
                }
                if step.terminated || step.truncated {
                    fresh_values.extend(complete_value_game(
                        &mut pending_returns[index], returns[index], e.battle.ended,
                        args.value_return_weight, &mut value_replay, args.value_replay_capacity,
                    ));
                    let outcome = if e.battle.ended {
                        e.battle
                            .winner
                            .map_or(2, |s| if s == e.learner_side() { 0 } else { 1 })
                    } else {
                        2
                    };
                    let predictions = std::mem::take(&mut pending_values[index]);
                    writeln!(
                        value_samples,
                        "{}",
                        json!({"game":game_ids[index],"update":update,"outcome":outcome,
                        "predictions":predictions.iter().map(|p|json!({"value":p.value,"hp_difference":p.hp_difference})).collect::<Vec<_>>()})
                    )?;
                    value_audit.complete(predictions, outcome);
                    for w in &mut witnesses {
                        if w["game"].as_u64() == Some(game_ids[index] as u64) {
                            w["game_outcome"] = json!(outcome);
                        }
                    }
                    games += 1;
                    completed_returns.push(returns[index]);
                    returns[index] = 0.;
                    e.reset();
                    game_ids[index] = next_game_id;
                    next_game_id += 1;
                }
            }
        }
        Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        let collect = timer.elapsed().as_secs_f64();
        collect_total += collect;
        let timer = Instant::now();
        let (mut pl, mut vl, mut entropy, mut kl, mut updates) = (0., 0., 0., 0., 0usize);
        let mut order: Vec<_> = (0..batch.len()).collect();
        let anneal = if args.anneal_seconds > 0. {
            args.anneal_seconds
        } else {
            args.max_seconds as f64
        };
        let fraction = if anneal > 0. {
            (args.elapsed_offset_seconds + start.elapsed().as_secs_f64()) / anneal
        } else {
            0.
        };
        let lr = 3e-4 * (1. - fraction).max(0.05);
        for _ in 0..args.epochs {
            order.shuffle(&mut rng);
            for ids in order.chunks(args.batch) {
                if ids.is_empty() {
                    continue;
                }
                let size = ids.len();
                let input = Tensor::<Train, 2>::from_data(
                    TensorData::new(
                        ids.iter()
                            .flat_map(|&i| batch[i].obs.iter().copied())
                            .collect::<Vec<_>>(),
                        [size, WIDTH],
                    ),
                    device,
                );
                let target = Tensor::<Train, 2>::from_data(
                    TensorData::new(
                        ids.iter()
                            .flat_map(|&i| batch[i].policy.iter().copied())
                            .collect::<Vec<_>>(),
                        [size, N],
                    ),
                    device,
                );
                let mask = Tensor::<Train, 2>::from_data(
                    TensorData::new(
                        ids.iter()
                            .flat_map(|&i| batch[i].mask.iter().copied())
                            .collect::<Vec<_>>(),
                        [size, N],
                    ),
                    device,
                );
                let (logits, v) = model.forward(input);
                let lp = log_softmax(logits + mask, 1);
                let p = lp.clone().exp();
                let policy_loss = (target.clone() * lp.clone())
                    .sum_dim(1)
                    .mean()
                    .neg()
                    .reshape([1]);
                // Policy uses current search examples only. Value replay never
                // distills stale action distributions into the policy head.
                let value_loss = if value_replay.is_empty() {
                    v.mean().mul_scalar(0.).reshape([1])
                } else {
                    let samples: Vec<&ValueExample> = (0..size).map(|i| {
                        if i % 2 == 0 && !fresh_values.is_empty() {
                            &fresh_values[replay_rng.random_range(0..fresh_values.len())]
                        } else {
                            &value_replay[replay_rng.random_range(0..value_replay.len())]
                        }
                    }).collect();
                    value_training_samples += samples.len();
                    let states = Tensor::<Train, 2>::from_data(TensorData::new(
                        samples.iter().flat_map(|x| x.state.iter().copied()).collect::<Vec<_>>(),
                        [size, STATE]), device);
                    let targets = Tensor::<Train, 1>::from_data(TensorData::new(
                        samples.iter().map(|x| x.target).collect::<Vec<_>>(), [size]), device);
                    (model.state_value(states) - targets).powf_scalar(2.).mean()
                        .mul_scalar(0.5).reshape([1])
                };
                let ent = (p * lp.clone()).sum_dim(1).mean().neg().reshape([1]);
                let divergence = (target.clone() * (target.clamp_min(1e-12).log() - lp))
                    .sum_dim(1)
                    .mean();
                pl += policy_loss.clone().into_data().to_vec::<f32>().unwrap()[0] as f64;
                vl += value_loss.clone().into_data().to_vec::<f32>().unwrap()[0] as f64;
                entropy += ent.into_data().to_vec::<f32>().unwrap()[0] as f64;
                kl += divergence.into_data().to_vec::<f32>().unwrap()[0] as f64;
                updates += 1;
                let loss = policy_loss + value_loss;
                let grads = GradientsParams::from_grads(loss.backward(), &model);
                let grads = clip_grad_norm(&model, grads, 0.5);
                model = optim.step(lr, model, grads);
            }
        }
        Train::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        let optimize = timer.elapsed().as_secs_f64();
        optimize_total += optimize;
        let denom = updates.max(1) as f64;
        let (pl, vl, entropy, kl) = (pl / denom, vl / denom, entropy / denom, kl / denom);
        ensure!(
            [pl, vl, entropy, kl].iter().all(|x| x.is_finite()),
            "Nonfinite Model2 loss"
        );
        let mut outcomes = [0usize; 3];
        let (mut turns, mut reward, mut unfinished) = (0u64, 0., 0.);
        for e in &envs {
            for (a, b) in outcomes.iter_mut().zip(e.outcomes.lock().unwrap().iter()) {
                *a += b;
            }
            let s = e.turn_rewards.lock().unwrap();
            turns += s.turns;
            reward += s.reward;
            unfinished += s.unfinished_reward;
        }
        let completed: [usize; 3] = std::array::from_fn(|i| outcomes[i] - previous_outcomes[i]);
        previous_outcomes = outcomes;
        let nt = turns - previous_turns;
        let nr = reward - previous_rewards;
        previous_turns = turns;
        previous_rewards = reward;
        let log = json!({"algorithm":"model2_matrix_regret_distillation","update":update,"decisions":decisions,"games":games,"seconds":start.elapsed().as_secs_f64(),
            "value_return_weight":args.value_return_weight,"value_replay_size":value_replay.len(),
            "value_replay_capacity":args.value_replay_capacity,"value_fresh_positions":fresh_values.len(),
            "value_training_samples":value_training_samples,
            "policy_loss":pl,"value_loss":vl,"entropy":entropy,"kl":kl,"wins":completed[0],"losses":completed[1],"draws":completed[2],
            "mean_episode_return":if completed_returns.is_empty(){None}else{Some(completed_returns.iter().sum::<f64>()/completed_returns.len() as f64)},
            "training_games_seconds":collect_total,"model_training_seconds":optimize_total,"training_games_update_seconds":collect,"model_training_update_seconds":optimize,
            "online_audit":audit.lock().unwrap().snapshot(),"league_snapshots":snapshot_count,"league_size":league.lock().unwrap().len(),
            "value_outcome_audit":value_audit.snapshot(),
            "completed_battle_turns":nt,"completed_turn_reward_sum":nr,"mean_reward_per_turn":if nt>0{Some(nr/nt as f64)}else{None},
            "unfinished_turn_reward_excluded":unfinished,"matrix_positions":solved,"matrix_simulations":cell_count,
            "symmetric_matrix_opponents":args.matrix_opponents,"opponent_move_requests":opponent_requests,
            "opponent_matrix_positions":opponent_solved,"opponent_matrix_simulations":opponent_cells,
            "opponent_search_fallbacks":opponent_requests-opponent_solved,
            "mean_matrix_duality_gap":if solved>0{Some(gap_sum/solved as f64)}else{None},"unsearched_preview_or_replacement_decisions":excluded});
        println!("{log}");
        writeln!(metrics, "{log}")?;
        metrics.flush()?;
        let elapsed = start.elapsed().as_secs_f64();
        let done =
            args.max_seconds > 0 && elapsed >= args.max_seconds as f64 || update == args.updates;
        if crate::league_snapshot_due(
            update,
            elapsed,
            last_snapshot,
            args.snapshot_every,
            args.snapshot_seconds,
        ) || done
        {
            witness.set_len(0)?;
            witness.seek(SeekFrom::Start(0))?;
            for w in &witnesses {
                writeln!(witness, "{w}")?;
            }
            witness.flush()?;
            model
                .clone()
                .save_file(args.out.join(format!("checkpoint-{update:06}")), &recorder)
                .map_err(|e| anyhow::anyhow!("save: {e:?}"))?;
            let mut bank = league.lock().unwrap();
            bank.push(model.valid());
            if bank.len() > args.league_size {
                bank.remove(0);
            }
            snapshot_count += 1;
            last_snapshot = elapsed;
            writeln!(
                snapshots,
                "{}",
                json!({"update":update,"seconds":elapsed,"league_size":bank.len(),"snapshot":snapshot_count,"reason":if done{"final"}else{"interval"}})
            )?;
            snapshots.flush()?;
        }
        let hour = if args.hour_seconds > 0 {
            start.elapsed().as_secs() / args.hour_seconds
        } else {
            0
        };
        if hour > last_hour || done {
            let hour = hour.max(last_hour + 1);
            model
                .clone()
                .save_file(args.out.join(format!("hour-{hour:03}")), &recorder)
                .map_err(|e| anyhow::anyhow!("save: {e:?}"))?;
            std::fs::write(
                args.out.join(format!("hour-{hour:03}.json")),
                serde_json::to_vec(
                    &json!({"hour":hour,"update":update,"seconds":start.elapsed().as_secs_f64(),"games":games,"decisions":decisions}),
                )?,
            )?;
            last_hour = hour;
        }
        if done {
            break;
        }
    }
    let wall = start.elapsed().as_secs_f64();
    std::fs::write(
        args.out.join("timing.json"),
        serde_json::to_vec_pretty(&json!({"training_wall_seconds":wall,
        "training_games_seconds":collect_total,"model_training_seconds":optimize_total,"games":games,
        "measured_audit_seconds":audit.lock().unwrap().seconds,"method":"Collection includes Model2 matrix simulation and search; optimization is distribution/value distillation"}))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn almost_precise_repeat_protect_has_one_third_success_mass() {
        use ninjask::state::SlotChoice;
        let mut b = crate::tests::battle();
        env::apply(&mut b, 0, Choice::Preview(vec![0, 1, 2, 3]));
        env::apply(&mut b, 1, Choice::Preview(vec![0, 1, 2, 3]));
        let protect = b.dex.move_id("protect").unwrap();
        for side in &mut b.sides {
            for m in side.mons.iter_mut().take(2) {
                m.moves[0].id = protect;
                m.moves[0].pp = 16;
            }
        }
        // Isolate Protect probability without unrelated repeated speed-tie permutations.
        for (side_index, side) in b.sides.iter_mut().enumerate() {
            for (index, mon) in side.mons.iter_mut().enumerate() {
                mon.stats[5] = 240 - (side_index * 6 + index) as u16 * 10;
                mon.speed = mon.stats[5] as i32;
            }
        }
        b.sides[0].mons[0].vol.stall_counter = 3;
        b.sides[0].mons[0].vol.stall_duration = 1;
        b.log_enabled = true;
        b.log.clear();
        let actions = vec![SlotChoice::Move { slot: 0, target: 0, mega: false }; 2];
        let outcomes = b.enumerate_outcomes(&actions, &actions, ninjask::EnumerationMode::PreciceButSame).unwrap();
        let mass: f64 = outcomes.iter().map(|o| o.probability).sum();
        let success: f64 = outcomes.iter().filter(|o| o.battle.log_lines(Viewer::Side(0), 0)
            .iter().any(|e| e.starts_with("|-singleturn|p1a:") && e.ends_with("|Protect")))
            .map(|o| o.probability).sum();
        assert!((mass - 1.).abs() < 1e-9);
        assert!((success - 1./3.).abs() < 1e-9, "{success}: {:?}", outcomes.iter().map(|o| (o.probability, o.battle.log_lines(Viewer::Side(0), 0))).collect::<Vec<_>>());
    }

    #[test]
    fn enumerated_leaf_values_use_probability_not_branch_count() {
        let ev = expected_leaf_value(&[1./3., 2./3.], &[1., -1.]);
        assert!((ev + 1./3.).abs() < 1e-12);
        let split = expected_leaf_value(&[1./6., 1./6., 2./3.], &[1., 1., -1.]);
        assert!((ev - split).abs() < 1e-12);
        assert_eq!(expected_leaf_value(&[0.], &[2.]), 0.);
    }

    #[test]
    fn worst_speed_uses_total_return_and_matching_reward_decomposition() {
        let totals = vec![vec![vec![1., -0.8]], vec![vec![-1., 0.4]]];
        let immediate = vec![vec![vec![0.1, 0.2]], vec![vec![-0.3, 0.5]]];
        let (total, reward, selected) = worst_speed_cells(&totals, &immediate);
        assert_eq!(total, vec![vec![-1., -0.8]]);
        assert_eq!(reward, vec![vec![-0.3, 0.2]]);
        assert_eq!(selected, vec![vec![1, 0]]);
    }

    #[test]
    fn speed_worlds_use_adverse_bounds_for_both_sides_and_trick_room() {
        let mut battle = crate::tests::battle();
        env::apply(&mut battle, 0, Choice::Preview(vec![0, 1, 2, 3]));
        env::apply(&mut battle, 1, Choice::Preview(vec![0, 1, 2, 3]));
        for side in 0..2 {
            for trick_room in [false, true] {
                battle.field.trick_room = if trick_room { 5 } else { 0 };
                let mut view = Tracker::new(battle.dex.clone(), side);
                for line in battle.log_lines(Viewer::Side(side), 0) { view.feed(&line); }
                view.feed_engine(&battle);
                let worlds = speed_bound_worlds(&battle, &view);
                assert_eq!(worlds.len(), 1);
                let w = &worlds[0];
                for (actual, original) in w.sides[side].mons.iter().zip(&battle.sides[side].mons) {
                    assert_eq!(actual.stats, original.stats);
                    assert_eq!(actual.speed, original.speed);
                }
                let foe = 1-side;
                let mut checked = 0;
                for (k, m) in view.sides[foe].mons.iter().enumerate().filter(|(_, m)| m.slot.is_some()) {
                    let bound = view.speed_bounds(foe, k).unwrap();
                    let position = battle.sides[foe].mons.iter().position(|p| p.name.as_ref() == m.name).unwrap();
                    let expected = if trick_room { bound.min } else { bound.max };
                    assert_eq!(w.sides[foe].mons[position].stats[5], expected);
                    assert_eq!(w.sides[foe].mons[position].speed, expected as i32);
                    assert_eq!(w.sides[foe].mons[position].stats[..5], battle.sides[foe].mons[position].stats[..5]);
                    checked += 1;
                }
                assert!(checked >= 2);
            }
        }
    }

    #[test]
    fn documents_current_protect_history_observation_gap() {
        // Same HP, move-use totals, types, etc.; only ordering differs. Update
        // this probe when explicit consecutive-Protect features are introduced.
        let dex = crate::tests::battle().dex;
        let build = |sequence: [bool; 3]| {
            let mut log = vec![
                "|poke|p1|Pawmot, L50|".to_string(),
                "|poke|p2|Gastrodon, L50|".to_string(),
                "|switch|p1a: Pawmot|Pawmot, L50|100/100".to_string(),
                "|switch|p2a: Gastrodon|Gastrodon, L50|100/100".to_string(),
            ];
            for (i, protect) in sequence.iter().enumerate() {
                log.push(format!("|turn|{}", i + 1));
                if *protect {
                    log.push("|move|p1a: Pawmot|Protect|p1a: Pawmot".into());
                    log.push("|-singleturn|p1a: Pawmot|Protect".into());
                } else {
                    log.push("|move|p1a: Pawmot|Thunderbolt|p2a: Gastrodon".into());
                    log.push("|-immune|p2a: Gastrodon".into());
                }
                log.push("|upkeep".into());
            }
            log.push("|turn|4".into());
            let mut t = Tracker::new(dex.clone(), 0);
            for line in &log {
                t.feed(line);
            }
            (t, log)
        };
        let (a, la) = build([true, false, true]);
        let (b, lb) = build([true, true, false]);
        assert_eq!(
            public_history(la.iter().map(String::as_str))["p1:Pawmot"].stall,
            3
        );
        assert_eq!(
            public_history(lb.iter().map(String::as_str))["p1:Pawmot"].stall,
            0
        );
        assert_eq!(
            env::encode_state(&a),
            env::encode_state(&b),
            "The present neural encoding omits this history distinction"
        );
    }
    #[test]
    fn public_history_restores_repeat_protect_and_first_turn_counters() {
        use ninjask::state::SlotChoice;
        let mut b = crate::tests::battle();
        env::apply(&mut b, 0, Choice::Preview(vec![0, 1, 2, 3]));
        env::apply(&mut b, 1, Choice::Preview(vec![0, 1, 2, 3]));
        let protect = b.dex.move_id("protect").unwrap();
        for side in &mut b.sides {
            for m in side.mons.iter_mut().take(2) {
                m.moves[0].id = protect;
                m.moves[0].pp = 16;
            }
        }
        for _ in 0..6 {
            for side in 0..2 {
                env::apply(
                    &mut b,
                    side,
                    Choice::Turn(vec![
                        SlotChoice::Move {
                            slot: 0,
                            target: 0,
                            mega: false
                        };
                        2
                    ]),
                );
            }
            for viewer in 0..2 {
                let mut w = b.clone();
                for side in &mut w.sides {
                    for m in &mut side.mons {
                        m.active_move_actions = 0;
                        m.vol.stall_counter = 0;
                        m.vol.stall_duration = 0;
                    }
                }
                restore_public_history(&mut w, &b, viewer);
                for side in 0..2 {
                    for k in 0..2 {
                        let a = &b.sides[side].mons[k];
                        let z = &w.sides[side].mons[k];
                        assert_eq!(a.active_move_actions, z.active_move_actions);
                        assert_eq!(a.vol.stall_counter, z.vol.stall_counter);
                        assert_eq!(a.vol.stall_duration, z.vol.stall_duration);
                    }
                }
            }
        }
    }
    #[test]
    fn matching_pennies_has_half_half_solution() {
        let (p, q, v, gap) = solve(&[vec![1., -1.], vec![-1., 1.]], 1000);
        assert!(
            (p[0] - 0.5).abs() < 1e-8 && (q[0] - 0.5).abs() < 1e-8 && v.abs() < 1e-8 && gap < 1e-8
        );
    }
    #[test]
    fn dominated_action_loses_probability_and_value_matches_saddle() {
        let (p, q, v, gap) = solve(&[vec![1., 2.], vec![-1., 0.]], 4096);
        assert!(p[0] > 0.99 && q[0] > 0.99 && (v - 1.).abs() < 0.01 && gap < 0.01);
    }
}

#[cfg(test)]
mod return_replay_tests {
    use super::*;
    #[test]
    fn return_blend_includes_current_and_later_rewards_and_bounds_replay() {
        let example = |target| ValueExample { state: vec![1.], target };
        let mut pending = vec![(example(0.4), 0.0), (example(-0.2), 0.25)];
        let mut replay = VecDeque::new();
        let fresh = complete_value_game(&mut pending, 1.25, true, 0.5, &mut replay, 1);
        assert!((fresh[0].target - 0.825).abs() < 1e-6);
        assert!((fresh[1].target - 0.4).abs() < 1e-6);
        assert!(pending.is_empty());
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].target, fresh[1].target);
    }
    #[test]
    fn cutoffs_are_discarded_and_real_draws_retained() {
        let mut replay = VecDeque::new();
        let mut pending = vec![(ValueExample { state: vec![], target: 1. }, 0.)];
        assert!(complete_value_game(&mut pending, 0.2, false, 0.5, &mut replay, 8).is_empty());
        assert!(pending.is_empty() && replay.is_empty());
        pending.push((ValueExample { state: vec![], target: 1. }, 0.2));
        let fresh = complete_value_game(&mut pending, 0., true, 1., &mut replay, 8);
        assert!((fresh[0].target + 0.2).abs() < 1e-6);
    }
}
