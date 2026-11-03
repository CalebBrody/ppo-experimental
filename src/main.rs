mod env;
mod model;
#[cfg(test)]
mod tests;

use crate::{
    env::BattleEnv,
    model::{Model, ACTIONS},
};
use anyhow::{ensure, Context, Result};
use burn::{
    module::{AutodiffModule, Module},
    optim::AdamConfig,
    prelude::*,
    record::{BinFileRecorder, FullPrecisionSettings},
};
use clap::Parser;
use ninjask::{
    battle::Team,
    dex::{support::team_problems, team::parse_packed},
    Dex,
};
use rand::{rngs::SmallRng, SeedableRng};
use rl4burn::{
    masked_ppo_collect, masked_ppo_update, ActionDist, MaskedActorCritic, PpoConfig, SyncVecEnv,
};
use std::{cell::RefCell, io::Write, path::PathBuf, rc::Rc, sync::Arc, time::Instant};

#[cfg(feature = "gpu")]
type Inner = burn::backend::Wgpu;
#[cfg(all(feature = "cpu", not(feature = "gpu")))]
type Inner = burn::backend::NdArray;
type Train = burn::backend::Autodiff<Inner>;

#[derive(Parser, Debug, serde::Serialize)]
#[command(about = "Small transformer PPO league trainer for Ninjask VGC")]
struct Args {
    #[arg(
        long,
        default_value = "vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json"
    )]
    dex: PathBuf,
    #[arg(long, default_value = "data/teams.txt")]
    teams: PathBuf,
    #[arg(long, default_value = "runs/model1")]
    out: PathBuf,
    #[arg(long, default_value_t = 1000)]
    updates: usize,
    #[arg(long, default_value_t = 4)]
    envs: usize,
    #[arg(long, default_value_t = 64)]
    steps: usize,
    #[arg(long, default_value_t = 16)]
    batch: usize,
    #[arg(long, default_value_t = 4)]
    epochs: usize,
    #[arg(long, default_value_t = 20)]
    snapshot_every: usize,
    #[arg(long, default_value_t = 8)]
    league_size: usize,
    #[arg(long, default_value_t = 150)]
    max_turns: u32,
    #[arg(long, default_value_t = 37)]
    seed: u64,
    /// Load model weights; optimizer and opponent league start fresh.
    #[arg(long)]
    weights: Option<PathBuf>,
    /// Validate the imported teams against simulator support without training.
    #[arg(long)]
    validate: bool,
    /// Small GPU forward/backward/checkpoint test, not a meaningful training run.
    #[arg(long)]
    smoke: bool,
    /// Evaluate the checkpoint against random legal play on --teams.
    #[arg(long, default_value_t = 0)]
    evaluate: usize,
    /// Fixed checkpoint opponent for evaluation; otherwise random legal play.
    #[arg(long)]
    eval_opponent: Option<PathBuf>,
    /// Choose the learner's highest-scoring legal action during evaluation.
    #[arg(long)]
    eval_greedy: bool,
    /// Stop after this many training seconds, saving the completed update (0 = unlimited).
    #[arg(long, default_value_t = 0)]
    max_seconds: u64,
    /// Emit independently readable hourly model snapshots (0 disables).
    #[arg(long, default_value_t = 0)]
    hour_seconds: u64,
}

fn load_teams(args: &Args, dex: &Arc<Dex>) -> Result<Arc<Vec<Team>>> {
    let text = std::fs::read_to_string(&args.teams).with_context(|| {
        format!(
            "Read {:?}; run python scripts/import_teams.py first",
            args.teams
        )
    })?;
    let mut teams = vec![];
    let mut rejected = vec![];
    for (line, packed) in text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.trim().is_empty())
    {
        match team_problems(dex, packed) {
            Ok(problems) if problems.is_empty() => {
                let sets = parse_packed(dex, packed).map_err(anyhow::Error::msg)?;
                if sets.len() != 6 {
                    rejected
                        .push(serde_json::json!({"line":line+1,"reason":"requires six Pokemon"}));
                    continue;
                }
                teams.push(Team::new(dex, &sets));
            }
            result => {
                rejected.push(serde_json::json!({"line":line+1,"reason":format!("{result:?}")}))
            }
        }
    }
    std::fs::write(
        args.out.join("team-validation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"accepted":teams.len(),"rejected":rejected}),
        )?,
    )?;
    println!(
        "Teams: {} supported, {} rejected (report in {})",
        teams.len(),
        rejected.len(),
        args.out.display()
    );
    ensure!(teams.len() >= 2, "Need at least two supported teams");
    Ok(Arc::new(teams))
}

