//! Drawing. Everything the page shows is decided here.
//!
//! The page (`web/render.js`) may hold no rule and no number (BUILDER-SPEC
//! D3), so this module lays out every frame completely and the page only
//! replays it:
//!
//! - `assets_json` is sent once: op codes, the palette, every shape as integer
//!   polygons (ants per species × 16 headings, queen, eggs, nodes) and the
//!   scale table the page needs to place them.
//! - `terrain_image` paints the map's ground once, at several pixels per
//!   cell, with smooth, organic edges (no tile grid is ever visible).
//! - `fog_image` paints Fog of War at one pixel per cell; the page scales it
//!   with smoothing, which gives it soft edges.
//! - `frame` writes a flat draw list in sixteenths of a screen pixel.
//! - `minimap` paints the overview.
//!
//! Units: world positions are sub-units (`CELL` per cell). Draw-list
//! coordinates are sixteenths of a pixel (`SUB_PX`); the page sets the base
//! transform `base` = 1/16 once per op. Shapes are in sub-units and are drawn
//! with a scale index `s`, meaning a factor of `s / SCALE_ONE` from sub-units
//! to draw-list units; `Camera::shape_scale` gives the index for the zoom.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::dice::Rng;
use crate::fixed::{cos64, isqrt, sin64, Pos, CELL, STEPS, TRIG_ONE};
use crate::fog::Bits;
use crate::map::Ground;
use crate::rules::{Resource, Species, StructureKind};
use crate::settings::Biome;
use crate::world::{EffectKind, Structure, World};

/// Draw-list units per screen pixel.
pub const SUB_PX: i32 = 16;
/// A shape scale index of `SCALE_ONE` draws a shape at one draw-list unit
/// per sub-unit.
pub const SCALE_ONE: i32 = 1024;
/// The largest scale index the page's table holds.
pub const SCALE_MAX: i32 = 4096;
/// Terrain image pixels per cell, the default the app should use.
pub const TERRAIN_SCALE: i32 = 6;
/// Headings per species shape (0 = east, clockwise on screen).
pub const HEADINGS: usize = 16;
/// Zoom levels, in draw-list units (sixteenths of a pixel) per cell: from
/// 0.75 to 24 pixels per cell.
pub const ZOOMS: [i32; 15] = [
    12, 16, 20, 24, 32, 40, 48, 64, 80, 96, 128, 160, 192, 256, 384,
];
/// From this zoom ants are drawn with legs and antennae.
const DETAIL_ZOOM: i32 = 96;
/// From this zoom ants get a dark outline.
const OUTLINE_ZOOM: i32 = 64;

/// Op codes of the draw list and their argument layouts. Coordinates and
/// widths are draw-list units; colours are palette indices.
pub mod op {
    /// `CLEAR colour` fills the whole canvas.
    pub const CLEAR: i32 = 0;
    /// `IMAGE image sx sy sw sh dx dy dw dh`: source rectangle in image
    /// pixels, destination in draw-list units.
    pub const IMAGE: i32 = 1;
    /// `SHAPE shape x y scale fill stroke width`: the outline is stroked
    /// under the fill, so only the silhouette's edge shows (half the width,
    /// which is in shape units).
    pub const SHAPE: i32 = 2;
    /// `FILL_SHAPE shape x y scale fill`.
    pub const FILL_SHAPE: i32 = 3;
    /// `POLY fill n x1 y1 … xn yn`: a filled closed polygon.
    pub const POLY: i32 = 4;
    /// `OUTLINE stroke width n x1 y1 …`: a stroked closed polygon.
    pub const OUTLINE: i32 = 5;
    /// `LINE stroke width n x1 y1 …`: a stroked open polyline.
    pub const LINE: i32 = 6;

    pub const NAMES: [(&str, i32); 7] = [
        ("clear", CLEAR),
        ("image", IMAGE),
        ("shape", SHAPE),
        ("fill_shape", FILL_SHAPE),
        ("poly", POLY),
        ("outline", OUTLINE),
        ("line", LINE),
    ];

    /// Arguments before the point list, and whether a point list follows
    /// (its count is the last fixed argument).
    pub fn layout(code: i32) -> Option<(usize, bool)> {
        match code {
            CLEAR => Some((1, false)),
            IMAGE => Some((9, false)),
            SHAPE => Some((7, false)),
            FILL_SHAPE => Some((5, false)),
            POLY => Some((2, true)),
            OUTLINE | LINE => Some((3, true)),
            _ => None,
        }
    }
}

/// Image ids the page keeps its offscreen canvases under.
pub const IMAGE_TERRAIN: i32 = 0;
pub const IMAGE_FOG: i32 = 1;
pub const IMAGE_MINIMAP: i32 = 2;

// ---- palette ----

/// Every colour the page uses. Player colours come first, by seat: red, blue,
/// yellow and white, chosen to be as far apart as possible and to stand out
/// against both biomes' ground, always with a dark outline.
pub const PALETTE: [&str; 27] = [
    "#f0302a",
    "#2f7cff",
    "#ffd400",
    "#f4f4f4",
    "rgba(240,48,42,0.42)",
    "rgba(47,124,255,0.42)",
    "rgba(255,212,0,0.42)",
    "rgba(244,244,244,0.42)",
    "#140d08",
    "#0c0907",
    "#3a2c21",
    "rgba(20,13,8,0.82)",
    "rgba(80,230,110,0.45)",
    "rgba(255,50,50,0.5)",
    "#f3dcae",
    "#ff6fb5",
    "#ffd23f",
    "#4fae35",
    "#ffe08a",
    "#ffffff",
    "#b8ff3a",
    "rgba(255,140,30,0.5)",
    "#8dffbe",
    "#ffcf3f",
    "rgba(255,207,63,0.13)",
    "#ffffff",
    "#2a1d12",
];
const C_PLAYER: i32 = 0;
const C_SOFT: i32 = 4;
const C_INK: i32 = 8;
const C_BACKDROP: i32 = 9;
const C_BASE: i32 = 10;
const C_ICON: i32 = 11;
const C_VALID: i32 = 12;
const C_INVALID: i32 = 13;
const C_CRUMB: i32 = 14;
const C_PETAL: i32 = 15;
const C_POLLEN: i32 = 16;
const C_LEAF: i32 = 17;
const C_CACHE: i32 = 18;
const C_BITE: i32 = 19;
const C_ACID: i32 = 20;
const C_BLAST: i32 = 21;
const C_HEAL: i32 = 22;
const C_HILL: i32 = 23;
const C_HILL_FILL: i32 = 24;
const C_HOVER: i32 = 25;
const C_POLE: i32 = 26;

/// Player colours as RGB, for the minimap.
pub const PLAYER_RGB: [[u8; 3]; 4] = [
    [240, 48, 42],
    [47, 124, 255],
    [255, 212, 0],
    [244, 244, 244],
];

fn player(colony: u8) -> i32 {
    C_PLAYER + (colony % 4) as i32
}

fn soft(colony: u8) -> i32 {
    C_SOFT + (colony % 4) as i32
}

// ---- camera ----

/// What part of the map the screen shows. The app owns one and moves it with
/// these helpers; `frame` reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Camera {
    /// The world point at the centre of the view, in sub-units.
    pub centre_x: i32,
    pub centre_y: i32,
    /// Index into `ZOOMS`.
    pub zoom: usize,
    /// View size in screen pixels.
    pub view_w: i32,
    pub view_h: i32,
}

impl Camera {
    pub fn new(view_w: i32, view_h: i32) -> Camera {
        Camera {
            centre_x: 0,
            centre_y: 0,
            zoom: 10,
            view_w: view_w.max(1),
            view_h: view_h.max(1),
        }
    }

    /// Draw-list units per cell.
    pub fn units_per_cell(&self) -> i32 {
        ZOOMS[self.zoom.min(ZOOMS.len() - 1)]
    }

    /// The scale index that draws a sub-unit shape at this zoom.
    pub fn shape_scale(&self) -> i32 {
        self.units_per_cell() * SCALE_ONE / CELL
    }

    /// A world point in draw-list units.
    pub fn screen(&self, p: Pos) -> (i32, i32) {
        let z = self.units_per_cell() as i64;
        let c = CELL as i64;
        let x = (p.x - self.centre_x) as i64 * z;
        let y = (p.y - self.centre_y) as i64 * z;
        (
            (x.div_euclid(c) + (self.view_w * SUB_PX / 2) as i64) as i32,
            (y.div_euclid(c) + (self.view_h * SUB_PX / 2) as i64) as i32,
        )
    }

