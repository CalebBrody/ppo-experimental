from ninjask.choices import joint_choices, team_preview_choices


def mon(name, active=False, hp="100/100", **extra):
    return {"ident": f"p1: {name}", "details": f"{name}, L50", "condition": hp, "active": active, **extra}


def move(name, target="normal", disabled=False):
    return {"move": name, "id": name.lower(), "pp": 8, "maxpp": 8, "target": target, "disabled": disabled}


def side(*mons):
    return {"side": {"name": "A", "id": "p1", "pokemon": list(mons)}}


def test_team_preview_has_90_plans_for_bring_four():
    req = {"teamPreview": True, "maxChosenTeamSize": 4, **side(*[mon(str(i)) for i in range(6)])}
    plans = team_preview_choices(req)
    assert len(plans) == 90
    assert len(set(plans)) == 90
    assert "team 1234" in plans and "team 3412" in plans


def test_targets_mega_and_switches():
    req = {
        "active": [
            {"moves": [move("Tackle"), move("Protect", "self")], "canMegaEvo": True},
            {"moves": [move("Helping Hand", "adjacentAlly")]},
        ],
        **side(mon("A", True), mon("B", True), mon("C"), mon("D", hp="0 fnt")),
    }
    choices = joint_choices(req)
    # Slot 1: Tackle at foe 1 or 2, Protect, each with or without Mega, or switch to C.
    # Slot 2: Helping Hand at our slot 1 (-1), or switch to C.
    assert "move 1 1 mega, move 1 -1" in choices
    assert "move 2, move 1 -1" in choices
    assert "switch 3, move 1 -1" in choices
    assert "switch 3, switch 3" not in choices  # same Pokémon twice
    assert not any("switch 4" in c for c in choices)  # fainted
    # Slot 1: Tackle (2 targets) and Protect, each with or without Mega, or
    # switch: 7 options. Slot 2: Helping Hand at slot 1, or switch: 2.
    # 7 * 2 minus the double switch.
    assert len(choices) == 13


def test_only_one_mega_per_turn():
    req = {
        "active": [
            {"moves": [move("Protect", "self")], "canMegaEvo": True},
            {"moves": [move("Protect", "self")], "canMegaEvo": True},
        ],
        **side(mon("A", True), mon("B", True)),
    }
    choices = joint_choices(req)
    assert "move 1 mega, move 1 mega" not in choices
    assert set(choices) == {"move 1, move 1", "move 1 mega, move 1", "move 1, move 1 mega"}


def test_locked_move_takes_no_target():
    req = {
        "active": [
            {"moves": [{"move": "Electro Shot", "id": "electroshot"}], "trapped": True},
            {"moves": [move("Protect", "self")]},
        ],
        **side(mon("A", True), mon("B", True), mon("C")),
    }
    choices = joint_choices(req)
    assert "move 1, move 1" in choices
    assert not any(c.startswith("move 1 1") for c in choices)
    assert not any(c.startswith("switch") for c in choices)  # trapped


def test_one_active_request_reads_as_singles_without_a_game_type():
    # A side down to its last Pokémon: the request names only the survivor.
    # Nothing in it says the battle is doubles, so the default (and an
    # explicit singles) reading drops move targets, as singles requires.
    req = {
        "active": [{"moves": [move("Fake Out"), move("Quick Attack"), move("Protect", "self")]}],
        **side(mon("Pikachu", True, "97/110"), mon("B", hp="0 fnt"), mon("C", hp="0 fnt")),
    }
    assert joint_choices(req) == ["move 1", "move 2", "move 3"]
    assert joint_choices(req, game_type="singles") == ["move 1", "move 2", "move 3"]


def test_one_active_doubles_endgame_enumerates_targets():
    # The same request in a doubles battle: single-target moves still need
    # a target (the engine rejects them bare), so naming the game type
    # enumerates both foe targets and no bare form.
    req = {
        "active": [{"moves": [move("Fake Out"), move("Quick Attack"), move("Protect", "self")]}],
        **side(mon("Pikachu", True, "97/110"), mon("B", hp="0 fnt"), mon("C", hp="0 fnt")),
    }
    choices = joint_choices(req, game_type="doubles")
    assert sorted(choices) == ["move 1 1", "move 1 2", "move 2 1", "move 2 2", "move 3"]
    assert "move 1" not in choices


def test_game_type_changes_nothing_with_a_full_field():
    req = {
        "active": [
            {"moves": [move("Tackle"), move("Protect", "self")]},
            {"moves": [move("Tackle")]},
        ],
        **side(mon("A", True), mon("B", True), mon("C")),
    }
    assert joint_choices(req, game_type="doubles") == joint_choices(req)


def test_forced_switch_with_one_replacement_for_two_slots():
    req = {"forceSwitch": [True, True], **side(mon("A", True, "0 fnt"), mon("B", True, "0 fnt"), mon("C"))}
    assert sorted(joint_choices(req)) == ["pass, switch 3", "switch 3, pass"]


def test_revival_blessing_picks_a_fainted_teammate():
    req = {
        "forceSwitch": [True, False],
        **side(mon("Pawmot", True, reviving=True), mon("B", True), mon("C", hp="0 fnt"), mon("D")),
    }
    assert joint_choices(req) == ["switch 3, pass"]
