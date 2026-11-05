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

pub(crate) fn search_position_mode(
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
    let mut payoff = vec![vec![0.; opp.len()]; own.len()];
    let mut leaf_states = vec![];
    let mut leaves = vec![];
    let before = env::hp_advantage(&world, side);
    let detailed = args.inspect_decision > 0
        && b.turn == 16
        && b.sides[side].mons.iter().filter(|m| !m.fainted).count() == 1;
    let mut cell_traces = vec![];
    for _ in 0..args.matrix_samples {
        let seed = rng.random::<u64>(); // Common random numbers across the matrix.
        for (i, a) in own.iter().enumerate() {
            for (j, b) in opp.iter().enumerate() {
                let mut w = world.clone();
                w.rng = ninjask::rng::Rng::new(seed);
                w.log_enabled = true;
                w.log.clear();
                env::apply(&mut w, side, a.clone());
                env::apply(&mut w, 1 - side, b.clone());
                if detailed {
                    cell_traces.push(json!({"sample_seed":seed,"row":i,"column":j,
                        "reward":env::battle_reward(&w,side,w.ended,before),
                        "before_hp_potential":before,"after_hp_potential":env::hp_advantage(&w,side),
                        "ended":w.ended,"request":format!("{:?}",w.request),
                        "events":w.log_lines(Viewer::Side(side),0)}));
                }
                payoff[i][j] += if hp_only { 2. * env::hp_advantage(&w, side) as f64 }
                    else { env::battle_reward(&w, side, w.ended, before) as f64 }
                    / args.matrix_samples as f64;
                if !hp_only && !w.ended && w.turn < args.max_turns {
                    let mut next = t.clone();
                    for line in w.log_lines(Viewer::Side(side), 0) {
                        next.feed(&line);
                    }
                    next.feed_engine(&w);
                    leaf_states.extend(env::encode_state(&next));
                    leaves.push((i, j));
                }
            }
        }
    }
    let immediate_payoff = payoff.clone();
    if !leaves.is_empty() {
        let values = model
            .state_value(Tensor::from_data(
                TensorData::new(leaf_states, [leaves.len(), STATE]),
                device,
            ))
            .into_data()
            .to_vec::<f32>()
            .unwrap();
        for ((i, j), v) in leaves.into_iter().zip(values) {
            payoff[i][j] += v.clamp(-2., 2.) as f64 / args.matrix_samples as f64;
        }
    }
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
    let evidence = json!({"turn":b.turn,"side":side,"state":crate::diagnosis::state(t),
        "candidates":own.iter().map(|a|crate::diagnosis::choice(b,t,a,cs)).collect::<Vec<_>>(),
        "payoff":payoff,"policy":p,"opponent_policy":q,"value":value,"duality_gap":gap,
        "immediate_payoff":immediate_payoff,"bootstrap_payoff":bootstrap_payoff,
        "opponent_candidates":opp.iter().map(|a|crate::diagnosis::choice(&world,&opp_view,a,&opp_all)).collect::<Vec<_>>(),
        "cell_traces":cell_traces,
        "chosen_candidate":selected,"world_source":"Tracker plus catalog assumptions; no actual private opponent set"});
    Ok(Some(Search {
        example: Example {
            obs: env::observe_padded(b, t, &own, N),
            policy,
            value: value as f32,
            mask,
        },
        chosen: own_ids[selected],
        gap,
        cells: own.len() * opp.len() * args.matrix_samples,
        evidence,
    }))
}

