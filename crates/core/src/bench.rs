//! The Benchmark (PLAN decision 25): the performance-test scenario running
//! live, so the person can see what the budget means.
//!
//! The core cannot read a clock, so the page passes timestamps in
//! (microseconds since the page loaded) and this module does all the
//! arithmetic: how many ticks are due, how long each tick and each frame
//! took, and the panel's label–value text.

use crate::fixed::Pos;
use crate::render::{self, Camera, Overlay, TERRAIN_SCALE};
use crate::rules::Species;
use crate::settings::{Biome, Format, MatchSettings, SeatKind};
use crate::world::{Command, World};

/// The scenario's map seed, the same as `tests/perf.rs`.
pub const BENCH_SEED: u64 = 31;
/// Ticks kept for the tick average and worst.
const TICK_WINDOW: usize = 50;
/// Frames kept for the draw average.
const DRAW_WINDOW: usize = 120;
/// Ticks that may pile up behind a slow frame before they are dropped.
const MAX_BACKLOG: i64 = 3;
/// The budget: a tick under 8 ms and 60 frames per second (57 or more
/// counts, since the browser's frame clock jitters).
pub const TICK_BUDGET_US: i64 = 8_000;
pub const FPS_BUDGET: i64 = 60;
const FPS_FLOOR: i64 = 57;
/// When fighting has left fewer ants than this, the scenario starts again.
const RESTART_BELOW: usize = 400;

/// Four colonies of about 300 ants each at Tier 3, mixed species, every
/// combat species sent to the centre chamber with All Ants.
pub fn bench_world(seed: u64, biome: Biome) -> World {
    let s = MatchSettings {
        format: Format::FreeForAll,
        events: false,
        biome,
        ..MatchSettings::default()
    };
    let mut w = World::new(seed, s, &[SeatKind::Human; 4]);
    let mix = [
        (Species::Harvester, 40),
        (Species::Soldier, 120),
        (Species::Acid, 40),
        (Species::Turtle, 30),
        (Species::Fire, 50),
        (Species::Flying, 20),
    ];
    for seat in 0..4u8 {
        w.colonies[seat as usize].tier = 3;
        let h = w.map.home(seat as usize).clone();
        for (sp, n) in mix {
            for _ in 0..n {
                w.spawn_ant(seat, sp, Pos::of_cell(h.cx, h.cy));
            }
        }
    }
    let c = w.map.chambers[w.map.centre].clone();
    let cmds: Vec<(u8, Command)> = (0..4u8)
        .map(|seat| {
            (
                seat,
                Command::SetWaypoint {
                    species: None,
                    x: c.cx,
                    y: c.cy,
                    add: false,
                },
            )
        })
        .collect();
    w.step(&cmds, &[]);
    w
}

pub struct Bench {
    pub world: World,
    pub camera: Camera,
    /// The last frame's timestamp.
    clock: Option<i64>,
    /// Simulated time owed, in microseconds.
    owed: i64,
    /// Timestamps of the frames in the last second.
    frames: Vec<i64>,
    ticks: Vec<i64>,
    draws: Vec<i64>,
    drag: Option<(i32, i32)>,
    pub restarts: u32,
    biome: Biome,
    /// Whose Fog of War to draw; `None` shows everything.
    viewer: Option<u8>,
    /// `?gallery`: draw the species gallery instead of the scenario.
    pub gallery: bool,
}

/// Whether the page asked for the species gallery (`?gallery`): every
/// species side by side at the zooms players use, for review.
pub fn wants_gallery(query: &str) -> bool {
    query
        .trim_start_matches('?')
        .split('&')
        .any(|p| p == "gallery")
}

/// Benchmark options from the page's query string: `biome=flower_field`
/// (or `picnic`), and `viewer=0`…`3` to draw that colony's Fog of War.
pub fn parse_options(query: &str) -> (Biome, Option<u8>) {
    let mut biome = Biome::Picnic;
    let mut viewer = None;
    for pair in query.trim_start_matches('?').split('&') {
        match pair.split_once('=') {
            Some(("biome", "flower_field")) => biome = Biome::FlowerField,
            Some(("biome", "picnic")) => biome = Biome::Picnic,
            Some(("viewer", v)) => viewer = v.parse::<u8>().ok().filter(|v| *v < 4),
            _ => {}
        }
    }
    (biome, viewer)
}

fn push_window(v: &mut Vec<i64>, x: i64, n: usize) {
    v.push(x);
    if v.len() > n {
        v.remove(0);
    }
}

