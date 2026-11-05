//! Diagnostics never alter choices or expose opposing hidden sets to the policy.
use crate::env::Choice;
use ninjask::{
    state::SlotChoice,
    tracker::{MonView, Tracker},
    Battle,
};
use serde_json::{json, Value};

fn types(t: &Tracker, m: &MonView) -> Vec<String> {
    let mut ids = m.types.clone().unwrap_or_else(|| {
        m.species
            .map(|id| t.dex().species(id).types.into_iter().collect())
            .unwrap_or_default()
    });
    if let Some(ty) = m.added_type {
        ids.push(ty);
    }
    ids.into_iter()
        .filter_map(|ty| t.dex().type_names.get(ty as usize).cloned())
        .collect()
}

pub fn state(t: &Tracker) -> Value {
    json!({"sides":(0..2).map(|rel| {
        let s = if rel == 0 {t.me} else {1-t.me};
        t.sides[s].mons.iter().map(|m| json!({"name":m.name,
            "species":m.species.map(|id| &t.dex().species(id).name),"types":types(t,m),
            "type_override":m.types.is_some(),"added_type":m.added_type.is_some(),
            "hp":m.hp,"maxhp":m.maxhp,"slot":m.slot,"team_pos":m.team_pos,
            "fainted":m.fainted,"volatiles":m.volatiles,
            "ability":m.ability.map(|id|&t.dex().ability(id).name),
            "moves":m.moves.iter().map(|mv|json!({"move":t.dex().mv(mv.id).name,"pp":mv.pp})).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    }).collect::<Vec<_>>(),"terrain":t.field.terrain.map(|(ty,_)|ty.id()),
    "weather":t.field.weather.map(|(ty,_)|ty.id())})
}

pub fn choice(b: &Battle, t: &Tracker, c: &Choice, legal: &[Choice]) -> Value {
    let side = t.me;
    match c {
        Choice::Preview(order) => json!({"preview":order.iter().map(|&i|json!({
            "index":i,"species":b.dex.species(b.sides[side].mons[i].species).name})).collect::<Vec<_>>()}),
        Choice::Turn(slots) => json!(slots.iter().enumerate().map(|(k,c)| {
            let actor = &b.sides[side].mons[k];
            let species = &b.dex.species(actor.species).name;
            match c {
                SlotChoice::Pass => json!({"actor":species,"pass":true}),
                SlotChoice::Switch{index} => json!({"actor":species,"switch_index":index,
                    "switch":b.dex.species(b.sides[side].mons[*index].species).name}),
                SlotChoice::Move{slot,target,mega} => {
                    let mv = b.choice_move_id((side,k),*slot).map(|id|b.dex.mv(id));
                    let has_target = |wanted| legal.iter().any(|c|match c {
                        Choice::Turn(cs) => matches!(cs.get(k),Some(SlotChoice::Move{slot:s,target:loc,mega:m}) if s == slot && *loc == wanted && m == mega),
                        _ => false,
                    });
                    json!({"actor":species,"move":mv.map(|m|m.name.as_str()).unwrap_or("Struggle/recharge"),
                        "move_slot":slot,"target":b.choice_target_loc((side,k),*target),"requested_target":target,"mega":mega,
                        "category":mv.map(|m|format!("{:?}",m.category)),"power":mv.map(|m|m.base_power),
                        "type":mv.and_then(|m|b.dex.type_names.get(m.type_ as usize)),
                        "accuracy":mv.map(|m|m.accuracy.unwrap_or(100)),
                        "actor_slot":k,"active_move_actions":actor.active_move_actions,
                        "both_foe_targets_legal":*target > 0 && has_target(1) && has_target(2)})
                }
            }
        }).collect::<Vec<_>>()),
    }
}
