# Model1 experimental: speed ranges and entity-based actions

Experimental descendant of [ppo-cfr-checkpoint](https://github.com/CalebBrody/ppo-cfr-checkpoint). The baseline architecture remains in that repository. This repository preserves the shared baseline commit and changes the observation/action architecture.

## Changes

- Uses Ninjask's existing `Tracker::speed_bounds(side, index)` directly. Each Pokemon receives a known flag, minimum, maximum, interval width, and exact-value flag. Numeric speeds are divided by 500. These are **unmodified Speed bounds**, not effective speed after weather, boosts, items, or Trick Room. Those battle conditions remain separate inputs. No new speed inference engine is added.
- Removes the roster-index and team-position one-hot inputs. The shared transformer has no positional embeddings and pools entities by averaging, so merely rearranging entity storage does not change the value prediction.
- Scores each action with contextual embeddings of the Pokemon it involves. Preview actions refer to four selected Pokemon in lead/bench roles. Turn actions refer to both active actors and their switch destinations or explicit move targets. Four integer references are used only for tensor lookup; their numeric values never enter a learned layer.
- Retains meaningful battlefield roles: your side versus their side, active position, target position, and lead versus bench roles. Reassigning those roles can change battle outcomes and should change predictions.
- Uses a 384-to-64 joint-action layer combining pooled state, action descriptors, and four entity embeddings. Legal-action masking and the sampling retry guard remain in place.

Changing team order requires remapping engine action references, which the observation adapter handles. With matched references, the policy scores corresponding actions equally within floating-point tolerance. This does not require the engine to abandon its internal indices.

## HP reward

Training pays for changes in HP advantage, not repeatedly for the same state:
`potential = 0.5 * (own_health - opponent_health)` and
`reward = next_potential - previous_potential + terminal_result`.
Each side's health is the average of
`hp / maxhp` over its four brought Pokemon, including the bench; fainted Pokemon
contribute zero and remain in the denominator. Preview rewards are zero.
Rewards use simulator HP for training only; opponent hidden HP is not added to
policy observations. This reward is collected after the environment advances
to the next learner decision, including any forced replacement requests.

For example, moving from 100%/100% to 80%/40% yields +0.2. Staying at 80%/40%
yields zero; moving next to 80%/20% yields +0.1. A health reversal pays a negative
reward, so healing/damage cycles cannot farm net reward. Forced replacements
with no HP change also pay zero.

On the final decision, the final HP change is still paid, and a win adds +1,
a loss -1, a draw or finite-horizon cutoff 0. We retain actual final HP rather
than forcing terminal potential to zero. With gamma 1, health rewards telescope:
`episode_return = terminal_result + final_potential - initial_potential`.
Starting with both teams full, a win with 80% team HP remaining has return 1.4,
regardless of idle decisions along the way. Thus this keeps an explicit final-HP
preference in addition to winning; it is not strictly win-only invariant shaping.
The critic predicts remaining future rewards, not the current HP advantage.

The experimental value head is unbounded because cumulative rewards can exceed
[-1, 1]. Metrics count actual battle outcomes independently of reward sign;
`mean_episode_return` reports the shaped return of games completed that update
(null when none finish). Evaluation still reports battle wins/losses/draws.

## Checkpoint compatibility

This architecture needs **new training**. Baseline checkpoints do not contain the new action layer and must not be used as a resume checkpoint. The original 24-hour run remains stopped. Short smoke runs validate execution, not playing strength.

## Setup and commands (CMD)

Install Rust and use a Vulkan-capable GPU. Ninjask uses the adjacent checkout; rl4burn is fetched from GitHub at the revision in Cargo.lock. Licenses and provenance are in [DEPENDENCIES.md](DEPENDENCIES.md). Team data and generated dex data are excluded from Git.

Place packed training teams in `data/teams.txt` and evaluation teams in `data/eval-teams.txt`. The spreadsheet importer is `scripts/import_teams.py`. Supply a Ninjask format export with `--dex PATH`, or place it at `vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json`. The tests use that default path.

```cmd
cargo test --no-default-features --features cpu
cargo run --release -- --smoke --out runs\gpu-smoke
cargo run --release -- --updates 1000 --out runs\training-new
```

The test suite checks speed features against tracker observations (including unknown/Transform), hidden-opponent-stat isolation, legal actions over complete games, and policy/value invariance to entity/action permutations at both preview and battle phases. GPU smoke additionally checks backward propagation and checkpoint round-trip.

## Validation on November 3, 2026

- Five CPU regression tests passed, including entity/action-order invariance at preview and during battle.
- GPU build and Clippy with warnings denied passed.
- RTX 2060 GPU smoke: 143,298 parameters; finite PPO update, changed weights, and exact checkpoint round-trip (maximum logit error 0).
- Two full-game PPO updates: 128 learner decisions, eight completed games, and finite losses. This is an execution check, not evidence of stronger play.

## Online training audit and collection workers

Pokemon types are now explicitly encoded by stable names in a 20-bit current-type
block (ordinary types, Stellar and typeless), with known/override flags. Tracked
type changes replace species defaults; added types are unioned. The legacy type
band also reflects current ordinary types. Move types use the same name mapping,
fixing Water being dropped by the former raw-ID `<18` cutoff. Stellar move flags
are separate from move-category bits. This Pokemon-state change preserves the
256-feature entity width. No unrevealed opponent typing is read.
The old encoder is retained only for `--benchmark-types N`, an explicit CPU-only
microbenchmark on identical fixtures; it is not used during training.

Candidate actions now append two explicit 20-type one-hot blocks to the original
32 numeric/hash features: actor 1 types at 32..52 and actor 2 types at 52..72
(Rust exclusive-end ranges), using `TYPE_FEATURE_NAMES`. The action descriptor is
72 wide, followed by four entity routing references (76 values total). These
flags encode the selected move's dex type directly. They do not compute type
effectiveness or dynamic type conversions. Preview/switch/pass type blocks stay
zero; an unavailable move entry also stays unknown. The network grows from
143,298 to 145,858 parameters. Previous 32-wide action checkpoints are incompatible
and rejected explicitly; use fresh weights, and use archived executables to inspect
old runs. The Pokemon entity input stays at 256 features.

Training emits `online_audit` in every metrics row: ratios of rolling event counts
on existing randomly sampled learner transitions. Defaults are one transition in
16 and a bounded window of 2048 samples. It counts learner immunity events, known
type-immunity events on unchanged targets, failed moves, Double Shock/Steel Roller
prerequisite warnings and actual failures, target shares when both foes are legal,
preview concentration, and descriptive switching/Protect/accuracy signals. No extra
policy inference or evaluation games are run, and full battle histories are not
copied or saved. Diagnostics use a separate RNG and do not affect training choices.
Use `--audit-every 0` to disable or `--audit-every 1` to inspect every transition.
Null rates mean no denominator, not zero mistakes. Self-play opponents change;
these rates are not a fixed-opponent playing-strength estimate.

`train_timed.py` now defaults to `--games 0` and plots rolling immunity/failure rates
from training metrics. Explicit `--games 200` retains fixed-opponent evaluations.
`--collection-workers 1` uses upstream rl4burn's sequential collector. The former
local parallel extension is retained only in the archived vendor snapshot.

## Other limitations

Phase timings are stored in each training/evaluation output's `timing.json` and
in training metrics. Run `python scripts/summarize_timing.py RUN_DIRECTORY` for
Training games (collection/inference), Model training (PPO optimization), and
Evaluation games. The issue checker also includes this summary. GPU synchronization
at phase boundaries attributes queued work to the appropriate phase. Timings are
wall-clock durations, including resource contention; concurrent evaluation times
must not be added to training time as end-to-end elapsed time.

After every run, inspect `issue-check.json`; see `AGENTS.md` for the required
workflow. Timed runs audit rolling training counters automatically. Explicitly
requested evaluations additionally record decision traces. For a standalone audit,
evaluate with `--eval-traces`, then run
`python scripts/check_run.py <evaluation-directory>`. The checker reports target
and preview collapse, observed immunity events, visible type-immunity candidates,
known move prerequisite problems, per-team losses, and training/source checks.
Missing traces are untested, not clean. Candidate flags require replay review;
the report states the sample thresholds and limitations. Add `--fail-on-issues`
when a nonzero exit status is desired for detected issues or incomplete coverage.

The model still uses compact hashed species, ability, item, and move features, with possible collisions. It does not prove full invariance to every change in simulator history or stochastic sampling. It preserves move feature ordering inside a Pokemon. Evaluation against strong opponents is still required before claiming an improvement over the baseline.


## Timed training with interval graphs

Use the reusable runner from this folder in CMD:

```cmd
python scripts\train_timed.py --duration 20m --interval 5m --out E:\tmp\experiment-new
```

For four hours with a graph every 30 minutes:

```cmd
python scripts\train_timed.py --duration 4h --interval 30m --games 200 --out E:\tmp\experiment-four-hours
```

The output directory must be new. The script builds and freezes the executable,
training teams, dex and driver sources (plus held-out teams when evaluations are
enabled). One continuous trainer preserves
its optimizer and opponent league between reports. By default no evaluation games
are run: graphs show rolling immunity/failure rates, self-play record and value
loss, plus average reward per completed battle turn (five plots total). The league
score is against sampled frozen training opponents, not random legal play.
`--league-interval 5m` adds frozen opponents approximately every five training
minutes, independent of reporting frequency. Actual snapshot times are logged in
`training/league-snapshots.jsonl`; current games retain their original opponent.
Explicit positive `--games` opts into starting/interval/final evaluations
against random play and the frozen starting model. The learning rate anneals by
elapsed time in interval-checkpoint mode.

`status.json` records lifecycle and errors; `latest-report.json` links to the newest
PNG and statistics; individual graphics live in `reports/update-XXXXXX/`.
When explicitly enabled, evaluations share GPU time with training and can delay
reports; final evaluation adds time after training stops. Very short intervals
may be crossed by a single PPO update; only actual saved checkpoints are plotted.

Use `--weights PATH_WITHOUT_BIN` for a compatible warm start (optimizer resets),
`--teams`, `--eval-teams`, and `--dex` for custom inputs. NumPy and Matplotlib are
required. `--skip-build` is only for a verified current executable. Creating a
`STOP` file in the output folder or pressing Ctrl+C stops the run while preserving
saved checkpoints. This script does not create chat notification schedules.

Future agents: follow [AGENTS.md](AGENTS.md) for this workflow.