    /// A length in sub-units as draw-list units.
    pub fn length(&self, sub: i32) -> i32 {
        (sub as i64 * self.units_per_cell() as i64 / CELL as i64) as i32
    }

    /// The world point under a screen pixel.
    pub fn world_at(&self, px: i32, py: i32) -> Pos {
        let z = self.units_per_cell() as i64;
        let c = CELL as i64;
        let dx = (px * SUB_PX - self.view_w * SUB_PX / 2) as i64 * c;
        let dy = (py * SUB_PX - self.view_h * SUB_PX / 2) as i64 * c;
        Pos::new(
            self.centre_x + dx.div_euclid(z) as i32,
            self.centre_y + dy.div_euclid(z) as i32,
        )
    }

    pub fn resize(&mut self, view_w: i32, view_h: i32) {
        self.view_w = view_w.max(1);
        self.view_h = view_h.max(1);
    }

    /// Moves the view by a mouse drag of (dx, dy) pixels: the map follows
    /// the pointer.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        let z = self.units_per_cell() as i64;
        let c = CELL as i64;
        self.centre_x -= ((dx * SUB_PX) as i64 * c / z) as i32;
        self.centre_y -= ((dy * SUB_PX) as i64 * c / z) as i32;
    }

    /// Zooms by `steps` levels (positive = closer), keeping the world point
    /// under pixel (px, py) where it is.
    pub fn zoom_at(&mut self, steps: i32, px: i32, py: i32) {
        let before = self.world_at(px, py);
        let z = (self.zoom as i32 + steps).clamp(0, ZOOMS.len() as i32 - 1);
        self.zoom = z as usize;
        let after = self.world_at(px, py);
        self.centre_x += before.x - after.x;
        self.centre_y += before.y - after.y;
    }

    /// Keeps the view centre on the map.
    pub fn clamp(&mut self, map_w: i32, map_h: i32) {
        self.centre_x = self.centre_x.clamp(0, map_w * CELL);
        self.centre_y = self.centre_y.clamp(0, map_h * CELL);
    }

    /// Centres the map and picks the closest zoom that shows all of it.
    pub fn fit(&mut self, map_w: i32, map_h: i32) {
        self.centre_x = map_w * CELL / 2;
        self.centre_y = map_h * CELL / 2;
        self.zoom = 0;
        for (i, z) in ZOOMS.iter().enumerate() {
            if map_w * z <= self.view_w * SUB_PX && map_h * z <= self.view_h * SUB_PX {
                self.zoom = i;
            }
        }
    }
}

// ---- shapes ----

type Pt = (i32, i32);
type Shape = Vec<Vec<Pt>>;

fn rotate(p: Pt, step: usize) -> Pt {
    let (c, s) = (cos64(step) as i64, sin64(step) as i64);
    let (x, y) = (p.0 as i64, p.1 as i64);
    let t = TRIG_ONE as i64;
    (((x * c - y * s) / t) as i32, ((x * s + y * c) / t) as i32)
}

/// An ellipse polygon with `n` vertices, its long axis turned by `rot` steps.
fn ellipse(cx: i32, cy: i32, rx: i32, ry: i32, rot: usize, n: usize) -> Vec<Pt> {
    (0..n)
        .map(|k| {
            let st = k * STEPS / n;
            let p = (rx * cos64(st) / TRIG_ONE, ry * sin64(st) / TRIG_ONE);
            let q = rotate(p, rot);
            (q.0 + cx, q.1 + cy)
        })
        .collect()
}

/// A straight limb of thickness `t` from a to b.
fn bar(a: Pt, b: Pt, t: i32) -> Vec<Pt> {
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let len = (isqrt((dx * dx + dy * dy) as u64) as i64).max(1);
    let nx = (-dy * t as i64 / (2 * len)) as i32;
    let ny = (dx * t as i64 / (2 * len)) as i32;
    vec![
        (a.0 + nx, a.1 + ny),
        (b.0 + nx, b.1 + ny),
        (b.0 - nx, b.1 - ny),
        (a.0 - nx, a.1 - ny),
    ]
}

/// A pointed leaf (or wing) of length `len` and width `wid` from `at`,
/// pointing along `dir` steps.
fn lens(at: Pt, len: i32, wid: i32, dir: usize) -> Vec<Pt> {
    let mut pts = Vec::new();
    for k in 0..=8 {
        pts.push((len * k / 8, wid / 2 * sin64(k as usize * 4) / TRIG_ONE));
    }
    for k in (1..8).rev() {
        pts.push((len * k / 8, -wid / 2 * sin64(k as usize * 4) / TRIG_ONE));
    }
    pts.into_iter()
        .map(|p| {
            let q = rotate(p, dir);
            (q.0 + at.0, q.1 + at.1)
        })
        .collect()
}

fn signed_area(poly: &[Pt]) -> i64 {
    let mut a = 0i64;
    for i in 0..poly.len() {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % poly.len()];
        a += x0 as i64 * y1 as i64 - x1 as i64 * y0 as i64;
    }
    a
}

/// Gives every part the same winding, so the page's nonzero fill joins
/// overlapping parts instead of cutting holes.
fn same_winding(shape: &mut Shape) {
    for part in shape.iter_mut() {
        if signed_area(part) < 0 {
            part.reverse();
        }
    }
}

/// The parts of a species' body in template units (body radius 100, facing
/// east). `detailed` adds legs and antennae. Every species has its own
/// silhouette.
fn ant_template(s: Species, detailed: bool) -> Shape {
    // (centre x, radius x, radius y) of abdomen, thorax and head.
    let (abd, tho, head) = match s {
        Species::Harvester => ((-78, 55, 42), (2, 26, 16), (46, 24, 22)),
        Species::Soldier => ((-72, 46, 34), (2, 26, 17), (52, 32, 30)),
        Species::Carpenter => ((-82, 58, 30), (2, 26, 16), (50, 24, 24)),
        Species::Leafcutter => ((-66, 40, 30), (0, 22, 14), (36, 20, 18)),
        Species::Fire => ((-62, 42, 22), (0, 22, 11), (38, 18, 16)),
        Species::Turtle => ((-62, 48, 52), (0, 24, 24), (46, 22, 40)),
        Species::Acid => ((-70, 50, 40), (2, 24, 15), (44, 22, 20)),
        Species::Nurse => ((-66, 44, 44), (2, 24, 15), (44, 22, 20)),
        Species::Flying => ((-74, 48, 30), (2, 28, 18), (46, 22, 20)),
        Species::Exploding => ((-62, 58, 58), (6, 22, 14), (44, 20, 18)),
        Species::TrapJaw => ((-70, 44, 32), (0, 26, 16), (46, 26, 22)),
        Species::Dinosaur => ((-90, 60, 38), (0, 36, 20), (62, 30, 26)),
    };
    let mut parts: Shape = Vec::new();
    let (hx, hr) = (head.0, head.1);
    if detailed {
        for side in [1, -1] {
            let legs = [
                [(10, 6), (32, 40), (58, 58)],
                [(0, 8), (-2, 46), (-18, 70)],
                [(-12, 6), (-36, 40), (-70, 62)],
            ];
            for leg in legs {
                let p: Vec<Pt> = leg.iter().map(|&(x, y)| (x, y * side)).collect();
                parts.push(bar(p[0], p[1], 8));
                parts.push(bar(p[1], p[2], 7));
            }
            let a0 = (hx + hr / 2, 6 * side);
            let a1 = (hx + hr + 16, 26 * side);
            let a2 = (hx + hr + 42, 24 * side);
            parts.push(bar(a0, a1, 6));
            parts.push(bar(a1, a2, 5));
        }
    }
    parts.push(ellipse(abd.0, 0, abd.1, abd.2, 0, 16));
    parts.push(ellipse((abd.0 + abd.1 + tho.0 - tho.1) / 2, 0, 10, 8, 0, 8));
    parts.push(ellipse(tho.0, 0, tho.1, tho.2, 0, 12));
    match s {
        Species::Carpenter => {
            // A square, heavy head.
            parts.push(vec![
                (hx - hr, -head.2),
                (hx + hr - 6, -head.2),
                (hx + hr, -head.2 + 8),
                (hx + hr, head.2 - 8),
                (hx + hr - 6, head.2),
                (hx - hr, head.2),
            ]);
        }
        _ => parts.push(ellipse(hx, 0, hr, head.2, 0, 14)),
    }
    let front = hx + hr;
    match s {
        Species::Soldier => {
            for side in [1, -1] {
                parts.push(vec![
                    (front - 8, 12 * side),
                    (front + 24, 4 * side),
                    (front + 4, 22 * side),
                ]);
            }
        }
        Species::Leafcutter => {
            // The leaf it carries, held up over its head.
            parts.push(lens((front - 30, 0), 120, 74, 0));
        }
        Species::Fire => {
            parts.push(vec![
                (abd.0 - abd.1 + 6, -8),
                (abd.0 - abd.1 - 30, 0),
                (abd.0 - abd.1 + 6, 8),
            ]);
        }
        Species::Turtle => {
            // A flat shield across the thorax.
            parts.push(ellipse(tho.0, 0, 18, 36, 0, 12));
        }
        Species::Acid => {
            // A long, sharp tip to spray from.
            parts.push(vec![
                (abd.0 - abd.1 + 14, -18),
                (abd.0 - abd.1 - 46, 0),
                (abd.0 - abd.1 + 14, 18),
            ]);
        }
        Species::Nurse => {
            parts.push(bar((abd.0, -64), (abd.0, 64), 24));
            parts.push(bar((abd.0 - 64, 0), (abd.0 + 64, 0), 24));
        }
        Species::Flying => {
            for (side, rot) in [(1, STEPS - 5), (-1, 5)] {
                parts.push(ellipse(-44, 40 * side, 78, 22, rot, 16));
            }
        }
        Species::Exploding => {
            for k in 0..6 {
                let st = k * STEPS / 6 + 5;
                let a = rotate((52, -14), st);
                let b = rotate((88, 0), st);
                let c = rotate((52, 14), st);
                parts.push(vec![
                    (a.0 + abd.0, a.1),
                    (b.0 + abd.0, b.1),
                    (c.0 + abd.0, c.1),
                ]);
            }
        }
        Species::TrapJaw => {
            for side in [1, -1] {
                parts.push(bar((front - 6, 10 * side), (front + 62, 14 * side), 9));
                parts.push(bar((front + 56, 14 * side), (front + 62, 2 * side), 8));
            }
        }
        Species::Dinosaur => {
            for side in [1, -1] {
                parts.push(bar((front - 6, 12 * side), (front + 34, 6 * side), 12));
            }
            parts.push(ellipse(tho.0, 0, 20, 30, 0, 10));
        }
        Species::Harvester | Species::Carpenter => {
            for side in [1, -1] {
                parts.push(bar((front - 4, 8 * side), (front + 14, 4 * side), 8));
            }
        }
    }
    parts
}

