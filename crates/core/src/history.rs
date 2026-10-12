//! Match History (TRIAGE row 88): the last matches played or spectated on
//! this device, newest first, on a Main Menu screen.
//!
//! The core owns the stored format and its version. When a match ends the
//! core records it and hands the page the whole history as JSON text to
//! keep in browser storage (`ui().save_history`), as it does for the
//! settings; on load the page passes that text back (`history.load`). The
//! core reads no clock, so matches are numbered (Match 1, Match 2, …) rather
//! than dated.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app::{biome_key, clock, difficulty_key, format_key, map_type_key, App, Screen};
use crate::lockstep::SeatKind as NetSeat;
use crate::rules::Resource;
use crate::world::Controller;

/// The stored format's version. Text with another version is not read.
pub const HISTORY_VERSION: u32 = 1;
/// How many matches are kept; the oldest goes first.
pub const HISTORY_KEEP: usize = 50;
/// Matches listed per page of the Match History screen.
pub const HISTORY_PAGE: usize = 10;

/// How a recorded match ended for this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchResult {
    Victory,
    Defeat,
    Conceded,
    /// Watched, not played: the winner is shown instead.
    Spectate,
}

/// One colony of a recorded match: its seat, team, who played it, and
/// the end-screen summary numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryColony {
    pub seat: u8,
    pub team: u8,
    /// This device's colony.
    #[serde(default)]
    pub you: bool,
    /// An online player's Player Name; empty otherwise.
    #[serde(default)]
    pub name: String,
    /// The AI's Difficulty key ("pupa"), when an AI played it at the end.
    #[serde(default)]
    pub ai: Option<String>,
    pub trained: i32,
    pub killed: i32,
    pub lost: i32,
    pub gathered: [i32; 3],
}

/// One recorded match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryMatch {
    pub number: u32,
    pub result: MatchResult,
    /// The winning team; `None` when no team was left.
    pub winner: Option<u8>,
    pub online: bool,
    pub format: String,
    pub map_type: String,
    pub biome: String,
    /// Match Length in ticks.
    pub ticks: i32,
    pub colonies: Vec<HistoryColony>,
}

/// What the page keeps in browser storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub version: u32,
    /// The number the next recorded match gets.
    pub next: u32,
    /// Newest first.
    pub matches: Vec<HistoryMatch>,
}

impl Default for Stored {
    fn default() -> Self {
        Stored {
            version: HISTORY_VERSION,
            next: 1,
            matches: Vec::new(),
        }
    }
}

impl Stored {
    /// Saved text from the page. Anything unreadable, or of another
    /// version, gives an empty history.
    pub fn parse(text: &str) -> Stored {
        match serde_json::from_str::<Stored>(text) {
            Ok(mut s) if s.version == HISTORY_VERSION => {
                s.matches.truncate(HISTORY_KEEP);
                let top = s.matches.iter().map(|m| m.number).max().unwrap_or(0);
                s.next = s.next.max(top + 1);
                s
            }
            _ => Stored::default(),
        }
    }

    /// Adds a match as the newest, numbering it, and drops the oldest past
    /// `HISTORY_KEEP`.
    pub fn push(&mut self, mut m: HistoryMatch) {
        m.number = self.next;
        self.next += 1;
        self.matches.insert(0, m);
        self.matches.truncate(HISTORY_KEEP);
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string(self).expect("serialises")
    }
}

/// Kept on each match (`Game::history_mark`), so a new match always
/// starts unrecorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mark {
    /// This match is already recorded.
    pub recorded: bool,
    /// You pressed Concede in this match.
    pub conceded: bool,
}

/// The stored history and the screen's state.
#[derive(Debug, Clone, Default)]
pub struct MatchHistory {
    pub stored: Stored,
    /// The number of the match whose summary is shown; `None` is the newest.
    pub selected: Option<u32>,
    /// The page of the list, 0 newest.
    pub page: usize,
    pub confirm_clear: bool,
    /// Text for the page to store, taken by `ui()`.
    pub save: Option<String>,
}

impl MatchHistory {
    fn changed(&mut self) {
        self.save = Some(self.stored.to_text());
    }
}

impl App {
    /// Called from `ui()` every frame: records a match once it has a
    /// winner, and hands the page the history to store when it changed.
    pub fn history_frame(&mut self, v: &mut Value) {
        let due = self
            .game
            .as_ref()
            .is_some_and(|g| g.world.winner.is_some() && !g.history_mark.recorded);
        if due {
            self.record_match();
        }
        if let Some(text) = self.match_history.save.take() {
            v["save_history"] = json!(text);
        }
    }

