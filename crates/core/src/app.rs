//! The application around a match: screens, menus, Match Settings and
//! Player Settings, the action grid, tooltips, the HUD, input, and running a
//! match locally (Play vs AI) or in lockstep (Online Multiplayer).
//!
//! The page draws what `ui()` returns and forwards input; it decides nothing.
//! Every on-screen word comes from `data/strings.json`, every hover hint from
//! `data/glossary.json`, and all of it follows GLOSSARY.md: labels and
//! label-value pairs, not prose.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::fixed::{Pos, CELL};
use crate::keys::{key_label, PlayerSettings};
use crate::lockstep::{
    Lobby, LobbyStatus, Match, MatchStatus, Outgoing, PeerId, Seat, SeatKind as NetSeat, Target,
    Version,
};
use crate::rules::{
    evolve_cost, Resource, Role, Species, StructureKind, Upgrade, TICKS_PER_SECOND,
};
use crate::settings::{
    can_disable, Biome, Difficulty, Format, GameSpeed, Level, MatchSettings, SeatKind, TUNNEL_MAX,
    TUNNEL_MIN,
};
use crate::world::{Amount, Command, Controller, Focus, Refusal, Work, World};

/// Peers with different builds refuse each other; raise on every release
/// that changes the simulation or the wire.
pub const BUILD_ID: &str = "ant-colony-1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MainMenu,
    PlayVsAi,
    MatchSettings,
    PlayerSettings,
    Online,
    Lobby,
    Match,
}

/// What a grid button does in the current tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Train(Species),
    Place(StructureKind),
    Waypoint(Option<Species>),
    Research(Upgrade),
    Evolve,
    Retreat,
    Focus(Focus),
}

pub const TABS: [&str; 5] = ["ants", "build", "waypoints", "upgrades", "colony"];

const ROW_SPECIES: [Species; 8] = [
    Species::Fire,
    Species::Turtle,
    Species::Acid,
    Species::Nurse,
    Species::Flying,
    Species::Exploding,
    Species::TrapJaw,
    Species::Dinosaur,
];

/// The 12 buttons of a tab, in grid order (rows Q W E R / A S D F / Z X C V).
/// A species keeps its position across tabs: Fire Ant is always A.
pub fn grid(tab: usize) -> [Option<Button>; 12] {
    let mut g = [None; 12];
    match tab {
        0 => {
            g[0] = Some(Button::Train(Species::Harvester));
            g[1] = Some(Button::Train(Species::Soldier));
            g[2] = Some(Button::Train(Species::Carpenter));
            g[3] = Some(Button::Train(Species::Leafcutter));
            for (i, s) in ROW_SPECIES.iter().enumerate() {
                g[4 + i] = Some(Button::Train(*s));
            }
        }
        1 => {
            g[0] = Some(Button::Place(StructureKind::Queen));
            g[1] = Some(Button::Place(StructureKind::BroodCell));
            g[2] = Some(Button::Place(StructureKind::Wall));
            g[3] = Some(Button::Place(StructureKind::Hatchery(Species::Leafcutter)));
            for (i, s) in ROW_SPECIES.iter().enumerate() {
                g[4 + i] = Some(Button::Place(StructureKind::Hatchery(*s)));
            }
        }
        2 => {
            g[0] = Some(Button::Waypoint(None));
            g[1] = Some(Button::Waypoint(Some(Species::Soldier)));
            for (i, s) in ROW_SPECIES.iter().enumerate() {
                g[4 + i] = Some(Button::Waypoint(Some(*s)));
            }
        }
        3 => {
            g[0] = Some(Button::Research(Upgrade::Damage));
            g[1] = Some(Button::Research(Upgrade::Species(Species::Soldier)));
            g[2] = Some(Button::Research(Upgrade::Armor));
            g[3] = Some(Button::Research(Upgrade::Health));
            for (i, s) in ROW_SPECIES.iter().enumerate() {
                g[4 + i] = Some(Button::Research(Upgrade::Species(*s)));
            }
        }
        _ => {
            g[0] = Some(Button::Evolve);
            g[1] = Some(Button::Retreat);
            g[2] = Some(Button::Research(Upgrade::GatherRate));
            g[3] = Some(Button::Research(Upgrade::CarryCapacity));
            g[4] = Some(Button::Focus(Focus::Balanced));
            g[5] = Some(Button::Focus(Focus::Food));
            g[6] = Some(Button::Focus(Focus::Nectar));
            g[7] = Some(Button::Focus(Focus::Leaves));
            g[8] = Some(Button::Research(Upgrade::BuildRate));
        }
    }
    g
}

/// What a click on the map does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Place(StructureKind),
    /// A wall: press at one end, drag, release at the other.
    Wall(Option<(i32, i32)>),
    Waypoint(Option<Species>),
}

/// The camera: the world point at the centre of the view, and the zoom as
/// screen pixels per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub cx: i32,
    pub cy: i32,
    pub px_per_cell: i32,
    pub w: i32,
    pub h: i32,
}

pub const ZOOM_MIN: i32 = 3;
pub const ZOOM_MAX: i32 = 24;

impl View {
    pub fn to_world(&self, sx: i32, sy: i32) -> Pos {
        Pos::new(
            self.cx + (sx - self.w / 2) * CELL / self.px_per_cell,
            self.cy + (sy - self.h / 2) * CELL / self.px_per_cell,
        )
    }

    pub fn to_screen(&self, p: Pos) -> (i32, i32) {
        (
            (p.x - self.cx) * self.px_per_cell / CELL + self.w / 2,
            (p.y - self.cy) * self.px_per_cell / CELL + self.h / 2,
        )
    }

    pub fn clamp(&mut self, map_w: i32, map_h: i32) {
        self.px_per_cell = self.px_per_cell.clamp(ZOOM_MIN, ZOOM_MAX);
        self.cx = self.cx.clamp(0, map_w * CELL);
        self.cy = self.cy.clamp(0, map_h * CELL);
    }
}

pub struct Game {
    pub world: World,
    pub me: u8,
    pub tab: usize,
    pub mode: Mode,
    pub view: View,
    pub mouse: (i32, i32),
    pub drag: Option<(i32, i32, i32, i32)>,
    pub pan_keys: [bool; 4],
    pub menu_open: bool,
    pub confirm_concede: bool,
    /// Milliseconds of game time not yet simulated.
    pub acc_ms: i64,
    pub last_ms: Option<i64>,
    pub pending: Vec<Command>,
    pub lockstep: Option<Match>,
    pub ended_at: Option<i32>,
}

impl Game {
    pub fn new(world: World, me: u8, lockstep: Option<Match>) -> Game {
        let h = world.map.home(me as usize);
        let view = View {
            cx: h.cx * CELL + CELL / 2,
            cy: h.cy * CELL + CELL / 2,
            px_per_cell: 10,
            w: 1280,
            h: 720,
        };
        Game {
            world,
            me,
            tab: 0,
            mode: Mode::Idle,
            view,
            mouse: (0, 0),
            drag: None,
            pan_keys: [false; 4],
            menu_open: false,
            confirm_concede: false,
            acc_ms: 0,
            last_ms: None,
            pending: Vec::new(),
            lockstep,
            ended_at: None,
        }
    }

    /// Issues a command for the local colony: into the next tick when
    /// local, into the next turn when online.
    pub fn issue(&mut self, cmd: Command) {
        match &mut self.lockstep {
            Some(m) => m.queue_command(serde_json::to_string(&cmd).expect("serialises")),
            None => self.pending.push(cmd),
        }
    }

    pub fn ms_per_tick(&self) -> i64 {
        self.world.ms_per_tick()
    }

    /// Thousandths of the way from the last tick to the next, for drawing.
    pub fn interp_permille(&self) -> i32 {
        (self.acc_ms * 1000 / self.ms_per_tick().max(1)).clamp(0, 1000) as i32
    }

    fn mouse_cell(&self) -> (i32, i32) {
        self.view.to_world(self.mouse.0, self.mouse.1).cell()
    }
}

/// A lobby or match being set up online.
pub struct Online {
    pub lobby: Option<Lobby>,
    pub room: String,
    pub outbox: Vec<Outgoing>,
    /// Requests for the page's transport: host/join a room, or code steps.
    pub requests: Vec<Value>,
    pub invite: String,
    pub reply: String,
    pub status: String,
    /// Peers met before the host: told to the lobby once it exists.
    pub early_peers: Vec<PeerId>,
}

pub struct App {
    pub s: BTreeMap<String, String>,
    pub hints: BTreeMap<String, String>,
    pub screen: Screen,
    pub defaults: MatchSettings,
    pub player: PlayerSettings,
    /// Play vs AI: this match's settings and each seat's difficulty.
    pub setup: MatchSettings,
    pub ai_seats: Vec<Difficulty>,
    pub seed_text: String,
    pub rebinding: Option<String>,
    pub game: Option<Game>,
    pub online: Online,
    pub client_id: String,
    pub now_ms: i64,
    /// Settings to save in browser storage, taken by the page.
    pub save: Option<Value>,
    /// The seed for a new match, supplied by the page (it has the entropy).
    pub entropy: u64,
    pub navigate: Option<String>,
}