/// Microseconds as milliseconds with two decimals.
fn ms(us: i64) -> String {
    let us = us.max(0);
    format!("{}.{:02} ms", us / 1000, us % 1000 / 10)
}

impl Bench {
    pub fn new(view_w: i32, view_h: i32, query: &str) -> Bench {
        let (biome, viewer) = parse_options(query);
        let world = bench_world(BENCH_SEED, biome);
        let mut camera = Camera::new(view_w, view_h);
        camera.fit(world.map.w, world.map.h);
        Bench {
            world,
            camera,
            clock: None,
            owed: 0,
            frames: Vec::new(),
            ticks: Vec::new(),
            draws: Vec::new(),
            drag: None,
            restarts: 0,
            biome,
            viewer,
            gallery: wants_gallery(query),
        }
    }

    /// Fog of War at one pixel per cell: (width, height) and RGBA bytes.
    pub fn fog_size(&self) -> Vec<i32> {
        vec![self.world.map.w, self.world.map.h]
    }

    pub fn fog(&self) -> Vec<u8> {
        render::fog_image(&self.world, self.viewer)
    }

    fn tick_us(&self) -> i64 {
        self.world.ms_per_tick() * 1000
    }

    pub fn terrain_size(&self) -> Vec<i32> {
        vec![
            self.world.map.w * TERRAIN_SCALE,
            self.world.map.h * TERRAIN_SCALE,
        ]
    }

    pub fn terrain(&self) -> Vec<u8> {
        render::terrain_image(&self.world, TERRAIN_SCALE).2
    }

    pub fn terrain_stamp(&self) -> u32 {
        render::terrain_stamp(&self.world) + self.restarts
    }

    /// Fits the camera to a new view size.
    pub fn resize(&mut self, view_w: i32, view_h: i32) {
        self.camera.resize(view_w, view_h);
        self.camera.fit(self.world.map.w, self.world.map.h);
    }

    /// A new animation frame at `now` microseconds: owes the simulation the
    /// time since the last frame.
    pub fn frame_start(&mut self, now: i64) {
        if let Some(last) = self.clock {
            self.owed += (now - last).clamp(0, self.tick_us() * MAX_BACKLOG);
            self.owed = self.owed.min(self.tick_us() * MAX_BACKLOG);
        }
        self.clock = Some(now);
        self.frames.push(now);
        self.frames.retain(|t| now - *t < 1_000_000);
    }

    /// Whether a tick should run before this frame is drawn.
    pub fn tick_due(&self) -> bool {
        !self.gallery && self.owed >= self.tick_us()
    }

    /// Runs one tick. When fighting has worn the ants down, the scenario
    /// starts again.
    pub fn tick(&mut self) {
        self.owed = (self.owed - self.tick_us()).max(0);
        self.world.step(&[], &[]);
        if self.world.ants.len() < RESTART_BELOW {
            self.world = bench_world(BENCH_SEED, self.biome);
            self.restarts += 1;
        }
    }

    pub fn tick_done(&mut self, start: i64, end: i64) {
        push_window(&mut self.ticks, end - start, TICK_WINDOW);
    }

    /// This frame's draw list, interpolated between the last two ticks.
    pub fn draw(&self) -> Vec<i32> {
        if self.gallery {
            return render::species_gallery(self.camera.view_w, self.camera.view_h);
        }
        let interp = self.owed * 1000 / self.tick_us().max(1);
        render::frame(
            &self.world,
            self.viewer,
            &self.camera,
            &Overlay::default(),
            interp as i32,
        )
    }

    /// How long making and drawing the list took, from the page's clock.
    pub fn draw_done(&mut self, start: i64, end: i64) {
        push_window(&mut self.draws, end - start, DRAW_WINDOW);
    }

    pub fn fps(&self) -> i64 {
        self.frames.len() as i64
    }

    pub fn tick_average(&self) -> Option<i64> {
        (!self.ticks.is_empty()).then(|| self.ticks.iter().sum::<i64>() / self.ticks.len() as i64)
    }

    pub fn tick_worst(&self) -> Option<i64> {
        self.ticks.iter().copied().max()
    }

