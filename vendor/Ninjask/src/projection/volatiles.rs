//! Which Showdown volatile ids the engine tracks as residual volatiles.

use crate::battle::Battle;
use crate::dex::Dex;
use crate::state::*;
use serde_json::{json, Map, Value};

/// The volatile (indexing `Volatiles::added`) a projected volatile id stands
/// for, among those whose insertion order the engine keeps: `charging` names
/// the charging move's own volatile. Keep in step with `ORDERED_VOLATILES` in
/// `refsim/server.js`.
pub(super) fn residual_vol(id: &str, charging: Option<&str>) -> Option<ResidualVol> {
    use ResidualVol::*;
    Some(match id {
        "throatchop" => ThroatChop,
        "taunt" => Taunt,
        "encore" => Encore,
        "disable" => Disable,
        "yawn" => Yawn,
        "roost" => Roost,
        "perishsong" => Perish,
        "healblock" => HealBlock,
        "flinch" => Flinch,
        "protect" | "spikyshield" | "banefulbunker" | "kingsshield" | "obstruct" => Protect,
        "stall" => Stall,
        "helpinghand" => HelpingHand,
        "followme" => FollowMe,
        "ragepowder" => RagePowder,
        "mustrecharge" => MustRecharge,
        "twoturnmove" => TwoTurnMove,
        "partiallytrapped" => PartialTrap,
        "leechseed" => LeechSeed,
        "aquaring" => AquaRing,
        "ingrain" => Ingrain,
        "saltcure" => SaltCure,
        "chillyreception" => ChillyReception,
        "magnetrise" => MagnetRise,
        "uproar" => Uproar,
        "gem" => Gem,
        "counter" => Counter,
        "beakblast" => BeakBlast,
        "mirrorcoat" => MirrorCoat,
        "focuspunch" => FocusPunch,
        "syrupbomb" => SyrupBomb,
        "fling" => Fling,
        "endure" => Endure,
        "allyswitch" => AllySwitch,
        "curse" => Curse,
        "lockedmove" => LockedMove,
        "charge" => Charge,
        "electrify" => Electrify,
        "lockon" => LockOn,
        "octolock" => Octolock,
        "attract" => Attract,
        _ if Some(id) == charging => Charging,
        _ => return None,
    })
}

/// A slot's conditions from a Showdown projection.
pub(crate) fn import_slot_conds(c: &mut SlotConds, conds: &Value) {
    let letter = |v: &Value| {
        v.as_str()
            .and_then(|s| s.as_bytes().get(2))
            .map(|b| b.wrapping_sub(b'a') as usize)
    };
    if let Some(hw) = conds.get("healingwish") {
        c.healing_wish = letter(&hw["sourceSlot"]);
        c.healing_wish_detached = hw.get("detached").is_some();
    }
    if let Some(f) = conds.get("futuremove") {
        if let Some(source) = crate::projection::parse_mref(&f["source"]) {
            c.future = Some(FutureState {
                ending_turn: f["endingTurn"].as_u64().unwrap_or(0) as u32,
                source,
            });
        }
    }
    if let Some(w) = conds.get("wish") {
        c.wish = Some(WishState {
            hp: w["hp"].as_f64().unwrap_or(0.0),
            starting_turn: w["startingTurn"].as_u64().unwrap_or(0) as u32,
            source_slot: letter(&w["sourceSlot"]).unwrap_or(0),
            wisher: w["wisher"].as_str().unwrap_or("").to_string(),
        });
    }
}

/// Side conditions from a Showdown projection; whether `id` is one.
pub(crate) fn import_side_cond(
    a: &mut SideExtras,
    conds: &mut crate::state::SideConditions,
    id: &str,
    state: &Value,
) -> bool {
    match id {
        "safeguard" => a.safeguard = state["duration"].as_u64().unwrap_or(0) as u8,
        "spikes" => {
            a.spikes = state["layers"].as_u64().unwrap_or(1) as u8;
            conds.next_added += 1;
            a.spikes_added = conds.next_added;
        }
        "stickyweb" => {
            a.sticky_web = true;
            conds.next_added += 1;
            a.sticky_web_added = conds.next_added;
        }
        _ => return false,
    }
    true
}

/// Ability state in the projection (Showdown `abilityState`).
pub(crate) fn project_ability_state(a: &AbilityState, out: &mut Map<String, Value>) {
    if let Some(x) = a.protean {
        out.insert("protean".into(), json!(x));
    }
    if let Some(x) = a.libero {
        out.insert("libero".into(), json!(x));
    }
    if let Some(x) = a.checked_berserk {
        out.insert("checkedBerserk".into(), json!(x));
    }
}