fn transform(template: &Shape, num: i32, den: i32, step: usize) -> Shape {
    let mut shape: Shape = template
        .iter()
        .map(|part| {
            part.iter()
                .map(|&(x, y)| {
                    let p = (
                        (x as i64 * num as i64 / den as i64) as i32,
                        (y as i64 * num as i64 / den as i64) as i32,
                    );
                    rotate(p, step)
                })
                .collect()
        })
        .collect();
    same_winding(&mut shape);
    shape
}

/// The drawn body radius of a species, in sub-units: a little more than its
/// physical radius, so small ants stay readable.
fn drawn_radius(s: Species) -> i32 {
    s.stats().radius * 5 / 4
}

/// One species facing one of 16 headings, in sub-units.
pub fn ant_shape(s: Species, heading: u8, detailed: bool) -> Shape {
    let step = (heading as usize % HEADINGS) * (STEPS / HEADINGS);
    transform(&ant_template(s, detailed), drawn_radius(s), 100, step)
}

/// Shape ids: ants first, `[detailed][species][heading]`, then the rest.
pub fn ant_shape_id(s: Species, heading: u8, detailed: bool) -> i32 {
    ((detailed as usize * Species::ALL.len() + s.index()) * HEADINGS + heading as usize % HEADINGS)
        as i32
}
const ANT_SHAPES: i32 = (2 * Species::ALL.len() * HEADINGS) as i32;
const SH_QUEEN: i32 = ANT_SHAPES;
const SH_EGGS: i32 = ANT_SHAPES + 1;
const SH_CRUMBS: i32 = ANT_SHAPES + 2;
const SH_PETALS: i32 = ANT_SHAPES + 3;
const SH_POLLEN: i32 = ANT_SHAPES + 4;
const SH_LEAVES: i32 = ANT_SHAPES + 5;
/// Headings for an icon that faces up the screen.
const UP: u8 = 12;

fn queen_shape() -> Shape {
    let mut t: Shape = Vec::new();
    for side in [1, -1] {
        for leg in [
            [(10, 6), (34, 44), (62, 62)],
            [(0, 8), (-4, 50), (-22, 76)],
            [(-12, 6), (-40, 44), (-78, 70)],
        ] {
            let p: Vec<Pt> = leg.iter().map(|&(x, y)| (x, y * side)).collect();
            t.push(bar(p[0], p[1], 9));
            t.push(bar(p[1], p[2], 8));
        }
        t.push(ellipse(
            -34,
            34 * side,
            46,
            15,
            if side > 0 { STEPS - 4 } else { 4 },
            12,
        ));
    }
    t.push(ellipse(-92, 0, 72, 50, 0, 18));
    t.push(ellipse(-16, 0, 10, 8, 0, 8));
    t.push(ellipse(4, 0, 30, 22, 0, 12));
    t.push(ellipse(50, 0, 24, 22, 0, 14));
    // A crown of three points on the head.
    t.push(vec![
        (64, -18),
        (90, -14),
        (78, -4),
        (94, 0),
        (78, 4),
        (90, 14),
        (64, 18),
    ]);
    transform(&t, 2, 1, UP as usize * (STEPS / HEADINGS))
}

fn eggs_shape() -> Shape {
    let mut t = vec![
        ellipse(-70, 34, 34, 46, 0, 14),
        ellipse(0, -36, 34, 46, 0, 14),
        ellipse(70, 34, 34, 46, 0, 14),
    ];
    same_winding(&mut t);
    t
}

fn crumbs_shape() -> Shape {
    let mut t: Shape = [
        (0, 0, 92, 78),
        (-128, -64, 62, 54),
        (118, -76, 66, 58),
        (-96, 104, 60, 50),
        (110, 96, 70, 56),
        (8, -150, 52, 46),
        (-170, 30, 46, 40),
    ]
    .iter()
    .enumerate()
    .map(|(i, &(x, y, rx, ry))| ellipse(x, y, rx, ry, i * 9, 10))
    .collect();
    same_winding(&mut t);
    t
}

fn petals_shape() -> Shape {
    let mut t: Shape = (0..6)
        .map(|k| {
            let dir = k * STEPS / 6 + 2;
            let c = rotate((150, 0), dir);
            ellipse(c.0, c.1, 108, 64, dir, 14)
        })
        .collect();
    same_winding(&mut t);
    t
}

fn pollen_shape() -> Shape {
    vec![ellipse(0, 0, 86, 86, 0, 16)]
}

fn leaves_shape() -> Shape {
    let mut t: Shape = [(0usize, 300), (22, 260), (44, 280)]
        .iter()
        .map(|&(dir, len)| lens((0, 0), len, 130, dir + 3))
        .collect();
    same_winding(&mut t);
    t
}

/// Every shape the page builds once, indexed by shape id.
pub fn shapes() -> Vec<Shape> {
    let mut out = Vec::new();
    for detailed in [false, true] {
        for s in Species::ALL {
            for h in 0..HEADINGS {
                out.push(ant_shape(s, h as u8, detailed));
            }
        }
    }
    out.push(queen_shape());
    out.push(eggs_shape());
    out.push(crumbs_shape());
    out.push(petals_shape());
    out.push(pollen_shape());
    out.push(leaves_shape());
    out
}

/// `num / den` as an exact decimal, for a power-of-two `den`.
fn decimal(num: i32, den: i32) -> String {
    let whole = num / den;
    let mut rem = (num % den) as i64;
    if rem == 0 {
        return whole.to_string();
    }
    let mut digits = String::new();
    while rem != 0 {
        rem *= 10;
        digits.push(char::from(b'0' + (rem / den as i64) as u8));
        rem %= den as i64;
    }
    format!("{whole}.{digits}")
}

