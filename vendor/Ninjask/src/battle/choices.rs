//! Choices: parsing and validating them, the legal ones as structured values (no
//! strings, for self-play and search; mirrors `python/ninjask/choices.py`), and the
//! request sent to each player.

use arrayvec::ArrayVec;
use serde_json::{json, Value};

use super::log::secret_health;
use super::{Battle, MonRef, UNAVAILABLE};
use crate::dex::types::*;
use crate::state::*;

impl Battle {
    /// Submit a choice for a side, in Showdown's syntax. When both sides
    /// have chosen, the turn (or preview, or switch phase) runs.
    pub fn choose(&mut self, s: usize, input: &str) -> Result<(), String> {
        if self.ended {
            return Err("The battle is over".into());
        }
        match self.request {
            RequestState::TeamPreview => {
                let order = self.parse_team_choice(s, input)?;
                self.sides[s].team_preview_choice = Some(order);
            }
            RequestState::Move | RequestState::Switch => {
                if !self.side_needs_choice(s) {
                    return Err("There is nothing for you to choose right now".into());
                }
                let choice = match self.parse_choice(s, input) {
                    Ok(c) => c,
                    Err(e) => {
                        if e.starts_with(UNAVAILABLE) {
                            self.record_reveal(s, input);
                        }
                        return Err(e);
                    }
                };
                self.sides[s].choice = Some(choice);
            }
            RequestState::Ended => return Err("The battle is over".into()),
        }
        if self.all_choices_done() {
            self.commit();
        }
        Ok(())
    }

    /// Record what an "[Unavailable choice]" revealed: the first slot whose
    /// part was refused.
    fn record_reveal(&mut self, s: usize, input: &str) {
        let parts: Vec<&str> = input.split(',').map(|p| p.trim()).collect();
        for slot in 0..self.sides[s].n_active {
            let part = parts.get(slot).copied().unwrap_or("");
            if let Err(e) = self.parse_slot_choice(s, slot, part) {
                if e.starts_with(UNAVAILABLE) {
                    let what = if e.contains("trapped") {
                        Reveal::Trapped
                    } else {
                        Reveal::Disabled
                    };
                    let rqid = self.rqid;
                    let side = &mut self.sides[s];
                    if side.reveals_rqid != rqid {
                        side.reveals.clear();
                        side.reveals_rqid = rqid;
                    }
                    if !side.reveals.contains(&(slot, what)) {
                        side.reveals.push((slot, what));
                    }
                }
                break;
            }
        }
    }

    /// Whether a refused choice revealed this for the current request.
    fn revealed(&self, s: usize, slot: usize, what: Reveal) -> bool {
        let side = &self.sides[s];
        side.reveals_rqid == self.rqid && side.reveals.contains(&(slot, what))
    }

    pub fn side_needs_choice(&self, s: usize) -> bool {
        match self.request {
            RequestState::TeamPreview => self.sides[s].team_preview_choice.is_none(),
            RequestState::Move => self.sides[s].choice.is_none() && self.side_has_actives(s),
            RequestState::Switch => {
                self.sides[s].choice.is_none() && self.force_switch[s].iter().any(|&f| f)
            }
            RequestState::Ended => false,
        }
    }

    fn side_has_actives(&self, s: usize) -> bool {
        (0..self.sides[s].n_active).any(|slot| {
            self.active(s, slot)
                .map(|r| !self.mon(r).fainted)
                .unwrap_or(false)
        })
    }

    pub(super) fn all_choices_done(&self) -> bool {
        (0..2).all(|s| !self.side_needs_choice(s))
    }

    fn parse_team_choice(&self, s: usize, input: &str) -> Result<Vec<usize>, String> {
        let input = input.trim();
        let rest = input
            .strip_prefix("team")
            .ok_or("expected a team choice")?
            .trim();
        let n = self.sides[s].mons.len();
        let picks: Vec<usize> = if rest.contains(',') || rest.contains(' ') {
            rest.split([',', ' '])
                .filter(|x| !x.is_empty())
                .map(|x| x.parse::<usize>().unwrap_or(0))
                .collect()
        } else {
            rest.chars()
                .map(|c| c.to_digit(10).unwrap_or(0) as usize)
                .collect()
        };
        let want = self.dex.format.picked_team_size.unwrap_or(n).min(n);
        let mut order = Vec::new();
        for p in picks {
            if p == 0 || p > n {
                return Err(format!("Can't choose for Team Preview: invalid slot {p}"));
            }
            if order.contains(&(p - 1)) {
                return Err("Can't choose for Team Preview: duplicate slot".into());
            }
            order.push(p - 1);
        }
        // Showdown fills in the rest in team order.
        for i in 0..n {
            if order.len() >= want {
                break;
            }
            if !order.contains(&i) {
                order.push(i);
            }
        }
        order.truncate(want);
        Ok(order)
    }

