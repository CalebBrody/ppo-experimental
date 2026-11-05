mod common;
use std::sync::Arc;
use ninjask::{Dex, tracker::Tracker, world::{World, Assumptions}};

fn view() -> Option<Tracker> {
    let json = common::format_export("gen9championsvgc2026regmc")?;
    let dex = Arc::new(Dex::from_json(&json).unwrap());
    let mut t = Tracker::new(dex, 0);
    for line in ["|gametype|doubles", "|switch|p1a: Gengar|Gengar, L50|100/100",
        "|switch|p2a: Garchomp|Garchomp, L50|100/100", "|turn|6",
        "|move|p2a: Garchomp|Dragon Claw|p1a: Gengar",
        "|-start|p2a: Garchomp|Disable|Dragon Claw|[from] ability: Cursed Body|[of] p1a: Gengar"] { t.feed(line); }
    Some(t)
}

#[test]
fn disable_keeps_original_move_in_reconstructed_world() {
    let Some(mut t) = view() else { return };
    for (turn, name) in [(7, "Rock Slide"), (8, "Protect")] {
        t.feed(&format!("|turn|{turn}"));
        t.feed(&format!("|move|p2a: Garchomp|{name}|p1a: Gengar"));
        let w = World::from_view(&t, &Assumptions::default()).unwrap();
        let m = w.sides[1].mons.iter().find(|m| m.name.as_ref() == "Garchomp").unwrap();
        assert_eq!(m.vol.disable, t.dex().move_id("Dragon Claw"));
        assert_ne!(m.vol.disable, t.dex().move_id(name));
    }
    t.feed("|-end|p2a: Garchomp|Disable");
    assert_eq!(t.sides[1].mons[0].disabled_move, None);
    let w = World::from_view(&t, &Assumptions::default()).unwrap();
    assert_eq!(w.sides[1].mons[0].vol.disable, None);
}

#[test]
fn disable_clears_on_switch_and_new_effect_uses_named_move() {
    let Some(mut t) = view() else { return };
    t.feed("|switch|p2a: Pikachu|Pikachu, L50|100/100");
    assert_eq!(t.sides[1].mons[0].disabled_move, None);
    t.feed("|switch|p2a: Garchomp|Garchomp, L50|100/100");
    t.feed("|move|p2a: Garchomp|Protect|p2a: Garchomp");
    t.feed("|-start|p2a: Garchomp|Disable|Rock Slide");
    assert_eq!(t.sides[1].mons[0].disabled_move, t.dex().move_id("Rock Slide"));
}
