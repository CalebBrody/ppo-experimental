# Model1

A small Rust PPO self-play trainer using the adjacent `Ninjask` simulator and
`rl4burn` 0.1.1 source package. GPU training uses Burn 0.20.1 through WGPU/Vulkan
on the first discrete GPU, suitable for the RTX 2060 installed on this machine.
CUDA Toolkit and Python ML packages are not required.

## Verified on this machine (2026-11-02)

- Detected and used NVIDIA GeForce RTX 2060 via Vulkan, driver 616.56.
- Model size: 118,658 trainable parameters.
- Imported 755 spreadsheet records: excluded 147 incomplete sets and deduplicated
  six repeated packed teams, leaving 547 training and 55 held-out sets. Ninjask
  rejected two additional training sets (unknown Triage / Precipice Blades), so
  the effective pool is 545 training and 55 evaluation teams.
- Three Rust environment tests and four Python importer tests passed; formatting
  and Clippy passed with warnings denied.
- `runs/gpu-smoke`: GPU PPO update changed weights, losses stayed finite, and
  reloaded checkpoint logits matched exactly (maximum error 0).
- `runs/gpu-validation`: two PPO updates, 128 learner decisions, ten completed
  full games and two league snapshots. These short runs verify execution, not
  learned playing strength.
- `runs/evaluation-smoke`: loaded a saved model and completed four held-out-team
  games against random legal play. The sample is too small to assess strength.

The imported rl4burn crate archive is version 0.1.1, SHA-256
`d1efdf889dcfe5861098e5ae80ab3040aca2d9b1a480b5c1448ec5f0acfe817e`.
Its packaged source records Git commit `658b34a812a88ae14c558e6f102719b42da9bb6b`.
The tested dependency versions are pinned by `Cargo.lock`.

## Measuring learning progress

The bounded experiment driver trains for a configured wall-clock budget, then
evaluates the starting model, intermediate checkpoints and final model against
both random legal play and the frozen starting model. It uses held-out teams,
independent seeds for each matchup, and side-swapped game pairs. Checkpoints
share the same evaluation setups, regardless of how long earlier games lasted.

```cmd
python scripts\progress_run.py --out E:\tmp\Model1-progress-new --seconds 900
python scripts\plot_progress.py E:\tmp\Model1-progress-new --out reports\progress-new
```

The plotter requires NumPy and Matplotlib (available on this machine). The runner
uses 400 evaluation games per opponent at the initial/final endpoints and 100 at
intermediate checkpoints. It records per-game results and source/binary provenance.
The graph's uncertainty bars resample matchup pairs, keeping side-swapped games
together. Endpoint changes use paired resampling of the same evaluation setups.
These intervals describe evaluation noise for one trained model, not variation
across independent training seeds. Self-play scores and value loss are diagnostics;
fixed-opponent results are the progress measurement.

The training CLI also accepts `--max-seconds 900`; it stops after the current
update and saves that checkpoint. `--eval-opponent PATH` evaluates against a
specified frozen model instead of random play. Every training run now saves
`initial.bin` so its starting point can be measured.

## Start from CMD

```cmd
cd /d C:\Users\main\Documents\AI\poke\njk\Model1
cargo run --release -- --validate
cargo run --release -- --smoke --out runs\smoke-new
cargo run --release -- --updates 1000 --out runs\training-001
```

The initial Rust build takes several minutes. The smoke run checks a GPU forward
and backward pass, finite PPO losses, changed weights, and checkpoint save/load.
It is deliberately short and does not produce a useful trained policy.
Choose a fresh output directory for each run; the trainer refuses to overwrite
an existing metrics log. Stop training with Ctrl+C; completed periodic checkpoints
remain available.

To re-import the spreadsheet (public network access required on the first run):

```cmd
python scripts\import_teams.py --xlsx ..\..\vgc.xlsx
```

Pastes are cached in `data/pastes`. The source spreadsheet is read only. Its first
sheet provides team IDs and Pokepaste URLs; strategy labels are not training
targets. Complete six-Pokemon sets are converted to Showdown packed format with
Champions stat points, split deterministically by a hash into training and held-out
teams, and deduplicated. `data/import-report.json` records source hash, provenance,
failures and missing fields. Missing spreads/natures are excluded by default.
`--allow-incomplete` explicitly allows zero-point/Serious defaults, with assumptions
recorded. Conventional 252-EV spreads are rejected rather than silently converted.

The trainer additionally checks every team's moves, items and abilities against
Ninjask's support report. Unsupported teams are excluded, with reasons and line
numbers in the run's `team-validation.json`. This is simulator support validation,
not a substitute for a tournament team legality checker.

## Training design

