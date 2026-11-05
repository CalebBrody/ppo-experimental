use crate::model::*;
use crate::online_audit::{Sample, TrainingAudit};
use burn::prelude::*;
use ninjask::{
    battle::{log::Viewer, Team},
    state::{RequestState, SlotChoice},
    tracker::{ItemView, Tracker},
    Battle, Dex,
};
use rand::{rngs::SmallRng, RngExt, SeedableRng};
use rl4burn::{ActionDist, Env, MaskedActorCritic, Space, Step};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Preview(Vec<usize>),
    Turn(Vec<SlotChoice>),
}

/// Stable names, independent of dex IDs. First 18 are the ordinary battle types.
pub const TYPE_FEATURE_NAMES: [&str; 20] = [
    "Bug", "Dark", "Dragon", "Electric", "Fairy", "Fighting", "Fire", "Flying", "Ghost", "Grass",
    "Ground", "Ice", "Normal", "Poison", "Psychic", "Rock", "Steel", "Water", "Stellar", "???",
];

fn type_feature(t: &Tracker, id: u8) -> Option<usize> {
    let name = t.dex().type_names.get(id as usize)?;
    TYPE_FEATURE_NAMES.iter().position(|known| *known == name)
}

pub fn choices(b: &Battle, side: usize) -> Vec<Choice> {
    if !b.side_needs_choice(side) {
        return vec![];
    }
    if b.request != RequestState::TeamPreview {
        return b
            .joint_choices(side)
            .into_iter()
            .map(Choice::Turn)
            .collect();
    }
    let n = b.sides[side].mons.len();
    let mut result = vec![];
    for a in 0..n {
        for c in 0..n {
            for d in 0..n {
                for e in 0..n {
                    let order = vec![a, c, d, e];
                    if order
                        .iter()
                        .enumerate()
                        .all(|(i, x)| !order[..i].contains(x))
                    {
                        result.push(Choice::Preview(order));
                    }
                }
            }
        }
    }
    result
}

pub fn apply(b: &mut Battle, side: usize, c: Choice) {
    match c {
        Choice::Preview(order) => b.choose_team(side, order),
        Choice::Turn(c) => b.choose_structured(side, c),
    }
}

fn hash(out: &mut [f32], name: &str) {
    let h = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    out[h as usize % out.len()] += 1.;
}

