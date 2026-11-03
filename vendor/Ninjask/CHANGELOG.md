# Changelog

## Unreleased

- Add Rust and Python outcome enumeration for search and regret minimization,
  with exhaustive chance probabilities or a damage-endpoint approximation,
  and hashable full battle-state keys.

## 0.2.0 (2026-10-29)

Behaviour is unchanged: every seeded game gives the same logs, requests and
random draws as 0.1.0 (the fingerprint is still `6e2f85907f97e4e9`), except
for the one line noted under Changed.

### Faster

- About 21% fewer instructions per random game on a pool of real Reg M-C
  teams: roughly 7,000 random games per second on one core with logging off,
  and about 250 times Showdown's speed with logging on in both. The turn loop
  allocates far less (short handler and target lists are inline `SmallVec`s,
  a new dependency), event sorts and `eachEvent('Update')` skip work when no
  handler can act or draw, choice generation hoists per-Pokémon checks, and
  the release profile uses fat LTO.
- `ninjask.load()` parses each format export once per process (keyed on the
  file and its modification time), as `dex()` already did. The spec is
  shared, so treat it as read-only.

### Changed

- The open-team-sheets format `gen9championsvgc2026regmcots` reports the
  base format's name, as PKMN Arena and Showdown do, so its `|tier|` line no
  longer ends in "OTS". Its id and forced-open sheets are unchanged.
- LICENSE: the copyright holder's name is corrected to David Cabatingan
  (0.1.0's packages carry a misspelling).

### Added

- `Dex::mega_forme` and `EventHandlers::has_event`. No public type changed.
- `examples/quickstart.rs`, and `--log` for `examples/bench.rs`.
- `refsim/bench_showdown.js`: Showdown's own throughput on the same random
  games, for comparison.
- Documentation: the README reorganised around why to use Ninjask, how, and
  how to contribute; crate-level docs for docs.rs; CONTRIBUTING.md; issue and
  pull request templates.

## 0.1.0

First public release.

- Pokémon Champions VGC doubles (Reg M-C) engine in Rust, matching Pokémon
  Showdown at the pinned commit (`refsim/SHOWDOWN_COMMIT`) log line for log
  line in reference mode, on every supported move, item and ability.
- Python bindings (`ninjask`): `Dex`, `Battle`, `Tracker`, worlds for
  search, the damage and turn-order calculation, and the parity checks.
- The Python package bundles the format exports for Reg M-C (closed and open
  team sheets), Reg M-B and the Champions doubles custom game.
