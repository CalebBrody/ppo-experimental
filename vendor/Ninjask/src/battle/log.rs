//! Battle log in Showdown's protocol, split per viewer like Showdown does.

use super::{Battle, MonRef};

/// A log entry. `Split` lines differ by viewer: the owning side sees `secret`
/// (exact HP), everyone else sees `shared` (percentage HP).
#[derive(Clone, Debug)]
pub enum LogEntry {
    Plain(String),
    Split {
        side: usize,
        secret: String,
        shared: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Side(usize),
    Spectator,
    Omniscient,
}

impl LogEntry {
    pub fn render(&self, viewer: Viewer) -> &str {
        match self {
            LogEntry::Plain(s) => s,
            LogEntry::Split {
                side,
                secret,
                shared,
            } => match viewer {
                Viewer::Side(v) if v == *side => secret,
                Viewer::Omniscient => secret,
                _ => shared,
            },
        }
    }
}

/// HP as Showdown reports it to the owner: "hp/maxhp" plus status.
pub fn secret_health(hp: u16, maxhp: u16, status: &str) -> String {
    if hp == 0 {
        return "0 fnt".into();
    }
    if status.is_empty() {
        format!("{hp}/{maxhp}")
    } else {
        format!("{hp}/{maxhp} {status}")
    }
}

/// HP as Champions shows it to others (Showdown `getHealth`, champions branch):
/// floor percentage (at least 1), with a colour flag at 20% and 50%.
pub fn shared_health(hp: u16, maxhp: u16, status: &str) -> String {
    if hp == 0 {
        return "0 fnt".into();
    }
    let pct = ((100 * hp as u32) / maxhp as u32).max(1);
    let mut out = format!("{pct}/100");
    if pct == 20 {
        out.push(if (hp as u32) * 5 > maxhp as u32 {
            'y'
        } else {
            'r'
        });
    } else if pct == 50 {
        out.push(if (hp as u32) * 2 > maxhp as u32 {
            'g'
        } else {
            'y'
        });
    }
    if !status.is_empty() {
        out.push(' ');
        out.push_str(status);
    }
    out
}

impl Battle {
    /// Appends a log line; with logging off, nothing (a `&str` line is not
    /// even copied).
    pub fn add(&mut self, line: impl Into<String>) {
        if self.log_enabled {
            self.log.push(LogEntry::Plain(line.into()));
        }
    }

    fn add_split(&mut self, side: usize, secret: String, shared: String) {
        if self.log_enabled {
            self.log.push(LogEntry::Split {
                side,
                secret,
                shared,
            });
        }
    }

    /// Log lines for a viewer, from index `from`.
    pub fn log_lines(&self, viewer: Viewer, from: usize) -> Vec<String> {
        self.log[from.min(self.log.len())..]
            .iter()
            .map(|e| e.render(viewer).to_string())
            .collect()
    }

    /// A Pokémon's log identity ("p1a: Name"); empty when logging is off,
    /// since only log lines use it.
    #[inline(always)]
    pub fn ident(&self, r: MonRef) -> String {
        if !self.log_enabled {
            return String::new();
        }
        self.format_ident(r)
    }

    #[inline(never)]
    fn format_ident(&self, (s, i): MonRef) -> String {
        let m = &self.sides[s].mons[i];
        // Illusion names the Pokémon after its disguise.
        let name = m.illusion.as_deref().unwrap_or(&m.name);
        // A fainted Pokémon is no longer active (Showdown clears `isActive`
        // right after its faint line), so it is named without a slot.
        if m.is_active && !m.fainted && !m.revived {
            format!(
                "{}{}: {}",
                self.sides[s].id(),
                ["a", "b", "c"][m.position],
                name
            )
        } else {
            format!("{}: {}", self.sides[s].id(), name)
        }
    }

    pub fn details(&self, s: usize, i: usize) -> String {
        let species = &self.dex.species(self.sides[s].mons[i].base_species).name;
        let mut d = String::with_capacity(species.len() + 16);
        self.write_details(s, i, &mut d);
        d
    }

    /// `details` appended to `d`.
    pub(crate) fn write_details(&self, s: usize, i: usize, d: &mut String) {
        let m = &self.sides[s].mons[i];
        let species = &self.dex.species(m.base_species).name;
        d.push_str(species);
        if m.level != 100 {
            d.push_str(", L");
            // The level's decimal digits (a u8), without the formatter.
            let l = m.level;
            if l >= 100 {
                d.push((b'0' + l / 100) as char);
            }
            if l >= 10 {
                d.push((b'0' + l / 10 % 10) as char);
            }
            d.push((b'0' + l % 10) as char);
        }
        if m.gender == "M" || m.gender == "F" {
            d.push_str(", ");
            d.push_str(&m.gender);
        }
        if m.shiny {
            d.push_str(", shiny");
        }
    }

    fn health_pair(&self, (s, i): MonRef) -> (String, String) {
        let m = &self.sides[s].mons[i];
        (
            secret_health(m.hp, m.maxhp, m.status.id()),
            shared_health(m.hp, m.maxhp, m.status.id()),
        )
    }

    /// Add a line whose last field is the Pokémon's health, split per viewer.
    pub(crate) fn add_health(
        &mut self,
        prefix: std::fmt::Arguments<'_>,
        who: MonRef,
        suffix: &str,
    ) {
        if !self.log_enabled {
            return;
        }
        let (sec, sha) = self.health_pair(who);
        self.add_split(
            who.0,
            format!("{prefix}|{sec}{suffix}"),
            format!("{prefix}|{sha}{suffix}"),
        );
    }
}