/// Reads only Tracker and static dex data. Never reads the opposing hidden set.
pub fn encode_state(t: &Tracker) -> Vec<f32> {
    let mut out = vec![0.; STATE];
    for relative in 0..2 {
        let side = if relative == 0 { t.me } else { 1 - t.me };
        for (i, m) in t.sides[side].mons.iter().take(6).enumerate() {
            let f = &mut out[(relative * 6 + i) * FEATURES..(relative * 6 + i + 1) * FEATURES];
            f[0] = 1.;
            f[1] = relative as f32;
            f[8] = m.hp as f32 / m.maxhp.max(1) as f32;
            f[9] = m.fainted as u8 as f32;
            f[10] = m.seen as u8 as f32;
            if let Some(slot) = m.slot {
                f[11 + slot] = 1.;
            }
            hash(&mut f[13..21], m.status.id());
            for (j, v) in m.boosts.iter().enumerate() {
                f[21 + j] = *v as f32 / 6.;
            }
            if let Some(id) = m.species {
                let sp = t.dex().species(id);
                hash(&mut f[28..44], &sp.id);
                for j in 0..6 {
                    f[44 + j] = sp.base_stats[j] as f32 / 255.;
                }
            }
            // Both sides use only player-visible current types. Overrides replace
            // species defaults; added types are unioned. No hidden opponent state.
            f[2] = (m.types.is_some() || m.species.is_some()) as u8 as f32;
            f[3] = m.types.is_some() as u8 as f32;
            let mut encode_type = |ty| {
                if let Some(index) = type_feature(t, ty) {
                    f[236 + index] = 1.;
                    if index < 18 {
                        f[50 + index] = 1.;
                    }
                }
            };
            if let Some(types) = &m.types {
                for &ty in types {
                    encode_type(ty);
                }
            } else if let Some(id) = m.species {
                for ty in t.dex().species(id).types {
                    encode_type(ty);
                }
            }
            if let Some(ty) = m.added_type {
                encode_type(ty);
            }
            if let Some(id) = m.ability {
                hash(&mut f[68..76], &t.dex().ability(id).id);
            }
            match m.item {
                ItemView::Known(id) => hash(&mut f[76..84], &t.dex().item(id).id),
                ItemView::None => f[84] = 1.,
                _ => {}
            }
            for (j, mv) in m.moves.iter().take(4).enumerate() {
                let data = t.dex().mv(mv.id);
                let start = 85 + j * 8;
                hash(&mut f[start..start + 4], &data.id);
                f[start + 4] = data.base_power as f32 / 250.;
                f[start + 5] = data.accuracy.unwrap_or(100) as f32 / 100.;
                f[start + 6] = mv.pp.unwrap_or(0) as f32 / 64.;
                f[start + 7] = mv.pp.is_some() as u8 as f32;
                let extra = 128 + j * 24;
                if let Some(index) = type_feature(t, data.type_) {
                    if index < 18 {
                        f[extra + index] = 1.;
                    }
                    // Stellar has a separate flag rather than colliding with category.
                    if index == 18 {
                        f[4 + j] = 1.;
                    }
                }
                f[extra + 18 + data.category as usize] = 1.;
                f[extra + 21] = data.priority as f32 / 7.;
                f[extra + 22] = mv.uses as f32 / 64.;
                f[extra + 23] = 1.;
            }
            for v in &m.volatiles {
                hash(&mut f[117..128], v);
            }
            if let Some(stats) = m.stats {
                for (j, stat) in stats.iter().enumerate() {
                    f[224 + j] = *stat as f32 / 500.;
                }
                f[229] = 1.;
            }
            f[230] = m.mega as u8 as f32;
            if let Some(bounds) = t.speed_bounds(side, i) {
                f[231] = 1.;
                f[232] = bounds.min as f32 / 500.;
                f[233] = bounds.max as f32 / 500.;
                f[234] = (bounds.max - bounds.min) as f32 / 500.;
                f[235] = (bounds.min == bounds.max) as u8 as f32;
            }
        }
    }
    let f = &mut out[12 * FEATURES..];
    f[0] = t.turn as f32 / 150.;
    if let Some((w, start)) = t.field.weather {
        hash(&mut f[1..9], w.id());
        f[9] = t.turn.saturating_sub(start) as f32 / 8.;
    }
    if let Some((w, start)) = t.field.terrain {
        hash(&mut f[10..18], w.id());
        f[18] = t.turn.saturating_sub(start) as f32 / 8.;
    }
    f[19] = t.field.trick_room.is_some() as u8 as f32;
    for rel in 0..2 {
        let side = if rel == 0 { t.me } else { 1 - t.me };
        for (cond, _) in &t.sides[side].conditions {
            hash(&mut f[20 + rel * 16..36 + rel * 16], cond.id());
        }
    }
    out
}

pub fn observe(b: &Battle, t: &Tracker, cs: &[Choice]) -> Vec<f32> {
    observe_padded(b, t, cs, ACTIONS)
}

