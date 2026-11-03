//! State projection in the same shape as `refsim/server.js` `project()`, and
//! import of such a projection, so engine and Showdown states can be compared
//! and the engine can continue from any Showdown position.

mod import;
mod volatiles;

pub(crate) use import::projected_volatiles;

use serde_json::{json, Map, Value};

use crate::battle::log::Viewer;
use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::state::*;
use volatiles::residual_vol;

/// A Pokémon as the projection names it: its side and its position in the team.
pub(crate) fn mref_id(r: MonRef) -> String {
    format!("p{}:{}", r.0 + 1, r.1)
}

/// The Pokémon `mref_id` names.
pub(crate) fn parse_mref(v: &Value) -> Option<MonRef> {
    let (side, pos) = v.as_str()?.strip_prefix('p')?.split_once(':')?;
    Some((
        side.parse::<usize>().ok()?.checked_sub(1)?,
        pos.parse().ok()?,
    ))
}

/// A slot id (`p1a`) as `(side, slot)`.
pub(crate) fn parse_slot(v: &Value) -> Option<(usize, usize)> {
    let b = v.as_str()?.as_bytes();
    Some((
        b.get(1)?.checked_sub(b'1')? as usize,
        b.get(2)?.checked_sub(b'a')? as usize,
    ))
}

fn boosts_json(b: &[i8; 7]) -> Value {
    let mut m = Map::new();
    for x in Boost::ALL {
        m.insert(x.id().into(), json!(b[x as usize]));
    }
    Value::Object(m)
}

