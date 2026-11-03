//! Importing a projection: rebuilding a battle from a Showdown state.

use std::sync::Arc;

use serde_json::Value;

use super::parse_mref;
use super::volatiles::{import_volatiles, known_volatile, residual_vol};
use crate::battle::Battle;
use crate::dex::team::{champions_max_pp, champions_stats};
use crate::dex::types::*;
use crate::dex::{Dex, Nature};
use crate::state::*;

/// The engine's volatiles from a projection's `volatiles` object (the ids
/// `import_mon` accepts; others are ignored). The stamps of their order are
/// left at 0.
pub(crate) fn projected_volatiles(dex: &Dex, vols: &Value) -> Volatiles {
    // twoturnmove's charging move also has a volatile of its own id.
    let two_turn = vols["twoturnmove"]["move"]
        .as_str()
        .and_then(|m| dex.move_id(m));
    let charging = two_turn.filter(|&id| vols.get(dex.mv(id).id.as_str()).is_some());
    let mut vol = Volatiles {
        flinch: vols.get("flinch").is_some(),
        confusion: vols["confusion"]["time"].as_u64().unwrap_or(0) as u8,
        protect: vols
            .as_object()
            .and_then(|o| o.keys().find_map(|k| crate::kinds::ProtectKind::from_id(k))),
        stall_counter: vols["stall"]["counter"].as_u64().unwrap_or(0) as u16,
        stall_duration: vols["stall"]["duration"].as_u64().unwrap_or(0) as u8,
        choice_lock: vols["choicelock"]["move"]
            .as_str()
            .and_then(|m| dex.move_id(m)),
        flash_fire: vols.get("flashfire").is_some(),
        unburden: vols.get("unburden").is_some(),
        // The multiplier is 1.5 per Helping Hand.
        helping_hand: match vols["helpinghand"]["multiplier"].as_f64() {
            Some(x) => (x.ln() / 1.5f64.ln()).round().max(1.0) as u8,
            None if vols.get("helpinghand").is_some() => 1,
            None => 0,
        },
        follow_me: vols.get("followme").is_some(),
        rage_powder: vols.get("ragepowder").is_some(),
        throat_chop: vols["throatchop"]["duration"].as_u64().unwrap_or(0) as u8,
        taunt: vols["taunt"]["duration"].as_u64().unwrap_or(0) as u8,
        must_recharge: if vols.get("mustrecharge").is_some() {
            vols["mustrecharge"]["duration"].as_u64().unwrap_or(1) as u8
        } else {
            0
        },
        two_turn,
        two_turn_duration: vols["twoturnmove"]["duration"].as_u64().unwrap_or(0) as u8,
        charging,
        charging_target_loc: charging
            .map(|c| {
                vols[dex.mv(c).id.as_str()]["targetLoc"]
                    .as_i64()
                    .unwrap_or(0) as i8
            })
            .unwrap_or(0),
        charging_duration: charging
            .map(|c| {
                vols[dex.mv(c).id.as_str()]["duration"]
                    .as_u64()
                    .unwrap_or(0) as u8
            })
            .unwrap_or(0),
        encore: vols["encore"]["move"].as_str().and_then(|m| dex.move_id(m)),
        encore_duration: vols["encore"]["duration"].as_u64().unwrap_or(0) as u8,
        disable: vols["disable"]["move"]
            .as_str()
            .and_then(|m| dex.move_id(m)),
        disable_duration: vols["disable"]["duration"].as_u64().unwrap_or(0) as u8,
        imprison: vols.get("imprison").is_some(),
        yawn: vols["yawn"]["duration"].as_u64().unwrap_or(0) as u8,
        glaive_rush: vols.get("glaiverush").is_some(),
        roost: vols.get("roost").is_some(),
        perish: vols["perishsong"]["duration"].as_u64().unwrap_or(0) as u8,
        heal_block: vols["healblock"]["duration"].as_u64().unwrap_or(0) as u8,
        substitute: vols["substitute"]["hp"].as_u64().unwrap_or(0) as u16,
        added: Default::default(),
        ..Default::default()
    };
    import_volatiles(&mut vol, vols, dex);
    vol
}