/// Everything the page needs once, as JSON text: op codes, image ids, the
/// palette, every shape (lists of `[x, y]` polygons) and two exact decimals
/// the page cannot work out itself: `base`, the draw-list unit in pixels,
/// and `scales`, the factor for every shape scale index.
pub fn assets_json() -> String {
    let mut ops = serde_json::Map::new();
    for (name, code) in op::NAMES {
        ops.insert(name.to_string(), json!(code));
    }
    let shape_list: Vec<Vec<Vec<[i32; 2]>>> = shapes()
        .iter()
        .map(|s| {
            s.iter()
                .map(|p| p.iter().map(|&(x, y)| [x, y]).collect())
                .collect()
        })
        .collect();
    let v = json!({
        "ops": ops,
        "images": {"terrain": IMAGE_TERRAIN, "fog": IMAGE_FOG, "minimap": IMAGE_MINIMAP},
        "palette": PALETTE,
        "shapes": shape_list,
    });
    let mut text = v.to_string();
    text.pop();
    text.push_str(",\"base\":");
    text.push_str(&decimal(1, SUB_PX));
    text.push_str(",\"scales\":[");
    for i in 0..=SCALE_MAX {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&decimal(i, SCALE_ONE));
    }
    text.push_str("]}");
    text
}

// ---- terrain ----

/// The biome actually drawn: Random picks one from the seed.
pub fn biome_of(world: &World) -> Biome {
    match world.settings.biome {
        Biome::Random => {
            if world.seed.is_multiple_of(2) {
                Biome::Picnic
            } else {
                Biome::FlowerField
            }
        }
        b => b,
    }
}

/// Changes whenever the ground changes (dirt dug away), so the app knows to
/// paint the terrain again.
pub fn terrain_stamp(world: &World) -> u32 {
    world
        .map
        .ground
        .iter()
        .filter(|g| **g == Ground::Dirt)
        .count() as u32
}

/// Per-cell fields, padded by one rock cell on every side, sampled
/// bilinearly per pixel. Values run 0..=256.
struct Fields {
    pw: i32,
    /// Rock or dirt, blurred, plus a low-frequency wobble: the edge is where
    /// this crosses 128.
    solid: Vec<i32>,
    /// Dirt, blurred.
    dirt: Vec<i32>,
    /// How much solid ground is near, widely blurred: shading for floors.
    shade: Vec<i32>,
}

fn blur3(src: &[i32], w: i32, h: i32) -> Vec<i32> {
    let mut out = src.to_vec();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = |dx: i32, dy: i32| src[((y + dy) * w + x + dx) as usize];
            let s = at(-1, -1)
                + 2 * at(0, -1)
                + at(1, -1)
                + 2 * at(-1, 0)
                + 4 * at(0, 0)
                + 2 * at(1, 0)
                + at(-1, 1)
                + 2 * at(0, 1)
                + at(1, 1);
            out[(y * w + x) as usize] = s / 16;
        }
    }
    out
}

fn box_blur(src: &[i32], w: i32, h: i32, r: i32) -> Vec<i32> {
    let mut tmp = vec![0; src.len()];
    let n = 2 * r + 1;
    for y in 0..h {
        for x in 0..w {
            let mut s = 0;
            for dx in -r..=r {
                let xx = (x + dx).clamp(0, w - 1);
                s += src[(y * w + xx) as usize];
            }
            tmp[(y * w + x) as usize] = s / n;
        }
    }
    let mut out = vec![0; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0;
            for dy in -r..=r {
                let yy = (y + dy).clamp(0, h - 1);
                s += tmp[(yy * w + x) as usize];
            }
            out[(y * w + x) as usize] = s / n;
        }
    }
    out
}

/// Seeded texture tiles: fine grain, soft blotches and the flower lattice.
struct Texture {
    grain: Vec<i32>,
    blot: Vec<i32>,
    wobble: Vec<i32>,
    flowers: Vec<(i32, i32, usize, bool, usize)>,
}

const GRAIN: i32 = 64;
const BLOT: i32 = 32;
const FLOWERS: i32 = 16;

impl Texture {
    fn new(seed: u64) -> Texture {
        let mut rng = Rng::new(seed ^ 0x7465_7272_6169_6E00);
        let grain = (0..GRAIN * GRAIN).map(|_| rng.range(-128, 128)).collect();
        let blot = (0..BLOT * BLOT).map(|_| rng.range(-128, 128)).collect();
        let wobble = (0..BLOT * BLOT).map(|_| rng.range(-40, 40)).collect();
        let flowers = (0..FLOWERS * FLOWERS)
            .map(|_| {
                (
                    rng.range(-20, 20),
                    rng.range(-20, 20),
                    rng.below(FLOWER_RGB.len() as u32) as usize,
                    rng.chance(72),
                    rng.below(STEPS as u32) as usize,
                )
            })
            .collect();
        Texture {
            grain,
            blot,
            wobble,
            flowers,
        }
    }

    fn grain(&self, x: i32, y: i32) -> i32 {
        self.grain[(y.rem_euclid(GRAIN) * GRAIN + x.rem_euclid(GRAIN)) as usize]
    }

    /// Smooth value noise from a tile; (x, y) in 1/256 lattice steps.
    fn smooth(tile: &[i32], x: i32, y: i32) -> i32 {
        let (ix, fx) = (x >> 8, x & 255);
        let (iy, fy) = (y >> 8, y & 255);
        let at = |a: i32, b: i32| tile[(b.rem_euclid(BLOT) * BLOT + a.rem_euclid(BLOT)) as usize];
        let top = at(ix, iy) * (256 - fx) + at(ix + 1, iy) * fx;
        let bottom = at(ix, iy + 1) * (256 - fx) + at(ix + 1, iy + 1) * fx;
        (top * (256 - fy) + bottom * fy) >> 16
    }

    fn blot(&self, x: i32, y: i32) -> i32 {
        Texture::smooth(&self.blot, x, y)
    }
}

const FLOWER_RGB: [[i32; 3]; 5] = [
    [255, 138, 196],
    [252, 250, 240],
    [255, 210, 70],
    [196, 128, 255],
    [255, 120, 90],
];

impl Fields {
    fn new(world: &World, tex: &Texture) -> Fields {
        let (w, h) = (world.map.w, world.map.h);
        let (pw, ph) = (w + 2, h + 2);
        let mut solid = vec![256; (pw * ph) as usize];
        let mut dirt = vec![0; (pw * ph) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y + 1) * pw + x + 1) as usize;
                match world.map.at(x, y) {
                    Ground::Floor => solid[i] = 0,
                    Ground::Dirt => dirt[i] = 256,
                    Ground::Rock => {}
                }
            }
        }
        let shade = box_blur(&box_blur(&solid, pw, ph, 2), pw, ph, 2);
        let mut solid = blur3(&solid, pw, ph);
        let dirt = blur3(&dirt, pw, ph);
        // A low-frequency wobble (one lattice step per four cells) makes the
        // edges organic without moving them far from the cells ants use.
        for y in 0..ph {
            for x in 0..pw {
                let i = (y * pw + x) as usize;
                if solid[i] > 0 && solid[i] < 256 {
                    solid[i] += Texture::smooth(&tex.wobble, x * 64, y * 64);
                }
            }
        }
        Fields {
            pw,
            solid,
            dirt,
            shade,
        }
    }

    fn sample(&self, f: &[i32], ix: i32, iy: i32, fx: i32, fy: i32) -> i32 {
        let i = ((iy + 1) * self.pw + ix + 1) as usize;
        let p = self.pw as usize;
        let top = f[i] * (256 - fx) + f[i + 1] * fx;
        let bottom = f[i + p] * (256 - fx) + f[i + p + 1] * fx;
        (top * (256 - fy) + bottom * fy) >> 16
    }
}

/// A pixel's centre relative to cell centres: the cell to its upper left
/// and the fraction (0..256) towards the next.
fn split(p: i32, scale: i32) -> (i32, i32) {
    let u = (2 * p + 1) * 256 / (2 * scale) - 128;
    (u >> 8, u & 255)
}

fn mix(a: [i32; 3], b: [i32; 3], t: i32) -> [i32; 3] {
    let t = t.clamp(0, 256);
    [
        (a[0] * (256 - t) + b[0] * t) >> 8,
        (a[1] * (256 - t) + b[1] * t) >> 8,
        (a[2] * (256 - t) + b[2] * t) >> 8,
    ]
}

fn add(c: [i32; 3], d: i32) -> [i32; 3] {
    [c[0] + d, c[1] + d, c[2] + d]
}

struct Painter<'a> {
    tex: &'a Texture,
    biome: Biome,
    scale: i32,
}