fn text_map(json: &str) -> Result<BTreeMap<String, String>, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

fn roman(n: u8) -> &'static str {
    match n {
        1 => "I",
        2 => "II",
        3 => "III",
        _ => "",
    }
}

/// "12.5" style text from tenths.
fn tenths(v: i32) -> String {
    format!("{}.{}", v / 10, (v % 10).abs())
}

impl App {
    pub fn new(
        strings_json: &str,
        glossary_json: &str,
        saved_defaults: &str,
        saved_player: &str,
        client_id: &str,
    ) -> Result<App, String> {
        let s = text_map(strings_json)?;
        let hints = text_map(glossary_json)?;
        let defaults = MatchSettings::parse(saved_defaults).unwrap_or_default();
        let player = PlayerSettings::parse(saved_player).unwrap_or_default();
        Ok(App {
            s,
            hints,
            screen: Screen::MainMenu,
            setup: defaults.clone(),
            defaults,
            player,
            ai_seats: vec![Difficulty::Pupa; 3],
            seed_text: String::new(),
            rebinding: None,
            game: None,
            online: Online {
                lobby: None,
                room: String::new(),
                outbox: Vec::new(),
                requests: Vec::new(),
                invite: String::new(),
                reply: String::new(),
                status: String::new(),
                early_peers: Vec::new(),
            },
            client_id: client_id.to_string(),
            now_ms: 0,
            save: None,
            entropy: 1,
            navigate: None,
        })
    }

