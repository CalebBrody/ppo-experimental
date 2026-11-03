//! Conservative bounds on unmodified Champions Speed, inclusive of ties.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SpeedBounds {
    pub min: u16,
    pub max: u16,
}

impl Tracker {
    /// Possible unmodified Speed of the shown species (not effective battle
    /// Speed). Own requests give an exact value. Unknown nature permits both
    /// Speed natures; Champions permits 0..=32 points. None for unknown species
    /// or Transform, whose copied stats require a separate model.
    pub fn speed_bounds(&self, side: usize, index: usize) -> Option<SpeedBounds> {
        let m = self.sides.get(side)?.mons.get(index)?;
        if m.transformed.is_some() {
            return None;
        }
        if m.speed_swapped {
            return m.speed_swap_bounds;
        }
        if let Some(stats) = m.stats {
            return Some(SpeedBounds {
                min: stats[4],
                max: stats[4],
            });
        }
        let species = m.species?;
        let base = self.dex.species(species).base_stats[5] + 20;
        let (low, high) = if let Some(nature) = &m.nature {
            let n = self.dex.nature(nature);
            let factor = if n.plus == Some(Stat::Spe) {
                110
            } else if n.minus == Some(Stat::Spe) {
                90
            } else {
                100
            };
            (factor, factor)
        } else {
            (90, 110)
        };
        let mut bounds = SpeedBounds {
            min: base * low / 100,
            max: (base + 32) * high / 100,
        };
        if let Some((shown, observed)) = m.speed_observed {
            if shown == species {
                bounds.min = bounds.min.max(observed.min);
                bounds.max = bounds.max.min(observed.max);
            }
        }
        (bounds.min <= bounds.max).then_some(bounds)
    }

    pub(super) fn speed_protocol(&mut self, raw: &str) {
        if let Some(line) = Line::parse(raw) {
            if line.kind == "-activate" && line.arg(1) == "move: Speed Swap" {
                if let (Some(a), Some(b)) = (
                    self.mon_index(line.arg(0)),
                    line.tag("of").and_then(|r| self.mon_index(r)),
                ) {
                    let (a_bounds, b_bounds) =
                        (self.speed_bounds(a.0, a.1), self.speed_bounds(b.0, b.1));
                    for (r, bounds) in [(a, b_bounds), (b, a_bounds)] {
                        let m = &mut self.sides[r.0].mons[r.1];
                        m.speed_swapped = true;
                        m.speed_swap_bounds = bounds;
                    }
                }
            }
        }
        let kind = raw.split('|').nth(1).unwrap_or("");
        if kind == "turn" {
            self.speed_order_blocked = false;
        }
        // No comparison across a change of ability, item, status, boosts,
        // field, identity or active slot. Damage alone is safe for the neutral
        // ability/item whitelist used below.
        if !matches!(
            kind,
            "move" | "-damage" | "-heal" | "-crit" | "-supereffective" | "-resisted" | "-miss" | ""
        ) {
            self.speed_previous = None;
        }
        // These effects can rearrange later actions, even after their own
        // protocol messages finish. Do not infer any more order this turn.
        if [
            "Quick Claw",
            "Quick Draw",
            "Custap Berry",
            "After You",
            "Quash",
            "Round",
            "Instruct",
            "Fire Pledge",
            "Water Pledge",
            "Grass Pledge",
        ]
        .iter()
        .any(|s| raw.contains(s))
        {
            self.speed_order_blocked = true;
            self.speed_previous = None;
        }
    }

    /// Only use actors whose known ability/item cannot alter priority or
    /// speed. Unknown sets are deliberately not assumed neutral.
    fn plain_speed_actor(&self, r: (usize, usize)) -> bool {
        let m = &self.sides[r.0].mons[r.1];
        if m.transformed.is_some()
            || !m.volatiles.is_empty()
            || self.field.weather.is_some()
            || self.field.terrain.is_some()
            || self.sides.iter().flat_map(|s| &s.mons).any(|m| {
                m.species
                    .is_some_and(|s| self.dex.species(s).id.starts_with("zoroark"))
            })
        {
            return false;
        }
        let Some(a) = m.ability else {
            return false;
        };
        if !matches!(
            self.dex.ability(a).id.as_str(),
            "goodasgold"
                | "intimidate"
                | "rockhead"
                | "psychicsurge"
                | "grassysurge"
                | "defiant"
                | "armortail"
                | "pixilate"
                | "innerfocus"
                | "scrappy"
                | "solidrock"
                | "levitate"
                | "roughskin"
                | "ironbarbs"
                | "clearbody"
                | "sturdy"
                | "friendguard"
                | "overcoat"
                | "regenerator"
                | "waterabsorb"
                | "flashfire"
                | "magicguard"
                | "moldbreaker"
        ) {
            return false;
        }
        match m.item {
            ItemView::None => true,
            ItemView::Known(i) => {
                let item = self.dex.item(i);
                matches!(
                    item.id.as_str(),
                    "lifeorb"
                        | "focussash"
                        | "leftovers"
                        | "rockyhelmet"
                        | "assaultvest"
                        | "choicespecs"
                        | "choiceband"
                        | "sitrusberry"
                        | "safetygoggles"
                        | "clearamulet"
                        | "protectivepads"
                        | "covertcloak"
                        | "choicescarf"
                        | "ironball"
                        | "machobrace"
                        | "poweranklet"
                        | "powerband"
                        | "powerbelt"
                        | "powerbracer"
                        | "powerlens"
                        | "powerweight"
                ) || item.id.ends_with("ite")
            }
            ItemView::Unknown => false,
        }
    }

