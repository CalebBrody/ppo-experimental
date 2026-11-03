//! Pseudo-weather: Trick Room.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::dex::Move;

impl Battle {
    /// Trick Room: starts, or ends when already active (`onFieldRestart`).
    pub(crate) fn use_trick_room(&mut self, source: MonRef) {
        if self.field.trick_room > 0 {
            self.field.trick_room = 0;
            self.add("|-fieldend|move: Trick Room");
        } else {
            self.field.trick_room = self.cond_u8("trickroom", "duration", 5);
            let who = self.ident(source);
            log_line!(self, "|-fieldstart|move: Trick Room|[of] {who}");
        }
    }

    /// Magic Room: starts, or ends when already active (`onFieldRestart`).
    /// Its start ends every active Pokémon's item effect (nothing visible).
    pub(crate) fn use_magic_room(&mut self, r: MonRef) -> bool {
        if self.field.magic_room > 0 {
            self.field.magic_room = 0;
            self.end_magic_room();
            return true;
        }
        self.field.magic_room = self
            .dex
            .cond_num("magicroom", "duration")
            .map(|x| x as u8)
            .unwrap_or(5);
        self.field.magic_room_source = Some(r);
        let ri = self.ident(r);
        log_line!(self, "|-fieldstart|move: Magic Room|[of] {ri}");
        true
    }

    pub(crate) fn end_magic_room(&mut self) {
        let who = self
            .field
            .magic_room_source
            .take()
            .map(|s| self.ident(s))
            .unwrap_or_default();
        log_line!(self, "|-fieldend|move: Magic Room|[of] {who}");
    }

    /// Wonder Room: starts, or ends when already active (`onFieldRestart`).
    pub(crate) fn use_wonder_room(&mut self, r: MonRef) -> bool {
        if self.field.wonder_room > 0 {
            self.field.wonder_room = 0;
            log_line!(self, "|-fieldend|move: Wonder Room");
            return true;
        }
        self.field.wonder_room = self
            .dex
            .cond_num("wonderroom", "duration")
            .map(|x| x as u8)
            .unwrap_or(5);
        self.field.wonder_room_armed = false;
        let ri = self.ident(r);
        log_line!(self, "|-fieldstart|move: Wonder Room|[of] {ri}");
        true
    }

    /// `calculateStat`'s swap: under Wonder Room the stored Defense and
    /// Special Defense trade places (the boosts and the events keep the
    /// stat the move asked for).
    pub(crate) fn wonder_room_stat(&self, st: Stat) -> Stat {
        if self.field.wonder_room == 0 {
            return st;
        }
        match st {
            Stat::Def => Stat::Spd,
            Stat::Spd => Stat::Def,
            other => other,
        }
    }

    /// Wonder Room's `onModifyMove`: a move that attacks with a defensive
    /// stat (Body Press) uses the other one's boosts.
    pub(crate) fn wonder_room_on_modify_move(&mut self, mv: &Move) -> Option<Move> {
        if self.field.wonder_room == 0 {
            return None;
        }
        let st = mv
            .override_offensive_stat
            .filter(|s| matches!(s, Stat::Def | Stat::Spd))?;
        let sp = if st == Stat::Def { "" } else { "Sp. " };
        self.add(format!(
            "|-hint|{} uses {sp}Def boosts when Wonder Room is active.",
            mv.name
        ));
        let mut m = mv.clone();
        m.override_offensive_stat = Some(self.wonder_room_stat(st));
        Some(m)
    }

    /// `addPseudoWeather('gravity')`: fails when already in effect. Its start
    /// would also bring down Pokémon in the air (Fly, Bounce, Sky Drop,
    /// Magnet Rise, Telekinesis), none of which the engine models yet.
    pub(crate) fn add_gravity(&mut self) -> bool {
        if self.field.gravity > 0 {
            return false;
        }
        self.field.gravity = self
            .dex
            .cond_num("gravity", "duration")
            .map(|x| x as u8)
            .unwrap_or(5);
        self.add("|-fieldstart|move: Gravity");
        self.gravity_on_field_start();
        true
    }

    /// Gravity's `BeforeMove` (priority 6, after the Pokémon's own handlers
    /// of that priority): moves with the `gravity` flag can't be used.
    pub(crate) fn gravity_on_before_move(&mut self, r: MonRef, mv: &Move) -> bool {
        if self.field.gravity == 0 || !mv.has(MoveFlag::Gravity) {
            return false;
        }
        let ri = self.ident(r);
        log_line!(self, "|cant|{ri}|move: Gravity|{}", mv.name);
        true
    }

    /// `addPseudoWeather('fairylock')`: fails when already in effect.
    pub(crate) fn add_fairy_lock(&mut self) -> bool {
        if self.field.fairy_lock > 0 {
            return false;
        }
        self.field.fairy_lock = self
            .dex
            .cond_num("fairylock", "duration")
            .map(|x| x as u8)
            .unwrap_or(2);
        self.add("|-fieldactivate|move: Fairy Lock");
        true
    }

    /// Gravity's `onFieldStart`: Pokémon in the air (Fly, Bounce) come down,
    /// losing the move and their queued action.
    pub(crate) fn gravity_on_field_start(&mut self) {
        use crate::kinds::ChargeKind as C;
        for r in self.all_active() {
            let mut applies = false;
            if matches!(self.charging_kind(r), Some(C::Fly | C::Bounce)) {
                if let Some(i) = self.queue.iter().position(|a| {
                    matches!(a.kind, crate::state::ActionKind::Move { side, mon, .. } if (side, mon) == r)
                }) {
                    self.queue.remove(i);
                }
                self.end_two_turn(r);
                applies = true;
            }
            // Magnet Rise ends silently.
            if self.mon(r).vol.magnet_rise > 0 {
                self.mon_mut(r).vol.magnet_rise = 0;
                applies = true;
            }
            if applies {
                let ri = self.ident(r);
                log_line!(self, "|-activate|{ri}|move: Gravity");
            }
        }
    }
}