    fn parse_choice(&self, s: usize, input: &str) -> Result<Vec<SlotChoice>, String> {
        let side = &self.sides[s];
        let n = side.n_active;
        let parts: Vec<&str> = input.split(',').map(|p| p.trim()).collect();
        let mut out = Vec::new();
        let mut switching: Vec<usize> = Vec::new();
        let mut mega = false;
        for slot in 0..n {
            let part = parts.get(slot).copied().unwrap_or("");
            let choice = self.parse_slot_choice(s, slot, part)?;
            match choice {
                SlotChoice::Switch { index } => {
                    if switching.contains(&index) {
                        return Err(format!(
                            "Can't switch: The Pokémon in slot {} can only switch in once",
                            index + 1
                        ));
                    }
                    switching.push(index);
                }
                SlotChoice::Move { mega: true, .. } => {
                    if mega {
                        return Err("Can't move: You can only Mega-Evolve once per battle".into());
                    }
                    mega = true;
                }
                _ => {}
            }
            out.push(choice);
        }
        if self.request == RequestState::Switch {
            // Fill as many forced slots as possible (Showdown rejects passing otherwise).
            let forced: Vec<usize> = (0..n).filter(|&sl| self.force_switch[s][sl]).collect();
            let bench = (n..side.mons.len())
                .filter(|&i| !side.mons[i].fainted)
                .count();
            if switching.len() < forced.len().min(bench) {
                return Err("Can't pass: You need to switch in a Pokémon".into());
            }
        }
        Ok(out)
    }

