//! Python bindings (feature `python`): the `Dex`, `Battle` and `Tracker` classes.
//!
//! `register` adds them to a Python module. With the feature `python-module`
//! this crate is itself the `ninjask._core` extension module (built with
//! maturin from `pyproject.toml`), which the `ninjask` Python package
//! re-exports. A crate that adds classes of its own taking these (say, an
//! observation encoder that reads a `Battle`) depends on this one with the
//! feature `python` only, calls `register` from its own `#[pymodule]`, and
//! declares that module under the `ninjask.extension` entry-point group so
//! that the `ninjask` package uses its classes: one `Battle` class per
//! process, not two incompatible ones (README, "Embedding in another
//! extension"). The classes report `ninjask` as their module, which is where
//! the package exposes them either way.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::battle::Battle;
use crate::dex::support;
use crate::dex::Dex;

/// Damage enumeration policy. Other chance events are exhaustive in both modes.
#[pyclass(name = "EnumerationMode", module = "ninjask", eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyEnumerationMode {
    Precise,
    AlmostPrecise,
}
impl From<PyEnumerationMode> for crate::EnumerationMode {
    fn from(m: PyEnumerationMode) -> Self {
        match m {
            PyEnumerationMode::Precise => Self::Precise,
            PyEnumerationMode::AlmostPrecise => Self::AlmostPrecise,
        }
    }
}

#[pyclass(name = "Outcome", module = "ninjask", frozen)]
pub struct PyOutcome {
    inner: crate::Outcome,
}
#[pymethods]
impl PyOutcome {
    #[getter]
    fn probability(&self) -> f64 {
        self.inner.probability
    }
    #[getter]
    fn battle(&self) -> PyBattle {
        PyBattle {
            inner: self.inner.battle.clone(),
        }
    }
}

#[pyclass(name = "BattleStateKey", module = "ninjask", frozen, eq)]
#[derive(Clone, PartialEq, Eq)]
pub struct PyBattleStateKey {
    inner: crate::BattleStateKey,
}
#[pymethods]
impl PyBattleStateKey {
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut h);
        h.finish()
    }
}

/// "p1" or "p2" as a side index.
pub fn side_index(side: &str) -> PyResult<usize> {
    match side {
        "p1" => Ok(0),
        "p2" => Ok(1),
        _ => Err(PyValueError::new_err(format!("bad side {side}"))),
    }
}

#[pyclass(name = "Dex", module = "ninjask", frozen)]
#[derive(Clone)]
pub struct PyDex {
    pub inner: Arc<Dex>,
}

#[pymethods]
impl PyDex {
    /// Load an export written by refsim/export_format.js.
    #[staticmethod]
    fn load(path: &str) -> PyResult<PyDex> {
        let text =
            std::fs::read_to_string(path).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let dex = Dex::from_json(&text).map_err(PyValueError::new_err)?;
        Ok(PyDex {
            inner: Arc::new(dex),
        })
    }

    #[getter]
    fn format_id(&self) -> String {
        self.inner.format.id.clone()
    }

    /// Reasons a packed team is not fully simulated (empty when it is).
    fn team_problems(&self, packed: &str) -> PyResult<Vec<String>> {
        support::team_problems(&self.inner, packed).map_err(PyValueError::new_err)
    }

    /// Support level of a move by id: "generic", "implemented" or "unsupported (...)".
    fn move_support(&self, move_id: &str) -> PyResult<String> {
        let id = self
            .inner
            .move_id(move_id)
            .ok_or_else(|| PyValueError::new_err("unknown move"))?;
        Ok(support::move_support(&self.inner, self.inner.mv(id)).to_string())
    }

    /// Ids of all species in this format's data.
    fn species_ids(&self) -> Vec<String> {
        self.inner.species.iter().map(|s| s.id.clone()).collect()
    }

    /// Ids of all moves in this format's data, in dex order.
    fn move_ids(&self) -> Vec<String> {
        self.inner.moves.iter().map(|m| m.id.clone()).collect()
    }

    /// Ids of all moves the engine simulates generically.
    fn generic_moves(&self) -> Vec<String> {
        self.inner
            .moves
            .iter()
            .filter(|m| {
                support::move_support(&self.inner, m) == crate::dex::types::Support::Generic
            })
            .map(|m| m.id.clone())
            .collect()
    }

    /// Ids of moves with hand-ported handlers.
    fn implemented_moves(&self) -> Vec<String> {
        self.inner
            .moves
            .iter()
            .filter(|m| {
                support::move_support(&self.inner, m) == crate::dex::types::Support::Implemented
            })
            .map(|m| m.id.clone())
            .collect()
    }

