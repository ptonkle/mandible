//! Props painted into the terrain image (TRIAGE rows 44 and 58): picnic
//! food on the blanket, flowers and ladybugs in the bushes, mushrooms and
//! pinecones on the roots, lily pads and stones in the stream.
//!
//! A prop is only ever placed deep inside rock (never dirt, which can be
//! dug away, and never floor), so it can never hide an ant or a node. Where
//! props go, which they are, how big and how turned all come from the map
//! seed, and rock never changes, so every repaint gives the same props.
//!
//! Each prop is drawn in local units with its radius at `R` (256), y down,
//! as layers painted in order, two by two samples per pixel so the edges
//! are smooth.

use super::{add, bar, lens, rotate, Pt};
use crate::dice::Rng;
use crate::fixed::{cos64, sin64, STEPS, TRIG_ONE};
use crate::map::Ground;
use crate::settings::Biome;
use crate::world::World;

/// A prop's radius in local units.
const R: i32 = 256;
/// Fine units per cell for prop positions and sizes.
const U: i32 = 256;
/// Cells between candidate prop spots.
const SPACING: i32 = 8;
/// Dark outline colour.
const INK: [i32; 3] = [44, 30, 22];

#[derive(Clone, Copy)]
enum Paint {
    Solid([i32; 3]),
    /// Back to the ground under the prop (a bite out of a cookie, the
    /// notch in a lily pad).
    Erase,
}

enum Prim {
    /// An axis-aligned ellipse: centre and radii.
    Ell(i32, i32, i32, i32),
    Poly(Vec<Pt>),
}

struct Layer {
    prim: Prim,
    paint: Paint,
    /// Bounding box, to skip the layer quickly.
    bx: (i32, i32, i32, i32),
}

impl Layer {
    fn hit(&self, x: i32, y: i32) -> bool {
        if x < self.bx.0 || x > self.bx.2 || y < self.bx.1 || y > self.bx.3 {
            return false;
        }
        match &self.prim {
            Prim::Ell(cx, cy, rx, ry) => {
                let (dx, dy) = ((x - cx) as i64 * *ry as i64, (y - cy) as i64 * *rx as i64);
                let r = *rx as i64 * *ry as i64;
                dx * dx + dy * dy <= r * r
            }
            Prim::Poly(pts) => inside(pts, x, y),
        }
    }
}

fn inside(pts: &[Pt], x: i32, y: i32) -> bool {
    let mut c = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (xi, yi) = (pts[i].0 as i64, pts[i].1 as i64);
        let (xj, yj) = (pts[j].0 as i64, pts[j].1 as i64);
        let (x, y) = (x as i64, y as i64);
        if (yi > y) != (yj > y) && ((x - xi) * (yj - yi) < (xj - xi) * (y - yi)) == (yj > yi) {
            c = !c;
        }
        j = i;
    }
    c
}

/// A prop's look: its layers, bottom first.
#[derive(Default)]
struct Art {
    layers: Vec<Layer>,
    /// For each square of a `GRID` × `GRID` grid over the local square
    /// ±`GRID_HALF`, a bit per layer whose box touches it: `at` only tries
    /// those.
    grid: Vec<u64>,
    /// The top layer (plus one; 0 for none) at every texel of a `TABLE` ×
    /// `TABLE` raster of the same square, worked out once with `exact`, so
    /// painting a pixel costs a few table reads.
    table: Vec<u8>,
}

const TABLE: i32 = 128;
const TEXEL: i32 = 2 * GRID_HALF / TABLE;

const GRID: i32 = 16;
const GRID_HALF: i32 = 384;
const GRID_STEP: i32 = 2 * GRID_HALF / GRID;

