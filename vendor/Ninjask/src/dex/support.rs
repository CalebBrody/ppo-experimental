//! Which moves, items and abilities this engine simulates correctly.
//!
//! A move is *generic* when Showdown attaches no code to it, the battle core
//! doesn't single it out, and every data field it has is one the engine
//! models. Moves, items and abilities with code are supported only when the
//! engine has a ported handler for them (kinds/); everything else is
//! reported unsupported, with the reason.

use crate::dex::types::*;
use crate::dex::{Dex, Move};
use crate::kinds::{AbilityKind, ItemKind, MoveKind};

/// Top-level move fields whose meaning the engine implements.
const GENERIC_MOVE_FIELDS: &[&str] = &[
    // Behaviour the engine implements.
    "type",
    "category",
    "basePower",
    "accuracy",
    "pp",
    "priority",
    "target",
    "flags",
    "critRatio",
    "drain",
    "recoil",
    "multihit",
    "secondary",
    "secondaries",
    "self",
    "boosts",
    "status",
    "volatileStatus",
    "heal",
    "willCrit",
    "thawsTarget",
    "selfBoost",
    "ignoreDefensive",
    "ignoreEvasion",
    "damage",
    "overrideOffensiveStat",
    "overrideDefensiveStat",
    "overrideOffensivePokemon",
    "forceSwitch",
    "ohko",
    "breaksProtect",
    "hasSheerForceBoost",
    "noPPBoosts",
    "smartTarget",
    // Metadata with no battle effect.
    "id",
    "num",
    "name",
    "fullname",
    "effectType",
    "exists",
    "gen",
    "baseMoveType",
    "shortDesc",
    "desc",
    "contestType",
    "zMove",
    "maxMove",
    "callbacks",
    "coreRef",
    "tags",
    "isNonstandard",
];

/// Volatile effects the engine models (from secondaries or status moves).
const MODELED_VOLATILES: &[&str] = &["flinch", "confusion", "healblock"];

pub fn move_support(dex: &Dex, m: &Move) -> Support {
    if let Some(id) = dex.move_id(&m.id) {
        if dex.move_kind[id as usize] != MoveKind::Plain {
            return Support::Implemented;
        }
    }
    if !m.callbacks.is_empty() {
        return Support::Unsupported(format!("has code: {}", m.callbacks.join(",")));
    }
    if m.core_ref {
        return Support::Unsupported("referenced in the battle core".into());
    }
    if matches!(
        m.target,
        Target::All | Target::AllySide | Target::FoeSide | Target::AllyTeam
    ) {
        return Support::Unsupported("field or side move without a ported handler".into());
    }
    for f in &m.fields {
        let ok = GENERIC_MOVE_FIELDS.contains(&f.as_str())
            // Plain self-switching (not Baton Pass or Shed Tail) is modelled.
            || (f == "selfSwitch" && m.self_switch)
            // Status moves ignore type immunity by default; true or false is modelled for them.
            || (f == "ignoreImmunity" && m.category == Category::Status);
        if !ok {
            return Support::Unsupported(format!("field {f}"));
        }
    }
    if let Some(u) = m.unmodeled.first() {
        return Support::Unsupported(u.clone());
    }
    for sec in &m.secondaries {
        if let Some(v) = &sec.volatile {
            if !MODELED_VOLATILES.contains(&v.as_str()) {
                return Support::Unsupported(format!("volatile {v}"));
            }
        }
    }
    if let Some(v) = &m.volatile_status {
        if !MODELED_VOLATILES.contains(&v.as_str()) {
            return Support::Unsupported(format!("volatile {v}"));
        }
    }
    if m.multihit.is_some() && m.category == Category::Status {
        return Support::Unsupported("status multihit".into());
    }
    Support::Generic
}

pub fn item_support(dex: &Dex, item: Option<ItemId>) -> Support {
    match item {
        None => Support::Generic,
        Some(i) => match dex.item_kind[i as usize] {
            ItemKind::Unsupported => Support::Unsupported(format!("item {}", dex.item(i).id)),
            _ => Support::Implemented,
        },
    }
}

pub fn ability_support(dex: &Dex, ability: AbilityId) -> Support {
    match dex.ability_kind[ability as usize] {
        AbilityKind::None => Support::Generic,
        AbilityKind::Unsupported => {
            Support::Unsupported(format!("ability {}", dex.ability(ability).id))
        }
        _ => Support::Implemented,
    }
}

/// Every reason a packed team is not fully supported (empty when it is).
/// A Pokémon holding its Mega Stone is also checked with the Mega's ability.
pub fn team_problems(dex: &Dex, packed: &str) -> Result<Vec<String>, String> {
    let sets = crate::dex::team::parse_packed(dex, packed)?;
    let mut out = Vec::new();
    for s in &sets {
        for &m in &s.moves {
            if let Support::Unsupported(why) = move_support(dex, dex.mv(m)) {
                out.push(format!("move {} ({why})", dex.mv(m).id));
            }
        }
        if let Support::Unsupported(why) = item_support(dex, s.item) {
            out.push(why);
        }
        if let Support::Unsupported(why) = ability_support(dex, s.ability) {
            out.push(why);
        }
        if let Some(item) = s.item {
            let base = &dex.species(s.species).name;
            let mega = dex
                .item(item)
                .mega_stone
                .iter()
                .find(|(b, _)| b == base)
                .and_then(|(_, m)| dex.species_id(m));
            if let Some(mega) = mega {
                let ability = dex
                    .species(mega)
                    .abilities
                    .first()
                    .and_then(|(_, a)| dex.ability_id(a));
                if let Some(a) = ability {
                    if let Support::Unsupported(why) = ability_support(dex, a) {
                        out.push(format!("mega {why}"));
                    }
                }
            }
        }
    }
    Ok(out)
}
