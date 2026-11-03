//! Showdown packed teams and stat calculation.

use crate::dex::types::*;
use crate::dex::{Dex, Nature};

/// One Pokémon as written in a team, before battle.
#[derive(Clone, Debug)]
pub struct MonSet {
    pub nickname: String,
    pub species: SpeciesId,
    pub item: Option<ItemId>,
    pub ability: AbilityId,
    pub moves: Vec<MoveId>,
    pub nature: String,
    /// Stat points (Champions) or EVs, in Showdown stat order.
    pub points: [u16; 6],
    pub gender: String,
    pub level: u8,
    pub shiny: bool,
}

const NATURES: [&str; 25] = [
    "Adamant", "Bashful", "Bold", "Brave", "Calm", "Careful", "Docile", "Gentle", "Hardy", "Hasty",
    "Impish", "Jolly", "Lax", "Lonely", "Mild", "Modest", "Naive", "Naughty", "Quiet", "Quirky",
    "Rash", "Relaxed", "Sassy", "Serious", "Timid",
];

/// A nature's name as Showdown spells it, or "" for none or an unknown one.
pub fn nature_name(name: &str) -> &'static str {
    NATURES
        .iter()
        .find(|n| n.eq_ignore_ascii_case(name))
        .copied()
        .unwrap_or("")
}

/// Appends `name` as Showdown's `Teams.packName` writes it: letters and
/// digits only. Bytes are tested one by one: those of a multi-byte character
/// are never ASCII, so this keeps exactly the ASCII letters and digits.
fn push_packed_name(out: &mut Vec<u8>, name: &str) {
    out.extend(name.bytes().filter(u8::is_ascii_alphanumeric));
}

/// The team as Showdown's `showOpenTeamSheets` packs it: species, item,
/// ability, moves, nature (Champions) and gender, without stat points.
/// Each set is `SPECIES||ITEM|ABILITY|MOVES|NATURE||GENDER|||LEVEL|`, and
/// sets are joined by `]`. Written as bytes into one buffer, since every
/// battle builds its sides' sheets.
pub fn open_sheet(dex: &Dex, sets: &[MonSet]) -> String {
    use std::io::Write;
    let mut out: Vec<u8> = Vec::with_capacity(96 * sets.len());
    for (i, s) in sets.iter().enumerate() {
        if i > 0 {
            out.push(b']');
        }
        out.extend_from_slice(dex.species(s.species).name.as_bytes());
        out.extend_from_slice(b"||");
        if let Some(item) = s.item {
            push_packed_name(&mut out, &dex.item(item).name);
        }
        out.push(b'|');
        push_packed_name(&mut out, &dex.ability(s.ability).name);
        out.push(b'|');
        for (k, &m) in s.moves.iter().enumerate() {
            if k > 0 {
                out.push(b',');
            }
            push_packed_name(&mut out, &dex.mv(m).name);
        }
        out.push(b'|');
        if dex.format.champions {
            out.extend_from_slice(s.nature.as_bytes());
        }
        out.extend_from_slice(b"||");
        if s.gender == "M" || s.gender == "F" {
            out.extend_from_slice(s.gender.as_bytes());
        }
        out.extend_from_slice(b"|||");
        if s.level != 100 {
            // Writing to a Vec cannot fail.
            let _ = write!(out, "{}", s.level);
        }
        out.push(b'|');
    }
    // Whole strings and ASCII bytes only: still UTF-8.
    String::from_utf8(out).expect("a team sheet is UTF-8")
}

/// Parse a packed team: NICK|SPECIES|ITEM|ABILITY|MOVES|NATURE|EVS|GENDER|IVS|SHINY|LEVEL|MISC, joined by "]".
pub fn parse_packed(dex: &Dex, packed: &str) -> Result<Vec<MonSet>, String> {
    let mut out = Vec::new();
    for chunk in packed.split(']').filter(|c| !c.is_empty()) {
        let mut f: Vec<&str> = chunk.split('|').collect();
        while f.len() < 12 {
            f.push("");
        }
        let nickname = f[0].to_string();
        let species_name = if f[1].is_empty() { f[0] } else { f[1] };
        let species = dex
            .species_id(species_name)
            .ok_or_else(|| format!("unknown species {species_name}"))?;
        let item = if f[2].is_empty() {
            None
        } else {
            Some(
                dex.item_id(f[2])
                    .ok_or_else(|| format!("unknown item {}", f[2]))?,
            )
        };
        let ability = if f[3].is_empty() {
            dex.ability_id("noability").unwrap()
        } else {
            dex.ability_id(f[3])
                .ok_or_else(|| format!("unknown ability {}", f[3]))?
        };
        let mut moves = Vec::new();
        for m in f[4].split(',').filter(|m| !m.is_empty()) {
            moves.push(dex.move_id(m).ok_or_else(|| format!("unknown move {m}"))?);
        }
        let mut points = [0u16; 6];
        if !f[6].is_empty() {
            for (i, v) in f[6].split(',').enumerate().take(6) {
                points[i] = v.parse().unwrap_or(0);
            }
        }
        let parsed_level = f[10].parse::<u8>().ok();
        let level = dex.format.level.or(parsed_level).unwrap_or(100);
        out.push(MonSet {
            nickname,
            species,
            item,
            ability,
            moves,
            nature: f[5].to_string(),
            points,
            gender: f[7].to_string(),
            level,
            shiny: f[9] == "S",
        });
    }
    if out.is_empty() {
        return Err("empty team".into());
    }
    Ok(out)
}

/// Stats under the Champions formula (Showdown `data/mods/champions/scripts.ts`
/// `statModify` without Level Clause Mod): HP = base + points + 75; others
/// = (base + points + 20), then the nature's x1.1 / x0.9 with 16-bit truncation.
pub fn champions_stats(base: &[u16; 6], points: &[u16; 6], nature: Nature) -> [u16; 6] {
    let mut out = [0u16; 6];
    for st in Stat::ALL {
        let i = st as usize;
        if st == Stat::Hp {
            out[i] = base[i] + points[i] + 75;
            continue;
        }
        let mut v = (base[i] + points[i] + 20) as u32;
        if nature.plus == Some(st) {
            v = ((v * 110) & 0xffff) / 100;
        } else if nature.minus == Some(st) {
            v = ((v * 90) & 0xffff) / 100;
        }
        out[i] = v as u16;
    }
    out
}

/// Maximum PP under the Champions rule (`calculatePP`): (pp / 5 + 1) * 4,
/// with base PP above 20 capped at 20 by the mod's `init`; a move without
/// PP boosts keeps its base PP.
pub fn champions_max_pp(base_pp: u8, no_pp_boosts: bool) -> u8 {
    if no_pp_boosts {
        return base_pp;
    }
    let pp = base_pp.min(20) as u32;
    ((pp / 5 + 1) * 4) as u8
}
