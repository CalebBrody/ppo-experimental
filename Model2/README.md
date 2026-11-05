# Model2

Separate training mode, implemented in `../src/model2.rs`; Model1 PPO remains the
default. Run with the existing supervisor:

```powershell
python scripts/train_timed.py --trainer model2 --duration 20m --interval 5m --league-interval 5m --envs 4 --steps 8 --batch 16 --epochs 2 --audit-every 4 --audit-window 1024 --games 0 --out Model2/runs/matrix-history-20m-20261103
```

Run that command from `Model1-experimental`. Model2 starts with fresh weights.
Its network has 145,858 parameters, including direct candidate move-type flags.
Original 143,298-parameter experimental checkpoints are preserved and incompatible.

For a battle move request, select up to eight joint actions per side: six highest
network scores plus two random remaining choices. A public-view `World` supplies
the simulator state. Unknown opponent sets are sampled by species from supported
training-team sets; revealed information overrides these assumptions. Actual
opponent private sets are never passed to search. This first version uses one
sampled hidden world per decision and two chance samples per action pair.

Each matrix entry is immediate HP-progress/terminal reward plus a batched next-state
value estimate. Bootstrap values and matrix entries are bounded to [-2,2], the
remaining-return range of the current shaped reward. Run 256 synchronous regret
matching iterations. Train the policy on the average row strategy using cross
entropy over the candidate set; train state value on the solved matrix expectation
using half squared error. Actual learner actions are sampled from the solved
strategy. This is search-assisted collection, not PPO and not full-game CFR.

Preview and forced replacements use the network without search and are excluded
from these supervised targets. Opponent candidates use a conservative public
perspective with our private moves, item, ability and stats removed. World state
reconstruction has documented approximations (unknown HP/spreads/condition lengths).
The World builder does not retain Protect's consecutive-use counters or the
number of move attempts since entry. Model2 restores those from player-visible
protocol history before searching; tests check repeated Protect against the engine.
The preliminary `matrix-20m-20261103` run was stopped after finding this gap and
is not the completed comparison experiment. The corrected run starts fresh.
Candidates outside the selected eight are unexplored in that matrix. Conditional
policy distillation, bootstrapped values, and sampled hidden sets are approximations;
finite-iteration duality gaps measure the local matrix, not whole-game exploitability.

Actual games use the same sampled frozen opponent league as Model1, with five-minute
snapshots. Search simulations are not counted as played games or learner decisions.
Frozen Model2 opponents now use the same matrix search, powered by their frozen
weights and their own Tracker. Both choices are computed before either is submitted,
with independent search RNG streams. Budget/candidate sampling/assumptions/regret
iterations are identical. Preview and forced replacements use neural policies on
both sides. `--matrix-opponents false` reproduces the earlier asymmetric collection.
Metrics report opponent searched positions, simulations and fallback move requests.
Collection timing includes matrix search; metrics separately count simulated cells
and solved positions. CPU collection workers are not used by the Model2 loop.
`mistake-examples.jsonl` retains the rolling last 128 immunity/prerequisite witnesses,
including matrix, teacher distribution, pre-choice public state, events and eventual
game outcomes. It is saved at checkpoints; unfinished games have null outcomes.
Earlier runs retained the first 128 witnesses without eventual outcomes.

Inspect `issue-check.json` after each run. Online counters describe search-assisted
training, not standalone policy strength. Comparing to historical Model1 changes
both action encoding and training algorithm; no encoder-only causal conclusion.

`value_outcome_audit` in each metrics row tracks four rolling winner-accuracy
series: all value/HP sign disagreements; HP behind with positive value; HP ahead
with negative value; and strong disagreements (absolute team HP fraction
difference >= 0.1, absolute raw value >= 0.3). Samples use the same audit interval,
per environment, on Move requests. Pre-choice Tracker-only neural values are
labelled after a decisive game finishes; draws/cutoffs are excluded. The last
`audit_window` completed sampled positions form the shared window. Each group
logs its numerator and denominator; no samples means null, never zero accuracy.
`value-outcome-samples.jsonl` preserves completed-game raw samples for reanalysis.
The progress infographic adds one panel with all four lines. These describe the
changing training league, not a fixed-distribution test of value improvement.
Inspect denominators and `measured_seconds` (extra neural inference time) after
each run. In-flight predictions retain the value from when they were chosen.

## Completed-return value training

Value training now waits for genuinely completed games. For each searched Move
state, retain the search expectation and the cumulative reward before its action.
At completion, remaining return is final cumulative reward minus that prefix,
including intervening replacement rewards and the terminal bonus. The target is
50% search expectation plus 50% remaining return (`--value-return-weight`, default
0.5). Artificial turn cutoffs are discarded; genuine completed draws are retained.

