# Timed training and graphs

Model2 matrix-search/distillation instructions are in `Model2/README.md`. Use the
same supervisor with `--trainer model2`. Keep its run folders under `Model2/runs`.
Do not describe its optimization losses as PPO or search-assisted diagnostics as
standalone policy evaluation. Count matrix simulations separately from played games.
For symmetric Model2 runs, verify `opponent_matrix_positions` against
`opponent_move_requests` and report search fallbacks. Plot labels must distinguish
matrix opponents from the older policy-only league. Use
`scripts/compare_symmetric_league.py RUN` for the three-hour continuation comparison.
For interruption recovery, preserve the failed run and resume into a new folder.
The supervisor accepts `--league-weights` to restore frozen checkpoints,
`--elapsed-offset-seconds` and `--anneal-seconds` to preserve Model2's LR schedule.
Adam state is not saved. Exclude unsaved-tail metrics from a stitched learning
curve, but report their actual compute cost separately. JSON status writes retry
transient Windows sharing violations instead of killing a healthy trainer at once.

For requests to train this experimental model for a duration and produce progress
graphs, use `scripts/train_timed.py`. Do not create another one-off runner or
restart training at every reporting interval.

Example from this repository in CMD:

```cmd
python scripts\train_timed.py --duration 20m --interval 5m --games 0 --out E:\tmp\experiment-new
```

- Duration suffixes: `s`, `m`, `h`, `d`. Bare numbers are seconds.
- Always use a new output directory. Preserve existing model checkpoints.
- Default behavior builds the current release executable and freezes it and inputs
  into the run directory. Use `--skip-build` only when the release executable is
  verified to match the intended sources. Do not share Cargo target directories
  between the baseline and experimental working copies.
- `--weights PATH_WITHOUT_BIN` optionally warm-starts compatible weights. Optimizer
  and league start fresh; this is not exact optimizer-state resume. Original
  baseline weights are incompatible with the experimental architecture.
- Current candidate-action encoding has 72 descriptor features (32 original plus
  two explicit 20-type blocks), followed by four routing references. Older
  experimental checkpoints with 32 descriptor features are also incompatible.
  Start fresh for this encoding; inspect old runs using their frozen executable.
  Entity vectors remain 256 wide. Candidate flags currently describe dex move
  types, not dynamically converted types or a computed effectiveness multiplier.
- `--games` defaults to 0: do not run evaluation games for routine online monitoring.
  Rolling counters come from existing sampled learner transitions. `--audit-every`
  defaults to 16 and `--audit-window` to 2048 sampled transitions. Report numerator,
  denominator and sample count; zero events in a small window is not resolution.
- Explicit `--games 200` opts into fixed-opponent strength checks against random
  play and the frozen starting model. Positive counts must be even. Setups and
  swapped sides match across checkpoints. Do not silently add evaluation games.
- Use `--collection-workers 1`: upstream GitHub rl4burn provides sequential
  collection. The former local parallel extension is not in the active dependency.
- Frozen league opponents use `--league-interval 5m` by default in timed runs.
  This passes `--snapshot-seconds 300` and replaces update-count scheduling for
  league snapshots. Reporting intervals remain independent. Check actual times in
  `training/league-snapshots.jsonl`; in-flight games keep their original opponent.
- Keep one league training-score plot, titled to say it is against sampled frozen
  league opponents. It is wins plus half draws, not random-play evaluation. The
  fifth panel plots average training reward per completed battle turn: sum the
  `completed_turn_reward_sum` and divide by `completed_battle_turns` over the last
  20 updates. Preview is excluded; forced replacement choices share their turn.
  Unfinished artificial-cutoff rewards are tracked separately. Historical runs
  without these fields must show unavailable, never derive the rate from win rate
  or episode return. Metrics expose reward per learner decision separately.
- Training is continuous. Evaluations share GPU resources with it; graphics appear
  after evaluation finishes, not necessarily exactly on the interval. If a PPO
  update crosses multiple intervals, only actual saved checkpoints are evaluated.
  Final evaluation can extend total runtime beyond the training budget.