impl Art {
    fn push(&mut self, prim: Prim, paint: Paint) {
        let bx = match &prim {
            Prim::Ell(cx, cy, rx, ry) => (cx - rx, cy - ry, cx + rx, cy + ry),
            Prim::Poly(pts) => pts
                .iter()
                .fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, &(x, y)| {
                    (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y))
                }),
        };
        self.layers.push(Layer { prim, paint, bx });
    }

    fn ell(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, c: [i32; 3]) {
        self.push(Prim::Ell(cx, cy, rx, ry), Paint::Solid(c));
    }

    /// An ellipse with a dark rim `w` wide.
    fn ell_ink(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, w: i32, c: [i32; 3]) {
        self.ell(cx, cy, rx + w, ry + w, INK);
        self.ell(cx, cy, rx, ry, c);
    }

    fn poly(&mut self, pts: Vec<Pt>, c: [i32; 3]) {
        self.push(Prim::Poly(pts), Paint::Solid(c));
    }

    /// A polygon with a dark rim: the same outline grown by `w` from its
    /// centre, under it.
    fn poly_ink(&mut self, pts: Vec<Pt>, w: i32, c: [i32; 3]) {
        let n = pts.len() as i32;
        let (sx, sy) = pts.iter().fold((0, 0), |s, p| (s.0 + p.0, s.1 + p.1));
        let (cx, cy) = (sx / n, sy / n);
        let grown = pts
            .iter()
            .map(|&(x, y)| {
                let (dx, dy) = ((x - cx) as i64, (y - cy) as i64);
                let len = (crate::fixed::isqrt((dx * dx + dy * dy) as u64) as i64).max(1);
                (
                    x + (dx * w as i64 / len) as i32,
                    y + (dy * w as i64 / len) as i32,
                )
            })
            .collect();
        self.poly(grown, INK);
        self.poly(pts, c);
    }

    fn erase_ell(&mut self, cx: i32, cy: i32, rx: i32, ry: i32) {
        self.push(Prim::Ell(cx, cy, rx, ry), Paint::Erase);
    }

    /// Builds the lookup grid once every layer is in.
    fn finish(mut self) -> Art {
        assert!(self.layers.len() <= 64, "a prop has at most 64 layers");
        let cell = |v: i32| ((v + GRID_HALF).div_euclid(GRID_STEP)).clamp(0, GRID - 1);
        self.grid = vec![0; (GRID * GRID) as usize];
        for (i, l) in self.layers.iter().enumerate() {
            for gy in cell(l.bx.1)..=cell(l.bx.3) {
                for gx in cell(l.bx.0)..=cell(l.bx.2) {
                    self.grid[(gy * GRID + gx) as usize] |= 1 << i;
                }
            }
        }
        let mut table = vec![0u8; (TABLE * TABLE) as usize];
        for ty in 0..TABLE {
            for tx in 0..TABLE {
                let (x, y) = (
                    tx * TEXEL + TEXEL / 2 - GRID_HALF,
                    ty * TEXEL + TEXEL / 2 - GRID_HALF,
                );
                if let Some(i) = self.exact(x, y) {
                    table[(ty * TABLE + tx) as usize] = i as u8 + 1;
                }
            }
        }
        self.table = table;
        self
    }

    /// The top layer at local point (x, y), from the table: `None` for
    /// bare ground.
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let (tx, ty) = (
            (x + GRID_HALF).div_euclid(TEXEL),
            (y + GRID_HALF).div_euclid(TEXEL),
        );
        if !(0..TABLE).contains(&tx) || !(0..TABLE).contains(&ty) {
            return None;
        }
        match self.table[(ty * TABLE + tx) as usize] {
            0 => None,
            i => Some(i as usize - 1),
        }
    }

    /// The top layer at local point (x, y), tested shape by shape.
    fn exact(&self, x: i32, y: i32) -> Option<usize> {
        let (gx, gy) = (
            (x + GRID_HALF).div_euclid(GRID_STEP),
            (y + GRID_HALF).div_euclid(GRID_STEP),
        );
        if !(0..GRID).contains(&gx) || !(0..GRID).contains(&gy) {
            return None;
        }
        let mut mask = self.grid[(gy * GRID + gx) as usize];
        while mask != 0 {
            let i = 63 - mask.leading_zeros() as usize;
            if self.layers[i].hit(x, y) {
                return Some(i);
            }
            mask &= !(1u64 << i);
        }
        None
    }
}

/// The lower half of a circle of radius r (flat side up) centred on (cx, cy).
fn half_disc(cx: i32, cy: i32, r: i32) -> Vec<Pt> {
    (0..=16)
        .map(|k| {
            let st = k * STEPS / 32;
            (cx + r * cos64(st) / TRIG_ONE, cy + r * sin64(st) / TRIG_ONE)
        })
        .collect()
}

