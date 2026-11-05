//! Bounded, sampled diagnostics on existing learner transitions. No extra inference.
use crate::env::Choice;
use ninjask::{
    battle::log::Viewer,
    state::{RequestState, SlotChoice},
    tracker::{MonView, Tracker},
    Battle,
};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

const LABELS: [&str; 20] = [
    "selected_moves",
    "executed_move_attempts",
    "observed_immunity_events",
    "known_type_immunity_events",
    "failed_move_events",
    "double_shock_failures",
    "steel_roller_failures",
    "double_shock_without_electric",
    "double_shock_risk_states",
    "steel_roller_without_terrain",
    "steel_roller_risk_states",
    "first_turn_move_after_first_action",
    "dual_target_1",
    "dual_target_2",
    "voluntary_switches",
    "protect_selections",
    "miss_events",
    "ally_damage_candidate",
    "low_accuracy_selections",
    "previews",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts(pub [u64; 20]);

#[derive(Clone, Debug)]
struct Known {
    actor: usize,
    target: usize,
    name: String,
    mv: String,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub counts: Counts,
    pub preview: Option<[u8; 4]>,
    known: Vec<Known>,
}

fn has_type(t: &Tracker, m: &MonView, ty: u8) -> bool {
    m.added_type == Some(ty)
        || m.types.as_ref().map_or_else(
            || {
                m.species
                    .is_some_and(|id| t.dex().species(id).types.contains(&ty))
            },
            |types| types.contains(&ty),
        )
}

fn immune(t: &Tracker, m: &MonView, attack: u8) -> bool {
    let check = |ty| t.dex().is_immune(attack, ty);
    m.added_type.is_some_and(check)
        || m.types.as_ref().map_or_else(
            || {
                m.species
                    .is_some_and(|id| t.dex().species(id).types.into_iter().any(check))
            },
            |types| types.iter().copied().any(check),
        )
}

impl Sample {
    pub fn prepare(b: &Battle, t: &Tracker, c: &Choice, legal: &[Choice]) -> Self {
        let mut sample = Self::default();
        let n = &mut sample.counts.0;
        if let Choice::Preview(order) = c {
            n[19] = 1;
            sample.preview = Some(std::array::from_fn(|i| order[i] as u8));
            return sample;
        }
        let Choice::Turn(slots) = c else {
            return sample;
        };
        let electric = t.dex().type_id("Electric");
        let foe = 1 - t.me;
        let both_alive = (0..2).all(|slot| {
            t.sides[foe]
                .active(slot)
                .is_some_and(|m| m.hp > 0 && !m.fainted)
        });
        for (k, c) in slots.iter().enumerate() {
            let own = t.sides[t.me].active(k);
            if b.request == RequestState::Move {
                if let Some(m) = own.filter(|m| m.hp > 0 && !m.fainted) {
                    for mv in &m.moves {
                        if mv.pp.unwrap_or(0) == 0 {
                            continue;
                        }
                        match t.dex().mv(mv.id).id.as_str() {
                            "doubleshock" if electric.is_some_and(|ty| !has_type(t, m, ty)) => {
                                n[8] += 1
                            }
                            "steelroller" if t.field.terrain.is_none() => n[10] += 1,
                            _ => {}
                        }
                    }
                }
            }
            match c {
                SlotChoice::Switch { .. } if b.request == RequestState::Move => n[14] += 1,
                SlotChoice::Move { slot, target, mega } => {
                    n[0] += 1;
                    let Some(ms) = b.sides[t.me].mons[k].moves.get(*slot) else {
                        continue;
                    };
                    let mv = b.dex.mv(ms.id);
                    if mv.accuracy.unwrap_or(100) < 80 {
                        n[18] += 1;
                    }
                    if matches!(mv.id.as_str(), "protect" | "detect") {
                        n[15] += 1;
                    }
                    if *target < 0
                        && mv.base_power > 0
                        && !matches!(mv.id.as_str(), "beatup" | "pollenpuff")
                    {
                        n[17] += 1;
                    }
                    if both_alive && *target > 0 {
                        let exists = |loc| {
                            legal.iter().any(|c|matches!(c,Choice::Turn(cs) if matches!(cs.get(k),Some(SlotChoice::Move{slot:s,target:tar,mega:m}) if s==slot && *tar==loc && m==mega)))
                        };
                        if exists(1) && exists(2) {
                            n[11 + *target as usize] += 1;
                        }
                    }
                    if mv.id == "doubleshock"
                        && own.is_some_and(|m| electric.is_some_and(|ty| !has_type(t, m, ty)))
                    {
                        n[7] += 1;
                    }
                    if mv.id == "steelroller" && t.field.terrain.is_none() {
                        n[9] += 1;
                    }
                    if matches!(mv.id.as_str(), "fakeout" | "firstimpression")
                        && b.sides[t.me].mons[k].active_move_actions > 0
                    {
                        n[11] += 1;
                    }
                    if *target <= 0 || mv.base_power == 0 || mv.ignore_immunity == Some(true) {
                        continue;
                    }
                    let Some(defender) = t.sides[foe]
                        .active(*target as usize - 1)
                        .filter(|m| !m.fainted)
                    else {
                        continue;
                    };
                    let ability = own
                        .and_then(|m| m.ability)
                        .map(|id| t.dex().ability(id).id.as_str())
                        .unwrap_or("");
                    let mut ty = mv.type_;
                    if b.dex
                        .type_names
                        .get(ty as usize)
                        .is_some_and(|s| s == "Normal")
                    {
                        let converted = match ability {
                            "pixilate" => Some("Fairy"),
                            "aerilate" => Some("Flying"),
                            "refrigerate" => Some("Ice"),
                            "galvanize" => Some("Electric"),
                            _ => None,
                        };
                        if let Some(id) = converted.and_then(|s| b.dex.type_id(s)) {
                            ty = id;
                        }
                    }
                    if ability == "normalize" {
                        ty = b.dex.type_id("Normal").unwrap();
                    }
                    if matches!(ability, "scrappy" | "mindseye")
                        && b.dex
                            .type_names
                            .get(ty as usize)
                            .is_some_and(|s| s == "Normal" || s == "Fighting")
                    {
                        continue;
                    }
                    if matches!(
                        mv.id.as_str(),
                        "thousandarrows"
                            | "flyingpress"
                            | "revelationdance"
                            | "weatherball"
                            | "terablast"
                            | "judgment"
                            | "multiattack"
                            | "technoblast"
                    ) {
                        continue;
                    }
                    if immune(t, defender, ty) {
                        sample.known.push(Known {
                            actor: k,
                            target: *target as usize - 1,
                            name: defender.name.clone(),
                            mv: mv.name.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        sample
    }

    /// Only reads the already-generated incremental log, by borrowed string.
    pub fn finish(&mut self, b: &Battle, side: usize, from: usize) {
        self.consume(
            b.log[from..]
                .iter()
                .map(|line| line.render(Viewer::Side(side))),
            side,
        );
    }

    pub fn consume<'a>(&mut self, lines: impl Iterator<Item = &'a str>, side: usize) {
        let own = if side == 0 { "p1" } else { "p2" };
        let foe = if side == 0 { "p2" } else { "p1" };
        let mut last: Option<(&str, &str)> = None;
        for line in lines {
            let mut parts = line.split('|');
            parts.next();
            let event = parts.next().unwrap_or("");
            let who = parts.next().unwrap_or("");
            match event {
                "move" => {
                    let mv = parts.next().unwrap_or("");
                    last = if who.starts_with(own) {
                        self.counts.0[1] += 1;
                        Some((who, mv))
                    } else {
                        None
                    };
                }
                "-immune" if last.is_some() => {
                    self.counts.0[2] += 1;
                    let (actor, mv) = last.unwrap();
                    if who.starts_with(foe)
                        && self.known.iter().any(|c| {
                            actor.as_bytes().get(2) == Some(&(b'a' + c.actor as u8))
                                && who.as_bytes().get(2) == Some(&(b'a' + c.target as u8))
                                && c.mv == mv
                                && who.split_once(": ").is_some_and(|(_, name)| name == c.name)
                        })
                    {
                        self.counts.0[3] += 1;
                    }
                }
                "-fail" if last.is_some_and(|(actor, _)| actor == who) => {
                    let reason = parts.next().unwrap_or("");
                    if reason.is_empty() || reason.starts_with("move:") {
                        self.counts.0[4] += 1;
                        match last.unwrap().1 {
                            "Double Shock" => self.counts.0[5] += 1,
                            "Steel Roller" => self.counts.0[6] += 1,
                            _ => {}
                        }
                    }
                }
                "-miss" if who.starts_with(own) => self.counts.0[16] += 1,
                "cant" => last = None,
                _ => {}
            }
        }
    }
}

pub struct TrainingAudit {
    window: VecDeque<Sample>,
    max_window: usize,
    every: usize,
    totals: Counts,
    previews: HashMap<[u8; 4], u64>,
    lifetime: u64,
    pub seconds: f64,
}

impl TrainingAudit {
    pub fn new(max_window: usize, every: usize) -> Self {
        Self {
            window: VecDeque::new(),
            max_window,
            every,
            totals: Counts::default(),
            previews: HashMap::new(),
            lifetime: 0,
            seconds: 0.,
        }
    }
    pub fn push(&mut self, mut sample: Sample) {
        sample.known.clear(); // retain counters only, not strings or battle history
        self.lifetime += 1;
        for (a, b) in self.totals.0.iter_mut().zip(sample.counts.0) {
            *a += b;
        }
        if let Some(order) = sample.preview {
            *self.previews.entry(order).or_default() += 1;
        }
        self.window.push_back(sample);
        if self.window.len() > self.max_window {
            let old = self.window.pop_front().unwrap();
            for (a, b) in self.totals.0.iter_mut().zip(old.counts.0) {
                *a -= b;
            }
            if let Some(order) = old.preview {
                let count = self.previews.get_mut(&order).unwrap();
                *count -= 1;
                if *count == 0 {
                    self.previews.remove(&order);
                }
            }
        }
    }
    pub fn snapshot(&self) -> Value {
        let n = &self.totals.0;
        let rate = |numer: u64, denom: u64| {
            if denom > 0 {
                Some(100. * numer as f64 / denom as f64)
            } else {
                None
            }
        };
        let counts: serde_json::Map<String, Value> = LABELS
            .iter()
            .zip(n)
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        let mut orders: Vec<_> = self.previews.values().copied().collect();
        orders.sort_unstable_by(|a, b| b.cmp(a));
        json!({"window_samples":self.window.len(),"window_capacity":self.max_window,"measured_audit_seconds":self.seconds,
            "lifetime_samples":self.lifetime,"sample_one_in":self.every,"counts":counts,
            "immunity_per_100_attempts":rate(n[2],n[1]),"known_type_immunity_per_100_attempts":rate(n[3],n[1]),
            "failed_moves_per_100_attempts":rate(n[4],n[1]),"double_shock_failures_per_100_attempts":rate(n[5],n[1]),"steel_roller_failures_per_100_attempts":rate(n[6],n[1]),
            "double_shock_bad_per_100_risk_states":rate(n[7],n[8]),"steel_roller_bad_per_100_risk_states":rate(n[9],n[10]),
            "slot2_share_percent":rate(n[13],n[12]+n[13]),"preview_top_two_share_percent":rate(orders.iter().take(2).sum(),n[19]),
            "method":"Randomly sampled existing learner transitions; learner actions only, changing self-play opponents; ratios of rolling counts, no extra games or inference"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_counts_evict_old_samples_and_weight_denominators() {
        let mut audit = TrainingAudit::new(2, 16);
        for (attempts, immune) in [(10, 1), (1, 1), (4, 0)] {
            let mut sample = Sample::default();
            sample.counts.0[1] = attempts;
            sample.counts.0[2] = immune;
            audit.push(sample);
        }
        let r = audit.snapshot();
        assert_eq!(r["counts"]["executed_move_attempts"], 5);
        assert_eq!(r["immunity_per_100_attempts"], 20.);
        assert_eq!(r["window_samples"], 2);
        assert_eq!(r["lifetime_samples"], 3);
    }
    #[test]
    fn borrowed_log_parser_attributes_immunities_and_excludes_ability_failures() {
        let mut sample = Sample::default();
        sample.consume(
            [
                "|move|p1a: Sneasler|Close Combat|p2b: Gholdengo",
                "|-immune|p2b: Gholdengo",
                "|move|p2a: Gengar|Shadow Ball|p1b: Indeedee",
                "|-immune|p1b: Indeedee",
                "|-fail|p1b: Indeedee|unboost|atk",
                "|move|p1b: Pawmot|Double Shock||[still]",
                "|-fail|p1b: Pawmot|move: Double Shock",
            ]
            .into_iter(),
            0,
        );
        assert_eq!(sample.counts.0[1], 2);
        assert_eq!(sample.counts.0[2], 1);
        assert_eq!(sample.counts.0[4], 1);
        assert_eq!(sample.counts.0[5], 1);
    }

    #[test]
    fn known_immunity_requires_the_same_pre_action_target() {
        let mut sample = Sample::default();
        sample.known.push(Known {
            actor: 0,
            target: 1,
            name: "Gholdengo".into(),
            mv: "Close Combat".into(),
        });
        sample.consume(
            [
                "|move|p1a: Sneasler|Close Combat|p2b: Gholdengo",
                "|-immune|p2b: Gholdengo",
                "|move|p1a: Sneasler|Close Combat|p2b: Gengar",
                "|-immune|p2b: Gengar",
            ]
            .into_iter(),
            0,
        );
        assert_eq!(sample.counts.0[2], 2);
        assert_eq!(sample.counts.0[3], 1);
    }
}