    /// Ids of abilities the engine simulates (ported, or No Ability).
    fn supported_abilities(&self) -> Vec<String> {
        (0..self.inner.abilities.len())
            .filter(|&i| {
                !matches!(
                    support::ability_support(&self.inner, i as u16),
                    crate::dex::types::Support::Unsupported(_)
                )
            })
            .map(|i| self.inner.abilities[i].id.clone())
            .collect()
    }

    /// Ids of items the engine simulates.
    fn supported_items(&self) -> Vec<String> {
        (0..self.inner.items.len())
            .filter(|&i| {
                !matches!(
                    support::item_support(&self.inner, Some(i as u16)),
                    crate::dex::types::Support::Unsupported(_)
                )
            })
            .map(|i| self.inner.items[i].id.clone())
            .collect()
    }
}

#[pyclass(name = "Battle", module = "ninjask")]
pub struct PyBattle {
    pub inner: Battle,
}

#[pymethods]
impl PyBattle {
    #[new]
    #[pyo3(signature = (dex, team1, team2, seed, names=None))]
    fn new(
        dex: &PyDex,
        team1: &str,
        team2: &str,
        seed: u64,
        names: Option<(String, String)>,
    ) -> PyResult<PyBattle> {
        let (n1, n2) = names.unwrap_or(("Player 1".into(), "Player 2".into()));
        let inner = Battle::new(dex.inner.clone(), [team1, team2], [&n1, &n2], seed)
            .map_err(PyValueError::new_err)?;
        Ok(PyBattle { inner })
    }

    /// Reference mode: a battle whose RNG is Showdown's, seeded like Showdown's
    /// ("sodium,<hex>"), so it draws the same numbers.
    #[staticmethod]
    #[pyo3(signature = (dex, team1, team2, seed, names=None))]
    fn reference(
        dex: &PyDex,
        team1: &str,
        team2: &str,
        seed: &str,
        names: Option<(String, String)>,
    ) -> PyResult<PyBattle> {
        let (n1, n2) = names.unwrap_or(("Player 1".into(), "Player 2".into()));
        let rng = crate::rng::Rng::from_showdown_seed(seed).map_err(PyValueError::new_err)?;
        let inner = Battle::new_with_rng(dex.inner.clone(), [team1, team2], [&n1, &n2], rng)
            .map_err(PyValueError::new_err)?;
        Ok(PyBattle { inner })
    }

    /// Record every RNG draw with its call site (see `rng_trace`).
    fn set_rng_trace(&mut self, on: bool) {
        self.inner.rng.trace = if on { Some(Vec::new()) } else { None };
    }