    fn parse_slot_choice(&self, s: usize, slot: usize, part: &str) -> Result<SlotChoice, String> {
        let side = &self.sides[s];
        let words: Vec<&str> = part.split_whitespace().collect();
        let kind = words.first().copied().unwrap_or("default");
        let mon_ref = (s, slot);
        let fainted_or_empty = self
            .active(s, slot)
            .map(|r| self.mon(r).fainted)
            .unwrap_or(true);
        if self.request == RequestState::Switch {
            if !self.force_switch[s][slot] {
                return Ok(SlotChoice::Pass);
            }
            if side.revival[slot] {
                // Revival Blessing: any fainted teammate, even one still in
                // its slot; with no target given, the first fainted one.
                return match kind {
                    "switch" | "default" => {
                        let idx = match words.get(1) {
                            None => side
                                .mons
                                .iter()
                                .position(|m| m.fainted)
                                .ok_or("Can't switch: no fainted Pokémon")?,
                            Some(w) => match w.parse::<usize>() {
                                Ok(k) if k >= 1 && k <= side.mons.len() => k - 1,
                                _ => side
                                    .mons
                                    .iter()
                                    .position(|m| to_id(&m.name) == to_id(w))
                                    .ok_or_else(|| format!("Can't switch: invalid Pokémon {w}"))?,
                            },
                        };
                        if !side.mons[idx].fainted {
                            return Err(
                                "Can't switch: You have to pass to a fainted Pokémon".into()
                            );
                        }
                        Ok(SlotChoice::Switch { index: idx })
                    }
                    "pass" => Ok(SlotChoice::Pass),
                    _ => Err(format!("Can't {kind}: You need a switch response")),
                };
            }
            return match kind {
                "switch" => self.parse_switch_target(s, words.get(1).copied()),
                "pass" | "default" => {
                    if kind == "default" {
                        if let Some(i) =
                            (side.n_active..side.mons.len()).find(|&i| !side.mons[i].fainted)
                        {
                            return Ok(SlotChoice::Switch { index: i });
                        }
                    }
                    Ok(SlotChoice::Pass)
                }
                _ => Err(format!("Can't {kind}: You need a switch response")),
            };
        }
        if fainted_or_empty {
            return Ok(SlotChoice::Pass);
        }
        if let Some(lock) = self.locked_move(mon_ref) {
            // The request offers only the locked move, without a target, and
            // traps the Pokémon; Showdown ignores "mega" here.
            let (name, id) = match lock {
                crate::moves::LockedMove::Recharge => {
                    ("Recharge".to_string(), "recharge".to_string())
                }
                crate::moves::LockedMove::Move(m) => {
                    (self.dex.mv(m).name.clone(), self.dex.mv(m).id.clone())
                }
            };
            return match kind {
                "move" | "default" => {
                    if kind == "move" {
                        let w = words.get(1).copied().ok_or("Can't move: no move given")?;
                        if w != "1" && to_id(w) != id {
                            return Err(format!(
                                "Can't move: Your {} doesn't have a move matching {w}",
                                self.mon(mon_ref).name
                            ));
                        }
                        if words.iter().skip(2).any(|w| w.parse::<i8>().is_ok()) {
                            return Err(format!(
                                "Can't move: You can't choose a target for {name}"
                            ));
                        }
                    }
                    Ok(SlotChoice::Move {
                        slot: 0,
                        target: 0,
                        mega: false,
                    })
                }
                "switch" => Err("Can't switch: The active Pokémon is trapped".into()),
                "pass" => Err("Can't pass: Your Pokémon must act".into()),
                other => Err(format!("Unrecognized choice {other}")),
            };
        }
        match kind {
            "pass" => Err("Can't pass: Your Pokémon must act".into()),
            // "[Unavailable choice]" when the request hid the trap.
            "switch" if self.mon(mon_ref).trapped != Trapped::No => {
                let shown = self.mon(mon_ref).trapped == Trapped::Yes
                    || !self.is_last_active(mon_ref)
                    || self.revealed(s, slot, Reveal::Trapped);
                let prefix = if shown { "" } else { UNAVAILABLE };
                Err(format!(
                    "{prefix}Can't switch: The active Pokémon is trapped"
                ))
            }
            "switch" => self.parse_switch_target(s, words.get(1).copied()),
            "move" | "default" => {
                // Showdown `chooseMove`, in its order. "default" is autoChoose.
                let m = self.mon(mon_ref);
                let auto = kind == "default";
                let mut target: i8 = 0;
                let mut mega = false;
                for w in words.iter().skip(2) {
                    if let Ok(t) = w.parse::<i8>() {
                        target = t;
                    } else if *w == "mega" {
                        mega = true;
                    } else {
                        return Err(format!("Can't move: unknown modifier {w}"));
                    }
                }
                let usable: Vec<usize> = (0..m.moves.len())
                    .filter(|&i| self.move_usable(mon_ref, i))
                    .collect();
                // The request's moves: all of them, or Struggle alone when it
                // shows none usable (Imprison's hidden disables aside).
                let shown: Vec<usize> = (0..m.moves.len())
                    .filter(|&i| !self.move_shown_disabled(mon_ref, i))
                    .collect();
                // The chosen request move; `None` is the request's Struggle.
                let chosen = if shown.is_empty() {
                    if !auto {
                        let w = words.get(1).copied().ok_or("Can't move: no move given")?;
                        if w.parse::<usize>().is_ok_and(|k| k != 1) {
                            return Err(format!(
                                "Can't move: Your {} doesn't have a move {w}",
                                m.name
                            ));
                        } else if w.parse::<usize>().is_err() && to_id(w) != "struggle" {
                            return Err(format!(
                                "Can't move: Your {} doesn't have a move matching {w}",
                                m.name
                            ));
                        }
                    }
                    None
                } else if auto {
                    Some(usable.first().copied().unwrap_or(shown[0]))
                } else {
                    let w = words.get(1).copied().ok_or("Can't move: no move given")?;
                    Some(match w.parse::<usize>() {
                        Ok(k) if k >= 1 && k <= m.moves.len() => k - 1,
                        Ok(_) => {
                            return Err(format!(
                                "Can't move: Your {} doesn't have a move {w}",
                                m.name
                            ))
                        }
                        Err(_) => {
                            let id = to_id(w);
                            m.moves
                                .iter()
                                .position(|ms| self.dex.mv(ms.id).id == id)
                                .ok_or_else(|| {
                                    format!(
                                        "Can't move: Your {} doesn't have a move matching {w}",
                                        m.name
                                    )
                                })?
                        }
                    })
                };
                // Targeting, by the request move's target type (autoChoose
                // leaves the target to resolveAction).
                let (name, tt) = match chosen {
                    Some(i) => {
                        let mv = self.dex.mv(m.moves[i].id);
                        (mv.name.as_str(), self.move_target_at_choice(mon_ref, mv))
                    }
                    None => ("Struggle", Target::RandomNormal),
                };
                if auto {
                    target = 0;
                } else if self.sides[s].n_active > 1 && tt.needs_choice() {
                    if !self.valid_target_loc(s, slot, target, tt) {
                        if target == 0 {
                            return Err(format!("Can't move: {name} needs a target"));
                        }
                        return Err(format!("Can't move: Invalid target for {name}"));
                    }
                } else if target != 0 {
                    return Err(format!("Can't move: You can't choose a target for {name}"));
                }
                // With no usable move, whatever was chosen is Struggle at a
                // random target, and "mega" is ignored, unchecked.
                let Some(idx) = chosen.filter(|_| !usable.is_empty()) else {
                    return Ok(SlotChoice::Move {
                        slot: STRUGGLE,
                        target: 0,
                        mega: false,
                    });
                };
                if !self.move_usable(mon_ref, idx) {
                    // "[Unavailable choice]" when the request showed the move
                    // usable (hidden by Imprison), "[Invalid choice]" otherwise.
                    let hidden = !self.move_shown_disabled(mon_ref, idx)
                        && !self.revealed(s, slot, Reveal::Disabled);
                    let prefix = if hidden { UNAVAILABLE } else { "" };
                    return Err(format!(
                        "{prefix}Can't move: {}'s {name} is disabled",
                        m.name
                    ));
                }
                if mega && (self.can_mega_evo(mon_ref).is_none() || self.sides[s].mega_used) {
                    return Err(format!("Can't move: {} can't mega evolve", m.name));
                }
                Ok(SlotChoice::Move {
                    slot: idx,
                    target,
                    mega,
                })
            }
            other => Err(format!("Unrecognized choice {other}")),
        }
    }