/// Every kind of prop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Basket,
    Cookie,
    Sandwich,
    Watermelon,
    Napkin,
    Grapes,
    Cheese,
    Sugar,
    Daisy,
    Ladybug,
    Clover,
    Petals,
    Mushroom,
    Acorn,
    Pinecone,
    FallenLeaf,
    Twig,
    LilyPad,
    Stones,
    Shell,
    Reeds,
}

/// The props a biome scatters, with their radius range in cells × 4.
fn kinds(b: Biome) -> &'static [(Kind, i32, i32)] {
    match b {
        Biome::FlowerField => &[
            (Kind::Daisy, 4, 7),
            (Kind::Ladybug, 3, 5),
            (Kind::Clover, 5, 7),
            (Kind::Petals, 4, 7),
        ],
        Biome::ForestFloor => &[
            (Kind::Mushroom, 5, 9),
            (Kind::Acorn, 4, 6),
            (Kind::Pinecone, 6, 10),
            (Kind::FallenLeaf, 6, 10),
            (Kind::Twig, 10, 16),
        ],
        Biome::Riverbank => &[
            (Kind::LilyPad, 6, 10),
            (Kind::Stones, 6, 10),
            (Kind::Shell, 4, 6),
            (Kind::Reeds, 8, 12),
        ],
        _ => &[
            (Kind::Basket, 14, 20),
            (Kind::Cookie, 5, 8),
            (Kind::Sandwich, 9, 12),
            (Kind::Watermelon, 9, 13),
            (Kind::Napkin, 10, 14),
            (Kind::Grapes, 8, 11),
            (Kind::Cheese, 7, 10),
            (Kind::Sugar, 6, 10),
        ],
    }
}