    /// The Match History actions, and the moments it must notice: Concede,
    /// and leaving a match you were defeated in before it ended (2v2, a
    /// Free-for-all). Returns true when the action was its own.
    pub fn history_act(&mut self, id: &str, value: &str) -> bool {
        let h = &mut self.match_history;
        match id {
            "history.load" => h.stored = Stored::parse(value),
            "go.match_history" => {
                h.selected = None;
                h.page = 0;
                h.confirm_clear = false;
                self.screen = Screen::MatchHistory;
            }
            "history.older" => {
                if (h.page + 1) * HISTORY_PAGE < h.stored.matches.len() {
                    h.page += 1;
                }
            }
            "history.newer" => h.page = h.page.saturating_sub(1),
            "history.clear" => h.confirm_clear = true,
            "history.clear_no" => h.confirm_clear = false,
            "history.clear_yes" => {
                let next = h.stored.next;
                h.stored = Stored::default();
                // Numbers keep counting, so a number is never reused.
                h.stored.next = next;
                h.selected = None;
                h.page = 0;
                h.confirm_clear = false;
                h.changed();
            }
            _ => {
                if let Some(n) = id.strip_prefix("history.show.") {
                    h.selected = n.parse().ok();
                    return true;
                }
                let Some(g) = &mut self.game else {
                    return false;
                };
                if id == "menu.concede_yes" && !g.spectate {
                    g.history_mark.conceded = true;
                }
                let leaving = id == "go.main_menu" || id == "end.main_menu";
                let defeated_early = !g.spectate
                    && !g.history_mark.recorded
                    && g.world.winner.is_none()
                    && g.world.colonies[g.me as usize].defeated;
                if leaving && defeated_early {
                    self.record_match();
                }
                return false;
            }
        }
        true
    }