    /// The draws recorded since the last call.
    fn rng_trace(&mut self) -> Vec<String> {
        self.inner
            .rng
            .trace
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The RNG state as Showdown prints it (reference mode), else None.
    #[getter]
    fn rng_seed(&self) -> Option<String> {
        self.inner.rng.showdown_seed()
    }

    /// Continue from a Showdown state projection (JSON text) with a new seed.
    #[staticmethod]
    #[pyo3(signature = (dex, projection, seed, names=None))]
    fn from_projection(
        dex: &PyDex,
        projection: &str,
        seed: u64,
        names: Option<(String, String)>,
    ) -> PyResult<PyBattle> {
        let (n1, n2) = names.unwrap_or(("Player 1".into(), "Player 2".into()));
        let v: serde_json::Value =
            serde_json::from_str(projection).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let inner = Battle::from_projection(dex.inner.clone(), &v, [&n1, &n2], seed)
            .map_err(PyValueError::new_err)?;
        Ok(PyBattle { inner })
    }

    /// The side's current request as Showdown JSON text, or None.
    fn request(&self, side: &str) -> PyResult<Option<String>> {
        Ok(self
            .inner
            .request_json(side_index(side)?)
            .map(|v| v.to_string()))
    }

    /// Whether the side has to answer a request now.
    fn needs_choice(&self, side: &str) -> PyResult<bool> {
        Ok(self.inner.side_needs_choice(side_index(side)?))
    }

    /// Submit a choice; raises ValueError with Showdown's message if invalid.
    fn choose(&mut self, side: &str, choice: &str) -> PyResult<()> {
        self.inner
            .choose(side_index(side)?, choice)
            .map_err(PyValueError::new_err)
    }

    /// Enumerate to the next request. Empty strings preserve already-submitted choices.
    #[pyo3(signature = (choice_p1, choice_p2, mode=PyEnumerationMode::Precise))]
    fn enumerate_outcomes(
        &self,
        py: Python<'_>,
        choice_p1: &str,
        choice_p2: &str,
        mode: PyEnumerationMode,
    ) -> PyResult<Vec<PyOutcome>> {
        py.allow_threads(|| {
            self.inner
                .enumerate_outcomes_text(choice_p1, choice_p2, mode.into())
        })
        .map(|outcomes| {
            outcomes
                .into_iter()
                .map(|inner| PyOutcome { inner })
                .collect()
        })
        .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn state_key(&self) -> PyBattleStateKey {
        PyBattleStateKey {
            inner: self.inner.state_key(),
        }
    }

    fn log(&self, viewer: &str, start: usize) -> Vec<String> {
        self.inner.log_for(viewer, start)
    }

    fn log_len(&self) -> usize {
        self.inner.log.len()
    }

    fn set_logging(&mut self, on: bool) {
        self.inner.log_enabled = on;
    }

    /// Open team sheets: log both teams' sets, as Showdown does at team preview.
    fn show_team_sheets(&mut self) {
        self.inner.show_team_sheets();
    }

    fn projection(&self) -> String {
        self.inner.projection().to_string()
    }

    fn copy(&self) -> PyBattle {
        PyBattle {
            inner: self.inner.clone(),
        }
    }

    /// Reseed the engine's RNG (for drawing independent outcomes from a copy).
    fn reseed(&mut self, seed: u64) {
        self.inner.rng = crate::rng::Rng::new(seed);
    }

    /// The read-only damage calculation (`ninjask::calc`) for the move
    /// `move_id` of the Pokémon in slot `attacker` ("p1a") against the one in
    /// `target` ("p2b"), as JSON: immune, accuracy, crit chance, spread,
    /// hit counts, the 16 rolls of each hit without and with a crit, the
    /// expected damage, and the chances to KO this turn and within two uses.
    fn damage(&self, attacker: &str, move_id: &str, target: &str) -> PyResult<String> {
        let (a, t) = (self.slot_ref(attacker)?, self.slot_ref(target)?);
        let id = self
            .inner
            .dex
            .move_id(move_id)
            .ok_or_else(|| PyValueError::new_err(format!("unknown move {move_id}")))?;
        let c = self.inner.damage_calc(a, id, t);
        let hits: Vec<serde_json::Value> = c
            .hits
            .iter()
            .map(|h| serde_json::json!({"normal": h.normal, "crit": h.crit}))
            .collect();
        Ok(serde_json::json!({
            "damaging": c.damaging, "immune": c.immune, "accuracy": c.accuracy,
            "critChance": c.crit_chance, "spread": c.spread, "hitCounts": c.hit_counts,
            "hits": hits, "fixed": c.fixed, "targetHp": c.target_hp, "targetMaxhp": c.target_maxhp,
            "expected": c.expected, "ko": c.ko, "ko2": c.ko2,
        })
        .to_string())
    }

    /// The chance that the Pokémon in slot `a` doing `plan_a` acts before
    /// the one in slot `b` doing `plan_b`, and that a speed tie decides it:
    /// `(first, tie)`. A plan is a move id, "switch", or "priority:N" (a
    /// damaging move of priority N).
    fn order(&self, a: &str, plan_a: &str, b: &str, plan_b: &str) -> PyResult<(f64, f64)> {
        let (ra, rb) = (self.slot_ref(a)?, self.slot_ref(b)?);
        let o = self
            .inner
            .order_odds(ra, self.plan(plan_a)?, rb, self.plan(plan_b)?);
        Ok((o.first, o.tie))
    }

    /// Each active Pokémon's effective Speed: `{"p1a": speed, ...}` as JSON.
    fn speeds(&self) -> String {
        let mut m = serde_json::Map::new();
        for r in self.inner.all_active() {
            m.insert(
                slot_name(r),
                serde_json::json!(self.inner.effective_speed(r)),
            );
        }
        serde_json::Value::Object(m).to_string()
    }

    #[getter]
    fn ended(&self) -> bool {
        self.inner.ended
    }

    #[getter]
    fn winner(&self) -> Option<String> {
        self.inner.winner_name().map(str::to_string)
    }

    /// The winning side ("p1" or "p2"), or None for a tie or a battle
    /// still running. Unlike `winner`, it doesn't depend on player names.
    #[getter]
    fn winner_side(&self) -> Option<&'static str> {
        self.inner.winner.map(|s| if s == 0 { "p1" } else { "p2" })
    }

    #[getter]
    fn turn(&self) -> u32 {
        self.inner.turn
    }
}

impl PyBattle {
    /// The active Pokémon in a slot named like "p1a".
    fn slot_ref(&self, slot: &str) -> PyResult<crate::battle::MonRef> {
        let b = slot.as_bytes();
        let s = side_index(slot.get(..2).unwrap_or(""))?;
        let k = match b.get(2) {
            Some(b'a') => 0,
            Some(b'b') => 1,
            _ => return Err(PyValueError::new_err(format!("bad slot {slot}"))),
        };
        self.inner
            .active(s, k)
            .ok_or_else(|| PyValueError::new_err(format!("no Pokémon in {slot}")))
    }

