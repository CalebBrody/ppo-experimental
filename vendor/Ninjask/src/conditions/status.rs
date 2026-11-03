//! Applying statuses.

use crate::battle::{Battle, MonRef};
use crate::dex::types::*;
use crate::kinds::EffectRef;
use crate::state::*;

impl Battle {
    /// Showdown `cureStatus`.
    pub(crate) fn cure_status(&mut self, r: MonRef) {
        let m = self.mon(r);
        if m.hp == 0 || m.status == Status::None {
            return;
        }
        let ident = self.ident(r);
        let st = m.status.id();
        log_line!(self, "|-curestatus|{ident}|{st}|[msg]");
        let m = self.mon_mut(r);
        m.status = Status::None;
        m.status_state = Status::None;
        m.status_time = 0;
        m.toxic_stage = 0;
    }

    /// Status from an ability (Flame Body, Static, Poison Touch...).
    pub(crate) fn try_set_status_from(
        &mut self,
        t: MonRef,
        st: Status,
        src: MonRef,
        effect: EffectRef,
    ) -> bool {
        self.try_set_status(t, st, src, effect, false)
    }

    /// Showdown `trySetStatus` / `setStatus` with type, weather, ability and
    /// terrain immunities. `status_move` is true when the status is the move's
    /// own effect (Thunder Wave), which adds failure messages.
    pub(crate) fn try_set_status(
        &mut self,
        t: MonRef,
        st: Status,
        src: MonRef,
        effect: EffectRef,
        status_move: bool,
    ) -> bool {
        let m = self.mon(t);
        if m.hp == 0 {
            return false;
        }
        let ti = self.ident(t);
        if m.status != Status::None {
            if status_move {
                if m.status == st {
                    log_line!(self, "|-fail|{ti}|{}", st.id());
                } else {
                    self.fail_move(src);
                }
            }
            return false;
        }
        let types = self.types_of(t);
        let immunity = if st == Status::Tox { "psn" } else { st.id() };
        let sun_freeze = st == Status::Frz && self.effective_weather() == Some(Weather::Sun);
        let magma = st == Status::Frz && self.ability_id(t) == "magmaarmor";
        let corrosion = self.corrosion_poisons(src, st);
        if (!corrosion
            && types
                .iter()
                .any(|&x| self.dex.type_immune_to_effect(x, immunity)))
            || sun_freeze
            || magma
        {
            if status_move {
                log_line!(self, "|-immune|{ti}");
            }
            return false;
        }
        // runEvent('SetStatus'): every handler joins the speed sort before
        // any acts, and the first to stop the status wins (Sweet Veil on a
        // faster ally before the target's Insomnia).
        if self
            .run_set_status(t, st, src, effect, status_move, false)
            .is_some()
        {
            return false;
        }
        let time = match st {
            Status::Slp => self.rng.sample(&[2u8, 3, 3]),
            Status::Frz => 3,
            _ => 0,
        };
        let m = self.mon_mut(t);
        m.status = st;
        m.status_state = st;
        m.status_time = time;
        m.status_start_time = time;
        m.toxic_stage = 0;
        let suffix = match effect {
            EffectRef::Ability(a) => {
                let si = self.ident(src);
                format!("|[from] ability: {}|[of] {si}", self.dex.ability(a).name)
            }
            EffectRef::Move(mv) if st == Status::Slp => {
                format!("|[from] move: {}", self.dex.mv(mv).name)
            }
            _ => String::new(),
        };
        log_line!(self, "|-status|{ti}|{}{suffix}", st.id());
        self.after_set_status(t, st, src, effect);
        true
    }
}
