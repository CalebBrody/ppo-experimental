//! Enumerate by replaying explicit chance paths; the Rust stack runs normally.
use super::{Battle, BattleStateKey};
use crate::rng::{PreciseState, Rng};
use crate::state::{RequestState, SlotChoice};
use crate::{EnumerationError, EnumerationMode, Outcome};
use std::collections::HashMap;

impl Battle {
    /// Enumerate until the next player request or battle end without mutating self.
    /// Supply an empty choice for a side that does not currently need one.
    /// Team preview uses its separate team-order API and is not accepted here.
    pub fn enumerate_outcomes(
        &self,
        choice_p1: &[SlotChoice],
        choice_p2: &[SlotChoice],
        mode: EnumerationMode,
    ) -> Result<Vec<Outcome>, EnumerationError> {
        let choices = [choice_p1, choice_p2];
        if self.request == RequestState::TeamPreview {
            return Err(EnumerationError(
                "enumeration requires a move or switch request".into(),
            ));
        }
        let needed = [self.side_needs_choice(0), self.side_needs_choice(1)];
        for s in 0..2 {
            if needed[s] {
                if !self.joint_choices(s).iter().any(|c| c == choices[s]) {
                    return Err(EnumerationError(format!(
                        "invalid structured choice for p{}",
                        s + 1
                    )));
                }
            } else if !choices[s].is_empty() {
                return Err(EnumerationError(format!(
                    "p{} does not need a choice; supply an empty choice",
                    s + 1
                )));
            }
        }
        self.enumerate_with(mode, |b| {
            for s in 0..2 {
                if needed[s] {
                    b.choose_structured(s, choices[s].to_vec());
                }
            }
            Ok(())
        })
    }

    /// String choices reuse Showdown's existing parser, including hidden reveals.
    pub fn enumerate_outcomes_text(
        &self,
        choice_p1: &str,
        choice_p2: &str,
        mode: EnumerationMode,
    ) -> Result<Vec<Outcome>, EnumerationError> {
        if self.request == RequestState::TeamPreview {
            return Err(EnumerationError(
                "enumeration requires a move or switch request".into(),
            ));
        }
        let needed = [self.side_needs_choice(0), self.side_needs_choice(1)];
        let choices = [choice_p1, choice_p2];
        for s in 0..2 {
            if !needed[s] && !choices[s].is_empty() {
                return Err(EnumerationError(format!(
                    "p{} does not need a choice; supply an empty choice",
                    s + 1
                )));
            }
        }
        self.enumerate_with(mode, |b| {
            for s in 0..2 {
                if needed[s] {
                    b.choose(s, choices[s]).map_err(EnumerationError)?;
                }
            }
            Ok(())
        })
    }

    fn enumerate_with(
        &self,
        mode: EnumerationMode,
        apply: impl Fn(&mut Battle) -> Result<(), EnumerationError>,
    ) -> Result<Vec<Outcome>, EnumerationError> {
        let mut path = PreciseState::new(mode);
        let mut outcomes: Vec<Outcome> = Vec::new();
        let mut indices: HashMap<BattleStateKey, usize> = HashMap::new();
        let mut replayed_paths = 0u64;
        let diagnose = std::env::var_os("NINJASK_ENUM_DECISIONS").is_some();
        let mut signatures = std::collections::BTreeMap::<String, u64>::new();
        loop {
            replayed_paths += 1;
            let mut battle = self.clone();
            battle.rng = Rng::precise(path);
            apply(&mut battle)?;
            path = std::mem::replace(&mut battle.rng, self.rng.clone()).into_precise();
            if diagnose { *signatures.entry(path.diagnostic_signature()).or_default() += 1; }
            let probability = path.probability;
            let key = battle.state_key();
            if let Some(&index) = indices.get(&key) {
                outcomes[index].probability += probability;
            } else {
                indices.insert(key, outcomes.len());
                outcomes.push(Outcome {
                    probability,
                    battle,
                });
            }
            if !path.advance() {
                break;
            }
        }
        if std::env::var_os("NINJASK_COUNT_ENUM_PATHS").is_some() {
            eprintln!("ENUM_PATH_COUNT paths={} merged_outcomes={}", replayed_paths, outcomes.len());
        }
        for (signature, count) in signatures { eprintln!("ENUM_DECISIONS count={count} {signature}"); }
        Ok(outcomes)
    }
}
