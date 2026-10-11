//! The match simulation: colonies, ants, structures and nodes, advanced one
//! tick at a time by `World::step` from the commands every player issued for
//! that tick. Everything here is integer arithmetic over state that lives in
//! `World`, so every peer that feeds the same commands gets the same world.
//!
//! Ant behaviour lives in `ants.rs`; the AI in `ai.rs`.

use serde::{Deserialize, Serialize};

use crate::dice::{fnv, fnv_i64, Rng, FNV_START};
use crate::fixed::{cos64, sin64, Pos, CELL, TRIG_ONE};
use crate::fog::{Bits, Rays};
use crate::map::{self, Ground, Map, DIRT_HEALTH};
use crate::nav::{Nav, NO_WALL};
use crate::rules::{
    affordable, evolve_cost, times_affordable, Cost, Ranks, Resource, Species, StructureKind,
    Upgrade, POP_CAP, TICKS_PER_SECOND, WALL_MAX_CELLS, WAYPOINTS_PER_SPECIES,
};
use crate::settings::{Difficulty, MatchSettings, SeatKind};

pub type Id = u32;

/// Which resources Harvester Ants favour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    Balanced,
    Food,
    Nectar,
    Leaves,
}

impl Focus {
    pub fn weights(self) -> [i32; 3] {
        match self {
            Focus::Balanced => [2, 1, 1],
            Focus::Food => [3, 1, 1],
            Focus::Nectar => [1, 3, 1],
            Focus::Leaves => [1, 1, 3],
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Focus::Balanced => "balanced",
            Focus::Food => "food",
            Focus::Nectar => "nectar",
            Focus::Leaves => "leaves",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Amount {
    One,
    Ten,
    Max,
}

/// Everything a player (or the AI) can do. Coordinates are cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "do")]
pub enum Command {
    Train {
        species: Species,
        amount: Amount,
    },
    Research {
        upgrade: Upgrade,
    },
    Evolve,
    /// Cancel queue item `slot` of a queen or hatchery, refunding it.
    Cancel {
        structure: Id,
        slot: u32,
    },
    Place {
        kind: StructureKind,
        x: i32,
        y: i32,
        /// The far end of a wall; ignored for other kinds.
        x2: i32,
        y2: i32,
    },
    CancelBlueprint {
        id: Id,
    },
    /// `species: None` sets the waypoint for every combat species (All Ants).
    SetWaypoint {
        species: Option<Species>,
        x: i32,
        y: i32,
    },
    /// Clears the waypoint nearest (x, y), within 3 cells.
    ClearWaypoint {
        x: i32,
        y: i32,
    },
    Retreat {
        on: bool,
    },
    Focus {
        focus: Focus,
    },
    Concede,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Job {
    Idle,
    /// Gatherer: walking to, or gathering at, a node.
    Gather(Id),
    /// Gatherer: taking its inventory to a queen.
    Deliver,
    /// Builder: fetching a blueprint's cost from a queen.
    Fetch(Id),
    /// Builder: carrying to a blueprint.
    Haul(Id),
    /// Builder: building a blueprint.
    Build(Id),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Ant(Id),
    Structure(Id),
    Dirt(i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ant {
    pub id: Id,
    pub colony: u8,
    pub species: Species,
    pub pos: Pos,
    pub prev: Pos,
    pub heading: u8,
    pub health: i32,
    pub cooldown: i32,
    pub job: Job,
    pub target: Option<Target>,
    pub inventory: Cost,
    pub gather_acc: i32,
    /// Set when the ant has spent its life (an Exploding Ant that attacked).
    pub spent: bool,
}

impl Ant {
    pub fn carried(&self) -> i32 {
        self.inventory.iter().sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    Train(Species),
    Research(Upgrade, u8),
    Evolve(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItem {
    pub work: Work,
    pub progress: i32,
    pub total: i32,
    pub cost: Cost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Structure {
    pub id: Id,
    pub colony: u8,
    pub kind: StructureKind,
    pub pos: Pos,
    /// For a wall blueprint: every cell of the line. For a wall: its one cell.
    pub cells: Vec<(i32, i32)>,
    pub health: i32,
    pub max_health: i32,
    pub complete: bool,
    pub cost: Cost,
    pub delivered: Cost,
    /// Cost on its way in Carpenter Ants' inventories.
    pub promised: Cost,
    pub work: i32,
    pub work_done: i32,
    pub lane: Vec<QueueItem>,
}

impl Structure {
    pub fn radius(&self) -> i32 {
        match self.kind {
            StructureKind::Wall => {
                // A wall blueprint spans its whole line.
                if self.cells.len() > 1 {
                    CELL * self.cells.len() as i32 / 2
                } else {
                    CELL / 2
                }
            }
            k => k.stats().radius,
        }
    }

    pub fn still_needed(&self) -> Cost {
        let mut n = [0; 3];
        for (i, slot) in n.iter_mut().enumerate() {
            *slot = (self.cost[i] - self.delivered[i] - self.promised[i]).max(0);
        }
        n
    }

    pub fn fully_delivered(&self) -> bool {
        (0..3).all(|i| self.delivered[i] >= self.cost[i])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: Id,
    pub pos: Pos,
    pub resource: Resource,
    pub amount: i32,
    pub start: i32,
    pub cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Controller {
    Human,
    Ai(Difficulty),
}

/// Running totals for the end screen and the AI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Tally {
    pub trained: i32,
    pub lost: i32,
    pub killed: i32,
    pub gathered: Cost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colony {
    pub seat: u8,
    pub team: u8,
    pub controller: Controller,
    pub defeated: bool,
    pub stock: Cost,
    pub tier: u8,
    pub ranks: Ranks,
    pub focus: Focus,
    pub retreat: bool,
    pub waypoints: Vec<Vec<Pos>>,
    /// Deliveries per tick for the last 10 seconds, for Income.
    pub income_ring: Vec<Cost>,
    pub tally: Tally,
    pub ai: crate::ai::AiState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    pub tick: i32,
    pub key: String,
    /// Shown only to this colony (a notice), or to everyone if `None`.
    pub colony: Option<u8>,
    pub at: Option<Pos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hill {
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub ends: i32,
    pub hold: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Events {
    pub next_at: i32,
    pub hill: Option<Hill>,
}

/// A short-lived mark for drawing (a bite, an acid spray, a blast).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub from: Pos,
    pub to: Pos,
    pub until: i32,
    pub colony: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectKind {
    Bite,
    Spray,
    Blast,
    Heal,
}

pub const HILL_TICKS: i32 = 45 * TICKS_PER_SECOND;
pub const HILL_REWARD: Cost = [300, 150, 150];
pub const WINDFALL_AMOUNT: i32 = 400;
pub const INCOME_TICKS: usize = 100;

#[derive(Serialize, Deserialize)]
pub struct World {
    pub tick: i32,
    pub seed: u64,
    pub settings: MatchSettings,
    pub map: Map,
    pub dirt: Vec<i16>,
    pub wall: Vec<u8>,
    pub ants: Vec<Ant>,
    pub structures: Vec<Structure>,
    pub nodes: Vec<Node>,
    pub colonies: Vec<Colony>,
    pub next_id: Id,
    pub rng: Rng,
    pub events: Events,
    pub visible: Vec<Bits>,
    pub explored: Vec<Bits>,
    pub announcements: Vec<Announcement>,
    pub effects: Vec<Effect>,
    pub winner: Option<u8>,
    #[serde(skip)]
    pub nav: Nav,
    #[serde(skip)]
    pub rays: Rays,
    #[serde(skip)]
    pub grid: crate::ants::Buckets,
}

/// Why a command was refused, shown to its colony as a notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotEnough(Resource),
    NeedsTier(u8),
    NeedsHatchery(Species),
    NeedsQueen,
    Disabled,
    BadPlace,
    MaxRank,
    AlreadyQueued,
    PopCap,
    TooManyWaypoints,
    Defeated,
}

impl Refusal {
    pub fn key(&self) -> String {
        match self {
            Refusal::NotEnough(r) => format!("notice.not_enough.{}", r.key()),
            Refusal::NeedsTier(t) => format!("notice.needs_tier.{t}"),
            Refusal::NeedsHatchery(s) => format!("notice.needs_hatchery.{}", s.key()),
            Refusal::NeedsQueen => "notice.needs_queen".into(),
            Refusal::Disabled => "notice.disabled".into(),
            Refusal::BadPlace => "notice.bad_place".into(),
            Refusal::MaxRank => "notice.max_rank".into(),
            Refusal::AlreadyQueued => "notice.already_queued".into(),
            Refusal::PopCap => "notice.pop_cap".into(),
            Refusal::TooManyWaypoints => "notice.too_many_waypoints".into(),
            Refusal::Defeated => "notice.defeated".into(),
        }
    }
}

fn starting_stock(level: crate::settings::Level) -> Cost {
    match level {
        crate::settings::Level::Low => [150, 50, 100],
        crate::settings::Level::Normal => [300, 100, 200],
        crate::settings::Level::High => [600, 250, 400],
    }
}

impl World {
    /// A new match. `seats` lists who plays each seat, in map order.
    pub fn new(seed: u64, settings: MatchSettings, seats: &[SeatKind]) -> World {
        let map = map::generate(seed, &settings);
        let n = (map.w * map.h) as usize;
        let mut dirt = vec![0i16; n];
        for (i, g) in map.ground.iter().enumerate() {
            if *g == Ground::Dirt {
                dirt[i] = DIRT_HEALTH as i16;
            }
        }
        let mut w = World {
            tick: 0,
            seed,
            settings: settings.clone(),
            dirt,
            wall: vec![NO_WALL; n],
            ants: Vec::new(),
            structures: Vec::new(),
            nodes: Vec::new(),
            colonies: Vec::new(),
            next_id: 1,
            rng: Rng::new(seed),
            events: Events {
                next_at: 90 * TICKS_PER_SECOND,
                hill: None,
            },
            visible: Vec::new(),
            explored: Vec::new(),
            announcements: Vec::new(),
            effects: Vec::new(),
            winner: None,
            nav: Nav::default(),
            rays: Rays::new(),
            grid: Default::default(),
            map,
        };
        for spot in w.map.nodes.clone() {
            let id = w.new_id();
            w.nodes.push(Node {
                id,
                pos: spot.pos,
                resource: spot.resource,
                amount: spot.amount,
                start: spot.amount,
                cache: spot.cache,
            });
        }
        let seat_count = w.map.seats;
        for seat in 0..seat_count {
            let kind = seats
                .get(seat)
                .copied()
                .unwrap_or(SeatKind::Ai(Difficulty::Pupa));
            w.colonies.push(Colony {
                seat: seat as u8,
                team: settings.format.team_of(seat),
                controller: match kind {
                    SeatKind::Human => Controller::Human,
                    SeatKind::Ai(d) => Controller::Ai(d),
                },
                defeated: false,
                stock: starting_stock(settings.starting_resources),
                tier: 1,
                ranks: Ranks::default(),
                focus: Focus::Balanced,
                retreat: false,
                waypoints: vec![Vec::new(); Species::ALL.len()],
                income_ring: vec![[0; 3]; INCOME_TICKS],
                tally: Tally::default(),
                ai: Default::default(),
            });
            w.visible.push(Bits::new(n));
            w.explored.push(Bits::new(n));
            let home = w.map.home(seat).clone();
            let qpos = Pos::of_cell(home.cx, home.cy);
            let q = w.add_structure(seat as u8, StructureKind::Queen, qpos, Vec::new(), true);
            let _ = q;
            let start = [
                (Species::Harvester, 6),
                (Species::Soldier, 2),
                (Species::Carpenter, 2),
            ];
            for (s, count) in start {
                for _ in 0..count {
                    w.spawn_ant(seat as u8, s, qpos);
                }
            }
        }
        // A colony knows its own home chamber from the start.
        for seat in 0..seat_count {
            let home = w.map.home(seat).clone();
            let r = home.r + 3;
            for (dx, dy) in map::disk_offsets(r) {
                let (x, y) = (home.cx + dx, home.cy + dy);
                if w.map.inside(x, y) {
                    let i = w.map.idx(x, y);
                    w.explored[seat].set(i);
                }
            }
        }
        w.update_fog();
        w
    }

    pub fn new_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn ms_per_tick(&self) -> i64 {
        self.settings.game_speed.ms_per_tick()
    }

    // ---- lookups ----

    pub fn ant(&self, id: Id) -> Option<&Ant> {
        self.ants
            .binary_search_by_key(&id, |a| a.id)
            .ok()
            .map(|i| &self.ants[i])
    }

    pub fn ant_index(&self, id: Id) -> Option<usize> {
        self.ants.binary_search_by_key(&id, |a| a.id).ok()
    }

    pub fn structure(&self, id: Id) -> Option<&Structure> {
        self.structures
            .binary_search_by_key(&id, |s| s.id)
            .ok()
            .map(|i| &self.structures[i])
    }

    pub fn structure_index(&self, id: Id) -> Option<usize> {
        self.structures.binary_search_by_key(&id, |s| s.id).ok()
    }

    pub fn node_index(&self, id: Id) -> Option<usize> {
        self.nodes.binary_search_by_key(&id, |n| n.id).ok()
    }

    pub fn allied(&self, a: u8, b: u8) -> bool {
        self.colonies[a as usize].team == self.colonies[b as usize].team
    }

    pub fn population(&self, colony: u8) -> i32 {
        self.ants
            .iter()
            .filter(|a| a.colony == colony)
            .map(|a| a.species.stats().pop)
            .sum()
    }

    /// Population already paid for in queues.
    pub fn queued_pop(&self, colony: u8) -> i32 {
        self.structures
            .iter()
            .filter(|s| s.colony == colony)
            .flat_map(|s| s.lane.iter())
            .map(|q| match q.work {
                Work::Train(sp) => sp.stats().pop,
                _ => 0,
            })
            .sum()
    }

    pub fn housing(&self, colony: u8) -> i32 {
        self.structures
            .iter()
            .filter(|s| s.colony == colony && s.complete)
            .map(|s| s.kind.stats().housing)
            .sum::<i32>()
            .min(POP_CAP)
    }

    pub fn queens(&self, colony: u8) -> impl Iterator<Item = &Structure> {
        self.structures
            .iter()
            .filter(move |s| s.colony == colony && s.complete && s.kind == StructureKind::Queen)
    }

    pub fn nearest_queen(&self, colony: u8, p: Pos) -> Option<&Structure> {
        self.queens(colony).min_by_key(|q| (q.pos.dist2(p), q.id))
    }

    pub fn has_hatchery(&self, colony: u8, s: Species) -> bool {
        self.structures
            .iter()
            .any(|st| st.colony == colony && st.complete && st.kind == StructureKind::Hatchery(s))
    }

    /// Income per second for each resource, in tenths.
    pub fn income_tenths(&self, colony: u8) -> Cost {
        let mut sum = [0; 3];
        for c in &self.colonies[colony as usize].income_ring {
            for i in 0..3 {
                sum[i] += c[i];
            }
        }
        // Sum over 10 seconds is the same number as tenths per second.
        sum
    }

    // ---- creation ----

    pub fn spawn_ant(&mut self, colony: u8, species: Species, at: Pos) -> Id {
        let id = self.new_id();
        let st = self.colonies[colony as usize].ranks.apply(species);
        let a = ((id as usize) * 13) % 64;
        let r = CELL * 2 + (id as i32 % 5) * CELL / 2;
        let mut pos = Pos::new(
            at.x + r * cos64(a) / TRIG_ONE,
            at.y + r * sin64(a) / TRIG_ONE,
        );
        let (cx, cy) = pos.cell();
        if !self.map.is_floor(cx, cy) {
            pos = at;
        }
        self.ants.push(Ant {
            id,
            colony,
            species,
            pos,
            prev: pos,
            heading: (a / 4) as u8,
            health: st.health,
            cooldown: 0,
            job: Job::Idle,
            target: None,
            inventory: [0; 3],
            gather_acc: 0,
            spent: false,
        });
        id
    }

    pub fn add_structure(
        &mut self,
        colony: u8,
        kind: StructureKind,
        pos: Pos,
        cells: Vec<(i32, i32)>,
        complete: bool,
    ) -> Id {
        let id = self.new_id();
        let st = kind.stats();
        let n = cells.len().max(1) as i32;
        let cost = if kind == StructureKind::Wall {
            [st.cost[0] * n, st.cost[1] * n, st.cost[2] * n]
        } else {
            st.cost
        };
        let health = if complete {
            st.health
        } else {
            st.health / 10 + 1
        };
        self.structures.push(Structure {
            id,
            colony,
            kind,
            pos,
            cells,
            health: health.max(1),
            max_health: st.health,
            complete,
            cost,
            delivered: if complete { cost } else { [0; 3] },
            promised: [0; 3],
            work: st.work * n,
            work_done: if complete { st.work * n } else { 0 },
            lane: Vec::new(),
        });
        if complete && kind == StructureKind::Wall {
            for &(x, y) in &self.structures.last().expect("pushed").cells.clone() {
                let i = self.map.idx(x, y);
                self.wall[i] = colony;
            }
            self.nav.invalidate();
        }
        id
    }

    pub fn announce(&mut self, key: &str, colony: Option<u8>, at: Option<Pos>) {
        self.announcements.push(Announcement {
            tick: self.tick,
            key: key.into(),
            colony,
            at,
        });
        if self.announcements.len() > 40 {
            self.announcements.remove(0);
        }
    }

    // ---- the tick ----

    /// Advances one tick. `commands` are (seat, command) in seat order;
    /// `dropped` lists seats whose player left, now handed to the AI.
    pub fn step(&mut self, commands: &[(u8, Command)], dropped: &[u8]) {
        self.tick += 1;
        for &seat in dropped {
            if let Some(c) = self.colonies.get_mut(seat as usize) {
                if c.controller == Controller::Human {
                    c.controller = Controller::Ai(Difficulty::Pupa);
                    self.announce("announce.player_left", None, None);
                }
            }
        }
        for (seat, cmd) in commands {
            self.apply(*seat, cmd);
        }
        for c in 0..self.colonies.len() {
            if let Controller::Ai(d) = self.colonies[c].controller {
                if !self.colonies[c].defeated && crate::ai::due(self.tick, c, d) {
                    let cmds = crate::ai::decide(self, c as u8, d);
                    for cmd in cmds {
                        self.apply(c as u8, &cmd);
                    }
                }
            }
        }
        let slot = self.tick as usize % INCOME_TICKS;
        for c in self.colonies.iter_mut() {
            c.income_ring[slot] = [0; 3];
        }
        self.run_lanes();
        self.run_events();
        self.run_ants();
        if self.tick % 2 == 0 {
            self.update_fog();
        }
        self.effects.retain(|e| e.until > self.tick);
        self.check_victory();
    }

    /// Applies one command for a colony; refusals become notices.
    pub fn apply(&mut self, colony: u8, cmd: &Command) {
        if let Err(r) = self.try_apply(colony, cmd) {
            self.announce(&r.key(), Some(colony), None);
        }
    }

    pub fn try_apply(&mut self, colony: u8, cmd: &Command) -> Result<(), Refusal> {
        let ci = colony as usize;
        if ci >= self.colonies.len() || self.colonies[ci].defeated {
            return Err(Refusal::Defeated);
        }
        match cmd {
            Command::Train { species, amount } => self.train(colony, *species, *amount),
            Command::Research { upgrade } => self.research(colony, *upgrade),
            Command::Evolve => self.evolve(colony),
            Command::Cancel { structure, slot } => {
                let si = self.structure_index(*structure).ok_or(Refusal::BadPlace)?;
                let s = &mut self.structures[si];
                if s.colony != colony || *slot as usize >= s.lane.len() {
                    return Err(Refusal::BadPlace);
                }
                let item = s.lane.remove(*slot as usize);
                for i in 0..3 {
                    self.colonies[ci].stock[i] += item.cost[i];
                }
                Ok(())
            }
            Command::Place { kind, x, y, x2, y2 } => {
                self.place(colony, *kind, *x, *y, *x2, *y2).map(|_| ())
            }
            Command::CancelBlueprint { id } => {
                let si = self.structure_index(*id).ok_or(Refusal::BadPlace)?;
                let s = &self.structures[si];
                if s.colony != colony || s.complete {
                    return Err(Refusal::BadPlace);
                }
                let cost = s.cost;
                self.structures.remove(si);
                for i in 0..3 {
                    self.colonies[ci].stock[i] += cost[i];
                }
                for a in self.ants.iter_mut() {
                    if a.colony == colony
                        && matches!(a.job, Job::Fetch(b) | Job::Haul(b) | Job::Build(b) if b == *id)
                    {
                        a.job = Job::Idle;
                        a.inventory = [0; 3];
                    }
                }
                Ok(())
            }
            Command::SetWaypoint { species, x, y } => {
                // Anywhere on the map, even unexplored: the waypoint snaps
                // to the nearest open ground within 6 cells.
                let (x, y) = self.nearest_open(*x, *y, 6).ok_or(Refusal::BadPlace)?;
                let p = Pos::of_cell(x, y);
                let list: Vec<Species> = match species {
                    Some(s) => vec![*s],
                    None => Species::ALL.into_iter().filter(|s| s.is_combat()).collect(),
                };
                for s in list {
                    if s.is_worker() {
                        return Err(Refusal::BadPlace);
                    }
                    let wps = &mut self.colonies[ci].waypoints[s.index()];
                    if species.is_none() {
                        // All Ants replaces every combat species' waypoints.
                        wps.clear();
                    }
                    if wps.len() >= WAYPOINTS_PER_SPECIES {
                        return Err(Refusal::TooManyWaypoints);
                    }
                    wps.push(p);
                }
                Ok(())
            }
            Command::ClearWaypoint { x, y } => {
                let p = Pos::of_cell(*x, *y);
                let mut best: Option<(i64, usize, usize)> = None;
                for (si, wps) in self.colonies[ci].waypoints.iter().enumerate() {
                    for (wi, w) in wps.iter().enumerate() {
                        let d = w.dist2(p);
                        if d <= (3 * CELL as i64).pow(2) && best.map(|b| d < b.0).unwrap_or(true) {
                            best = Some((d, si, wi));
                        }
                    }
                }
                let (_, si, wi) = best.ok_or(Refusal::BadPlace)?;
                let at = self.colonies[ci].waypoints[si][wi];
                // Every species with a waypoint on that spot loses it.
                for wps in self.colonies[ci].waypoints.iter_mut() {
                    wps.retain(|w| *w != at);
                }
                Ok(())
            }
            Command::Retreat { on } => {
                self.colonies[ci].retreat = *on;
                Ok(())
            }
            Command::Focus { focus } => {
                self.colonies[ci].focus = *focus;
                Ok(())
            }
            Command::Concede => {
                self.defeat(colony);
                Ok(())
            }
        }
    }

    /// The nearest floor or dirt cell to (x, y) within `r` cells.
    pub fn nearest_open(&self, x: i32, y: i32, r: i32) -> Option<(i32, i32)> {
        let mut best: Option<(i32, i32, i32)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let d = dx * dx + dy * dy;
                if d <= r * r
                    && self.map.at(x + dx, y + dy) != Ground::Rock
                    && best.map(|b| d < b.0).unwrap_or(true)
                {
                    best = Some((d, x + dx, y + dy));
                }
            }
        }
        best.map(|b| (b.1, b.2))
    }

    fn pay(&mut self, colony: u8, cost: &Cost) -> Result<(), Refusal> {
        let stock = &mut self.colonies[colony as usize].stock;
        for r in Resource::ALL {
            if stock[r.index()] < cost[r.index()] {
                return Err(Refusal::NotEnough(r));
            }
        }
        for i in 0..3 {
            stock[i] -= cost[i];
        }
        Ok(())
    }

    /// The least-busy complete producer for a piece of work.
    fn producer(&self, colony: u8, hatchery_for: Option<Species>) -> Option<usize> {
        self.structures
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.colony == colony
                    && s.complete
                    && match hatchery_for {
                        Some(sp) => s.kind == StructureKind::Hatchery(sp),
                        None => s.kind == StructureKind::Queen,
                    }
            })
            .min_by_key(|(_, s)| {
                (
                    s.lane.iter().map(|q| q.total - q.progress).sum::<i32>(),
                    s.id,
                )
            })
            .map(|(i, _)| i)
    }

    pub fn can_train(&self, colony: u8, s: Species) -> Result<(), Refusal> {
        let c = &self.colonies[colony as usize];
        if self.settings.is_disabled(s) {
            return Err(Refusal::Disabled);
        }
        if c.tier < s.stats().tier {
            return Err(Refusal::NeedsTier(s.stats().tier));
        }
        if s.from_queen() {
            if self.queens(colony).next().is_none() {
                return Err(Refusal::NeedsQueen);
            }
        } else if !self.has_hatchery(colony, s) {
            return Err(Refusal::NeedsHatchery(s));
        }
        Ok(())
    }

    fn train(&mut self, colony: u8, s: Species, amount: Amount) -> Result<(), Refusal> {
        self.can_train(colony, s)?;
        let st = s.stats();
        let stock = self.colonies[colony as usize].stock;
        let room = (POP_CAP - self.population(colony) - self.queued_pop(colony)) / st.pop;
        if room <= 0 {
            return Err(Refusal::PopCap);
        }
        let can = times_affordable(&stock, &st.cost);
        if can <= 0 {
            let short = Resource::ALL
                .into_iter()
                .find(|r| stock[r.index()] < st.cost[r.index()])
                .unwrap_or(Resource::Food);
            return Err(Refusal::NotEnough(short));
        }
        let want = match amount {
            Amount::One => 1,
            Amount::Ten => 10,
            Amount::Max => i32::MAX,
        };
        let n = want.min(can).min(room);
        let hatch = if s.from_queen() { None } else { Some(s) };
        for _ in 0..n {
            let pi = self.producer(colony, hatch).ok_or(Refusal::NeedsQueen)?;
            self.pay(colony, &st.cost)?;
            self.structures[pi].lane.push(QueueItem {
                work: Work::Train(s),
                progress: 0,
                total: st.train_ticks,
                cost: st.cost,
            });
        }
        Ok(())
    }

    /// The next rank of an upgrade a colony could queue, counting queued ones.
    pub fn next_rank(&self, colony: u8, u: Upgrade) -> u8 {
        let queued = self
            .structures
            .iter()
            .filter(|s| s.colony == colony)
            .flat_map(|s| s.lane.iter())
            .filter(|q| matches!(q.work, Work::Research(x, _) if x == u))
            .count() as u8;
        self.colonies[colony as usize].ranks.get(u) + queued + 1
    }

    pub fn can_research(&self, colony: u8, u: Upgrade) -> Result<u8, Refusal> {
        let rank = self.next_rank(colony, u);
        if rank > u.max_rank() {
            return Err(Refusal::MaxRank);
        }
        if rank > self.colonies[colony as usize].ranks.get(u) + 1 {
            return Err(Refusal::AlreadyQueued);
        }
        let need = u.tier_for(rank);
        if self.colonies[colony as usize].tier < need {
            return Err(Refusal::NeedsTier(need));
        }
        if let Upgrade::Species(s) = u {
            if self.settings.is_disabled(s) {
                return Err(Refusal::Disabled);
            }
        }
        match u.at_hatchery() {
            Some(s) if !self.has_hatchery(colony, s) => Err(Refusal::NeedsHatchery(s)),
            None if self.queens(colony).next().is_none() => Err(Refusal::NeedsQueen),
            _ => Ok(rank),
        }
    }

    fn research(&mut self, colony: u8, u: Upgrade) -> Result<(), Refusal> {
        let rank = self.can_research(colony, u)?;
        let cost = u.cost(rank);
        let pi = self
            .producer(colony, u.at_hatchery())
            .ok_or(Refusal::NeedsQueen)?;
        self.pay(colony, &cost)?;
        self.structures[pi].lane.push(QueueItem {
            work: Work::Research(u, rank),
            progress: 0,
            total: u.research_ticks(rank),
            cost,
        });
        Ok(())
    }

    pub fn evolving(&self, colony: u8) -> bool {
        self.structures
            .iter()
            .filter(|s| s.colony == colony)
            .flat_map(|s| s.lane.iter())
            .any(|q| matches!(q.work, Work::Evolve(_)))
    }

    pub fn can_evolve(&self, colony: u8) -> Result<u8, Refusal> {
        let tier = self.colonies[colony as usize].tier;
        if tier >= 3 {
            return Err(Refusal::MaxRank);
        }
        if self.evolving(colony) {
            return Err(Refusal::AlreadyQueued);
        }
        if self.queens(colony).next().is_none() {
            return Err(Refusal::NeedsQueen);
        }
        Ok(tier + 1)
    }

    fn evolve(&mut self, colony: u8) -> Result<(), Refusal> {
        let to = self.can_evolve(colony)?;
        let (cost, ticks) = evolve_cost(to);
        let pi = self.producer(colony, None).ok_or(Refusal::NeedsQueen)?;
        self.pay(colony, &cost)?;
        self.structures[pi].lane.push(QueueItem {
            work: Work::Evolve(to),
            progress: 0,
            total: ticks,
            cost,
        });
        Ok(())
    }

    // ---- placement ----

    /// The cells a wall from (x, y) to (x2, y2) covers, at most
    /// `WALL_MAX_CELLS` long.
    pub fn wall_cells(x: i32, y: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
        let mut cells = crate::fog::line(x, y, x2, y2);
        cells.truncate(WALL_MAX_CELLS as usize);
        cells
    }

    /// Whether a structure may be placed; used for the placement ghost too.
    pub fn check_place(
        &self,
        colony: u8,
        kind: StructureKind,
        x: i32,
        y: i32,
        x2: i32,
        y2: i32,
    ) -> Result<(), Refusal> {
        let ci = colony as usize;
        let c = &self.colonies[ci];
        if c.tier < kind.stats().tier {
            return Err(Refusal::NeedsTier(kind.stats().tier));
        }
        if let StructureKind::Hatchery(s) = kind {
            if self.settings.is_disabled(s) {
                return Err(Refusal::Disabled);
            }
        }
        let cells: Vec<(i32, i32)> = if kind == StructureKind::Wall {
            World::wall_cells(x, y, x2, y2)
        } else {
            let r = kind.stats().radius / CELL;
            map::disk_offsets(r)
                .into_iter()
                .map(|(dx, dy)| (x + dx, y + dy))
                .collect()
        };
        for &(cx, cy) in &cells {
            if !self.map.is_floor(cx, cy)
                || self.wall[self.map.idx(cx, cy)] != NO_WALL
                || !self.explored[ci].get(self.map.idx(cx, cy))
            {
                return Err(Refusal::BadPlace);
            }
        }
        let centre = if kind == StructureKind::Wall {
            Pos::of_cell((x + x2) / 2, (y + y2) / 2)
        } else {
            Pos::of_cell(x, y)
        };
        let my_r = if kind == StructureKind::Wall {
            CELL / 2
        } else {
            kind.stats().radius
        };
        // Clear of other structures and of nodes.
        for s in &self.structures {
            let blocks = if s.kind == StructureKind::Wall || kind == StructureKind::Wall {
                let mine: Vec<(i32, i32)> = if kind == StructureKind::Wall {
                    cells.clone()
                } else {
                    vec![(x, y)]
                };
                let theirs: Vec<(i32, i32)> = if s.kind == StructureKind::Wall {
                    s.cells.clone()
                } else {
                    vec![s.pos.cell()]
                };
                let reach = if kind == StructureKind::Wall && s.kind == StructureKind::Wall {
                    0
                } else {
                    let other = if s.kind == StructureKind::Wall {
                        my_r
                    } else {
                        s.radius()
                    };
                    other / CELL + 1
                };
                mine.iter().any(|a| {
                    theirs
                        .iter()
                        .any(|b| (a.0 - b.0).abs() <= reach && (a.1 - b.1).abs() <= reach)
                })
            } else {
                s.pos.dist(centre) < s.radius() + my_r + CELL / 2
            };
            if blocks {
                return Err(Refusal::BadPlace);
            }
        }
        for n in &self.nodes {
            if n.amount > 0
                && cells
                    .iter()
                    .any(|&(cx, cy)| n.pos.dist(Pos::of_cell(cx, cy)) < CELL * 2)
            {
                return Err(Refusal::BadPlace);
            }
        }
        match kind {
            StructureKind::Queen => {
                if self.map.chamber_at(x, y).is_none() {
                    return Err(Refusal::BadPlace);
                }
                let too_close = self
                    .structures
                    .iter()
                    .any(|s| s.kind == StructureKind::Queen && s.pos.dist(centre) < 14 * CELL);
                if too_close {
                    return Err(Refusal::BadPlace);
                }
            }
            StructureKind::Wall => {}
            _ => {
                let near = self.structures.iter().any(|s| {
                    s.colony == colony
                        && s.kind == StructureKind::Queen
                        && s.pos.dist(centre) <= 16 * CELL
                });
                if !near {
                    return Err(Refusal::NeedsQueen);
                }
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        colony: u8,
        kind: StructureKind,
        x: i32,
        y: i32,
        x2: i32,
        y2: i32,
    ) -> Result<Id, Refusal> {
        self.check_place(colony, kind, x, y, x2, y2)?;
        let (cells, pos) = if kind == StructureKind::Wall {
            let cells = World::wall_cells(x, y, x2, y2);
            let (sx, sy) = cells.iter().fold((0, 0), |a, c| (a.0 + c.0, a.1 + c.1));
            let n = cells.len() as i32;
            let pos = Pos::new(sx * CELL / n + CELL / 2, sy * CELL / n + CELL / 2);
            (cells, pos)
        } else {
            (Vec::new(), Pos::of_cell(x, y))
        };
        let n = cells.len().max(1) as i32;
        let base = kind.stats().cost;
        let cost = if kind == StructureKind::Wall {
            [base[0] * n, base[1] * n, base[2] * n]
        } else {
            base
        };
        self.pay(colony, &cost)?;
        Ok(self.add_structure(colony, kind, pos, cells, false))
    }

    /// Turns a finished blueprint into its structure.
    pub fn complete(&mut self, si: usize) {
        let s = &mut self.structures[si];
        s.complete = true;
        s.health = s.max_health;
        if s.kind == StructureKind::Wall {
            // One wall structure per cell, so a hole can be broken.
            let s = self.structures.remove(si);
            for (x, y) in s.cells {
                let id = self.add_structure(
                    s.colony,
                    StructureKind::Wall,
                    Pos::of_cell(x, y),
                    vec![(x, y)],
                    true,
                );
                let _ = id;
            }
            self.structures.sort_by_key(|s| s.id);
        }
    }

    // ---- production ----

    fn run_lanes(&mut self) {
        for si in 0..self.structures.len() {
            if !self.structures[si].complete || self.structures[si].lane.is_empty() {
                continue;
            }
            let colony = self.structures[si].colony;
            let work = self.structures[si].lane[0].work;
            if let Work::Train(sp) = work {
                if self.population(colony) + sp.stats().pop > self.housing(colony) {
                    continue; // Housing Full
                }
            }
            let item = &mut self.structures[si].lane[0];
            item.progress += 1;
            if item.progress < item.total {
                continue;
            }
            self.structures[si].lane.remove(0);
            let at = self.structures[si].pos;
            let ci = colony as usize;
            match work {
                Work::Train(sp) => {
                    self.spawn_ant(colony, sp, at);
                    self.colonies[ci].tally.trained += 1;
                }
                Work::Research(u, _) => {
                    let before: Vec<i32> = Species::ALL
                        .iter()
                        .map(|s| self.colonies[ci].ranks.apply(*s).health)
                        .collect();
                    self.colonies[ci].ranks.raise(u);
                    let after: Vec<i32> = Species::ALL
                        .iter()
                        .map(|s| self.colonies[ci].ranks.apply(*s).health)
                        .collect();
                    for a in self.ants.iter_mut().filter(|a| a.colony == colony) {
                        a.health += after[a.species.index()] - before[a.species.index()];
                    }
                    self.announce(&format!("notice.upgraded.{}", u.key()), Some(colony), None);
                }
                Work::Evolve(t) => {
                    self.colonies[ci].tier = t;
                    self.announce(&format!("notice.evolved.{t}"), Some(colony), None);
                }
            }
        }
    }

    // ---- events ----

    fn run_events(&mut self) {
        if let Some(h) = &mut self.events.hill {
            let mut present: Vec<u8> = Vec::new();
            let (hx, hy, hr) = (h.x, h.y, h.r);
            for a in &self.ants {
                if a.species.is_combat() {
                    let (cx, cy) = a.pos.cell();
                    if (cx - hx).pow(2) + (cy - hy).pow(2) <= hr * hr {
                        let team = self.colonies[a.colony as usize].team;
                        if !present.contains(&team) {
                            present.push(team);
                        }
                    }
                }
            }
            let h = self.events.hill.as_mut().expect("checked");
            if present.len() == 1 {
                h.hold[present[0] as usize] += 1;
            }
            if self.tick >= h.ends {
                let hold = h.hold.clone();
                self.events.hill = None;
                let best = hold.iter().copied().max().unwrap_or(0);
                let winners: Vec<usize> = (0..hold.len()).filter(|t| hold[*t] == best).collect();
                if best > 0 && winners.len() == 1 {
                    let team = winners[0] as u8;
                    for c in self.colonies.iter_mut() {
                        if c.team == team && !c.defeated {
                            for i in 0..3 {
                                c.stock[i] += HILL_REWARD[i];
                            }
                        }
                    }
                    self.announce(&format!("announce.hill_won.{team}"), None, None);
                } else {
                    self.announce("announce.hill_ended", None, None);
                }
            }
        }
        if !self.settings.events || self.tick < self.events.next_at {
            return;
        }
        self.events.next_at = self.tick + self.rng.range(60, 90) * TICKS_PER_SECOND;
        let hill_free = self.events.hill.is_none();
        if hill_free && self.rng.chance(50) {
            let c = &self.map.chambers[self.map.centre];
            let teams = self.colonies.iter().map(|c| c.team).max().unwrap_or(0) as usize + 1;
            self.events.hill = Some(Hill {
                x: c.cx,
                y: c.cy,
                r: c.r,
                ends: self.tick + HILL_TICKS,
                hold: vec![0; teams],
            });
            let at = Pos::of_cell(c.cx, c.cy);
            self.announce("announce.hill", None, Some(at));
        } else {
            // A Windfall in one chamber and its rotations: fair to every seat.
            let seats = self.map.seats;
            let per = (self.map.chambers.len() - 1) / seats;
            let candidates: Vec<usize> = (0..per)
                .filter(|i| self.map.chambers[*i].home.is_none())
                .collect();
            if candidates.is_empty() {
                return;
            }
            let pick = candidates[self.rng.below(candidates.len() as u32) as usize];
            let res = Resource::ALL[self.rng.below(3) as usize];
            for t in 0..seats {
                let c = self.map.chambers[pick + t * per].clone();
                let id = self.new_id();
                let pos = Pos::of_cell(c.cx, c.cy);
                self.nodes.push(Node {
                    id,
                    pos,
                    resource: res,
                    amount: WINDFALL_AMOUNT,
                    start: WINDFALL_AMOUNT,
                    cache: true,
                });
                self.announce(&format!("announce.windfall.{}", res.key()), None, Some(pos));
            }
        }
    }

    // ---- fog ----

    pub fn update_fog(&mut self) {
        let (w, h) = (self.map.w, self.map.h);
        let ground = &self.map.ground;
        let wall = &self.wall;
        let opaque = |i: usize| ground[i] != Ground::Floor || wall[i] != NO_WALL;
        for c in 0..self.colonies.len() {
            let mut sources: Vec<(i32, i32, i32)> = Vec::new();
            let ranks = &self.colonies[c].ranks;
            for a in self.ants.iter().filter(|a| a.colony as usize == c) {
                let (x, y) = a.pos.cell();
                sources.push((x, y, ranks.apply(a.species).vision));
            }
            for s in self.structures.iter().filter(|s| s.colony as usize == c) {
                let (x, y) = s.pos.cell();
                let v = if s.complete { s.kind.stats().vision } else { 3 };
                sources.push((x, y, v));
            }
            sources.sort();
            sources.dedup();
            let vis = &mut self.visible[c];
            vis.clear_all();
            for (x, y, r) in sources {
                self.rays.cast(w, h, x, y, r, &opaque, vis);
            }
            self.explored[c].union_with(&self.visible[c]);
        }
    }

    /// What a colony and its allies see right now.
    pub fn team_visible(&self, colony: u8) -> Bits {
        let mut v = self.visible[colony as usize].clone();
        for (i, c) in self.colonies.iter().enumerate() {
            if i != colony as usize && c.team == self.colonies[colony as usize].team {
                v.union_with(&self.visible[i]);
            }
        }
        v
    }

    pub fn team_explored(&self, colony: u8) -> Bits {
        let mut v = self.explored[colony as usize].clone();
        for (i, c) in self.colonies.iter().enumerate() {
            if i != colony as usize && c.team == self.colonies[colony as usize].team {
                v.union_with(&self.explored[i]);
            }
        }
        v
    }

    // ---- defeat and victory ----

    pub fn defeat(&mut self, colony: u8) {
        let ci = colony as usize;
        if self.colonies[ci].defeated {
            return;
        }
        self.colonies[ci].defeated = true;
        self.ants.retain(|a| a.colony != colony);
        let had_walls = self
            .structures
            .iter()
            .any(|s| s.colony == colony && s.kind == StructureKind::Wall && s.complete);
        self.structures.retain(|s| s.colony != colony);
        if had_walls {
            for w in self.wall.iter_mut() {
                if *w == colony {
                    *w = NO_WALL;
                }
            }
            self.nav.invalidate();
        }
        self.announce(&format!("announce.defeated.{colony}"), None, None);
    }

    fn check_victory(&mut self) {
        for c in 0..self.colonies.len() as u8 {
            if self.colonies[c as usize].defeated {
                continue;
            }
            let alive = self.ants.iter().any(|a| a.colony == c)
                || self
                    .structures
                    .iter()
                    .any(|s| s.colony == c && s.complete && s.kind.makes_ants());
            if !alive {
                self.defeat(c);
            }
        }
        if self.winner.is_some() {
            return;
        }
        let mut teams: Vec<u8> = self
            .colonies
            .iter()
            .filter(|c| !c.defeated)
            .map(|c| c.team)
            .collect();
        teams.sort();
        teams.dedup();
        if teams.len() == 1 && self.colonies.len() > 1 {
            self.winner = Some(teams[0]);
        } else if teams.is_empty() {
            self.winner = Some(u8::MAX);
        }
    }

    // ---- checksum and snapshot ----

    /// A compact digest of the state that matters: every peer must agree.
    pub fn checksum(&self) -> u64 {
        let mut h = fnv_i64(FNV_START, self.tick as i64);
        h = fnv_i64(h, self.rng.state() as i64);
        for a in &self.ants {
            h = fnv_i64(h, a.id as i64);
            h = fnv_i64(h, a.pos.x as i64);
            h = fnv_i64(h, a.pos.y as i64);
            h = fnv_i64(h, a.health as i64);
        }
        for s in &self.structures {
            h = fnv_i64(h, s.id as i64);
            h = fnv_i64(h, s.health as i64);
            h = fnv_i64(h, s.work_done as i64);
            h = fnv_i64(h, s.lane.len() as i64);
        }
        for n in &self.nodes {
            h = fnv_i64(h, n.amount as i64);
        }
        for c in &self.colonies {
            for v in c.stock {
                h = fnv_i64(h, v as i64);
            }
            h = fnv(h, &[c.tier, c.defeated as u8]);
        }
        h
    }

    pub fn snapshot(&self) -> String {
        serde_json::to_string(self).expect("the world serialises")
    }

    pub fn restore(json: &str) -> Result<World, String> {
        let mut w: World = serde_json::from_str(json).map_err(|e| e.to_string())?;
        w.rays = Rays::new();
        w.nav = Nav::default();
        Ok(w)
    }

    /// Whether `affordable` holds for a colony and cost (for the interface).
    pub fn can_pay(&self, colony: u8, cost: &Cost) -> bool {
        affordable(&self.colonies[colony as usize].stock, cost)
    }
}