fn import_mon(dex: &Dex, v: &Value, position: usize, stamp: &mut u32) -> Result<Mon, String> {
    let species = dex
        .species_id(v["species"].as_str().unwrap_or(""))
        .ok_or("unknown species in projection")?;
    let base_species = dex
        .species_id(v["baseSpecies"].as_str().unwrap_or(""))
        .unwrap_or(species);
    let set = &v["set"];
    let nature: Nature = dex.nature(set["nature"].as_str().unwrap_or(""));
    let mut points = [0u16; 6];
    if let Some(evs) = set["evs"].as_object() {
        for st in Stat::ALL {
            points[st as usize] = evs.get(st.id()).and_then(|x| x.as_u64()).unwrap_or(0) as u16;
        }
    }
    let ss = &v["storedStats"];
    let maxhp = v["maxhp"].as_u64().unwrap_or(1) as u16;
    let stats = [
        maxhp,
        ss["atk"].as_u64().unwrap_or(0) as u16,
        ss["def"].as_u64().unwrap_or(0) as u16,
        ss["spa"].as_u64().unwrap_or(0) as u16,
        ss["spd"].as_u64().unwrap_or(0) as u16,
        ss["spe"].as_u64().unwrap_or(0) as u16,
    ];
    let mut types: Vec<TypeId> = v["types"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| dex.type_id(t.as_str()?)).collect())
        .unwrap_or_default();
    // `getTypes()` ends with the added type, which `addedType` names.
    let added_type = v["addedType"]
        .as_str()
        .and_then(|t| dex.type_id(t))
        .unwrap_or(NO_TYPE);
    if added_type != NO_TYPE && types.len() > 1 && types.last() == Some(&added_type) {
        types.pop();
    }
    let status = Status::from_id(v["status"].as_str().unwrap_or("")).unwrap_or(Status::None);
    let st = &v["statusState"];
    let mut boosts = [0i8; 7];
    for b in Boost::ALL {
        boosts[b as usize] = v["boosts"][b.id()].as_i64().unwrap_or(0) as i8;
    }
    let move_slots = |key: &str| -> Vec<MoveSlot> {
        v[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| {
                        let id = dex.move_id(m["id"].as_str()?)?;
                        Some(MoveSlot {
                            id,
                            pp: m["pp"].as_u64().unwrap_or(0) as u8,
                            max_pp: m["maxpp"].as_u64().map(|x| x as u8).unwrap_or(
                                champions_max_pp(dex.mv(id).pp, dex.mv(id).no_pp_boosts),
                            ),
                            disabled: m["disabled"].as_bool().unwrap_or(false),
                            used: m["used"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let moves = move_slots("moves");
    let bs = &v["baseStoredStats"];
    let transformed = (v["transformed"].as_bool() == Some(true)).then(|| {
        let g = |k: &str| bs[k].as_u64().unwrap_or(0) as u16;
        Box::new(crate::transform::Transformed {
            moves: move_slots("baseMoveSlots"),
            stats: [maxhp, g("atk"), g("def"), g("spa"), g("spd"), g("spe")],
        })
    });
    let vols = &v["volatiles"];
    // twoturnmove's charging move also has a volatile of its own id.
    let two_turn = vols["twoturnmove"]["move"]
        .as_str()
        .and_then(|m| dex.move_id(m));
    let charging = two_turn.filter(|&id| vols.get(dex.mv(id).id.as_str()).is_some());
    for id in vols.as_object().into_iter().flatten().map(|(k, _)| k) {
        let known = matches!(
            id.as_str(),
            "flinch"
                | "confusion"
                | "stall"
                | "choicelock"
                | "flashfire"
                | "unburden"
                | "helpinghand"
                | "followme"
                | "ragepowder"
                | "throatchop"
                | "taunt"
                | "mustrecharge"
                | "twoturnmove"
                | "encore"
                | "disable"
                | "imprison"
                | "yawn"
                | "glaiverush"
                | "roost"
                | "perishsong"
                | "healblock"
                | "substitute"
        ) || known_volatile(id)
            || matches!(
                id.as_str(),
                "partiallytrapped" | "leechseed" | "aquaring" | "ingrain" | "saltcure" | "uproar"
            )
            || id == "stockpile"
            || id == "chillyreception"
            || id == "magnetrise"
            || id == "smackdown"
            || charging.map(|c| dex.mv(c).id == *id).unwrap_or(false);
        if !known && crate::kinds::ProtectKind::from_id(id).is_none() {
            return Err(format!("unsupported volatile {id}"));
        }
    }
    let mut vol = projected_volatiles(dex, vols);
    // The order the volatiles with handlers were added in.
    let charging_id = vol
        .charging
        .filter(|_| vol.charging_duration > 0)
        .map(|c| dex.mv(c).id.as_str());
    for id in v["volatileOrder"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
    {
        if let Some(rv) = residual_vol(id, charging_id) {
            *stamp += 1;
            vol.added[rv as usize] = *stamp;
        }
    }
    // Speed Swap, the splits and Power Trick changed the stored stats: the
    // species' own come back when the volatiles clear.
    if transformed.is_none() {
        let sp = dex.species(species);
        let own = champions_stats(&sp.base_stats, &points, nature);
        vol.stats_changed = stats[1..] != own[1..];
    }
    let item = v["item"]
        .as_str()
        .filter(|s| !s.is_empty())
        .and_then(|s| dex.item_id(s));
    let ability = dex
        .ability_id(v["ability"].as_str().unwrap_or("noability"))
        .unwrap_or(0);
    let name = v["name"].as_str().unwrap_or("").to_string();
    Ok(Mon {
        name: if name.is_empty() {
            dex.species(species).name.as_str().into()
        } else {
            name.into()
        },
        species,
        base_species,
        types: [
            types.first().copied().unwrap_or(0),
            types.get(1).copied().unwrap_or(NO_TYPE),
        ],
        level: set["level"].as_u64().unwrap_or(50) as u8,
        gender: set["gender"].as_str().unwrap_or("").to_string(),
        shiny: set["shiny"].as_bool().unwrap_or(false),
        revived: false,
        hurt_this_turn: v["hurtThisTurn"].as_u64().map(|h| h as u16),
        nature_name: crate::dex::team::nature_name(set["nature"].as_str().unwrap_or("")),
        nature_plus: nature.plus,
        nature_minus: nature.minus,
        points,
        stats,
        hp: v["hp"].as_u64().unwrap_or(0) as u16,
        maxhp,
        status,
        status_time: st["time"].as_u64().unwrap_or(0) as u8,
        status_start_time: st["startTime"].as_u64().unwrap_or(0) as u8,
        // The fields tell which status set the state (sleep and freeze alike).
        status_state: if st.get("startTime").is_some() {
            if status == Status::Frz {
                Status::Frz
            } else {
                Status::Slp
            }
        } else if st.get("stage").is_some() {
            Status::Tox
        } else if matches!(status, Status::Psn | Status::Brn | Status::Par) {
            status
        } else {
            Status::None
        },
        toxic_stage: st["stage"].as_u64().unwrap_or(0) as u8,
        boosts,
        item,
        last_item: v["lastItem"]
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(|s| dex.item_id(s)),
        used_item_this_turn: v["usedItemThisTurn"].as_bool().unwrap_or(false),
        ate_berry: v["ateBerry"].as_bool().unwrap_or(false),
        syrup_triggered: v["syrupTriggered"].as_bool().unwrap_or(false),
        ability,
        base_ability: v["baseAbility"]
            .as_str()
            .and_then(|a| dex.ability_id(a))
            .unwrap_or(ability),
        ability_state: AbilityState {
            seek: v["abilityState"]["seek"].as_bool(),
            unnerved: v["abilityState"]["unnerved"].as_bool(),
            busted: v["abilityState"]["busted"].as_bool(),
            protean: v["abilityState"]["protean"].as_bool(),
            libero: v["abilityState"]["libero"].as_bool(),
            checked_berserk: v["abilityState"]["checkedBerserk"].as_bool(),
            ending: v["abilityState"]["ending"].as_bool(),
            gluttony: v["abilityState"]["gluttony"].as_bool(),
            cud_berry: v["abilityState"]["berry"]
                .as_str()
                .and_then(|i| dex.item_id(i)),
            cud_counter: v["abilityState"]["counter"].as_i64().map(|c| c as i8),
            berry_weaken: v["abilityState"]["berryWeaken"].as_bool(),
            opp_boosts: v["abilityState"]["oppBoosts"].as_array().map(|pairs| {
                let mut mb = crate::dex::types::MoveBoosts::default();
                let mut seen = Vec::new();
                for p in pairs {
                    if let Some(b) = Boost::ALL.iter().find(|b| Some(b.id()) == p[0].as_str()) {
                        mb.vals[*b as usize] = p[1].as_i64().unwrap_or(0) as i8;
                        seen.push(*b as u8);
                    }
                }
                // The seen stats first, in that order, then the rest.
                let rest: Vec<u8> = (0..7u8).filter(|i| !seen.contains(i)).collect();
                for (k, i) in seen.iter().chain(rest.iter()).enumerate() {
                    mb.order[k] = *i;
                }
                mb
            }),
            fallen: v["abilityState"]["fallen"].as_u64().map(|x| x as u8),
        },
        // The rank among the active Pokémon stands in for effectOrder.
        ability_order: v["abilityOrder"].as_u64().unwrap_or(0) as u32,
        moves,
        vol,
        last_move: v["lastMove"].as_str().and_then(|m| dex.move_id(m)),
        last_move_target_loc: v["lastMoveTargetLoc"].as_i64().unwrap_or(0) as i8,
        move_this_turn_result: v["moveThisTurnResult"].as_bool(),
        move_last_turn_result: v["moveLastTurnResult"].as_bool(),
        position,
        is_active: v["isActive"].as_bool().unwrap_or(false),
        fainted: v["fainted"].as_bool().unwrap_or(false),
        faint_queued: false,
        switch_flag: false,
        force_switch_flag: false,
        switch_from_move: None,
        skip_before_switch_out: false,
        mega_evolved: dex.species(species).is_mega,
        // Recomputed after import (the projection doesn't hold them).
        trapped: Trapped::No,
        maybe_trapped: false,
        times_attacked: v["timesAttacked"].as_u64().unwrap_or(0) as u16,
        active_turns: v["activeTurns"].as_u64().unwrap_or(1) as u16,
        moved_this_turn: false,
        active_move_actions: v["activeMoveActions"].as_u64().unwrap_or(1) as u16,
        speed: 0,
        illusion: v["illusion"].as_str().map(str::to_string),
        hero_message_displayed: v["heroMessageDisplayed"].as_bool().unwrap_or(false),
        added_type,
        transformed,
        attacked_by: Battle::import_attacked_by(dex, v),
        stats_raised_this_turn: v["statsRaisedThisTurn"].as_bool().unwrap_or(false),
        stats_lowered_this_turn: v["statsLoweredThisTurn"].as_bool().unwrap_or(false),
        set_species: v["setSpecies"]
            .as_str()
            .and_then(|s| dex.species_id(s))
            .unwrap_or(0),
    })
}

impl Battle {
    /// Build a battle from a Showdown projection taken at a move or switch
    /// request. `seed` seeds the engine's own RNG.
    pub fn from_projection(
        dex: Arc<Dex>,
        p: &Value,
        names: [&str; 2],
        seed: u64,
    ) -> Result<Battle, String> {
        // At team preview Showdown's queue holds only its battle-start
        // action, which the engine queues itself when the teams are chosen.
        let preview = p["requestState"].as_str() == Some("teampreview");
        if !preview && p["queueLength"].as_u64().unwrap_or(0) > 0 {
            return Err("can't import a mid-turn state with actions still queued".into());
        }
        let mut b = Battle::skeleton(dex.clone(), names, seed);
        b.log.clear();
        // Ability ranks (1 to 4) come first; the stamps of the volatiles added later follow them.
        b.vol_stamp = 8;
        b.turn = p["turn"].as_u64().unwrap_or(0) as u32;
        b.turn_state.last_move = p["lastMove"].as_str().and_then(|m| dex.move_id(m));
        b.ended = p["ended"].as_bool().unwrap_or(false);
        b.request = match p["requestState"].as_str() {
            Some("move") => RequestState::Move,
            Some("switch") => RequestState::Switch,
            Some("teampreview") => RequestState::TeamPreview,
            _ => {
                if b.ended {
                    RequestState::Ended
                } else {
                    RequestState::Move
                }
            }
        };
        let f = &p["field"];
        if let Some(w) = f["weather"].as_str() {
            b.field.weather =
                Some(Weather::from_id(w).ok_or_else(|| format!("unsupported weather {w}"))?);
            b.field.weather_turns = f["weatherState"]["duration"].as_u64().unwrap_or(0) as u8;
        }
        if let Some(t) = f["terrain"].as_str() {
            b.field.terrain =
                Some(Terrain::from_id(t).ok_or_else(|| format!("unsupported terrain {t}"))?);
            b.field.terrain_turns = f["terrainState"]["duration"].as_u64().unwrap_or(0) as u8;
        }
        for (id, state) in f["pseudoWeather"].as_object().into_iter().flatten() {
            match id.as_str() {
                "trickroom" => b.field.trick_room = state["duration"].as_u64().unwrap_or(0) as u8,
                "gravity" => b.field.gravity = state["duration"].as_u64().unwrap_or(0) as u8,
                "wonderroom" => {
                    b.field.wonder_room = state["duration"].as_u64().unwrap_or(0) as u8;
                    b.field.wonder_room_armed = !state["target"].is_null();
                }
                "magicroom" => {
                    b.field.magic_room = state["duration"].as_u64().unwrap_or(0) as u8;
                    b.field.magic_room_source = parse_mref(&state["source"]);
                }
                "fairylock" => b.field.fairy_lock = state["duration"].as_u64().unwrap_or(0) as u8,
                other => return Err(format!("unsupported field condition {other}")),
            }
        }
        let sides = p["sides"].as_array().ok_or("missing sides")?;
        for (s, sv) in sides.iter().enumerate().take(2) {
            let mut mons = Vec::new();
            for (i, mv) in sv["pokemon"]
                .as_array()
                .ok_or("missing pokemon")?
                .iter()
                .enumerate()
            {
                mons.push(import_mon(&dex, mv, i, &mut b.vol_stamp)?);
            }
            let side = &mut b.sides[s];
            // In the order they were added: the listed ones first, then the rest (no handler to tie).
            let listed = sv["sideConditionOrder"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str());
            let conds_in_order: Vec<(&String, &Value)> = {
                let table = sv["sideConditions"].as_object();
                let first = listed.filter_map(|id| table.and_then(|t| t.get_key_value(id)));
                let rest = table.into_iter().flatten().filter(|(k, _)| {
                    !sv["sideConditionOrder"]
                        .as_array()
                        .is_some_and(|o| o.iter().any(|x| x.as_str() == Some(k.as_str())))
                });
                first.chain(rest).collect()
            };
            for (id, state) in conds_in_order {
                if id == "toxicspikes" {
                    side.conds.toxic_spikes = state["layers"].as_u64().unwrap_or(1) as u8;
                    side.conds.next_added += 1;
                    side.conds.toxic_spikes_added = side.conds.next_added;
                    continue;
                }
                if crate::projection::volatiles::import_side_cond(
                    &mut b.side_extras[s],
                    &mut side.conds,
                    id,
                    state,
                ) {
                    continue;
                }
                if id == "stealthrock" {
                    side.conds.stealth_rock = true;
                    side.conds.next_added += 1;
                    side.conds.stealth_rock_added = side.conds.next_added;
                    continue;
                }
                let c = SideCond::from_id(id)
                    .ok_or_else(|| format!("unsupported side condition {id}"))?;
                *side.conds.get_mut(c) = state["duration"].as_u64().unwrap_or(0) as u8;
                // The JSON object's key order (alphabetical here) stands in for insertion order.
                side.conds.stamp(c);
            }
            for (i, conds) in sv["slotConditions"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .take(2)
            {
                for id in conds.as_object().into_iter().flatten().map(|(k, _)| k) {
                    match id.as_str() {
                        "revivalblessing" => side.revival[i] = true,
                        "healingwish" | "wish" | "futuremove" => {}
                        _ => return Err(format!("unsupported slot condition {id}")),
                    }
                }
                crate::projection::volatiles::import_slot_conds(&mut b.slot_conds[s][i], conds);
            }
            side.mega_used = mons.iter().any(|m: &Mon| dex.species(m.species).is_mega);
            side.mons = mons;
            side.pokemon_left = sv["pokemonLeft"].as_u64().unwrap_or(0) as usize;
            side.total_fainted = sv["totalFainted"].as_u64().unwrap_or(0) as u8;
            // A fainted Pokémon still occupying its slot counts as there.
            for slot in 0..side.n_active.min(side.mons.len()) {
                let named = sv["active"].get(slot).and_then(|v| v.as_str()).is_some();
                if named {
                    let m = &mut side.mons[slot];
                    // Revival Blessing leaves the revived Pokémon in its slot but not active.
                    m.revived = !m.is_active && !m.fainted;
                    m.is_active = true;
                }
            }
            if let Some(fs) = sv["forceSwitch"].as_array() {
                let flags: Vec<bool> = fs.iter().map(|x| x.as_bool().unwrap_or(false)).collect();
                b.set_force_switch(s, &flags);
            }
        }
        b.mid_turn = b.request == RequestState::Switch;
        b.refresh_speeds();
        // The trap pass of the turn's start, without drawing.
        for r in b.all_active() {
            b.run_trap_pokemon(r, false);
        }
        Ok(b)
    }
}
