# Model1 experimental: speed ranges and entity-based actions

Experimental descendant of [ppo-cfr-checkpoint](https://github.com/CalebBrody/ppo-cfr-checkpoint). The baseline architecture remains in that repository. This repository preserves the shared baseline commit and changes the observation/action architecture.

## Changes

- Uses Ninjask's existing `Tracker::speed_bounds(side, index)` directly. Each Pokemon receives a known flag, minimum, maximum, interval width, and exact-value flag. Numeric speeds are divided by 500. These are **unmodified Speed bounds**, not effective speed after weather, boosts, items, or Trick Room. Those battle conditions remain separate inputs. No new speed inference engine is added.
- Removes the roster-index and team-position one-hot inputs. The shared transformer has no positional embeddings and pools entities by averaging, so merely rearranging entity storage does not change the value prediction.
- Scores each action with contextual embeddings of the Pokemon it involves. Preview actions refer to four selected Pokemon in lead/bench roles. Turn actions refer to both active actors and their switch destinations or explicit move targets. Four integer references are used only for tensor lookup; their numeric values never enter a learned layer.
- Retains meaningful battlefield roles: your side versus their side, active position, target position, and lead versus bench roles. Reassigning those roles can change battle outcomes and should change predictions.
- Uses a 384-to-64 joint-action layer combining pooled state, action descriptors, and four entity embeddings. Legal-action masking and the sampling retry guard remain in place.

Changing team order requires remapping engine action references, which the observation adapter handles. With matched references, the policy scores corresponding actions equally within floating-point tolerance. This does not require the engine to abandon its internal indices.

## Checkpoint compatibility

This architecture needs **new training**. Baseline checkpoints do not contain the new action layer and must not be used as a resume checkpoint. The original 24-hour run remains stopped. Short smoke runs validate execution, not playing strength.

## Setup and commands (CMD)

Install Rust and use a Vulkan-capable GPU. The source dependencies are vendored; licenses and provenance are in [DEPENDENCIES.md](DEPENDENCIES.md). Team data and generated dex data are excluded from Git.

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

## Limitations

The model still uses compact hashed species, ability, item, and move features, with possible collisions. It does not prove full invariance to every change in simulator history or stochastic sampling. It preserves move feature ordering inside a Pokemon. Evaluation against strong opponents is still required before claiming an improvement over the baseline.