    /// The text for a key; a missing key shows itself, so tests catch it.
    pub fn t(&self, key: &str) -> String {
        self.s.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    pub fn species_name(&self, s: Species) -> String {
        self.t(&format!("species.{}", s.key()))
    }

    pub fn structure_name(&self, k: StructureKind) -> String {
        self.t(&format!("structure.{}", k.key()))
    }

    pub fn upgrade_name(&self, u: Upgrade) -> String {
        self.t(&format!("upgrade.{}", u.key()))
    }

    fn cost_text(&self, c: &[i32; 3]) -> String {
        let parts: Vec<String> = Resource::ALL
            .iter()
            .filter(|r| c[r.index()] > 0)
            .map(|r| {
                format!(
                    "{} {}",
                    c[r.index()],
                    self.t(&format!("resource.{}", r.key()))
                )
            })
            .collect();
        if parts.is_empty() {
            self.t("stat.none")
        } else {
            parts.join(" · ")
        }
    }

    fn seconds(&self, ticks: i32) -> String {
        format!("{}{}", ticks / TICKS_PER_SECOND, self.t("unit.seconds"))
    }

    // ---- view model ----

    /// Everything the page shows, as JSON.
    pub fn ui(&mut self) -> Value {
        let mut v = json!({
            "screen": match self.screen {
                Screen::MainMenu => "main_menu",
                Screen::PlayVsAi => "play_vs_ai",
                Screen::MatchSettings => "match_settings",
                Screen::PlayerSettings => "player_settings",
                Screen::Online => "online",
                Screen::Lobby => "lobby",
                Screen::Match => "match",
            },
            "title": self.t("title"),
        });
        match self.screen {
            Screen::Match => {
                v["match"] = self.match_view();
            }
            _ => {
                v["panels"] = Value::Array(self.menu_panels());
            }
        }
        if let Some(save) = self.save.take() {
            v["save"] = save;
        }
        if !self.online.requests.is_empty() {
            v["net_requests"] = Value::Array(std::mem::take(&mut self.online.requests));
        }
        if let Some(n) = self.navigate.take() {
            v["navigate"] = json!(n);
        }
        v
    }

    fn button(&self, id: &str, key: &str) -> Value {
        json!({"type": "button", "id": id, "text": self.t(key)})
    }

    fn select(
        &self,
        id: &str,
        label_key: &str,
        options: Vec<(String, String)>,
        value: &str,
    ) -> Value {
        json!({
            "type": "select",
            "id": id,
            "label": self.t(label_key),
            "options": options.into_iter().map(|(v, t)| json!({"value": v, "text": t})).collect::<Vec<_>>(),
            "value": value,
        })
    }

    fn settings_fields(&self, m: &MatchSettings, prefix: &str, editable: bool) -> Vec<Value> {
        let lvl = |k: &str| -> Vec<(String, String)> {
            ["low", "normal", "high"]
                .iter()
                .map(|v| (v.to_string(), self.t(&format!("{k}.{v}"))))
                .collect()
        };
        let lv = |l: Level| match l {
            Level::Low => "low",
            Level::Normal => "normal",
            Level::High => "high",
        };
        if !editable {
            let rows = vec![
                (
                    self.t("setting.format"),
                    self.t(&format!("format.{}", format_key(m.format))),
                ),
                (
                    self.t("setting.map_size"),
                    self.t(&format!("size.{}", lv(m.map_size))),
                ),
                (
                    self.t("setting.chamber_size"),
                    self.t(&format!("size.{}", lv(m.chamber_size))),
                ),
                (self.t("setting.tunnel_width"), m.tunnel_width.to_string()),
                (
                    self.t("setting.resource_density"),
                    self.t(&format!("level.{}", lv(m.resource_density))),
                ),
                (
                    self.t("setting.starting_resources"),
                    self.t(&format!("level.{}", lv(m.starting_resources))),
                ),
                (
                    self.t("setting.game_speed"),
                    self.t(&format!("speed.{}", speed_key(m.game_speed))),
                ),
                (
                    self.t("setting.biome"),
                    self.t(&format!("biome.{}", biome_key(m.biome))),
                ),
                (
                    self.t("setting.events"),
                    self.t(if m.events { "on" } else { "off" }),
                ),
            ];
            return vec![json!({"type": "list", "rows": rows})];
        }
        let mut f = vec![
            self.select(
                &format!("{prefix}.format"),
                "setting.format",
                ["one_v_one", "two_v_two", "free_for_all"]
                    .iter()
                    .map(|v| (v.to_string(), self.t(&format!("format.{v}"))))
                    .collect(),
                format_key(m.format),
            ),
            self.select(
                &format!("{prefix}.map_size"),
                "setting.map_size",
                lvl("size"),
                lv(m.map_size),
            ),
            self.select(
                &format!("{prefix}.chamber_size"),
                "setting.chamber_size",
                lvl("size"),
                lv(m.chamber_size),
            ),
            self.select(
                &format!("{prefix}.tunnel_width"),
                "setting.tunnel_width",
                (TUNNEL_MIN..=TUNNEL_MAX)
                    .map(|w| (w.to_string(), w.to_string()))
                    .collect(),
                &m.tunnel_width.to_string(),
            ),
            self.select(
                &format!("{prefix}.resource_density"),
                "setting.resource_density",
                lvl("level"),
                lv(m.resource_density),
            ),
            self.select(
                &format!("{prefix}.starting_resources"),
                "setting.starting_resources",
                lvl("level"),
                lv(m.starting_resources),
            ),
            self.select(
                &format!("{prefix}.game_speed"),
                "setting.game_speed",
                ["slow", "normal", "fast"]
                    .iter()
                    .map(|v| (v.to_string(), self.t(&format!("speed.{v}"))))
                    .collect(),
                speed_key(m.game_speed),
            ),
            self.select(
                &format!("{prefix}.biome"),
                "setting.biome",
                ["picnic", "flower_field", "random"]
                    .iter()
                    .map(|v| (v.to_string(), self.t(&format!("biome.{v}"))))
                    .collect(),
                biome_key(m.biome),
            ),
            self.select(
                &format!("{prefix}.events"),
                "setting.events",
                vec![("on".into(), self.t("on")), ("off".into(), self.t("off"))],
                if m.events { "on" } else { "off" },
            ),
        ];
        let mut toggles = Vec::new();
        for s in Species::ALL {
            if can_disable(s) {
                toggles.push(json!({
                    "id": format!("{prefix}.disabled.{}", s.key()),
                    "label": self.species_name(s),
                    "value": m.is_disabled(s),
                }));
            }
        }
        f.push(json!({"type": "toggles", "label": self.t("setting.disabled"), "items": toggles}));
        f
    }

    fn menu_panels(&self) -> Vec<Value> {
        match self.screen {
            Screen::MainMenu => vec![json!({
                "kind": "leaves",
                "items": [
                    self.button("go.play_vs_ai", "menu.play_vs_ai"),
                    self.button("go.online", "menu.online"),
                    self.button("go.match_settings", "menu.match_settings"),
                    self.button("go.player_settings", "menu.player_settings"),
                ]
            })],
            Screen::MatchSettings => vec![
                json!({"kind": "form", "heading": self.t("menu.match_settings"),
                       "items": self.settings_fields(&self.defaults, "def", true)}),
                json!({"kind": "row", "items": [self.button("go.main_menu", "menu.back"), self.button("def.reset", "menu.reset")]}),
            ],
            Screen::PlayVsAi => {
                let mut seats = Vec::new();
                for i in 0..self.setup.format.seats() {
                    let team = format!(
                        "{} {}",
                        self.t("seat.team"),
                        self.setup.format.team_of(i) + 1
                    );
                    let color = self.t(&format!("color.{i}"));
                    if i == 0 {
                        seats.push(json!({"type": "list", "rows": [[color, format!("{} · {}", self.t("seat.you"), team)]]}));
                    } else {
                        let d = self.ai_seats[i - 1];
                        let mut sel = self.select(
                            &format!("ai.{i}"),
                            &format!("color.{i}"),
                            ["larva", "pupa", "imago"]
                                .iter()
                                .map(|v| {
                                    (
                                        v.to_string(),
                                        format!(
                                            "{} {} · {}",
                                            self.t("seat.ai"),
                                            self.t(&format!("difficulty.{v}")),
                                            team
                                        ),
                                    )
                                })
                                .collect(),
                            difficulty_key(d),
                        );
                        sel["term"] = json!(self.t(&format!("difficulty.{}", difficulty_key(d))));
                        seats.push(sel);
                    }
                }
                seats.push(json!({"type": "input", "id": "pva.seed", "label": self.t("setting.seed"), "value": self.seed_text, "placeholder": self.t("random")}));
                vec![
                    json!({"kind": "form", "heading": self.t("menu.play_vs_ai"), "items": seats}),
                    json!({"kind": "form", "heading": self.t("menu.match_settings"), "items": self.settings_fields(&self.setup, "pva", true)}),
                    json!({"kind": "row", "items": [self.button("go.main_menu", "menu.back"), self.button("pva.start", "menu.start")]}),
                ]
            }
            Screen::PlayerSettings => {
                let mut rows = Vec::new();
                for slot in crate::keys::all_slots() {
                    let label = if let Some(n) = slot.strip_prefix("grid.") {
                        format!(
                            "{} {}",
                            self.t("keys.grid"),
                            n.parse::<i32>().unwrap_or(0) + 1
                        )
                    } else if let Some(n) = slot.strip_prefix("tab.") {
                        format!(
                            "{} {}",
                            self.t("keys.tab"),
                            n.parse::<i32>().unwrap_or(0) + 1
                        )
                    } else {
                        self.t(&format!("keys.{slot}"))
                    };
                    let key = if self.rebinding.as_deref() == Some(slot) {
                        self.t("keys.press")
                    } else {
                        key_label(&self.player.key_of(slot))
                    };
                    rows.push(json!({"type": "keybind", "id": format!("bind.{slot}"), "label": label, "key": key}));
                }
                rows.push(self.select(
                    "player.edge_pan",
                    "keys.edge_pan",
                    vec![("on".into(), self.t("on")), ("off".into(), self.t("off"))],
                    if self.player.edge_pan { "on" } else { "off" },
                ));
                vec![
                    json!({"kind": "form", "heading": self.t("keys.title"), "items": rows}),
                    json!({"kind": "row", "items": [
                        self.button("go.main_menu", "menu.back"),
                        self.button("keys.reset", "menu.reset"),
                        self.button("go.benchmark", "menu.benchmark"),
                    ]}),
                ]
            }
            Screen::Online => vec![
                json!({"kind": "form", "heading": self.t("menu.online"), "items": [
                    {"type": "input", "id": "online.room", "label": self.t("online.room_code"), "value": self.online.room},
                    {"type": "list", "rows": [[self.t("online.status"), self.online.status]]},
                ]}),
                json!({"kind": "row", "items": [
                    self.button("online.host", "online.host"),
                    self.button("online.join", "online.join"),
                ]}),
                json!({"kind": "form", "items": [
                    {"type": "code", "id": "online.invite", "label": self.t("online.invite_code"), "value": self.online.invite},
                    {"type": "code", "id": "online.reply", "label": self.t("online.reply_code"), "value": self.online.reply},
                ]}),
                json!({"kind": "row", "items": [
                    self.button("online.host_code", "online.host_by_code"),
                    self.button("online.join_code", "online.join_by_code"),
                    self.button("online.connect", "online.connect"),
                    self.button("go.main_menu", "menu.back"),
                ]}),
            ],
            Screen::Lobby => self.lobby_panels(),
            Screen::Match => Vec::new(),
        }
    }

    fn lobby_panels(&self) -> Vec<Value> {
        let Some(l) = &self.online.lobby else {
            return Vec::new();
        };
        let host = l.is_host();
        let settings = MatchSettings::parse(l.settings()).unwrap_or_default();
        let mut seats = Vec::new();
        for (i, seat) in l.seats().iter().enumerate() {
            let color = self.t(&format!("color.{i}"));
            let team = format!("{} {}", self.t("seat.team"), settings.format.team_of(i) + 1);
            match &seat.kind {
                NetSeat::Human { name, .. } => seats
                    .push(json!({"type": "list", "rows": [[color, format!("{name} · {team}")]]})),
                kind if host => {
                    let value = match kind {
                        NetSeat::Ai { difficulty } => format!("ai.{difficulty}"),
                        _ => "open".into(),
                    };
                    let mut opts = vec![("open".to_string(), self.t("seat.open"))];
                    for (k, d) in ["larva", "pupa", "imago"].iter().enumerate() {
                        opts.push((
                            format!("ai.{k}"),
                            format!(
                                "{} {}",
                                self.t("seat.ai"),
                                self.t(&format!("difficulty.{d}"))
                            ),
                        ));
                    }
                    let _ = team;
                    seats.push(self.select(
                        &format!("seat.{i}"),
                        &format!("color.{i}"),
                        opts,
                        &value,
                    ));
                }
                NetSeat::Ai { difficulty } => {
                    let d = ["larva", "pupa", "imago"]
                        .get(*difficulty as usize)
                        .copied()
                        .unwrap_or("pupa");
                    seats.push(json!({"type": "list", "rows": [[color, format!("{} {} · {team}", self.t("seat.ai"), self.t(&format!("difficulty.{d}")))]]}));
                }
                _ => seats.push(json!({"type": "list", "rows": [[color, self.t("seat.open")]]})),
            }
        }
        let status = match l.status() {
            LobbyStatus::Joining => self.t("online.connecting"),
            LobbyStatus::Seated(_) => self.t("online.waiting"),
            LobbyStatus::Refused(r) => format!("{}: {:?}", self.t("online.refused"), r),
            LobbyStatus::Dissolved => self.t("online.dissolved"),
            LobbyStatus::Left => self.t("online.dissolved"),
            LobbyStatus::Started => self.t("online.connecting"),
        };
        seats.push(json!({"type": "list", "rows": [[self.t("online.room_code"), self.online.room], [self.t("online.status"), status]]}));
        let mut row = vec![self.button("lobby.leave", "online.leave")];
        if host {
            row.push(self.button("lobby.start", "menu.start"));
        }
        let mut panels =
            vec![json!({"kind": "form", "heading": self.t("online.lobby"), "items": seats})];
        if host {
            // Pasted codes: one Invite Code per joiner, answered by a Reply Code.
            panels.push(json!({"kind": "form", "items": [
                {"type": "code", "id": "online.invite", "label": self.t("online.invite_code"), "value": self.online.invite},
                {"type": "code", "id": "online.reply", "label": self.t("online.reply_code"), "value": self.online.reply},
            ]}));
            panels.push(json!({"kind": "row", "items": [
                self.button("online.host_code", "online.invite_code"),
                self.button("online.connect", "online.connect"),
            ]}));
        }
        panels.push(json!({"kind": "form", "heading": self.t("menu.match_settings"), "items": self.settings_fields(&settings, "lobby", host)}));
        panels.push(json!({"kind": "row", "items": row}));
        panels
    }

    // ---- the match view ----

    fn match_view(&self) -> Value {
        let Some(g) = &self.game else {
            return Value::Null;
        };
        let w = &g.world;
        let me = g.me;
        let c = &w.colonies[me as usize];
        let income = w.income_tenths(me);
        let resources: Vec<Value> = Resource::ALL
            .iter()
            .map(|r| {
                json!({
                    "key": r.key(),
                    "label": self.t(&format!("resource.{}", r.key())),
                    "value": c.stock[r.index()].to_string(),
                    "income": format!("{}{}", tenths(income[r.index()]), self.t("unit.per_second")),
                    "term": self.t("stat.income"),
                })
            })
            .collect();
        let pop = w.population(me);
        let housing = w.housing(me);
        let queued_train = w
            .structures
            .iter()
            .filter(|s| s.colony == me && s.complete)
            .any(|s| {
                s.lane
                    .first()
                    .map(|q| matches!(q.work, Work::Train(sp) if pop + sp.stats().pop > housing))
                    .unwrap_or(false)
            });
        let mut hud = json!({
            "resources": resources,
            "population": {"label": self.t("stat.population"), "value": format!("{pop} / {housing}"), "term": self.t("stat.population")},
            "tier": {"label": self.t("stat.tier"), "value": c.tier.to_string(), "term": self.t("stat.tier")},
            "clock": format!("{}:{:02}", w.tick / TICKS_PER_SECOND / 60, w.tick / TICKS_PER_SECOND % 60),
            "colour": self.t(&format!("color.{me}")),
        });
        let mut flags = Vec::new();
        if queued_train {
            flags.push(
                json!({"text": self.t("hud.housing_full"), "term": self.t("hud.housing_full")}),
            );
        }
        if c.retreat {
            flags.push(json!({"text": self.t("hud.retreat_on"), "term": self.t("action.retreat")}));
        }
        if let Some(m) = &g.lockstep {
            match m.status() {
                MatchStatus::WaitingOn(seats) => {
                    let who: Vec<String> = seats
                        .iter()
                        .map(|s| self.t(&format!("color.{s}")))
                        .collect();
                    flags.push(json!({"text": format!("{}: {}", self.t("hud.waiting_on"), who.join(", "))}));
                }
                MatchStatus::Resyncing => flags.push(json!({"text": self.t("hud.resyncing")})),
                _ => {}
            }
        }
        hud["flags"] = Value::Array(flags);

        // Queue bar: every training and research item, front first.
        let mut queue = Vec::new();
        for s in w.structures.iter().filter(|s| s.colony == me) {
            let all = s.lane.iter().chain(s.research.iter());
            for (slot, q) in all.enumerate() {
                let (label, icon) = match q.work {
                    Work::Train(sp) => (self.species_name(sp), format!("species.{}", sp.key())),
                    Work::Research(u, r) => (
                        format!("{} {}", self.upgrade_name(u), roman(r)),
                        format!("upgrade.{}", u.key()),
                    ),
                    Work::Evolve(t) => (
                        format!("{} {} {t}", self.t("action.evolve"), self.t("stat.tier")),
                        "action.evolve".into(),
                    ),
                };
                let pct = q.progress * 100 / q.total.max(1);
                queue.push(json!({
                    "id": format!("cancel.{}.{}", s.id, slot),
                    "label": label,
                    "icon": icon,
                    "progress": format!("{pct}%"),
                    "rows": [[self.t("stat.duration"), self.seconds(q.total - q.progress)], [self.t("stat.cost"), self.cost_text(&q.cost)]],
                }));
            }
        }

        // Action grid.
        let tabs: Vec<Value> = TABS
            .iter()
            .enumerate()
            .map(|(i, t)| {
                json!({
                    "id": format!("tab.{i}"),
                    "label": self.t(&format!("tab.{t}")),
                    "key": key_label(&self.player.key_of(&format!("tab.{i}"))),
                    "active": i == g.tab,
                })
            })
            .collect();
        let buttons: Vec<Value> = grid(g.tab)
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let key = key_label(&self.player.key_of(&format!("grid.{i}")));
                match b {
                    Some(b) => self.grid_button(g, *b, i, &key),
                    None => json!({"id": format!("grid.{i}"), "empty": true, "key": key}),
                }
            })
            .collect();

