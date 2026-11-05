mod baseline_env;
mod baseline_model;
mod diagnosis;
mod env;
mod model;
mod model2;
mod online_audit;
#[cfg(test)]
mod tests;
mod type_benchmark;
mod value_audit;

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
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

#[cfg(feature = "gpu")]
type Inner = burn::backend::Wgpu;
#[cfg(all(feature = "cpu", not(feature = "gpu")))]
type Inner = burn::backend::NdArray;
type Train = burn::backend::Autodiff<Inner>;

#[derive(Parser, Debug, serde::Serialize)]
#[command(about = "Small transformer PPO league trainer for Ninjask VGC")]
struct Args {
    /// Model2: 8x8 matrix regret matching and policy/value distillation instead of PPO.
    #[arg(long)]
    model2: bool,
    /// Fixed-weight Model2 diagnostic games, with complete failed-move traces.
    #[arg(long, default_value_t = 0)]
    diagnose_games: usize,
    /// Record raw neural values and true HP potentials in diagnostic games.
    #[arg(long)]
    diagnose_values: bool,
    /// Diagnostic replay: capture this learner decision and stop before applying it.
    #[arg(long, default_value_t = 0)]
    inspect_decision: usize,
    /// Record reconstructed and actual stats plus simulated events at this turn (diagnostics only).
    #[arg(long, default_value_t = 0)]
    search_trace_turn: u32,
    /// Replay sampled decisions to this turn, benchmark identical matrices, then exit.
    #[arg(long, default_value_t = 0)]
    benchmark_chance_turn: u32,
    /// Read encoded_state from an inspected-decision JSON and report its value.
    #[arg(long)]
    predict_inspected_state: Option<PathBuf>,
    /// Evaluate a JSON array of encoded states in batches without games or training.
    #[arg(long)]
    predict_value_states: Option<PathBuf>,
    #[arg(long, default_value_t = 256)]
    regret_iterations: usize,
    #[arg(long, default_value_t = 2)]
    matrix_samples: usize, // Legacy CLI compatibility; PreciceButSame enumeration ignores this.
    /// Maximum completed chance paths per action pair (before successor merging).
    #[arg(long, default_value_t = 16)]
    matrix_max_scenarios: usize,
    /// Mix completed-game remaining returns into Model2 value targets.
    #[arg(long, default_value_t = 0.5)]
    value_return_weight: f32,
    /// Maximum completed positions retained for value-only replay.
    #[arg(long, default_value_t = 16384)]
    value_replay_capacity: usize,
    /// Give frozen Model2 opponents the same search budget as the learner.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    matrix_opponents: bool,
    #[arg(long, num_args=1..)]
    league_weights: Vec<PathBuf>,
    #[arg(long, default_value_t = 0.)]
    elapsed_offset_seconds: f64,
    #[arg(long, default_value_t = 0.)]
    anneal_seconds: f64,
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
    /// Sample one existing training transition in N for rolling diagnostics; 0 disables.
    #[arg(long, default_value_t = 16)]
    audit_every: usize,
    /// Retain this many sampled learner transitions in the rolling window.
    #[arg(long, default_value_t = 2048)]
    audit_window: usize,
    /// Environment workers; upstream rl4burn currently requires 1 (sequential).
    #[arg(long, default_value_t = 1)]
    collection_workers: usize,
    #[arg(long, default_value_t = 20)]
    snapshot_every: usize,
    /// Add frozen league opponents at this wall-clock spacing; 0 uses --snapshot-every.
    #[arg(long, default_value_t = 0)]
    snapshot_seconds: u64,
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
    /// Compare old/new CPU type encoding on identical fixtures; no games or training.
    #[arg(long, default_value_t = 0)]
    benchmark_types: usize,
    /// Small GPU forward/backward/checkpoint test, not a meaningful training run.
    #[arg(long)]
    smoke: bool,
    /// Evaluate the checkpoint against random legal play on --teams.
    #[arg(long, default_value_t = 0)]
    evaluate: usize,
    /// Fixed checkpoint opponent for evaluation; otherwise random legal play.
    #[arg(long)]
    eval_opponent: Option<PathBuf>,
    /// Frozen original Model1 architecture as opponent (evaluation only).
    #[arg(long, conflicts_with = "eval_opponent")]
    eval_baseline: Option<PathBuf>,
    /// Choose the learner's highest-scoring legal action during evaluation.
    #[arg(long)]
    eval_greedy: bool,
    /// Save player-visible decision traces and battle events for the post-run checker.
    #[arg(long)]
    eval_traces: bool,
    /// Use identical Model2 matrix search for both fixed evaluation checkpoints.
    #[arg(long, conflicts_with = "eval_greedy")]
    eval_matrix: bool,
    /// Pure HP-difference matrix opponent with uniform candidate sampling.
    #[arg(long, requires = "eval_matrix", conflicts_with_all = ["eval_opponent", "eval_baseline"])]
    eval_hp_opponent: bool,
    /// Use sampled chance outcomes only for the HP evaluation opponent.
    #[arg(long, requires = "eval_hp_opponent")]
    eval_hp_sampled: bool,
    /// Uniform random legal learner actions, for baseline-only evaluations.
    #[arg(long, requires = "eval_matrix", conflicts_with = "eval_greedy")]
    eval_random_learner: bool,
    /// Training-team set catalog used for public hidden-set assumptions in evaluation.
    #[arg(long, requires = "eval_matrix")]
    eval_search_teams: Option<PathBuf>,
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
    if args.benchmark_types > 0 {
        return type_benchmark::run(dex, teams, &args.out, args.benchmark_types);
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
        ensure!(model.action_input_width() == model::ACTION_DESCRIPTOR,
            "Checkpoint uses {} action features; current encoding requires {}. Use fresh weights for explicit candidate move types. Old runs retain their original executable.",
            model.action_input_width(), model::ACTION_DESCRIPTOR);
    }
    println!("Parameters: {}", model.num_params());
    if let Some(path) = &args.predict_inspected_state {
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let state: Vec<f32> = serde_json::from_value(record["encoded_state"].clone())?;
        ensure!(state.len() == model::STATE, "Wrong inspected state width");
        let value = model
            .valid()
            .state_value(Tensor::from_data(
                TensorData::new(state, [1, model::STATE]),
                &device,
            ))
            .into_data()
            .to_vec::<f32>()
            .unwrap()[0];
        let result = serde_json::json!({"state":path,"weights":args.weights,"value":value});
        std::fs::write(
            args.out.join("inspected-state-value.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        println!("{result}");
        return Ok(());
    }
    if let Some(path) = &args.predict_value_states {
        let states: Vec<Vec<f32>> = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(states.iter().all(|s| s.len() == model::STATE && s.iter().all(|v| v.is_finite())), "Invalid value state");
        let predictor = model.valid();
        let mut predictions = Vec::<f32>::new();
        for chunk in states.chunks(64) {
            predictions.extend(predictor.state_value(Tensor::from_data(
                TensorData::new(chunk.iter().flatten().copied().collect::<Vec<_>>(), [chunk.len(), model::STATE]), &device
            )).into_data().to_vec::<f32>().unwrap());
        }
        ensure!(predictions.iter().all(|v| v.is_finite()), "Nonfinite predictions");
        std::fs::write(args.out.join("value-predictions.json"), serde_json::to_vec(&predictions)?)?;
        return Ok(());
    }
    if args.diagnose_games > 0 {
        ensure!(args.model2, "--diagnose-games requires --model2");
        return model2::diagnose(&args, dex, teams, model.valid(), &device);
    }
    if args.evaluate > 0 {
        return evaluate(&args, dex, teams, model.valid(), &device);
    }
    if args.model2 {
        return model2::train(&args, dex, teams, model, &device);
    }
    ensure!(args.collection_workers == 1, "Upstream rl4burn supports sequential collection only; use --collection-workers 1");
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
    ensure!(
        args.audit_window > 0 && args.collection_workers > 0,
        "Audit window and workers must be positive"
    );
    let audit = Arc::new(Mutex::new(online_audit::TrainingAudit::new(
        args.audit_window,
        args.audit_every,
    )));
    let league = Arc::new(Mutex::new(vec![model.valid()]));
    let envs: Vec<_> = (0..args.envs)
        .map(|i| {
            let mut env = BattleEnv::<Inner>::new(
                dex.clone(),
                teams.clone(),
                league.clone(),
                device.clone(),
                args.seed + i as u64 + 1,
                args.max_turns,
            );
            if args.audit_every > 0 {
                env.audit = Some(audit.clone());
                env.audit_every = args.audit_every;
            }
            env
        })
        .collect();
    let outcomes: Vec<_> = envs.iter().map(|env| env.outcomes.clone()).collect();
    let turn_rewards: Vec<_> = envs.iter().map(|env| env.turn_rewards.clone()).collect();
    let mut previous_turns = 0u64;
    let mut previous_turn_reward = 0.;
    let mut previous_outcomes = [0usize; 3];
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
        ..PpoConfig::new()
    };
    let mut rng = SmallRng::seed_from_u64(args.seed);
    let dist = ActionDist::Discrete(ACTIONS);
    Train::sync(&device).map_err(|e| anyhow::anyhow!("initial GPU sync: {e:?}"))?;
    let start = Instant::now();
    let mut training_games_seconds = 0.;
    let mut model_training_seconds = 0.;
    let mut games = 0;
    let mut last_hour = 0;
    let mut last_league_snapshot_seconds = 0.;
    let mut league_snapshots = 0usize;
    let mut league_log = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(args.out.join("league-snapshots.jsonl"))?;
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
        let collection_start = Instant::now();
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
        Train::sync(&device).map_err(|e| anyhow::anyhow!("rollout GPU sync: {e:?}"))?;
        let collection_seconds = collection_start.elapsed().as_secs_f64();
        training_games_seconds += collection_seconds;
        games += rollout.episode_returns.len();
        let fraction = if args.hour_seconds > 0 && args.max_seconds > 0 {
            (start.elapsed().as_secs_f64() / args.max_seconds as f64).min(1.)
        } else {
            (update - 1) as f64 / args.updates as f64
        };
        let lr = config.lr * (1. - fraction).max(0.05);
        let optimization_start = Instant::now();
        let (next, stats) = masked_ppo_update(
            model, &mut optim, &rollout, &dist, &config, lr, &device, &mut rng,
        );
        Train::sync(&device).map_err(|e| anyhow::anyhow!("optimization GPU sync: {e:?}"))?;
        let optimization_seconds = optimization_start.elapsed().as_secs_f64();
        model_training_seconds += optimization_seconds;
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
        let mut totals = [0usize; 3];
        for counts in &outcomes {
            for (total, count) in totals.iter_mut().zip(counts.lock().unwrap().iter()) {
                *total += count;
            }
        }
        let completed = std::array::from_fn::<_, 3, _>(|i| totals[i] - previous_outcomes[i]);
        previous_outcomes = totals;
        let mean_return = if rollout.episode_returns.is_empty() {
            None
        } else {
            Some(rollout.episode_returns.iter().sum::<f32>() / rollout.episode_returns.len() as f32)
        };
        let mut log = serde_json::json!({"update":update,"decisions":update*args.envs*args.steps,"games":games,"seconds":start.elapsed().as_secs_f64(),"policy_loss":stats.policy_loss,"value_loss":stats.value_loss,"entropy":stats.entropy,"kl":stats.approx_kl,"wins":completed[0],"losses":completed[1],"draws":completed[2],"mean_episode_return":mean_return,"training_games_seconds":training_games_seconds,"model_training_seconds":model_training_seconds,"training_games_update_seconds":collection_seconds,"model_training_update_seconds":optimization_seconds});
        log["online_audit"] = if args.audit_every > 0 {
            audit.lock().unwrap().snapshot()
        } else {
            serde_json::Value::Null
        };
        log["league_snapshots"] = serde_json::json!(league_snapshots);
        log["league_size"] = serde_json::json!(league.lock().unwrap().len());
        let (turns, rewards, unfinished) =
            turn_rewards
                .iter()
                .fold((0u64, 0., 0.), |(turns, reward, unfinished), stats| {
                    let stats = stats.lock().unwrap();
                    (
                        turns + stats.turns,
                        reward + stats.reward,
                        unfinished + stats.unfinished_reward,
                    )
                });
        let update_turns = turns - previous_turns;
        let update_reward = rewards - previous_turn_reward;
        log["completed_battle_turns"] = serde_json::json!(update_turns);
        log["completed_turn_reward_sum"] = serde_json::json!(update_reward);
        log["mean_reward_per_turn"] = serde_json::json!(if update_turns > 0 {
            Some(update_reward / update_turns as f64)
        } else {
            None
        });
        log["mean_reward_per_decision"] =
            serde_json::json!(rollout.rewards.iter().sum::<f32>() / rollout.rewards.len() as f32);
        log["unfinished_turn_reward_excluded"] = serde_json::json!(unfinished);
        previous_turns = turns;
        previous_turn_reward = rewards;
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
        let snapshot_due = league_snapshot_due(
            update,
            start.elapsed().as_secs_f64(),
            last_league_snapshot_seconds,
            args.snapshot_every,
            args.snapshot_seconds,
        );
        if snapshot_due || update == args.updates || time_limit {
            model
                .clone()
                .save_file(args.out.join(format!("checkpoint-{update:06}")), &recorder)
                .map_err(|e| anyhow::anyhow!("save checkpoint: {e:?}"))?;
            let mut bank = league.lock().unwrap();
            bank.push(model.valid());
            if bank.len() > args.league_size {
                bank.remove(0);
            }
            league_snapshots += 1;
            last_league_snapshot_seconds = start.elapsed().as_secs_f64();
            writeln!(
                league_log,
                "{}",
                serde_json::json!({"update":update,"seconds":last_league_snapshot_seconds,
                "league_size":bank.len(),"snapshot":league_snapshots,"reason":if snapshot_due {"interval"} else {"final"}})
            )?;
            league_log.flush()?;
        }
        if time_limit {
            println!("Training time budget reached after update {update}");
            break;
        }
    }
    let training_wall_seconds = start.elapsed().as_secs_f64();
    std::fs::write(
        args.out.join("timing.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "training_wall_seconds":training_wall_seconds,
            "training_games_seconds":training_games_seconds,
        "measured_audit_seconds":audit.lock().unwrap().seconds,
            "model_training_seconds":model_training_seconds,
            "training_other_seconds":(training_wall_seconds-training_games_seconds-model_training_seconds).max(0.),
            "games":games,"method":"Wall-clock phase timers with GPU synchronization; excludes initial setup and smoke verification"
        }))?,
    )?;
    println!("Timing: training games {training_games_seconds:.3}s; model training {model_training_seconds:.3}s");
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