Inspired by the public [Mikumiku37 write-up](https://www.smogon.com/forums/threads/mikumiku37-a-self-play-vgc-bot-reached-1-on-the-vgc-reg-m-c-ladder.3789199/).
This is an independent, much smaller baseline, not its unpublished implementation
or weights. It makes no claim to reproduce its ladder performance or training speed.

- Two 64-wide, four-head transformer blocks encode 12 Pokemon tokens and a field
  token. A shared encoder feeds a policy scorer and a value head.
- Inputs come from Ninjask's `Tracker`: own information, public opponent reveals,
  and static dex data. Opponent unrevealed moves, items, abilities and exact stats
  are not taken from the simulator state. Categorical identities use small hashed
  feature groups, which can collide; this is a compact baseline representation.
- One policy score per complete legal doubles action captures coordinated choices,
  targets, switches and Mega Evolution. It also scores all 360 ordered bring-four
  choices at preview. Padding is masked; exceeding the 2,048-action capacity fails
  explicitly rather than silently removing actions.
- Each game samples a learner side and two training teams. A frozen opponent is
  sampled uniformly from the league and stays fixed for that game. Both sides
  choose before either sees the other side's submitted action.
- Rewards are +1 for a win, -1 for a loss and 0 for a draw, with no damage shaping.
  The finite-horizon variant declares a draw at 150 turns or 512 learner decisions.
  These draws are terminal, so GAE does not bootstrap across resets.
- `rl4burn::masked_ppo_collect` collects learner experience; `masked_ppo_update`
  trains both heads using clipped PPO, GAE, entropy regularization, value clipping,
  gradient clipping and target-KL early stopping. Gamma is 1, lambda is 0.95.
- Every 20 updates a frozen snapshot joins the league; the most recent eight are
  retained. The initial opponent is an untrained copy. In-flight games retain
  their original opponent even when a snapshot is evicted from the bank.

Defaults: four environments, 64 learner decisions per environment per update,
minibatches of 16, four optimization epochs, linearly annealed learning rate from
0.00025, clip 0.2 and entropy coefficient 0.01. GPU inference and gradients use
float32. Battle execution and opponent inference are currently sequential through
the synchronous vector environment; learner inference is batched. This is a
correctness-first training baseline, not a high-throughput distributed trainer.

The reference bot's inference-time simultaneous-game search is **not implemented**
here. Training uses sampled battle outcomes, as ordinary PPO does, and does not
call exhaustive outcome enumeration. No Showdown networking or ladder play is included.

## Checkpoints, metrics and evaluation

Each update prints and appends JSON to `runs/<name>/metrics.jsonl`: completed games,
decisions, wins/losses/draws, elapsed seconds, policy/value losses, entropy and KL.
Checkpoints are saved every 20 updates and at the final update as `.bin` files.
The configuration and support-validation report are stored beside them.

Warm-start a new run from saved **model weights**:

```cmd
cargo run --release -- --weights runs\training-001\checkpoint-001000 --updates 1000 --out runs\training-002
```

Pass the checkpoint path without `.bin`. This resets Adam, the learning-rate
schedule, the RNG streams and league; it is not an exact optimizer-state resume.

Evaluate on held-out teams against a random legal policy, alternating sides:

```cmd
cargo run --release -- --weights runs\training-001\checkpoint-001000 --teams data\eval-teams.txt --evaluate 100 --out runs\evaluation-001
```

This produces `evaluation.json`. Win rate against random play is only an initial
sanity check, not an Elo estimate or proof of competitive strength.

Evaluation samples the learned policy by default, including its remaining
exploration. Add `--eval-greedy` to select the learner's highest-scoring legal
action instead. The opponent's behavior is unchanged. Compare both modes with
the same checkpoint, teams, seed and game count; `evaluation.json` records the
`greedy` setting. Greedy selection is a diagnostic and need not improve play.

Run environment tests and formatting checks:

```cmd
cargo test --release
cargo fmt --check
```

An optional CPU backend is available for debugging:

```cmd
cargo run --release --no-default-features --features cpu -- --smoke --out runs\cpu-smoke
```

If VRAM is tight, reduce `--envs` and `--batch` (batch must be at least 2), and close
other GPU-heavy apps. `--steps` primarily changes CPU rollout storage. The default
GPU build explicitly selects the first discrete GPU and prints its adapter name.

## Continuous 24-hour run

From this folder, in CMD:

```cmd
python scripts\hourly_run.py --out E:\tmp\Model1-new-24h --weights E:\tmp\Model1-20261102-progress\training\checkpoint-000148
```

The output folder must be new. This warm-starts the previous model and keeps one
trainer running for 24 hours, preserving its optimizer and opponent league across
hour boundaries. Hour 0 is the starting model. Each hour saves a checkpoint,
evaluates 100 side-swapped games against each earlier hourly checkpoint and 200
against random play, and writes progress and matchup graphics under `reports`.
Evaluation runs alongside training; the final report takes additional evaluation
time after training stops. `status.json` and `latest-report.json` identify progress
and the latest graphics. Executable, inputs and source are copied into the run.

The supervisor prevents automatic Windows sleep while running. Keep the computer
powered on; scheduled chat delivery additionally requires the Codex app running.
To request an early stop, create a `STOP` file in the run folder, for example:

```cmd
type nul > E:\tmp\Model1-new-24h\STOP
```

Existing saved checkpoints remain available. This stops training and evaluation;
it does not save an extra checkpoint at the moment of stopping.

## Files

- `src/model.rs`: transformer and policy/value heads.
- `src/env.rs`: player-view features, legal joint actions and league opponent play.
- `src/main.rs`: validation, PPO training, checkpointing and evaluation CLI.
- `scripts/import_teams.py`: spreadsheet and paste import using Python's standard library.
- `../rl4burn`: published 0.1.1 crate source with a local legal-action sampling guard, including its MIT license.
- `../Ninjask`: existing simulator dependency; Model1 does not modify it.