    fn parse_switch_target(&self, s: usize, word: Option<&str>) -> Result<SlotChoice, String> {
        let side = &self.sides[s];
        let w = word.ok_or("Can't switch: no Pokémon given")?;
        let idx = match w.parse::<usize>() {
            Ok(k) if k >= 1 && k <= side.mons.len() => k - 1,
            _ => side
                .mons
                .iter()
                .position(|m| to_id(&m.name) == to_id(w))
                .ok_or_else(|| format!("Can't switch: invalid Pokémon {w}"))?,
        };
        if idx < side.n_active {
            return Err("Can't switch: You can't switch to an active Pokémon".into());
        }
        if side.mons[idx].fainted {
            return Err("Can't switch: You can't switch to a fainted Pokémon".into());
        }
        Ok(SlotChoice::Switch { index: idx })
    }

    fn move_usable(&self, r: MonRef, idx: usize) -> bool {
        !self.move_disabled(r, idx)
    }

    /// Showdown `validTargetLoc` for doubles.
    #[inline(always)]
    pub(super) fn valid_target_loc(&self, s: usize, slot: usize, loc: i8, target: Target) -> bool {
        let n = self.sides[s].n_active as i8;
        if loc == 0 || loc.abs() > n {
            return false;
        }
        let source_loc = -(slot as i8 + 1);
        let is_self = loc == source_loc;
        let is_foe = loc > 0;
        let across = -(n + 1 - loc);
        let adjacent = if loc > 0 {
            (across - source_loc).abs() <= 1
        } else {
            (loc - source_loc).abs() == 1
        };
        match target {
            Target::RandomNormal | Target::Scripted | Target::Normal => adjacent,
            Target::AdjacentAlly => adjacent && !is_foe,
            Target::AdjacentAllyOrSelf => (adjacent && !is_foe) || is_self,
            Target::AdjacentFoe => adjacent && is_foe,
            Target::Any => !is_self,
            _ => false,
        }
    }