    fn plan(&self, plan: &str) -> PyResult<crate::calc::Plan> {
        if plan == "switch" {
            return Ok(crate::calc::Plan::Switch);
        }
        if let Some(p) = plan.strip_prefix("priority:") {
            let p: i8 = p
                .parse()
                .map_err(|_| PyValueError::new_err(format!("bad plan {plan}")))?;
            return Ok(crate::calc::Plan::Priority(p));
        }
        self.inner
            .dex
            .move_id(plan)
            .map(crate::calc::Plan::Move)
            .ok_or_else(|| PyValueError::new_err(format!("unknown move {plan}")))
    }
}

fn slot_name(r: crate::battle::MonRef) -> String {
    format!("p{}{}", r.0 + 1, ["a", "b"][r.1.min(1)])
}

/// What one player knows, built from its protocol lines and requests.
#[pyclass(name = "Tracker", module = "ninjask")]
pub struct PyTracker {
    pub inner: crate::tracker::Tracker,
}

#[pymethods]
impl PyTracker {
    #[new]
    fn new(dex: &PyDex, side: &str) -> PyResult<PyTracker> {
        Ok(PyTracker {
            inner: crate::tracker::Tracker::new(dex.inner.clone(), side_index(side)?),
        })
    }

    /// Feed protocol lines from this player's stream.
    fn feed(&mut self, lines: Vec<String>) {
        for l in &lines {
            self.inner.feed(l);
        }
    }

    /// A copy of this tracker: search advances the copy into a stepped
    /// world battle, leaving the live tracker untouched.
    fn copy(&self) -> PyTracker {
        PyTracker {
            inner: self.inner.clone(),
        }
    }

    /// Feed this player's request (JSON).
    fn feed_request(&mut self, request: &str) -> PyResult<()> {
        let v: serde_json::Value = serde_json::from_str(request)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        self.inner.feed_request(&v);
        Ok(())
    }

    /// Feed this player's own side straight from an engine battle (self-play;
    /// the same view as `feed_request` on the battle's request).
    fn feed_battle(&mut self, battle: &PyBattle) {
        self.inner.feed_engine(&battle.inner);
    }

    /// The view as JSON.
    fn view(&self) -> String {
        self.inner.to_json().to_string()
    }

    /// A world: the battle this view describes, with `assumptions` (JSON,
    /// see `World::from_view`; None for the defaults) for the foe's hidden
    /// sets. Raises ValueError on bad assumptions.
    #[pyo3(signature = (assumptions=None, seed=0))]
    fn world(&self, assumptions: Option<&str>, seed: u64) -> PyResult<PyBattle> {
        let mut a = match assumptions {
            Some(text) => {
                let v: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| PyValueError::new_err(e.to_string()))?;
                crate::world::Assumptions::from_json(self.inner.dex(), &v)
                    .map_err(PyValueError::new_err)?
            }
            None => crate::world::Assumptions::default(),
        };
        a.seed = seed;
        let inner =
            crate::world::World::from_view(&self.inner, &a).map_err(PyValueError::new_err)?;
        Ok(PyBattle { inner })
    }
}

/// Add the engine's classes to `m`.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEnumerationMode>()?;
    m.add_class::<PyOutcome>()?;
    m.add_class::<PyBattleStateKey>()?;
    m.add_class::<PyDex>()?;
    m.add_class::<PyBattle>()?;
    m.add_class::<PyTracker>()?;
    Ok(())
}

/// `ninjask._core`, the extension module the `ninjask` package re-exports.
#[cfg(feature = "python-module")]
#[pymodule(name = "_core")]
fn core_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register(m)
}
