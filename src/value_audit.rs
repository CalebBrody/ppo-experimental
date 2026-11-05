//! Outcome-labelled online value diagnostics; never used as training targets.
use serde_json::json;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub struct Prediction {
    pub value: f32,
    pub hp_difference: f32,
}
impl Prediction {
    fn groups(self) -> [bool; 4] {
        let disagree = self.value * self.hp_difference < 0.;
        [
            disagree,
            disagree && self.hp_difference < 0.,
            disagree && self.hp_difference > 0.,
            disagree && self.hp_difference.abs() >= 0.1 && self.value.abs() >= 0.3,
        ]
    }
}
pub struct ValueAudit {
    capacity: usize,
    samples: VecDeque<(Prediction, bool)>,
    pub seconds: f64,
}
impl ValueAudit {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            samples: VecDeque::new(),
            seconds: 0.,
        }
    }
    pub fn complete(&mut self, predictions: Vec<Prediction>, outcome: usize) {
        if outcome == 2 {
            return;
        } // Draws/cutoffs have no binary winner label.
        for p in predictions {
            self.samples.push_back((p, outcome == 0));
            if self.samples.len() > self.capacity {
                self.samples.pop_front();
            }
        }
    }
    pub fn snapshot(&self) -> serde_json::Value {
        let names = [
            "all_disagreements",
            "hp_behind_value_positive",
            "hp_ahead_value_negative",
            "material_disagreements_hp10pp_value03",
        ];
        let mut result = json!({"window_capacity":self.capacity,"completed_positions":self.samples.len(),
            "measured_seconds":self.seconds,"method":"Sampled pre-choice raw network value; actual team HP difference; labels assigned only on completed decisive games; rolling completed positions, not held-out evaluation"});
        for (i, name) in names.iter().enumerate() {
            let mut n = 0usize;
            let mut correct = 0usize;
            for (p, win) in &self.samples {
                if p.groups()[i] {
                    n += 1;
                    correct += usize::from((p.value > 0.) == *win);
                }
            }
            result[*name] = json!({"positions":n,"value_correct":correct,"accuracy_percent":if n>0 {Some(100.*correct as f64/n as f64)} else {None}});
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_groups_boundaries_and_eviction() {
        let mut a = ValueAudit::new(3);
        let p = |value, hp_difference| Prediction {
            value,
            hp_difference,
        };
        a.complete(vec![p(0.4, -0.2), p(-0.4, 0.2), p(0.1, 0.)], 0);
        let s = a.snapshot();
        assert_eq!(s["all_disagreements"]["positions"], 2);
        assert_eq!(s["all_disagreements"]["accuracy_percent"], 50.);
        assert_eq!(s["hp_behind_value_positive"]["value_correct"], 1);
        assert_eq!(s["hp_ahead_value_negative"]["value_correct"], 0);
        a.complete(vec![p(0.4, -0.2)], 2);
        assert_eq!(a.snapshot()["completed_positions"], 3);
        a.complete(vec![p(-0.1, 0.2)], 1);
        let s = a.snapshot();
        assert_eq!(s["all_disagreements"]["positions"], 2);
        assert_eq!(s["material_disagreements_hp10pp_value03"]["positions"], 1);
        assert!(ValueAudit::new(3).snapshot()["all_disagreements"]["accuracy_percent"].is_null());
    }
}
