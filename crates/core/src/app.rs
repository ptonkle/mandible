//! The application around a match: screens, menus, Match Settings and
//! Player Settings, the action grid, tooltips, the HUD, input, and running a
//! match locally (Play vs AI) or in lockstep (Online Multiplayer).
//!
//! The page draws what `ui()` returns and forwards input; it decides nothing.
//! Every on-screen word comes from `data/strings.json`, every hover hint from
//! `data/glossary.json`, and all of it follows GLOSSARY.md: labels and
//! label-value pairs, not prose.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::dice::Rng;
use crate::fixed::{Pos, CELL};
use crate::keys::{key_label, PlayerSettings};
use crate::lockstep::{
    clean_name, Lobby, LobbyStatus, Match, MatchStatus, Outgoing, PeerId, RefuseReason, Seat,
    SeatKind as NetSeat, Target, Version, MAX_NAME_CHARS,
};
use crate::render::{self, Camera, Overlay, Placing, SUB_PX};
use crate::rules::{
    evolve_cost, Resource, Role, Species, StructureKind, Upgrade, TICKS_PER_SECOND,
};
use crate::settings::{
    can_disable, Biome, Difficulty, Format, GameSpeed, Level, MapType, MatchSettings, SeatKind,
    TUNNEL_MAX, TUNNEL_MIN,
};
use crate::sound::Cue;
use crate::world::{
    Amount, Command, Focus, Refusal, Sample, Work, World, HILL_REWARD, SAMPLE_TICKS,
};

/// Peers with different builds refuse each other; raise on every release
/// that changes the simulation or the wire.
pub const BUILD_ID: &str = "mandible-1";

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
    AutoScout,
    Focus(Focus),
}

/// Time Scale steps for a Sandbox match or a spectated one, as
/// (numerator, denominator) of real time. Never online: lockstep peers
/// must all run at the agreed Game Speed.
pub const TIME_SCALES: [(i64, i64); 4] = [(1, 2), (1, 1), (2, 1), (4, 1)];

/// Room Code symbols: capital letters and digits without 1, 0, I and O.
pub const ROOM_CODE_SYMBOLS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const ROOM_CODE_LEN: usize = 6;

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
            g[9] = Some(Button::AutoScout);
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

/// Screen pixels of a world point under the camera.
pub fn to_pixels(cam: &Camera, p: Pos) -> (i32, i32) {
    let (x, y) = cam.screen(p);
    (x / SUB_PX, y / SUB_PX)
}

pub struct Game {
    pub world: World,
    /// The last tick whose announcements were turned into sounds.
    pub heard: i32,
    /// Victory or Defeat has played.
    pub sounded_end: bool,
    pub me: u8,
    pub tab: usize,
    pub mode: Mode,
    pub view: Camera,
    /// Pixels a pan key or screen edge moves the view per second.
    pub pan_speed: i32,
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
    /// No seat is You: the match is watched (Spectate), never played.
    pub spectate: bool,
    /// While spectating, whose Fog of War is drawn; `None` shows the whole map.
    pub vision: Option<u8>,
    /// Pause and Time Scale: local matches only (Sandbox or Spectate).
    pub paused: bool,
    pub time_scale: usize,
    /// The end screen's graph tab: an index into `END_GRAPHS`.
    pub end_graph: usize,
}

impl Game {
    pub fn new(world: World, me: u8, lockstep: Option<Match>) -> Game {
        let h = world.map.home(me as usize);
        let mut view = Camera::new(1280, 720);
        view.zoom = 12; // 12 pixels per cell: ants clearly visible
        view.centre_x = h.cx * CELL + CELL / 2;
        view.centre_y = h.cy * CELL + CELL / 2;
        Game {
            world,
            me,
            tab: 0,
            mode: Mode::Idle,
            view,
            pan_speed: 900,
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
            heard: 0,
            sounded_end: false,
            spectate: false,
            vision: None,
            paused: false,
            time_scale: 1,
            end_graph: 0,
        }
    }

    /// A watched match: no seat is You, and the whole map is visible.
    pub fn spectating(world: World) -> Game {
        let mut g = Game::new(world, 0, None);
        g.spectate = true;
        g
    }

    /// The colony the top bar, the queue bar and the drawing follow:
    /// yours, or while spectating the one whose Fog of War is shown.
    pub fn follow(&self) -> Option<u8> {
        if self.spectate {
            self.vision
        } else {
            Some(self.me)
        }
    }

    /// Pause and Time Scale are offered: a local Sandbox or spectated match.
    /// Time Scale: local Sandbox or spectated matches only.
    pub fn time_controls(&self) -> bool {
        self.lockstep.is_none() && (self.spectate || self.world.settings.sandbox)
    }

    /// Pause: any match that is not online (Play vs AI included).
    pub fn can_pause(&self) -> bool {
        self.lockstep.is_none()
    }