    /// Records the current match as the newest.
    fn record_match(&mut self) {
        let Some(g) = &self.game else {
            return;
        };
        let w = &g.world;
        let seats = g.lockstep.as_ref().map(|m| m.seats());
        let colonies: Vec<HistoryColony> = w
            .colonies
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let name = match seats.as_ref().and_then(|s| s.get(i)).map(|s| &s.kind) {
                    Some(NetSeat::Human { name, .. }) => name.clone(),
                    _ => String::new(),
                };
                HistoryColony {
                    seat: i as u8,
                    team: c.team,
                    you: !g.spectate && i == g.me as usize,
                    name,
                    ai: match c.controller {
                        Controller::Ai(d) => Some(difficulty_key(d).to_string()),
                        Controller::Human => None,
                    },
                    trained: c.tally.trained,
                    killed: c.tally.killed,
                    lost: c.tally.lost,
                    gathered: c.tally.gathered,
                }
            })
            .collect();
        let mine = w.colonies[g.me as usize].team;
        let result = if g.spectate {
            MatchResult::Spectate
        } else if g.history_mark.conceded {
            MatchResult::Conceded
        } else if w.winner == Some(mine) {
            MatchResult::Victory
        } else {
            MatchResult::Defeat
        };
        let entry = HistoryMatch {
            number: 0,
            result,
            winner: w.winner.filter(|t| w.colonies.iter().any(|c| c.team == *t)),
            online: g.lockstep.is_some(),
            format: format_key(w.settings.format).to_string(),
            map_type: map_type_key(w.settings.map_type).to_string(),
            biome: biome_key(w.settings.biome).to_string(),
            ticks: g.ended_at.unwrap_or(w.tick),
            colonies,
        };
        if let Some(g) = &mut self.game {
            g.history_mark.recorded = true;
        }
        let h = &mut self.match_history;
        h.stored.push(entry);
        h.selected = None;
        h.page = 0;
        h.changed();
    }

    fn history_button(&self, id: &str, key: &str, disabled: bool) -> Value {
        json!({"type": "button", "id": id, "text": self.t(key), "disabled": disabled})
    }

    fn history_match_name(&self, m: &HistoryMatch) -> String {
        format!("{} {}", self.t("history.match"), m.number)
    }

    /// A colony's column head: "Blue · AI · Pupa", "Red · You", "Yellow · Ana".
    fn history_colony_name(&self, c: &HistoryColony) -> String {
        let mut parts = vec![self.t(&format!("color.{}", c.seat))];
        if c.you {
            parts.push(self.t("seat.you"));
        } else if !c.name.is_empty() {
            parts.push(c.name.clone());
        }
        if let Some(d) = &c.ai {
            parts.push(self.t("seat.ai"));
            parts.push(self.t(&format!("difficulty.{d}")));
        }
        parts.join(" · ")
    }

    /// A team's name, as on the end screen: its player color when it is one
    /// colony, else "Team 2".
    fn history_team_name(&self, m: &HistoryMatch, team: u8) -> String {
        let members: Vec<&HistoryColony> = m.colonies.iter().filter(|c| c.team == team).collect();
        if members.len() == 1 {
            self.t(&format!("color.{}", members[0].seat))
        } else {
            format!("{} {}", self.t("seat.team"), team as i32 + 1)
        }
    }

    fn history_result(&self, m: &HistoryMatch) -> String {
        match m.result {
            MatchResult::Victory => self.t("end.victory"),
            MatchResult::Defeat => self.t("end.defeat"),
            MatchResult::Conceded => self.t("history.conceded"),
            MatchResult::Spectate => match m.winner {
                Some(t) => format!(
                    "{}: {}",
                    self.t("end.victory"),
                    self.history_team_name(m, t)
                ),
                None => self.t("stat.none"),
            },
        }
    }

    fn history_mode(&self, m: &HistoryMatch) -> String {
        self.t(match (m.online, m.result) {
            (true, _) => "menu.online",
            (false, MatchResult::Spectate) => "history.spectate",
            (false, _) => "menu.play_vs_ai",
        })
    }

    /// The Match History screen: one page of matches beside the chosen
    /// match's summary, and Back, Older, Newer, Clear.
    pub fn history_panels(&self) -> Vec<Value> {
        let h = &self.match_history;
        let all = &h.stored.matches;
        let page = h.page.min(all.len().saturating_sub(1) / HISTORY_PAGE);
        let shown = h
            .selected
            .and_then(|n| all.iter().find(|m| m.number == n))
            .or(all.first());
        let rows: Vec<Value> = all
            .iter()
            .skip(page * HISTORY_PAGE)
            .take(HISTORY_PAGE)
            .map(|m| {
                let mut b = json!({"type": "button", "id": format!("history.show.{}", m.number),
                    "text": self.history_match_name(m)});
                if shown.is_some_and(|s| s.number == m.number) {
                    b["active"] = json!(true);
                }
                json!([
                    b,
                    self.history_result(m),
                    self.t(&format!("format.{}", m.format)),
                    self.t(&format!("map_type.{}", m.map_type)),
                    clock(m.ticks),
                ])
            })
            .collect();
        let mut list = vec![json!({
            "type": "table",
            "head": [
                self.t("history.match"),
                self.t("history.result"),
                self.t("setting.format"),
                self.t("setting.map_type"),
                self.t("stat.match_length"),
            ],
            "rows": rows,
        })];
        if all.is_empty() {
            list.push(
                json!({"type": "list", "rows": [[self.t("history.match"), self.t("stat.none")]]}),
            );
        }
        // Clear asks first, in the list's heading, as Concede does.
        let heading = self.t(if h.confirm_clear {
            "history.clear_confirm"
        } else {
            "menu.match_history"
        });
        let mut panels =
            vec![json!({"kind": "form", "single": true, "heading": heading, "items": list})];
        if let Some(m) = shown {
            panels.push(self.history_summary(m));
        }
        let buttons = if h.confirm_clear {
            vec![
                self.history_button("history.clear_no", "menu.back", false),
                self.history_button("history.clear_yes", "history.clear", false),
            ]
        } else {
            vec![
                self.history_button("go.main_menu", "menu.back", false),
                self.history_button("history.newer", "history.newer", page == 0),
                self.history_button(
                    "history.older",
                    "history.older",
                    (page + 1) * HISTORY_PAGE >= all.len(),
                ),
                self.history_button("history.clear", "history.clear", all.is_empty()),
            ]
        };
        panels.push(json!({"kind": "row", "items": buttons}));
        panels
    }

    /// One match: label-value pairs, then the end-screen summary with a
    /// column per colony.
    fn history_summary(&self, m: &HistoryMatch) -> Value {
        let pairs = json!({"type": "list", "rows": [
            [self.t("history.result"), self.history_result(m)],
            [self.t("history.mode"), self.history_mode(m)],
            [self.t("setting.format"), self.t(&format!("format.{}", m.format))],
            [self.t("setting.map_type"), self.t(&format!("map_type.{}", m.map_type))],
            [self.t("setting.biome"), self.t(&format!("biome.{}", m.biome))],
            [self.t("stat.match_length"), clock(m.ticks)],
        ]});
        let mut head = vec![String::new()];
        head.extend(m.colonies.iter().map(|c| self.history_colony_name(c)));
        let row = |label: String, f: &dyn Fn(&HistoryColony) -> String| {
            let mut r = vec![label];
            r.extend(m.colonies.iter().map(f));
            r
        };
        let mut rows = vec![
            row(self.t("seat.team"), &|c| (c.team as i32 + 1).to_string()),
            row(self.t("stat.trained"), &|c| c.trained.to_string()),
            row(self.t("stat.killed"), &|c| c.killed.to_string()),
            row(self.t("stat.lost"), &|c| c.lost.to_string()),
        ];
        for r in Resource::ALL {
            let label = format!(
                "{}: {}",
                self.t("stat.gathered"),
                self.t(&format!("resource.{}", r.key()))
            );
            rows.push(row(label, &|c| c.gathered[r.index()].to_string()));
        }
        json!({"kind": "form", "single": true, "heading": self.history_match_name(m),
            "items": [pairs, {"type": "table", "head": head, "rows": rows}]})
    }
}