impl Battle {
    /// Canonical projection of the full state (see refsim `project()`).
    pub fn projection(&self) -> Value {
        let request_state = match self.request {
            RequestState::TeamPreview => json!("teampreview"),
            RequestState::Move => json!("move"),
            RequestState::Switch => json!("switch"),
            RequestState::Ended => Value::Null,
        };
        let mut ability_orders: Vec<u32> = self
            .sides
            .iter()
            .flat_map(|s| s.mons.iter())
            .filter(|m| m.is_active && !m.fainted && !m.revived)
            .map(|m| m.ability_order)
            .collect();
        ability_orders.sort();
        ability_orders.dedup();
        let sides: Vec<Value> = (0..2)
            .map(|s| {
                let side = &self.sides[s];
                // The side's current request (Showdown `activeRequest`), answered or not.
                let force = if self.request == RequestState::Switch && (0..side.n_active).any(|i| self.force_switch_flag(s, i)) {
                    json!((0..side.n_active).map(|i| self.force_switch_flag(s, i)).collect::<Vec<_>>())
                } else {
                    Value::Null
                };
                let active: Vec<Value> = (0..side.n_active)
                    .map(|slot| {
                        if slot < side.mons.len() && (side.mons[slot].is_active || side.mons[slot].fainted) && self.turn > 0 {
                            json!(&*side.mons[slot].name)
                        } else {
                            Value::Null
                        }
                    })
                    .collect();
                let pokemon: Vec<Value> = side
                    .mons
                    .iter()
                    .map(|m| {
                        let sp = self.dex.species(m.species);
                        let mut status_state = Map::new();
                        match m.status_state {
                            Status::Slp | Status::Frz => {
                                status_state.insert("startTime".into(), json!(m.status_start_time));
                                status_state.insert("time".into(), json!(m.status_time));
                            }
                            Status::Tox => {
                                status_state.insert("stage".into(), json!(m.toxic_stage));
                            }
                            _ => {}
                        }
                        let mut volatiles = Map::new();
                        if m.vol.flinch {
                            volatiles.insert("flinch".into(), json!({"duration": 1}));
                        }
                        if m.vol.confusion > 0 {
                            volatiles.insert("confusion".into(), json!({"time": m.vol.confusion}));
                        }
                        if let Some(p) = m.vol.protect {
                            volatiles.insert(p.id().into(), json!({"duration": 1}));
                        }
                        if m.vol.helping_hand > 0 {
                            // onStart sets 1.5; each onRestart multiplies it by 1.5.
                            let multiplier = 1.5f64.powi(m.vol.helping_hand as i32);
                            volatiles.insert("helpinghand".into(), json!({"duration": 1, "multiplier": multiplier}));
                        }
                        if m.vol.follow_me {
                            volatiles.insert("followme".into(), json!({"duration": 1}));
                        }
                        if m.vol.rage_powder {
                            volatiles.insert("ragepowder".into(), json!({"duration": 1}));
                        }
                        if m.vol.throat_chop > 0 {
                            volatiles.insert("throatchop".into(), json!({"duration": m.vol.throat_chop}));
                        }
                        if m.vol.taunt > 0 {
                            volatiles.insert("taunt".into(), json!({"duration": m.vol.taunt}));
                        }
                        if m.vol.flash_fire {
                            volatiles.insert("flashfire".into(), json!({}));
                        }
                        if m.vol.unburden {
                            volatiles.insert("unburden".into(), json!({}));
                        }
                        if let Some(lock) = m.vol.choice_lock {
                            volatiles.insert("choicelock".into(), json!({"move": self.dex.mv(lock).id}));
                        }
                        if m.vol.must_recharge > 0 {
                            volatiles.insert("mustrecharge".into(), json!({"duration": m.vol.must_recharge}));
                        }
                        if let Some(id) = m.vol.two_turn {
                            volatiles.insert(
                                "twoturnmove".into(),
                                json!({"duration": m.vol.two_turn_duration, "move": self.dex.mv(id).id}),
                            );
                        }
                        if let Some(id) = m.vol.charging {
                            volatiles.insert(
                                self.dex.mv(id).id.clone(),
                                if m.vol.charging_duration > 0 {
                                    json!({"duration": m.vol.charging_duration, "targetLoc": m.vol.charging_target_loc})
                                } else {
                                    json!({"targetLoc": m.vol.charging_target_loc})
                                },
                            );
                        }
                        if let Some(id) = m.vol.encore {
                            volatiles.insert(
                                "encore".into(),
                                json!({"duration": m.vol.encore_duration, "move": self.dex.mv(id).id}),
                            );
                        }
                        if let Some(id) = m.vol.disable {
                            volatiles.insert(
                                "disable".into(),
                                json!({"duration": m.vol.disable_duration, "move": self.dex.mv(id).id}),
                            );
                        }
                        if m.vol.imprison {
                            volatiles.insert("imprison".into(), json!({}));
                        }
                        if m.vol.yawn > 0 {
                            volatiles.insert("yawn".into(), json!({"duration": m.vol.yawn}));
                        }
                        if m.vol.glaive_rush {
                            volatiles.insert("glaiverush".into(), json!({}));
                        }
                        if m.vol.roost {
                            volatiles.insert("roost".into(), json!({"duration": 1}));
                        }
                        if m.vol.perish > 0 {
                            volatiles.insert("perishsong".into(), json!({"duration": m.vol.perish}));
                        }
                        if m.vol.heal_block > 0 {
                            volatiles.insert("healblock".into(), json!({"duration": m.vol.heal_block}));
                        }
                        if m.vol.substitute > 0 {
                            volatiles.insert("substitute".into(), json!({"hp": m.vol.substitute}));
                        }
                        self.project_volatiles(m, &mut volatiles);
                        if m.vol.stall_counter > 0 {
                            volatiles.insert(
                                "stall".into(),
                                json!({"counter": m.vol.stall_counter, "duration": m.vol.stall_duration}),
                            );
                        }
                        let stats = json!({
                            "atk": m.stats[1], "def": m.stats[2], "spa": m.stats[3], "spd": m.stats[4], "spe": m.stats[5],
                        });
                        let types: Vec<&str> = self
                            .types_of((s, m.position))
                            .iter()
                            .filter(|&&t| t != NO_TYPE)
                            .map(|&t| self.dex.type_names[t as usize].as_str())
                            .collect();
                        let mega = if side.mega_used { None } else { self.can_mega_evo((s, m.position)) };
                        let mut ability_state = Map::new();
                        if let Some(x) = m.ability_state.seek {
                            ability_state.insert("seek".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.unnerved {
                            ability_state.insert("unnerved".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.busted {
                            ability_state.insert("busted".into(), json!(x));
                        }
                        crate::projection::volatiles::project_ability_state(&m.ability_state, &mut ability_state);
                        if let Some(x) = m.ability_state.ending {
                            ability_state.insert("ending".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.gluttony {
                            ability_state.insert("gluttony".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.berry_weaken {
                            ability_state.insert("berryWeaken".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.cud_counter {
                            ability_state.insert("counter".into(), json!(x));
                        }
                        if let Some(x) = m.ability_state.fallen {
                            ability_state.insert("fallen".into(), json!(x));
                        }
                        if let Some(item) = m.ability_state.cud_berry {
                            ability_state.insert("berry".into(), json!(self.dex.item(item).id));
                        }
                        // Opportunist's boosts, in the order Showdown first saw each stat.
                        if let Some(mb) = &m.ability_state.opp_boosts {
                            let seen: Vec<Value> = mb
                                .order
                                .iter()
                                .filter(|&&i| mb.vals[i as usize] != 0)
                                .map(|&i| json!([Boost::ALL[i as usize].id(), mb.vals[i as usize]]))
                                .collect();
                            ability_state.insert("oppBoosts".into(), Value::Array(seen));
                        }
                        let p = &m.points;
                        let mut out = json!({
                            "name": &*m.name,
                            "species": sp.name,
                            "baseSpecies": self.dex.species(m.base_species).name,
                            "set": {
                                "nature": m.nature_name,
                                "evs": {"hp": p[0], "atk": p[1], "def": p[2], "spa": p[3], "spd": p[4], "spe": p[5]},
                                "level": m.level,
                                "gender": m.gender,
                                "shiny": m.shiny,
                            },
                            "position": m.position,
                            "isActive": m.is_active && !m.fainted && !m.revived,
                            "hp": m.hp,
                            "maxhp": m.maxhp,
                            "status": if m.status == Status::None { Value::Null } else { json!(m.status.id()) },
                            "statusState": Value::Object(status_state),
                            "fainted": m.fainted,
                            "boosts": boosts_json(&m.boosts),
                            "storedStats": stats,
                            "types": types,
                            "item": m.item.map(|i| json!(self.dex.item(i).id)).unwrap_or(Value::Null),
                            "lastItem": m.last_item.map(|i| json!(self.dex.item(i).id)).unwrap_or(Value::Null),
                            "usedItemThisTurn": m.used_item_this_turn,
                            "syrupTriggered": m.syrup_triggered,
                            "ateBerry": m.ate_berry,
                            "timesAttacked": m.times_attacked,
                            "itemState": {},
                            "ability": self.dex.ability(m.ability).id,
                            "baseAbility": self.dex.ability(m.base_ability).id,
                            "abilityState": Value::Object(ability_state),
                            "moves": m.moves.iter().map(|ms| json!({
                                "id": self.dex.mv(ms.id).id, "pp": ms.pp, "maxpp": ms.max_pp, "disabled": ms.disabled, "used": ms.used,
                            })).collect::<Vec<_>>(),
                            "volatiles": Value::Object(volatiles),
                            "lastMove": m.last_move.map(|id| json!(self.dex.mv(id).id)).unwrap_or(Value::Null),
                            "lastMoveTargetLoc": m.last_move_target_loc,
                            "moveThisTurnResult": m.move_this_turn_result,
                            "moveLastTurnResult": m.move_last_turn_result,
                            "canMegaEvo": mega.map(|id| json!(self.dex.species(id).name)).unwrap_or(Value::Null),
                            "activeMoveActions": m.active_move_actions,
                            "activeTurns": m.active_turns,
                            "illusion": m.illusion,
                            "heroMessageDisplayed": m.hero_message_displayed,
                            "addedType": (m.added_type != NO_TYPE).then(|| self.dex.type_names[m.added_type as usize].clone()),
                        });
                        self.project_mon_extras(m, &mut out);
                        // The volatiles with handlers, in the order they were added.
                        let charging = m.vol.charging.filter(|_| m.vol.charging_duration > 0).map(|c| self.dex.mv(c).id.clone());
                        let mut order: Vec<(u32, String)> = out["volatiles"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .filter_map(|(k, _)| residual_vol(k, charging.as_deref()).map(|rv| (m.vol.added[rv as usize], k.clone())))
                            .collect();
                        order.sort();
                        out["volatileOrder"] = json!(order.into_iter().map(|(_, k)| k).collect::<Vec<_>>());
                        // Ties between abilities go to the older ability state: its rank among the active.
                        let on_field = m.is_active && !m.fainted && !m.revived;
                        out["abilityOrder"] = if on_field {
                            json!(ability_orders.iter().position(|&o| o == m.ability_order).map(|i| i + 1))
                        } else {
                            Value::Null
                        };
                        out["transformed"] = json!(m.transformed.is_some());
                        if let Some(t) = &m.transformed {
                            out["baseMoveSlots"] = json!(t.moves.iter().map(|ms| json!({
                                "id": self.dex.mv(ms.id).id, "pp": ms.pp, "maxpp": ms.max_pp, "disabled": ms.disabled, "used": ms.used,
                            })).collect::<Vec<_>>());
                            out["baseStoredStats"] = json!({
                                "atk": t.stats[1], "def": t.stats[2], "spa": t.stats[3], "spd": t.stats[4], "spe": t.stats[5],
                            });
                        }
                        out
                    })
                    .collect();
                let mut side_conds: Map<String, Value> = SideCond::ALL
                    .iter()
                    .filter(|&&c| side.conds.get(c) > 0)
                    .map(|&c| (c.id().to_string(), json!({"duration": side.conds.get(c)})))
                    .collect();
                if side.conds.stealth_rock {
                    side_conds.insert("stealthrock".into(), json!({}));
                }
                if side.conds.toxic_spikes > 0 {
                    side_conds.insert("toxicspikes".into(), json!({"layers": side.conds.toxic_spikes}));
                }
                self.project_side_conditions(s, &mut side_conds);
                // The conditions with handlers, in the order they were added.
                let a = &self.side_extras[s];
                let mut cond_order: Vec<(u16, String)> = SideCond::ALL
                    .iter()
                    .filter(|&&c| side.conds.get(c) > 0)
                    .map(|&c| (side.conds.added[c as usize], c.id().to_string()))
                    .collect();
                for (on, stamp, id) in [
                    (side.conds.stealth_rock, side.conds.stealth_rock_added, "stealthrock"),
                    (side.conds.toxic_spikes > 0, side.conds.toxic_spikes_added, "toxicspikes"),
                    (a.spikes > 0, a.spikes_added, "spikes"),
                    (a.sticky_web, a.sticky_web_added, "stickyweb"),
                ] {
                    if on {
                        cond_order.push((stamp, id.to_string()));
                    }
                }
                cond_order.sort();
                json!({
                    "id": side.id(),
                    "pokemonLeft": side.pokemon_left,
                    "totalFainted": side.total_fainted,
                    "forceSwitch": force,
                    "sideConditions": Value::Object(side_conds),
                    "sideConditionOrder": cond_order.into_iter().map(|(_, id)| id).collect::<Vec<_>>(),
                    "slotConditions": (0..side.n_active)
                        .map(|slot| {
                            let mut conds = self.project_slot_conditions(s, slot);
                            if side.revival[slot] {
                                conds["revivalblessing"] = json!({"duration": 1, "isSlotCondition": true});
                            }
                            conds
                        })
                        .collect::<Vec<_>>(),
                    "active": active,
                    "pokemon": pokemon,
                })
            })
            .collect();
        json!({
            "turn": self.turn,
            "ended": self.ended,
            "winner": self.winner_name(),
            "prng": self.rng.showdown_seed(),
            "queueLength": self.queue.len(),
            "lastMove": self.turn_state.last_move.map(|m| json!(self.dex.mv(m).id)).unwrap_or(Value::Null),
            "requestState": request_state,
            "field": self.field_projection(),
            "sides": sides,
        })
    }

    fn field_projection(&self) -> Value {
        let f = &self.field;
        let mut pseudo = Map::new();
        if f.trick_room > 0 {
            pseudo.insert("trickroom".into(), json!({"duration": f.trick_room}));
        }
        self.project_pseudo_weather(&mut pseudo);
        self.project_fairy_lock(&mut pseudo);
        json!({
            "weather": f.weather.map(|w| json!(w.id())).unwrap_or(Value::Null),
            "weatherState": if f.weather.is_some() { json!({"duration": f.weather_turns}) } else { json!({}) },
            "terrain": f.terrain.map(|t| json!(t.id())).unwrap_or(Value::Null),
            "terrainState": if f.terrain.is_some() { json!({"duration": f.terrain_turns}) } else { json!({}) },
            "pseudoWeather": Value::Object(pseudo),
        })
    }

    /// Log lines since `from` for a viewer ("p1", "p2", "spectator", "omniscient").
    pub fn log_for(&self, viewer: &str, from: usize) -> Vec<String> {
        let v = match viewer {
            "p1" => Viewer::Side(0),
            "p2" => Viewer::Side(1),
            "omniscient" => Viewer::Omniscient,
            _ => Viewer::Spectator,
        };
        self.log_lines(v, from)
    }
}