    /// Issues a command for the local colony: into the next tick when
    /// local, into the next turn when online.
    pub fn issue(&mut self, cmd: Command) {
        // A spectator owns no colony, so it never issues a command.
        if self.spectate {
            return;
        }
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
        self.view.world_at(self.mouse.0, self.mouse.1).cell()
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
    /// The pasted-code path (Invite Code, Reply Code) is shown: only with
    /// the page's `?codes` flag, for the automated online check.
    pub codes: bool,
    /// Peers met before the host: told to the lobby once it exists.
    pub early_peers: Vec<PeerId>,
    /// The page's transport has a session open (Host or Join pressed, not
    /// yet left): leaving must close it.
    pub session: bool,
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
    /// Seat 1 in Play vs AI: You (`None`) or an AI of this difficulty.
    pub first_seat: Option<Difficulty>,
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
    /// Sounds to play this frame, taken by the page.
    pub sounds: Vec<Cue>,
    /// When each sound last played, for its cooldown.
    pub last_cue: BTreeMap<Cue, i64>,
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
            first_seat: None,
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
                codes: false,
                early_peers: Vec::new(),
                session: false,
            },
            client_id: client_id.to_string(),
            now_ms: 0,
            save: None,
            entropy: 1,
            navigate: None,
            sounds: Vec::new(),
            last_cue: BTreeMap::new(),
        })
        .map(|mut app: App| {
            app.online.status = app.t("online.idle");
            app
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
        self.listen();
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
            // Keybinds is waiting for a key: the page keeps Tab, Space and
            // Enter from moving focus or pressing a button.
            "keys_captured": self.rebinding.is_some(),
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
        if !self.sounds.is_empty() {
            let keys: Vec<&str> = self.sounds.drain(..).map(|c| c.key()).collect();
            v["sounds"] = json!(keys);
        }
        v["volume"] = json!(self.player.volume);
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

    fn on_off(&self, id: &str, label_key: &str, on: bool) -> Value {
        let mut f = self.select(
            id,
            label_key,
            vec![("on".into(), self.t("on")), ("off".into(), self.t("off"))],
            if on { "on" } else { "off" },
        );
        f["term"] = json!(self.t(label_key));
        f
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
                    self.t("setting.map_type"),
                    self.t(&format!("map_type.{}", map_type_key(m.map_type))),
                ),
                (
                    self.t("setting.events"),
                    self.t(if m.events { "on" } else { "off" }),
                ),
                (
                    self.t("setting.map_revealed"),
                    self.t(if m.map_revealed { "on" } else { "off" }),
                ),
                (
                    self.t("setting.sandbox"),
                    self.t(if m.sandbox { "on" } else { "off" }),
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
                [
                    "picnic",
                    "flower_field",
                    "forest_floor",
                    "riverbank",
                    "random",
                ]
                .iter()
                .map(|v| (v.to_string(), self.t(&format!("biome.{v}"))))
                .collect(),
                biome_key(m.biome),
            ),
            self.select(
                &format!("{prefix}.map_type"),
                "setting.map_type",
                ["tunnels", "open", "maze"]
                    .iter()
                    .map(|v| (v.to_string(), self.t(&format!("map_type.{v}"))))
                    .collect(),
                map_type_key(m.map_type),
            ),
            self.select(
                &format!("{prefix}.events"),
                "setting.events",
                vec![("on".into(), self.t("on")), ("off".into(), self.t("off"))],
                if m.events { "on" } else { "off" },
            ),
            self.on_off(
                &format!("{prefix}.map_revealed"),
                "setting.map_revealed",
                m.map_revealed,
            ),
            self.on_off(&format!("{prefix}.sandbox"), "setting.sandbox", m.sandbox),
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
                    let d = if i == 0 {
                        self.first_seat
                    } else {
                        Some(self.ai_seats[i - 1])
                    };
                    let mut options = Vec::new();
                    if i == 0 {
                        // Seat 1 may be You or an AI; with no You, the match
                        // is spectated.
                        options.push((
                            "you".to_string(),
                            format!("{} · {}", self.t("seat.you"), team),
                        ));
                    }
                    for v in ["larva", "pupa", "imago"] {
                        options.push((
                            v.to_string(),
                            format!(
                                "{} {} · {}",
                                self.t("seat.ai"),
                                self.t(&format!("difficulty.{v}")),
                                team
                            ),
                        ));
                    }
                    let value = d.map(difficulty_key).unwrap_or("you");
                    let mut sel =
                        self.select(&format!("ai.{i}"), &format!("color.{i}"), options, value);
                    if let Some(d) = d {
                        sel["term"] = json!(self.t(&format!("difficulty.{}", difficulty_key(d))));
                    }
                    seats.push(sel);
                }
                if self.first_seat.is_some() {
                    seats.push(json!({"type": "list", "rows": [[self.t("stat.status"), self.t("hud.spectate")]]}));
                }
                seats.push(json!({"type": "input", "id": "pva.seed", "label": self.t("setting.seed"), "value": self.seed_text, "placeholder": self.t("random")}));
                vec![
                    json!({"kind": "form", "single": true, "heading": self.t("menu.play_vs_ai"), "items": seats}),
                    json!({"kind": "form", "heading": self.t("menu.match_settings"), "items": self.settings_fields(&self.setup, "pva", true)}),
                    json!({"kind": "row", "items": [self.button("go.main_menu", "menu.back"), self.button("pva.start", "menu.start")]}),
                ]
            }
            Screen::PlayerSettings => {
                let mut rows = Vec::new();
                // The All Ants hotkey sits with the grid tabs it works from.
                let mut slots: Vec<&str> = crate::keys::all_slots()
                    .into_iter()
                    .filter(|s| *s != "all_ants")
                    .collect();
                let after_tabs = slots
                    .iter()
                    .position(|s| *s == "tab.4")
                    .map_or(0, |i| i + 1);
                slots.insert(after_tabs, "all_ants");
                for slot in slots {
                    // Named as in the match (TRIAGE row 57): a grid button
                    // by its default key, "Action Button Q"; a tab by its
                    // name, "Grid Tab: Ants".
                    let label = if slot.starts_with("grid.") {
                        format!(
                            "{} {}",
                            self.t("keys.grid"),
                            crate::keys::default_label(slot)
                        )
                    } else if let Some(n) = slot.strip_prefix("tab.") {
                        let i = n.parse::<usize>().unwrap_or(0).min(TABS.len() - 1);
                        format!(
                            "{}: {}",
                            self.t("keys.tab"),
                            self.t(&format!("tab.{}", TABS[i]))
                        )
                    } else if slot == "all_ants" {
                        format!(
                            "{}: {}",
                            self.t("action.set_waypoint"),
                            self.t("action.all_ants")
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
                rows.push(self.volume_slider());
                let speeds = crate::keys::EDGE_PAN_SPEEDS
                    .iter()
                    .map(|(k, _)| (k.to_string(), self.t(&format!("speed.word.{k}"))))
                    .collect();
                let speed = crate::keys::EDGE_PAN_SPEEDS
                    .get(self.player.edge_pan_speed)
                    .map_or("normal", |(k, _)| *k);
                rows.push(self.select(
                    "player.edge_pan_speed",
                    "keys.edge_pan_speed",
                    speeds,
                    speed,
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
            Screen::Online => self.online_panels(),
            Screen::Lobby => self.lobby_panels(),
            Screen::Match => Vec::new(),
        }
    }

    fn online_panels(&self) -> Vec<Value> {
        let mut items = vec![
            json!({"type": "input", "id": "online.name", "label": self.t("online.name"), "value": self.player.name, "placeholder": self.t("seat.player")}),
            json!({"type": "input", "id": "online.room", "label": self.t("online.room_code"), "value": self.online.room}),
        ];
        if !self.online.room.is_empty() {
            items.push(self.copy_button(&self.online.room));
        }
        items.push(json!({"type": "list", "rows": [[self.t("online.status"), self.net_status()]]}));
        let mut panels = vec![
            json!({"kind": "form", "single": true, "heading": self.t("menu.online"), "items": items}),
            json!({"kind": "row", "items": [
                self.button("online.host", "online.host"),
                self.button("online.join", "online.join"),
                self.button("go.main_menu", "menu.back"),
            ]}),
        ];
        if self.online.codes {
            // The pasted-code path, behind the page's `?codes` flag.
            panels.push(json!({"kind": "form", "items": [
                {"type": "code", "id": "online.invite", "label": self.t("online.invite_code"), "value": self.online.invite},
                {"type": "code", "id": "online.reply", "label": self.t("online.reply_code"), "value": self.online.reply},
            ]}));
            panels.push(json!({"kind": "row", "items": [
                self.button("online.host_code", "online.host_by_code"),
                self.button("online.join_code", "online.join_by_code"),
                self.button("online.connect", "online.connect"),
            ]}));
        }
        panels
    }

    /// The Copy button: the page puts `text` on the clipboard.
    fn copy_button(&self, text: &str) -> Value {
        json!({"type": "button", "id": "online.copy", "text": self.t("online.copy"), "copy": text})
    }

    /// Status on the Online Multiplayer screen and in the Lobby: the lobby's
    /// own state while there is one, else the last thing that happened.
    fn net_status(&self) -> String {
        let Some(l) = &self.online.lobby else {
            return self.online.status.clone();
        };
        match l.status() {
            LobbyStatus::Joining | LobbyStatus::Started => self.t("online.connecting"),
            LobbyStatus::Seated(_) => self.t("online.in_lobby"),
            LobbyStatus::Refused(r) => self.refused_text(r),
            LobbyStatus::Dissolved | LobbyStatus::Left => self.t("online.dissolved"),
            LobbyStatus::Lost => self.t("online.lost"),
        }
    }

    fn refused_text(&self, r: &RefuseReason) -> String {
        let why = match r {
            RefuseReason::Full => "online.full",
            RefuseReason::WrongVersion { .. } => "online.wrong_version",
            RefuseReason::AlreadyStarted => "online.already_started",
        };
        format!("{}: {}", self.t("online.refused"), self.t(why))
    }

    /// One seat's Player cell: a name, AI and its Difficulty, Open or Closed.
    fn seat_text(&self, kind: &NetSeat) -> String {
        match kind {
            NetSeat::Human { name, .. } => name.clone(),
            NetSeat::Ai { difficulty } => format!(
                "{} {}",
                self.t("seat.ai"),
                self.t(&format!("difficulty.{}", net_difficulty_key(*difficulty)))
            ),
            NetSeat::Open => self.t("seat.open"),
            NetSeat::Closed => self.t("seat.closed"),
        }
    }

    /// The Lobby, after Civilization's: one row per seat (player color,
    /// player, team, Status), the host's Match Settings beside it (read-only
    /// for the others), and Leave, Ready Up or Start below.
    fn lobby_panels(&self) -> Vec<Value> {
        let Some(l) = &self.online.lobby else {
            return Vec::new();
        };
        let host = l.is_host();
        let settings = MatchSettings::parse(l.settings()).unwrap_or_default();
        let mut rows = Vec::new();
        for (i, seat) in l.seats().iter().enumerate() {
            let player = if host && i != 0 && seat.client_id().is_none() {
                // The host fills each seat without a human: Open, Closed,
                // or an AI of any Difficulty, in every format.
                let value = match &seat.kind {
                    NetSeat::Ai { difficulty } => format!("ai.{difficulty}"),
                    NetSeat::Closed => "closed".to_string(),
                    _ => "open".to_string(),
                };
                let mut opts = vec![
                    ("open".to_string(), self.t("seat.open")),
                    ("closed".to_string(), self.t("seat.closed")),
                ];
                for d in 0..3u8 {
                    opts.push((
                        format!("ai.{d}"),
                        self.seat_text(&NetSeat::Ai { difficulty: d }),
                    ));
                }
                let mut sel = self.select(&format!("seat.{i}"), "seat.player", opts, &value);
                sel["bare"] = json!(true);
                sel
            } else {
                let mut text = self.seat_text(&seat.kind);
                if l.status() == &LobbyStatus::Seated(i as u8) {
                    text = format!("{text} · {}", self.t("seat.you"));
                }
                json!(text)
            };
            let status = if i == 0 {
                self.t("seat.host")
            } else if seat.ready() {
                self.t("online.readied")
            } else {
                String::new()
            };
            rows.push(json!([
                self.t(&format!("color.{i}")),
                player,
                format!("{} {}", self.t("seat.team"), settings.format.team_of(i) + 1),
                status,
            ]));
        }
        let mut seats = vec![json!({
            "type": "table",
            "head": [self.t("seat.label"), self.t("seat.player"), self.t("seat.team"), self.t("online.status")],
            "rows": rows,
        })];
        let mut pairs = Vec::new();
        if !self.online.room.is_empty() {
            pairs.push(json!([self.t("online.room_code"), self.online.room]));
        }
        pairs.push(json!([self.t("online.status"), self.net_status()]));
        seats.push(json!({"type": "list", "rows": pairs}));
        if !self.online.room.is_empty() {
            seats.push(self.copy_button(&self.online.room));
        }
        let mut row = vec![self.button("lobby.leave", "online.leave")];
        if host {
            let mut start = self.button("lobby.start", "menu.start");
            start["disabled"] = json!(!self.can_start_online());
            row.push(start);
        } else {
            let mut ready = self.button("lobby.ready", "online.ready_up");
            ready["active"] = json!(l.wants_ready());
            ready["term"] = json!(self.t("online.ready_up"));
            row.push(ready);
        }
        let mut panels = vec![
            json!({"kind": "form", "single": true, "heading": self.t("online.lobby"), "items": seats}),
        ];
        panels.push(json!({"kind": "form", "heading": self.t("menu.match_settings"), "items": self.settings_fields(&settings, "lobby", host)}));
        if host && self.online.codes {
            // Pasted codes (`?codes` only): one Invite Code per joiner,
            // answered by a Reply Code.
            panels.push(json!({"kind": "form", "items": [
                {"type": "code", "id": "online.invite", "label": self.t("online.invite_code"), "value": self.online.invite},
                {"type": "code", "id": "online.reply", "label": self.t("online.reply_code"), "value": self.online.reply},
            ]}));
            row.push(self.button("online.host_code", "online.invite_code"));
            row.push(self.button("online.connect", "online.connect"));
        }
        panels.push(json!({"kind": "row", "items": row}));
        panels
    }

    // ---- the match view ----

    fn match_view(&self) -> Value {
        let Some(g) = &self.game else {
            return Value::Null;
        };
        let w = &g.world;
        let follow = g.follow();
        let mut hud = match follow {
            Some(me) => self.colony_bar(w, me),
            None => self.summary_bar(w),
        };
        let mut flags = Vec::new();
        if let Some(me) = follow {
            let c = &w.colonies[me as usize];
            let pop = w.population(me);
            let housing = w.housing(me);
            let queued_train = w
                .structures
                .iter()
                .filter(|s| s.colony == me && s.complete)
                .any(|s| {
                    s.lane.first().is_some_and(
                        |q| matches!(q.work, Work::Train(sp) if pop + sp.stats().pop > housing),
                    )
                });
            if queued_train {
                flags.push(
                    json!({"text": self.t("hud.housing_full"), "term": self.t("hud.housing_full")}),
                );
            }
            if c.retreat {
                flags.push(
                    json!({"text": self.t("hud.retreat_on"), "term": self.t("action.retreat")}),
                );
            }
        }
        if g.spectate {
            flags.push(json!({"text": self.t("hud.spectate"), "term": self.t("hud.spectate")}));
        }
        if g.paused {
            flags.push(json!({"text": self.t("hud.paused"), "term": self.t("time.pause")}));
        }
        if let Some(m) = &g.lockstep {
            match m.status() {
                MatchStatus::WaitingOn(seats) => {
                    let who: Vec<String> = seats
                        .iter()
                        .map(|s| self.seat_name(m.seats(), *s))
                        .collect();
                    flags.push(json!({"text": format!("{}: {}", self.t("hud.waiting_on"), who.join(", "))}));
                }
                MatchStatus::Resyncing => flags.push(json!({"text": self.t("hud.resyncing")})),
                _ => {}
            }
        }
        hud["flags"] = Value::Array(flags);
        hud["clock"] = json!(clock(w.tick));
        // While a Hill is open: its time left and the team leading it.
        if let Some(h) = &w.events.hill {
            let best = h.hold.iter().copied().max().unwrap_or(0);
            let leaders: Vec<usize> = (0..h.hold.len()).filter(|t| h.hold[*t] == best).collect();
            let leader = if best > 0 && leaders.len() == 1 {
                self.team_name(w, leaders[0] as u8)
            } else {
                self.t("stat.none")
            };
            let left = clock((h.ends - w.tick).max(0));
            if let Some(stats) = hud["stats"].as_array_mut() {
                stats.push(json!({"key": "hill", "label": self.t("event.hill"), "value": format!("{left} · {leader}"), "term": self.t("event.hill")}));
            }
        }

        let queue = match follow {
            Some(me) => self.queue_bar(w, me),
            None => Vec::new(),
        };

        // Action grid: none while spectating.
        let (tabs, buttons): (Vec<Value>, Vec<Value>) = if g.spectate {
            (Vec::new(), Vec::new())
        } else {
            let tabs = TABS
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
            let buttons = grid(g.tab)
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
            (tabs, buttons)
        };

        // Pause, Time Scale and (spectating) whose Fog of War is shown.
        let mut controls = Vec::new();
        if g.spectate {
            let whose = match g.vision {
                Some(c) => self.t(&format!("color.{c}")),
                None => self.t("off"),
            };
            controls.push(json!({"id": "spectate.vision", "text": format!("{}: {whose}", self.t("hud.fog_of_war")), "term": self.t("hud.fog_of_war"), "active": false}));
        }
        if g.can_pause() {
            controls.push(json!({"id": "time.pause", "text": self.t("time.pause"), "term": self.t("time.pause"), "active": g.paused}));
        }
        if g.time_controls() {
            controls.push(json!({"id": "time.scale", "text": format!("{}: {}", self.t("time.scale"), self.t(&format!("time.scale.{}", g.time_scale))), "term": self.t("time.scale"), "active": false}));
        }

        let notices: Vec<Value> = w
            .announcements
            .iter()
            .rev()
            .filter(|a| a.colony.is_none() || a.colony == follow)
            .filter(|a| w.tick - a.tick < 6 * TICKS_PER_SECOND)
            .take(4)
            .map(|a| json!({"text": self.announcement_text(w, &a.key)}))
            .collect();

        let mut v = json!({
            "hud": hud,
            "queue": queue,
            "tabs": tabs,
            "grid": buttons,
            "spectate": g.spectate,
            "controls": controls,
            "notices": notices,
            "mode": match g.mode {
                Mode::Idle => "idle",
                Mode::Place(_) | Mode::Wall(_) => "place",
                Mode::Waypoint(_) => "waypoint",
            },
        });
        if let Some(team) = w.winner {
            v["end"] = self.end_view(g, team);
            if self.can_rematch(g) {
                v["end"]["items"] = json!([
                    self.button("end.rematch", "end.rematch"),
                    v["end"]["button"].clone()
                ]);
            }
        } else if g.menu_open {
            v["menu"] = if g.confirm_concede {
                json!({"heading": self.t("menu.concede_confirm"), "items": [self.button("menu.concede_yes", "menu.concede"), self.button("menu.resume", "menu.resume")]})
            } else if g.spectate {
                // A spectator has nothing to concede: Resume or leave.
                json!({"items": [self.button("menu.resume", "menu.resume"), self.button("end.main_menu", "menu.main_menu")]})
            } else {
                json!({"items": [self.button("menu.resume", "menu.resume"), self.button("menu.concede", "menu.concede")]})
            };
            // Volume is in the Menu too, so it can change mid-match.
            if !g.confirm_concede {
                let slider = self.volume_slider();
                if let Some(items) = v["menu"]["items"].as_array_mut() {
                    items.push(slider);
                }
            }
        }
        v
    }

    /// The end screen (TRIAGE row 39): the result, Match Length, a graph
    /// of the match with one line per colony in its player color (tabs
    /// pick what it shows), and a label-value summary for every colony.
    /// The core lays out the graph: the page only copies the SVG path
    /// strings, label positions and colors into the page.
    fn end_view(&self, g: &Game, team: u8) -> Value {
        let w = &g.world;
        let end = g.ended_at.unwrap_or(w.tick);
        let title = if g.spectate {
            format!("{}: {}", self.t("end.victory"), self.team_name(w, team))
        } else {
            let mine = w.colonies[g.me as usize].team;
            self.t(if team == mine {
                "end.victory"
            } else {
                "end.defeat"
            })
        };
        let columns: Vec<Value> = (0..w.colonies.len())
            .map(|i| {
                let name = self.t(&format!("color.{i}"));
                let text = if !g.spectate && i == g.me as usize {
                    format!("{name} · {}", self.t("seat.you"))
                } else {
                    name
                };
                json!({"text": text, "colour": render::PALETTE[i % 4]})
            })
            .collect();
        let row = |label: String, f: &dyn Fn(&crate::world::Colony) -> i32| json!({"label": label, "values": w.colonies.iter().map(|c| f(c).to_string()).collect::<Vec<_>>()});
        let mut summary = vec![
            row(self.t("stat.trained"), &|c| c.tally.trained),
            row(self.t("stat.killed"), &|c| c.tally.killed),
            row(self.t("stat.lost"), &|c| c.tally.lost),
        ];
        for r in Resource::ALL {
            let label = format!(
                "{}: {}",
                self.t("stat.gathered"),
                self.t(&format!("resource.{}", r.key()))
            );
            summary.push(row(label, &|c| c.tally.gathered[r.index()]));
        }
        let tabs: Vec<Value> = END_GRAPHS
            .iter()
            .enumerate()
            .map(|(i, key)| {
                json!({"id": format!("end.graph.{i}"), "text": self.t(key), "active": i == g.end_graph})
            })
            .collect();
        json!({
            "title": title,
            "rows": [[self.t("stat.match_length"), clock(end)]],
            "tabs": tabs,
            "graph": self.end_graph(w, g.end_graph.min(END_GRAPHS.len() - 1), end),
            "columns": columns,
            "summary": summary,
            "button": self.button("end.main_menu", "menu.main_menu"),
        })
    }

    /// One end-screen graph, laid out in SVG units: the plot inside
    /// margins for the axis labels, a line per colony, and gridlines at 0,
    /// half and the top of a round scale.
    fn end_graph(&self, w: &World, graph: usize, end: i32) -> Value {
        let (left, top) = (GRAPH_MARGIN_LEFT, GRAPH_MARGIN_TOP);
        let plot_w = GRAPH_W - GRAPH_MARGIN_LEFT - GRAPH_MARGIN_RIGHT;
        let plot_h = GRAPH_H - GRAPH_MARGIN_TOP - GRAPH_MARGIN_BOTTOM;
        let span = end.max(SAMPLE_TICKS) as i64;
        // Gathered, Killed and Lost count from nothing at the start.
        let from_zero = !matches!(graph, 3 | 4);
        let series: Vec<Vec<(i32, i32)>> = w
            .colonies
            .iter()
            .map(|c| {
                let mut pts: Vec<(i32, i32)> = Vec::new();
                if from_zero {
                    pts.push((0, 0));
                }
                pts.extend(
                    c.history
                        .iter()
                        .filter(|s| s.tick <= end)
                        .map(|s| (s.tick, sample_value(s, graph))),
                );
                pts
            })
            .collect();
        let most = series.iter().flatten().map(|p| p.1).max().unwrap_or(0);
        let scale = nice_ceiling(most) as i64;
        let x = |t: i32| left + (t as i64 * plot_w as i64 / span) as i32;
        let y = |v: i32| top + plot_h - (v as i64 * plot_h as i64 / scale) as i32;
        let lines: Vec<Value> = series
            .iter()
            .enumerate()
            .filter(|(_, pts)| !pts.is_empty())
            .map(|(i, pts)| {
                let d: Vec<String> = pts
                    .iter()
                    .enumerate()
                    .map(|(k, (t, v))| {
                        format!("{} {} {}", if k == 0 { "M" } else { "L" }, x(*t), y(*v))
                    })
                    .collect();
                json!({"key": format!("colony.{i}"), "d": d.join(" "), "colour": render::PALETTE[i % 4]})
            })
            .collect();
        let right = left + plot_w;
        // A gridline at half the scale only when it is a whole round
        // number (250 of 500, 10 of 20), never 2 of 5.
        let mut marks = vec![0, scale];
        if scale % 2 == 0 {
            marks.insert(1, scale / 2);
        }
        let grid: Vec<String> = marks
            .iter()
            .map(|v| format!("M {left} {} H {right}", y(*v as i32)))
            .collect();
        let axis_y = top + plot_h;
        let mut labels: Vec<Value> = marks
            .iter()
            .map(|v| {
                json!({"x": left - 6, "y": y(*v as i32) + 4, "anchor": "end", "text": v.to_string()})
            })
            .collect();
        for (t, anchor) in [(0, "start"), (end / 2, "middle"), (end, "end")] {
            labels.push(json!({"x": x(t), "y": axis_y + 18, "anchor": anchor, "text": clock(t)}));
        }
        let title = match graph {
            0..=2 => format!("{}: {}", self.t("stat.gathered"), self.t(END_GRAPHS[graph])),
            3 => format!(
                "{}: {}",
                self.t("stat.combat_ants"),
                self.t("stat.pop_cost")
            ),
            _ => self.t(END_GRAPHS[graph]),
        };
        json!({
            "title": title,
            "width": GRAPH_W.to_string(),
            "height": GRAPH_H.to_string(),
            "view_box": format!("0 0 {GRAPH_W} {GRAPH_H}"),
            "grid": grid.join(" "),
            "axis": format!("M {left} {top} V {axis_y} H {right}"),
            "lines": lines,
            "labels": labels,
        })
    }

    /// The top bar for one colony: resources with Income, Population, Tier.
    fn colony_bar(&self, w: &World, me: u8) -> Value {
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
        // Army (TRIAGE row 48): every ant out now that is not a
        // Gatherer or Builder, and their Pop Cost; the queue is not counted.
        let (combat, combat_pop) = w
            .ants
            .iter()
            .filter(|a| a.colony == me && a.species.is_combat())
            .fold((0, 0), |(n, p), a| (n + 1, p + a.species.stats().pop));
        json!({
            "resources": resources,
            "stats": [
                {"key": "population", "label": self.t("stat.population"), "value": format!("{pop} / {housing}"), "term": self.t("stat.population")},
                {"key": "combat_ants", "label": self.t("stat.combat_ants"), "value": format!("{combat} · {} {combat_pop}", self.t("stat.pop_cost")), "term": self.t("stat.combat_ants")},
                {"key": "tier", "label": self.t("stat.tier"), "value": c.tier.to_string(), "term": self.t("stat.tier")},
            ],
            "colour": self.t(&format!("color.{me}")),
        })
    }

    /// The top bar while spectating the whole map: every colony's
    /// Population and Tier.
    fn summary_bar(&self, w: &World) -> Value {
        let stats: Vec<Value> = w
            .colonies
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let value = if c.defeated {
                    self.t("announce.defeated")
                } else {
                    format!(
                        "{} {} / {} · {} {}",
                        self.t("stat.population"),
                        w.population(i as u8),
                        w.housing(i as u8),
                        self.t("stat.tier"),
                        c.tier
                    )
                };
                json!({"key": format!("colony.{i}"), "label": self.t(&format!("color.{i}")), "value": value, "term": ""})
            })
            .collect();
        json!({"resources": [], "stats": stats, "colour": ""})
    }

    /// The name of a team: its player color when it is one colony (1v1,
    /// Free-for-all), else "Team 2".
    fn team_name(&self, w: &World, team: u8) -> String {
        let members: Vec<usize> = (0..w.colonies.len())
            .filter(|i| w.colonies[*i].team == team)
            .collect();
        if members.len() == 1 {
            self.t(&format!("color.{}", members[0]))
        } else {
            format!("{} {}", self.t("seat.team"), team as i32 + 1)
        }
    }

    /// Queue bar: for each queen or hatchery, one item per species in
    /// training (with its count) and one per upgrade or evolution, front
    /// first. A click cancels one: the last of its kind, so the front
    /// keeps its progress.
    fn queue_bar(&self, w: &World, me: u8) -> Vec<Value> {
        let mut queue = Vec::new();
        for s in w.structures.iter().filter(|s| s.colony == me) {
            let items: Vec<&crate::world::QueueItem> =
                s.lane.iter().chain(s.research.iter()).collect();
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            for (slot, q) in items.iter().enumerate() {
                let key = match q.work {
                    Work::Train(sp) => format!("species.{}", sp.key()),
                    _ => format!("slot.{slot}"),
                };
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, slots)) => slots.push(slot),
                    None => groups.push((key, vec![slot])),
                }
            }
            for (key, slots) in groups {
                let front = items[slots[0]];
                // Upgrades are told apart (TRIAGE row 53): the rank on the
                // picture, the stat they raise beside it, and a kind that
                // the page styles differently for a Colony Upgrade and a
                // Species Upgrade. An evolution shows the tier it reaches.
                let mut extra: Vec<Value> = Vec::new();
                let (label, icon, kind, rank, short) = match front.work {
                    Work::Train(sp) => (
                        self.species_name(sp),
                        format!("species.{}", sp.key()),
                        "train",
                        String::new(),
                        String::new(),
                    ),
                    Work::Research(u, r) => {
                        let (stat, _) = upgrade_change(u);
                        let species = matches!(u, Upgrade::Species(_));
                        extra.push(json!([self.t("stat.rank"), roman(r)]));
                        extra.push(json!([
                            self.t("stat.upgrade"),
                            self.t(if species {
                                "stat.species_upgrade"
                            } else {
                                "stat.colony_upgrade"
                            })
                        ]));
                        extra.push(json!([self.t("stat.effect"), self.upgrade_effect(u)]));
                        (
                            format!("{} {}", self.upgrade_name(u), roman(r)),
                            format!("upgrade.{}", u.key()),
                            if species {
                                "species_upgrade"
                            } else {
                                "colony_upgrade"
                            },
                            roman(r).to_string(),
                            self.t(stat),
                        )
                    }
                    Work::Evolve(t) => (
                        format!("{} {} {t}", self.t("action.evolve"), self.t("stat.tier")),
                        "action.evolve".into(),
                        "evolve",
                        format!("{}{t}", self.t("short.tier")),
                        String::new(),
                    ),
                };
                let n = slots.len();
                let count = if n > 1 {
                    format!("×{n}")
                } else {
                    String::new()
                };
                let title = if n > 1 {
                    format!("{label} {count}")
                } else {
                    label
                };
                let left: i32 = slots
                    .iter()
                    .map(|i| items[*i].total - items[*i].progress)
                    .sum();
                let pct = front.progress * 100 / front.total.max(1);
                let last = slots[n - 1];
                let mut rows = extra;
                rows.push(json!([self.t("stat.duration"), self.seconds(left)]));
                rows.push(json!([self.t("stat.cost"), self.cost_text(&front.cost)]));
                rows.push(json!([
                    self.t("stat.hotkey"),
                    format!("{}: {}", self.t("hud.click"), self.t("action.cancel"))
                ]));
                queue.push(json!({
                    "key": format!("{}.{key}", s.id),
                    "id": format!("cancel.{}.{last}", s.id),
                    "label": title,
                    "icon": icon,
                    "kind": kind,
                    "rank": rank,
                    "short": short,
                    "count": count,
                    "progress": format!("{pct}%"),
                    "rows": rows,
                }));
            }
        }
        queue
    }

    fn announcement_text(&self, w: &World, key: &str) -> String {
        if key == "announce.hill" {
            return format!(
                "{} · {}: {}",
                self.t("announce.hill"),
                self.t("stat.reward"),
                self.cost_text(&HILL_REWARD)
            );
        }
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
            let gains: Vec<String> = Resource::ALL
                .iter()
                .filter(|r| HILL_REWARD[r.index()] > 0)
                .map(|r| {
                    format!(
                        "+{} {}",
                        HILL_REWARD[r.index()],
                        self.t(&format!("resource.{}", r.key()))
                    )
                })
                .collect();
            return format!(
                "{}: {} · {}",
                self.t("announce.hill_won"),
                self.team_name(w, t.parse::<u8>().unwrap_or(0)),
                gains.join(" · ")
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
        // The tier badge: the tier a button needs (Ants, Build, Upgrades).
        let mut tier: Option<u8> = None;
        let (label, icon, state, active) = match b {
            Button::Train(s) => {
                let st = c.ranks.apply(s);
                tier = Some(st.tier);
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
                        format!(
                            "{}{}",
                            tenths(st.attack_ticks * 10 / TICKS_PER_SECOND),
                            self.t("unit.seconds")
                        ),
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
                            tenths(TICKS_PER_SECOND * 100 / st.gather_ticks.max(1)),
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
                tier = Some(st.tier);
                rows.push(row("stat.cost", self.cost_text(&st.cost)));
                rows.push(row("stat.health", st.health.to_string()));
                if st.housing > 0 {
                    rows.push(row("stat.housing", st.housing.to_string()));
                }
                rows.push(row("stat.tier", st.tier.to_string()));
                match k {
                    StructureKind::Hatchery(s) => rows.push(row(
                        "stat.effect",
                        format!("{} {}", self.t("action.train"), self.species_name(s)),
                    )),
                    StructureKind::Queen => {
                        let names: Vec<String> = Species::ALL
                            .iter()
                            .filter(|s| s.from_queen())
                            .map(|s| self.species_name(*s))
                            .collect();
                        rows.push(row(
                            "stat.effect",
                            format!(
                                "{} {} · {} · {} +{}",
                                self.t("action.train"),
                                names.join(", "),
                                self.t("stat.drop_off"),
                                self.t("stat.housing"),
                                st.housing
                            ),
                        ));
                    }
                    StructureKind::BroodCell => rows.push(row(
                        "stat.effect",
                        format!("{} +{}", self.t("stat.housing"), st.housing),
                    )),
                    _ => {}
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
                // All Ants also has its own hotkey, Tab, from any grid tab.
                let keys = match sp {
                    None => format!("{key} · {}", key_label(&self.player.key_of("all_ants"))),
                    Some(_) => key.to_string(),
                };
                rows.push(row(
                    "stat.hotkey",
                    format!(
                        "{keys} · {}: {} · Shift {}: {}",
                        self.t("hud.click"),
                        self.t("action.set_waypoint"),
                        self.t("hud.click"),
                        self.t("action.add_waypoint")
                    ),
                ));
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
                    tier = Some(u.tier_for(rank));
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
            Button::AutoScout => {
                rows.push(row(
                    "stat.status",
                    self.t(if c.auto_scout { "on" } else { "off" }),
                ));
                rows.push(row("stat.hotkey", key.to_string()));
                (
                    self.t("action.auto_scout"),
                    button_icon(b),
                    "ready".into(),
                    c.auto_scout,
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
            Button::AutoScout => self.t("action.auto_scout"),
            Button::Focus(_) => self.t("action.gather_focus"),
            _ => String::new(),
        };
        // Not enough resources: the state names the first one missing.
        let (state, state_text) = match state.split_once(':') {
            Some((s, r)) => (s.to_string(), self.t(&format!("notice.not_enough.{r}"))),
            None => {
                let text = self.t(&format!("state.{state}"));
                (state, text)
            }
        };
        // All Ants shows its short form, ALL, in place of a picture.
        let (icon, icon_text) = match b {
            Button::Waypoint(None) => (String::new(), self.t("short.all_ants")),
            _ => (icon, String::new()),
        };
        json!({
            "id": format!("grid.{i}"),
            "label": label,
            "icon": icon,
            "icon_text": icon_text,
            "tier": tier.map(|t| format!("{}{t}", self.t("short.tier"))).unwrap_or_default(),
            "key": key,
            "state": state,
            "state_text": state_text,
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
        let (stat, by) = upgrade_change(u);
        format!("{} {by}", self.t(stat))
    }

    /// "ready", "locked" (tier, hatchery), "disabled", or "unaffordable:"
    /// and the first resource short.
    fn state_of(&self, gate: Result<(), Refusal>, cost: &[i32; 3], w: &World, me: u8) -> String {
        match gate {
            Err(Refusal::Disabled) => "disabled".into(),
            Err(Refusal::AlreadyQueued) => "queued".into(),
            Err(Refusal::MaxRank) => "max_rank".into(),
            Err(_) => "locked".into(),
            Ok(()) if !w.can_pay(me, cost) => {
                let stock = &w.colonies[me as usize].stock;
                let short = Resource::ALL
                    .iter()
                    .find(|r| stock[r.index()] < cost[r.index()])
                    .unwrap_or(&Resource::Food);
                format!("unaffordable:{}", short.key())
            }
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
                // Typed codes are upper-cased: k7mxp4 joins K7MXP4.
                self.online.room = value
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .map(|c| c.to_ascii_uppercase())
                    .take(32)
                    .collect()
            }
            "flag.codes" => self.online.codes = value == "on",
            "online.invite" => self.online.invite = value.trim().to_string(),
            "online.reply" => self.online.reply = value.trim().to_string(),
            "online.host" => {
                // Host with the field empty fills in a random Room Code.
                if self.online.room.is_empty() {
                    self.online.room = self.new_room_code();
                }
                let room = format!("{BUILD_ID}-{}", self.online.room);
                self.host_online(json!({"op": "host_room", "room": room}))
            }
            "online.join" => {
                if self.online.room.is_empty() {
                    self.online.status =
                        format!("{}: {}", self.t("online.room_code"), self.t("stat.none"));
                } else {
                    let room = format!("{BUILD_ID}-{}", self.online.room);
                    self.join_online(json!({"op": "join_room", "room": room}))
                }
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
            "lobby.ready" => self.lobby_ready(),
            "end.rematch" => self.rematch(),
            "online.name" => {
                self.player.name = value
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_NAME_CHARS)
                    .collect();
                self.save_settings();
            }
            // The page copies the Room Code; nothing changes here.
            "online.copy" => {}
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
            "time.pause" => {
                if let Some(g) = self.game.as_mut().filter(|g| g.can_pause()) {
                    g.paused = !g.paused;
                }
            }
            "time.scale" => {
                if let Some(g) = self.game.as_mut().filter(|g| g.time_controls()) {
                    g.time_scale = (g.time_scale + 1) % TIME_SCALES.len();
                }
            }
            "spectate.vision" => {
                // Whole map, then each colony's Fog of War in seat order.
                if let Some(g) = self.game.as_mut().filter(|g| g.spectate) {
                    let n = g.world.colonies.len() as u8;
                    g.vision = match g.vision {
                        None => Some(0),
                        Some(c) if c + 1 < n => Some(c + 1),
                        Some(_) => None,
                    };
                }
            }
            "player.edge_pan" => {
                self.player.edge_pan = value == "on";
                self.save_settings();
            }
            "player.volume" => {
                if let Ok(v) = value.parse::<u8>() {
                    // Multiples of the slider's step only, so a stored 1–3
                    // can never be mistaken for the old 4-step setting.
                    self.player.volume = (v.min(100) / VOLUME_STEP) * VOLUME_STEP;
                    self.save_settings();
                    self.cue(Cue::Click);
                }
            }
            "player.edge_pan_speed" => {
                if let Some(i) = crate::keys::EDGE_PAN_SPEEDS
                    .iter()
                    .position(|(k, _)| *k == value)
                {
                    self.player.edge_pan_speed = i;
                    self.save_settings();
                }
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
        } else if let Some(n) = id.strip_prefix("end.graph.") {
            if let (Ok(i), Some(g)) = (n.parse::<usize>(), &mut self.game) {
                g.end_graph = i.min(END_GRAPHS.len() - 1);
            }
        } else if let Some(rest) = id.strip_prefix("cancel.") {
            let mut it = rest.split('.');
            let sid = it.next().and_then(|v| v.parse::<u32>().ok());
            let slot = it.next().and_then(|v| v.parse::<u32>().ok());
            if let (Some(structure), Some(slot), Some(g)) = (sid, slot, &mut self.game) {
                g.issue(Command::Cancel { structure, slot });
            }
        } else if let Some(rest) = id.strip_prefix("ai.") {
            match (rest.parse::<usize>(), parse_difficulty(value)) {
                (Ok(0), d) if d.is_some() || value == "you" => self.first_seat = d,
                (Ok(i), Some(d)) if (1..=3).contains(&i) => self.ai_seats[i - 1] = d,
                _ => {}
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

    /// A fresh number from the page's entropy; each call gives another.
    fn fresh(&mut self) -> u64 {
        let mut r = Rng::new(self.entropy);
        let n = ((r.next_u32() as u64) << 32) | r.next_u32() as u64;
        self.entropy = r.state().max(1);
        n
    }

    /// A random Room Code: 6 symbols, without 1, 0, I and O.
    pub fn new_room_code(&mut self) -> String {
        let mut r = Rng::new(self.fresh());
        (0..ROOM_CODE_LEN)
            .map(|_| ROOM_CODE_SYMBOLS[r.below(ROOM_CODE_SYMBOLS.len() as u32) as usize] as char)
            .collect()
    }

    fn start_local(&mut self) {
        // No Seed typed: a new random map for every match.
        let seed = match self.seed_text.parse::<u64>() {
            Ok(s) => s,
            Err(_) => self.fresh(),
        };
        self.start_local_seeded(seed);
    }

    /// Play vs AI with the current settings and seats on this seed.
    fn start_local_seeded(&mut self, seed: u64) {
        let mut settings = self.setup.clone();
        settings.biome = settings.biome.resolve(seed);
        let mut seats = vec![match self.first_seat {
            Some(d) => SeatKind::Ai(d),
            None => SeatKind::Human,
        }];
        for i in 1..settings.format.seats() {
            seats.push(SeatKind::Ai(self.ai_seats[i - 1]));
        }
        let world = World::new(seed, settings, &seats);
        self.game = Some(match self.first_seat {
            Some(_) => Game::spectating(world),
            None => Game::new(world, 0, None),
        });
        self.screen = Screen::Match;
    }

    fn press_grid(&mut self, i: usize, ctrl: bool, shift: bool) {
        let Some(g) = &mut self.game else {
            return;
        };
        if g.spectate {
            return;
        }
        let Some(b) = grid(g.tab).get(i).copied().flatten() else {
            return;
        };
        match b {
            Button::Train(species) => {
                // No housing room left: warn now (training would only wait).
                let w = &g.world;
                let me = g.me;
                let full = !g.spectate
                    && w.population(me) + w.queued_pop(me) + species.stats().pop > w.housing(me);
                let amount = if shift {
                    Amount::Max
                } else if ctrl {
                    Amount::Ten
                } else {
                    Amount::One
                };
                g.issue(Command::Train { species, amount });
                if full {
                    self.cue(Cue::Warning);
                }
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
            Button::AutoScout => {
                let on = !g.world.colonies[g.me as usize].auto_scout;
                g.issue(Command::AutoScout { on });
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
        } else if slot == "pause" {
            self.act("time.pause", "", false, false);
        } else if slot == "all_ants" {
            // From any grid tab, exactly like the Waypoints tab's All Ants.
            if let Some(g) = self.game.as_mut().filter(|g| !g.spectate) {
                g.mode = Mode::Waypoint(None);
            }
        } else if slot == "home" {
            if let Some(g) = &mut self.game {
                let here = Pos::new(g.view.centre_x, g.view.centre_y);
                if let Some(q) = g.world.nearest_queen(g.me, here) {
                    g.view.centre_x = q.pos.x;
                    g.view.centre_y = q.pos.y;
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
                    // A plain click replaces the species' waypoints and
                    // leaves the mode; Shift + click adds one and stays.
                    g.issue(Command::SetWaypoint {
                        species,
                        x: cx,
                        y: cy,
                        add: shift,
                    });
                    if !shift {
                        g.mode = Mode::Idle;
                    }
                }
                _ => g.drag = Some((x, y, 0, 0)),
            },
            (0, 1) => g.drag = Some((x, y, 0, 0)),
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
                if let Some((sx, sy, _, _)) = g.drag {
                    g.view.pan(x - sx, y - sy);
                    g.view.clamp(g.world.map.w, g.world.map.h);
                    g.drag = Some((x, y, 0, 0));
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
            let step = if dy < 0 { 1 } else { -1 };
            g.view.zoom_at(step, g.mouse.0, g.mouse.1);
            g.view.clamp(g.world.map.w, g.world.map.h);
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        if let Some(g) = &mut self.game {
            g.view.resize(w, h);
        }
    }

    /// Jumps the camera to a point on the minimap, which is `w` x `h`
    /// pixels and shows the whole map.
    pub fn jump(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if let Some(g) = &mut self.game {
            // The minimap shows the whole map, square, centred in its box.
            let m = g.world.map.w.max(g.world.map.h);
            let side = w.min(h).max(1);
            let ox = (w - side) / 2 + (m - g.world.map.w) * side / m / 2;
            let oy = (h - side) / 2 + (m - g.world.map.h) * side / m / 2;
            g.view.centre_x = (x - ox) * m * CELL / side;
            g.view.centre_y = (y - oy) * m * CELL / side;
            g.view.clamp(g.world.map.w, g.world.map.h);
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
        // Camera: arrow keys, and screen edges at the Edge Pan Speed. On an
        // axis with an arrow key held, the key's speed wins.
        let key_pan = g.pan_speed * dt as i32 / 1000;
        let edge_pan = self.player.edge_pan_px() * dt as i32 / 1000;
        let edge = 4;
        let (mx, my) = g.mouse;
        let e = self.player.edge_pan && g.drag.is_none() && g.mouse != (0, 0);
        let keys_x = g.pan_keys[0] as i32 - g.pan_keys[1] as i32;
        let keys_y = g.pan_keys[2] as i32 - g.pan_keys[3] as i32;
        let edge_x = (e && mx <= edge) as i32 - (e && mx >= g.view.view_w - edge) as i32;
        let edge_y = (e && my <= edge) as i32 - (e && my >= g.view.view_h - edge) as i32;
        let axis = |keys: i32, edges: i32| {
            if keys != 0 {
                key_pan * keys
            } else {
                edge_pan * edges
            }
        };
        // pan() moves the map with the pointer, so the view goes the other way.
        g.view.pan(axis(keys_x, edge_x), axis(keys_y, edge_y));
        g.view.clamp(g.world.map.w, g.world.map.h);

        if g.world.winner.is_some() {
            g.ended_at.get_or_insert(g.world.tick);
            if g.lockstep.is_none() {
                return;
            }
        }
        let ms = g.ms_per_tick();
        // Pause and Time Scale (local Sandbox or spectated matches only)
        // scale real time before it reaches the simulation.
        let (num, den) = if g.time_controls() {
            TIME_SCALES[g.time_scale]
        } else {
            (1, 1)
        };
        // Paused, or the Menu is open: time stands still when not online.
        let paused = (g.paused || g.menu_open) && g.can_pause();
        let dt = if paused { 0 } else { dt * num / den };
        g.acc_ms = (g.acc_ms + dt).min(ms * 8 * num);
        match &mut g.lockstep {
            None => {
                let mut ran = 0;
                while g.acc_ms >= ms && ran < 5 * num && g.world.winner.is_none() {
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

    // ---- sound ----

    /// The Volume slider, 0–100%, for Player Settings and the in-match Menu.
    fn volume_slider(&self) -> Value {
        json!({
            "type": "slider",
            "id": "player.volume",
            "label": self.t("keys.volume"),
            "value": self.player.volume,
            "min": 0,
            "max": 100,
            "step": VOLUME_STEP,
            "text": format!("{}%", self.player.volume),
        })
    }

    /// Queues a sound unless it played within its cooldown.
    pub fn cue(&mut self, c: Cue) {
        let last = self.last_cue.get(&c).copied();
        if last.is_some_and(|t| self.now_ms - t < c.cooldown_ms()) {
            return;
        }
        self.last_cue.insert(c, self.now_ms);
        self.sounds.push(c);
    }

    /// Turns the match's new notices and announcements into sounds.
    fn listen(&mut self) {
        let Some(g) = &mut self.game else {
            return;
        };
        let me = g.me;
        let mut cues = Vec::new();
        for a in g.world.announcements.iter().filter(|a| a.tick > g.heard) {
            if a.colony.is_some_and(|c| c != me) {
                continue;
            }
            let k = a.key.as_str();
            let c = if k.starts_with("notice.upgraded") || k.starts_with("notice.evolved") {
                Some(Cue::Complete)
            } else if k == "notice.queen_lost" {
                Some(Cue::Alert)
            } else if k.starts_with("announce.windfall") || k == "announce.hill" {
                Some(Cue::Event)
            } else if k.starts_with("notice.") {
                // Every other notice is a refusal: not enough, needs a tier,
                // Pop Cap, a blocked placement…
                Some(Cue::Warning)
            } else {
                None
            };
            cues.extend(c);
        }
        g.heard = g.world.tick;
        if let (Some(team), false) = (g.world.winner, g.sounded_end) {
            g.sounded_end = true;
            let mine = g.world.colonies[me as usize].team;
            cues.push(if g.spectate || team == mine {
                Cue::Victory
            } else {
                Cue::Defeat
            });
        }
        for c in cues {
            self.cue(c);
        }
    }

    /// The WAV file for a sound, at the Player Settings volume.
    pub fn sound(&self, key: &str) -> Vec<u8> {
        match Cue::from_key(key) {
            Some(c) => crate::sound::wav(c, self.player.volume as i32),
            None => Vec::new(),
        }
    }

    // ---- drawing ----

    /// The draw list for this animation frame (empty outside a match).
    pub fn frame(&self) -> Vec<i32> {
        let Some(g) = &self.game else {
            return Vec::new();
        };
        let viewer = g.follow();
        let (cx, cy) = g.mouse_cell();
        let place = match g.mode {
            Mode::Place(kind) => Some((kind, cx, cy, cx, cy)),
            Mode::Wall(Some((sx, sy))) => Some((StructureKind::Wall, sx, sy, cx, cy)),
            Mode::Wall(None) => Some((StructureKind::Wall, cx, cy, cx, cy)),
            _ => None,
        }
        .map(|(kind, x, y, x2, y2)| Placing {
            colony: g.me,
            kind,
            x,
            y,
            x2,
            y2,
            valid: g.world.check_place(g.me, kind, x, y, x2, y2).is_ok()
                && g.world
                    .can_pay(g.me, &place_cost(&g.world, g.me, kind, x, y, x2, y2)),
        });
        let mouse = g.view.world_at(g.mouse.0, g.mouse.1);
        let hover_waypoint = if g.spectate {
            None
        } else {
            g.world.colonies[g.me as usize]
                .waypoints
                .iter()
                .flatten()
                .copied()
                .filter(|w| w.dist(mouse) <= 3 * CELL)
                .min_by_key(|w| w.dist2(mouse))
        };
        let overlay = Overlay {
            terrain_scale: render::TERRAIN_SCALE,
            place,
            hover_waypoint,
        };
        render::frame(&g.world, viewer, &g.view, &overlay, g.interp_permille())
    }

    /// Every interface icon for the local colony's colour, as draw lists:
    /// `{"size": px, "lists": {key: [...]}}`. Empty outside a match.
    pub fn icons(&self) -> Value {
        let Some(g) = &self.game else {
            return json!({"size": 0, "lists": {}});
        };
        let mut keys: Vec<String> = Vec::new();
        for tab in 0..TABS.len() {
            for b in grid(tab).iter().flatten() {
                keys.push(button_icon(*b));
            }
        }
        keys.push("action.evolve".into());
        let mut lists = serde_json::Map::new();
        for k in keys {
            if let Some(l) = render::icon_list(&k, g.me, ICON_PX) {
                lists.insert(k, json!(l));
            }
        }
        json!({"size": ICON_PX, "lists": lists})
    }

    /// Changes when the terrain image must be painted again: a new match,
    /// or dirt dug away.
    pub fn terrain_stamp(&self) -> u32 {
        match &self.game {
            Some(g) => render::terrain_stamp(&g.world) ^ (g.world.seed as u32).rotate_left(16),
            None => 0,
        }
    }

    /// [width, height] of the terrain image.
    pub fn terrain_size(&self) -> Vec<i32> {
        match &self.game {
            Some(g) => vec![
                g.world.map.w * render::TERRAIN_SCALE,
                g.world.map.h * render::TERRAIN_SCALE,
            ],
            None => vec![0, 0],
        }
    }

    /// The terrain image's RGBA bytes.
    pub fn terrain(&self) -> Vec<u8> {
        match &self.game {
            Some(g) => render::terrain_image(&g.world, render::TERRAIN_SCALE).2,
            None => Vec::new(),
        }
    }

    /// Changes every tick: the fog and minimap images are painted again.
    /// Also changes when a spectator switches whose Fog of War is shown.
    pub fn fog_stamp(&self) -> i32 {
        self.game
            .as_ref()
            .map(|g| g.world.tick * 8 + g.follow().map_or(0, |c| c as i32 + 1))
            .unwrap_or(-1)
    }

    pub fn fog_size(&self) -> Vec<i32> {
        match &self.game {
            Some(g) => vec![g.world.map.w, g.world.map.h],
            None => vec![0, 0],
        }
    }

    pub fn fog(&self) -> Vec<u8> {
        match &self.game {
            Some(g) => render::fog_image(&g.world, g.follow()),
            None => Vec::new(),
        }
    }

    /// The minimap, `size` x `size` RGBA.
    pub fn minimap(&self, size: i32) -> Vec<u8> {
        match &self.game {
            Some(g) => render::minimap(&g.world, g.follow(), &g.view, size),
            None => Vec::new(),
        }
    }

    // ---- online ----

    fn version() -> Version {
        Version::current(BUILD_ID)
    }

    /// The Player Name for the Lobby: the one typed, or "Player".
    fn player_name(&self) -> String {
        let name = clean_name(&self.player.name);
        if name.is_empty() {
            self.t("seat.player")
        } else {
            name
        }
    }

    fn host_online(&mut self, request: Value) {
        self.leave_online();
        let settings = serde_json::to_string(&self.defaults).expect("serialises");
        let lobby = Lobby::host(
            App::version(),
            &self.client_id,
            &self.player_name(),
            self.defaults.format.seats(),
            &settings,
            self.now_ms,
        );
        self.online.lobby = Some(lobby);
        self.online.session = true;
        self.online.requests.push(request);
        self.screen = Screen::Lobby;
    }

    fn join_online(&mut self, request: Value) {
        self.leave_online();
        self.online.session = true;
        self.online.requests.push(request);
        self.online.status = self.t("online.connecting");
    }

    /// Leaves whatever online session there is (a lobby, a match, or a Join
    /// still connecting) and resets the transport, so the next Host or Join
    /// starts clean. Status: Not Connected.
    fn leave_online(&mut self) {
        if let Some(l) = &mut self.online.lobby {
            let out = l.leave();
            self.online.outbox.extend(out);
        }
        if self.online.session {
            self.online.requests.push(json!({"op": "leave"}));
        }
        self.online.session = false;
        self.online.lobby = None;
        self.online.early_peers.clear();
        self.online.invite.clear();
        self.online.reply.clear();
        self.online.status = self.t("online.idle");
    }

    /// The lobby ended without this client leaving it (refused, closed by
    /// the host, or the link to the host lost): back to Online Multiplayer,
    /// with Status saying why.
    fn lobby_ended(&mut self) {
        let ended = self.online.lobby.as_ref().is_some_and(|l| {
            matches!(
                l.status(),
                LobbyStatus::Refused(_) | LobbyStatus::Dissolved | LobbyStatus::Lost
            )
        });
        if ended {
            let why = self.net_status();
            self.leave_online();
            self.online.status = why;
            self.screen = Screen::Online;
        }
    }

    fn lobby_seat(&mut self, rest: &str, value: &str) {
        let now = self.now_ms;
        let (Ok(i), Some(l)) = (rest.parse::<usize>(), &mut self.online.lobby) else {
            return;
        };
        let kind = match value {
            "open" => NetSeat::Open,
            "closed" => NetSeat::Closed,
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
        // The format sets the number of seats; it is refused while more
        // humans sit in the lobby than it has seats.
        match l.set_seat_count(m.format.seats(), now) {
            Ok(out) => self.online.outbox.extend(out),
            Err(_) => return,
        }
        if let Ok(out) = l.set_settings(&serde_json::to_string(&m).expect("serialises"), now) {
            self.online.outbox.extend(out);
        }
    }

    /// Joiner: Ready Up, or undo it.
    fn lobby_ready(&mut self) {
        let now = self.now_ms;
        if let Some(l) = &mut self.online.lobby {
            let want = !l.wants_ready();
            if let Ok(out) = l.set_ready(want, now) {
                self.online.outbox.extend(out);
            }
        }
    }

    /// The host may Start: every joined human has pressed Ready Up, and
    /// seats that are not Closed make at least two teams.
    fn can_start_online(&self) -> bool {
        let Some(l) = &self.online.lobby else {
            return false;
        };
        let m = MatchSettings::parse(l.settings()).unwrap_or_default();
        let teams: BTreeSet<u8> = l
            .seats()
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind != NetSeat::Closed)
            .map(|(i, _)| m.format.team_of(i))
            .collect();
        l.is_host() && l.all_ready() && teams.len() >= 2
    }

    fn start_online(&mut self) {
        if !self.can_start_online() {
            return;
        }
        let now = self.now_ms;
        let seed = self.fresh();
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
                NetSeat::Ai { difficulty } => SeatKind::Ai(net_difficulty(difficulty)),
                _ => SeatKind::Human,
            })
            .collect();
        let me = start.local_seat;
        let (m, out) = Match::new(start.clone(), self.now_ms);
        self.online.outbox.extend(out);
        let mut settings = settings;
        settings.biome = settings.biome.resolve(start.seed);
        let mut world = World::new(start.seed, settings, &seats);
        // A Closed seat is not played: its colony is out from the first
        // tick, alike on every client.
        let mut closed = false;
        for (i, s) in start.seats.iter().enumerate() {
            if matches!(s.kind, NetSeat::Closed) && i < world.colonies.len() {
                world.defeat(i as u8);
                closed = true;
            }
        }
        if closed {
            world.announcements.clear();
        }
        self.game = Some(Game::new(world, me, Some(m)));
        self.online.lobby = None;
        self.screen = Screen::Match;
    }

    /// Rematch is offered: after a local match, or to the host online.
    fn can_rematch(&self, g: &Game) -> bool {
        g.lockstep.as_ref().is_none_or(|m| m.local_seat() == 0)
    }

    /// Rematch. Play vs AI: a new match with the same settings and seats on
    /// a new seed. Online (host): everyone back to the same Lobby, with
    /// Ready Up cleared.
    fn rematch(&mut self) {
        let Some(g) = &self.game else {
            return;
        };
        if !self.can_rematch(g) {
            return;
        }
        match &g.lockstep {
            None => {
                let seed = self.fresh();
                self.start_local_seeded(seed);
            }
            Some(m) => {
                let (l, out) = Lobby::rematch(
                    App::version(),
                    &self.client_id,
                    &self.player_name(),
                    m,
                    self.now_ms,
                );
                self.online.outbox.extend(out);
                self.online.lobby = Some(l);
                self.game = None;
                self.screen = Screen::Lobby;
            }
        }
    }

    /// The name shown for a seat in Waiting On: its Player Name, or its
    /// player color.
    fn seat_name(&self, seats: &[Seat], seat: u8) -> String {
        match seats.get(seat as usize).map(|s| &s.kind) {
            Some(NetSeat::Human { name, .. }) if !name.is_empty() => name.clone(),
            _ => self.t(&format!("color.{seat}")),
        }
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
        let name = self.player_name();
        if let Some(g) = &mut self.game {
            if let Some(m) = &mut g.lockstep {
                if kind == "message" && m.is_rematch(&peer, text) {
                    // The host pressed Rematch: back to the same Lobby.
                    let (mut l, out) =
                        Lobby::rejoin(App::version(), peer.clone(), &self.client_id, &name, m, now);
                    self.online.outbox.extend(out);
                    let out = l.receive(&peer, text, now);
                    self.online.outbox.extend(out);
                    self.online.lobby = Some(l);
                    self.game = None;
                    self.screen = Screen::Lobby;
                    return;
                }
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
                let (mut l, out) = Lobby::join(App::version(), peer, &self.client_id, &name, now);
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
        self.lobby_ended();
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

/// What a blueprint will cost: walls cost per cell actually built.
fn place_cost(
    w: &World,
    colony: u8,
    kind: StructureKind,
    x: i32,
    y: i32,
    x2: i32,
    y2: i32,
) -> [i32; 3] {
    let base = kind.stats().cost;
    if kind == StructureKind::Wall {
        let n = w.wall_plan(colony, x, y, x2, y2).len() as i32;
        [base[0] * n, base[1] * n, base[2] * n]
    } else {
        base
    }
}

/// Icons are drawn at this many pixels square.
pub const ICON_PX: i32 = 48;

/// The end screen's graph tabs, as string keys, in order (TRIAGE row 39):
/// Food, Nectar and Leaves gathered; Army (its Pop Cost);
/// Population; Killed; Lost.
pub const END_GRAPHS: [&str; 7] = [
    "resource.food",
    "resource.nectar",
    "resource.leaves",
    "stat.combat_ants",
    "stat.population",
    "stat.killed",
    "stat.lost",
];

/// End-screen graph size in SVG units (pixels at 1:1), and the margins
/// around the plot that hold the axis labels.
pub const GRAPH_W: i32 = 600;
pub const GRAPH_H: i32 = 260;
const GRAPH_MARGIN_LEFT: i32 = 52;
const GRAPH_MARGIN_RIGHT: i32 = 14;
const GRAPH_MARGIN_TOP: i32 = 10;
const GRAPH_MARGIN_BOTTOM: i32 = 26;

/// What graph `graph` plots from one Sample.
fn sample_value(s: &Sample, graph: usize) -> i32 {
    match graph {
        0..=2 => s.gathered[graph],
        3 => s.army,
        4 => s.population,
        5 => s.killed,
        _ => s.lost,
    }
}

/// The top of a graph's scale: the smallest of 1, 2 or 5 times a power of
/// ten that is at least `v` (and at least 1).
fn nice_ceiling(v: i32) -> i32 {
    let v = v.max(1) as i64;
    let mut p: i64 = 1;
    loop {
        for m in [1, 2, 5] {
            if p * m >= v {
                return (p * m) as i32;
            }
        }
        p *= 10;
    }
}

/// The stat an upgrade raises (a string key) and by how much.
fn upgrade_change(u: Upgrade) -> (&'static str, &'static str) {
    match u {
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
    }
}

/// The icon key of a grid button.
fn button_icon(b: Button) -> String {
    match b {
        Button::Train(s) => format!("species.{}", s.key()),
        Button::Place(k) => format!("structure.{}", k.key()),
        Button::Waypoint(Some(s)) => format!("species.{}", s.key()),
        Button::Waypoint(None) => "action.all_ants".into(),
        Button::Research(u) => format!("upgrade.{}", u.key()),
        Button::Evolve => "action.evolve".into(),
        Button::Retreat => "action.retreat".into(),
        // No picture of its own yet (render.rs): the fast scout's species.
        Button::AutoScout => "action.auto_scout".into(),
        Button::Focus(f) => format!("focus.{}", f.key()),
    }
}

/// "m:ss" from ticks.
fn clock(ticks: i32) -> String {
    format!(
        "{}:{:02}",
        ticks / TICKS_PER_SECOND / 60,
        ticks / TICKS_PER_SECOND % 60
    )
}

/// The Volume slider moves in steps of this many percent.
pub const VOLUME_STEP: u8 = 5;

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

fn map_type_key(t: MapType) -> &'static str {
    match t {
        MapType::Tunnels => "tunnels",
        MapType::Open => "open",
        MapType::Maze => "maze",
    }
}

fn biome_key(b: Biome) -> &'static str {
    match b {
        Biome::Picnic => "picnic",
        Biome::FlowerField => "flower_field",
        Biome::ForestFloor => "forest_floor",
        Biome::Riverbank => "riverbank",
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

/// A lobby seat's AI Difficulty (0 Larva, 1 Pupa, 2 Imago) as a word key.
fn net_difficulty_key(d: u8) -> &'static str {
    difficulty_key(net_difficulty(d))
}

fn net_difficulty(d: u8) -> Difficulty {
    match d {
        0 => Difficulty::Larva,
        2 => Difficulty::Imago,
        _ => Difficulty::Pupa,
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
            "forest_floor" => m.biome = Biome::ForestFloor,
            "riverbank" => m.biome = Biome::Riverbank,
            "random" => m.biome = Biome::Random,
            _ => return false,
        },
        "map_type" => match value {
            "tunnels" => m.map_type = MapType::Tunnels,
            "open" => m.map_type = MapType::Open,
            "maze" => m.map_type = MapType::Maze,
            _ => return false,
        },
        "events" => m.events = value == "on",
        "map_revealed" => m.map_revealed = value == "on",
        "sandbox" => m.sandbox = value == "on",
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