        let notices: Vec<Value> = w
            .announcements
            .iter()
            .rev()
            .filter(|a| a.colony.is_none() || a.colony == Some(me))
            .filter(|a| w.tick - a.tick < 6 * TICKS_PER_SECOND)
            .take(4)
            .map(|a| json!({"text": self.announcement_text(&a.key)}))
            .collect();

        let mut v = json!({
            "hud": hud,
            "queue": queue,
            "tabs": tabs,
            "grid": buttons,
            "notices": notices,
            "mode": match g.mode {
                Mode::Idle => "idle",
                Mode::Place(_) | Mode::Wall(_) => "place",
                Mode::Waypoint(_) => "waypoint",
            },
        });
        if let Some(team) = w.winner {
            let mine = w.colonies[me as usize].team;
            let t = &c.tally;
            v["end"] = json!({
                "title": self.t(if team == mine { "end.victory" } else { "end.defeat" }),
                "rows": [
                    [self.t("stat.match_length"), format!("{}:{:02}", g.ended_at.unwrap_or(w.tick) / TICKS_PER_SECOND / 60, g.ended_at.unwrap_or(w.tick) / TICKS_PER_SECOND % 60)],
                    [self.t("stat.trained"), t.trained.to_string()],
                    [self.t("stat.killed"), t.killed.to_string()],
                    [self.t("stat.lost"), t.lost.to_string()],
                    [self.t("stat.gathered"), self.cost_text(&t.gathered)],
                ],
                "button": self.button("end.main_menu", "menu.main_menu"),
            });
        } else if g.menu_open {
            v["menu"] = if g.confirm_concede {
                json!({"heading": self.t("menu.concede_confirm"), "items": [self.button("menu.concede_yes", "menu.concede"), self.button("menu.resume", "menu.resume")]})
            } else {
                json!({"items": [self.button("menu.resume", "menu.resume"), self.button("menu.concede", "menu.concede")]})
            };
        }
        v
    }

    fn announcement_text(&self, key: &str) -> String {
        if let Some(r) = key.strip_prefix("notice.needs_hatchery.") {
            let name = Species::from_key(r)
                .map(|s| self.structure_name(StructureKind::Hatchery(s)))
                .unwrap_or_default();
            return format!("{}: {name}", self.t("notice.needs_hatchery"));
        }
        if let Some(u) = key.strip_prefix("notice.upgraded.") {
            let name = Upgrade::from_key(u)
                .map(|u| self.upgrade_name(u))
                .unwrap_or_default();
            return format!("{}: {name}", self.t("notice.upgraded"));
        }
        if let Some(r) = key.strip_prefix("announce.windfall.") {
            return format!(
                "{}: {}",
                self.t("announce.windfall"),
                self.t(&format!("resource.{r}"))
            );
        }
        if let Some(t) = key.strip_prefix("announce.hill_won.") {
            return format!(
                "{}: {} {}",
                self.t("announce.hill_won"),
                self.t("seat.team"),
                t.parse::<i32>().unwrap_or(0) + 1
            );
        }
        if let Some(c) = key.strip_prefix("announce.defeated.") {
            return format!(
                "{}: {}",
                self.t("announce.defeated"),
                self.t(&format!("color.{c}"))
            );
        }
        self.t(key)
    }

    /// One grid button: its label, hotkey, state and label-value tooltip.
    fn grid_button(&self, g: &Game, b: Button, i: usize, key: &str) -> Value {
        let w = &g.world;
        let me = g.me;
        let c = &w.colonies[me as usize];
        let mut rows: Vec<Value> = Vec::new();
        let row = |l: &str, v: String| json!([self.t(l), v]);
        let (label, icon, state, active) = match b {
            Button::Train(s) => {
                let st = c.ranks.apply(s);
                rows.push(row("stat.role", self.t(&format!("role.{}", st.role.key()))));
                rows.push(row("stat.tier", st.tier.to_string()));
                rows.push(row("stat.cost", self.cost_text(&st.cost)));
                rows.push(row("stat.pop_cost", st.pop.to_string()));
                rows.push(row("stat.duration", self.seconds(st.train_ticks)));
                rows.push(row("stat.health", st.health.to_string()));
                if st.armor > 0 {
                    rows.push(row("stat.armor", st.armor.to_string()));
                }
                if st.damage > 0 {
                    rows.push(row("stat.damage", st.damage.to_string()));
                    rows.push(row(
                        "stat.attack_speed",
                        format!("{}{}", tenths(st.attack_ticks), self.t("unit.seconds")),
                    ));
                    rows.push(row(
                        "stat.range",
                        if st.range > CELL {
                            format!("{} {}", st.range / CELL, self.t("unit.cells"))
                        } else {
                            self.t("stat.melee")
                        },
                    ));
                }
                if st.splash > 0 {
                    rows.push(row(
                        "stat.splash",
                        format!("{} {}", tenths(st.splash * 10 / CELL), self.t("unit.cells")),
                    ));
                }
                if st.heal > 0 {
                    rows.push(row("stat.heal_rate", st.heal.to_string()));
                }
                if st.carry > 0 {
                    rows.push(row("stat.carry_capacity", st.carry.to_string()));
                }
                if st.gather_ticks > 0 && st.role == Role::Gatherer {
                    // Resources per second, in tenths: 100 / tenths-of-ticks.
                    rows.push(row(
                        "stat.gather_rate",
                        format!(
                            "{}{}",
                            tenths(1000 / st.gather_ticks.max(1)),
                            self.t("unit.per_second")
                        ),
                    ));
                }
                rows.push(row("stat.move_speed", self.speed_word(st.speed)));
                rows.push(row(
                    "stat.vision",
                    format!("{} {}", st.vision, self.t("unit.cells")),
                ));
                let requires = if s.from_queen() {
                    self.structure_name(StructureKind::Queen)
                } else {
                    self.structure_name(StructureKind::Hatchery(s))
                };
                rows.push(row("stat.requires", requires));
                rows.push(row(
                    "stat.hotkey",
                    format!(
                        "{key} · Ctrl/Alt {key}: {} · Shift {key}: {}",
                        self.t("action.train_10"),
                        self.t("action.train_max")
                    ),
                ));
                let state = self.state_of(w.can_train(me, s).map(|_| ()), &st.cost, w, me);
                (
                    self.species_name(s),
                    format!("species.{}", s.key()),
                    state,
                    false,
                )
            }
            Button::Place(k) => {
                let st = k.stats();
                rows.push(row("stat.cost", self.cost_text(&st.cost)));
                rows.push(row("stat.health", st.health.to_string()));
                if st.housing > 0 {
                    rows.push(row("stat.housing", st.housing.to_string()));
                }
                rows.push(row("stat.tier", st.tier.to_string()));
                if let StructureKind::Hatchery(s) = k {
                    rows.push(row(
                        "stat.effect",
                        format!("{} {}", self.t("action.train"), self.species_name(s)),
                    ));
                }
                rows.push(row("stat.hotkey", key.to_string()));
                let gate = if c.tier < st.tier {
                    Err(Refusal::NeedsTier(st.tier))
                } else if let StructureKind::Hatchery(s) = k {
                    if w.settings.is_disabled(s) {
                        Err(Refusal::Disabled)
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                };
                let state = self.state_of(gate, &st.cost, w, me);
                let active = g.mode == Mode::Place(k)
                    || (k == StructureKind::Wall && matches!(g.mode, Mode::Wall(_)));
                (
                    self.structure_name(k),
                    format!("structure.{}", k.key()),
                    state,
                    active,
                )
            }
            Button::Waypoint(sp) => {
                let (label, icon, count) = match sp {
                    Some(s) => (
                        self.species_name(s),
                        format!("species.{}", s.key()),
                        c.waypoints[s.index()].len(),
                    ),
                    None => (self.t("action.all_ants"), "action.all_ants".into(), 0),
                };
                rows.push(row("action.set_waypoint", label.clone()));
                if sp.is_some() {
                    rows.push(row(
                        "stat.status",
                        format!("{count} / {}", crate::rules::WAYPOINTS_PER_SPECIES),
                    ));
                }
                rows.push(row("stat.hotkey", key.to_string()));
                let state = match sp {
                    Some(s) if w.settings.is_disabled(s) => "disabled",
                    _ => "ready",
                };
                (label, icon, state.to_string(), g.mode == Mode::Waypoint(sp))
            }
            Button::Research(u) => {
                let rank = w.next_rank(me, u);
                let name = self.upgrade_name(u);
                let max = rank > u.max_rank();
                rows.push(row(
                    "stat.rank",
                    if max {
                        self.t("state.max_rank")
                    } else {
                        roman(rank).to_string()
                    },
                ));
                if !max {
                    rows.push(row("stat.cost", self.cost_text(&u.cost(rank))));
                    rows.push(row("stat.duration", self.seconds(u.research_ticks(rank))));
                    rows.push(row("stat.tier", u.tier_for(rank).to_string()));
                }
                rows.push(row("stat.effect", self.upgrade_effect(u)));
                if let Some(s) = u.at_hatchery() {
                    rows.push(row(
                        "stat.requires",
                        self.structure_name(StructureKind::Hatchery(s)),
                    ));
                }
                rows.push(row("stat.hotkey", key.to_string()));
                let state = if max {
                    "max_rank".to_string()
                } else {
                    self.state_of(w.can_research(me, u).map(|_| ()), &u.cost(rank), w, me)
                };
                let label = if max {
                    name
                } else {
                    format!("{name} {}", roman(rank))
                };
                (label, format!("upgrade.{}", u.key()), state, false)
            }
            Button::Evolve => {
                let to = c.tier + 1;
                if to <= 3 {
                    let (cost, ticks) = evolve_cost(to);
                    rows.push(row("stat.next", format!("{} {to}", self.t("stat.tier"))));
                    rows.push(row("stat.cost", self.cost_text(&cost)));
                    rows.push(row("stat.duration", self.seconds(ticks)));
                    rows.push(row("stat.hotkey", key.to_string()));
                    let state = self.state_of(w.can_evolve(me).map(|_| ()), &cost, w, me);
                    (
                        format!("{} {} {to}", self.t("action.evolve"), self.t("stat.tier")),
                        "action.evolve".into(),
                        state,
                        false,
                    )
                } else {
                    (
                        self.t("action.evolve"),
                        "action.evolve".into(),
                        "max_rank".into(),
                        false,
                    )
                }
            }
            Button::Retreat => {
                rows.push(row(
                    "stat.status",
                    self.t(if c.retreat { "on" } else { "off" }),
                ));
                rows.push(row("stat.hotkey", key.to_string()));
                (
                    self.t("action.retreat"),
                    "action.retreat".into(),
                    "ready".into(),
                    c.retreat,
                )
            }
            Button::Focus(f) => {
                let weights = f.weights();
                let split: Vec<String> = Resource::ALL
                    .iter()
                    .map(|r| {
                        format!(
                            "{} {}",
                            weights[r.index()],
                            self.t(&format!("resource.{}", r.key()))
                        )
                    })
                    .collect();
                rows.push(row("action.gather_focus", split.join(" : ")));
                rows.push(row("stat.hotkey", key.to_string()));
                (
                    self.t(&format!("focus.{}", f.key())),
                    format!("focus.{}", f.key()),
                    "ready".into(),
                    c.focus == f,
                )
            }
        };
        let term = match b {
            Button::Waypoint(None) => self.t("action.all_ants"),
            Button::Evolve => self.t("action.evolve"),
            Button::Retreat => self.t("action.retreat"),
            Button::Focus(_) => self.t("action.gather_focus"),
            _ => String::new(),
        };
        json!({
            "id": format!("grid.{i}"),
            "label": label,
            "icon": icon,
            "key": key,
            "state": state,
            "state_text": self.t(&format!("state.{state}")),
            "active": active,
            "rows": rows,
            "term": term,
        })
    }

    fn speed_word(&self, speed: i32) -> String {
        let k = match speed {
            0..=55 => "very_slow",
            56..=74 => "slow",
            75..=90 => "normal",
            91..=115 => "fast",
            _ => "very_fast",
        };
        self.t(&format!("speed.word.{k}"))
    }

    fn upgrade_effect(&self, u: Upgrade) -> String {
        let (stat, by) = match u {
            Upgrade::Damage => ("stat.damage", "+1"),
            Upgrade::Armor => ("stat.armor", "+1"),
            Upgrade::Health => ("stat.health", "+10%"),
            Upgrade::GatherRate => ("stat.gather_rate", "+20%"),
            Upgrade::CarryCapacity => ("stat.carry_capacity", "+3"),
            Upgrade::BuildRate => ("stat.build_rate", "+50%"),
            Upgrade::Species(s) => match s {
                Species::Soldier => ("stat.health", "+15"),
                Species::Fire => ("stat.move_speed", "+15%"),
                Species::Turtle => ("stat.armor", "+2"),
                Species::Acid => ("stat.range", "+1"),
                Species::Nurse => ("stat.heal_rate", "+1"),
                Species::Flying => ("stat.damage", "+2"),
                Species::Exploding => ("stat.splash", "+0.5"),
                Species::TrapJaw => ("stat.damage", "+5"),
                _ => ("stat.damage", "+4"),
            },
        };
        format!("{} {by}", self.t(stat))
    }

    /// "ready", "locked" (tier, hatchery), "disabled", or the resource short.
    fn state_of(&self, gate: Result<(), Refusal>, cost: &[i32; 3], w: &World, me: u8) -> String {
        match gate {
            Err(Refusal::Disabled) => "disabled".into(),
            Err(Refusal::AlreadyQueued) => "queued".into(),
            Err(Refusal::MaxRank) => "max_rank".into(),
            Err(_) => "locked".into(),
            Ok(()) if !w.can_pay(me, cost) => "unaffordable".into(),
            Ok(()) => "ready".into(),
        }
    }

    // ---- actions from the page ----

    /// A click or change on a menu element, a grid button, a tab, or a
    /// queue item. `value` carries the new value of a field.
    pub fn act(&mut self, id: &str, value: &str, ctrl: bool, shift: bool) {
        match id {
            "go.main_menu" | "end.main_menu" => {
                self.leave_online();
                self.game = None;
                self.screen = Screen::MainMenu;
            }
            "go.play_vs_ai" => {
                self.setup = self.defaults.clone();
                self.screen = Screen::PlayVsAi;
            }
            "go.online" => self.screen = Screen::Online,
            "go.match_settings" => self.screen = Screen::MatchSettings,
            "go.player_settings" => self.screen = Screen::PlayerSettings,
            "go.benchmark" => self.navigate = Some("bench.html".into()),
            "def.reset" => {
                self.defaults = MatchSettings::default();
                self.save_settings();
            }
            "keys.reset" => {
                self.player.reset();
                self.save_settings();
            }
            "pva.start" => self.start_local(),
            "pva.seed" => {
                self.seed_text = value
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .take(18)
                    .collect()
            }
            "online.room" => {
                self.online.room = value
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .take(32)
                    .collect()
            }
            "online.invite" => self.online.invite = value.trim().to_string(),
            "online.reply" => self.online.reply = value.trim().to_string(),
            "online.host" => {
                let room = format!("{BUILD_ID}-{}", self.room_name());
                self.host_online(json!({"op": "host_room", "room": room}))
            }
            "online.join" => {
                let room = format!("{BUILD_ID}-{}", self.room_name());
                self.join_online(json!({"op": "join_room", "room": room}))
            }
            "online.host_code" => {
                // Hosting already: a fresh Invite Code for the next joiner.
                let hosting = self
                    .online
                    .lobby
                    .as_ref()
                    .map(|l| l.is_host())
                    .unwrap_or(false);
                if hosting {
                    self.online.requests.push(json!({"op": "invite_code"}));
                } else {
                    self.host_online(json!({"op": "host_code"}));
                }
            }
            "online.join_code" => {
                self.join_online(json!({"op": "join_code", "invite": self.online.invite}))
            }
            "online.connect" => self
                .online
                .requests
                .push(json!({"op": "accept_reply", "reply": self.online.reply})),
            "lobby.leave" => {
                self.leave_online();
                self.screen = Screen::Online;
            }
            "lobby.start" => self.start_online(),
            "menu.resume" => {
                if let Some(g) = &mut self.game {
                    g.menu_open = false;
                    g.confirm_concede = false;
                }
            }
            "menu.concede" => {
                if let Some(g) = &mut self.game {
                    g.confirm_concede = true;
                }
            }
            "menu.concede_yes" => {
                if let Some(g) = &mut self.game {
                    g.issue(Command::Concede);
                    g.menu_open = false;
                    g.confirm_concede = false;
                }
            }
            "player.edge_pan" => {
                self.player.edge_pan = value == "on";
                self.save_settings();
            }
            _ => self.act_prefixed(id, value, ctrl, shift),
        }
    }

    fn act_prefixed(&mut self, id: &str, value: &str, ctrl: bool, shift: bool) {
        if let Some(slot) = id.strip_prefix("bind.") {
            self.rebinding = Some(slot.to_string());
        } else if let Some(n) = id.strip_prefix("grid.") {
            if let Ok(i) = n.parse::<usize>() {
                self.press_grid(i, ctrl, shift);
            }
        } else if let Some(n) = id.strip_prefix("tab.") {
            if let (Ok(i), Some(g)) = (n.parse::<usize>(), &mut self.game) {
                g.tab = i.min(TABS.len() - 1);
            }
        } else if let Some(rest) = id.strip_prefix("cancel.") {
            let mut it = rest.split('.');
            let sid = it.next().and_then(|v| v.parse::<u32>().ok());
            let slot = it.next().and_then(|v| v.parse::<u32>().ok());
            if let (Some(structure), Some(slot), Some(g)) = (sid, slot, &mut self.game) {
                g.issue(Command::Cancel { structure, slot });
            }
        } else if let Some(rest) = id.strip_prefix("ai.") {
            if let (Ok(i), Some(d)) = (rest.parse::<usize>(), parse_difficulty(value)) {
                if (1..=3).contains(&i) {
                    self.ai_seats[i - 1] = d;
                }
            }
        } else if let Some(rest) = id.strip_prefix("seat.") {
            self.lobby_seat(rest, value);
        } else if let Some(rest) = id.strip_prefix("def.") {
            let mut m = self.defaults.clone();
            if apply_setting(&mut m, rest, value) {
                self.defaults = m;
                self.save_settings();
            }
        } else if let Some(rest) = id.strip_prefix("pva.") {
            let mut m = self.setup.clone();
            if apply_setting(&mut m, rest, value) {
                self.setup = m;
            }
        } else if let Some(rest) = id.strip_prefix("lobby.") {
            self.lobby_setting(rest, value);
        }
    }

    fn save_settings(&mut self) {
        self.save = Some(json!({
            "defaults": serde_json::to_string(&self.defaults).expect("serialises"),
            "player": serde_json::to_string(&self.player).expect("serialises"),
        }));
    }

    fn start_local(&mut self) {
        let seed = self.seed_text.parse::<u64>().unwrap_or(self.entropy);
        let mut settings = self.setup.clone();
        if settings.biome == Biome::Random {
            settings.biome = if seed.is_multiple_of(2) {
                Biome::Picnic
            } else {
                Biome::FlowerField
            };
        }
        let mut seats = vec![SeatKind::Human];
        for i in 1..settings.format.seats() {
            seats.push(SeatKind::Ai(self.ai_seats[i - 1]));
        }
        let world = World::new(seed, settings, &seats);
        self.game = Some(Game::new(world, 0, None));
        self.screen = Screen::Match;
    }

    fn press_grid(&mut self, i: usize, ctrl: bool, shift: bool) {
        let Some(g) = &mut self.game else {
            return;
        };
        let Some(b) = grid(g.tab).get(i).copied().flatten() else {
            return;
        };
        match b {
            Button::Train(species) => {
                let amount = if shift {
                    Amount::Max
                } else if ctrl {
                    Amount::Ten
                } else {
                    Amount::One
                };
                g.issue(Command::Train { species, amount });
            }
            Button::Place(StructureKind::Wall) => g.mode = Mode::Wall(None),
            Button::Place(k) => g.mode = Mode::Place(k),
            Button::Waypoint(s) => g.mode = Mode::Waypoint(s),
            Button::Research(upgrade) => g.issue(Command::Research { upgrade }),
            Button::Evolve => g.issue(Command::Evolve),
            Button::Retreat => {
                let on = !g.world.colonies[g.me as usize].retreat;
                g.issue(Command::Retreat { on });
            }
            Button::Focus(focus) => g.issue(Command::Focus { focus }),
        }
    }

    // ---- keyboard and mouse ----

    /// A key went down or up. `code` is `KeyboardEvent.code`.
    pub fn key(&mut self, code: &str, down: bool, ctrl: bool, shift: bool) {
        if let Some(slot) = self.rebinding.take() {
            if down {
                self.player.bind(&slot, code);
                self.save_settings();
            } else {
                self.rebinding = Some(slot);
            }
            return;
        }
        let Some(slot) = self.player.slot_of(code) else {
            return;
        };
        if let Some(d) = slot.strip_prefix("pan.") {
            if let Some(g) = &mut self.game {
                let i = ["left", "right", "up", "down"]
                    .iter()
                    .position(|x| *x == d)
                    .unwrap_or(0);
                g.pan_keys[i] = down;
            }
            return;
        }
        if !down || self.screen != Screen::Match {
            return;
        }
        if let Some(n) = slot.strip_prefix("grid.") {
            if let Ok(i) = n.parse::<usize>() {
                self.press_grid(i, ctrl, shift);
            }
        } else if let Some(n) = slot.strip_prefix("tab.") {
            if let (Ok(i), Some(g)) = (n.parse::<usize>(), &mut self.game) {
                g.tab = i;
            }
        } else if slot == "menu" {
            if let Some(g) = &mut self.game {
                if g.mode != Mode::Idle {
                    g.mode = Mode::Idle;
                } else {
                    g.menu_open = !g.menu_open;
                    g.confirm_concede = false;
                }
            }
        } else if slot == "home" {
            if let Some(g) = &mut self.game {
                if let Some(q) = g.world.nearest_queen(g.me, Pos::new(g.view.cx, g.view.cy)) {
                    g.view.cx = q.pos.x;
                    g.view.cy = q.pos.y;
                }
            }
        }
    }

    /// Mouse on the map canvas. `kind`: "mousedown", "mouseup" or
    /// "mousemove". `button`: 0 left, 1 middle, 2 right.
    pub fn mouse(&mut self, kind: &str, button: i32, x: i32, y: i32, shift: bool) {
        let kind = match kind {
            "mousedown" => 0,
            "mouseup" => 1,
            "mousemove" => 2,
            _ => return,
        };
        let Some(g) = &mut self.game else {
            return;
        };
        g.mouse = (x, y);
        if g.menu_open || g.world.winner.is_some() {
            return;
        }
        let (cx, cy) = g.mouse_cell();
        match (kind, button) {
            (0, 0) => match g.mode {
                Mode::Place(kind) => {
                    g.issue(Command::Place {
                        kind,
                        x: cx,
                        y: cy,
                        x2: cx,
                        y2: cy,
                    });
                    if !shift {
                        g.mode = Mode::Idle;
                    }
                }
                Mode::Wall(None) => g.mode = Mode::Wall(Some((cx, cy))),
                Mode::Waypoint(species) => {
                    g.issue(Command::SetWaypoint {
                        species,
                        x: cx,
                        y: cy,
                    });
                    if !shift {
                        g.mode = Mode::Idle;
                    }
                }
                _ => g.drag = Some((x, y, g.view.cx, g.view.cy)),
            },
            (0, 1) => g.drag = Some((x, y, g.view.cx, g.view.cy)),
            (1, 0) => {
                if let Mode::Wall(Some((sx, sy))) = g.mode {
                    g.issue(Command::Place {
                        kind: StructureKind::Wall,
                        x: sx,
                        y: sy,
                        x2: cx,
                        y2: cy,
                    });
                    g.mode = if shift { Mode::Wall(None) } else { Mode::Idle };
                }
                g.drag = None;
            }
            (1, _) => g.drag = None,
            (0, 2) => {
                if g.mode != Mode::Idle {
                    g.mode = Mode::Idle;
                    return;
                }
                // Right-click clears a waypoint, or cancels a blueprint.
                let me = g.me;
                let p = Pos::of_cell(cx, cy);
                let near_wp = g.world.colonies[me as usize]
                    .waypoints
                    .iter()
                    .flatten()
                    .any(|w| w.dist(p) <= 3 * CELL);
                if near_wp {
                    g.issue(Command::ClearWaypoint { x: cx, y: cy });
                } else if let Some(bp) = g.world.structures.iter().find(|s| {
                    s.colony == me && !s.complete && s.pos.dist(p) <= s.radius() + CELL / 2
                }) {
                    let id = bp.id;
                    g.issue(Command::CancelBlueprint { id });
                }
            }
            (2, _) => {
                if let Some((sx, sy, vx, vy)) = g.drag {
                    g.view.cx = vx - (x - sx) * CELL / g.view.px_per_cell;
                    g.view.cy = vy - (y - sy) * CELL / g.view.px_per_cell;
                    g.view.clamp(g.world.map.w, g.world.map.h);
                }
            }
            _ => {}
        }
    }

    /// Mouse wheel: negative zooms in, about the pointer.
    pub fn wheel(&mut self, dy: i32) {
        if dy == 0 {
            return;
        }
        if let Some(g) = &mut self.game {
            let before = g.view.to_world(g.mouse.0, g.mouse.1);
            let step = if dy < 0 { 1 } else { -1 };
            g.view.px_per_cell += step * (1 + g.view.px_per_cell / 8);
            g.view.clamp(g.world.map.w, g.world.map.h);
            let after = g.view.to_world(g.mouse.0, g.mouse.1);
            g.view.cx += before.x - after.x;
            g.view.cy += before.y - after.y;
            g.view.clamp(g.world.map.w, g.world.map.h);
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        if let Some(g) = &mut self.game {
            g.view.w = w.max(1);
            g.view.h = h.max(1);
        }
    }

    /// Jumps the camera to a point on the minimap, which is `w` x `h`
    /// pixels and shows the whole map.
    pub fn jump(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if let Some(g) = &mut self.game {
            g.view.cx = g.world.map.w * CELL / w.max(1) * x.clamp(0, w);
            g.view.cy = g.world.map.h * CELL / h.max(1) * y.clamp(0, h);
        }
    }

    // ---- running the match ----

    /// Advances time to `now_ms`: pans the camera, runs every tick that is
    /// due (locally, or as lockstep allows), and exchanges network messages.
    pub fn advance(&mut self, now_ms: i64) {
        let dt = (now_ms - self.now_ms).clamp(0, 250);
        self.now_ms = now_ms;
        if let Some(l) = &mut self.online.lobby {
            let out = l.update(now_ms);
            self.online.outbox.extend(out);
            if let Some(start) = l.take_start() {
                self.begin_online(start);
            }
        }
        let Some(g) = &mut self.game else {
            return;
        };
        // Camera: arrow keys and screen edges.
        let pan = CELL * 40 * dt as i32 / 1000 * 12 / g.view.px_per_cell.max(1);
        let edge = 4;
        let (mx, my) = g.mouse;
        let e = self.player.edge_pan && g.drag.is_none() && g.mouse != (0, 0);
        let left = g.pan_keys[0] || (e && mx <= edge);
        let right = g.pan_keys[1] || (e && mx >= g.view.w - edge);
        let up = g.pan_keys[2] || (e && my <= edge);
        let down = g.pan_keys[3] || (e && my >= g.view.h - edge);
        g.view.cx += pan * (right as i32 - left as i32);
        g.view.cy += pan * (down as i32 - up as i32);
        g.view.clamp(g.world.map.w, g.world.map.h);

        if g.world.winner.is_some() {
            g.ended_at.get_or_insert(g.world.tick);
            if g.lockstep.is_none() {
                return;
            }
        }
        let ms = g.ms_per_tick();
        g.acc_ms = (g.acc_ms + dt).min(ms * 8);
        match &mut g.lockstep {
            None => {
                let mut ran = 0;
                while g.acc_ms >= ms && ran < 5 && g.world.winner.is_none() {
                    let me = g.me;
                    let cmds: Vec<(u8, Command)> = g.pending.drain(..).map(|c| (me, c)).collect();
                    g.world.step(&cmds, &[]);
                    g.acc_ms -= ms;
                    ran += 1;
                }
            }
            Some(m) => {
                let out = m.update(now_ms);
                self.online.outbox.extend(out);
                if let Some(tick) = m.wants_snapshot() {
                    let snap = g.world.snapshot();
                    let out = m.provide_snapshot(tick, &snap);
                    self.online.outbox.extend(out);
                }
                if let Some((_, snap)) = m.take_snapshot() {
                    if let Ok(w) = World::restore(&snap) {
                        g.world = w;
                    }
                }
                let mut ran = 0;
                while g.acc_ms >= ms && ran < 8 {
                    let next = g.world.tick as u32;
                    let Some(input) = m.take_tick(next) else {
                        break;
                    };
                    let cmds: Vec<(u8, Command)> = input
                        .commands
                        .iter()
                        .filter_map(|(seat, text)| {
                            serde_json::from_str::<Command>(text)
                                .ok()
                                .map(|c| (*seat, c))
                        })
                        // A player may never issue the AI's own memory.
                        .filter(|(_, c)| !matches!(c, Command::AiState { .. }))
                        .collect();
                    g.world.step(&cmds, &input.dropped);
                    if g.world.tick % 20 == 0 {
                        m.record_checksum(g.world.tick as u32, g.world.checksum());
                    }
                    g.acc_ms -= ms;
                    ran += 1;
                }
                // Behind (a peer waited): never run ahead of real time later.
                g.acc_ms = g.acc_ms.min(ms * 2);
            }
        }
    }

    // ---- online ----

    fn room_name(&self) -> String {
        if self.online.room.is_empty() {
            format!("colony-{}", self.entropy % 1_000_000)
        } else {
            self.online.room.clone()
        }
    }

    fn version() -> Version {
        Version::current(BUILD_ID)
    }

    fn host_online(&mut self, request: Value) {
        self.leave_online();
        if self.online.room.is_empty() {
            self.online.room = self.room_name();
        }
        let settings = serde_json::to_string(&self.defaults).expect("serialises");
        let lobby = Lobby::host(
            App::version(),
            &self.client_id,
            &self.t("seat.player"),
            self.defaults.format.seats(),
            &settings,
            self.now_ms,
        );
        self.online.lobby = Some(lobby);
        self.online.requests.push(request);
        self.screen = Screen::Lobby;
    }

    fn join_online(&mut self, request: Value) {
        self.leave_online();
        self.online.requests.push(request);
        self.online.status = self.t("online.connecting");
    }

    fn leave_online(&mut self) {
        if let Some(l) = &mut self.online.lobby {
            let out = l.leave();
            self.online.outbox.extend(out);
            self.online.requests.push(json!({"op": "leave"}));
        }
        self.online.lobby = None;
        self.online.early_peers.clear();
    }

    fn lobby_seat(&mut self, rest: &str, value: &str) {
        let now = self.now_ms;
        let (Ok(i), Some(l)) = (rest.parse::<usize>(), &mut self.online.lobby) else {
            return;
        };
        let kind = match value {
            "open" => NetSeat::Open,
            v => match v.strip_prefix("ai.").and_then(|d| d.parse::<u8>().ok()) {
                Some(d) => NetSeat::Ai {
                    difficulty: d.min(2),
                },
                None => return,
            },
        };
        if let Ok(out) = l.set_seat(i, kind, now) {
            self.online.outbox.extend(out);
        }
    }

    fn lobby_setting(&mut self, rest: &str, value: &str) {
        let now = self.now_ms;
        let Some(l) = &mut self.online.lobby else {
            return;
        };
        let mut m = MatchSettings::parse(l.settings()).unwrap_or_default();
        if !apply_setting(&mut m, rest, value) {
            return;
        }
        // The format sets the number of seats; seats beyond it close.
        let seats = m.format.seats();
        if let Ok(out) = l.set_settings(&serde_json::to_string(&m).expect("serialises"), now) {
            self.online.outbox.extend(out);
        }
        for i in 0..4 {
            let kind = if i < seats {
                NetSeat::Open
            } else {
                NetSeat::Closed
            };
            let is_open_or_closed = l
                .seats()
                .get(i)
                .map(|s| matches!(s.kind, NetSeat::Open | NetSeat::Closed))
                .unwrap_or(false);
            if is_open_or_closed {
                if let Ok(out) = l.set_seat(i, kind, now) {
                    self.online.outbox.extend(out);
                }
            }
        }
    }

    fn start_online(&mut self) {
        let now = self.now_ms;
        let seed = self.entropy;
        let Some(l) = &mut self.online.lobby else {
            return;
        };
        let m = MatchSettings::parse(l.settings()).unwrap_or_default();
        // Open seats are filled by Pupa when the match starts.
        for i in 0..l.seats().len() {
            if matches!(l.seats()[i].kind, NetSeat::Open) {
                if let Ok(out) = l.set_seat(i, NetSeat::Ai { difficulty: 1 }, now) {
                    self.online.outbox.extend(out);
                }
            }
        }
        if let Ok(out) = l.start(seed, m.game_speed.ms_per_tick() as u32, now) {
            self.online.outbox.extend(out);
        }
        if let Some(start) = l.take_start() {
            self.begin_online(start);
        }
    }

    fn begin_online(&mut self, start: crate::lockstep::MatchStart) {
        let settings = MatchSettings::parse(&start.settings).unwrap_or_default();
        let seats: Vec<SeatKind> = start
            .seats
            .iter()
            .map(|s: &Seat| match s.kind {
                NetSeat::Ai { difficulty } => SeatKind::Ai(match difficulty {
                    0 => Difficulty::Larva,
                    2 => Difficulty::Imago,
                    _ => Difficulty::Pupa,
                }),
                _ => SeatKind::Human,
            })
            .collect();
        let me = start.local_seat;
        let (m, out) = Match::new(start.clone(), self.now_ms);
        self.online.outbox.extend(out);
        let mut settings = settings;
        if settings.biome == Biome::Random {
            settings.biome = if start.seed.is_multiple_of(2) {
                Biome::Picnic
            } else {
                Biome::FlowerField
            };
        }
        let mut world = World::new(start.seed, settings, &seats);
        // Seats nobody sits in (Closed) play as AI too, so the map stays fair.
        for (i, s) in start.seats.iter().enumerate() {
            if matches!(s.kind, NetSeat::Closed) {
                if let Some(c) = world.colonies.get_mut(i) {
                    c.controller = Controller::Ai(Difficulty::Pupa);
                }
            }
        }
        self.game = Some(Game::new(world, me, Some(m)));
        self.online.lobby = None;
        self.screen = Screen::Match;
    }

    /// A transport event from the page: kind is "connected", "message",
    /// "disconnected" or "error"; `text` is the message or the error's key;
    /// `host` says the peer announced itself as the lobby's host.
    pub fn net_event(&mut self, kind: &str, peer: &str, text: &str, host: bool) {
        if kind == "error" {
            self.online.status = self.t(text);
            return;
        }
        let now = self.now_ms;
        let peer = PeerId(peer.to_string());
        if let Some(g) = &mut self.game {
            if let Some(m) = &mut g.lockstep {
                let out = match kind {
                    "message" => m.receive(&peer, text, now),
                    "disconnected" => {
                        m.peer_disconnected(&peer);
                        Vec::new()
                    }
                    _ => Vec::new(),
                };
                self.online.outbox.extend(out);
                return;
            }
        }
        match (&mut self.online.lobby, kind) {
            (None, "connected") if host => {
                // Joining: the peer that announced itself as host.
                let (mut l, out) = Lobby::join(
                    App::version(),
                    peer,
                    &self.client_id,
                    &self.t("seat.player"),
                    now,
                );
                self.online.outbox.extend(out);
                for p in std::mem::take(&mut self.online.early_peers) {
                    let out = l.peer_connected(p, now);
                    self.online.outbox.extend(out);
                }
                self.online.lobby = Some(l);
                self.screen = Screen::Lobby;
            }
            (None, "connected") => self.online.early_peers.push(peer),
            (Some(l), "connected") => {
                let out = l.peer_connected(peer, now);
                self.online.outbox.extend(out);
            }
            (Some(l), "message") => {
                let out = l.receive(&peer, text, now);
                self.online.outbox.extend(out);
                if let Some(start) = l.take_start() {
                    self.begin_online(start);
                }
            }
            (Some(l), "disconnected") => {
                let out = l.peer_disconnected(&peer, now);
                self.online.outbox.extend(out);
            }
            _ => {}
        }
    }

    /// A code for the pasted-code path, from the page's transport.
    pub fn net_code(&mut self, which: &str, code: &str) {
        match which {
            "invite" => self.online.invite = code.to_string(),
            "reply" => self.online.reply = code.to_string(),
            _ => self.online.status = code.to_string(),
        }
    }

    /// Messages for the page to send: [{"to": peer or null (everyone), "text"}].
    pub fn take_outbox(&mut self) -> Value {
        let v: Vec<Value> = self
            .online
            .outbox
            .drain(..)
            .map(|o| match o.to {
                Target::Peer(p) => json!({"to": p.0, "text": o.text}),
                Target::All => json!({"to": Value::Null, "text": o.text}),
            })
            .collect();
        Value::Array(v)
    }
}

pub fn format_key(f: Format) -> &'static str {
    match f {
        Format::OneVOne => "one_v_one",
        Format::TwoVTwo => "two_v_two",
        Format::FreeForAll => "free_for_all",
    }
}