fn art(kind: Kind) -> Art {
    let mut a = Art::default();
    match kind {
        Kind::Basket => {
            let rect =
                |x0: i32, y0: i32, x1: i32, y1: i32| vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            a.poly(rect(-246, -170, 246, 170), INK);
            a.poly(rect(-232, -156, 232, 156), [178, 126, 64]);
            // Woven bands.
            for k in 0..6 {
                let y = -140 + k * 54;
                a.poly(rect(-232, y, 232, y + 18), [140, 94, 44]);
            }
            for k in 0..8 {
                let x = -210 + k * 60;
                a.poly(rect(x, -156, x + 14, 156), [196, 146, 80]);
            }
            // A red-checked cloth peeking out of the lid, and the handle.
            a.poly_ink(
                vec![(-232, -156), (-40, -156), (-232, 40)],
                10,
                [214, 70, 64],
            );
            a.poly(rect(-200, -150, -150, -100), [246, 236, 220]);
            a.poly(rect(-150, -100, -100, -50), [246, 236, 220]);
            a.poly(bar((-210, 0), (210, 0), 40), INK);
            a.poly(bar((-200, 0), (200, 0), 24), [118, 78, 36]);
        }
        Kind::Cookie => {
            a.ell_ink(0, 0, 236, 236, 14, [194, 136, 70]);
            a.ell(-16, -18, 196, 192, [214, 160, 92]);
            for (x, y) in [
                (-96, -60),
                (60, -110),
                (112, 34),
                (-36, 92),
                (-130, 64),
                (16, -6),
                (84, 128),
            ] {
                a.ell(x, y, 32, 26, [74, 42, 24]);
            }
            // A bite.
            a.erase_ell(196, -150, 92, 92);
        }
        Kind::Sandwich => {
            for k in 0..5 {
                a.ell(-180 + k * 90, 170, 60, 44, [92, 168, 60]);
            }
            a.ell(150, 60, 70, 60, [232, 132, 138]);
            a.poly_ink(vec![(-236, 172), (236, 172), (0, -236)], 12, [190, 128, 66]);
            a.poly(vec![(-184, 142), (184, 142), (0, -178)], [240, 216, 164]);
        }
        Kind::Watermelon => {
            a.poly(half_disc(0, -110, 250), INK);
            a.poly(half_disc(0, -110, 236), [56, 132, 54]);
            a.poly(half_disc(0, -110, 206), [214, 236, 182]);
            a.poly(half_disc(0, -110, 186), [230, 62, 72]);
            for (x, y) in [(-90, -40), (0, -10), (90, -40), (-46, 40), (46, 40)] {
                a.ell(x, y, 12, 20, [36, 24, 20]);
            }
        }
        Kind::Napkin => {
            let sq = vec![(-200, -200), (200, -200), (200, 200), (-200, 200)];
            a.poly_ink(sq, 12, [176, 212, 238]);
            a.poly(vec![(-200, 200), (200, -200), (200, 200)], [148, 188, 222]);
            a.poly(bar((-200, 200), (200, -200), 8), [120, 156, 190]);
        }
        Kind::Grapes => {
            a.poly(bar((10, -240), (0, -150), 22), [96, 70, 40]);
            a.poly(lens((10, -200), 150, 80, STEPS - 8), [80, 150, 56]);
            for (x, y) in [
                (-90, -100),
                (0, -110),
                (90, -100),
                (-130, -10),
                (-44, -20),
                (44, -20),
                (130, -10),
                (-86, 70),
                (0, 70),
                (86, 70),
                (-40, 150),
                (40, 150),
                (0, 224),
            ] {
                a.ell_ink(x, y, 50, 50, 10, [112, 52, 140]);
                a.ell(x - 16, y - 16, 16, 14, [176, 120, 200]);
            }
        }
        Kind::Cheese => {
            a.poly_ink(
                vec![(-236, 170), (236, 170), (-120, -220)],
                12,
                [250, 206, 76],
            );
            a.poly(
                vec![(-236, 170), (236, 170), (220, 140), (-224, 140)],
                [226, 168, 52],
            );
            for (x, y, r) in [
                (-120, 80, 34),
                (-60, -60, 26),
                (40, 90, 22),
                (-150, -20, 18),
            ] {
                a.ell(x, y, r, r * 4 / 5, [216, 160, 44]);
            }
        }
        Kind::Sugar => {
            for (x, y, rx, ry) in [
                (-40, 20, 170, 120),
                (80, -60, 120, 100),
                (-120, -80, 90, 70),
                (120, 110, 80, 60),
            ] {
                a.ell(x, y, rx, ry, [236, 234, 226]);
            }
            for (x, y) in [(-70, -10), (30, 40), (110, -70), (-130, 70), (60, 120)] {
                a.ell(x, y, 14, 14, [255, 255, 255]);
            }
            let cube = |x: i32, y: i32| {
                vec![
                    (x - 56, y - 56),
                    (x + 56, y - 56),
                    (x + 56, y + 56),
                    (x - 56, y + 56),
                ]
            };
            a.poly_ink(cube(-10, -40), 10, [252, 252, 250]);
            a.poly(
                vec![(46, -96), (66, -76), (66, 36), (46, 16)],
                [208, 208, 204],
            );
        }
        Kind::Daisy => {
            for k in 0..10 {
                let dir = k * STEPS / 10;
                a.poly(lens((0, 0), 250, 92, dir), INK);
            }
            for k in 0..10 {
                let dir = k * STEPS / 10;
                a.poly(lens((0, 0), 236, 76, dir), [250, 250, 244]);
            }
            a.ell_ink(0, 0, 70, 70, 10, [250, 196, 40]);
        }
        Kind::Ladybug => {
            a.ell_ink(0, -150, 90, 76, 12, [30, 26, 24]);
            a.ell_ink(0, 40, 196, 210, 14, [222, 40, 36]);
            a.poly(bar((0, -170), (0, 250), 14), [30, 26, 24]);
            for (x, y, r) in [
                (-90, -20, 40),
                (90, -20, 40),
                (-110, 110, 34),
                (110, 110, 34),
                (-50, 190, 28),
                (50, 190, 28),
            ] {
                a.ell(x, y, r, r, [30, 26, 24]);
            }
            a.ell(-40, -170, 18, 14, [250, 250, 250]);
            a.ell(40, -170, 18, 14, [250, 250, 250]);
        }
        Kind::Clover => {
            a.poly(bar((0, 0), (40, 250), 26), [58, 112, 40]);
            for k in 0..3 {
                let c = rotate((0, -110), k * STEPS / 3);
                a.ell_ink(c.0, c.1, 120, 120, 12, [86, 168, 70]);
            }
            for k in 0..3 {
                let c = rotate((0, -110), k * STEPS / 3);
                a.ell(c.0, c.1, 70, 70, [120, 196, 102]);
            }
        }
        Kind::Petals => {
            for (k, (x, y)) in [
                (-120, -100),
                (90, -130),
                (140, 60),
                (-60, 120),
                (10, 0),
                (-170, 40),
            ]
            .into_iter()
            .enumerate()
            {
                let colour = [[255, 150, 200], [252, 246, 236], [255, 120, 176]][k % 3];
                a.poly(lens((x, y), 150, 80, k * 11), colour);
            }
        }
        Kind::Mushroom => {
            a.ell_ink(0, 0, 236, 236, 14, [206, 46, 40]);
            a.ell(-30, -36, 170, 160, [226, 66, 52]);
            for (x, y, r) in [
                (-90, -90, 40),
                (70, -120, 34),
                (110, 50, 44),
                (-60, 80, 36),
                (10, -10, 30),
                (-150, 10, 24),
            ] {
                a.ell(x, y, r, r, [250, 244, 230]);
            }
        }
        Kind::Acorn => {
            a.ell_ink(0, 60, 150, 190, 12, [176, 118, 56]);
            a.ell(-40, 40, 40, 90, [204, 148, 80]);
            a.ell_ink(0, -100, 196, 120, 12, [116, 84, 50]);
            for k in 0..4 {
                a.poly(
                    bar((-150 + k * 100, -150), (-120 + k * 100, -40), 16),
                    [88, 62, 36],
                );
            }
            a.poly(bar((0, -210), (20, -250), 30), [88, 62, 36]);
        }
        Kind::Pinecone => {
            a.ell_ink(0, 0, 150, 240, 12, [112, 72, 40]);
            for row in 0..7 {
                let y = -190 + row * 62;
                let w = 130 - (row - 3) * (row - 3) * 10;
                for k in -1..=1 {
                    let x = k * w * 2 / 3 + if row % 2 == 0 { 0 } else { w / 3 };
                    a.poly(
                        vec![(x, y - 20), (x + 40, y + 24), (x, y + 44), (x - 40, y + 24)],
                        [150, 100, 54],
                    );
                }
            }
        }
        Kind::FallenLeaf => {
            a.poly(lens((-250, 0), 500, 260, 0), INK);
            a.poly(lens((-236, 0), 472, 230, 0), [206, 112, 40]);
            a.poly(bar((-250, 0), (220, 0), 14), [150, 70, 30]);
            for x in [-120, 0, 110] {
                a.poly(bar((x, 0), (x + 70, -70), 10), [150, 70, 30]);
                a.poly(bar((x, 0), (x + 70, 70), 10), [150, 70, 30]);
            }
        }
        Kind::Twig => {
            a.poly(bar((-250, 30), (250, -30), 60), INK);
            a.poly(bar((40, 4), (170, 140), 46), INK);
            a.poly(bar((-246, 30), (246, -30), 40), [124, 88, 52]);
            a.poly(bar((40, 4), (166, 136), 28), [124, 88, 52]);
            a.poly(bar((-200, 20), (100, -10), 10), [156, 116, 72]);
        }
        Kind::LilyPad => {
            a.ell_ink(0, 0, 236, 236, 12, [70, 140, 62]);
            // The notch.
            a.push(
                Prim::Poly(vec![(0, 0), (260, -70), (260, 70)]),
                Paint::Erase,
            );
            for k in 1..8 {
                let p = rotate((210, 0), k * STEPS / 8);
                a.poly(bar((0, 0), p, 10), [100, 170, 84]);
            }
            a.ell_ink(-70, 60, 60, 60, 8, [255, 168, 206]);
            a.ell(-70, 60, 22, 22, [255, 220, 90]);
        }
        Kind::Stones => {
            for (x, y, rx, ry, t) in [
                (-80, -40, 150, 110, 0),
                (120, 70, 110, 90, 18),
                (-60, 140, 90, 70, -14),
                (130, -140, 80, 60, 8),
            ] {
                a.ell_ink(x, y, rx, ry, 10, add([138, 134, 126], t));
                a.ell(
                    x - rx / 4,
                    y - ry / 4,
                    rx / 2,
                    ry / 3,
                    add([176, 172, 164], t),
                );
            }
        }
        Kind::Shell => {
            let mut fan = vec![(0, 220)];
            for k in 0..=16 {
                let st = STEPS / 2 + k * STEPS / 32;
                fan.push((240 * cos64(st) / TRIG_ONE, 40 + 220 * sin64(st) / TRIG_ONE));
            }
            a.poly_ink(fan, 12, [240, 214, 196]);
            for k in 1..8 {
                let st = STEPS / 2 + k * STEPS / 16;
                let p = (220 * cos64(st) / TRIG_ONE, 40 + 200 * sin64(st) / TRIG_ONE);
                a.poly(bar((0, 210), p, 14), [214, 168, 150]);
            }
        }
        Kind::Reeds => {
            // Long blades fanning up from one root, two of them topped
            // with cattails.
            let root = (0, 236);
            for (dir, len) in [
                (40, 380),
                (44, 450),
                (47, 470),
                (50, 440),
                (54, 400),
                (57, 300),
            ] {
                a.poly(lens(root, len + 16, 70, dir), INK);
                a.poly(lens(root, len, 52, dir), [96, 156, 62]);
            }
            for dir in [45, 51] {
                let tip = rotate((360, 0), dir);
                let at = (root.0 + tip.0, root.1 + tip.1);
                a.poly(lens((at.0, at.1 + 60), 130, 60, 48), INK);
                a.poly(lens((at.0, at.1 + 54), 118, 46, 48), [120, 74, 38]);
            }
        }
    }
    a.finish()
}

