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
/// From this zoom ants carry their dark role mark.
const MARK_ZOOM: i32 = 96;

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
pub const PALETTE: [&str; 29] = [
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
    "#7a5c3e",
    "#ffb347",
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
const C_FLOOR: i32 = 27;
const C_BLAST_RING: i32 = 28;

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

/// A closed outline from its upper half (y ≤ 0, nose to tail or tail to
/// nose): the lower half is the mirror image.
fn mirrored(half: &[Pt]) -> Vec<Pt> {
    let mut v: Vec<Pt> = half.to_vec();
    v.extend(half.iter().rev().map(|&(x, y)| (x, -y)));
    v
}

/// A thick open polyline as one bar per segment.
fn bars(parts: &mut Shape, pts: &[Pt], t: i32) {
    for w in pts.windows(2) {
        parts.push(bar(w[0], w[1], t));
    }
}

/// A star with `n` points around (cx, cy), radii `outer` and `inner`.
fn star(cx: i32, cy: i32, outer: i32, inner: i32, n: usize) -> Vec<Pt> {
    (0..2 * n)
        .map(|k| {
            let r = if k % 2 == 0 { outer } else { inner };
            let st = k * STEPS / (2 * n);
            (cx + r * cos64(st) / TRIG_ONE, cy + r * sin64(st) / TRIG_ONE)
        })
        .collect()
}

/// A chevron pointing forward (east) with its tip at x.
fn chevron(x: i32, reach: i32, wide: i32, t: i32) -> Vec<Pt> {
    mirrored(&[
        (x, 0),
        (x - reach, -wide),
        (x - reach - t, -wide),
        (x - t, 0),
    ])
}

/// One species, in template units (body radius 100, facing east): the body
/// in the player colour, the dark role mark drawn on it, where the head is
/// (centre x, radius) for the antennae, and how far the legs spread
/// (percent).
struct Parts {
    body: Shape,
    mark: Shape,
    head: (i32, i32),
    legs: i32,
}

/// Small pincers in front of a head whose front is at x.
fn small_jaws(body: &mut Shape, front: i32) {
    for side in [1, -1] {
        body.push(bar((front - 4, 8 * side), (front + 14, 4 * side), 8));
    }
}

/// Every species has a silhouette of its own, readable at 8 pixels per cell:
/// workers are small and plain, fighters bigger and armed, and each role
/// carries a dark mark (a crumb for Gatherers, a beam for Builders, a
/// chevron for Melee, a droplet for Ranged, a cross for Support, shell
/// plates for the Tank, wing veins for the Flyer, a burst for the Bomber
/// and a spine for the Brute).
fn species_parts(s: Species) -> Parts {
    let mut body: Shape = Vec::new();
    let mut mark: Shape = Vec::new();
    let waist = |body: &mut Shape, x: i32| body.push(ellipse(x, 0, 10, 8, 0, 8));
    let (head, legs) = match s {
        Species::Harvester => {
            body.push(ellipse(-70, 0, 46, 34, 0, 16));
            waist(&mut body, -20);
            body.push(ellipse(2, 0, 22, 13, 0, 12));
            body.push(ellipse(38, 0, 18, 17, 0, 14));
            small_jaws(&mut body, 56);
            // A crumb.
            mark.push(ellipse(-72, -3, 19, 15, 6, 10));
            mark.push(ellipse(-56, 9, 9, 8, 0, 8));
            ((38, 18), 100)
        }
        Species::Carpenter => {
            // A long box of an abdomen and a square head as wide as it.
            body.push(mirrored(&[
                (-30, 0),
                (-30, -26),
                (-40, -36),
                (-118, -36),
                (-128, -26),
                (-128, 0),
            ]));
            waist(&mut body, -22);
            body.push(ellipse(0, 0, 22, 16, 0, 12));
            body.push(mirrored(&[
                (72, 0),
                (72, -28),
                (64, -36),
                (26, -36),
                (18, -28),
                (18, 0),
            ]));
            for side in [1, -1] {
                bars(
                    &mut body,
                    &[(64, 26 * side), (90, 24 * side), (100, 8 * side)],
                    13,
                );
            }
            // A beam carried across the back.
            mark.push(bar((-2, -62), (-2, 62), 22));
            mark.push(bar((-80, -24), (-80, 24), 12));
            ((46, 26), 100)
        }
        Species::Soldier => {
            body.push(ellipse(-74, 0, 44, 32, 0, 16));
            waist(&mut body, -24);
            body.push(ellipse(0, 0, 24, 15, 0, 12));
            body.push(ellipse(50, 0, 30, 30, 0, 14));
            for side in [1, -1] {
                body.push(vec![
                    (70, 20 * side),
                    (98, 26 * side),
                    (120, 4 * side),
                    (100, 12 * side),
                    (74, 8 * side),
                ]);
            }
            mark.push(chevron(-50, 30, 24, 14));
            ((50, 30), 105)
        }
        Species::Leafcutter => {
            body.push(ellipse(-66, 0, 40, 30, 0, 16));
            waist(&mut body, -18);
            body.push(ellipse(0, 0, 22, 14, 0, 12));
            body.push(ellipse(36, 0, 20, 18, 0, 14));
            // The leaf it carries, held up over its head, with its veins.
            body.push(lens((24, 0), 130, 84, 0));
            bars(&mut mark, &[(36, 0), (146, 0)], 9);
            for (x, l) in [(62, 26), (96, 22)] {
                for side in [1, -1] {
                    mark.push(bar((x, 0), (x + 20, l * side), 7));
                }
            }
            ((36, 20), 95)
        }
        Species::Fire => {
            // Slim and wasp-waisted, with a sting.
            body.push(ellipse(-62, 0, 44, 20, 0, 16));
            body.push(vec![(-100, -8), (-140, 0), (-100, 8)]);
            waist(&mut body, -14);
            body.push(ellipse(2, 0, 22, 11, 0, 12));
            body.push(ellipse(36, 0, 17, 15, 0, 14));
            small_jaws(&mut body, 53);
            mark.push(bar((-50, -22), (-50, 22), 12));
            mark.push(bar((-78, -20), (-78, 20), 12));
            ((36, 17), 90)
        }
        Species::Turtle => {
            // A round shell over thorax and abdomen, and a flat shield head.
            body.push(ellipse(-34, 0, 74, 62, 0, 20));
            body.push(ellipse(46, 0, 18, 36, 0, 14));
            body.push(ellipse(22, 0, 18, 18, 0, 10));
            let hex: Vec<Pt> = (0..7)
                .map(|k| {
                    let p = rotate((34, 0), k * STEPS / 6);
                    (p.0 - 34, p.1)
                })
                .collect();
            bars(&mut mark, &hex, 11);
            for k in 0..6 {
                let a = rotate((34, 0), k * STEPS / 6);
                let b = rotate((62, 0), k * STEPS / 6);
                mark.push(bar((a.0 - 34, a.1), (b.0 * 9 / 10 - 34, b.1 * 4 / 5), 9));
            }
            ((46, 18), 125)
        }
        Species::Acid => {
            // A swollen abdomen drawn out to a long spraying tip.
            body.push(ellipse(-62, 0, 46, 38, 0, 16));
            body.push(vec![(-92, -24), (-162, 0), (-92, 24)]);
            waist(&mut body, -16);
            body.push(ellipse(2, 0, 22, 14, 0, 12));
            body.push(ellipse(40, 0, 20, 19, 0, 14));
            small_jaws(&mut body, 60);
            mark.push(ellipse(-54, 0, 17, 17, 0, 12));
            mark.push(vec![(-56, -16), (-104, 0), (-56, 16)]);
            ((40, 20), 100)
        }
        Species::Nurse => {
            body.push(ellipse(-66, 0, 48, 48, 0, 18));
            waist(&mut body, -14);
            body.push(ellipse(4, 0, 22, 15, 0, 12));
            body.push(ellipse(40, 0, 19, 18, 0, 14));
            small_jaws(&mut body, 59);
            mark.push(bar((-66, -34), (-66, 34), 20));
            mark.push(bar((-100, 0), (-32, 0), 20));
            ((40, 19), 100)
        }
        Species::Flying => {
            body.push(ellipse(-74, 0, 46, 26, 0, 16));
            waist(&mut body, -24);
            body.push(ellipse(2, 0, 28, 18, 0, 12));
            body.push(ellipse(46, 0, 20, 19, 0, 14));
            small_jaws(&mut body, 66);
            for (side, rot) in [(1, STEPS - 5), (-1, 5)] {
                body.push(ellipse(-48, 46 * side, 84, 26, rot, 16));
                let a = rotate((64, 0), rot);
                let b = rotate((-70, 0), rot);
                mark.push(bar(
                    (a.0 - 48, a.1 + 46 * side),
                    (b.0 - 48, b.1 + 46 * side),
                    8,
                ));
            }
            ((46, 20), 95)
        }
        Species::Exploding => {
            body.push(ellipse(-62, 0, 58, 58, 0, 18));
            for k in 0..6 {
                let st = k * STEPS / 6 + 5;
                let a = rotate((52, -14), st);
                let b = rotate((90, 0), st);
                let c = rotate((52, 14), st);
                body.push(vec![(a.0 - 62, a.1), (b.0 - 62, b.1), (c.0 - 62, c.1)]);
            }
            waist(&mut body, -4);
            body.push(ellipse(10, 0, 20, 14, 0, 12));
            body.push(ellipse(44, 0, 18, 17, 0, 14));
            small_jaws(&mut body, 62);
            mark.push(star(-62, 0, 36, 14, 8));
            ((44, 18), 100)
        }
        Species::TrapJaw => {
            body.push(ellipse(-70, 0, 44, 32, 0, 16));
            waist(&mut body, -20);
            body.push(ellipse(2, 0, 26, 16, 0, 12));
            body.push(ellipse(48, 0, 26, 22, 0, 14));
            for side in [1, -1] {
                body.push(bar((66, 10 * side), (136, 16 * side), 10));
                body.push(bar((130, 16 * side), (138, 2 * side), 9));
            }
            mark.push(chevron(-44, 26, 22, 12));
            mark.push(chevron(-70, 26, 22, 12));
            ((48, 26), 105)
        }
        Species::Dinosaur => {
            // A huge armoured brute with spikes down both flanks and tusks.
            body.push(ellipse(-88, 0, 60, 40, 0, 18));
            for x in [-122, -90, -58] {
                let w = if x == -90 { 36 } else { 30 };
                for side in [1, -1] {
                    body.push(vec![
                        (x - 14, w * side),
                        (x + 4, (w + 30) * side),
                        (x + 12, w * side),
                    ]);
                }
            }
            waist(&mut body, -24);
            body.push(ellipse(0, 0, 34, 22, 0, 12));
            for side in [1, -1] {
                body.push(vec![(-12, 18 * side), (4, 46 * side), (14, 18 * side)]);
            }
            body.push(ellipse(60, 0, 30, 28, 0, 14));
            for side in [1, -1] {
                bars(
                    &mut body,
                    &[(78, 14 * side), (104, 24 * side), (122, 10 * side)],
                    13,
                );
            }
            for x in [-124, -92, -60, -4] {
                mark.push(mirrored(&[(x + 15, 0), (x, -10), (x - 15, 0)]));
            }
            ((60, 30), 112)
        }
    };
    Parts {
        body,
        mark,
        head,
        legs,
    }
}

/// The parts of a species' body in template units (body radius 100, facing
/// east); the abdomen is the first part. `detailed` adds legs and antennae.
fn ant_template(s: Species, detailed: bool) -> Shape {
    let p = species_parts(s);
    let mut parts = p.body;
    if detailed {
        let (hx, hr) = p.head;
        for side in [1, -1] {
            let legs = [
                [(10, 6), (32, 40), (58, 58)],
                [(0, 8), (-2, 46), (-18, 70)],
                [(-12, 6), (-36, 40), (-70, 62)],
            ];
            for leg in legs {
                let q: Vec<Pt> = leg
                    .iter()
                    .map(|&(x, y)| (x * p.legs / 100, y * side * p.legs / 100))
                    .collect();
                parts.push(bar(q[0], q[1], 8));
                parts.push(bar(q[1], q[2], 7));
            }
            let a0 = (hx + hr / 2, 6 * side);
            let a1 = (hx + hr + 16, 26 * side);
            let a2 = (hx + hr + 42, 24 * side);
            parts.push(bar(a0, a1, 6));
            parts.push(bar(a1, a2, 5));
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

/// The drawn body radius of a species, in sub-units: more than its physical
/// radius, so small ants stay readable, and more for the stronger species
/// (by role and Pop Cost), so size alone tells workers from fighters.
fn drawn_radius(s: Species) -> i32 {
    let pct = match s {
        Species::Harvester | Species::Leafcutter => 124,
        Species::Fire => 132,
        Species::Carpenter => 140,
        Species::Soldier => 146,
        Species::Turtle => 140,
        Species::TrapJaw => 144,
        Species::Acid | Species::Nurse | Species::Flying | Species::Exploding => 150,
        Species::Dinosaur => 120,
    };
    s.stats().radius * pct / 100
}

fn heading_step(heading: u8) -> usize {
    (heading as usize % HEADINGS) * (STEPS / HEADINGS)
}

/// One species facing one of 16 headings, in sub-units.
pub fn ant_shape(s: Species, heading: u8, detailed: bool) -> Shape {
    transform(
        &ant_template(s, detailed),
        drawn_radius(s),
        100,
        heading_step(heading),
    )
}

/// A species' dark role mark facing one of 16 headings, in sub-units: drawn
/// over the body from `MARK_ZOOM`.
pub fn mark_shape(s: Species, heading: u8) -> Shape {
    transform(
        &species_parts(s).mark,
        drawn_radius(s),
        100,
        heading_step(heading),
    )
}

/// Shape ids: ants first, `[plain, detailed, mark][species][heading]`,
/// then the rest.
pub fn ant_shape_id(s: Species, heading: u8, detailed: bool) -> i32 {
    ((detailed as usize * Species::ALL.len() + s.index()) * HEADINGS + heading as usize % HEADINGS)
        as i32
}

pub fn mark_shape_id(s: Species, heading: u8) -> i32 {
    ((2 * Species::ALL.len() + s.index()) * HEADINGS + heading as usize % HEADINGS) as i32
}
const ANT_SHAPES: i32 = (3 * Species::ALL.len() * HEADINGS) as i32;
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
    for s in Species::ALL {
        for h in 0..HEADINGS {
            out.push(mark_shape(s, h as u8));
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

/// How much of a blueprint's build work is done, in permille: its progress
/// pie, which grows a little with every tick of work.
fn built(s: &Structure) -> i32 {
    if s.work > 0 {
        (s.work_done as i64 * 1000 / s.work as i64) as i32
    } else {
        1000
    }
}

/// How much of a blueprint's cost has been delivered, in permille: the bright
/// rim drawn round it as resources arrive.
fn paid(s: &Structure) -> i32 {
    let cost: i32 = s.cost.iter().sum();
    let got: i32 = s.delivered.iter().sum();
    if cost > 0 {
        (got as i64 * 1000 / cost as i64) as i32
    } else {
        1000
    }
}

/// Cell `k` of `n` in a wall line's share of a whole-line permille: the line
/// fills cell by cell.
fn cell_share(permille: i32, k: i32, n: i32) -> i32 {
    (permille * n - 1000 * k).clamp(0, 1000)
}

/// A blueprint's outline: thin and dark, with the delivered part (from the
/// top, clockwise) traced over it in the player colour.
fn delivery_rim(list: &mut List, b: Body, at: Pt, r: i32, paid: i32, colony: u8) {
    let ink = ink_width(list.cam.units_per_cell());
    let unit = body_unit(b);
    list.outline(C_INK, ink, &scaled(&unit, at.0, at.1, r));
    if paid >= 1000 {
        list.outline(player(colony), ink * 2, &scaled(&unit, at.0, at.1, r));
    } else if paid > 0 {
        let rim = pie(b, at.0, at.1, r, paid);
        list.line(player(colony), ink * 2, &rim[1..]);
    }
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
        let (work, got, n) = (built(s), paid(s), s.cells.len() as i32);
        for (k, &(cx, cy)) in s.cells.iter().enumerate() {
            let (x, y) = cam.screen(Pos::of_cell(cx, cy));
            if !list.on_screen(x, y, r) {
                continue;
            }
            let k = k as i32;
            list.poly(soft(s.colony), &pie(body, x, y, r, cell_share(work, k, n)));
            delivery_rim(list, body, (x, y), r, cell_share(got, k, n), s.colony);
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
        list.poly(soft(s.colony), &pie(body, x, y, r, built(s)));
        delivery_rim(list, body, (x, y), r, paid(s), s.colony);
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

/// Ticks an effect of this kind lasts (as `World::strike` and the Nurse
/// Ant's heal set `until`).
fn effect_life(kind: EffectKind) -> i32 {
    match kind {
        EffectKind::Bite | EffectKind::Spray => 2,
        EffectKind::Heal => 3,
        EffectKind::Blast => 4,
    }
}

/// How far through its life an effect is, in permille, between ticks.
fn effect_phase(e: &crate::world::Effect, tick: i32, interp: i32) -> i32 {
    let life = effect_life(e.kind);
    let age = (life - (e.until - tick)).clamp(0, life - 1);
    ((age * 1000 + interp) / life).clamp(0, 999)
}

/// The Splash radius of a blast in sub-units: a Dinosaur Ant's if one of the
/// colony's stands where the blast came from, otherwise an Exploding Ant's,
/// with the colony's upgrades.
fn blast_radius(world: &World, e: &crate::world::Effect) -> i32 {
    let reach = (2 * CELL) as i64;
    let dino = world.ants.iter().any(|a| {
        a.colony == e.colony
            && a.species == Species::Dinosaur
            && a.pos.dist2(e.from) <= reach * reach
    });
    let s = if dino {
        Species::Dinosaur
    } else {
        Species::Exploding
    };
    match world.colonies.get(e.colony as usize) {
        Some(c) => c.ranks.apply(s).splash,
        None => s.stats().splash,
    }
}

/// The unit vector (scaled by `TRIG_ONE`) from a to b; east when they meet.
fn unit(a: Pt, b: Pt) -> Pt {
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let len = isqrt((dx * dx + dy * dy) as u64) as i64;
    if len == 0 {
        return (TRIG_ONE, 0);
    }
    (
        (dx * TRIG_ONE as i64 / len) as i32,
        (dy * TRIG_ONE as i64 / len) as i32,
    )
}

/// p + u·a + v·b, where u and v are unit vectors scaled by `TRIG_ONE`.
fn along(p: Pt, u: Pt, a: i32, v: Pt, b: i32) -> Pt {
    (
        p.0 + (u.0 * a + v.0 * b) / TRIG_ONE,
        p.1 + (u.1 * a + v.1 * b) / TRIG_ONE,
    )
}

/// A plus sign of half-size `d` and arm thickness `t` around (x, y).
fn plus(x: i32, y: i32, d: i32, t: i32) -> Vec<Pt> {
    let h = t / 2;
    vec![
        (x - h, y - d),
        (x + h, y - d),
        (x + h, y - h),
        (x + d, y - h),
        (x + d, y + h),
        (x + h, y + h),
        (x + h, y + d),
        (x - h, y + d),
        (x - h, y + h),
        (x - d, y + h),
        (x - d, y - h),
        (x - h, y - h),
    ]
}

/// One hit, `phase` permille through its life. `under` is the pass drawn
/// below the ants (the blast's flash on the ground); the rest is drawn over
/// them. Each kind has its own look: a Bite snaps a pair of jaws shut in the
/// attacker's colour with a white flash on the target, a Spray lobs a green
/// droplet that splats, a Blast throws out a ring as wide as its Splash and
/// a Heal sends a green plus rising from the healed ant.
fn draw_effect(list: &mut List, e: &crate::world::Effect, phase: i32, splash: i32, under: bool) {
    let cam = list.cam;
    let z = cam.units_per_cell();
    let to = cam.screen(e.to);
    let reach = cam.length(splash.max(CELL * 2));
    if !list.on_screen(to.0, to.1, reach) {
        return;
    }
    let px = SUB_PX;
    let fade = 1000 - phase;
    match (e.kind, under) {
        (EffectKind::Blast, true) => {
            if phase < 500 {
                let r = cam.length(splash) * (500 + phase) / 1000;
                list.poly(C_BLAST, &circle(to.0, to.1, r, 24));
            }
        }
        (EffectKind::Blast, false) => {
            let r = cam.length(splash) * (300 + phase * 7 / 10) / 1000;
            let w = (px * 4 * fade / 1000).max(px);
            list.outline(C_BLAST_RING, w, &circle(to.0, to.1, r.max(px * 2), 24));
        }
        (EffectKind::Bite, false) => {
            let l = cam.length(CELL * 3 / 4).max(px * 4);
            if phase < 600 {
                let r = l * (750 - phase) / 1000 + px;
                list.poly(C_BITE, &star(to.0, to.1, r, r / 3, 4));
            }
            if z < OUTLINE_ZOOM {
                return;
            }
            // Two jaws either side of the target, closing as the bite lands.
            let from = cam.screen(e.from);
            let u = unit(from, to);
            let v = (-u.1, u.0);
            let open = l * (300 + fade * 5 / 10) / 1000;
            let base = along(to, u, -l * 3 / 4, v, 0);
            for side in [1, -1] {
                let jaw = [
                    along(base, u, 0, v, side * open / 2),
                    along(base, u, l * 2 / 3, v, side * (open + l / 4)),
                    along(base, u, l * 3 / 2, v, side * open / 3),
                ];
                list.line(C_INK, px * 3, &jaw);
                list.line(player(e.colony), px * 3 / 2, &jaw);
            }
        }
        (EffectKind::Spray, false) => {
            let from = cam.screen(e.from);
            // The droplet flies for the first 60% of the effect, on a lob a
            // quarter as high as it is long, then splats.
            let (dx, dy) = ((to.0 - from.0) as i64, (to.1 - from.1) as i64);
            let height = (isqrt((dx * dx + dy * dy) as u64) / 4) as i32;
            let at = |t: i32| {
                let lift = (height as i64 * 4 * t as i64 * (1000 - t) as i64 / 1_000_000) as i32;
                (
                    from.0 + (dx * t as i64 / 1000) as i32,
                    from.1 + (dy * t as i64 / 1000) as i32 - lift,
                )
            };
            let t = (phase * 1000 / 600).min(1000);
            let drop = cam.length(CELL / 6).max(px * 2);
            if t < 1000 {
                let trail: Vec<Pt> = (0..=4).map(|k| at((t - 400 + k * 100).max(0))).collect();
                list.line(C_ACID, (drop * 2 / 3).max(px), &trail);
                let p = at(t);
                list.poly(C_ACID, &circle(p.0, p.1, drop, 8));
            } else {
                let r = drop + cam.length(CELL / 2) * (phase - 600) / 400;
                list.poly(C_ACID, &star(to.0, to.1, r, r / 2, 6));
            }
        }
        (EffectKind::Heal, false) => {
            if phase < 400 {
                let from = cam.screen(e.from);
                list.line(C_HEAL, px, &[from, to]);
            }
            let rise = cam.length(CELL * 3 / 4) * phase / 1000;
            let d = cam.length(CELL / 3).max(px * 3);
            let p = plus(to.0, to.1 - rise, d, d * 3 / 4);
            list.poly(C_HEAL, &p);
            list.outline(C_INK, px, &p);
        }
        _ => {}
    }
}

/// One ant at the camera's zoom: its body in `fill`, a dark outline once
/// the zoom allows, and its role mark from `MARK_ZOOM`.
fn draw_ant(list: &mut List, s: Species, heading: u8, at: Pt, scale: i32, fill: i32) {
    let z = list.cam.units_per_cell();
    let id = ant_shape_id(s, heading, z >= DETAIL_ZOOM);
    if z >= OUTLINE_ZOOM {
        list.shape(id, at.0, at.1, scale, fill, C_INK, ink_width(z) * 2 / 3);
    } else {
        // Too small for an outline: it would only darken the colour.
        list.fill_shape(id, at.0, at.1, scale, fill);
    }
    if z >= MARK_ZOOM {
        list.fill_shape(mark_shape_id(s, heading), at.0, at.1, scale, C_INK);
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
    // Hits on screen, with how far through its life each is and, for a
    // blast, its Splash radius.
    let effects: Vec<(&crate::world::Effect, i32, i32)> = world
        .effects
        .iter()
        .filter(|e| sight.shows(world, e.colony, e.to))
        .map(|e| {
            let splash = if e.kind == EffectKind::Blast {
                blast_radius(world, e)
            } else {
                0
            };
            (e, effect_phase(e, world.tick, interp as i32), splash)
        })
        .collect();
    for &(e, phase, splash) in &effects {
        draw_effect(&mut list, e, phase, splash, true);
    }

    // Ants.
    let base_scale = cam.shape_scale();
    let radii = Species::ALL.map(drawn_radius);
    for a in &world.ants {
        let p = Pos::new(
            a.prev.x + ((a.pos.x - a.prev.x) as i64 * interp / 1000) as i32,
            a.prev.y + ((a.pos.y - a.prev.y) as i64 * interp / 1000) as i32,
        );
        let (x, y) = cam.screen(p);
        let radius = radii[a.species.index()];
        let r = cam.length(radius * 3).max(z);
        if !list.on_screen(x, y, r) || !sight.shows(world, a.colony, p) {
            continue;
        }
        draw_ant(
            &mut list,
            a.species,
            a.heading,
            (x, y),
            base_scale,
            player(a.colony),
        );
        let carried = a.carried();
        if carried > 0 && z >= 48 {
            let most = (0..3).max_by_key(|&i| a.inventory[i]).unwrap_or(0);
            let colour = [C_CRUMB, C_POLLEN, C_LEAF][most];
            let step = a.heading as usize * (STEPS / HEADINGS);
            let d = cam.length(radius * 7 / 10);
            let (hx, hy) = (
                x + d * cos64(step) / TRIG_ONE,
                y + d * sin64(step) / TRIG_ONE,
            );
            list.poly(colour, &circle(hx, hy, cam.length(radius * 2 / 5), 8));
        }
    }

    for &(e, phase, splash) in &effects {
        draw_effect(&mut list, e, phase, splash, false);
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

/// The bounding box of a shape: (min x, min y, max x, max y).
fn bounds(shape: &Shape) -> (i32, i32, i32, i32) {
    let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in shape.iter().flatten() {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    if b.0 > b.2 {
        (0, 0, 0, 0)
    } else {
        b
    }
}

/// Where and at what scale index to draw a shape so that its real bounding
/// box (legs and antennae included) fits a square of `side` draw-list units
/// centred on `at`.
fn fit_box(b: (i32, i32, i32, i32), at: Pt, side: i32) -> (Pt, i32) {
    let span = (b.2 - b.0).max(b.3 - b.1).max(1);
    let scale = (side as i64 * SCALE_ONE as i64 / span as i64) as i32;
    let scale = scale.clamp(1, SCALE_MAX);
    let mid = ((b.0 + b.2) / 2, (b.1 + b.3) / 2);
    let off = |m: i32| (m as i64 * scale as i64 / SCALE_ONE as i64) as i32;
    ((at.0 - off(mid.0), at.1 - off(mid.1)), scale)
}

/// The bounding box of every species' plain shape facing up, worked out
/// once: waypoint flags fit their icons with it every frame.
fn flag_bounds() -> &'static [(i32, i32, i32, i32); 12] {
    static CACHE: std::sync::OnceLock<[(i32, i32, i32, i32); 12]> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Species::ALL.map(|s| bounds(&ant_shape(s, UP, false))))
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
    // One icon box on the flag's cloth per species sent here: about a cell,
    // kept between 12 and 18 pixels so the flag stays a marker at any zoom.
    let cell = (cam.length(CELL) * 3 / 4).clamp(SUB_PX * 12, SUB_PX * 18);
    for ((wx, wy), species) in at {
        let (x, y) = cam.screen(Pos::new(wx, wy));
        let n = species.len() as i32;
        let (cols, rows) = (n.clamp(1, 3), (n + 2) / 3);
        let pad = cell / 6;
        let (cloth_w, cloth_h) = (cols * cell + 2 * pad, rows * cell + 2 * pad);
        let pole = cloth_h + cell;
        if !list.on_screen(x, y, pole + cloth_w) {
            continue;
        }
        if hover == Some(Pos::new(wx, wy)) {
            list.outline(C_HOVER, SUB_PX * 2, &circle(x, y, cell, 24));
        }
        let top = y - pole;
        list.line(C_POLE, ink * 2, &[(x, y), (x, top)]);
        // A swallow-tailed pennant in the player colour.
        let notch = cell / 3;
        let cloth = [
            (x, top),
            (x + cloth_w + notch, top),
            (x + cloth_w, top + cloth_h / 2),
            (x + cloth_w + notch, top + cloth_h),
            (x, top + cloth_h),
        ];
        list.poly(colour, &cloth);
        list.outline(C_INK, ink, &cloth);
        list.poly(C_INK, &circle(x, y, cell / 5, 8));
        // Each species printed on the cloth in ink, three to a row, so it
        // never looks like an ant standing on the ground.
        for (i, s) in species.iter().enumerate() {
            let (col, row) = ((i % 3) as i32, (i / 3) as i32);
            let centre = (
                x + pad + col * cell + cell / 2,
                top + pad + row * cell + cell / 2,
            );
            let b = flag_bounds()[s.index()];
            let (p, scale) = fit_box(b, centre, cell * 9 / 10);
            list.fill_shape(ant_shape_id(*s, UP, false), p.0, p.1, scale, C_INK);
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

/// Every species side by side, for review (TRIAGE row 17): rows at 8, 12
/// and 24 pixels per cell, each first in red and then in blue, the species
/// in `Species::ALL` order, facing up, on floor-coloured panels, drawn
/// exactly as `frame` draws ants at those zooms.
pub fn species_gallery(view_w: i32, view_h: i32) -> Vec<i32> {
    let mut out = vec![op::CLEAR, C_BACKDROP];
    let mut top = SUB_PX * 8;
    for zoom in [10, 12, 14] {
        let mut cam = Camera::new(view_w, view_h);
        cam.zoom = zoom;
        let z = cam.units_per_cell();
        let (col, row) = (z * 3, z * 11 / 2);
        for colony in [0u8, 1] {
            let mut list = List {
                out: Vec::new(),
                cam: &cam,
                w: view_w * SUB_PX,
                h: view_h * SUB_PX,
            };
            let left = SUB_PX * 8;
            let right = left + col * Species::ALL.len() as i32;
            list.poly(
                C_FLOOR,
                &[
                    (left, top),
                    (right, top),
                    (right, top + row),
                    (left, top + row),
                ],
            );
            for (i, s) in Species::ALL.iter().enumerate() {
                let at = (left + col * i as i32 + col / 2, top + row / 2);
                draw_ant(&mut list, *s, UP, at, cam.shape_scale(), player(colony));
            }
            out.extend(list.out);
            top += row + SUB_PX * 4;
        }
        top += SUB_PX * 8;
    }
    // Then every hit kind at 12 pixels per cell, at the start, a quarter,
    // half and three quarters of its life: the attacker (red) on the left,
    // the target on the right.
    let mut cam = Camera::new(view_w, view_h);
    cam.zoom = 12;
    let z = cam.units_per_cell();
    let (col, row) = (z * 7, z * 6);
    let left = SUB_PX * 8;
    let mut list = List {
        out: Vec::new(),
        cam: &cam,
        w: view_w * SUB_PX,
        h: view_h * SUB_PX,
    };
    let kinds = [
        (EffectKind::Bite, Species::Soldier, 2),
        (EffectKind::Spray, Species::Acid, 3),
        (EffectKind::Blast, Species::Exploding, 1),
        (EffectKind::Heal, Species::Nurse, 3),
    ];
    let right = left + col * 16;
    list.poly(
        C_FLOOR,
        &[
            (left, top),
            (right, top),
            (right, top + row),
            (left, top + row),
        ],
    );
    for (k, &(kind, s, gap)) in kinds.iter().enumerate() {
        for (j, phase) in [0, 250, 500, 750].into_iter().enumerate() {
            let i = (k * 4 + j) as i32;
            let (cx, cy) = (left + col * i + col / 2, top + row / 2);
            let from_at = (cx - z * gap / 2, cy);
            let to_at = (cx + z * gap / 2, cy);
            let e = crate::world::Effect {
                kind,
                from: cam.world_at(from_at.0 / SUB_PX, from_at.1 / SUB_PX),
                to: cam.world_at(to_at.0 / SUB_PX, to_at.1 / SUB_PX),
                until: 0,
                colony: 0,
            };
            let splash = if kind == EffectKind::Blast {
                s.stats().splash
            } else {
                0
            };
            let target = if kind == EffectKind::Heal { 0 } else { 1 };
            draw_effect(&mut list, &e, phase, splash, true);
            draw_ant(&mut list, s, 0, from_at, cam.shape_scale(), player(0));
            let (h, scale) = (Species::Harvester, cam.shape_scale());
            draw_ant(&mut list, h, 8, to_at, scale, player(target));
            draw_effect(&mut list, &e, phase, splash, false);
        }
    }
    out.extend(list.out);
    out
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

// ---- icons ----

/// A straight arrow from `tail` to `tip`: shaft `t` thick, head `head`
/// wide and long.
fn arrow(tail: Pt, tip: Pt, t: i32, head: i32) -> Vec<Pt> {
    let u = unit(tail, tip);
    let v = (-u.1, u.0);
    let (dx, dy) = ((tip.0 - tail.0) as i64, (tip.1 - tail.1) as i64);
    let len = isqrt((dx * dx + dy * dy) as u64) as i32;
    let neck = along(tail, u, len - head, v, 0);
    vec![
        along(tail, u, 0, v, -t / 2),
        along(neck, u, 0, v, -t / 2),
        along(neck, u, 0, v, -head / 2),
        tip,
        along(neck, u, 0, v, head / 2),
        along(neck, u, 0, v, t / 2),
        along(tail, u, 0, v, t / 2),
    ]
}

/// The draw list for one interface icon, `size` × `size` pixels, in the
/// colony's player colour: a species, structure, upgrade or action key as
/// used by the action grid and the queue bar. `None` for an unknown key.
/// Every shape is sized from its real bounding box (legs and antennae
/// included), so it stays inside the image.
pub fn icon_list(key: &str, colony: u8, size: i32) -> Option<Vec<i32>> {
    let cam = Camera::new(size, size);
    let mut list = List {
        out: Vec::new(),
        cam: &cam,
        w: size * SUB_PX,
        h: size * SUB_PX,
    };
    let full = size * SUB_PX;
    let c = full / 2;
    let colour = player(colony);
    // Shapes fill 84% of the icon, leaving room for the outline.
    let side = full * 84 / 100;
    let ant = |list: &mut List, s: Species| {
        let mut both = ant_shape(s, UP, true);
        both.extend(mark_shape(s, UP));
        let (p, scale) = fit_box(bounds(&both), (c, c), side);
        list.shape(
            ant_shape_id(s, UP, true),
            p.0,
            p.1,
            scale,
            colour,
            C_INK,
            24,
        );
        list.fill_shape(mark_shape_id(s, UP), p.0, p.1, scale, C_INK);
    };
    let body = |list: &mut List, kind: StructureKind| {
        let rr = full * 9 / 20;
        let pts: Vec<Pt> = body_unit(body_of(kind))
            .into_iter()
            .map(|(x, y)| (c + x * rr / TRIG_ONE, c + y * rr / TRIG_ONE))
            .collect();
        list.poly(soft(colony), &pts);
        list.outline(colour, full / 16, &pts);
    };
    // A fixed shape fitted into a square of `side` centred on `at`, in
    // `fill` with a dark outline; `extra` shares its box (pollen on petals).
    let item =
        |list: &mut List, id: i32, at: Pt, side: i32, fill: i32, extra: Option<(i32, i32)>| {
            let shape = match id {
                SH_QUEEN => queen_shape(),
                SH_CRUMBS => crumbs_shape(),
                SH_PETALS => petals_shape(),
                _ => leaves_shape(),
            };
            let (p, scale) = fit_box(bounds(&shape), at, side);
            let width = (24 * side / full).max(12);
            list.shape(id, p.0, p.1, scale, fill, C_INK, width);
            if let Some((id2, fill2)) = extra {
                list.shape(id2, p.0, p.1, scale, fill2, C_INK, width);
            }
        };
    if let Some(s) = key.strip_prefix("species.").and_then(Species::from_key) {
        ant(&mut list, s);
        return Some(list.out);
    }
    if let Some(rest) = key.strip_prefix("upgrade.species.") {
        let s = Species::from_key(rest)?;
        ant(&mut list, s);
        return Some(list.out);
    }
    if let Some(rest) = key.strip_prefix("structure.") {
        let kind = StructureKind::from_key(rest)?;
        body(&mut list, kind);
        let scale = full * SCALE_ONE * 4 / 5 / (CELL * 3);
        icon(&mut list, kind, c, c, scale, C_INK);
        return Some(list.out);
    }
    let nectar = Some((SH_POLLEN, C_POLLEN));
    let s = match key {
        "upgrade.damage" => Species::TrapJaw,
        "upgrade.armor" => Species::Turtle,
        "upgrade.health" => Species::Nurse,
        "upgrade.gather_rate" => Species::Harvester,
        "upgrade.carry_capacity" => Species::Leafcutter,
        "upgrade.build_rate" => Species::Carpenter,
        "action.all_ants" => Species::Soldier,
        "action.evolve" => {
            // The queen with an arrow up: on to the next tier.
            item(
                &mut list,
                SH_QUEEN,
                (full * 2 / 5, full * 3 / 5),
                full * 7 / 10,
                colour,
                None,
            );
            let a = arrow(
                (full * 4 / 5, full * 4 / 5),
                (full * 4 / 5, full / 10),
                full / 7,
                full * 3 / 10,
            );
            list.poly(colour, &a);
            list.outline(C_INK, full / 24, &a);
            return Some(list.out);
        }
        "action.retreat" => {
            // An arrow running home to the queen.
            item(
                &mut list,
                SH_QUEEN,
                (full / 4, full / 2),
                full / 2,
                colour,
                None,
            );
            let a = arrow(
                (full * 19 / 20, full / 2),
                (full * 9 / 20, full / 2),
                full / 7,
                full * 3 / 10,
            );
            list.poly(colour, &a);
            list.outline(C_INK, full / 24, &a);
            return Some(list.out);
        }
        "focus.food" => {
            item(&mut list, SH_CRUMBS, (c, c), side, C_CRUMB, None);
            return Some(list.out);
        }
        "focus.nectar" => {
            item(&mut list, SH_PETALS, (c, c), side, C_PETAL, nectar);
            return Some(list.out);
        }
        "focus.leaves" => {
            item(&mut list, SH_LEAVES, (c, c), side, C_LEAF, None);
            return Some(list.out);
        }
        "focus.balanced" => {
            let small = full * 9 / 20;
            item(
                &mut list,
                SH_CRUMBS,
                (full / 4, full * 3 / 10),
                small,
                C_CRUMB,
                None,
            );
            item(
                &mut list,
                SH_PETALS,
                (full * 3 / 4, full * 3 / 10),
                small,
                C_PETAL,
                nectar,
            );
            item(&mut list, SH_LEAVES, (c, full * 3 / 4), small, C_LEAF, None);
            return Some(list.out);
        }
        _ => return None,
    };
    ant(&mut list, s);
    Some(list.out)
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn every_interface_key_has_an_icon() {
        let mut keys: Vec<String> = Species::ALL
            .iter()
            .map(|s| format!("species.{}", s.key()))
            .collect();
        keys.extend(
            StructureKind::all()
                .iter()
                .map(|k| format!("structure.{}", k.key())),
        );
        keys.extend(
            crate::rules::Upgrade::all()
                .iter()
                .map(|u| format!("upgrade.{}", u.key())),
        );
        for k in [
            "action.all_ants",
            "action.evolve",
            "action.retreat",
            "focus.balanced",
            "focus.food",
            "focus.nectar",
            "focus.leaves",
        ] {
            keys.push(k.into());
        }
        for k in keys {
            let list = icon_list(&k, 0, 40).unwrap_or_else(|| panic!("no icon for {k}"));
            assert!(!list.is_empty(), "{k} draws something");
        }
        assert!(icon_list("nonsense", 0, 40).is_none());
    }
}
