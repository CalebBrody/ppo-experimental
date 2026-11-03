mod common;

use ninjask::{
    tracker::{SpeedBounds, Tracker},
    Dex,
};
use std::sync::Arc;

fn tracker() -> Option<Tracker> {
    let json = common::format_export("gen9championsvgc2026regmc")?;
    let mut t = Tracker::new(Arc::new(Dex::from_json(&json).unwrap()), 0);
    for line in [
        "|poke|p2|Gholdengo, L50|",
        "|poke|p2|Incineroar, L50|",
        "|teamsize|p2|4",
        "|switch|p1a: Arcanine|Arcanine-Hisui, L50|100/100",
        "|switch|p2a: Gholdengo|Gholdengo, L50|100/100",
        "|-ability|p1a: Arcanine|Rock Head",
        "|-item|p1a: Arcanine|Focus Sash",
        "|-ability|p2a: Gholdengo|Good as Gold",
        "|-item|p2a: Gholdengo|Life Orb",
        "|turn|1",
    ] {
        t.feed(line);
    }
    t.sides[0].mons[0].stats = Some([100, 100, 100, 100, 120]);
    Some(t)
}

#[test]
fn legal_bounds_and_revealed_picks() {
    let Some(mut t) = tracker() else { return };
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 93, max: 149 })
    );
    assert_eq!(
        t.speed_bounds(0, 0),
        Some(SpeedBounds { min: 120, max: 120 })
    );
    let v = t.to_json();
    assert_eq!(v["sides"][1]["pokemon"].as_array().unwrap().len(), 2);
    assert_eq!(v["sides"][1]["knownPicks"], serde_json::json!([0]));
    assert_eq!(v["sides"][1]["pokemon"][1]["seen"], false);
    assert_eq!(v["sides"][1]["pokemon"][0]["speedBounds"]["max"], 149);
    assert!(t.speed_bounds(2, 0).is_none());
    t.feed("|switch|p2a: Incineroar|Incineroar, L50|100/100");
    let v = t.to_json();
    assert_eq!(v["sides"][1]["knownPicks"], serde_json::json!([0, 1]));
    assert_eq!(v["sides"][1]["pokemon"][0]["slot"], serde_json::Value::Null);
}

#[test]
fn normal_order_and_ties_tighten_bounds() {
    let Some(mut t) = tracker() else { return };
    t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
    t.feed("|-damage|p2a: Gholdengo|80/100");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 93, max: 120 })
    );
    t.feed("|turn|2");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 120, max: 120 })
    );
}

#[test]
fn speed_swap_exchanges_bounds_and_switching_restores_them() {
    let Some(mut t) = tracker() else { return };
    t.feed("|-activate|p1a: Arcanine|move: Speed Swap|[of] p2a: Gholdengo");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 120, max: 120 })
    );
    assert_eq!(
        t.speed_bounds(0, 0),
        Some(SpeedBounds { min: 93, max: 149 })
    );
    t.feed("|switch|p2a: Incineroar|Incineroar, L50|100/100");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 93, max: 149 })
    );
}

#[test]
fn trick_room_reverses_comparison() {
    let Some(mut t) = tracker() else { return };
    t.feed("|-fieldstart|move: Trick Room");
    t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 120, max: 149 })
    );
}

#[test]
fn priority_unknown_items_modifiers_and_reordering_do_not_imply_speed() {
    for setup in [
        "",
        "|-start|p2a: Gholdengo|confusion",
        "|-weather|RainDance",
        "|move|p1a: Arcanine|After You|p2a: Gholdengo",
    ] {
        let Some(mut t) = tracker() else { return };
        if setup.is_empty() {
            t.sides[1].mons[0].item = ninjask::tracker::ItemView::Unknown;
        } else {
            t.feed(setup);
        }
        t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
        t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
        assert_eq!(
            t.speed_bounds(1, 0),
            Some(SpeedBounds { min: 93, max: 149 }),
            "{setup}"
        );
    }
    let Some(mut t) = tracker() else { return };
    t.feed("|move|p1a: Arcanine|Extreme Speed|p2a: Gholdengo");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 93, max: 149 })
    );
}

#[test]
fn known_speed_modifiers_are_inverted_with_engine_rounding() {
    for setup in [
        vec!["|-item|p2a: Gholdengo|Choice Scarf"],
        vec!["|-boost|p2a: Gholdengo|spe|1"],
        vec![
            "|-sidestart|p2: Foe|move: Tailwind",
            "|-status|p2a: Gholdengo|par",
        ],
    ] {
        let Some(mut t) = tracker() else { return };
        let combined = setup.len() == 2;
        t.sides[0].mons[0].stats.as_mut().unwrap()[4] = if combined { 120 } else { 150 };
        for l in setup {
            t.feed(l);
        }
        t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
        t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
        assert_eq!(
            t.speed_bounds(1, 0),
            Some(SpeedBounds {
                min: 93,
                max: if combined { 120 } else { 100 }
            })
        );
    }
}

#[test]
fn bounds_follow_known_nature_and_species_changes() {
    let Some(mut t) = tracker() else { return };
    t.sides[1].mons[0].nature = Some("Timid".into());
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 114, max: 149 })
    );
    t.feed("|move|p1a: Arcanine|Flare Blitz|p2a: Gholdengo");
    t.feed("|move|p2a: Gholdengo|Shadow Ball|p1a: Arcanine");
    t.sides[1].mons[0].nature = None;
    t.sides[1].mons[0].species = t.dex().species_id("Incineroar");
    assert_eq!(
        t.speed_bounds(1, 0),
        Some(SpeedBounds { min: 72, max: 123 })
    );
}