/// One placed prop: centre and radius in fine units (`U` per cell) and its
/// turn in steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Prop {
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub rot: usize,
}

/// Whether every cell within `r` fine units (plus a cell) of (x, y) is rock.
fn clear_rock(world: &World, x: i32, y: i32, r: i32) -> bool {
    let reach = r + U;
    let (x0, x1) = ((x - reach).div_euclid(U), (x + reach).div_euclid(U));
    let (y0, y1) = ((y - reach).div_euclid(U), (y + reach).div_euclid(U));
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            let (dx, dy) = ((cx * U + U / 2 - x) as i64, (cy * U + U / 2 - y) as i64);
            let near = (reach + U) as i64;
            if dx * dx + dy * dy <= near * near && world.map.at(cx, cy) != Ground::Rock {
                return false;
            }
        }
    }
    true
}

/// Every prop on the map, from its seed: one try per candidate spot on a
/// jittered lattice, shrunk once if it does not fit, skipped if it still
/// does not.
pub(super) fn props(world: &World, biome: Biome) -> Vec<Prop> {
    let mut rng = Rng::new(world.seed ^ 0x7072_6F70_7300_0000);
    let list = kinds(biome);
    let mut out = Vec::new();
    let (w, h) = (world.map.w, world.map.h);
    let mut gy = SPACING / 2;
    while gy < h {
        let mut gx = SPACING / 2;
        while gx < w {
            let (kind, lo, hi) = list[rng.below(list.len() as u32) as usize];
            let r = rng.range(lo, hi) * U / 4;
            let x = gx * U + rng.range(-3 * U, 3 * U);
            let y = gy * U + rng.range(-3 * U, 3 * U);
            let rot = rng.below(STEPS as u32) as usize;
            let present = rng.chance(if biome == Biome::FlowerField { 45 } else { 75 });
            if present {
                for r in [r, r * 2 / 3] {
                    if clear_rock(world, x, y, r * 5 / 4) {
                        out.push(Prop { kind, x, y, r, rot });
                        break;
                    }
                }
            }
            gx += SPACING;
        }
        gy += SPACING;
    }
    out
}