    /// Showdown `canMegaEvo` (Champions): the held item's Mega Stone for this species.
    pub fn can_mega_evo(&self, r: MonRef) -> Option<SpeciesId> {
        let m = self.mon(r);
        if m.mega_evolved {
            return None;
        }
        self.dex.mega_forme(m.item?, m.base_species)
    }

    /// The request a side should answer now, in Showdown's JSON shape.
    pub fn request_json(&self, s: usize) -> Option<Value> {
        let side_data = self.side_request_data(s);
        match self.request {
            RequestState::Ended => None,
            RequestState::TeamPreview => {
                let size = self
                    .dex
                    .format
                    .picked_team_size
                    .unwrap_or(self.sides[s].mons.len());
                if self.sides[s].team_preview_choice.is_some() {
                    return Some(json!({"wait": true, "side": side_data, "rqid": self.rqid}));
                }
                Some(
                    json!({"teamPreview": true, "maxChosenTeamSize": size, "side": side_data, "rqid": self.rqid}),
                )
            }
            RequestState::Switch => {
                if !self.force_switch[s].iter().any(|&f| f) || self.sides[s].choice.is_some() {
                    return Some(json!({"wait": true, "side": side_data, "rqid": self.rqid}));
                }
                let n = self.sides[s].n_active;
                let flags: Vec<bool> = (0..n).map(|i| self.force_switch[s][i]).collect();
                Some(json!({"forceSwitch": flags, "side": side_data, "rqid": self.rqid}))
            }
            RequestState::Move => {
                if self.sides[s].choice.is_some() || !self.side_has_actives(s) {
                    return Some(json!({"wait": true, "side": side_data, "rqid": self.rqid}));
                }
                let mut active = Vec::new();
                for slot in 0..self.sides[s].n_active {
                    let Some(r) = self.active(s, slot) else {
                        continue;
                    };
                    let m = self.mon(r);
                    if let Some(lock) = self.locked_move(r) {
                        // getMoveRequestData for a locked Pokémon: one move, trapped.
                        let (name, id) = match lock {
                            crate::moves::LockedMove::Recharge => {
                                ("Recharge".to_string(), "recharge".to_string())
                            }
                            crate::moves::LockedMove::Move(mv) => {
                                (self.dex.mv(mv).name.clone(), self.dex.mv(mv).id.clone())
                            }
                        };
                        active.push(json!({"moves": [{"move": name, "id": id}], "trapped": true}));
                        continue;
                    }
                    let mut moves: Vec<Value> = m
                        .moves
                        .iter()
                        .enumerate()
                        .map(|(i, ms)| {
                            let mv = self.dex.mv(ms.id);
                            json!({
                                "move": mv.name, "id": mv.id, "pp": ms.pp, "maxpp": ms.max_pp,
                                "target": self.move_target_at_choice(r, mv).id(), "disabled": self.move_shown_disabled(r, i),
                            })
                        })
                        .collect();
                    // A refused choice revealed the stored disabled flags
                    // (updateDisabledRequest).
                    if self.revealed(s, slot, Reveal::Disabled) {
                        for (mv, ms) in moves.iter_mut().zip(&m.moves) {
                            if ms.disabled {
                                mv["disabled"] = json!(true);
                            }
                        }
                    }
                    // getMoveRequestData: with no move shown usable, Struggle is the only one.
                    if (0..m.moves.len()).all(|i| self.move_shown_disabled(r, i)) {
                        moves = vec![
                            json!({"move": "Struggle", "id": "struggle", "target": "randomNormal", "disabled": false}),
                        ];
                    }
                    let mut entry = json!({"moves": moves});
                    // getMoveRequestData: a hidden trap shows only to the
                    // side's other Pokémon; the last active is told it may be trapped.
                    // The last active is told a foe's Imprison may disable
                    // moves it hasn't revealed, until a refused choice does.
                    let imprisoned_by_foe = self
                        .foes_of(r)
                        .into_iter()
                        .any(|f| self.mon(f).vol.imprison);
                    if imprisoned_by_foe
                        && self.is_last_active(r)
                        && !self.revealed(s, slot, Reveal::Disabled)
                    {
                        entry["maybeDisabled"] = json!(true);
                        entry["maybeLocked"] = json!(true);
                    }
                    let can_switch = (self.sides[s].n_active..self.sides[s].mons.len())
                        .any(|i| !self.sides[s].mons[i].fainted);
                    if can_switch {
                        let revealed = self.revealed(s, slot, Reveal::Trapped);
                        if m.trapped == Trapped::Yes
                            || (m.trapped == Trapped::Hidden
                                && (!self.is_last_active(r) || revealed))
                        {
                            entry["trapped"] = json!(true);
                        } else if m.maybe_trapped && self.is_last_active(r) {
                            entry["maybeTrapped"] = json!(true);
                        }
                    }
                    if !self.sides[s].mega_used && self.can_mega_evo(r).is_some() {
                        entry["canMegaEvo"] = json!(true);
                    }
                    active.push(entry);
                }
                Some(json!({"active": active, "side": side_data, "rqid": self.rqid}))
            }
        }
    }