fn main() -> Result<()> {
    let mut args = Args::parse();
    if args.smoke {
        args.updates = 1;
        args.envs = 2;
        args.steps = 4;
        args.batch = 4;
        args.epochs = 1;
        args.max_turns = 2;
    }
    ensure!(
        args.envs > 0
            && args.steps > 0
            && args.batch > 1
            && args.updates > 0
            && args.epochs > 0
            && args.snapshot_every > 0
            && args.league_size > 0
            && args.max_turns > 0,
        "Counts must be positive; batch must exceed one"
    );
    std::fs::create_dir_all(&args.out)?;
    let dex =
        Arc::new(Dex::from_json(&std::fs::read_to_string(&args.dex)?).map_err(anyhow::Error::msg)?);
    let teams = load_teams(&args, &dex)?;
    if args.validate {
        return Ok(());
    }
    #[cfg(feature = "gpu")]
    let device = burn::backend::wgpu::WgpuDevice::DiscreteGpu(0);
    #[cfg(not(feature = "gpu"))]
    let device = <Inner as Backend>::Device::default();
    #[cfg(feature = "gpu")]
    {
        let setup = burn::backend::wgpu::init_setup::<burn::backend::wgpu::graphics::Vulkan>(
            &device,
            Default::default(),
        );
        println!(
            "GPU backend: Vulkan / WGPU; adapter {:?}",
            setup.adapter.get_info()
        );
    }
    #[cfg(not(feature = "gpu"))]
    println!("CPU backend selected");
    Train::seed(&device, args.seed);
    let recorder = BinFileRecorder::<FullPrecisionSettings>::new();
    let mut model = Model::<Train>::new(&device);
    if let Some(path) = &args.weights {
        model = model
            .load_file(path, &recorder, &device)
            .map_err(|e| anyhow::anyhow!("checkpoint: {e:?}"))?;
    }
    println!("Parameters: {}", model.num_params());
    if args.evaluate > 0 {
        return evaluate(&args, dex, teams, model.valid(), &device);
    }
    // Refuse to append an unrelated run to an existing metrics log.
    let metrics_path = args.out.join("metrics.jsonl");
    let mut metrics=std::fs::OpenOptions::new().write(true).create_new(true).open(&metrics_path)
        .context("Output already contains a run; choose a new --out directory (use --weights for a warm start)")?;
    std::fs::write(
        args.out.join("config.json"),
        serde_json::to_vec_pretty(&args)?,
    )?;
    model
        .clone()
        .save_file(args.out.join("initial"), &recorder)
        .map_err(|e| anyhow::anyhow!("save initial weights: {e:?}"))?;
    let league = Rc::new(RefCell::new(vec![model.valid()]));
    let envs = (0..args.envs)
        .map(|i| {
            BattleEnv::<Inner>::new(
                dex.clone(),
                teams.clone(),
                league.clone(),
                device.clone(),
                args.seed + i as u64 + 1,
                args.max_turns,
            )
        })
        .collect();
    let mut envs = SyncVecEnv::new(envs);
    let mut obs = envs.reset();
    let mut episode_returns = vec![0.; args.envs];
    let mut optim = AdamConfig::new().with_epsilon(1e-5).init();
    let config = PpoConfig {
        n_steps: args.steps,
        minibatch_size: args.batch,
        update_epochs: args.epochs,
        gamma: 1.,
        target_kl: Some(0.03),
        ..Default::default()
    };
    let mut rng = SmallRng::seed_from_u64(args.seed);
    let dist = ActionDist::Discrete(ACTIONS);
    let start = Instant::now();
    let mut games = 0;
    let mut last_hour = 0;
    if args.hour_seconds > 0 {
        model
            .clone()
            .save_file(args.out.join("hour-000"), &recorder)
            .map_err(|e| anyhow::anyhow!("save hour zero: {e:?}"))?;
        std::fs::write(
            args.out.join("hour-000.json"),
            serde_json::to_vec(
                &serde_json::json!({"hour":0,"update":0,"seconds":0.,"games":0,"decisions":0}),
            )?,
        )?;
    }
    for update in 1..=args.updates {
        let rollout = masked_ppo_collect(
            &model.valid(),
            &mut envs,
            &dist,
            &config,
            &device,
            &mut rng,
            &mut obs,
            &mut episode_returns,
        );
        games += rollout.episode_returns.len();
        let fraction = if args.hour_seconds > 0 && args.max_seconds > 0 {
            (start.elapsed().as_secs_f64() / args.max_seconds as f64).min(1.)
        } else {
            (update - 1) as f64 / args.updates as f64
        };
        let lr = config.lr * (1. - fraction).max(0.05);
        let (next, stats) = masked_ppo_update(
            model, &mut optim, &rollout, &dist, &config, lr, &device, &mut rng,
        );
        model = next;
        ensure!(
            [
                stats.policy_loss,
                stats.value_loss,
                stats.entropy,
                stats.approx_kl
            ]
            .iter()
            .all(|v| v.is_finite()),
            "Non-finite PPO update"
        );
        let log = serde_json::json!({"update":update,"decisions":update*args.envs*args.steps,"games":games,"seconds":start.elapsed().as_secs_f64(),"policy_loss":stats.policy_loss,"value_loss":stats.value_loss,"entropy":stats.entropy,"kl":stats.approx_kl,"wins":rollout.episode_returns.iter().filter(|&&r|r>0.).count(),"losses":rollout.episode_returns.iter().filter(|&&r|r<0.).count(),"draws":rollout.episode_returns.iter().filter(|&&r|r==0.).count()});
        println!("{log}");
        writeln!(metrics, "{log}")?;
        metrics.flush()?;
        if let Some(hour) = start.elapsed().as_secs().checked_div(args.hour_seconds) {
            if hour > last_hour {
                model
                    .clone()
                    .save_file(args.out.join(format!("hour-{hour:03}")), &recorder)
                    .map_err(|e| anyhow::anyhow!("save hourly model: {e:?}"))?;
                // The ready marker is written only after the model file closes.
                std::fs::write(
                    args.out.join(format!("hour-{hour:03}.json")),
                    serde_json::to_vec(
                        &serde_json::json!({"hour":hour,"update":update,"seconds":start.elapsed().as_secs_f64(),"games":games,"decisions":update*args.envs*args.steps}),
                    )?,
                )?;
                last_hour = hour;
            }
        }
        let time_limit = args.max_seconds > 0 && start.elapsed().as_secs() >= args.max_seconds;
        if update % args.snapshot_every == 0 || update == args.updates || time_limit {
            model
                .clone()
                .save_file(args.out.join(format!("checkpoint-{update:06}")), &recorder)
                .map_err(|e| anyhow::anyhow!("save checkpoint: {e:?}"))?;
            let mut bank = league.borrow_mut();
            bank.push(model.valid());
            if bank.len() > args.league_size {
                bank.remove(0);
            }
        }
        if time_limit {
            println!("Training time budget reached after update {update}");
            break;
        }
    }
    if args.smoke {
        use rl4burn::MaskedActorCritic;
        let path = args.out.join(format!("checkpoint-{:06}", args.updates));
        ensure!(
            std::fs::read(args.out.join("initial.bin"))?
                != std::fs::read(path.with_extension("bin"))?,
            "GPU update did not change model weights"
        );
        let restored = Model::<Train>::new(&device)
            .load_file(path, &recorder, &device)
            .map_err(|e| anyhow::anyhow!("reload checkpoint: {e:?}"))?;
        let input = Tensor::<Inner, 2>::from_data(
            TensorData::new(obs[0].clone(), [1, model::OBS]),
            &device,
        );
        let (a, _) = model.valid().forward(input.clone());
        let (b, _) = restored.valid().forward(input);
        let delta = (a - b).abs().max().into_data().to_vec::<f32>().unwrap()[0];
        ensure!(delta < 1e-5, "Checkpoint changed inference: {delta}");
        println!("Smoke verified: finite PPO losses, changed weights, checkpoint round-trip (max error {delta})");
    }
    println!("Finished. Checkpoints and metrics: {}", args.out.display());
    Ok(())
}