impl Painter<'_> {
    fn floor(&self, px: i32, py: i32, shade: i32) -> [i32; 3] {
        let base = match self.biome {
            Biome::FlowerField => [144, 108, 70],
            _ => [122, 92, 62],
        };
        let lat = 256 * 2 / self.scale.max(1);
        let b = self.tex.blot(px * lat / 4, py * lat / 4) * 14 / 128;
        let g = self.tex.grain(px, py) * 9 / 128;
        let c = add(base, b + g);
        let f = 256 - shade * 120 / 256;
        [c[0] * f / 256, c[1] * f / 256, c[2] * f / 256]
    }

    fn dirt(&self, px: i32, py: i32) -> [i32; 3] {
        let lat = 256 * 2 / self.scale.max(1);
        let clod = self.tex.blot(px * lat + 4000, py * lat + 4000);
        let g = self.tex.grain(px + 17, py + 31);
        let mut c = add([206, 154, 88], g * 10 / 128);
        if clod > 30 {
            c = mix(c, [124, 80, 40], (clod - 30) * 4);
        }
        if g > 100 {
            c = [96, 62, 32];
        }
        c
    }

    fn rock(&self, px: i32, py: i32, depth: i32) -> [i32; 3] {
        match self.biome {
            Biome::FlowerField => self.bush(px, py, depth),
            _ => self.blanket(px, py, depth),
        }
    }

    /// Picnic rock: a gingham picnic blanket, edged with grass.
    fn blanket(&self, px: i32, py: i32, depth: i32) -> [i32; 3] {
        let g = self.tex.grain(px, py);
        if depth < 48 {
            // Grass fringe where the blanket meets the soil.
            let blade = self.tex.grain(px / 2, py * 3);
            return add([84, 150, 58], blade * 26 / 128 + g * 6 / 128);
        }
        let check = (self.scale * 2).max(2);
        let a = px.div_euclid(check) & 1;
        let b = py.div_euclid(check) & 1;
        let c = match a + b {
            0 => [244, 234, 214],
            1 => [226, 168, 158],
            _ => [196, 96, 94],
        };
        let weave = if (px + py) & 1 == 0 { 5 } else { -5 };
        add(c, weave + g * 5 / 128)
    }

    /// Flower Field rock: dense bushes covered in flowers.
    fn bush(&self, px: i32, py: i32, depth: i32) -> [i32; 3] {
        let lat = 256 * 2 / self.scale.max(1);
        let b = self.tex.blot(px * lat / 2 + 900, py * lat / 2 + 900);
        let g = self.tex.grain(px, py);
        let mut c = add([44, 94, 40], b * 22 / 128 + g * 10 / 128);
        if depth < 40 {
            c = mix([30, 60, 28], c, depth * 6);
        }
        if depth < 64 {
            return c;
        }
        // Flowers on a jittered lattice, 1.5 cells apart.
        let l = self.scale * SUB_PX * 3 / 2;
        let (x, y) = (px * SUB_PX + SUB_PX / 2, py * SUB_PX + SUB_PX / 2);
        let (lx, ly) = (x.div_euclid(l), y.div_euclid(l));
        let f =
            self.tex.flowers[(ly.rem_euclid(FLOWERS) * FLOWERS + lx.rem_euclid(FLOWERS)) as usize];
        if !f.3 {
            return c;
        }
        let cx = l / 2 + f.0 * l / 100;
        let cy = l / 2 + f.1 * l / 100;
        let (dx, dy) = (x.rem_euclid(l) - cx, y.rem_euclid(l) - cy);
        let centre_r = l * 11 / 100;
        if dx * dx + dy * dy < centre_r * centre_r {
            return [250, 196, 40];
        }
        let petal_r = l * 15 / 100;
        let ring = l * 20 / 100;
        for k in 0..5 {
            let st = f.4 + k * STEPS / 5;
            let (qx, qy) = (cos64(st) * ring / TRIG_ONE, sin64(st) * ring / TRIG_ONE);
            let (ex, ey) = (dx - qx, dy - qy);
            if ex * ex + ey * ey < petal_r * petal_r {
                let p = FLOWER_RGB[f.2];
                return add(p, -(ex * ex + ey * ey) * 30 / (petal_r * petal_r));
            }
        }
        c
    }
}

/// The ground of the whole map at `scale` pixels per cell: (width, height,
/// RGBA bytes). Rock, dirt and floor each have their own texture, edges are
/// smooth curves with anti-aliasing, and the result depends only on the map
/// and its seed.
pub fn terrain_image(world: &World, scale: i32) -> (i32, i32, Vec<u8>) {
    terrain_region(world, scale, 0, 0, world.map.w, world.map.h)
}

/// Part of the terrain image: cells x0..x1, y0..y1. The pixels are exactly
/// those of the same area in `terrain_image`, so the app can repaint the
/// area round dug dirt.
pub fn terrain_region(
    world: &World,
    scale: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> (i32, i32, Vec<u8>) {
    let scale = scale.clamp(1, 32);
    let (x0, x1) = (x0.clamp(0, world.map.w), x1.clamp(0, world.map.w));
    let (y0, y1) = (y0.clamp(0, world.map.h), y1.clamp(0, world.map.h));
    let tex = Texture::new(world.seed);
    let f = Fields::new(world, &tex);
    let paint = Painter {
        tex: &tex,
        biome: biome_of(world),
        scale,
    };
    let (px0, py0, px1, py1) = (x0 * scale, y0 * scale, x1 * scale, y1 * scale);
    let (pw, ph) = ((px1 - px0).max(0), (py1 - py0).max(0));
    let mut out = vec![0u8; (pw * ph * 4) as usize];
    let cols: Vec<(i32, i32)> = (px0..px1).map(|px| split(px, scale)).collect();
    // Field units per pixel near an edge, for the anti-aliasing ramp and the
    // dark outline.
    let per_px = (128 / scale).max(1);
    let mut o = 0;
    for py in py0..py1 {
        let (iy, fy) = split(py, scale);
        for (k, px) in (px0..px1).enumerate() {
            let (ix, fx) = cols[k];
            let s = f.sample(&f.solid, ix, iy, fx, fy);
            let shade = f.sample(&f.shade, ix, iy, fx, fy);
            let floor = paint.floor(px, py, shade);
            let alpha = ((s - 128) * 256 / (per_px * 2) + 128).clamp(0, 256);
            let c = if alpha == 0 {
                floor
            } else {
                let depth = s - 128;
                let d = f.sample(&f.dirt, ix, iy, fx, fy);
                let solid = if d > 128 + per_px {
                    paint.dirt(px, py)
                } else if d > 128 - per_px {
                    mix(
                        paint.rock(px, py, depth),
                        paint.dirt(px, py),
                        (d - 128 + per_px) * 128 / per_px,
                    )
                } else {
                    paint.rock(px, py, depth)
                };
                // A dark line traces every edge.
                let edge = if d > 128 { [70, 44, 24] } else { [34, 30, 20] };
                let line = (256 - (depth - per_px) * 256 / per_px.max(1)).clamp(0, 256);
                let solid = mix(solid, edge, line * 3 / 4);
                mix(floor, solid, alpha)
            };
            out[o] = c[0].clamp(0, 255) as u8;
            out[o + 1] = c[1].clamp(0, 255) as u8;
            out[o + 2] = c[2].clamp(0, 255) as u8;
            out[o + 3] = 255;
            o += 4;
        }
    }
    (pw, ph, out)
}

// ---- fog ----

/// What a viewer may see: everything (`None`, a spectator or the
/// Benchmark), or what its team sees now and has explored.
struct Sight {
    all: bool,
    team: u8,
    w: i32,
    h: i32,
    visible: Bits,
    explored: Bits,
}

impl Sight {
    fn new(world: &World, viewer: Option<u8>) -> Sight {
        let (w, h) = (world.map.w, world.map.h);
        match viewer.filter(|v| (*v as usize) < world.colonies.len()) {
            Some(v) => Sight {
                all: false,
                team: world.colonies[v as usize].team,
                w,
                h,
                visible: world.team_visible(v),
                explored: world.team_explored(v),
            },
            None => Sight {
                all: true,
                team: u8::MAX,
                w,
                h,
                visible: Bits::new(0),
                explored: Bits::new(0),
            },
        }
    }

    fn index(&self, p: Pos) -> Option<usize> {
        let (x, y) = p.cell();
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then_some((y * self.w + x) as usize)
    }

    fn sees(&self, p: Pos) -> bool {
        self.all || self.index(p).map(|i| self.visible.get(i)).unwrap_or(false)
    }

    fn knows(&self, p: Pos) -> bool {
        self.all || self.index(p).map(|i| self.explored.get(i)).unwrap_or(false)
    }

    fn friendly(&self, world: &World, colony: u8) -> bool {
        world
            .colonies
            .get(colony as usize)
            .map(|c| c.team == self.team)
            .unwrap_or(false)
    }

    /// Own and allied things always show; enemy things only where the team
    /// can see now.
    fn shows(&self, world: &World, colony: u8, p: Pos) -> bool {
        self.all || self.friendly(world, colony) || self.sees(p)
    }
}

/// Fog of War at one pixel per cell (RGBA): unexplored is opaque near-black,
/// explored is dimmed, visible is clear. Allies share vision. `None` sees
/// everything.
pub fn fog_image(world: &World, viewer: Option<u8>) -> Vec<u8> {
    let sight = Sight::new(world, viewer);
    let n = (world.map.w * world.map.h) as usize;
    let mut out = vec![0u8; n * 4];
    if sight.all {
        return out;
    }
    let (w, h) = (world.map.w, world.map.h);
    let alpha: Vec<i32> = (0..n)
        .map(|i| {
            if sight.visible.get(i) {
                0
            } else if sight.explored.get(i) {
                150
            } else {
                255
            }
        })
        .collect();
    // Softened once more before the page's smooth scaling, so the edge of
    // vision is a gentle curve rather than a staircase of cells.
    let alpha = blur3(&blur3(&alpha, w, h), w, h);
    for i in 0..n {
        out[i * 4] = 10;
        out[i * 4 + 1] = 8;
        out[i * 4 + 2] = 6;
        out[i * 4 + 3] = alpha[i].clamp(0, 255) as u8;
    }
    out
}

// ---- frame ----

/// What the app adds on top of the world: a blueprint being placed and the
/// waypoint under the pointer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Overlay {
    /// Terrain image pixels per cell, as passed to `terrain_image`;
    /// 0 means `TERRAIN_SCALE`.
    pub terrain_scale: i32,
    pub place: Option<Placing>,
    pub hover_waypoint: Option<Pos>,
}