    fn side_request_data(&self, s: usize) -> Value {
        let side = &self.sides[s];
        let pokemon: Vec<Value> = side
            .mons
            .iter()
            .enumerate()
            .map(|(i, m)| {
                // baseStoredStats: its own, also while transformed, and
                // without Speed Swap and the splits.
                let st = self.base_stored_stats((s, i));
                let stats = json!({
                    "atk": st[1], "def": st[2], "spa": st[3], "spd": st[4], "spe": st[5],
                });
                json!({
                    "ident": format!("{}: {}", side.id(), m.name),
                    "details": self.details(s, i),
                    "condition": secret_health(m.hp, m.maxhp, m.status.id()),
                    "active": m.is_active,
                    "stats": stats,
                    "moves": m.moves.iter().map(|ms| self.dex.mv(ms.id).id.clone()).collect::<Vec<_>>(),
                    "baseAbility": self.dex.ability(m.base_ability).id,
                    "item": m.item.map(|it| self.dex.item(it).id.clone()).unwrap_or_default(),
                    "pokeball": "pokeball",
                    "ability": self.dex.ability(m.ability).id,
                    "commanding": false,
                    "reviving": m.is_active && m.position < 2 && side.revival[m.position],
                })
            })
            .collect();
        json!({"name": side.name, "id": side.id(), "pokemon": pokemon})
    }

    /// Legal options for one active slot of a side, ignoring the other slot.
    pub fn slot_options(&self, s: usize, slot: usize) -> Vec<SlotChoice> {
        let mut out = Vec::with_capacity(self.slot_options_bound(s, slot));
        self.slot_options_with(s, slot, &mut |c| out.push(c));
        out
    }

    /// An upper bound on `slot_options(s, slot).len()`: every move at every
    /// target, with and without Mega Evolution, then Struggle and the
    /// switches (or Pass).
    fn slot_options_bound(&self, s: usize, slot: usize) -> usize {
        let side = &self.sides[s];
        let moves = self.active(s, slot).map_or(0, |r| self.mon(r).moves.len());
        moves * 2 * (2 * side.n_active) + side.mons.len() + 1
    }