    /// The panel as label–value pairs, JSON `[[label, value], …]`.
    pub fn panel(&self) -> Vec<(String, String)> {
        let dash = "–".to_string();
        let avg = self.tick_average();
        let draw = (!self.draws.is_empty())
            .then(|| self.draws.iter().sum::<i64>() / self.draws.len() as i64);
        let status = match avg {
            None => "Measuring",
            Some(a) if a < TICK_BUDGET_US && self.fps() >= FPS_FLOOR => "Within budget",
            Some(_) => "Over budget",
        };
        vec![
            ("Frames per second".into(), self.fps().to_string()),
            ("Tick average".into(), avg.map(ms).unwrap_or(dash.clone())),
            (
                "Tick worst".into(),
                self.tick_worst().map(ms).unwrap_or(dash.clone()),
            ),
            ("Draw average".into(), draw.map(ms).unwrap_or(dash)),
            ("Ants".into(), self.world.ants.len().to_string()),
            (
                "Budget".into(),
                format!(
                    "Tick under {} ms · {} frames per second",
                    TICK_BUDGET_US / 1000,
                    FPS_BUDGET
                ),
            ),
            ("Status".into(), status.into()),
        ]
    }

    pub fn panel_json(&self) -> String {
        serde_json::to_string(&self.panel()).unwrap_or_default()
    }

    /// A mouse wheel turn: a negative delta zooms in, at the pointer.
    pub fn wheel(&mut self, delta: i32, x: i32, y: i32) {
        let steps = -delta.signum();
        self.camera.zoom_at(steps, x, y);
        self.camera.clamp(self.world.map.w, self.world.map.h);
    }

    pub fn drag_start(&mut self, x: i32, y: i32) {
        self.drag = Some((x, y));
    }

    pub fn drag_move(&mut self, x: i32, y: i32) {
        if let Some((lx, ly)) = self.drag {
            self.camera.pan(x - lx, y - ly);
            self.camera.clamp(self.world.map.w, self.world.map.h);
            self.drag = Some((x, y));
        }
    }

    pub fn drag_end(&mut self) {
        self.drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_fall_due_every_hundred_milliseconds() {
        let mut b = Bench::new(800, 600, "");
        b.frame_start(0);
        assert!(!b.tick_due());
        b.frame_start(16_000);
        assert!(!b.tick_due());
        b.frame_start(250_000);
        let mut n = 0;
        while b.tick_due() {
            b.tick();
            n += 1;
        }
        assert_eq!(n, 2, "250 ms owes two ticks of 100 ms");
    }

    #[test]
    fn a_long_stall_is_not_caught_up() {
        let mut b = Bench::new(800, 600, "");
        b.frame_start(0);
        b.frame_start(10_000_000);
        let mut n = 0;
        while b.tick_due() {
            b.tick();
            n += 1;
        }
        assert_eq!(n, MAX_BACKLOG);
    }

    #[test]
    fn the_panel_is_label_value_pairs_and_judges_the_budget() {
        let mut b = Bench::new(800, 600, "");
        assert_eq!(b.panel()[6].1, "Measuring");
        for i in 0..60 {
            b.frame_start(i * 16_000);
        }
        b.tick_done(0, 3_250);
        b.tick_done(0, 4_750);
        let p = b.panel();
        let labels: Vec<&str> = p.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Frames per second",
                "Tick average",
                "Tick worst",
                "Draw average",
                "Ants",
                "Budget",
                "Status"
            ]
        );
        assert_eq!(p[0].1, "60");
        assert_eq!(p[1].1, "4.00 ms");
        assert_eq!(p[2].1, "4.75 ms");
        assert_eq!(p[6].1, "Within budget");
        b.tick_done(0, 30_000);
        assert_eq!(b.panel()[6].1, "Over budget");
    }

    #[test]
    fn options_choose_the_biome_and_the_viewer() {
        assert_eq!(parse_options(""), (Biome::Picnic, None));
        assert_eq!(
            parse_options("?biome=flower_field&viewer=2"),
            (Biome::FlowerField, Some(2))
        );
        assert_eq!(parse_options("viewer=9"), (Biome::Picnic, None));
        assert!(wants_gallery("?biome=picnic&gallery"));
        assert!(!wants_gallery("?biome=picnic"));
        let mut b = Bench::new(800, 600, "?gallery");
        b.frame_start(0);
        b.frame_start(1_000_000);
        assert!(!b.tick_due(), "the gallery stands still");
        assert_eq!(b.draw(), render::species_gallery(800, 600));
    }

    #[test]
    fn the_scenario_has_twelve_hundred_ants() {
        let b = Bench::new(800, 600, "");
        assert!(b.world.ants.len() >= 1200, "{}", b.world.ants.len());
    }
}