/// A blueprint following the pointer before it is placed. Cells, as in
/// `Command::Place`; `valid` is the answer of `World::check_place`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placing {
    pub colony: u8,
    pub kind: StructureKind,
    pub x: i32,
    pub y: i32,
    pub x2: i32,
    pub y2: i32,
    pub valid: bool,
}

/// The outline every structure kind is drawn and filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    Circle,
    Hexagon,
    Octagon,
    Square,
}

fn body_of(kind: StructureKind) -> Body {
    match kind {
        StructureKind::Queen => Body::Circle,
        StructureKind::Hatchery(_) => Body::Hexagon,
        StructureKind::BroodCell => Body::Octagon,
        StructureKind::Wall => Body::Square,
    }
}

/// The outline at radius `TRIG_ONE`, clockwise on screen.
fn body_unit(b: Body) -> Vec<Pt> {
    match b {
        Body::Circle => (0..32).map(|k| (cos64(k * 2), sin64(k * 2))).collect(),
        Body::Hexagon => vec![
            (0, -1024),
            (887, -512),
            (887, 512),
            (0, 1024),
            (-887, 512),
            (-887, -512),
        ],
        Body::Octagon => (0..8)
            .map(|k| (cos64(k * 8 + 4), sin64(k * 8 + 4)))
            .collect(),
        Body::Square => vec![(-1024, -1024), (1024, -1024), (1024, 1024), (-1024, 1024)],
    }
}

fn cross(a: (i64, i64), b: (i64, i64)) -> i64 {
    a.0 * b.1 - a.1 * b.0
}

/// Where a ray from the centre along (dx, dy) leaves a convex outline.
fn radial(unit: &[Pt], dx: i32, dy: i32) -> Pt {
    let d = (dx as i64, dy as i64);
    for i in 0..unit.len() {
        let a = (unit[i].0 as i64, unit[i].1 as i64);
        let b = unit[(i + 1) % unit.len()];
        let e = (b.0 as i64 - a.0, b.1 as i64 - a.1);
        let den = cross(d, e);
        if den == 0 {
            continue;
        }
        let s = cross(a, e);
        let t = cross(a, d);
        let ok = if den > 0 {
            s > 0 && (0..=den).contains(&t)
        } else {
            s < 0 && (den..=0).contains(&t)
        };
        if ok {
            return ((d.0 * s / den) as i32, (d.1 * s / den) as i32);
        }
    }
    (0, 0)
}

fn scaled(unit: &[Pt], x: i32, y: i32, r: i32) -> Vec<Pt> {
    unit.iter()
        .map(|&(ux, uy)| {
            (
                x + (ux as i64 * r as i64 / TRIG_ONE as i64) as i32,
                y + (uy as i64 * r as i64 / TRIG_ONE as i64) as i32,
            )
        })
        .collect()
}

/// A health or progress pie: the outline filled clockwise from the top, as
/// far as `permille` of the way round.
fn pie(b: Body, x: i32, y: i32, r: i32, permille: i32) -> Vec<Pt> {
    let unit = body_unit(b);
    if permille >= 1000 {
        return scaled(&unit, x, y, r);
    }
    if permille <= 0 {
        return Vec::new();
    }
    let start = STEPS * 3 / 4;
    let edge = |k: usize| {
        let p = radial(&unit, cos64(start + k), sin64(start + k));
        scaled(&[p], x, y, r)[0]
    };
    let total = permille as usize * STEPS;
    let (whole, rem) = (total / 1000, (total % 1000) as i32);
    let mut pts = vec![(x, y)];
    for k in 0..=whole {
        pts.push(edge(k));
    }
    if rem > 0 {
        let (a, c) = (edge(whole), edge(whole + 1));
        pts.push((
            a.0 + (c.0 - a.0) * rem / 1000,
            a.1 + (c.1 - a.1) * rem / 1000,
        ));
    }
    pts
}

fn circle(x: i32, y: i32, r: i32, n: usize) -> Vec<Pt> {
    (0..n)
        .map(|k| {
            let st = k * STEPS / n;
            (x + r * cos64(st) / TRIG_ONE, y + r * sin64(st) / TRIG_ONE)
        })
        .collect()
}

/// Builds the draw list.
struct List<'a> {
    out: Vec<i32>,
    cam: &'a Camera,
    w: i32,
    h: i32,
}

impl List<'_> {
    fn on_screen(&self, x: i32, y: i32, r: i32) -> bool {
        x + r >= 0 && y + r >= 0 && x - r <= self.w && y - r <= self.h
    }

    fn clear(&mut self, c: i32) {
        self.out.extend_from_slice(&[op::CLEAR, c]);
    }

    fn image(&mut self, id: i32, src: [i32; 4], dst: [i32; 4]) {
        self.out.extend_from_slice(&[op::IMAGE, id]);
        self.out.extend_from_slice(&src);
        self.out.extend_from_slice(&dst);
    }

    fn shape(&mut self, id: i32, x: i32, y: i32, scale: i32, fill: i32, stroke: i32, width16: i32) {
        let scale = scale.clamp(1, SCALE_MAX);
        // Stroked under the fill: half the width is hidden.
        let width = 2 * width16 * SCALE_ONE / scale;
        self.out
            .extend_from_slice(&[op::SHAPE, id, x, y, scale, fill, stroke, width]);
    }

    fn fill_shape(&mut self, id: i32, x: i32, y: i32, scale: i32, fill: i32) {
        let scale = scale.clamp(1, SCALE_MAX);
        self.out
            .extend_from_slice(&[op::FILL_SHAPE, id, x, y, scale, fill]);
    }

    fn points(&mut self, pts: &[Pt]) {
        self.out.push(pts.len() as i32);
        for &(x, y) in pts {
            self.out.push(x);
            self.out.push(y);
        }
    }

    fn poly(&mut self, fill: i32, pts: &[Pt]) {
        if pts.len() < 3 {
            return;
        }
        self.out.extend_from_slice(&[op::POLY, fill]);
        self.points(pts);
    }

    fn outline(&mut self, stroke: i32, width: i32, pts: &[Pt]) {
        if pts.len() < 2 {
            return;
        }
        self.out.extend_from_slice(&[op::OUTLINE, stroke, width]);
        self.points(pts);
    }

    fn line(&mut self, stroke: i32, width: i32, pts: &[Pt]) {
        if pts.len() < 2 {
            return;
        }
        self.out.extend_from_slice(&[op::LINE, stroke, width]);
        self.points(pts);
    }
}