    /// `slot_options`, handing each option to `push` in order.
    fn slot_options_with(&self, s: usize, slot: usize, push: &mut impl FnMut(SlotChoice)) {
        let side = &self.sides[s];
        let bench = || (side.n_active..side.mons.len()).filter(|&i| !side.mons[i].fainted);
        if self.request == RequestState::Switch {
            if !self.force_switch_flag(s, slot) {
                return push(SlotChoice::Pass);
            }
            if side.revival[slot] {
                // Revival Blessing: a fainted teammate, wherever it is.
                let mut any = false;
                for i in (0..side.mons.len()).filter(|&i| side.mons[i].fainted) {
                    any = true;
                    push(SlotChoice::Switch { index: i });
                }
                if !any {
                    push(SlotChoice::Pass);
                }
                return;
            }
            for i in bench() {
                push(SlotChoice::Switch { index: i });
            }
            return push(SlotChoice::Pass);
        }
        let Some(r) = self.active(s, slot) else {
            return push(SlotChoice::Pass);
        };
        let m = self.mon(r);
        if m.fainted {
            return push(SlotChoice::Pass);
        }
        // A locked Pokémon (recharging, or striking with a charged move) has
        // one choice, "move 1": no target, no Mega Evolution, no switch.
        if self.locked_move(r).is_some() {
            return push(SlotChoice::Move {
                slot: 0,
                target: 0,
                mega: false,
            });
        }
        // Every move at every target, with and without Mega Evolution, then
        // Struggle or the switches.
        let can_mega = !side.mega_used && self.can_mega_evo(r).is_some();
        // Moves the Pokémon can use. Imprison's hidden disables leave others
        // in the last active Pokémon's request, but Showdown refuses them.
        // `move_disabled`, with Imprison's test of the foes made once.
        let imprison = self.foe_side_imprisons(s);
        let usable = (0..m.moves.len()).filter(|&i| {
            !(self.move_disabled_openly(r, i) || (imprison && self.move_imprisoned(r, i)))
        });
        // The target locations of each target type met so far: they depend
        // only on the type here.
        type Locs = ArrayVec<i8, { 2 * MAX_ACTIVE_PER_SIDE }>;
        let mut by_type: ArrayVec<(Target, Locs), 4> = ArrayVec::new();
        let mut any_usable = false;
        for i in usable {
            any_usable = true;
            let mv = self.dex.mv(m.moves[i].id);
            let tt = self.move_target_at_choice(r, mv);
            // Target locations: 1..=n on the foe side, -1..=-n on the user's.
            let mut targets = Locs::new();
            if side.n_active > 1 && tt.needs_choice() {
                if let Some((_, known)) = by_type.iter().find(|(t, _)| *t == tt) {
                    targets = known.clone();
                } else {
                    let n = side.n_active as i8;
                    for t in (1..=n).chain((1..=n).map(|x| -x)) {
                        if self.target_ok(s, slot, t, tt) {
                            targets.push(t);
                        }
                    }
                    if !by_type.is_full() {
                        by_type.push((tt, targets.clone()));
                    }
                }
            } else {
                targets.push(0);
            }
            for &t in &targets {
                push(SlotChoice::Move {
                    slot: i,
                    target: t,
                    mega: false,
                });
                if can_mega {
                    push(SlotChoice::Move {
                        slot: i,
                        target: t,
                        mega: true,
                    });
                }
            }
        }
        // A trapped Pokémon can't switch (Showdown refuses it even when the
        // request hid the trap).
        if !any_usable {
            // No usable move: Struggle, which ignores "mega".
            push(SlotChoice::Move {
                slot: STRUGGLE,
                target: 0,
                mega: false,
            });
        }
        if m.trapped == crate::state::Trapped::No {
            for i in bench() {
                push(SlotChoice::Switch { index: i });
            }
        }
    }

    /// All legal joint choices for a side's current request.
    pub fn joint_choices(&self, s: usize) -> Vec<Vec<SlotChoice>> {
        let n = self.sides[s].n_active;
        let per_slot: Vec<Vec<SlotChoice>> =
            (0..n).map(|slot| self.slot_options(s, slot)).collect();
        let mut combos: Vec<(usize, Vec<SlotChoice>)> = Vec::new();
        let mut current = Vec::with_capacity(n);
        fn rec(
            per_slot: &[Vec<SlotChoice>],
            depth: usize,
            current: &mut Vec<SlotChoice>,
            out: &mut Vec<(usize, Vec<SlotChoice>)>,
        ) {
            if depth == per_slot.len() {
                let mut switches = Vec::new();
                let mut megas = 0;
                for c in current.iter() {
                    match c {
                        SlotChoice::Switch { index } => switches.push(*index),
                        SlotChoice::Move { mega: true, .. } => megas += 1,
                        _ => {}
                    }
                }
                let mut sorted = switches.clone();
                sorted.sort_unstable();
                sorted.dedup();
                if sorted.len() == switches.len() && megas <= 1 {
                    out.push((switches.len(), current.clone()));
                }
                return;
            }
            for c in &per_slot[depth] {
                current.push(*c);
                rec(per_slot, depth + 1, current, out);
                current.pop();
            }
        }
        rec(&per_slot, 0, &mut current, &mut combos);
        if self.request == RequestState::Switch && !combos.is_empty() {
            let most = combos.iter().map(|(k, _)| *k).max().unwrap();
            combos.retain(|(k, _)| *k == most);
        }
        combos.into_iter().map(|(_, c)| c).collect()
    }