fn speed_key(s: GameSpeed) -> &'static str {
    match s {
        GameSpeed::Slow => "slow",
        GameSpeed::Normal => "normal",
        GameSpeed::Fast => "fast",
    }
}

fn biome_key(b: Biome) -> &'static str {
    match b {
        Biome::Picnic => "picnic",
        Biome::FlowerField => "flower_field",
        Biome::Random => "random",
    }
}

fn difficulty_key(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Larva => "larva",
        Difficulty::Pupa => "pupa",
        Difficulty::Imago => "imago",
    }
}

fn parse_difficulty(v: &str) -> Option<Difficulty> {
    match v {
        "larva" => Some(Difficulty::Larva),
        "pupa" => Some(Difficulty::Pupa),
        "imago" => Some(Difficulty::Imago),
        _ => None,
    }
}

fn parse_level(v: &str) -> Option<Level> {
    match v {
        "low" => Some(Level::Low),
        "normal" => Some(Level::Normal),
        "high" => Some(Level::High),
        _ => None,
    }
}

/// Applies one settings field; false if the value is not allowed.
pub fn apply_setting(m: &mut MatchSettings, field: &str, value: &str) -> bool {
    match field {
        "format" => match value {
            "one_v_one" => m.format = Format::OneVOne,
            "two_v_two" => m.format = Format::TwoVTwo,
            "free_for_all" => m.format = Format::FreeForAll,
            _ => return false,
        },
        "map_size" => match parse_level(value) {
            Some(l) => m.map_size = l,
            None => return false,
        },
        "chamber_size" => match parse_level(value) {
            Some(l) => m.chamber_size = l,
            None => return false,
        },
        "resource_density" => match parse_level(value) {
            Some(l) => m.resource_density = l,
            None => return false,
        },
        "starting_resources" => match parse_level(value) {
            Some(l) => m.starting_resources = l,
            None => return false,
        },
        "tunnel_width" => match value.parse::<i32>() {
            Ok(w) if (TUNNEL_MIN..=TUNNEL_MAX).contains(&w) => m.tunnel_width = w,
            _ => return false,
        },
        "game_speed" => match value {
            "slow" => m.game_speed = GameSpeed::Slow,
            "normal" => m.game_speed = GameSpeed::Normal,
            "fast" => m.game_speed = GameSpeed::Fast,
            _ => return false,
        },
        "biome" => match value {
            "picnic" => m.biome = Biome::Picnic,
            "flower_field" => m.biome = Biome::FlowerField,
            "random" => m.biome = Biome::Random,
            _ => return false,
        },
        "events" => m.events = value == "on",
        other => {
            let Some(s) = other.strip_prefix("disabled.").and_then(Species::from_key) else {
                return false;
            };
            if !can_disable(s) {
                return false;
            }
            let on = value == "on" || value == "true";
            m.disabled.retain(|x| *x != s);
            if on {
                m.disabled.push(s);
                m.disabled.sort();
            }
        }
    }
    m.check().is_ok()
}