impl Battle {
    /// Extra fields in a Pokémon's projection.
    pub(crate) fn project_mon_extras(&self, m: &crate::state::Mon, v: &mut Value) {
        if let Value::Object(o) = v {
            o.insert("hurtThisTurn".into(), json!(m.hurt_this_turn));
            o.insert(
                "statsRaisedThisTurn".into(),
                json!(m.stats_raised_this_turn),
            );
            o.insert(
                "statsLoweredThisTurn".into(),
                json!(m.stats_lowered_this_turn),
            );
            o.insert(
                "setSpecies".into(),
                json!(self.dex.species(m.set_species).name),
            );
            let attacked: Vec<Value> = m
                .attacked_by
                .iter()
                .map(|a| {
                    json!({
                        "source": format!("p{}:{}", a.side + 1, a.name),
                        "damage": a.damage,
                        "move": self.dex.mv(a.mv).id,
                        "thisTurn": a.this_turn,
                        // `getSlot()` counts bench positions too: a user that has left is "p1d".
                        "slot": format!("p{}{}", a.slot.0 + 1, ["a", "b", "c", "d", "e", "f"][a.slot.1 as usize]),
                    })
                })
                .collect();
            o.insert("attackedBy".into(), Value::Array(attacked));
        }
    }

    /// `attackedBy` read back from a Showdown projection.
    pub(crate) fn import_attacked_by(dex: &crate::dex::Dex, v: &Value) -> Vec<Attacker> {
        v["attackedBy"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let src = e["source"].as_str()?;
                        let (side, name) = src.split_once(':')?;
                        let slot = e["slot"].as_str()?.as_bytes();
                        Some(Attacker {
                            side: side.strip_prefix('p')?.parse::<u8>().ok()? - 1,
                            name: name.into(),
                            damage: e["damage"].as_u64()? as u32,
                            mv: dex.move_id(e["move"].as_str()?)?,
                            this_turn: e["thisTurn"].as_bool()?,
                            slot: (
                                slot.get(1)?.wrapping_sub(b'1'),
                                slot.get(2)?.wrapping_sub(b'a'),
                            ),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The slot's conditions as Showdown projects them.
    pub(crate) fn project_slot_conditions(&self, s: usize, slot: usize) -> Value {
        let mut out = Map::new();
        let Some(c) = self.slot_conds[s].get(slot) else {
            return Value::Object(out);
        };
        let slot_id = |pos: usize| format!("{}{}", self.sides[s].id(), ["a", "b", "c"][pos]);
        // Only Wish (and Leech Seed) keep `sourceSlot` in the projection.
        if let Some(user) = c.healing_wish {
            let mut hw = json!({"sourceSlot": slot_id(user), "isSlotCondition": true});
            if c.healing_wish_detached {
                hw["detached"] = json!(true);
            }
            out.insert("healingwish".into(), hw);
        }
        if let Some(f) = &c.future {
            out.insert(
                "futuremove".into(),
                json!({
                    "isSlotCondition": true, "move": "futuresight", "targetSlot": slot_id(slot), "endingTurn": f.ending_turn,
                    "source": crate::projection::mref_id(f.source),
                }),
            );
        }
        if let Some(w) = &c.wish {
            out.insert(
                "wish".into(),
                json!({
                    "sourceSlot": slot_id(w.source_slot), "isSlotCondition": true, "hp": w.hp, "startingTurn": w.starting_turn,
                    "wisher": w.wisher,
                }),
            );
        }
        Value::Object(out)
    }

    /// Pseudo-weathers: Gravity.
    pub(crate) fn project_pseudo_weather(&self, pseudo: &mut Map<String, Value>) {
        if self.field.gravity > 0 {
            pseudo.insert("gravity".into(), json!({"duration": self.field.gravity}));
        }
        if self.field.magic_room > 0 {
            let mut room = json!({"duration": self.field.magic_room});
            if let Some(s) = self.field.magic_room_source {
                room["source"] = json!(crate::projection::mref_id(s));
            }
            pseudo.insert("magicroom".into(), room);
        }
        if self.field.wonder_room > 0 {
            pseudo.insert(
                "wonderroom".into(),
                json!({"duration": self.field.wonder_room}),
            );
        }
    }

    pub(crate) fn project_volatiles(&self, m: &Mon, volatiles: &mut Map<String, Value>) {
        let v = &m.vol;
        if v.partial_trap > 0 {
            let mut trap =
                json!({"duration": v.partial_trap, "boundDivisor": v.partial_trap_divisor});
            // The source is only named while it is active (Showdown's `isActive`).
            if let Some(s) = v.partial_trap_source.filter(|&s| {
                let sm = self.mon(s);
                sm.is_active && !sm.fainted && !sm.revived
            }) {
                trap["source"] = json!(crate::projection::mref_id(s));
            }
            if let Some(mv) = v.partial_trap_move {
                trap["sourceEffect"] = json!(self.dex.mv(mv).id);
            }
            volatiles.insert("partiallytrapped".into(), trap);
        }
        if let Some((s, slot)) = v.leech_seed {
            let id = format!("{}{}", self.sides[s].id(), ["a", "b", "c"][slot]);
            volatiles.insert("leechseed".into(), json!({"sourceSlot": id}));
        }
        for (on, id) in [
            (v.aqua_ring, "aquaring"),
            (v.ingrain, "ingrain"),
            (v.salt_cure, "saltcure"),
        ] {
            if on {
                volatiles.insert(id.into(), json!({}));
            }
        }
        if v.smack_down {
            volatiles.insert("smackdown".into(), json!({}));
        }
        if v.chilly_reception {
            volatiles.insert("chillyreception".into(), json!({"duration": 1}));
        }
        if v.magnet_rise > 0 {
            volatiles.insert("magnetrise".into(), json!({"duration": v.magnet_rise}));
        }
        if v.uproar > 0 {
            volatiles.insert("uproar".into(), json!({"duration": v.uproar}));
        }
        if v.stockpile > 0 {
            volatiles.insert(
                "stockpile".into(),
                json!({"layers": v.stockpile, "def": v.stockpile_def, "spd": v.stockpile_spd}),
            );
        }
        {
            let (dex, out) = (&self.dex, &mut *volatiles);
            if v.gem {
                out.insert("gem".into(), json!({"duration": 1}));
            }
            if let Some(c) = v.counter {
                let slot = c
                    .slot
                    .map(|(s, p)| json!(format!("p{}{}", s + 1, ["a", "b", "c"][p as usize])));
                out.insert(
                    "counter".into(),
                    json!({"duration": 1, "slot": slot, "damage": c.damage}),
                );
            }
            if let Some(c) = v.mirror_coat {
                let slot = c
                    .slot
                    .map(|(s, p)| json!(format!("p{}{}", s + 1, ["a", "b", "c"][p as usize])));
                out.insert(
                    "mirrorcoat".into(),
                    json!({"duration": 1, "slot": slot, "damage": c.damage}),
                );
            }
            if v.beak_blast {
                out.insert("beakblast".into(), json!({"duration": 1}));
            }
            if let Some(lost) = v.focus_punch {
                out.insert(
                    "focuspunch".into(),
                    if lost {
                        json!({"duration": 1, "lostFocus": true})
                    } else {
                        json!({"duration": 1})
                    },
                );
            }
            if v.fling {
                out.insert("fling".into(), json!({}));
            }
            if v.syrup_bomb.is_some() {
                let src = v.syrup_bomb.map(crate::projection::mref_id);
                out.insert(
                    "syrupbomb".into(),
                    json!({"duration": v.syrup_bomb_duration, "source": src}),
                );
            }
            if let Some(m) = v.metronome {
                let last = m
                    .last_move
                    .map(|id| dex.mv(id).id.clone())
                    .unwrap_or_default();
                out.insert(
                    "metronome".into(),
                    json!({"lastMove": last, "numConsecutive": m.num}),
                );
            }
            if v.sparkling_aria {
                out.insert("sparklingaria".into(), json!({}));
            }
        }
        {
            let (dex, out) = (&self.dex, &mut *volatiles);
            if v.focus_energy {
                out.insert("focusenergy".into(), json!({}));
            }
            if let Some(dragon) = v.dragon_cheer {
                out.insert("dragoncheer".into(), json!({"hasDragonType": dragon}));
            }
            if v.endure {
                out.insert("endure".into(), json!({"duration": 1}));
            }
            if v.minimize {
                out.insert("minimize".into(), json!({}));
            }
            if v.ally_switch_counter > 0 {
                out.insert(
                    "allyswitch".into(),
                    json!({"counter": v.ally_switch_counter, "duration": v.ally_switch_duration}),
                );
            }
            if v.destiny_bond {
                out.insert("destinybond".into(), json!({}));
            }
            if v.curse {
                out.insert("curse".into(), json!({}));
            }
            if let Some(s) = v.attract {
                out.insert(
                    "attract".into(),
                    json!({"source": crate::projection::mref_id(s)}),
                );
            }
            // `trapped.linkedStatus` is the string "trapper"; `trapper`'s is the
            // condition itself, which the projection leaves out.
            if let Some(t) = v.trapped {
                out.insert(
                    "trapped".into(),
                    json!({"linkedStatus": "trapper", "linked": [crate::projection::mref_id(t)]}),
                );
            }
            if v.trapper != 0 {
                let mut linked: Vec<String> = (0..16usize)
                    .filter(|b| v.trapper & (1 << b) != 0)
                    .map(|b| crate::projection::mref_id((b / 8, b % 8)))
                    .collect();
                linked.sort();
                out.insert("trapper".into(), json!({"linked": linked}));
            }
            if v.charge {
                out.insert("charge".into(), json!({}));
            }
            if v.electrify {
                out.insert("electrify".into(), json!({"duration": 1}));
            }
            if v.gastro_acid {
                out.insert("gastroacid".into(), json!({}));
            }
            if v.lock_on_duration > 0 {
                let src = v.lock_on.map(crate::projection::mref_id);
                out.insert(
                    "lockon".into(),
                    json!({"duration": v.lock_on_duration, "source": src}),
                );
            }
            if v.no_retreat {
                out.insert("noretreat".into(), json!({}));
            }
            if let Some(s) = v.octolock {
                out.insert(
                    "octolock".into(),
                    json!({"source": crate::projection::mref_id(s)}),
                );
            }
            if v.power_trick {
                out.insert("powertrick".into(), json!({}));
            }
            if v.torment {
                out.insert("torment".into(), json!({}));
            }
            if let Some(m) = v.locked_move {
                out.insert(
                "lockedmove".into(),
                json!({"duration": v.locked_duration, "trueDuration": v.locked_true_duration, "move": dex.mv(m).id}),
            );
            }
        }
    }

    /// Side conditions in the projection.
    pub(crate) fn project_side_conditions(&self, s: usize, conds: &mut Map<String, Value>) {
        let a = &self.side_extras[s];
        if a.safeguard > 0 {
            conds.insert("safeguard".into(), json!({"duration": a.safeguard}));
        }
        if a.spikes > 0 {
            conds.insert("spikes".into(), json!({"layers": a.spikes}));
        }
        if a.sticky_web {
            conds.insert("stickyweb".into(), json!({}));
        }
    }

    /// This module's pseudo-weathers in the projection.
    pub(crate) fn project_fairy_lock(&self, pseudo: &mut Map<String, Value>) {
        if self.field.fairy_lock > 0 {
            pseudo.insert(
                "fairylock".into(),
                json!({"duration": self.field.fairy_lock}),
            );
        }
    }
}

/// Whether `id` is a volatile the engine projects and imports itself.
pub(crate) fn known_volatile(id: &str) -> bool {
    matches!(
        id,
        "gem"
            | "counter"
            | "mirrorcoat"
            | "beakblast"
            | "focuspunch"
            | "fling"
            | "metronome"
            | "sparklingaria"
            | "syrupbomb"
    ) || matches!(
        id,
        "focusenergy"
            | "dragoncheer"
            | "endure"
            | "minimize"
            | "allyswitch"
            | "destinybond"
            | "curse"
            | "lockedmove"
            | "charge"
            | "electrify"
            | "gastroacid"
            | "torment"
            | "attract"
            | "trapped"
            | "trapper"
            | "lockon"
            | "noretreat"
            | "octolock"
            | "powertrick"
    )
}

/// Import the engine's volatiles from a Showdown projection.
pub(crate) fn import_volatiles(vol: &mut Volatiles, vols: &Value, dex: &Dex) {
    {
        let v = &mut *vol;
        v.gem = vols.get("gem").is_some();
        v.counter = vols.get("counter").map(|c| CounterVol {
            slot: c["slot"].as_str().and_then(|s| {
                let b = s.as_bytes();
                Some((b.get(1)?.wrapping_sub(b'1'), b.get(2)?.wrapping_sub(b'a')))
            }),
            damage: c["damage"].as_u64().unwrap_or(0) as u32,
        });
        v.mirror_coat = vols.get("mirrorcoat").map(|c| CounterVol {
            slot: c["slot"].as_str().and_then(|s| {
                let b = s.as_bytes();
                Some((b.get(1)?.wrapping_sub(b'1'), b.get(2)?.wrapping_sub(b'a')))
            }),
            damage: c["damage"].as_u64().unwrap_or(0) as u32,
        });
        v.beak_blast = vols.get("beakblast").is_some();
        if let Some(sb) = vols.get("syrupbomb") {
            v.syrup_bomb = crate::projection::parse_mref(&sb["source"]);
            v.syrup_bomb_duration = sb["duration"].as_u64().unwrap_or(0) as u8;
        }
        v.focus_punch = vols.get("focuspunch").map(|c| c.get("lostFocus").is_some());
        v.fling = vols.get("fling").is_some();
        v.sparkling_aria = vols.get("sparklingaria").is_some();
        v.metronome = vols.get("metronome").map(|m| MetronomeVol {
            last_move: m["lastMove"].as_str().and_then(|s| dex.move_id(s)),
            num: m["numConsecutive"].as_u64().unwrap_or(0) as u16,
        });
    }
    {
        let v = &mut *vol;
        v.focus_energy = vols.get("focusenergy").is_some();
        v.dragon_cheer = vols
            .get("dragoncheer")
            .map(|d| d["hasDragonType"].as_bool().unwrap_or(false));
        v.endure = vols.get("endure").is_some();
        v.minimize = vols.get("minimize").is_some();
        v.ally_switch_counter = vols["allyswitch"]["counter"].as_u64().unwrap_or(0) as u16;
        v.ally_switch_duration = vols["allyswitch"]["duration"].as_u64().unwrap_or(0) as u8;
        v.destiny_bond = vols.get("destinybond").is_some();
        v.curse = vols.get("curse").is_some();
        v.charge = vols.get("charge").is_some();
        v.electrify = vols.get("electrify").is_some();
        v.gastro_acid = vols.get("gastroacid").is_some();
        v.torment = vols.get("torment").is_some();
        v.attract = crate::projection::parse_mref(&vols["attract"]["source"]);
        v.trapped = vols["trapped"]["linked"]
            .get(0)
            .and_then(crate::projection::parse_mref);
        for t in vols["trapper"]["linked"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(crate::projection::parse_mref)
        {
            v.trapper |= Battle::trap_bit(t);
        }
        if let Some(l) = vols.get("lockon") {
            v.lock_on_duration = l["duration"].as_u64().unwrap_or(0) as u8;
            v.lock_on = crate::projection::parse_mref(&l["source"]);
        }
        v.no_retreat = vols.get("noretreat").is_some();
        v.octolock = crate::projection::parse_mref(&vols["octolock"]["source"]);
        v.power_trick = vols.get("powertrick").is_some();
        let locked = &vols["lockedmove"];
        v.locked_move = locked["move"].as_str().and_then(|m| dex.move_id(m));
        v.locked_duration = locked["duration"].as_u64().unwrap_or(0) as u8;
        v.locked_true_duration = locked["trueDuration"].as_u64().unwrap_or(0) as u8;
    }
    {
        let trap = &vols["partiallytrapped"];
        vol.partial_trap = trap["duration"].as_u64().unwrap_or(0) as u8;
        if vols.get("partiallytrapped").is_some() {
            vol.partial_trap_divisor = trap["boundDivisor"].as_u64().unwrap_or(8) as u8;
            vol.partial_trap_move = trap["sourceEffect"].as_str().and_then(|m| dex.move_id(m));
            vol.partial_trap_source = crate::projection::parse_mref(&trap["source"]);
        }
        vol.leech_seed = crate::projection::parse_slot(&vols["leechseed"]["sourceSlot"]);
        vol.aqua_ring = vols.get("aquaring").is_some();
        vol.ingrain = vols.get("ingrain").is_some();
        vol.salt_cure = vols.get("saltcure").is_some();
        vol.stockpile = vols["stockpile"]["layers"].as_u64().unwrap_or(0) as u8;
        vol.stockpile_def = vols["stockpile"]["def"].as_i64().unwrap_or(0) as i8;
        vol.stockpile_spd = vols["stockpile"]["spd"].as_i64().unwrap_or(0) as i8;
        vol.chilly_reception = vols.get("chillyreception").is_some();
        vol.magnet_rise = vols["magnetrise"]["duration"].as_u64().unwrap_or(0) as u8;
        vol.uproar = vols["uproar"]["duration"].as_u64().unwrap_or(0) as u8;
        vol.smack_down = vols.get("smackdown").is_some();
    }
}