- This uses the trainer's `--hour-seconds` option as a general checkpoint interval.
  That mode also anneals learning rate by elapsed training time. It does not
  reproduce the earlier 20-minute run's constant learning rate exactly.
- Inspect `status.json`, `training.log`, `latest-report.json`, and each
  `reports/update-XXXXXX/summary.json`. Show the PNG named in `latest-report.json`
  inline using its absolute path. Include the actual duration, games and evaluation
  results. Do not present rising self-play scores as fixed-opponent strength.
- `STOP` in the output folder or Ctrl+C stops owned processes and preserves saved
  checkpoints. It does not save an additional checkpoint at the stopping instant.
- The script writes files; it does not schedule chat notifications. Only create
  those when the user requests them.
- `scripts/compare_run.py` is the separate existing 20-minute cross-architecture
  comparison workflow. `--eval-baseline` evaluates old architecture weights using
  its own encoder. Do not load those weights as a new-architecture model.

For runner changes, run a short GPU integration check with a duration not divisible
by its interval, small even evaluation counts, and small batches. Verify initial,
interval and final graphs, completed status, and refusal to reuse an output folder.
Use `scripts/plot_interval_run.py` to regenerate these graphs from recorded data.

## Mandatory post-run checks

Report wall-clock cost under **Training games**, **Model training**, and
**Evaluation games** after every run. Use `python scripts/summarize_timing.py RUN`
or the checker's `timing` section. Training games include policy inference and
rollout/GAE work; model training is PPO optimization. Report checkpoint/logging
overhead separately. Evaluation processes can overlap training and each other,
so do not add all three durations and call that total elapsed time. Historical
runs without timers must say "not recorded"; do not invent a phase allocation.

After every training run, run `python scripts/check_run.py RUN_DIRECTORY` and read
`issue-check.json` before claiming improvement or delivering the infographic.
`train_timed.py` now saves evaluation traces and invokes this checker automatically;
agents must still inspect its report. Re-run it after adding further evaluations.
For Model2 runs with `value_outcome_audit`, also run
`python scripts/summarize_value_audit.py RUN_DIRECTORY`: it reconstructs every
rolling counter from completed-game samples and reports first/full versus final
window winner accuracy with game-bootstrap uncertainty. Inspect denominators;
changing-league rolling curves alone cannot establish fixed-distribution gains.
Online-only runs are audited from `online_audit` snapshots in `metrics.jsonl`;
no separate evaluation or full replay export is needed. For explicitly requested
standalone evaluations, pass `--eval-traces` and check the evaluation directory.
Missing evaluation traces mean untested, never clean. Sampled online diagnostics
describe the learner's current self-play distribution, not held-out strength.

Check actual immunity events and visible type-immunity candidates, target collapse
when both opponents are alive and legally targetable, preview index collapse,
Double Shock without Electric typing, Steel Roller without terrain, and first-turn
moves after the first action. Inspect concrete losing-game examples. Also check
training completion, finite losses, and the tracked-type observation gap. Report
denominators, rates, evaluation mode, seeds and sample size. The report contains
explicit screening thresholds; low switching/Protect or low-accuracy choices are
descriptive signals, not proof of mistakes. Immunities following opponent switches
are not automatically avoidable. Never say "resolved" merely because win rate rose
or a finite sample happened to contain zero cases.

Use online target/preview counts for routine collapse monitoring. If fixed-opponent
evaluation is explicitly requested, use matched held-out setups for starting/final
checkpoints and separate sampled/greedy results. A continuation run changes both
training time and reward exposure; do not attribute its gains solely to rewards
without a separate unshaped control. Preserve all checkpoints and provenance.
Checker changes require `python scripts/test_check_run.py` and an audit of existing
archived traces or sampled training logs. Do not rerun evaluation games merely to
test online diagnostics. Runner changes also require the non-divisible duration
integration check above; online-only mode must produce graphs with no evaluators.