    /// A random legal joint choice, drawing each slot's option uniformly and
    /// redrawing combinations Showdown rejects (two switches to one
    /// Pokémon, two Mega Evolutions, a forced switch left unfilled). Cheaper
    /// than enumerating `joint_choices`; not uniform over joint choices.
    pub fn sample_joint_choice(&self, s: usize, rng: &mut crate::rng::Rng) -> Vec<SlotChoice> {
        let n = self.sides[s].n_active;
        // Each slot's options, on the stack when they fit (always, for
        // teams of at most six with four moves each), filled in place.
        let mut inline: [ArrayVec<SlotChoice, SLOT_OPTIONS_INLINE>; MAX_ACTIVE_PER_SIDE] =
            Default::default();
        let mut heap: [Vec<SlotChoice>; MAX_ACTIVE_PER_SIDE] = Default::default();
        for slot in 0..n {
            if self.slot_options_bound(s, slot) <= SLOT_OPTIONS_INLINE {
                let o = &mut inline[slot];
                self.slot_options_with(s, slot, &mut |c| o.push(c));
            } else {
                let o = &mut heap[slot];
                self.slot_options_with(s, slot, &mut |c| o.push(c));
            }
        }
        let options = |slot: usize| -> &[SlotChoice] {
            if heap[slot].is_empty() {
                &inline[slot]
            } else {
                &heap[slot]
            }
        };
        let bench = (n..self.sides[s].mons.len())
            .filter(|&i| !self.sides[s].mons[i].fainted)
            .count();
        let forced = (0..n)
            .filter(|&slot| self.force_switch_flag(s, slot))
            .count();
        let want_switches = if self.request == RequestState::Switch {
            forced.min(bench)
        } else {
            0
        };
        loop {
            let pick: ArrayVec<SlotChoice, MAX_ACTIVE_PER_SIDE> = (0..n)
                .map(|slot| {
                    let o = options(slot);
                    o[rng.random(o.len() as u32) as usize]
                })
                .collect();
            let mut switches: ArrayVec<usize, MAX_ACTIVE_PER_SIDE> = ArrayVec::new();
            let mut megas = 0;
            for c in &pick {
                match *c {
                    SlotChoice::Switch { index } => switches.push(index),
                    SlotChoice::Move { mega: true, .. } => megas += 1,
                    _ => {}
                }
            }
            let distinct = switches
                .iter()
                .enumerate()
                .all(|(i, x)| !switches[..i].contains(x));
            if distinct && megas <= 1 && switches.len() >= want_switches {
                return pick.to_vec();
            }
        }
    }

    /// Showdown choice string for a structured joint choice.
    pub fn choice_string(&self, choice: &[SlotChoice]) -> String {
        choice
            .iter()
            .map(|c| match *c {
                SlotChoice::Pass => "pass".to_string(),
                SlotChoice::Switch { index } => format!("switch {}", index + 1),
                SlotChoice::Move { slot, target, mega } => {
                    let mut s = format!("move {}", if slot >= RECHARGE { 1 } else { slot + 1 });
                    if target != 0 {
                        s.push_str(&format!(" {target}"));
                    }
                    if mega {
                        s.push_str(" mega");
                    }
                    s
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Choose a random legal option (team preview: a random order).
    pub fn random_choice(&mut self, s: usize, rng: &mut crate::rng::Rng) -> String {
        if self.request == RequestState::TeamPreview {
            let n = self.sides[s].mons.len();
            let mut order: Vec<usize> = (1..=n).collect();
            rng.shuffle(&mut order, 0, n);
            return format!(
                "team {}",
                order.iter().map(|x| x.to_string()).collect::<String>()
            );
        }
        let choices = self.joint_choices(s);
        let c = &choices[rng.random(choices.len() as u32) as usize];
        self.choice_string(c)
    }
}

/// How many options of one slot `sample_joint_choice` keeps on the stack:
/// four moves at four targets with and without Mega Evolution, then six
/// Pokémon and Struggle, fit.
const SLOT_OPTIONS_INLINE: usize = 48;