/// Paints the props over a region of the terrain image already painted:
/// `out` holds pixels px0..px0+pw, py0..py0+ph at `scale` pixels per cell.
pub(super) fn paint(
    world: &World,
    biome: Biome,
    scale: i32,
    region: (i32, i32, i32, i32),
    out: &mut [u8],
) {
    let (px0, py0, pw, ph) = region;
    let props = props(world, biome);
    if props.is_empty() || pw <= 0 || ph <= 0 {
        return;
    }
    let mut arts: Vec<(Kind, Art)> = Vec::new();
    for p in &props {
        if !arts.iter().any(|(k, _)| *k == p.kind) {
            arts.push((p.kind, art(p.kind)));
        }
    }
    for p in &props {
        let art = &arts.iter().find(|(k, _)| *k == p.kind).expect("art").1;
        // The prop's whole pixel box (not clipped, so a region's pixels
        // match the whole image's), skipped if it misses the region.
        let reach = p.r * 11 / 8 + U / 8;
        let bx0 = ((p.x - reach) * scale).div_euclid(U);
        let bx1 = ((p.x + reach) * scale).div_euclid(U);
        let by0 = ((p.y - reach) * scale).div_euclid(U);
        let by1 = ((p.y + reach) * scale).div_euclid(U);
        if bx1 < px0 || by1 < py0 || bx0 >= px0 + pw || by0 >= py0 + ph {
            continue;
        }
        // Fine offsets to local units: turned back by the prop's turn and
        // scaled from radius `p.r` to `R`, in 1/65536ths.
        let back = STEPS - p.rot % STEPS;
        let k = |v: i32| v as i64 * R as i64 * 65536 / (TRIG_ONE as i64 * p.r.max(1) as i64);
        let turn = (k(cos64(back)), k(sin64(back)));
        let far = reach as i64;
        // What a sample at fine offset (sx, sy) from the centre shows:
        // `GROUND`, `SHADOW` (a soft shadow down and to the right), or
        // `FIRST + i` for layer i.
        let tag = |sx: i32, sy: i32| -> usize {
            if (sx as i64).pow(2) + (sy as i64).pow(2) > far * far {
                return GROUND;
            }
            let look = |sx: i32, sy: i32| {
                let (sx, sy) = (sx as i64, sy as i64);
                let lx = ((sx * turn.0 - sy * turn.1) >> 16) as i32;
                let ly = ((sx * turn.1 + sy * turn.0) >> 16) as i32;
                art.at(lx, ly)
            };
            match look(sx, sy) {
                Some(i) if matches!(art.layers[i].paint, Paint::Solid(_)) => FIRST + i,
                _ => match look(sx - p.r / 10, sy - p.r / 8) {
                    Some(i) if matches!(art.layers[i].paint, Paint::Solid(_)) => SHADOW,
                    _ => GROUND,
                },
            }
        };
        let colour = |t: usize, ground: [i32; 3]| match t {
            GROUND => ground,
            SHADOW => [ground[0] * 3 / 4, ground[1] * 3 / 4, ground[2] * 3 / 4],
            i => match art.layers[i - FIRST].paint {
                Paint::Solid(c) => c,
                Paint::Erase => ground,
            },
        };
        // One sample per pixel centre first; pixels whose neighbours all
        // agree take it as is, the rest (edges) get two by two samples.
        let (bw, bh) = (bx1 - bx0 + 1, by1 - by0 + 1);
        let mut tags = vec![GROUND; (bw * bh) as usize];
        for y in 0..bh {
            for x in 0..bw {
                let (px, py) = (bx0 + x, by0 + y);
                tags[(y * bw + x) as usize] = tag(
                    (2 * px + 1) * U / (2 * scale) - p.x,
                    (2 * py + 1) * U / (2 * scale) - p.y,
                );
            }
        }
        let at = |x: i32, y: i32| tags[(y.clamp(0, bh - 1) * bw + x.clamp(0, bw - 1)) as usize];
        for y in 0..bh {
            for x in 0..bw {
                let t = at(x, y);
                let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .any(|&(dx, dy)| at(x + dx, y + dy) != t);
                let (px, py) = (bx0 + x, by0 + y);
                let outside = px < px0 || py < py0 || px >= px0 + pw || py >= py0 + ph;
                if outside || (t == GROUND && !edge) {
                    continue;
                }
                let o = (((py - py0) * pw + px - px0) * 4) as usize;
                let ground = [out[o] as i32, out[o + 1] as i32, out[o + 2] as i32];
                let c = if edge {
                    let mut sum = [0; 3];
                    for (kx, ky) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let c = colour(
                            tag(
                                (4 * px + 1 + 2 * kx) * U / (4 * scale) - p.x,
                                (4 * py + 1 + 2 * ky) * U / (4 * scale) - p.y,
                            ),
                            ground,
                        );
                        for k in 0..3 {
                            sum[k] += c[k];
                        }
                    }
                    [sum[0] / 4, sum[1] / 4, sum[2] / 4]
                } else {
                    colour(t, ground)
                };
                out[o] = c[0].clamp(0, 255) as u8;
                out[o + 1] = c[1].clamp(0, 255) as u8;
                out[o + 2] = c[2].clamp(0, 255) as u8;
            }
        }
    }
}

const GROUND: usize = 0;
const SHADOW: usize = 1;
const FIRST: usize = 2;
