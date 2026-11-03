//! The tracker's lengths of Taunt, Encore and Disable, which depend on
//! whether their target had already acted that turn (Showdown's
//! `queue.willMove`), read from the log's move order. Needs the format export
//! (`data/derived/format/`, see `common`); skipped without it.

mod common;

use std::sync::Arc;

use ninjask::tracker::Tracker;
use ninjask::Dex;

fn dex() -> Option<Arc<Dex>> {
    let json = common::format_export("gen9championsvgc2026regmc")?;
    Some(Arc::new(Dex::from_json(&json).ok()?))
}

const START: &[&str] = &[
    "|player|p1|Player 1|",
    "|player|p2|Player 2|",
    "|gametype|doubles",
    "|switch|p1a: Persian|Persian, L50, M|100/100",
    "|switch|p1b: Kleavor|Kleavor, L50, M|100/100",
    "|switch|p2a: Farigiraf|Farigiraf, L50, F|100/100",
    "|switch|p2b: Scrafty|Scrafty, L50, M|100/100",
    "|turn|1",
];

/// The length the tracker of player 1 holds for `vol` on Farigiraf after `lines`.
fn length(dex: &Arc<Dex>, lines: &[&str], vol: &str) -> Option<u8> {
    let mut t = Tracker::new(dex.clone(), 0);
    for l in START.iter().chain(lines) {
        t.feed(l);
    }
    let m = t.sides[1].mons.iter().find(|m| m.name == "Farigiraf")?;
    m.volatile_length(vol)
}

#[test]
fn taunt_lasts_longer_on_a_pokemon_that_has_moved() {
    let Some(dex) = dex() else { return };
    let taunt = [
        "|move|p1a: Persian|Taunt|p2a: Farigiraf",
        "|-start|p2a: Farigiraf|move: Taunt",
    ];
    assert_eq!(length(&dex, &taunt, "taunt"), Some(3));
    let moved = [
        "|move|p2a: Farigiraf|Thunderbolt|p1b: Kleavor",
        "|-damage|p1b: Kleavor|50/100",
        taunt[0],
        taunt[1],
    ];
    assert_eq!(length(&dex, &moved, "taunt"), Some(4));
    // Armor Tail's `cant` names its holder, but stops the attacker's move.
    let blocked = [
        "|move|p1b: Kleavor|Quick Attack||[still]",
        "|cant|p2a: Farigiraf|ability: Armor Tail|Quick Attack|[of] p1b: Kleavor",
        taunt[0],
        taunt[1],
    ];
    assert_eq!(length(&dex, &blocked, "taunt"), Some(3));
    // Couldn't move (flinch): it has acted.
    let flinched = ["|cant|p2a: Farigiraf|flinch", taunt[0], taunt[1]];
    assert_eq!(length(&dex, &flinched, "taunt"), Some(4));
    // Came in this turn: no move queued, but Taunt's extra turn needs
    // `activeTurns`.
    let fresh = [
        "|switch|p2a: Farigiraf|Farigiraf, L50, F|100/100",
        taunt[0],
        taunt[1],
    ];
    assert_eq!(length(&dex, &fresh, "taunt"), Some(3));
    // The next turn starts afresh.
    let next = [moved[0], "|upkeep", "|turn|2", taunt[0], taunt[1]];
    assert_eq!(length(&dex, &next, "taunt"), Some(3));
}

#[test]
fn encore_and_disable_follow_the_move_order() {
    let Some(dex) = dex() else { return };
    let used = "|move|p2a: Farigiraf|Thunderbolt|p1b: Kleavor";
    let encore = [
        "|move|p1a: Persian|Encore|p2a: Farigiraf",
        "|-start|p2a: Farigiraf|Encore",
    ];
    assert_eq!(
        length(
            &dex,
            &[used, "|upkeep", "|turn|2", encore[0], encore[1]],
            "encore"
        ),
        Some(3)
    );
    assert_eq!(
        length(&dex, &[used, encore[0], encore[1]], "encore"),
        Some(4)
    );
    let disable = [
        "|move|p1a: Persian|Disable|p2a: Farigiraf",
        "|-start|p2a: Farigiraf|Disable|Thunderbolt",
    ];
    assert_eq!(
        length(
            &dex,
            &[used, "|upkeep", "|turn|2", disable[0], disable[1]],
            "disable"
        ),
        Some(4)
    );
    assert_eq!(
        length(&dex, &[used, disable[0], disable[1]], "disable"),
        Some(5)
    );
    // Cursed Body's Disable: always 4 turns.
    let cursed = [
        used,
        "|-start|p2a: Farigiraf|Disable|Thunderbolt|[from] ability: Cursed Body|[of] p1b: Kleavor",
    ];
    assert_eq!(length(&dex, &cursed, "disable"), None);
}
