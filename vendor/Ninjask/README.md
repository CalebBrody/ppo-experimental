# Ninjask

[![CI](https://github.com/dcabatin/Ninjask/actions/workflows/ci.yml/badge.svg)](https://github.com/dcabatin/Ninjask/actions/workflows/ci.yml)
[![PyPI](https://img.shields.io/pypi/v/ninjask)](https://pypi.org/project/ninjask/)
[![crates.io](https://img.shields.io/crates/v/ninjask)](https://crates.io/crates/ninjask)

**A Pokémon Champions VGC doubles simulator that matches
[Pokémon Showdown](https://github.com/smogon/pokemon-showdown) exactly, about
250 times faster.** Written in Rust, with Python bindings, for bots,
reinforcement learning, search and large-scale team analysis. Named after
Ninjask, a famously fast Pokémon.

```sh
pip install ninjask
```

## Why Ninjask

If you are building a VGC bot, training an agent with self-play, or asking a
question that takes millions of battles to answer, Showdown is the reference
but it is slow, and a simulator that is fast but subtly wrong teaches your
agent the wrong game. Ninjask is meant to be both:

- **Exact.** Ninjask is a port of Showdown's battle code at a pinned commit.
  In *reference mode* it uses Showdown's random number generator and makes
  the same draws in the same order, so a seeded game produces the same log,
  the same requests and the same state after every input. It matches
  Showdown on 1,000 of 1,000 seeded games of random legal teams and 1,000 of
  1,000 games of real tournament teams, and CI replays 200 against Showdown
  on every change (see [How parity is verified](#how-parity-is-verified)).
- **Fast.** About 7,000 random games (70,000 to 85,000 turns) per second on
  one CPU core with logging off, and 50,000 turns per second with the full
  protocol log on, against Showdown's 200 turns per second on the same
  machine and teams (`examples/bench.rs --log`, `refsim/bench_showdown.js`).
  A `Battle` is plain data: copying one for search costs microseconds.
- **Complete for Reg M-C.** All 511 moves and 166 items in the format, and
  221 of its 224 abilities (the other three belong only to Pokémon that are
  not legal in it). Every team of a 549-team pool of public tournament pastes
  is fully supported. Nothing is silently approximated: `Dex.team_problems`
  names anything a team uses that the engine does not simulate.
- **Built for agents.** Besides the battle itself:
  - a per-player **tracker** that builds what one side knows from the
    protocol, and reads Showdown's live logs the same way;
  - **worlds**: battles consistent with one player's view, with the foe's
    hidden sets filled in by your assumptions, for search under hidden
    information;
  - an exact **damage and turn-order calculator** (all 16 rolls, crits,
    KO chances, Speed with Trick Room, Tailwind and priority) that runs the
    battle's own damage code, so it can't drift from play;
  - open team sheets, and Showdown's request and choice formats throughout,
    so a bot written for Showdown carries over with little change.

What it is not: an agent, a search algorithm or a user interface. It is the
simulator underneath them, with the tools to prove it matches the reference.

## Quick start

### Python

```sh
pip install ninjask      # Python 3.12+, Linux, macOS and Windows; the Reg M-C formats are bundled
```

Play a game between two random policies:

```python
import ninjask
from ninjask.policy import RandomPolicy

team1, team2 = ninjask.verification_teams()[:2]   # bundled test teams, Showdown's packed format
result = ninjask.play_engine_game(
    "gen9championsvgc2026regmc", 1, team1, team2, RandomPolicy(1), RandomPolicy(2)
)
print(result.winner, result.turns)
```

A policy is any object with `choose(request, log) -> str`: it reads Showdown's
request JSON and answers in Showdown's choice syntax (`"move 1 1, move 2 2"`),
so code written for Showdown bots carries over. To drive a battle yourself:

```python
import json
from ninjask import Battle, dex
from ninjask.policy import RandomPolicy

d = dex("gen9championsvgc2026regmc")
assert d.team_problems(team1) == []  # [] when the engine fully supports the team

b = Battle(d, team1, team2, seed=1)
b.set_logging(True)
policy = RandomPolicy(seed=1)
while not b.ended and b.turn < 150:
    for side in ("p1", "p2"):
        if b.needs_choice(side):
            while True:
                request = json.loads(b.request(side))            # Showdown's request JSON
                try:
                    b.choose(side, policy.choose(request, []))   # e.g. "move 1 1, move 2 2"
                    break
                except ValueError:                               # rejected, as Showdown rejects it
                    pass                                         # (a hidden disable); the request now shows it
print(b.winner, b.turn)
print("\n".join(b.log("p1", 0)[:10]))                           # p1's view of the protocol log
```

`b.copy()` is a cheap, independent copy for search: play a line out without
touching `b`. `b.damage("p1a", move_id, "p2b")` runs the damage calculation
(all 16 rolls, crit chance, KO chances) for the Pokémon in those slots.

`Battle.reference(d, team1, team2, "1,2,3,4")` runs a battle in reference
mode, seeded like Showdown, so it reproduces a Showdown game draw for draw.

### Rust

```sh
cargo add ninjask
```

```rust
use std::{error::Error, sync::Arc};
use ninjask::{rng::Rng, Battle, Dex};

fn main() -> Result<(), Box<dyn Error>> {
    let dex = Arc::new(Dex::from_json(&std::fs::read_to_string("gen9championsvgc2026regmc.json")?)?);
    let pool = std::fs::read_to_string("teams.txt")?; // one packed team per line
    let teams: Vec<&str> = pool.lines().collect();

    let mut battle = Battle::new(dex, [teams[0], teams[1]], ["Alice", "Bob"], 1)?;
    let mut rng = Rng::new(1);
    while !battle.ended && battle.turn < 150 {
        for side in 0..2 {
            if battle.side_needs_choice(side) {
                let choice = battle.random_choice(side, &mut rng); // or any "move 1 1, switch 3"
                battle.choose(side, &choice)?;
            }
        }
    }
    println!("{:?} won after {} turns", battle.winner_name(), battle.turn);
    Ok(())
}
```

The full program is [`examples/quickstart.rs`](examples/quickstart.rs). The
crate reads a format from its JSON export; the Python package carries the
bundled ones (`ninjask/data/formats/`), and a checkout generates any other.

The [API overview](#api-overview) lists everything else; `cargo doc --open`
(or [docs.rs](https://docs.rs/ninjask)) documents the crate.

## Contributing and ideas

Contributions are welcome, and the most valuable ones are concrete:

- **Found a difference from Showdown?** That is a bug, whatever its size.
  Open a [bug report](https://github.com/dcabatin/Ninjask/issues/new?template=bug.yml)
  with the teams, the seed and the choices that reproduce it;
  `scripts/check_reference.py --verbose` prints both simulators' logs and
  random draws at the first divergence.
- **Have an idea, a use case or a question?** Open an
  [idea](https://github.com/dcabatin/Ninjask/issues/new?template=idea.yml).
  Knowing what you are building helps decide what comes next. Things we
  would like to see: new regulations as they launch, an API that enumerates
  a turn's chance outcomes exactly (for search and endgame solvers), faster
  batched bindings for training, and more languages.
- **Want to write code?** [`CONTRIBUTING.md`](CONTRIBUTING.md) covers the
  setup, the checks a change must pass and how to port a mechanic from
  Showdown.

## Scope and parity

- **Format.** Champions doubles under the Reg M-C rules: bring 6, pick 4,
  Mega Evolution, no Terastallization, the Champions stat-point system, and its
  changed status and damage mechanics. Other regulations (M-A, M-B, and later
  ones) are data changes: the rules, species, moves, items and abilities are
  loaded from a JSON export of Showdown's data, not compiled in.
- **Parity.** For the entities it supports, the engine matches one pinned
  Showdown commit (`refsim/SHOWDOWN_COMMIT`) exactly. Run in *reference mode*, it
  uses Showdown's own random number generator and makes the same draws in
  the same order, so a seeded game gives the same log lines and the same state
  after every input. The normal mode uses a faster generator
  (xoshiro256++); its outcome *distributions* are checked against Showdown's.
- **Honest coverage.** Every move, item and ability is *generic* (fully
  described by its data fields), *implemented* (ported from Showdown's code),
  or *unsupported*. The engine never silently simulates an effect it lacks:
  `Dex.team_problems(team)` lists what a team uses that is unsupported, and
  `scripts/report_support.py` reports which entities block the most teams.

### How parity is verified

Showdown at the pinned commit is the specification. Four checks compare the
engine with it; all need Node.js and the Showdown checkout (see
[Working from source](#working-from-source)).

| Check | What it compares |
| --- | --- |
| `scripts/check_reference.py` | Reference mode against Showdown, game by game. Both simulators get the same seed and the same choices. After every input it compares each log line, each side's request and the full state projection, the RNG state included. On a divergence it prints both sequences of RNG draws with their call sites. |
| `cargo run --release --example fingerprint` | A hash of the logs, requests and RNG draws of 2000 seeded engine games. It must not change in a refactor that preserves behaviour. |
| `scripts/verify_engine.py` | Outcome distributions. At sampled decision points the Showdown state is imported into the engine and one joint choice is played N times in each simulator. A χ² test of homogeneity per feature (HP, status, boosts, volatiles, fainting, field, the winner) plus an exact-support check, with false-discovery-rate control across all comparisons. `--focus ids` concentrates on named moves, abilities and items. |
| `scripts/check_tracker.py` | The protocol tracker (below) against the true state: a player's view of its own side matches exactly, and everything it believes about the foe is true. It runs on engine games, on Showdown games (`--showdown`) and with open team sheets (`--ots`). |

The reference and tracker checks run on a pool of real-looking teams and on
`--synthetic` teams: random legal teams built over every supported move,
ability and item, so entities no popular team uses are exercised too.

The tests (`cargo test`, `pytest`) cover units of the same machinery: the RNG
against Node's output, the choice syntax, format loading and the reference
harness on a few games.

## Working from source

The parity checks, exporting formats other than the bundled ones, and
changing the engine need a checkout and the pinned Showdown.

Requirements: Python 3.12 or later, Rust (stable), Node.js 22 or later, git.

```sh
git clone https://github.com/dcabatin/Ninjask.git
cd Ninjask
python3 -m venv .venv
source .venv/bin/activate

# The package, with its compiled module built from the crate (optimised)
pip install -e '.[test]'

# Pokémon Showdown at the pinned commit, cloned and built into third_party/
./scripts/setup_showdown.sh

# The Reg M-C format, exported from Showdown to data/derived/format/
python -c "import ninjask; ninjask.load('gen9championsvgc2026regmc')"

# Rust and Python tests
cargo test --release
pytest -q
```

`import ninjask` then works from anywhere in that environment. The install is
editable because the checks below drive Showdown through `refsim/` and
`scripts/`, which live in the checkout; after changing Rust code, rebuild with
`pip install -e .` again (or `maturin develop --release`). The Rust tests
that play battles read the format export and skip without it; the rest of
`cargo test --release` needs neither Python nor Showdown. The Python tests
that need Showdown skip themselves when it is not built.

The engine loads a format from a JSON export of Showdown's data,
`data/derived/format/<format>.json`, which `ninjask.dex(format)` and
`ninjask.load(format)` generate from the Showdown checkout on first use. One
exported format is derived rather than Showdown's own:
`gen9championsvgc2026regmcots` (PKMN Arena's open-team-sheets Reg M-C) is the
base format with its sheet rule forced open, built by `refsim/export_format.js`;
its `FormatSpec` reports `open_team_sheets` and the base format it derives from.

### Run the checks

```sh
# Exact parity with Showdown, on 1000 seeded games of synthetic teams
# (4 processes, each with its own Showdown)
python scripts/check_reference.py --synthetic --games 1000 --workers 4

# Outcome distributions, tracker, support report
python scripts/verify_engine.py --cases 100 --n 300
python scripts/check_tracker.py --games 100
python scripts/report_support.py --pool tests/fixtures/regmc_teams.txt

# Throughput (add --log for logging on), Showdown's for comparison, and the fingerprint
cargo run --release --example bench -- data/derived/format/gen9championsvgc2026regmc.json tests/fixtures/regmc_teams.txt 20000
node refsim/bench_showdown.js tests/fixtures/regmc_teams.txt 300
cargo run --release --example fingerprint -- data/derived/format/gen9championsvgc2026regmc.json tests/fixtures/regmc_teams.txt 2000
```

On the bundled fixture teams (`tests/fixtures/regmc_teams.txt`) the fingerprint is
`6e2f85907f97e4e9`. A change that preserves behaviour must leave it unchanged.

The bench plays random legal games on one thread with logging off: about
6,000 to 7,500 games/s on one core of a 4-core cloud VM, on the fixture teams
or on a pool of real tournament teams (about 735,000 instructions per game
under callgrind on the latter). Speed on shared hosts varies by 10% or
more between runs, so compare instruction counts (`valgrind --tool=callgrind`
on the bench, a release build with debug info) rather than games/s across
days.

`check_reference.py` and `check_tracker.py` play on a pool of real teams when
you have one (`--pool`, a file with one packed team per line; or the
`supported-*.txt` pools that `report_support.py` writes to
`data/derived/teams/`), else on the fixture teams, and on random legal teams
with `--synthetic`. `check_reference.py` also takes `--first N` (start at game
N), `--require ids` (player 1's team must use one of those moves, items or
abilities) and `--verbose`; run any script with `--help`.
`report_support.py` reads a pool of packed teams, ranks the moves, items and
abilities that keep the most teams unsupported, and writes the supported ones
as a pool to verify on.

## API overview

**Rust** (`ninjask` crate; `cargo doc --open`)

- `Dex::from_json(text)`: the loaded format. Immutable, shared between
  battles through an `Arc`. Looks up species, moves, items, abilities and the
  type chart, and reports support.
- `Battle::new(dex, [team1, team2], names, seed)`: a battle from two packed
  teams. `Battle::new_with_rng` takes a given `Rng` (reference mode uses Showdown's);
  `Battle::from_sets` takes parsed teams. For many games over one pool,
  prepare each team once (`battle::Team::new(&dex, &sets)`: its Pokémon and
  open team sheet) and build each battle with `Battle::from_teams`, which
  copies them and plays exactly as `from_sets` would;
  `Battle::from_projection` imports a state exported by the reference.
  `Battle` is plain data and `Clone`, so copying it for search is cheap.
- `battle.request_json(side)`, `side_needs_choice(side)`, `choose(side, "move 1 1, switch 3")`
  accept and produce Showdown's request and choice formats.
  `joint_choices`, `sample_joint_choice` and `random_choice` enumerate or draw legal choices.
- `battle.log_lines(viewer, from)`: the protocol log as one viewer sees it
  (the spectator, or either player, hiding what that player has not seen).
- `battle.projection()`: the full state as JSON, in the shape of the reference
  harness's projection.
- `Tracker::new(dex, side)`: builds one player's observation from the protocol
  lines and requests it is fed (`feed`, `feed_request`), or from the engine
  directly (`feed_engine`, for self-play). The same tracker reads Showdown's
  logs, so live play and simulation give the same observations. `to_json()`
  exports the view.
  Each Pokémon includes `speedBounds: {min, max}`: inclusive bounds on its
  **unmodified Champions Speed**, using the shown species, 0–32 Speed points
  and its known nature (or all natures). Own request stats give an exact value.
  Rust exposes the same data as `tracker.speed_bounds(side, index)`.
  Consecutive moves at equal priority tighten these bounds when the actors'
  revealed items and abilities have supported Speed/order effects;
  Trick Room reverses the comparison and ties remain possible.
  Known Speed stages, Choice Scarf, halving items, Tailwind and paralysis use
  the engine's rounding helpers. Inference is deliberately conservative:
  unknown items/abilities, weather/terrain, Transform, possible Illusion and
  queue-changing effects can prevent narrowing. Speed Swap exchanges the
  current bounds until switching out. Transform or an unknown species returns null.
  Bounds follow the shown species and are not an effective battle-Speed range.
  `sides[*].pokemon` contains known preview teammates; `seen` identifies Pokémon
  revealed in battle, `slot` identifies the current active slot, and
  `sides[*].knownPicks` lists indices of those revealed Pokémon. `teamSize`
  records the brought-team count; unrevealed opponent picks remain unknown.
- `world::World::from_view(&tracker, &assumptions) -> Result<Battle, _>`: a
  *world*, the battle one player's view describes. One's own side and the
  field are as the view holds them; the foe's hidden sets (item, ability,
  unrevealed moves, nature, stat points) come from a `SetAssumption` per
  Pokémon, or by default: nothing unrevealed, the sheet's or a neutral nature,
  and a placeholder spread rule (`world::placeholder_points`). Search over
  hidden information can run on worlds. `check_tracker.py --world`
  checks that a world built with the true sets matches the true battle.
- `calc`: exact damage and turn order, read-only. `battle.calc(attacker,
  move, target)` (or `damage_calc` on a copy) gives the 16 rolls of each hit
  without and with a critical hit, the crit chance, the spread reduction,
  the hit-count odds, accuracy, immunity, the expected damage and the
  chances to KO this turn and within two uses (no end-of-turn effects
  between them). It runs `getDamage`'s own two phases (`damage_base`,
  `damage_mods`) for every roll, so it can't drift from play.
  `effective_speed`, `order_odds(a, plan, b, plan)` (switches, priority,
  Quick Claw and the like, Speed, Trick Room, ties) and `speed_order`.
  `cargo run --release --example damage_check -- <dex> <teams> 5000` plays
  random games recording every hit with its predicted distribution: on the
  fixture teams, 0 of 99,176 damages fell outside it, and critical hits, KOs
  and rolls came as often as predicted.
- `rng::Rng`: xoshiro256++ from `Rng::new(seed)`, or Showdown's generator from `Rng::from_showdown_seed`.

**Python, compiled classes** (`ninjask.Dex`, `ninjask.Battle`, `ninjask.Tracker`;
built from `src/python.rs` as `ninjask._core` and re-exported by the package)

- `Dex.load(path)`, `.team_problems(packed)`, `.move_support(id)`,
  `.species_ids()`, `.generic_moves()`, `.implemented_moves()`,
  `.supported_abilities()`, `.supported_items()`.
- `Battle(dex, team1, team2, seed)`, `Battle.reference(...)`,
  `Battle.from_projection(...)`, `.request(side)`, `.needs_choice(side)`,
  `.choose(side, choice)` (raises `ValueError` on an illegal choice, as
  Showdown rejects it), `.log(viewer, start)`, `.log_len()`, `.set_logging(on)`,
  `.projection()`, `.copy()`, `.reseed(seed)`, `.ended`, `.winner`, `.turn`.
- `Tracker(dex, side)` with `.feed(lines)`, `.feed_request(json)`,
  `.feed_battle(battle)`, `.view()` and `.world(assumptions=None, seed=0)`
  (a `Battle`; assumptions as JSON, see `world::Assumptions::from_json`).
- `Battle.damage(attacker, move, target)` (slots like "p1a"; JSON),
  `.order(a, plan_a, b, plan_b)` (a plan is a move id, "switch" or
  "priority:N"; returns `(first, tie)`) and `.speeds()`.

**Python, the rest of the package** (`python/ninjask/`)

- `format`: `load(format_id)` returns a `FormatSpec` (species, moves, learnsets,
  legal species) from the Showdown export.
- `__init__`: `dex`, `play_engine_game`, and generators of random supported
  teams (`supported_test_team`).
- `refsim`: a client for the Showdown reference server (`RefSim`, `Battle`).
- `choices`, `policy`, `play`, `verify`, `tracker_check`: legal-choice
  enumeration, the policy interface, game loops for either simulator, and the
  distribution and tracker checks.
- `paths`: where Showdown and generated data live. Defaults are inside the
  checkout (`third_party/`, `data/derived/`); override with `NINJASK_HOME`,
  `NINJASK_SHOWDOWN` and `NINJASK_DATA`. `NINJASK_POOL` sets the default team
  pool of the checks.

## Repository layout

```
Cargo.toml, pyproject.toml   crate and Python package (built with maturin)
src/                         the engine
  dex/                         format data, team validation, support reporting
  state/                       battle state: field, sides, Pokémon, the move in use
  battle/                      construction, the action queue, choices and requests, the log
  moves/                       move execution: targeting, hits, damage, multi-hit, two-turn moves
  events/                      Showdown's event hooks: modifiers, switch-in, residual, immunity, ...
  abilities/ items/ conditions/  ported handlers; weather, terrain, status, volatiles, side conditions
  kinds/                       the ids of every special-cased move, item and ability
  projection/                  state export and import, in the reference harness's format
  tracker.rs                   per-player observation built from protocol lines
  rng.rs                       xoshiro256++, and Showdown's ChaCha20 generator
  python.rs                    PyO3 bindings
examples/                    bench (throughput), fingerprint (behaviour hash), damage_check
python/ninjask/              the Python package; the compiled module lands here as _core
refsim/                      Node.js driver for the pinned Showdown: server, format export
scripts/                     the checks above, and setup_showdown.sh
tests/python/, tests/fixtures/   pytest files, a small team pool
```

Each ported handler cites the Showdown function it follows. To add an entity,
port it from the pinned commit, then run `check_reference.py --require <id>`
and `verify_engine.py --focus <id>`: a port is done when they pass.

## Embedding in another extension

A Rust extension of your own whose functions take Ninjask's objects (say, an
observation encoder that reads a `Battle` and a `Tracker`) must be compiled
into the same module as Ninjask's classes: PyO3 only accepts an object of the
class compiled into the module that receives it, so a second copy of `Battle`
would be incompatible. Build one module holding both, and tell the `ninjask`
package to take its classes from it:

1. Depend on the crate with the feature `python` (not `python-module`):
   `ninjask = { git = "https://github.com/dcabatin/Ninjask", features = ["python"] }`.
2. In your `#[pymodule]`, call `ninjask::python::register(m)?`, then add your
   own classes. Their methods take `&ninjask::python::PyBattle`,
   `&PyTracker` and `&PyDex`, whose `inner` fields are the Rust objects.
3. Declare your module under the `ninjask.extension` entry-point group in
   your `pyproject.toml`:

   ```toml
   [project.entry-points."ninjask.extension"]
   myproject = "my_module"
   ```

`import ninjask` then re-exports `my_module.Dex`, `.Battle` and `.Tracker`
(`ninjask/_extension.py`), so `ninjask.Battle is my_module.Battle`, and
everything in the package, the checks included, uses them. `ninjask._core` is
used only when no module is declared, so the package's Python files are all
you need besides your own module (for example, `python/` on your project's
path).

## Documentation

In [`docs/`](docs/):

- [`design.md`](docs/design.md): requirements, architecture, how it is verified,
  and the findings of reference mode (every non-obvious Showdown behaviour the
  engine reproduces).
- [`adding-a-mechanic.md`](docs/adding-a-mechanic.md): how to add a move,
  ability or item, and how to verify it.
- [`CHANGELOG.md`](CHANGELOG.md): what changed in each release.
- [`CONTRIBUTING.md`](CONTRIBUTING.md): how to report a bug, propose an idea or send a change.

## License

MIT; see [LICENSE](LICENSE). The engine ports Pokémon Showdown's logic and
data, which is also MIT-licensed; see [NOTICE](NOTICE).

## Disclaimer

Ninjask is an unofficial fan project. It is not affiliated with, endorsed,
sponsored or approved by Nintendo, Game Freak, Creatures or The Pokémon
Company, nor by Pokémon Showdown. Pokémon and Pokémon character names are
trademarks of their respective owners, used here only to identify the game
mechanics the engine simulates. No official artwork, sprites, sounds or other
game assets are included.


## Enumerating chance outcomes

`Battle::enumerate_outcomes(&choice_p1, &choice_p2, mode)` accepts the existing
structured side choices (`&[SlotChoice]`) and returns
`Result<Vec<Outcome>, EnumerationError>`. Each outcome contains `probability`
(`f64`) and a cloned `battle`. Python uses the existing choice-string syntax:

```python
from ninjask import EnumerationMode

outcomes = battle.enumerate_outcomes(
    "move 1 1, move 2 2", "move 1 1, move 1 2", EnumerationMode.Precise
)
expected_value = sum(o.probability * evaluate(o.battle) for o in outcomes)
```

Enumeration preserves the input and any already-submitted choices. Supply an
empty choice (`&[]` in Rust, `""` in Python) for a side that does not need one.
Execution stops at the next player decision request or battle end, including
mid-turn pivot/replacement requests; the returned battles preserve the remaining
queue. Team preview is not supported by this API. Invalid choices return an
error (`ValueError` in Python).

`Precise` exhaustively enumerates the integer outcomes of the existing 32-bit
scaling rule, accumulating probabilities in floating point. `AlmostPrecise`
enumerates only the minimum and maximum damage-variance rolls, each with weight
1/2, including confusion damage and each hit of a multi-hit move. All other
chance events remain exhaustive. These endpoint weights approximate the damage
distribution; they are not the actual probability of rolling either endpoint.

Execution replays explicit decision paths on fresh clones and never calls a
PRNG. Equivalent completed states are merged and their weights summed. The
number of paths can grow exponentially; there is no sampling, pruning or silent
truncation. Returned battles retain the input RNG snapshot for subsequent
ordinary play; enumeration itself is independent of that snapshot.

`battle.state_key()` creates an immutable, hashable `BattleStateKey` containing
full future-relevant simulator state, including pending choices and queued
actions. Keys exclude logs, RNG state and diagnostics, and are scoped to the
same shared Dex allocation (independently loaded dexes stay distinct). Merged
outcomes retain the first representative battle's logs and diagnostics, not a
history of every contributing path. These keys are not player information-set
keys for imperfect-information training. Python `Outcome.battle` returns an
independent copy, so modifying it cannot change the stored outcome.


Compare enumeration modes with `cargo run --release --example bench_outcomes`.
It uses the self-contained enumeration test fixture: no damage, one hit, a
two-hit move, and both players attacking. Setup is excluded, logging is off,
and timings are medians of seven alternating batches. Output includes merged
state counts. These synthetic singles scenarios isolate damage branching;
measure your training positions separately before projecting throughput.