fn league_snapshot_due(
    update: usize,
    elapsed: f64,
    last: f64,
    every_updates: usize,
    every_seconds: u64,
) -> bool {
    if every_seconds > 0 {
        elapsed - last >= every_seconds as f64
    } else {
        update.is_multiple_of(every_updates)
    }
}

#[cfg(test)]
mod snapshot_tests {
    #[test]
    fn timed_league_snapshots_use_elapsed_time_not_update_count() {
        let due = super::league_snapshot_due;
        assert!(!due(100, 299., 0., 100, 300));
        assert!(due(7, 300.1, 0., 100, 300));
        assert!(!due(8, 599., 300.1, 100, 300));
        assert!(due(9, 600.2, 300.1, 100, 300));
        assert!(due(100, 20., 0., 100, 0));
    }
}

fn evaluate(
    args: &Args,
    dex: Arc<Dex>,
    teams: Arc<Vec<Team>>,
    model: Model<Inner>,
    device: &<Inner as Backend>::Device,
) -> Result<()> {
    let evaluation_start = Instant::now();
    use ninjask::{tracker::Tracker, Battle};
    use rand::RngExt;
    let baseline = if let Some(path) = &args.eval_baseline {
        Some(
            baseline_model::Model::<Inner>::new(device)
                .load_file(
                    path,
                    &BinFileRecorder::<FullPrecisionSettings>::new(),
                    device,
                )
                .map_err(|e| anyhow::anyhow!("baseline checkpoint: {e:?}"))?,
        )
    } else {
        None
    };
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
    let search_bank = if args.eval_matrix {
        ensure!(args.evaluate > 0 && args.evaluate % 2 == 0, "Matrix evaluation requires positive even games");
        ensure!(args.regret_iterations > 0, "Invalid search budget");
        Some(model2::catalog_path(args.eval_search_teams.as_ref().ok_or_else(|| anyhow::anyhow!("--eval-search-teams is required"))?, &dex)?)
    } else { None };
    let mut search_requests = [0usize; 2];
    let mut search_positions = [0usize; 2];
    let mut search_simulations = [0usize; 2];
    let mut scores = [0usize; 3];
    if let Some(opponent) = &opponent {
        ensure!(
            opponent.action_input_width() == model::ACTION_DESCRIPTOR,
            "Opponent checkpoint action width does not match current encoding"
        );
    }
    let mut results = vec![];
    if args.eval_traces {
        std::fs::create_dir_all(args.out.join("traces"))?;
    }
    Inner::sync(device).map_err(|e| anyhow::anyhow!("evaluation GPU sync: {e:?}"))?;
    let games_start = Instant::now();
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
        let mut trace = vec![];
        while !b.ended && b.turn < args.max_turns && decisions < 512 {
            env::sync(&b, &mut ts, &mut cursor);
            let log_start = b.log.len();
            let previous_hp = env::hp_advantage(&b, side);
            let mut decision_trace = serde_json::json!({"decision":decisions,"turn":b.turn,
                "request":format!("{:?}", b.request)});
            if args.eval_traces {
                decision_trace["state"] = diagnosis::state(&ts[side]);
            }
            let mut selected = vec![];
            for (s, t) in ts.iter().enumerate() {
                if b.side_needs_choice(s) {
                    let cs = env::choices(&b, s);
                    let i = if s == side && args.eval_random_learner {
                        actor_rng.random_range(0..cs.len())
                    } else if args.eval_matrix && s != side && opponent.is_none() && !args.eval_hp_opponent {
                        opponent_rng.random_range(0..cs.len())
                    } else if let Some(bank) = &search_bank {
                        let role = if s == side { 0 } else { 1 };
                        let active_model = if s == side { &model } else { opponent.as_ref().unwrap_or(&model) };
                        let active_rng = if s == side { &mut actor_rng } else { &mut opponent_rng };
                        if b.request == ninjask::state::RequestState::Move { search_requests[role] += 1; }
                        let hp_only = s != side && args.eval_hp_opponent;
                        let searched = model2::search_position_mode(active_model, &b, t, &cs, bank, args, device, active_rng, hp_only)?;
                        if let Some(result) = searched {
                            search_positions[role] += 1;
                            search_simulations[role] += result.cells;
                            if args.eval_traces && s == side {
                                decision_trace["search"] = result.evidence;
                                decision_trace["observation_state"] = serde_json::json!(env::encode_state(t));
                            }
                            result.chosen
                        } else {
                            if hp_only { active_rng.random_range(0..cs.len()) } else { env::sample(active_model, &b, t, &cs, device, active_rng) }
                        }
                    } else if s == side {
                        if args.eval_traces {
                            // Save the exact vector used by this forward pass, not a
                            // reconstruction from tracker labels after the turn.
                            let values = env::observe(&b, t, &cs);
                            let obs = Tensor::from_data(
                                TensorData::new(values.clone(), [1, model::OBS]),
                                device,
                            );
                            let (logits, _) = model.forward(obs);
                            let scores = logits.clone().into_data().to_vec::<f32>().unwrap();
                            let i = if args.eval_greedy {
                                (0..cs.len())
                                    .max_by(|&a, &b| {
                                        scores[a].total_cmp(&scores[b]).then(b.cmp(&a))
                                    })
                                    .unwrap()
                            } else {
                                let mut mask = vec![0.; ACTIONS];
                                mask[..cs.len()].fill(1.);
                                let mask =
                                    Tensor::from_data(TensorData::new(mask, [1, ACTIONS]), device);
                                ActionDist::Discrete(ACTIONS).sample(
                                    &logits,
                                    Some(&mask),
                                    None,
                                    &mut actor_rng,
                                )[0][0] as usize
                            };
                            decision_trace["observation_state"] =
                                serde_json::json!(&values[..model::STATE]);
                            let start = model::STATE + i * model::ACTION_FEATURES;
                            decision_trace["selected_action_features"] =
                                serde_json::json!(&values[start..start + model::ACTION_FEATURES]);
                            decision_trace["action_descriptor_width"] =
                                serde_json::json!(model::ACTION_DESCRIPTOR);
                            decision_trace["action_type_offset"] =
                                serde_json::json!(model::ACTION_TYPE_OFFSET);
                            decision_trace["action_type_count"] =
                                serde_json::json!(model::ACTION_TYPE_COUNT);
                            let peak = scores[..cs.len()]
                                .iter()
                                .copied()
                                .fold(f32::NEG_INFINITY, f32::max);
                            let weights: Vec<f64> = scores[..cs.len()]
                                .iter()
                                .map(|x| ((*x - peak) as f64).exp())
                                .collect();
                            let total: f64 = weights.iter().sum();
                            let mut order: Vec<usize> = (0..cs.len()).collect();
                            order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
                            decision_trace["selected_probability"] =
                                serde_json::json!(weights[i] / total);
                            if let env::Choice::Turn(chosen) = &cs[i] {
                                let marginals: Vec<_> = (0..chosen.len())
                                    .map(|k| {
                                        let mut groups = std::collections::BTreeMap::<
                                            String,
                                            (usize, f64),
                                        >::new(
                                        );
                                        for (j, c) in cs.iter().enumerate() {
                                            if let env::Choice::Turn(slots) = c {
                                                let entry = groups
                                                    .entry(format!("{:?}", slots[k]))
                                                    .or_insert((j, 0.));
                                                entry.1 += weights[j] / total;
                                            }
                                        }
                                        groups
                                            .into_values()
                                            .map(|(j, p)| {
                                                serde_json::json!({
                                        "choice":diagnosis::choice(&b,t,&cs[j],&cs)[k],
                                        "probability":p})
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .collect();
                                decision_trace["slot_policy"] = serde_json::json!(marginals);
                            }
                            decision_trace["policy_top_actions"] = serde_json::json!(order
                                .into_iter()
                                .take(5)
                                .map(
                                    |j| serde_json::json!({"index":j,"probability":weights[j]/total,
                                    "choice":diagnosis::choice(&b,t,&cs[j],&cs)})
                                )
                                .collect::<Vec<_>>());
                            i
                        } else if args.eval_greedy {
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
                    } else if let Some(baseline) = &baseline {
                        baseline_env::sample(baseline, &b, t, &cs, device, &mut opponent_rng)
                    } else if let Some(opponent) = &opponent {
                        env::sample(opponent, &b, t, &cs, device, &mut opponent_rng)
                    } else {
                        opponent_rng.random_range(0..cs.len())
                    };
                    if args.eval_traces && s == side {
                        decision_trace["selected"] = diagnosis::choice(&b, t, &cs[i], &cs);
                        decision_trace["selected_index"] = serde_json::json!(i);
                        decision_trace["legal_actions"] = serde_json::json!(cs.len());
                    }
                    selected.push((s, cs[i].clone()));
                }
            }
            for (s, c) in selected {
                env::apply(&mut b, s, c);
            }
            if args.eval_traces {
                decision_trace["events"] = serde_json::json!(
                    b.log_lines(ninjask::battle::log::Viewer::Side(side), log_start)
                );
                decision_trace["hp_progress"] =
                    serde_json::json!(env::hp_advantage(&b, side) - previous_hp);
                trace.push(decision_trace);
            }
            decisions += 1;
        }
        let outcome = if b.ended {
            b.winner.map_or(2, |s| if s == side { 0 } else { 1 })
        } else {
            2
        };
        scores[outcome] += 1;
        if args.eval_traces {
            std::fs::write(
                args.out.join("traces").join(format!("game-{game:04}.json")),
                serde_json::to_vec(&serde_json::json!({"schema_version":1,"game":game,
                    "match_seed":match_seed,"learner_side":side,"team_a":a,"team_b":c,
                    "outcome":outcome,"decisions":trace}))?,
            )?;
        }
        results.push(serde_json::json!({"game":game,"match_seed":match_seed,"learner_side":side,"team_a":a,"team_b":c,"score":match outcome {0=>1.,1=>0.,_=>0.5},"turns":b.turn}));
    }
    Inner::sync(device).map_err(|e| anyhow::anyhow!("evaluation completion sync: {e:?}"))?;
    let evaluation_games_seconds = games_start.elapsed().as_secs_f64();
    let opponent_name = args
        .eval_baseline
        .as_ref()
        .or(args.eval_opponent.as_ref())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| if args.eval_hp_opponent { "HP-only matrix search".into() } else { "random legal".into() });
    let report = serde_json::json!({"opponent":opponent_name,"greedy":args.eval_greedy,"matrix_search":args.eval_matrix,"hp_only_opponent":args.eval_hp_opponent,"random_learner":args.eval_random_learner,
        "search_requests":search_requests,"search_positions":search_positions,"search_simulations":search_simulations,
        "search_fallbacks":[search_requests[0]-search_positions[0],search_requests[1]-search_positions[1]],
        "search_catalog":args.eval_search_teams,"matrix_samples":if args.eval_hp_sampled {Some(args.matrix_samples)} else {None},
        "chance_method":"enumerate_outcomes_precice_but_same",
        "hp_opponent_chance_method":if args.eval_hp_sampled {"sampled"} else {"enumerate_outcomes_precice_but_same"},
        "simulation_count_unit":if args.eval_hp_sampled {"learner: merged enumerated successors; HP opponent: sampled simulations"} else {"merged enumerated successor states"},
        "regret_iterations":args.regret_iterations,"games":args.evaluate,"wins":scores[0],"losses":scores[1],"draws":scores[2],"seed":args.seed,"teams":args.teams,"weights":args.weights,"results":results,"evaluation_games_seconds":evaluation_games_seconds});
    println!(
        "Evaluation: {} games, {} wins, {} losses, {} draws against {}",
        args.evaluate, scores[0], scores[1], scores[2], opponent_name
    );
    std::fs::write(
        args.out.join("evaluation.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let evaluation_wall_seconds = evaluation_start.elapsed().as_secs_f64();
    std::fs::write(
        args.out.join("timing.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "evaluation_games_seconds":evaluation_games_seconds,
            "evaluation_wall_seconds":evaluation_wall_seconds,
            "evaluation_other_seconds":(evaluation_wall_seconds-evaluation_games_seconds).max(0.),
            "games":args.evaluate,"method":"Games include inference, simulation and optional trace writing; initial learner/dex loading occurs before this function"
        }))?,
    )?;
    println!("Timing: evaluation games {evaluation_games_seconds:.3}s");
    Ok(())
}