fn evaluate(
    args: &Args,
    dex: Arc<Dex>,
    teams: Arc<Vec<Team>>,
    model: Model<Inner>,
    device: &<Inner as Backend>::Device,
) -> Result<()> {
    use ninjask::{tracker::Tracker, Battle};
    use rand::RngExt;
    let opponent = if let Some(path) = &args.eval_opponent {
        Some(
            Model::<Inner>::new(device)
                .load_file(
                    path,
                    &BinFileRecorder::<FullPrecisionSettings>::new(),
                    device,
                )
                .map_err(|e| anyhow::anyhow!("opponent checkpoint: {e:?}"))?,
        )
    } else {
        None
    };
    let mut scores = [0usize; 3];
    let mut results = vec![];
    for game in 0..args.evaluate {
        // Independent per-match seed: earlier game lengths cannot change later
        // matchups. Adjacent games swap learner side on the same team pair.
        let match_seed = args.seed.wrapping_add((game / 2) as u64);
        let mut rng = SmallRng::seed_from_u64(match_seed);
        let a = rng.random_range(0..teams.len());
        let c = rng.random_range(0..teams.len());
        let mut b = Battle::from_teams(
            dex.clone(),
            [&teams[a], &teams[c]],
            ["p1", "p2"],
            rng.random(),
        );
        let mut ts = [Tracker::new(dex.clone(), 0), Tracker::new(dex.clone(), 1)];
        let mut cursor = 0;
        let side = game % 2;
        let mut actor_rng = SmallRng::seed_from_u64(match_seed ^ 0xA137);
        let mut opponent_rng = SmallRng::seed_from_u64(match_seed ^ 0xB237);
        let mut decisions = 0;
        while !b.ended && b.turn < args.max_turns && decisions < 512 {
            env::sync(&b, &mut ts, &mut cursor);
            let mut selected = vec![];
            for (s, t) in ts.iter().enumerate() {
                if b.side_needs_choice(s) {
                    let cs = env::choices(&b, s);
                    let i = if s == side {
                        if args.eval_greedy {
                            let obs = Tensor::from_data(
                                TensorData::new(env::observe(&b, t, &cs), [1, model::OBS]),
                                device,
                            );
                            let (logits, _) = model.forward(obs);
                            let scores = logits.into_data().to_vec::<f32>().unwrap();
                            (0..cs.len())
                                .max_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(b.cmp(&a)))
                                .unwrap()
                        } else {
                            env::sample(&model, &b, t, &cs, device, &mut actor_rng)
                        }
                    } else if let Some(opponent) = &opponent {
                        env::sample(opponent, &b, t, &cs, device, &mut opponent_rng)
                    } else {
                        opponent_rng.random_range(0..cs.len())
                    };
                    selected.push((s, cs[i].clone()));
                }
            }
            for (s, c) in selected {
                env::apply(&mut b, s, c);
            }
            decisions += 1;
        }
        let outcome = if b.ended {
            b.winner.map_or(2, |s| if s == side { 0 } else { 1 })
        } else {
            2
        };
        scores[outcome] += 1;
        results.push(serde_json::json!({"game":game,"match_seed":match_seed,"learner_side":side,"team_a":a,"team_b":c,"score":match outcome {0=>1.,1=>0.,_=>0.5},"turns":b.turn}));
    }
    let opponent_name = args
        .eval_opponent
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "random legal".into());
    let report = serde_json::json!({"opponent":opponent_name,"greedy":args.eval_greedy,"games":args.evaluate,"wins":scores[0],"losses":scores[1],"draws":scores[2],"seed":args.seed,"teams":args.teams,"weights":args.weights,"results":results});
    println!(
        "Evaluation: {} games, {} wins, {} losses, {} draws against {}",
        args.evaluate, scores[0], scores[1], scores[2], opponent_name
    );
    std::fs::write(
        args.out.join("evaluation.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