    // Use the engine's rounding helpers, with the revealed modifiers that
    // plain_speed_actor permits. Unknown ability modifiers remain excluded.
    fn observed_speed(&self, r: (usize, usize), raw: u16) -> u32 {
        use crate::moves::damage::{apply_boost, Chain};
        let m = &self.sides[r.0].mons[r.1];
        let mut speed = apply_boost(raw as u32, m.boosts[4]);
        let mut chain = Chain::ONE;
        if self.field.magic_room.is_none() {
            if let ItemView::Known(i) = m.item {
                match self.dex.item(i).id.as_str() {
                    "choicescarf" => chain = chain.then(3, 2),
                    "ironball" | "machobrace" | "poweranklet" | "powerband" | "powerbelt"
                    | "powerbracer" | "powerlens" | "powerweight" => chain = chain.then(1, 2),
                    _ => {}
                }
            }
        }
        if self.sides[r.0]
            .condition_since(SideCond::Tailwind)
            .is_some()
        {
            chain = chain.then(2, 1);
        }
        speed = chain.apply(speed);
        if m.status == Status::Par {
            speed /= 2;
        }
        speed.min(10000)
    }

    pub(super) fn observe_move_speed(&mut self, line: &Line) {
        let previous = self.speed_previous.take();
        if self.speed_order_blocked || line.tag("from").is_some() {
            return;
        }
        let Some(actor) = self.mon_index(line.arg(0)) else {
            return;
        };
        let Some(mv) = self.dex.move_id(line.arg(1)) else {
            return;
        };
        let mv = self.dex.mv(mv);
        // Charged/called moves need queue-level evidence, not just move lines.
        if matches!(
            mv.id.as_str(),
            "focuspunch"
                | "shelltrap"
                | "beakblast"
                | "sleeptalk"
                | "copycat"
                | "metronome"
                | "naturepower"
                | "mefirst"
                | "struggle"
        ) {
            return;
        }
        if !self.plain_speed_actor(actor) {
            return;
        }
        let priority = mv.priority;
        if let Some((first, first_priority)) = previous {
            if first != actor && first_priority == priority && self.plain_speed_actor(first) {
                if let (Some(a), Some(b)) = (
                    self.speed_bounds(first.0, first.1),
                    self.speed_bounds(actor.0, actor.1),
                ) {
                    let reverse = self.field.trick_room.is_some();
                    let a_values: Vec<_> = (a.min..=a.max)
                        .filter(|&x| {
                            let s = self.observed_speed(first, x);
                            if reverse {
                                s <= self.observed_speed(actor, b.max)
                            } else {
                                s >= self.observed_speed(actor, b.min)
                            }
                        })
                        .collect();
                    let b_values: Vec<_> = (b.min..=b.max)
                        .filter(|&x| {
                            let s = self.observed_speed(actor, x);
                            if reverse {
                                s >= self.observed_speed(first, a.min)
                            } else {
                                s <= self.observed_speed(first, a.max)
                            }
                        })
                        .collect();
                    if let (Some(&amin), Some(&amax), Some(&bmin), Some(&bmax)) = (
                        a_values.first(),
                        a_values.last(),
                        b_values.first(),
                        b_values.last(),
                    ) {
                        let a = SpeedBounds {
                            min: amin,
                            max: amax,
                        };
                        let b = SpeedBounds {
                            min: bmin,
                            max: bmax,
                        };
                        for (r, bounds) in [(first, a), (actor, b)] {
                            let m = &mut self.sides[r.0].mons[r.1];
                            if m.speed_swapped {
                                m.speed_swap_bounds = Some(bounds);
                            } else if let Some(species) = m.species {
                                m.speed_observed = Some((species, bounds));
                            }
                        }
                    }
                }
            }
        }
        self.speed_previous = Some((actor, priority));
    }
}