A FIFO buffer retains 16,384 completed positions (`--value-replay-capacity`). Each
value minibatch samples half from positions completed in this collection update
and half uniformly from the buffer, with replacement. If no new game completed,
all samples come from the buffer. Until the first completion, value loss is zero.
Policy distillation still uses only current search examples. Replay updates the
shared state encoder and value head, without replaying old policy targets.
Metrics expose blend weight, replay size/capacity, fresh positions, and sampled
value-training positions. Replay and in-flight trajectories are memory-only and
start empty on resume, as does Adam; architecture and weight format are unchanged.

## Fixed-checkpoint head-to-head

`--evaluate N --eval-opponent WEIGHTS --eval-matrix --eval-search-teams TRAIN_TEAMS`
uses the same Model2 search for both fixed checkpoints. Supply held-out `--teams`,
an even game count, and `--eval-traces`. Adjacent games use the same team pair
and battle seed with learner sides swapped. Each role has an independent search
RNG. Both actions are selected before either is applied. Preview/replacement and
search fallbacks use sampled neural policies. Greedy evaluation is incompatible
with this search mode. The hidden-set catalog must be explicitly supplied from
training teams, not held-out teams. Reports distinguish game counts from matrix
simulations and record request, search, and fallback counts for both models.

Matrix evaluation also supports random legal opponents (omit `--eval-opponent`)
and `--eval-hp-opponent`. The learner retains full Model2 search. The HP opponent
uses the same public-world reconstruction and search budget, uniformly samples
up to eight candidates for each side, and scores each resulting world by twice
`hp_advantage` (mean remaining HP-fraction difference). It uses no neural scores,
bootstrap or terminal bonus. Preview, replacement and fallback actions are random.
This is a pure HP heuristic baseline, not a controlled value-head-only ablation,
since candidate proposals also differ from the trained model.

### Conservative speed-bound matrix search

Search evaluates Cartesian combinations of tracked lower/upper base Speed bounds for matched living opponents (including reserves), rather than trusting a single catalog spread's Speed. Engine modifiers and priority still apply. Chance outcomes are averaged within a scenario; each action pair then takes the minimum total return across scenarios before regret matching. Exact bounds produce one value. HP-only search uses the same rule with its HP objective. Traces record speed scenarios and the minimizing scenario per cell; simulation counts include every scenario. See `../techDebt.txt` for endpoint, consistency, compute and pessimism limitations. No neural feature layout or weights change.

### Probability-weighted chance outcomes
Current matrix search calls `Battle::enumerate_outcomes(..., EnumerationMode::Precise)` per action pair and speed scenario. It sums probability times immediate reward plus the bounded neural leaf value (zero leaf at terminal states), then applies the conservative worst-speed reduction. It checks probability mass and evaluates neural leaves in batches of at most 64. No chance sampling fallback is used. Legacy `--matrix-samples` is accepted but ignored; metadata records the enumeration method. `search_simulations` counts merged enumerated successors. Precise enumeration can be substantially slower.

Current enumeration mode is **AlmostPrecise** (user-directed): non-damage chance events retain their probabilities; damage endpoints each receive 1/2. Thus matrix entries are expected returns under this damage approximation. This supersedes the Precise mode noted above.

Current search mode is **PreciceButSame**: one shared damage endpoint for all attacks in each enumeration call, weighted 1/2 minimum and 1/2 maximum. Other chance branches retain their probabilities. This intentionally correlates damage rolls, reducing four damage-roll branches from sixteen paths to two; it does not reduce all chance events to two outcomes. Model2 now consumes the adjacent local Ninjask source; timed run bundles freeze it under their own build/vendor/Ninjask.


Chance search now caps completed paths at `--matrix-max-scenarios` (default 16)
per action pair and speed scenario. Both neural and HP search divide raw outcome
probabilities by their explored sum before computing expected payoffs. Traces
retain raw `outcome_probability_mass` and the cap. `chance_search_cutoff` log
lines record turn, side, matrix cell, cap and total explored probability whenever
mass is incomplete. The cap counts paths before identical successors merge.

Neural search now packs nonterminal leaf states across matrix cells into batches
of at most 64 before value inference. It preserves each cell's original weighted
reduction order, probability normalization, candidates and regret matching.
HP-only search and terminal leaves make no value calls. Traces record
`leaf_value_states`, `leaf_value_calls` and `leaf_batch_size`. Encoded inputs are
held on CPU for the matrix; GPU inference batches remain bounded at 64.
This applies to future builds; existing frozen run executables stay unchanged.

For a mixed-method evaluation, add `--eval-hp-sampled` to `--eval-matrix
--eval-hp-opponent`. Only the HP opponent uses `--matrix-samples` (default 2);
the neural learner retains capped, probability-normalized enumeration and leaf
batching. Evaluation metadata identifies the HP chance method and the different
simulation counting units for each side.