/// Line width in draw-list units for outlines: thinner when zoomed out.
fn ink_width(z: i32) -> i32 {
    (z / 8).clamp(6, 24)
}

fn progress(s: &Structure) -> i32 {
    let cost: i32 = s.cost.iter().sum();
    let got: i32 = s.delivered.iter().sum();
    let paid = if cost > 0 { got * 1000 / cost } else { 1000 };
    let built = if s.work > 0 {
        s.work_done * 1000 / s.work
    } else {
        1000
    };
    (paid + built) / 2
}

/// The dark icon on a structure.
fn icon(list: &mut List, kind: StructureKind, x: i32, y: i32, scale: i32, colour: i32) {
    match kind {
        StructureKind::Queen => list.fill_shape(SH_QUEEN, x, y, scale, colour),
        StructureKind::BroodCell => list.fill_shape(SH_EGGS, x, y, scale, colour),
        StructureKind::Hatchery(s) => {
            // About 2.4 cells long, whatever the species' size.
            let k = scale * 260 / drawn_radius(s).max(1);
            list.fill_shape(ant_shape_id(s, UP, false), x, y, k, colour);
        }
        StructureKind::Wall => {}
    }
}

fn draw_structure(list: &mut List, sight: &Sight, world: &World, s: &Structure) {
    if !sight.shows(world, s.colony, s.pos) {
        return;
    }
    let cam = list.cam;
    let z = cam.units_per_cell();
    let ink = ink_width(z);
    let body = body_of(s.kind);
    let unit = body_unit(body);
    if s.kind == StructureKind::Wall && !s.complete {
        // A wall blueprint: every cell of its line, filling as it is built.
        let r = cam.length(CELL / 2);
        let p = progress(s);
        for &(cx, cy) in &s.cells {
            let (x, y) = cam.screen(Pos::of_cell(cx, cy));
            if !list.on_screen(x, y, r) {
                continue;
            }
            list.poly(soft(s.colony), &pie(body, x, y, r, p));
            list.outline(player(s.colony), ink, &scaled(&unit, x, y, r));
        }
        return;
    }
    let (x, y) = cam.screen(s.pos);
    let r = cam.length(s.radius());
    if !list.on_screen(x, y, r) {
        return;
    }
    let full = scaled(&unit, x, y, r);
    if s.complete {
        list.poly(C_BASE, &full);
        let hp = s.health.max(0) as i64 * 1000 / s.max_health.max(1) as i64;
        list.poly(player(s.colony), &pie(body, x, y, r, hp as i32));
        list.outline(C_INK, ink, &full);
        icon(list, s.kind, x, y, cam.shape_scale(), C_ICON);
    } else {
        list.poly(soft(s.colony), &pie(body, x, y, r, progress(s)));
        list.outline(player(s.colony), ink, &full);
        icon(list, s.kind, x, y, cam.shape_scale(), soft(s.colony));
    }
}

fn draw_node(list: &mut List, sight: &Sight, n: &crate::world::Node) {
    if n.amount <= 0 || !sight.knows(n.pos) {
        return;
    }
    let cam = list.cam;
    let (x, y) = cam.screen(n.pos);
    if !list.on_screen(x, y, cam.length(CELL * 2)) {
        return;
    }
    // Shrinks to 45% as it is gathered.
    let left = (n.amount as i64 * 550 / n.start.max(n.amount).max(1) as i64) as i32;
    let scale = cam.shape_scale() * (450 + left) / 1000;
    let ink = ink_width(cam.units_per_cell());
    if n.cache {
        let r = cam.length(CELL * 3 / 2) * (450 + left) / 1000;
        list.outline(C_CACHE, ink * 2, &circle(x, y, r, 24));
    }
    match n.resource {
        Resource::Food => list.shape(SH_CRUMBS, x, y, scale, C_CRUMB, C_INK, ink),
        Resource::Nectar => {
            list.shape(SH_PETALS, x, y, scale, C_PETAL, C_INK, ink);
            list.shape(SH_POLLEN, x, y, scale, C_POLLEN, C_INK, ink);
        }
        Resource::Leaves => list.shape(SH_LEAVES, x, y, scale, C_LEAF, C_INK, ink),
    }
}

fn draw_effect(list: &mut List, e: &crate::world::Effect, blasts: bool) {
    let cam = list.cam;
    let (x, y) = cam.screen(e.to);
    let r = cam.length(CELL * 2);
    if !list.on_screen(x, y, r) {
        return;
    }
    let px = SUB_PX;
    match (e.kind, blasts) {
        (EffectKind::Blast, true) => list.poly(C_BLAST, &circle(x, y, r * 4 / 5, 20)),
        (EffectKind::Bite, false) => {
            let d = cam.length(CELL * 2 / 5);
            list.line(C_BITE, px * 3 / 2, &[(x - d, y - d), (x + d, y + d)]);
            list.line(C_BITE, px * 3 / 2, &[(x - d, y + d), (x + d, y - d)]);
        }
        (EffectKind::Spray, false) => {
            let from = cam.screen(e.from);
            list.line(C_ACID, px * 2, &[from, (x, y)]);
            list.poly(C_ACID, &circle(x, y, cam.length(CELL / 3), 8));
        }
        (EffectKind::Heal, false) => {
            let from = cam.screen(e.from);
            let d = cam.length(CELL / 3);
            list.line(C_HEAL, px * 3 / 2, &[from, (x, y)]);
            list.line(C_HEAL, px * 2, &[(x - d, y), (x + d, y)]);
            list.line(C_HEAL, px * 2, &[(x, y - d), (x, y + d)]);
        }
        _ => {}
    }
}