pub fn observe_padded(b: &Battle, t: &Tracker, cs: &[Choice], capacity: usize) -> Vec<f32> {
    assert!(
        !cs.is_empty() && cs.len() <= ACTIONS,
        "legal action capacity exceeded: {}",
        cs.len()
    );
    let mut out = encode_state(t);
    assert!(cs.len() <= capacity);
    out.resize(STATE + capacity * ACTION_FEATURES, 0.);
    for (j, c) in cs.iter().enumerate() {
        let f = &mut out[STATE + j * ACTION_FEATURES..STATE + (j + 1) * ACTION_FEATURES];
        f[ACTION_DESCRIPTOR..].fill(TOKENS as f32);
        match c {
            Choice::Preview(order) => {
                f[31] = 1.;
                for (slot, &index) in order.iter().enumerate() {
                    f[ACTION_DESCRIPTOR + slot] = own_reference(t, index);
                }
            }
            Choice::Turn(slots) => {
                for (k, c) in slots.iter().enumerate() {
                    if let SlotChoice::Move { slot, .. } = c {
                        // The action scorer receives its selected move's type
                        // directly, without resolving a hash into a state move slot.
                        if let Some(id) = b.choice_move_id((t.me, k), *slot) {
                            if let Some(ty) = type_feature(t, b.dex.mv(id).type_) {
                                f[ACTION_TYPE_OFFSET + k * ACTION_TYPE_COUNT + ty] = 1.;
                            }
                        }
                    }
                    f[ACTION_DESCRIPTOR + k] = active_reference(t, t.me, k);
                    f[ACTION_DESCRIPTOR + 2 + k] = match c {
                        SlotChoice::Switch { index } => own_reference(t, *index),
                        SlotChoice::Move { target, .. } => {
                            let target = b.choice_target_loc((t.me, k), *target);
                            if target == 0 {
                                TOKENS as f32
                            } else {
                                let side = if target > 0 { 1 - t.me } else { t.me };
                                active_reference(t, side, target.unsigned_abs() as usize - 1)
                            }
                        }
                        _ => TOKENS as f32,
                    };
                    let f = &mut f[k * 16..(k + 1) * 16];
                    match c {
                        SlotChoice::Pass => f[0] = 1.,
                        SlotChoice::Switch { .. } => {
                            f[1] = 1.;
                        }
                        SlotChoice::Move { slot, target, mega } => {
                            f[8] = 1.;
                            f[9] = b.choice_target_loc((t.me, k), *target) as f32 / 2.;
                            f[10] = *mega as u8 as f32;
                            if let Some(id) = b.choice_move_id((t.me, k), *slot) {
                                let m = b.dex.mv(id);
                                hash(&mut f[0..8], &m.id);
                                f[11] = m.base_power as f32 / 250.;
                                f[12] = m.accuracy.unwrap_or(100) as f32 / 100.;
                                f[13] = m.priority as f32 / 7.;
                            } else {
                                f[15] = 1.;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

// Engine indices only route actions to their entities. The policy receives the
// selected entity embedding, never the numeric index or a roster-position bit.
fn own_reference(t: &Tracker, position: usize) -> f32 {
    t.sides[t.me]
        .mons
        .iter()
        .position(|m| m.team_pos == Some(position))
        .expect("legal action must refer to a tracked own Pokemon") as f32
}

fn active_reference(t: &Tracker, side: usize, slot: usize) -> f32 {
    t.sides[side]
        .mons
        .iter()
        .position(|m| m.slot == Some(slot))
        .map_or(TOKENS as f32, |i| {
            (if side == t.me { 0 } else { 6 } + i) as f32
        })
}

pub fn sync(b: &Battle, trackers: &mut [Tracker; 2], cursor: &mut usize) {
    for (s, t) in trackers.iter_mut().enumerate() {
        for line in b.log_lines(Viewer::Side(s), *cursor) {
            t.feed(&line);
        }
        t.feed_engine(b);
    }
    *cursor = b.log.len();
}

pub fn sample<B: Backend>(
    model: &Model<B>,
    b: &Battle,
    t: &Tracker,
    cs: &[Choice],
    device: &B::Device,
    rng: &mut SmallRng,
) -> usize {
    let obs = Tensor::from_data(TensorData::new(observe(b, t, cs), [1, OBS]), device);
    let (logits, _) = model.forward(obs);
    let mut mask = vec![0.; ACTIONS];
    mask[..cs.len()].fill(1.);
    let mask = Tensor::from_data(TensorData::new(mask, [1, ACTIONS]), device);
    ActionDist::Discrete(ACTIONS).sample(&logits, Some(&mask), None, rng)[0][0] as usize
}

/// Average per-Pokemon HP fractions; bench Pokemon count and fainted ones stay zero.
/// After preview the simulator retains only the four brought Pokemon.
pub fn hp_advantage(b: &Battle, side: usize) -> f32 {
    if b.request == RequestState::TeamPreview {
        return 0.;
    }
    let health = |s: usize| {
        let mons = &b.sides[s].mons;
        mons.iter()
            .map(|m| {
                if m.fainted {
                    0.
                } else {
                    m.hp as f32 / m.maxhp.max(1) as f32
                }
            })
            .sum::<f32>()
            / mons.len().max(1) as f32
    };
    0.5 * (health(side) - health(1 - side))
}

/// Pay for progress since the previous decision, including the final transition.
/// The terminal win/loss bonus is separate; unchanged HP never pays twice.
pub fn battle_reward(b: &Battle, side: usize, done: bool, previous_advantage: f32) -> f32 {
    let result = if done && b.ended {
        b.winner
            .map_or(0., |winner| if winner == side { 1. } else { -1. })
    } else {
        0.
    };
    hp_advantage(b, side) - previous_advantage + result
}

#[derive(Default)]
pub struct TurnRewards {
    pub turns: u64,
    pub reward: f64,
    pub unfinished_reward: f64,
    pending: f64,
    counted: u32,
}
impl TurnRewards {
    pub fn record(&mut self, turn: u32, ended: bool, done: bool, reward: f32) {
        self.pending += reward as f64;
        let resolved = if ended { turn } else { turn.saturating_sub(1) };
        let delta = resolved.saturating_sub(self.counted);
        if delta > 0 {
            self.turns += delta as u64;
            self.reward += self.pending;
            self.pending = 0.;
            self.counted = resolved;
        }
        if done {
            self.reset_game();
        }
    }
    fn reset_game(&mut self) {
        self.unfinished_reward += self.pending;
        self.pending = 0.;
        self.counted = 0;
    }
}
pub struct BattleEnv<B: Backend> {
    pub turn_rewards: Arc<Mutex<TurnRewards>>,
    /// Actual completed-game outcomes, independent of shaped episode returns.
    pub outcomes: Arc<Mutex<[usize; 3]>>,
    pub audit: Option<Arc<Mutex<TrainingAudit>>>,
    pub audit_every: usize,
    audit_rng: SmallRng,
    dex: Arc<Dex>,
    teams: Arc<Vec<Team>>,
    pub battle: Battle,
    trackers: [Tracker; 2],
    cursor: usize,
    side: usize,
    steps: usize,
    cs: Vec<Choice>,
    rng: SmallRng,
    device: B::Device,
    league: Arc<Mutex<Vec<Model<B>>>>,
    opponent: Model<B>,
    prepared_opponent: Option<Choice>,
    pub max_turns: u32,
}

impl<B: Backend> BattleEnv<B> {
    pub fn side_view(&self, side: usize) -> &Tracker {
        &self.trackers[side]
    }
    pub fn frozen_opponent(&self) -> &Model<B> {
        &self.opponent
    }
    pub fn prepare_opponent(&mut self, choice: Choice) {
        assert!(self.prepared_opponent.is_none());
        assert!(self.battle.side_needs_choice(1 - self.side));
        assert!(choices(&self.battle, 1 - self.side)
            .iter()
            .any(|c| format!("{c:?}") == format!("{choice:?}")));
        self.prepared_opponent = Some(choice);
    }
    pub fn learner_view(&self) -> &Tracker {
        &self.trackers[self.side]
    }
    pub fn learner_side(&self) -> usize {
        self.side
    }
    pub fn legal_choices(&self) -> &[Choice] {
        &self.cs
    }
    pub fn new(
        dex: Arc<Dex>,
        teams: Arc<Vec<Team>>,
        league: Arc<Mutex<Vec<Model<B>>>>,
        device: B::Device,
        seed: u64,
        max_turns: u32,
    ) -> Self {
        let battle = Battle::from_teams(dex.clone(), [&teams[0], &teams[0]], ["p1", "p2"], seed);
        let opponent = league.lock().unwrap()[0].clone();
        Self {
            turn_rewards: Arc::new(Mutex::new(TurnRewards::default())),
            outcomes: Arc::new(Mutex::new([0; 3])),
            audit: None,
            audit_every: 0,
            audit_rng: SmallRng::seed_from_u64(seed ^ 0xD1A6_0057),
            trackers: [Tracker::new(dex.clone(), 0), Tracker::new(dex.clone(), 1)],
            dex,
            teams,
            battle,
            cursor: 0,
            side: 0,
            steps: 0,
            cs: vec![],
            rng: SmallRng::seed_from_u64(seed),
            device,
            league,
            opponent,
            prepared_opponent: None,
            max_turns,
        }
    }
    fn opponent_choice(&mut self) -> Choice {
        if let Some(choice) = self.prepared_opponent.take() {
            return choice;
        }
        let side = 1 - self.side;
        let cs = choices(&self.battle, side);
        let index = sample(
            &self.opponent,
            &self.battle,
            &self.trackers[side],
            &cs,
            &self.device,
            &mut self.rng,
        );
        cs[index].clone()
    }
    fn advance_to_learner(&mut self) {
        let mut guard = 0;
        while !self.battle.ended && !self.battle.side_needs_choice(self.side) {
            guard += 1;
            assert!(
                guard < 100,
                "no learner request after 100 opponent decisions"
            );
            let c = self.opponent_choice();
            apply(&mut self.battle, 1 - self.side, c);
            sync(&self.battle, &mut self.trackers, &mut self.cursor);
        }
    }
    fn obs(&mut self) -> Vec<f32> {
        self.cs = choices(&self.battle, self.side);
        observe(&self.battle, &self.trackers[self.side], &self.cs)
    }
}

impl<B: Backend> Env for BattleEnv<B> {
    type Observation = Vec<f32>;
    type Action = Vec<f32>;
    fn reset(&mut self) -> Vec<f32> {
        self.prepared_opponent = None;
        self.turn_rewards.lock().unwrap().reset_game();
        let a = self.rng.random_range(0..self.teams.len());
        let c = self.rng.random_range(0..self.teams.len());
        self.battle = Battle::from_teams(
            self.dex.clone(),
            [&self.teams[a], &self.teams[c]],
            ["p1", "p2"],
            self.rng.random(),
        );
        self.side = self.rng.random_range(0..2);
        self.steps = 0;
        self.cursor = 0;
        self.trackers = [
            Tracker::new(self.dex.clone(), 0),
            Tracker::new(self.dex.clone(), 1),
        ];
        let bank = self.league.lock().unwrap();
        self.opponent = bank[self.rng.random_range(0..bank.len())].clone();
        drop(bank);
        sync(&self.battle, &mut self.trackers, &mut self.cursor);
        self.obs()
    }
    fn step(&mut self, action: Vec<f32>) -> Step<Vec<f32>> {
        let index = action[0] as usize;
        assert!(index < self.cs.len());
        let mut diagnostic = if self.audit_every > 0
            && self.audit.is_some()
            && self.audit_rng.random_range(0..self.audit_every) == 0
        {
            let timer = Instant::now();
            let sample = Sample::prepare(
                &self.battle,
                &self.trackers[self.side],
                &self.cs[index],
                &self.cs,
            );
            Some((sample, self.battle.log.len(), timer.elapsed().as_secs_f64()))
        } else {
            None
        };
        let previous_advantage = hp_advantage(&self.battle, self.side);
        // Select both sides from the same pre-action state. No opponent sees
        // the learner's submitted choice before selecting its own.
        let opp = if self.battle.side_needs_choice(1 - self.side) {
            Some(self.opponent_choice())
        } else {
            None
        };
        apply(&mut self.battle, self.side, self.cs[index].clone());
        if let Some(c) = opp {
            apply(&mut self.battle, 1 - self.side, c);
        }
        sync(&self.battle, &mut self.trackers, &mut self.cursor);
        self.advance_to_learner();
        if let Some((mut sample, from, prepare_seconds)) = diagnostic.take() {
            let timer = Instant::now();
            sample.finish(&self.battle, self.side, from);
            let mut audit = self.audit.as_ref().unwrap().lock().unwrap();
            audit.push(sample);
            audit.seconds += prepare_seconds + timer.elapsed().as_secs_f64();
        }
        self.steps += 1;
        // The configured finite-horizon game ends in a draw, not a time-limit
        // truncation requiring a bootstrap from a reset observation.
        let done = self.battle.ended || self.battle.turn >= self.max_turns || self.steps >= 512;
        let reward = battle_reward(&self.battle, self.side, done, previous_advantage);
        self.turn_rewards
            .lock()
            .unwrap()
            .record(self.battle.turn, self.battle.ended, done, reward);
        if done {
            let outcome = if self.battle.ended {
                self.battle
                    .winner
                    .map_or(2, |s| if s == self.side { 0 } else { 1 })
            } else {
                2
            };
            self.outcomes.lock().unwrap()[outcome] += 1;
        }
        Step {
            observation: if done { vec![0.; OBS] } else { self.obs() },
            reward,
            terminated: done,
            truncated: false,
        }
    }
    fn action_mask(&self) -> Option<Vec<f32>> {
        let mut m = vec![0.; ACTIONS];
        m[..self.cs.len()].fill(1.);
        Some(m)
    }
    fn observation_space(&self) -> Space {
        Space::Box {
            low: vec![f32::NEG_INFINITY; OBS],
            high: vec![f32::INFINITY; OBS],
        }
    }
    fn action_space(&self) -> Space {
        Space::Discrete(ACTIONS)
    }
}

#[cfg(test)]
mod reconstruction_tests {
    use super::*;
    #[test]
    fn locked_action_encodes_executed_move_and_target() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|p| p.join("Model2/runs/opponent-suite-insample-heldout-20261104/dex.json"))
            .find(|p| p.exists())
            .expect("frozen regression dex required");
        let dex = Arc::new(Dex::from_json(&std::fs::read_to_string(path).unwrap()).unwrap());
        let mut t = Tracker::new(dex, 0);
        for line in [
            "|switch|p1a: Basculegion|Basculegion, L50|199/199",
            "|switch|p2a: Pelipper|Pelipper, L50|100/100",
            "|switch|p2b: Gengar|Gengar, L50|100/100",
            "|turn|1",
            "|move|p1a: Basculegion|Surf|p2a: Pelipper",
            "|move|p1a: Basculegion|Solar Beam|p2b: Gengar|[still]",
            "|-prepare|p1a: Basculegion|Solar Beam|p2b: Gengar",
        ] {
            t.feed(line);
        }
        let b = ninjask::world::World::from_view(&t, &Default::default()).unwrap();
        let c = Choice::Turn(vec![SlotChoice::Move {
            slot: 0,
            target: 0,
            mega: false,
        }]);
        let obs = observe_padded(&b, &t, &[c], 1);
        let f = &obs[STATE..];
        let mut expected = [0.; 8];
        hash(&mut expected, "solarbeam");
        assert_eq!(&f[..8], &expected);
        assert_eq!(f[9], 1.);
        assert_eq!(f[11], 120. / 250.);
        assert_eq!(f[ACTION_TYPE_OFFSET + 9], 1.);
        assert_eq!(f[ACTION_DESCRIPTOR + 2], active_reference(&t, 1, 1));
    }
}