/// Draws the part of a whole-map image (at `per_cell` pixels per cell)
/// that is in view, a cell beyond each edge so smoothing has its
/// neighbours.
fn map_image(list: &mut List, id: i32, per_cell: i32, mw: i32, mh: i32) {
    let cam = list.cam;
    let tl = cam.world_at(0, 0);
    let br = cam.world_at(cam.view_w, cam.view_h);
    let x0 = (tl.x.div_euclid(CELL) - 1).clamp(0, mw);
    let y0 = (tl.y.div_euclid(CELL) - 1).clamp(0, mh);
    let x1 = (br.x.div_euclid(CELL) + 2).clamp(0, mw);
    let y1 = (br.y.div_euclid(CELL) + 2).clamp(0, mh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let a = cam.screen(Pos::new(x0 * CELL, y0 * CELL));
    let b = cam.screen(Pos::new(x1 * CELL, y1 * CELL));
    list.image(
        id,
        [
            x0 * per_cell,
            y0 * per_cell,
            (x1 - x0) * per_cell,
            (y1 - y0) * per_cell,
        ],
        [a.0, a.1, b.0 - a.0, b.1 - a.1],
    );
}

/// One frame's draw list for `viewer` (`None` sees everything). Ant
/// positions are interpolated `interp_permille` of the way from the last
/// tick to this one. Off-screen things are left out, and enemy things only
/// appear where the viewer's team can see now.
pub fn frame(
    world: &World,
    viewer: Option<u8>,
    cam: &Camera,
    overlay: &Overlay,
    interp_permille: i32,
) -> Vec<i32> {
    let interp = interp_permille.clamp(0, 1000) as i64;
    let mut list = List {
        out: Vec::with_capacity(16 * 1024),
        cam,
        w: cam.view_w * SUB_PX,
        h: cam.view_h * SUB_PX,
    };
    let sight = Sight::new(world, viewer);
    let z = cam.units_per_cell();
    let ink = ink_width(z);
    let (mw, mh) = (world.map.w, world.map.h);

    list.clear(C_BACKDROP);

    let ts = if overlay.terrain_scale > 0 {
        overlay.terrain_scale
    } else {
        TERRAIN_SCALE
    };
    map_image(&mut list, IMAGE_TERRAIN, ts, mw, mh);

    let hill = world.events.hill.as_ref().map(|h| {
        let (x, y) = cam.screen(Pos::of_cell(h.x, h.y));
        circle(x, y, cam.length(h.r * CELL), 48)
    });
    if let Some(ring) = &hill {
        list.poly(C_HILL_FILL, ring);
    }

    for n in &world.nodes {
        draw_node(&mut list, &sight, n);
    }
    for s in &world.structures {
        draw_structure(&mut list, &sight, world, s);
    }
    let effect_shown = |e: &crate::world::Effect| sight.shows(world, e.colony, e.to);
    for e in world.effects.iter().filter(|e| effect_shown(e)) {
        draw_effect(&mut list, e, true);
    }

    // Ants.
    let detailed = z >= DETAIL_ZOOM;
    let base_scale = cam.shape_scale();
    for a in &world.ants {
        let p = Pos::new(
            a.prev.x + ((a.pos.x - a.prev.x) as i64 * interp / 1000) as i32,
            a.prev.y + ((a.pos.y - a.prev.y) as i64 * interp / 1000) as i32,
        );
        let (x, y) = cam.screen(p);
        let r = cam.length(drawn_radius(a.species) * 3).max(z);
        if !list.on_screen(x, y, r) || !sight.shows(world, a.colony, p) {
            continue;
        }
        let id = ant_shape_id(a.species, a.heading, detailed);
        if z >= OUTLINE_ZOOM {
            list.shape(id, x, y, base_scale, player(a.colony), C_INK, ink * 2 / 3);
        } else {
            // Too small for an outline: it would only darken the colour.
            list.fill_shape(id, x, y, base_scale, player(a.colony));
        }
        let carried = a.carried();
        if carried > 0 && z >= 48 {
            let most = (0..3).max_by_key(|&i| a.inventory[i]).unwrap_or(0);
            let colour = [C_CRUMB, C_POLLEN, C_LEAF][most];
            let step = a.heading as usize * (STEPS / HEADINGS);
            let d = cam.length(drawn_radius(a.species) * 7 / 10);
            let (hx, hy) = (
                x + d * cos64(step) / TRIG_ONE,
                y + d * sin64(step) / TRIG_ONE,
            );
            list.poly(
                colour,
                &circle(hx, hy, cam.length(drawn_radius(a.species) * 2 / 5), 8),
            );
        }
    }

    for e in world.effects.iter().filter(|e| effect_shown(e)) {
        draw_effect(&mut list, e, false);
    }

    if !sight.all {
        map_image(&mut list, IMAGE_FOG, 1, mw, mh);
    }

    if let Some(ring) = &hill {
        list.outline(C_HILL, SUB_PX * 3, ring);
    }

    if let Some(v) = viewer.filter(|v| (*v as usize) < world.colonies.len()) {
        draw_waypoints(&mut list, world, v, overlay.hover_waypoint);
    }

    if let Some(pl) = &overlay.place {
        let colour = if pl.valid { C_VALID } else { C_INVALID };
        let unit = body_unit(body_of(pl.kind));
        if pl.kind == StructureKind::Wall {
            let r = cam.length(CELL / 2);
            for (cx, cy) in World::wall_cells(pl.x, pl.y, pl.x2, pl.y2) {
                let (x, y) = cam.screen(Pos::of_cell(cx, cy));
                let sq = scaled(&unit, x, y, r);
                list.poly(colour, &sq);
                list.outline(player(pl.colony), ink, &sq);
            }
        } else {
            let (x, y) = cam.screen(Pos::of_cell(pl.x, pl.y));
            let shape = scaled(&unit, x, y, cam.length(pl.kind.stats().radius));
            list.poly(colour, &shape);
            list.outline(player(pl.colony), ink, &shape);
            icon(&mut list, pl.kind, x, y, base_scale, C_ICON);
        }
    }
    list.out
}

fn draw_waypoints(list: &mut List, world: &World, viewer: u8, hover: Option<Pos>) {
    let cam = list.cam;
    let z = cam.units_per_cell();
    let ink = ink_width(z);
    let mut at: BTreeMap<(i32, i32), Vec<Species>> = BTreeMap::new();
    for (si, points) in world.colonies[viewer as usize].waypoints.iter().enumerate() {
        for p in points {
            if let Some(s) = Species::from_index(si) {
                at.entry((p.x, p.y)).or_default().push(s);
            }
        }
    }
    let colour = player(viewer);
    for ((wx, wy), species) in at {
        let (x, y) = cam.screen(Pos::new(wx, wy));
        let pole = cam.length(CELL * 2).max(SUB_PX * 14);
        let reach = pole * 3;
        if !list.on_screen(x, y, reach) {
            continue;
        }
        if hover == Some(Pos::new(wx, wy)) {
            list.outline(C_HOVER, SUB_PX * 2, &circle(x, y, pole * 3 / 4, 24));
        }
        list.line(C_POLE, ink * 2, &[(x, y), (x, y - pole)]);
        let flag = [
            (x, y - pole),
            (x + pole * 2 / 3, y - pole + pole / 4),
            (x, y - pole + pole / 2),
        ];
        list.poly(colour, &flag);
        list.outline(C_INK, ink, &flag);
        list.poly(C_INK, &circle(x, y, pole / 8, 8));
        // A species icon for each species sent here, four to a row.
        let gap = pole * 3 / 4;
        for (i, s) in species.iter().enumerate() {
            let (col, row) = ((i % 4) as i32, (i / 4) as i32);
            let ix = x + gap * col - gap * 3 / 2;
            let iy = y + gap * (row + 1);
            // Shapes are about 2.3 drawn radii long; this makes the icon
            // about 0.9 of the gap.
            let k = gap * 420 / drawn_radius(*s).max(1);
            let id = ant_shape_id(*s, UP, z >= DETAIL_ZOOM);
            list.shape(id, ix, iy, k, colour, C_INK, ink);
        }
    }
}

// ---- minimap ----

/// The overview at `size` × `size` pixels (RGBA): ground, Fog of War, a dot
/// per ant and structure in its player colour, and the camera's view as a
/// white rectangle.
pub fn minimap(world: &World, viewer: Option<u8>, cam: &Camera, size: i32) -> Vec<u8> {
    let size = size.clamp(8, 1024);
    let (w, h) = (world.map.w, world.map.h);
    let m = w.max(h);
    // Map to minimap: pixel = offset + cell * size / m.
    let (ox, oy) = ((m - w) * size / (2 * m), (m - h) * size / (2 * m));
    let sight = Sight::new(world, viewer);
    let mut out = vec![0u8; (size * size * 4) as usize];
    let put = |out: &mut Vec<u8>, x: i32, y: i32, c: [u8; 3]| {
        if x >= 0 && y >= 0 && x < size && y < size {
            let i = ((y * size + x) * 4) as usize;
            out[i] = c[0];
            out[i + 1] = c[1];
            out[i + 2] = c[2];
            out[i + 3] = 255;
        }
    };
    let flower = biome_of(world) == Biome::FlowerField;
    for py in 0..size {
        for px in 0..size {
            let cx = (px - ox) * m / size;
            let cy = (py - oy) * m / size;
            let c = if !world.map.inside(cx, cy) {
                [12, 9, 7]
            } else {
                let i = world.map.idx(cx, cy);
                let base = match world.map.ground[i] {
                    Ground::Floor => [150, 114, 78],
                    Ground::Dirt => [206, 154, 88],
                    Ground::Rock if flower => [50, 92, 42],
                    Ground::Rock => [176, 110, 104],
                };
                if sight.all || sight.visible.get(i) {
                    base
                } else if sight.explored.get(i) {
                    [base[0] / 2, base[1] / 2, base[2] / 2]
                } else {
                    [12, 9, 7]
                }
            };
            put(&mut out, px, py, c);
        }
    }
    let to_px = |p: Pos| {
        (
            ox + (p.x as i64 * size as i64 / (m as i64 * CELL as i64)) as i32,
            oy + (p.y as i64 * size as i64 / (m as i64 * CELL as i64)) as i32,
        )
    };
    for s in &world.structures {
        if sight.shows(world, s.colony, s.pos) {
            let (x, y) = to_px(s.pos);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    put(&mut out, x + dx, y + dy, PLAYER_RGB[s.colony as usize % 4]);
                }
            }
        }
    }
    for a in &world.ants {
        if sight.shows(world, a.colony, a.pos) {
            let (x, y) = to_px(a.pos);
            put(&mut out, x, y, PLAYER_RGB[a.colony as usize % 4]);
        }
    }
    let (x0, y0) = to_px(cam.world_at(0, 0));
    let (x1, y1) = to_px(cam.world_at(cam.view_w, cam.view_h));
    let (x0, x1) = (x0.max(0), x1.min(size - 1));
    let (y0, y1) = (y0.max(0), y1.min(size - 1));
    for x in x0..=x1 {
        put(&mut out, x, y0, [255, 255, 255]);
        put(&mut out, x, y1, [255, 255, 255]);
    }
    for y in y0..=y1 {
        put(&mut out, x0, y, [255, 255, 255]);
        put(&mut out, x1, y, [255, 255, 255]);
    }
    out
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
