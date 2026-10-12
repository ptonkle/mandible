//! The map: a grid of rock, floor and dirt, generated from a seed.
//!
//! Fairness comes from symmetry. The generator lays out one sector of the map
//! (chambers, tunnels, dirt and nodes), then copies it by exact grid rotation:
//! 180° for two seats, 90° for four. Every seat therefore gets the same
//! layout. Then each sector is decorated on its own, so the map does not look
//! mirrored, and the result is kept only if the seats' walking distances stay
//! within a few percent (see `decorate`).

use serde::{Deserialize, Serialize};

use crate::dice::Rng;
use crate::fixed::{cos64, sin64, Pos, CELL, TRIG_ONE};
use crate::rules::Resource;
use crate::settings::{Level, MapType, MatchSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Ground {
    Rock = 0,
    Floor = 1,
    Dirt = 2,
}

pub const DIRT_HEALTH: i32 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chamber {
    pub cx: i32,
    pub cy: i32,
    pub r: i32,
    /// The seat whose home this is, if any.
    pub home: Option<u8>,
}

/// A node as generated, before the world gives it an id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpot {
    pub pos: Pos,
    pub resource: Resource,
    pub amount: i32,
    pub cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    pub w: i32,
    pub h: i32,
    pub ground: Vec<Ground>,
    pub chambers: Vec<Chamber>,
    pub nodes: Vec<NodeSpot>,
    /// Index into `chambers` of the centre chamber (Hill and rich nodes).
    pub centre: usize,
    pub seats: usize,
}

impl Map {
    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    pub fn at(&self, x: i32, y: i32) -> Ground {
        if self.inside(x, y) {
            self.ground[self.idx(x, y)]
        } else {
            Ground::Rock
        }
    }

    pub fn is_floor(&self, x: i32, y: i32) -> bool {
        self.at(x, y) == Ground::Floor
    }

    pub fn home(&self, seat: usize) -> &Chamber {
        self.chambers
            .iter()
            .find(|c| c.home == Some(seat as u8))
            .expect("every seat has a home chamber")
    }

    /// The chamber containing a cell, if any.
    pub fn chamber_at(&self, x: i32, y: i32) -> Option<usize> {
        self.chambers.iter().position(|c| {
            let dx = x - c.cx;
            let dy = y - c.cy;
            dx * dx + dy * dy <= c.r * c.r
        })
    }
}

/// Rotates a cell by `turns` steps of the map's symmetry.
fn rotate(w: i32, h: i32, seats: usize, turns: usize, x: i32, y: i32) -> (i32, i32) {
    let mut p = (x, y);
    for _ in 0..turns {
        p = if seats == 2 {
            (w - 1 - p.0, h - 1 - p.1)
        } else {
            (w - 1 - p.1, p.0)
        };
    }
    p
}

fn rotate_pos(w: i32, h: i32, seats: usize, turns: usize, p: Pos) -> Pos {
    // Rotate in sub-units about the exact map centre.
    let mut q = p;
    let (fw, fh) = (w * CELL, h * CELL);
    for _ in 0..turns {
        q = if seats == 2 {
            Pos::new(fw - q.x, fh - q.y)
        } else {
            Pos::new(fw - q.y, q.x)
        };
    }
    q
}

/// Map dimensions in cells for a seat count and Map Size.
pub fn dimensions(seats: usize, size: Level) -> (i32, i32) {
    let base = if seats == 2 { (224, 150) } else { (224, 224) };
    let p = size.percent();
    (base.0 * p / 100, base.1 * p / 100)
}

struct Mask {
    w: i32,
    h: i32,
    cells: Vec<u8>, // 0 rock, 1 floor, 2 dirt
}

impl Mask {
    fn set(&mut self, x: i32, y: i32, v: u8) {
        if x >= 1 && y >= 1 && x < self.w - 1 && y < self.h - 1 {
            let i = (y * self.w + x) as usize;
            if v > self.cells[i] {
                self.cells[i] = v;
            }
        }
    }

    fn disk(&mut self, cx: i32, cy: i32, r: i32, v: u8) {
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r * r + r {
                    self.set(x, y, v);
                }
            }
        }
    }
}

/// A chamber's outline: radius per 64-step angle, never below `r`.
fn blob(rng: &mut Rng, r: i32) -> Vec<i32> {
    let mut ctrl = [0i32; 8];
    for c in ctrl.iter_mut() {
        *c = rng.range(0, (r / 3).max(2));
    }
    (0..64)
        .map(|i| {
            let k = i / 8;
            let t = (i % 8) as i32;
            let a = ctrl[k];
            let b = ctrl[(k + 1) % 8];
            r + (a * (8 - t) + b * t) / 8
        })
        .collect()
}

fn carve_blob(m: &mut Mask, cx: i32, cy: i32, radii: &[i32]) {
    let rmax = *radii.iter().max().unwrap_or(&1);
    for y in cy - rmax..=cy + rmax {
        for x in cx - rmax..=cx + rmax {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            // Nearest of 64 angles by dot product.
            let mut best = 0;
            let mut best_dot = i64::MIN;
            for (s, _) in radii.iter().enumerate() {
                let dot = dx as i64 * cos64(s) as i64 + dy as i64 * sin64(s) as i64;
                if dot > best_dot {
                    best_dot = dot;
                    best = s;
                }
            }
            let r = radii[best];
            if d2 <= r * r {
                m.set(x, y, 1);
            }
        }
    }
}

/// Stamps a tunnel from a to b with a bend, every cell along the way a disk
/// of radius `half`, so its width is never below `2 * half`.
fn carve_tunnel(m: &mut Mask, rng: &mut Rng, a: (i32, i32), b: (i32, i32), half: i32) {
    let len = isqrt_i((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2)).max(1);
    let bend = rng.range(-(len / 6), len / 6);
    let mid = (
        (a.0 + b.0) / 2 + (-(b.1 - a.1)) * bend / len,
        (a.1 + b.1) / 2 + (b.0 - a.0) * bend / len,
    );
    for (p, q) in [(a, mid), (mid, b)] {
        let seg = isqrt_i((q.0 - p.0).pow(2) + (q.1 - p.1).pow(2)).max(1);
        for i in 0..=seg {
            let x = p.0 + (q.0 - p.0) * i / seg;
            let y = p.1 + (q.1 - p.1) * i / seg;
            m.disk(x, y, half, 1);
        }
    }
}

fn isqrt_i(n: i32) -> i32 {
    crate::fixed::isqrt(n.max(0) as u64) as i32
}

#[derive(Clone)]
struct ChamberPlan {
    cx: i32,
    cy: i32,
    r: i32,
    home: bool,
}

/// Generates the map for a seed and settings.
/// A Random Map Type becomes one of the three here, from the seed.
pub fn generate(seed: u64, settings: &MatchSettings) -> Map {
    let settings = &MatchSettings {
        map_type: settings.map_type.resolve(seed),
        ..settings.clone()
    };
    let mut map = match settings.map_type {
        MapType::Tunnels | MapType::Random => generate_tunnels(seed, settings),
        MapType::Open | MapType::Maze => generate_open(seed, settings, true),
    };
    let _ = decorate(&mut map, seed, settings.tunnel_width, settings.map_type);
    map
}

/// Where seat 0's home chamber goes.
fn home_spot(rng: &mut Rng, w: i32, h: i32, seats: usize) -> (i32, i32) {
    if seats == 2 {
        (w * 17 / 100, h / 2 + rng.range(-(h / 6), h / 6))
    } else {
        (
            w * 18 / 100 + rng.range(-3, 3),
            h * 18 / 100 + rng.range(-3, 3),
        )
    }
}

/// A point `d` cells from a cell centre at 64-step angle `a`.
fn spot_at(cx: i32, cy: i32, d: i32, a: usize) -> Pos {
    Pos::new(
        cx * CELL + CELL / 2 + d * CELL * cos64(a) / TRIG_ONE,
        cy * CELL + CELL / 2 + d * CELL * sin64(a) / TRIG_ONE,
    )
}

/// Node sizes before Resource Density, in whole resources (TRIAGE rows 47,
/// 51 and 52). The home chamber keeps its Food and Leaves; its Flower is
/// smaller, so Nectar is something to expand for by the middle of a match.
pub const HOME_AMOUNTS: [i32; 3] = [3600, 1000, 2200];
/// The home chamber's second, smaller Crumb Pile.
pub const HOME_SECOND_FOOD: i32 = 2400;
/// Every other chamber has one node of each resource, its size drawn from
/// these ranges (one draw per chamber, so its rotations match).
pub const CHAMBER_AMOUNTS: [(i32, i32); 3] = [(2200, 2800), (1300, 1700), (1100, 1500)];
/// The centre chamber's nodes, for each seat, by seat count (2, 4).
pub const CENTRE_AMOUNTS: [i32; 2] = [2500, 1500];
/// A cache behind dirt.
pub const CACHE_AMOUNT: i32 = 1000;

/// The home chamber's nodes: one of each resource and a second Crumb Pile.
fn home_nodes(rng: &mut Rng, home: &ChamberPlan, density: i32, out: &mut Vec<NodeSpot>) {
    let a0 = rng.range(0, 63) as usize;
    let d = home.r * 6 / 10;
    for (k, res) in Resource::ALL.into_iter().enumerate() {
        out.push(NodeSpot {
            pos: spot_at(home.cx, home.cy, d, (a0 + k * 21) % 64),
            resource: res,
            amount: HOME_AMOUNTS[k] * density / 100,
            cache: false,
        });
    }
    // A second, smaller Crumb Pile: Food pays for almost everything, and one
    // pile holds only 8 Harvester Ants (SECOND-ORDER row 35).
    out.push(NodeSpot {
        pos: spot_at(home.cx, home.cy, d, (a0 + 53) % 64),
        resource: Resource::Food,
        amount: HOME_SECOND_FOOD * density / 100,
        cache: false,
    });
}

/// An ordinary chamber's nodes: one of each resource (TRIAGE row 51), a
/// third of a turn apart, about halfway out, so a queen still fits in the
/// middle.
fn chamber_nodes(rng: &mut Rng, c: &ChamberPlan, density: i32, out: &mut Vec<NodeSpot>) {
    let a0 = rng.range(0, 63) as usize;
    let d = (c.r * 55 / 100).clamp(4, 6);
    for (k, res) in Resource::ALL.into_iter().enumerate() {
        let (lo, hi) = CHAMBER_AMOUNTS[k];
        out.push(NodeSpot {
            pos: spot_at(c.cx, c.cy, d, (a0 + k * 21) % 64),
            resource: res,
            amount: rng.range(lo, hi) * density / 100,
            cache: false,
        });
    }
}

/// The centre chamber's nodes, placed so every seat is equally far.
fn centre_nodes(w: i32, h: i32, seats: usize, base_r: i32, density: i32, out: &mut Vec<NodeSpot>) {
    let centre = Pos::new(w * CELL / 2, h * CELL / 2);
    for (k, res) in Resource::ALL.into_iter().enumerate() {
        let (a, amount) = if seats == 2 {
            (k * 21 + 5, CENTRE_AMOUNTS[0])
        } else {
            (k * 16 + 8, CENTRE_AMOUNTS[1])
        };
        let d = (base_r / 2) * CELL;
        let p = Pos::new(
            centre.x + d * cos64(a) / TRIG_ONE,
            centre.y + d * sin64(a) / TRIG_ONE,
        );
        for t in 0..seats {
            out.push(NodeSpot {
                pos: rotate_pos(w, h, seats, t, p),
                resource: res,
                amount: amount * density / 100,
                cache: false,
            });
        }
    }
}

/// The Tunnels map: chambers joined by tunnels.
fn generate_tunnels(seed: u64, settings: &MatchSettings) -> Map {
    let seats = settings.format.seats();
    let (w, h) = dimensions(seats, settings.map_size);
    let mut rng = Rng::new(seed ^ 0x6D61_7067_656E);
    let turns_total = seats;
    let base_r = match settings.chamber_size {
        Level::Low => 8,
        Level::Normal => 10,
        Level::High => 13,
    };
    let tunnel_w = settings.tunnel_width;
    let half = (tunnel_w + 1) / 2;

    // 1. Representative chambers in sector 0.
    let (sx0, sy0, sx1, sy1) = if seats == 2 {
        (0, 0, w / 2, h)
    } else {
        (0, 0, w / 2, h / 2)
    };
    let mut reps: Vec<ChamberPlan> = Vec::new();
    let home_r = base_r + 5;
    let (hx, hy) = home_spot(&mut rng, w, h, seats);
    reps.push(ChamberPlan {
        cx: hx,
        cy: hy,
        r: home_r,
        home: true,
    });
    let spacing = base_r * 2 + tunnel_w * 2 + 8;
    let target = ((sx1 - sx0) * (sy1 - sy0) / (spacing * spacing)).clamp(3, 9);
    let mut tries = 0;
    while (reps.len() as i32) < target + 1 && tries < 600 {
        tries += 1;
        let r = base_r + rng.range(-2, 3);
        let m = r + 3;
        let cx = rng.range(sx0 + m, sx1 - m / 2);
        let cy = rng.range(sy0 + m, sy1 - m);
        let ok = reps.iter().all(|c| {
            let d = isqrt_i((c.cx - cx).pow(2) + (c.cy - cy).pow(2));
            d >= c.r + r + tunnel_w
        });
        // Keep clear of the centre chamber.
        let dc = isqrt_i((w / 2 - cx).pow(2) + (h / 2 - cy).pow(2));
        if ok && dc >= base_r + 2 + r + tunnel_w {
            reps.push(ChamberPlan {
                cx,
                cy,
                r,
                home: false,
            });
        }
    }

    // 2. Every chamber: rotations of the representatives, then the centre.
    let mut all: Vec<(ChamberPlan, Option<u8>)> = Vec::new();
    for t in 0..turns_total {
        for c in &reps {
            let (x, y) = rotate(w, h, seats, t, c.cx, c.cy);
            all.push((
                ChamberPlan {
                    cx: x,
                    cy: y,
                    ..c.clone()
                },
                if c.home { Some(t as u8) } else { None },
            ));
        }
    }
    let centre_idx = all.len();
    all.push((
        ChamberPlan {
            cx: w / 2,
            cy: h / 2,
            r: base_r + 2,
            home: false,
        },
        None,
    ));
    let per = reps.len();
    // Orbit of chamber i under one rotation step.
    let orbit_next = |i: usize| -> usize {
        if i == centre_idx {
            centre_idx
        } else {
            (i + per) % (per * turns_total)
        }
    };

    // 3. Tunnels: Kruskal over orbit-closed candidate edges.
    let mut cands: Vec<(i32, usize, usize)> = Vec::new();
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            let a = &all[i].0;
            let b = &all[j].0;
            let d = isqrt_i((a.cx - b.cx).pow(2) + (a.cy - b.cy).pow(2));
            if d <= spacing * 3 / 2 + base_r {
                cands.push((d, i, j));
            }
        }
    }
    // Every chamber's two nearest neighbours are candidates at any distance,
    // so nothing (the centre especially) is left without a tunnel.
    for i in 0..all.len() {
        let mut near: Vec<(i32, usize)> = (0..all.len())
            .filter(|j| *j != i)
            .map(|j| {
                let a = &all[i].0;
                let b = &all[j].0;
                (isqrt_i((a.cx - b.cx).pow(2) + (a.cy - b.cy).pow(2)), j)
            })
            .collect();
        near.sort();
        for &(d, j) in near.iter().take(2) {
            let e = if i < j { (d, i, j) } else { (d, j, i) };
            if !cands.contains(&e) {
                cands.push(e);
            }
        }
    }
    cands.sort();
    let mut parent: Vec<usize> = (0..all.len()).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let n = p[c];
            p[c] = r;
            c = n;
        }
        r
    }
    let norm = |i: usize, j: usize| if i < j { (i, j) } else { (j, i) };
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let mut tree: Vec<(usize, usize)> = Vec::new();
    for &(_, i, j) in &cands {
        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
        if a != b {
            let mut e = (i, j);
            for _ in 0..turns_total {
                let n = norm(e.0, e.1);
                if !taken.contains(&n) {
                    taken.push(n);
                    tree.push(n);
                }
                let (pa, pb) = (find(&mut parent, n.0), find(&mut parent, n.1));
                parent[pa] = pb;
                e = (orbit_next(e.0), orbit_next(e.1));
            }
        }
    }
    // Extra loops: some of the remaining short edges, by orbit.
    let mut extras: Vec<(usize, usize)> = Vec::new();
    for &(_, i, j) in &cands {
        let n = norm(i, j);
        if taken.contains(&n) {
            continue;
        }
        let keep = rng.chance(25);
        let mut e = (i, j);
        for _ in 0..turns_total {
            let n = norm(e.0, e.1);
            if !taken.contains(&n) {
                taken.push(n);
                if keep {
                    extras.push(n);
                }
            }
            e = (orbit_next(e.0), orbit_next(e.1));
        }
    }

    // 4. Carve sector representatives into the mask; rotations come later.
    let mut mask = Mask {
        w,
        h,
        cells: vec![0; (w * h) as usize],
    };
    for c in &reps {
        let radii = blob(&mut rng, c.r);
        carve_blob(&mut mask, c.cx, c.cy, &radii);
    }
    let centre_radii = blob(&mut rng, all[centre_idx].0.r);
    carve_blob(&mut mask, w / 2, h / 2, &centre_radii);
    // Representative edges: those whose first end is in sector 0 or the centre.
    let is_rep_edge = |e: &(usize, usize)| -> bool {
        // An edge orbit is carved once: by its member with the smallest
        // (first, second) pair, which always touches sector 0 or the centre.
        let mut best = *e;
        let mut cur = *e;
        for _ in 0..turns_total {
            cur = (orbit_next(cur.0), orbit_next(cur.1));
            let n = norm(cur.0, cur.1);
            if n < best {
                best = n;
            }
        }
        best == *e
    };
    let mut dirt_spots: Vec<(i32, i32)> = Vec::new();
    for e in tree.iter().chain(extras.iter()) {
        if !is_rep_edge(e) {
            continue;
        }
        let a = &all[e.0].0;
        let b = &all[e.1].0;
        carve_tunnel(&mut mask, &mut rng, (a.cx, a.cy), (b.cx, b.cy), half);
    }
    for e in extras.iter() {
        if is_rep_edge(e) && rng.chance(30) {
            let a = &all[e.0].0;
            let b = &all[e.1].0;
            dirt_spots.push(((a.cx + b.cx) / 2, (a.cy + b.cy) / 2));
        }
    }

    // 5. Nodes in sector 0: home nodes, chamber nodes; centre nodes once.
    let density = settings.resource_density.percent();
    let mut rep_nodes: Vec<NodeSpot> = Vec::new();
    home_nodes(&mut rng, &reps[0], density, &mut rep_nodes);
    for c in reps.iter().skip(1) {
        chamber_nodes(&mut rng, c, density, &mut rep_nodes);
    }

    // 6. Cache alcoves: a small chamber behind a dirt plug.
    let mut alcoves: Vec<(i32, i32, i32)> = Vec::new();
    for c in reps.iter().skip(1) {
        if !rng.chance(45) {
            continue;
        }
        let a = rng.range(0, 63) as usize;
        let ar = half + 1;
        let dist = c.r + ar + 4;
        let ax = c.cx + dist * cos64(a) / TRIG_ONE;
        let ay = c.cy + dist * sin64(a) / TRIG_ONE;
        // Must sit in rock, inside the map, clear of the centre's orbit.
        let mut clear =
            ax - ar - 2 > 1 && ay - ar - 2 > 1 && ax + ar + 2 < w - 1 && ay + ar + 2 < h - 1;
        if clear {
            'scan: for y in ay - ar - 2..=ay + ar + 2 {
                for x in ax - ar - 2..=ax + ar + 2 {
                    if mask.cells[(y * w + x) as usize] != 0 {
                        clear = false;
                        break 'scan;
                    }
                }
            }
        }
        if !clear {
            continue;
        }
        alcoves.push((ax, ay, ar));
        mask.disk(ax, ay, ar, 1);
        // Connector from the chamber edge, all dirt.
        let ex = c.cx + (c.r - 1) * cos64(a) / TRIG_ONE;
        let ey = c.cy + (c.r - 1) * sin64(a) / TRIG_ONE;
        let seg = isqrt_i((ax - ex).pow(2) + (ay - ey).pow(2)).max(1);
        for i in 0..=seg {
            let x = ex + (ax - ex) * i / seg;
            let y = ey + (ay - ey) * i / seg;
            let i_cell = (y * w + x) as usize;
            if mask.cells.get(i_cell) == Some(&0) || i * 3 > seg {
                // Dirt only where there was rock, so the connector never
                // blocks an existing tunnel or chamber.
                for qy in y - half..=y + half {
                    for qx in x - half..=x + half {
                        let inside = (qx - x).pow(2) + (qy - y).pow(2) <= half * half + half;
                        if inside && qx > 0 && qy > 0 && qx < w - 1 && qy < h - 1 {
                            let q = (qy * w + qx) as usize;
                            if mask.cells[q] == 0 {
                                mask.cells[q] = 2;
                            }
                        }
                    }
                }
            }
        }
        let res = Resource::ALL[rng.below(3) as usize];
        rep_nodes.push(NodeSpot {
            pos: Pos::of_cell(ax, ay),
            resource: res,
            amount: CACHE_AMOUNT * density / 100,
            cache: true,
        });
    }
    // Dirt plugs on some loop tunnels, away from chambers.
    for (dx, dy) in &dirt_spots {
        let near_chamber = all.iter().any(|(c, _)| {
            let d = isqrt_i((c.cx - dx).pow(2) + (c.cy - dy).pow(2));
            d <= c.r + half + 3
        });
        if near_chamber {
            continue;
        }
        let r = half + 2;
        for y in dy - r..=dy + r {
            for x in dx - r..=dx + r {
                if (x - dx).pow(2) + (y - dy).pow(2) <= r * r
                    && x > 0
                    && y > 0
                    && x < w - 1
                    && y < h - 1
                {
                    let i = (y * w + x) as usize;
                    if mask.cells[i] == 1 {
                        mask.cells[i] = 2;
                    }
                }
            }
        }
    }

    // 7. Union of rotations.
    let mut ground = vec![Ground::Rock; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let v = mask.cells[(y * w + x) as usize];
            if v == 0 {
                continue;
            }
            for t in 0..turns_total {
                let (rx, ry) = rotate(w, h, seats, t, x, y);
                let i = (ry * w + rx) as usize;
                let g = if v == 2 { Ground::Dirt } else { Ground::Floor };
                if g > ground[i] {
                    ground[i] = g;
                }
            }
        }
    }
    // Centre nodes: one of each, placed symmetrically when possible.
    let mut nodes: Vec<NodeSpot> = Vec::new();
    for t in 0..turns_total {
        for n in &rep_nodes {
            nodes.push(NodeSpot {
                pos: rotate_pos(w, h, seats, t, n.pos),
                ..n.clone()
            });
        }
    }
    centre_nodes(w, h, seats, base_r, density, &mut nodes);

    let mut map = Map {
        w,
        h,
        ground,
        chambers: all
            .into_iter()
            .map(|(c, home)| Chamber {
                cx: c.cx,
                cy: c.cy,
                r: c.r,
                home,
            })
            .collect(),
        nodes,
        centre: centre_idx,
        seats,
    };
    open_to_width(&mut map, tunnel_w);
    // Nodes must stand on floor or dirt (alcove caches); drop any on rock.
    seal_unreachable(&mut map);
    map
}

/// Makes a layer symmetric: every cell takes the largest value in its orbit
/// under the map's rotations.
fn symmetrize(w: i32, h: i32, seats: usize, layer: &mut [u8]) {
    let src = layer.to_vec();
    for y in 0..h {
        for x in 0..w {
            let mut v = 0;
            for t in 0..seats {
                let (rx, ry) = rotate(w, h, seats, t, x, y);
                v = v.max(src[(ry * w + rx) as usize]);
            }
            layer[(y * w + x) as usize] = v;
        }
    }
}

/// A layer the size of the map that the Open and Maze generators paint.
struct Layer {
    w: i32,
    h: i32,
    v: Vec<u8>,
}

impl Layer {
    fn new(w: i32, h: i32) -> Layer {
        Layer {
            w,
            h,
            v: vec![0; (w * h) as usize],
        }
    }

    /// Raises a cell to `v`; the map's outer ring is never painted.
    fn raise(&mut self, x: i32, y: i32, v: u8) {
        if x >= 1 && y >= 1 && x < self.w - 1 && y < self.h - 1 {
            let i = (y * self.w + x) as usize;
            self.v[i] = self.v[i].max(v);
        }
    }

    fn disk(&mut self, cx: i32, cy: i32, r: i32, v: u8) {
        for (dx, dy) in disk_offsets(r) {
            self.raise(cx + dx, cy + dy, v);
        }
    }

    /// A straight band from a to b, every point a disk of radius `r`.
    fn line(&mut self, a: (i32, i32), b: (i32, i32), r: i32, v: u8) {
        let seg = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        for i in 0..=seg {
            let x = a.0 + (b.0 - a.0) * i / seg;
            let y = a.1 + (b.1 - a.1) * i / seg;
            self.disk(x, y, r, v);
        }
    }
}

/// The cells of a blob outline (from `blob`), as offsets from its centre.
fn blob_cells(radii: &[i32], angles: &Angles) -> Vec<(i32, i32)> {
    let rmax = *radii.iter().max().unwrap_or(&1);
    let rmin = *radii.iter().min().unwrap_or(&1);
    let mut v = Vec::new();
    for dy in -rmax..=rmax {
        for dx in -rmax..=rmax {
            let d2 = dx * dx + dy * dy;
            if d2 > rmax * rmax {
                continue;
            }
            if d2 <= rmin * rmin {
                v.push((dx, dy));
                continue;
            }
            let r = radii[angles.step(dx, dy)];
            if d2 <= r * r {
                v.push((dx, dy));
            }
        }
    }
    v
}

/// The nearest of the 64 angle steps for every offset within `r`, worked
/// out once per map (as `carve_blob` does cell by cell).
struct Angles {
    r: i32,
    step: Vec<u8>,
}

impl Angles {
    fn new(r: i32) -> Angles {
        let mut step = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
        for dy in -r..=r {
            for dx in -r..=r {
                let mut best = 0;
                let mut best_dot = i64::MIN;
                for s in 0..64 {
                    let dot = dx as i64 * cos64(s) as i64 + dy as i64 * sin64(s) as i64;
                    if dot > best_dot {
                        best_dot = dot;
                        best = s;
                    }
                }
                step.push(best as u8);
            }
        }
        Angles { r, step }
    }

    fn step(&self, dx: i32, dy: i32) -> usize {
        let n = 2 * self.r + 1;
        self.step[((dy + self.r) * n + dx + self.r) as usize] as usize
    }
}

fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    isqrt_i((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2))
}

/// Ground layers for the Open and Maze maps, painted in sector 0 (or
/// anywhere, for orbit-closed features) and made symmetric afterwards.
struct Paint {
    /// 0 floor, 1 dirt, 2 rock: the obstacles; the largest wins.
    solid: Layer,
    /// 1: dirt, over rock (the shell of a cache mound).
    shell: Layer,
    /// 1: floor, over everything (chambers and cache pockets).
    hollow: Layer,
}

/// The Open and Maze maps: open ground with rock in it, blobs for Open and
/// thin walls for Maze. Chambers are clearings kept free of rock, laid out
/// as in Tunnels, so homes, the centre, expansions and Windfalls work the
/// same way.
///
/// `rescue` runs `connect_homes`; only a test turns it off.
fn generate_open(seed: u64, settings: &MatchSettings, rescue: bool) -> Map {
    let seats = settings.format.seats();
    let (w, h) = dimensions(seats, settings.map_size);
    let maze = settings.map_type == MapType::Maze;
    let mut rng = Rng::new(
        seed ^ if maze {
            0x6D61_7A65_6765
        } else {
            0x6F70_656E_6765
        },
    );
    let base_r = match settings.chamber_size {
        Level::Low => 8,
        Level::Normal => 10,
        Level::High => 13,
    };
    let tw = settings.tunnel_width;
    let half = (tw + 1) / 2;
    let density = settings.resource_density.percent();
    let (sx1, sy1) = if seats == 2 {
        (w / 2, h)
    } else {
        (w / 2, h / 2)
    };
    let rot = |t: usize, p: (i32, i32)| rotate(w, h, seats, t, p.0, p.1);

    // 1. Chambers: the home and some clearings in sector 0, then every
    // rotation, then the centre (the same layout as Tunnels, which the
    // Windfall event relies on). In a Maze they sit in lattice cells.
    let lattice = if maze {
        Some(Lattice::new(w, h, seats, base_r, tw))
    } else {
        None
    };
    let home_r = base_r + 5;
    let (mut hx, mut hy) = home_spot(&mut rng, w, h, seats);
    if let Some(lat) = &lattice {
        (hx, hy) = lat.snap((hx, hy));
    }
    let mut reps = vec![ChamberPlan {
        cx: hx,
        cy: hy,
        r: home_r,
        home: true,
    }];
    let centre_r = base_r + 2;
    let spacing = base_r * 2 + tw * 2 + 8;
    let target = (sx1 * sy1 / (spacing * spacing)).clamp(3, 9);
    let mut tries = 0;
    while (reps.len() as i32) < target + 1 && tries < 600 {
        tries += 1;
        let r = base_r + rng.range(-2, 3);
        let m = r + 3;
        let mut p = (rng.range(m, sx1 - m / 2), rng.range(m, sy1 - m));
        if let Some(lat) = &lattice {
            p = lat.snap(p);
        }
        let ok = (0..seats).all(|t| {
            reps.iter()
                .all(|c| dist(rot(t, (c.cx, c.cy)), p) >= c.r + r + tw)
                && (t == 0 || dist(rot(t, p), p) >= 2 * r + tw)
        });
        if ok && dist((w / 2, h / 2), p) >= centre_r + r + tw {
            reps.push(ChamberPlan {
                cx: p.0,
                cy: p.1,
                r,
                home: false,
            });
        }
    }
    let mut all: Vec<(ChamberPlan, Option<u8>)> = Vec::new();
    for t in 0..seats {
        for c in &reps {
            let (x, y) = rot(t, (c.cx, c.cy));
            all.push((
                ChamberPlan {
                    cx: x,
                    cy: y,
                    ..c.clone()
                },
                if c.home { Some(t as u8) } else { None },
            ));
        }
    }
    let centre_idx = all.len();
    all.push((
        ChamberPlan {
            cx: w / 2,
            cy: h / 2,
            r: centre_r,
            home: false,
        },
        None,
    ));

    let mut rep_nodes: Vec<NodeSpot> = Vec::new();
    home_nodes(&mut rng, &reps[0], density, &mut rep_nodes);
    for c in reps.iter().skip(1) {
        chamber_nodes(&mut rng, c, density, &mut rep_nodes);
    }

    // 2. Rock and dirt.
    let mut paint = Paint {
        solid: Layer::new(w, h),
        shell: Layer::new(w, h),
        hollow: Layer::new(w, h),
    };
    // One or two caches per seat, about what Tunnels has in alcoves.
    let caches = if rng.chance(50) { 2 } else { 1 };
    let pocket = half + 1;
    let mound = pocket + 3;
    // How far each chamber's clearing reaches. In a Maze an ordinary
    // chamber is its lattice cell, so its clearing stays inside the cell's
    // walls; the homes and the centre clear a wider square.
    let clear: Vec<i32> = all
        .iter()
        .enumerate()
        .map(|(i, (c, _))| match &lattice {
            Some(lat) if !c.home && i != centre_idx => (c.r + 1).min(lat.s / 2 - 3),
            _ => c.r + 1,
        })
        .collect();
    let ctx = Ctx {
        w,
        h,
        seats,
        base_r,
        tw,
        sx1,
        sy1,
        chambers: all.iter().map(|(c, _)| c.clone()).collect(),
        clear: clear.clone(),
    };
    let cache_spots = if maze {
        paint_maze(&ctx, &mut rng, &mut paint, caches)
    } else {
        paint_blobs(&ctx, &mut rng, &mut paint, caches, pocket, mound)
    };
    for (x, y) in cache_spots {
        rep_nodes.push(NodeSpot {
            pos: Pos::of_cell(x, y),
            resource: Resource::ALL[rng.below(3) as usize],
            amount: CACHE_AMOUNT * density / 100,
            cache: true,
        });
    }
    // Clearings: every chamber is open ground.
    for ((c, _), r) in all.iter().zip(&clear) {
        paint.hollow.disk(c.cx, c.cy, *r, 1);
    }
    symmetrize(w, h, seats, &mut paint.solid.v);
    symmetrize(w, h, seats, &mut paint.shell.v);
    symmetrize(w, h, seats, &mut paint.hollow.v);
    let mut ground = vec![Ground::Rock; (w * h) as usize];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = (y * w + x) as usize;
            ground[i] = if paint.hollow.v[i] == 1 {
                Ground::Floor
            } else if paint.shell.v[i] == 1 {
                Ground::Dirt
            } else {
                match paint.solid.v[i] {
                    0 => Ground::Floor,
                    1 => Ground::Dirt,
                    _ => Ground::Rock,
                }
            };
        }
    }

    // 3. Nodes: sector 0's in every rotation, then the centre's.
    let mut nodes: Vec<NodeSpot> = Vec::new();
    for t in 0..seats {
        for n in &rep_nodes {
            nodes.push(NodeSpot {
                pos: rotate_pos(w, h, seats, t, n.pos),
                ..n.clone()
            });
        }
    }
    centre_nodes(w, h, seats, base_r, density, &mut nodes);

    let mut map = Map {
        w,
        h,
        ground,
        chambers: all
            .into_iter()
            .map(|(c, home)| Chamber {
                cx: c.cx,
                cy: c.cy,
                r: c.r,
                home,
            })
            .collect(),
        nodes,
        centre: centre_idx,
        seats,
    };
    open_to_width(&mut map, tw);
    if rescue {
        connect_homes(&mut map, tw);
    }
    drop_specks(&mut map, 12);
    // A dropped speck can leave floor in a gap narrower than the setting;
    // open again (this only ever trims those cells).
    open_to_width(&mut map, tw);
    seal_unreachable(&mut map);
    map
}

/// What the Open and Maze painters need to know about the map.
struct Ctx {
    w: i32,
    h: i32,
    seats: usize,
    base_r: i32,
    tw: i32,
    /// Sector 0 is `0..sx1` by `0..sy1`.
    sx1: i32,
    sy1: i32,
    chambers: Vec<ChamberPlan>,
    /// The radius each chamber's clearing reaches.
    clear: Vec<i32>,
}

impl Ctx {
    fn rot(&self, t: usize, p: (i32, i32)) -> (i32, i32) {
        rotate(self.w, self.h, self.seats, t, p.0, p.1)
    }
}

/// Open: rock blobs (some with dirt parts, some all dirt) and dirt mounds
/// with a cache in a hollow, scattered over sector 0. Every feature keeps at
/// least a Tunnel Width plus two cells from every other feature and every
/// rotation of one, cell by cell, so the open ground between them is one
/// connected region at least that wide. Some blobs sit on a map edge (see
/// below); the rest, and the mounds, keep that far from every edge. Returns
/// the cache cells.
fn paint_blobs(
    ctx: &Ctx,
    rng: &mut Rng,
    paint: &mut Paint,
    caches: usize,
    pocket: i32,
    mound: i32,
) -> Vec<(i32, i32)> {
    let (w, h, tw, base_r, seats) = (ctx.w, ctx.h, ctx.tw, ctx.base_r, ctx.seats);
    let gap = tw + 2;
    let gap_disk = disk_offsets(gap);
    // Blob radii reach r + r / 3 for r up to three times `base_r`.
    let angles = Angles::new(base_r * 4 + 2);
    // Cells within `gap` of a placed feature or any of its rotations.
    let mut near = vec![false; (w * h) as usize];
    let inside = |x: i32, y: i32| x >= 1 && y >= 1 && x < w - 1 && y < h - 1;
    // No feature covers a chamber's middle, so no clearing ends up as a
    // hole inside a blob: each one opens onto the ground around.
    for c in &ctx.chambers {
        for (dx, dy) in disk_offsets(tw / 2 + 2) {
            if inside(c.cx + dx, c.cy + dy) {
                near[((c.cy + dy) * w + c.cx + dx) as usize] = true;
            }
        }
    }
    // Placed features as (centre, smallest radius), for a quick rejection.
    let mut placed: Vec<((i32, i32), i32)> = Vec::new();
    let mut cache_spots = Vec::new();

    // A candidate's cells fit if none is near a placed feature, it keeps
    // clear of its own rotations, and the homes and centre keep open ground
    // round them; an ordinary chamber's clearing may cut into a blob.
    let fits =
        |near: &[bool], cells: &[(i32, i32, u8)], p: (i32, i32), bound: i32, plain: i32| -> bool {
            if (1..seats).any(|t| dist(ctx.rot(t, p), p) < 2 * bound + gap) {
                return false;
            }
            let last = ctx.chambers.len() - 1;
            let chambers_ok = ctx.chambers.iter().enumerate().all(|(i, c)| {
                let d = dist((c.cx, c.cy), p) - c.r;
                if c.home || i == last {
                    d >= bound + tw / 2
                } else {
                    d >= plain
                }
            });
            chambers_ok
                && cells
                    .iter()
                    .all(|&(x, y, _)| !inside(x, y) || !near[(y * w + x) as usize])
        };
    // Marks a placed feature, in every rotation, and the cells near it.
    let mark = |near: &mut Vec<bool>, cells: &[(i32, i32, u8)]| {
        let mut own = vec![false; 0];
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &(x, y, _) in cells {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let bw = x1 - x0 + 3;
        own.resize(((x1 - x0 + 3) * (y1 - y0 + 3)) as usize, false);
        let at = |x: i32, y: i32| ((y - y0 + 1) * bw + (x - x0 + 1)) as usize;
        for &(x, y, _) in cells {
            own[at(x, y)] = true;
        }
        for &(x, y, _) in cells {
            let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| !own[at(x + dx, y + dy)]);
            for t in 0..seats {
                let (rx, ry) = ctx.rot(t, (x, y));
                if !inside(rx, ry) {
                    continue;
                }
                near[(ry * w + rx) as usize] = true;
                if edge {
                    for (dx, dy) in &gap_disk {
                        if inside(rx + dx, ry + dy) {
                            near[((ry + dy) * w + rx + dx) as usize] = true;
                        }
                    }
                }
            }
        }
    };
    let quick = |placed: &[((i32, i32), i32)], p: (i32, i32), rmin: i32| -> bool {
        (0..seats).all(|t| {
            placed
                .iter()
                .all(|(q, qr)| dist(ctx.rot(t, *q), p) >= rmin + qr + gap)
        })
    };

    let mut tries = 0;
    while cache_spots.len() < caches && tries < 400 {
        tries += 1;
        let edge = mound + gap;
        let p = (rng.range(edge, ctx.sx1 - 1), rng.range(edge, ctx.sy1 - 1));
        if p.0 >= w - edge || p.1 >= h - edge || !quick(&placed, p, mound) {
            continue;
        }
        let cells: Vec<(i32, i32, u8)> = disk_offsets(mound)
            .into_iter()
            .map(|(dx, dy)| (p.0 + dx, p.1 + dy, 1))
            .collect();
        if !fits(&near, &cells, p, mound + 3 - tw / 2, mound + 3) {
            continue;
        }
        mark(&mut near, &cells);
        placed.push((p, mound));
        for &(x, y, v) in &cells {
            paint.solid.raise(x, y, v);
        }
        paint.hollow.disk(p.0, p.1, pocket, 1);
        cache_spots.push(p);
    }
    // Which map edges a cell is within `gap` of: left, top, right, bottom.
    let edges_near = |x: i32, y: i32| -> u8 {
        (x < gap) as u8
            | ((y < gap) as u8) << 1
            | ((x >= w - gap) as u8) << 2
            | ((y >= h - gap) as u8) << 3
    };
    // Sector 0 touches the left and top edges, and in a 1v1 the bottom.
    let sector_edges: &[u8] = if seats == 2 { &[1, 2, 8] } else { &[1, 2] };
    // Blobs: large ones first, then smaller ones in the spaces left. Some
    // sit on a map edge, centred on it; they may touch that edge but no
    // other, so with no lobe (each blob is star-shaped about its centre)
    // they never close off a pocket against the edge.
    for k in 0..2400 {
        let r = if k < 1200 {
            rng.range(base_r * 3 / 2, base_r * 3)
        } else {
            rng.range(base_r / 2, base_r * 3 / 2)
        };
        let on_edge = if rng.chance(25) {
            sector_edges[rng.below(sector_edges.len() as u32) as usize]
        } else {
            0
        };
        let p = match on_edge {
            1 => (0, rng.range(1, ctx.sy1 - 1)),
            2 => (rng.range(1, ctx.sx1 - 1), 0),
            8 => (rng.range(1, ctx.sx1 - 1), h - 1),
            _ => (rng.range(1, ctx.sx1 - 1), rng.range(1, ctx.sy1 - 1)),
        };
        if (inside(p.0, p.1) && near[(p.1 * w + p.0) as usize]) || !quick(&placed, p, r) {
            continue;
        }
        // An optional second lobe makes longer, less round shapes; it may
        // be dirt, and some whole blobs are dirt.
        let v = if on_edge == 0 && rng.chance(12) { 1 } else { 2 };
        let mut cells: Vec<(i32, i32, u8)> = blob_cells(&blob(rng, r), &angles)
            .into_iter()
            .map(|(dx, dy)| (p.0 + dx, p.1 + dy, v))
            .collect();
        let mut bound = r + r / 3 + 1;
        if on_edge != 0 {
            let other_edge = cells
                .iter()
                .any(|&(x, y, _)| edges_near(x, y) & !on_edge != 0);
            if other_edge {
                continue;
            }
        } else if rng.chance(45) {
            let a = rng.range(0, 63) as usize;
            let off = r * 3 / 4;
            let lr = (r * 2 / 3).max(2);
            let (lx, ly) = (
                p.0 + off * cos64(a) / TRIG_ONE,
                p.1 + off * sin64(a) / TRIG_ONE,
            );
            let lv = if rng.chance(35) { 1 } else { v };
            cells.extend(
                blob_cells(&blob(rng, lr), &angles)
                    .into_iter()
                    .map(|(dx, dy)| (lx + dx, ly + dy, lv)),
            );
            bound = bound.max(off + lr + lr / 3 + 1);
        }
        if on_edge == 0 && cells.iter().any(|&(x, y, _)| edges_near(x, y) != 0) {
            continue;
        }
        if !fits(&near, &cells, p, bound, 2) {
            continue;
        }
        mark(&mut near, &cells);
        placed.push((p, r));
        for &(x, y, v) in &cells {
            paint.solid.raise(x, y, v);
        }
    }
    cache_spots
}

/// The Maze's square lattice of posts: `nx` by `ny` cells about `s` cells
/// across, posts mirrored so the lattice is (nearly) symmetric.
struct Lattice {
    w: i32,
    h: i32,
    s: i32,
    nx: i32,
    ny: i32,
}

impl Lattice {
    fn new(w: i32, h: i32, seats: usize, base_r: i32, tw: i32) -> Lattice {
        let s = (2 * base_r + 2).max(tw + 6);
        let nx = ((w - 1 + s / 2) / s).max(2);
        let ny = if seats == 4 {
            nx
        } else {
            ((h - 1 + s / 2) / s).max(2)
        };
        Lattice { w, h, s, nx, ny }
    }

    fn coord(i: i32, n: i32, len: i32) -> i32 {
        if 2 * i <= n {
            i * (len - 1) / n
        } else {
            len - 1 - (n - i) * (len - 1) / n
        }
    }

    fn post(&self, (i, j): (i32, i32)) -> (i32, i32) {
        (
            Lattice::coord(i, self.nx, self.w),
            Lattice::coord(j, self.ny, self.h),
        )
    }

    /// The centre of the lattice cell holding `p`.
    fn snap(&self, p: (i32, i32)) -> (i32, i32) {
        let i = (0..self.nx)
            .find(|&i| self.post((i + 1, 0)).0 > p.0)
            .unwrap_or(self.nx - 1);
        let j = (0..self.ny)
            .find(|&j| self.post((0, j + 1)).1 > p.1)
            .unwrap_or(self.ny - 1);
        let (a, b) = (self.post((i, j)), self.post((i + 1, j + 1)));
        ((a.0 + b.0) / 2, (a.1 + b.1) / 2)
    }
}

/// Maze: thin walls along a square lattice of posts. Walls are chosen orbit
/// by orbit (a wall and its rotations together) so that the walls and the
/// map's edge never close a loop: the open ground is then one connected
/// region with no closed rooms. Some walls are left out (openings) and some
/// are dirt. The one exception is a vault: a lattice cell walled all round,
/// one side dirt, with a cache inside; its corners count as joined from the
/// start, so no other loop can form through it. Returns the cache cells.
fn paint_maze(ctx: &Ctx, rng: &mut Rng, paint: &mut Paint, caches: usize) -> Vec<(i32, i32)> {
    let (w, h, tw, seats) = (ctx.w, ctx.h, ctx.tw, ctx.seats);
    let lat = Lattice::new(w, h, seats, ctx.base_r, tw);
    let (s, nx, ny) = (lat.s, lat.nx, lat.ny);
    let post = |p: (i32, i32)| lat.post(p);
    let border = |(i, j): (i32, i32)| i == 0 || j == 0 || i == nx || j == ny;
    let node = |p: (i32, i32)| -> usize {
        if border(p) {
            0
        } else {
            (1 + p.1 * (nx + 1) + p.0) as usize
        }
    };
    let rot_idx = |(i, j): (i32, i32)| -> (i32, i32) {
        if seats == 2 {
            (nx - i, ny - j)
        } else {
            (nx - j, i)
        }
    };
    type Edge = ((i32, i32), (i32, i32));
    let norm = |a: (i32, i32), b: (i32, i32)| -> Edge {
        if (a.1, a.0) <= (b.1, b.0) {
            (a, b)
        } else {
            (b, a)
        }
    };
    let orbit_of = |e: Edge| -> Vec<Edge> {
        let mut v = vec![e];
        let mut cur = e;
        for _ in 1..seats {
            cur = norm(rot_idx(cur.0), rot_idx(cur.1));
            if !v.contains(&cur) {
                v.push(cur);
            }
        }
        v
    };
    let mut parent: Vec<usize> = (0..((nx + 1) * (ny + 1) + 1) as usize).collect();
    fn root(p: &[usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        r
    }
    let mut done: Vec<Edge> = Vec::new();
    // Walls: (a, b, value) with value 1 dirt, 2 rock.
    let mut walls: Vec<(Edge, u8)> = Vec::new();

    // 1. Vaults, in sector 0, clear of chambers, each other and the edge.
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for j in 1..ny - 1 {
        for i in 1..nx - 1 {
            let (a, b) = (post((i, j)), post((i + 1, j + 1)));
            let c = ((a.0 + b.0) / 2, (a.1 + b.1) / 2);
            if c.0 < ctx.sx1 && c.1 < ctx.sy1 {
                cells.push((i, j));
            }
        }
    }
    for k in (1..cells.len()).rev() {
        let m = rng.below(k as u32 + 1) as usize;
        cells.swap(k, m);
    }
    let mut vault_cells: Vec<(i32, i32)> = Vec::new();
    let mut cache_spots = Vec::new();
    for (i, j) in cells {
        if cache_spots.len() >= caches {
            break;
        }
        let (a, b) = (post((i, j)), post((i + 1, j + 1)));
        let c = ((a.0 + b.0) / 2, (a.1 + b.1) / 2);
        // No chamber's clearing may reach the vault's walls.
        let clear = ctx
            .chambers
            .iter()
            .zip(&ctx.clear)
            .all(|(ch, r)| dist((ch.cx, ch.cy), c) - r >= s * 3 / 4 + 2);
        if !clear {
            continue;
        }
        // The cell's corners and every rotation's corners must be fresh.
        let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
        let mut orbit_corners: Vec<Vec<(i32, i32)>> = vec![corners.to_vec()];
        for _ in 1..seats {
            let last = orbit_corners[orbit_corners.len() - 1].clone();
            orbit_corners.push(last.into_iter().map(rot_idx).collect());
        }
        let mut all_corners: Vec<(i32, i32)> = orbit_corners.concat();
        all_corners.sort();
        let distinct = all_corners.windows(2).all(|p| p[0] != p[1]);
        let fresh = all_corners.iter().all(|q| {
            vault_cells
                .iter()
                .all(|v| (q.0 - v.0).abs() > 2 || (q.1 - v.1).abs() > 2)
        });
        if !distinct || !fresh {
            continue;
        }
        vault_cells.push((i, j));
        let dirt_side = rng.below(4) as usize;
        for k in 0..4 {
            let e = norm(corners[k], corners[(k + 1) % 4]);
            let v = if k == dirt_side { 1 } else { 2 };
            for o in orbit_of(e) {
                done.push(o);
                walls.push((o, v));
                let (ra, rb) = (root(&parent, node(o.0)), root(&parent, node(o.1)));
                parent[ra] = rb;
            }
        }
        cache_spots.push(c);
    }

    // 2. Walls: every other lattice edge not along the map's edge, in a
    // random order, an orbit at a time while no loop forms.
    let mut edges: Vec<Edge> = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            if i < nx && !(border((i, j)) && border((i + 1, j))) {
                edges.push(((i, j), (i + 1, j)));
            }
            if j < ny && !(border((i, j)) && border((i, j + 1))) {
                edges.push(((i, j), (i, j + 1)));
            }
        }
    }
    for k in (1..edges.len()).rev() {
        let m = rng.below(k as u32 + 1) as usize;
        edges.swap(k, m);
    }
    for &e in &edges {
        if done.contains(&e) {
            continue;
        }
        let orbit = orbit_of(e);
        done.extend(orbit.iter().copied());
        let mut trial = parent.clone();
        let acyclic = orbit.iter().all(|(a, b)| {
            let (ra, rb) = (root(&trial, node(*a)), root(&trial, node(*b)));
            trial[ra] = rb;
            ra != rb
        });
        if !acyclic {
            continue;
        }
        parent = trial;
        // Some walls are left out: openings, so the maze has loops.
        if rng.chance(15) {
            continue;
        }
        let v = if rng.chance(12) { 1 } else { 2 };
        for o in orbit {
            walls.push((o, v));
        }
    }
    for ((a, b), v) in &walls {
        paint.solid.line(post(*a), post(*b), 1, *v);
    }
    cache_spots
}

/// Turns rock pieces smaller than `min` cells (the stubs a clearing leaves
/// of a wall or blob) into floor. Removing rock never narrows a passage.
fn drop_specks(map: &mut Map, min: usize) {
    let (w, h) = (map.w, map.h);
    let mut seen = vec![false; map.ground.len()];
    for start in 0..map.ground.len() {
        if seen[start] || map.ground[start] != Ground::Rock {
            continue;
        }
        let mut piece = vec![start];
        let mut stack = vec![start];
        seen[start] = true;
        let mut edge = false;
        while let Some(i) = stack.pop() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                edge = true;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if !seen[j] && map.ground[j] == Ground::Rock {
                    seen[j] = true;
                    stack.push(j);
                    piece.push(j);
                }
            }
        }
        if !edge && piece.len() < min {
            for i in piece {
                map.ground[i] = Ground::Floor;
            }
        }
    }
}

/// If seat 0 cannot walk to the centre or to every other home, cuts a
/// straight passage from each home to the centre (in every rotation) and
/// opens the map again. Clearances in the generators make this rare.
fn connect_homes(map: &mut Map, width: i32) {
    for _ in 0..3 {
        let h0 = map.home(0).clone();
        let reach = flood(map, h0.cx, h0.cy, false);
        let c = map.chambers[map.centre].clone();
        let ok = reach[map.idx(c.cx, c.cy)]
            && (0..map.seats).all(|s| {
                let h = map.home(s);
                reach[map.idx(h.cx, h.cy)]
            });
        if ok {
            return;
        }
        let mut cut = Layer::new(map.w, map.h);
        cut.line((h0.cx, h0.cy), (c.cx, c.cy), (width + 1) / 2 + 1, 1);
        symmetrize(map.w, map.h, map.seats, &mut cut.v);
        for (i, v) in cut.v.iter().enumerate() {
            if *v == 1 {
                map.ground[i] = Ground::Floor;
            }
        }
        open_to_width(map, width);
    }
}

/// Morphological opening with a disk: keeps only open cells (floor or dirt)
/// that lie in a fully open disk of radius `(width - 2) / 2`, so no passage,
/// at any angle, is narrower than the Tunnel Width setting allows.
fn open_to_width(map: &mut Map, width: i32) {
    open_ground(&mut map.ground, map.w, map.h, width);
}

fn open_ground(ground: &mut [Ground], w: i32, h: i32, width: i32) {
    let r = (width - 2) / 2;
    let open: Vec<bool> = ground.iter().map(|g| *g != Ground::Rock).collect();
    let full = disk_all(&open, w, h, r);
    let keep = disk_any(&full, w, h, r);
    for (i, g) in ground.iter_mut().enumerate() {
        if !keep[i] {
            *g = Ground::Rock;
        }
    }
}

/// Half-widths of `disk_offsets(r)`, row by row from `-r` to `r`.
fn disk_rows(r: i32) -> Vec<i32> {
    (-r..=r)
        .map(|dy| {
            (0..=r)
                .rev()
                .find(|dx| dx * dx + dy * dy <= r * r + r)
                .unwrap_or(0)
        })
        .collect()
}

/// Running counts of `v` along each row: `out[y * (w + 1) + x]` counts
/// the cells before x.
fn row_sums(v: &[bool], w: i32, h: i32) -> Vec<u32> {
    let mut out = vec![0u32; ((w + 1) * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = (y * (w + 1) + x) as usize;
            out[i + 1] = out[i] + v[(y * w + x) as usize] as u32;
        }
    }
    out
}

/// For every cell, whether every cell of `disk_offsets(r)` round it is set
/// in `v` (cells off the map count as unset).
fn disk_all(v: &[bool], w: i32, h: i32, r: i32) -> Vec<bool> {
    let rows = disk_rows(r);
    let sums = row_sums(v, w, h);
    let mut out = vec![false; v.len()];
    for y in 0..h {
        for x in 0..w {
            out[(y * w + x) as usize] = rows.iter().enumerate().all(|(k, &hw)| {
                let yy = y + k as i32 - r;
                let (x0, x1) = (x - hw, x + hw);
                yy >= 0 && yy < h && x0 >= 0 && x1 < w && {
                    let base = (yy * (w + 1)) as usize;
                    sums[base + x1 as usize + 1] - sums[base + x0 as usize] == (x1 - x0 + 1) as u32
                }
            });
        }
    }
    out
}

/// For every cell, whether any cell of `disk_offsets(r)` round it is set
/// in `v`.
fn disk_any(v: &[bool], w: i32, h: i32, r: i32) -> Vec<bool> {
    let rows = disk_rows(r);
    let sums = row_sums(v, w, h);
    let mut out = vec![false; v.len()];
    for y in 0..h {
        for x in 0..w {
            out[(y * w + x) as usize] = rows.iter().enumerate().any(|(k, &hw)| {
                let yy = y + k as i32 - r;
                let (x0, x1) = ((x - hw).max(0), (x + hw).min(w - 1));
                yy >= 0 && yy < h && x0 <= x1 && {
                    let base = (yy * (w + 1)) as usize;
                    sums[base + x1 as usize + 1] > sums[base + x0 as usize]
                }
            });
        }
    }
    out
}

/// Cell offsets within radius `r` of the origin.
pub fn disk_offsets(r: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r + r {
                v.push((dx, dy));
            }
        }
    }
    v
}

// ---- Per-sector decoration (TRIAGE row 50) ----
//
// The layout that decides a match is laid out once and rotated, so it is
// fair by construction: chambers, the home chamber's shape and nodes, every
// other node, the tunnels between chambers, dirt and caches. Then each
// sector is decorated on its own, from its own random stream, so the map no
// longer looks like copies of one piece: chamber and tunnel outlines bulge
// into the rock here and there, and rock bumps and specks stand out of the
// floor. Decoration never touches the home chambers, the centre chamber,
// the ground round any node or chamber centre, or any dirt, and it never
// makes a passage narrower than the Tunnel Width. The decorated map is kept
// only if every chamber can still be walked to and the seats' walking
// distances (`seat_distances`) stay within `FAIR_PERCENT` of each other;
// otherwise the generator tries other decorations, and in the end keeps
// the plain rotated map.

/// How far the seats' walking distances may differ, in percent of the
/// shortest, for a decorated map to be kept.
pub const FAIR_PERCENT: i64 = 4;

/// Walking distances (10 a straight step, 14 a diagonal one, no cutting
/// past rock or dirt corners) over floor from (x, y) to every cell;
/// `u32::MAX` where it cannot be walked to.
pub fn walk_distances(map: &Map, x: i32, y: i32) -> Vec<u32> {
    let (w, h) = (map.w, map.h);
    let mut dist = vec![u32::MAX; map.ground.len()];
    if !map.is_floor(x, y) {
        return dist;
    }
    // Dial's algorithm: steps cost 10 or 14, so a ring of 15 buckets by
    // distance holds every cell still to settle.
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); 15];
    let start = map.idx(x, y);
    dist[start] = 0;
    buckets[0].push(start as u32);
    let mut pending = 1usize;
    let mut d = 0u32;
    while pending > 0 {
        let slot = (d % 15) as usize;
        while let Some(i) = buckets[slot].pop() {
            pending -= 1;
            let i = i as usize;
            if dist[i] != d {
                continue;
            }
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            for (dx, dy, c) in [
                (1, 0, 10),
                (-1, 0, 10),
                (0, 1, 10),
                (0, -1, 10),
                (1, 1, 14),
                (1, -1, 14),
                (-1, 1, 14),
                (-1, -1, 14),
            ] {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h || !map.is_floor(nx, ny) {
                    continue;
                }
                if dx != 0 && dy != 0 && !(map.is_floor(cx + dx, cy) && map.is_floor(cx, cy + dy)) {
                    continue;
                }
                let ni = (ny * w + nx) as usize;
                if d + c < dist[ni] {
                    dist[ni] = d + c;
                    buckets[((d + c) % 15) as usize].push(ni as u32);
                    pending += 1;
                }
            }
        }
        d += 1;
    }
    dist
}

/// For each seat, the walking distances that decide fairness: home to the
/// centre; home to every other home, in seat order after its own (so the
/// same column is the same neighbour under rotation); home to its three
/// nearest ordinary chambers, nearest first. `u32::MAX` if one cannot be
/// walked to.
pub fn seat_distances(map: &Map) -> Vec<Vec<u32>> {
    (0..map.seats)
        .map(|s| {
            let h = map.home(s);
            let d = walk_distances(map, h.cx, h.cy);
            let at = |c: &Chamber| d[map.idx(c.cx, c.cy)];
            let mut v = vec![at(&map.chambers[map.centre])];
            for k in 1..map.seats {
                v.push(at(map.home((s + k) % map.seats)));
            }
            let mut near: Vec<u32> = map
                .chambers
                .iter()
                .enumerate()
                .filter(|(i, c)| c.home.is_none() && *i != map.centre)
                .map(|(_, c)| at(c))
                .collect();
            near.sort();
            near.truncate(3);
            v.extend(near);
            v
        })
        .collect()
}

/// Whether every column of `seat_distances` is within `percent` of its
/// shortest, with everything reachable.
pub fn distances_fair(d: &[Vec<u32>], percent: i64) -> bool {
    let cols = d.iter().map(|v| v.len()).min().unwrap_or(0);
    (0..cols).all(|k| {
        let lo = d.iter().map(|v| v[k]).min().unwrap_or(0) as i64;
        let hi = d.iter().map(|v| v[k]).max().unwrap_or(0) as i64;
        hi != u32::MAX as i64 && hi * 100 <= lo * (100 + percent)
    })
}

/// How much decoration a sector gets, per Map Type.
struct Decor {
    bulges: usize,
    bumps: usize,
    specks: usize,
}

fn decor_for(map_type: MapType) -> Decor {
    match map_type {
        MapType::Tunnels | MapType::Random => Decor {
            bulges: 10,
            bumps: 12,
            specks: 3,
        },
        MapType::Open => Decor {
            bulges: 5,
            bumps: 12,
            specks: 8,
        },
        MapType::Maze => Decor {
            bulges: 3,
            bumps: 10,
            specks: 4,
        },
    }
}

/// Decorates every sector on its own (see above).
/// Returns the attempt kept, or `None` for the plain rotated map.
fn decorate(map: &mut Map, seed: u64, tw: i32, map_type: MapType) -> Option<u64> {
    let base = map.ground.clone();
    let keep = keep_mask(map);
    let decor = decor_for(map_type);
    let reach_all = |m: &Map| {
        let h0 = m.home(0);
        let reach = flood(m, h0.cx, h0.cy, false);
        m.chambers.iter().all(|c| reach[m.idx(c.cx, c.cy)])
    };
    for attempt in 0..4u64 {
        let mut g = base.clone();
        let rngs = |pass: u64| -> Vec<Rng> {
            (0..map.seats as u64)
                .map(|t| Rng::new(seed ^ 0x6465_636F_0000 ^ (t + 1) << 24 ^ attempt << 12 ^ pass))
                .collect()
        };
        // Bulges first (floor only grows), then trim any sliver narrower
        // than the setting; then rock bumps and specks, each checked.
        // Each attempt after the first decorates a little less.
        let part = |n: usize| n * (4 - attempt as usize) / 4;
        let mut r1 = rngs(1);
        for (t, rng) in r1.iter_mut().enumerate() {
            for _ in 0..part(decor.bulges) {
                bulge(map, &mut g, &keep, rng, t, tw);
            }
        }
        open_ground(&mut g, map.w, map.h, tw);
        let mut width = Width::new(map, &g, tw);
        let mut r2 = rngs(2);
        for (t, rng) in r2.iter_mut().enumerate() {
            for _ in 0..part(decor.bumps) {
                bump(map, &mut g, &mut width, &keep, rng, t, tw);
            }
            for _ in 0..part(decor.specks) {
                speck(map, &mut g, &mut width, &keep, rng, t, tw);
            }
        }
        let mut trial = Map {
            ground: g,
            ..map.clone()
        };
        if !reach_all(&trial) {
            continue;
        }
        seal_unreachable(&mut trial);
        if trial.nodes.len() == map.nodes.len()
            && distances_fair(&seat_distances(&trial), FAIR_PERCENT)
        {
            *map = trial;
            return Some(attempt);
        }
    }
    None
}

/// Cells decoration leaves alone: the map's outer ring, the home chambers
/// and the centre chamber with a margin, the ground round every chamber
/// centre and node, and dirt with a margin.
fn keep_mask(map: &Map) -> Vec<bool> {
    let (w, h) = (map.w, map.h);
    let mut keep = vec![false; map.ground.len()];
    let mut mark = |cx: i32, cy: i32, r: i32| {
        for (dx, dy) in disk_offsets(r) {
            let (x, y) = (cx + dx, cy + dy);
            if map.inside(x, y) {
                keep[(y * w + x) as usize] = true;
            }
        }
    };
    for (i, c) in map.chambers.iter().enumerate() {
        let r = if c.home.is_some() || i == map.centre {
            c.r + c.r / 3 + 4
        } else {
            4
        };
        mark(c.cx, c.cy, r);
    }
    for n in &map.nodes {
        let (x, y) = n.pos.cell();
        mark(x, y, 5);
    }
    for y in 0..h {
        for x in 0..w {
            if map.at(x, y) == Ground::Dirt {
                mark(x, y, 3);
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if x < 2 || y < 2 || x >= w - 2 || y >= h - 2 {
                keep[(y * w + x) as usize] = true;
            }
        }
    }
    keep
}

/// A random cell of sector `t`: drawn in sector 0, then rotated.
fn sector_cell(map: &Map, rng: &mut Rng, t: usize) -> (i32, i32) {
    let (sx1, sy1) = if map.seats == 2 {
        (map.w / 2, map.h)
    } else {
        (map.w / 2, map.h / 2)
    };
    let x = rng.range(2, sx1 - 1);
    let y = rng.range(2, sy1 - 1);
    rotate(map.w, map.h, map.seats, t, x, y)
}

fn g_at(map: &Map, g: &[Ground], x: i32, y: i32) -> Ground {
    if map.inside(x, y) {
        g[map.idx(x, y)]
    } else {
        Ground::Rock
    }
}

/// A floor cell of sector `t` beside rock, and the way into the rock from
/// it (the sum of the offsets to rock cells within 2). Tries a few draws.
fn edge_cell(
    map: &Map,
    g: &[Ground],
    keep: &[bool],
    rng: &mut Rng,
    t: usize,
) -> Option<((i32, i32), (i32, i32))> {
    for _ in 0..300 {
        let (x, y) = sector_cell(map, rng, t);
        if g_at(map, g, x, y) != Ground::Floor || keep[map.idx(x, y)] {
            continue;
        }
        let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|(dx, dy)| g_at(map, g, x + dx, y + dy) == Ground::Rock);
        if !beside {
            continue;
        }
        let (mut sx, mut sy) = (0, 0);
        for dy in -2..=2 {
            for dx in -2..=2 {
                if g_at(map, g, x + dx, y + dy) == Ground::Rock {
                    sx += dx;
                    sy += dy;
                }
            }
        }
        if sx != 0 || sy != 0 {
            return Some(((x, y), (sx, sy)));
        }
    }
    None
}

/// Floor pushed into the rock beside a floor edge: a disk at least as wide
/// as a tunnel, so it is never a narrow slot. It is refused if it would
/// come near floor that is not the edge's own ground (another tunnel behind
/// the rock), so it never opens a shortcut.
fn bulge(map: &Map, g: &mut [Ground], keep: &[bool], rng: &mut Rng, t: usize, tw: i32) {
    let Some(((ex, ey), (sx, sy))) = edge_cell(map, g, keep, rng, t) else {
        return;
    };
    let rr = (tw - 2) / 2;
    let b = rng.range(rr + 1, rr + 3);
    let len = isqrt_i(sx * sx + sy * sy).max(1);
    let (cx, cy) = (ex + sx * (b - 1) / len, ey + sy * (b - 1) / len);
    let cells = disk_offsets(b);
    if cells.iter().any(|(dx, dy)| {
        let (x, y) = (cx + dx, cy + dy);
        x < 2 || y < 2 || x >= map.w - 2 || y >= map.h - 2 || keep[map.idx(x, y)]
    }) {
        return;
    }
    // Only into solid rock: at least two thirds of the ground round the
    // bulge is rock, so it never cuts the inside corner of a bend.
    let round = disk_offsets(b + 3);
    let rock = round
        .iter()
        .filter(|(dx, dy)| g_at(map, g, cx + dx, cy + dy) == Ground::Rock)
        .count();
    if rock * 3 < round.len() * 2 {
        return;
    }
    // Every floor cell within b + 3 of the bulge must be reached from the
    // edge cell through floor within b + 8: the bulge only widens the
    // ground it grows from.
    let far = b + 8;
    let n = 2 * far + 1;
    let mut seen = vec![false; (n * n) as usize];
    let at = |x: i32, y: i32| ((y - cy + far) * n + (x - cx + far)) as usize;
    let inside = |x: i32, y: i32| (x - cx).abs() <= far && (y - cy).abs() <= far;
    let mut stack = vec![(ex, ey)];
    if !inside(ex, ey) {
        return;
    }
    seen[at(ex, ey)] = true;
    while let Some((x, y)) = stack.pop() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if inside(nx, ny)
                && !seen[at(nx, ny)]
                && g_at(map, g, nx, ny) == Ground::Floor
                && (nx - cx).pow(2) + (ny - cy).pow(2) <= far * far
            {
                seen[at(nx, ny)] = true;
                stack.push((nx, ny));
            }
        }
    }
    for (dx, dy) in disk_offsets(b + 3) {
        let (x, y) = (cx + dx, cy + dy);
        if g_at(map, g, x, y) != Ground::Rock && !seen[at(x, y)] {
            return;
        }
    }
    for (dx, dy) in cells {
        let i = map.idx(cx + dx, cy + dy);
        if g[i] == Ground::Rock {
            g[i] = Ground::Floor;
        }
    }
}

/// The width rule while decorating: for every cell, whether the disk of
/// radius `(tw - 2) / 2` round it is fully open (floor or dirt). An open
/// cell that no such disk covers is in a passage narrower than the setting
/// (the rule of `open_ground`). Kept up to date as rock is added.
struct Width {
    offs: Vec<(i32, i32)>,
    full: Vec<bool>,
}

impl Width {
    fn new(map: &Map, g: &[Ground], tw: i32) -> Width {
        let r = (tw - 2) / 2;
        let open: Vec<bool> = g.iter().map(|c| *c != Ground::Rock).collect();
        Width {
            offs: disk_offsets(r),
            full: disk_all(&open, map.w, map.h, r),
        }
    }

    fn is_full(&self, map: &Map, x: i32, y: i32) -> bool {
        map.inside(x, y) && self.full[map.idx(x, y)]
    }

    /// Rock was added on `cells`: every disk holding one is no longer
    /// fully open. Returns the centres changed, for `reopen`.
    fn close(&mut self, map: &Map, cells: &[usize]) -> Vec<usize> {
        let mut out = Vec::new();
        for &c in cells {
            let (x, y) = (c as i32 % map.w, c as i32 / map.w);
            for (dx, dy) in &self.offs {
                let (qx, qy) = (x - dx, y - dy);
                if map.inside(qx, qy) {
                    let q = map.idx(qx, qy);
                    if self.full[q] {
                        self.full[q] = false;
                        out.push(q);
                    }
                }
            }
        }
        out
    }

    fn reopen(&mut self, centres: &[usize]) {
        for &q in centres {
            self.full[q] = true;
        }
    }

    /// The open cells within `rad` of (cx, cy) that no fully open disk
    /// covers.
    fn uncovered_near(
        &self,
        map: &Map,
        g: &[Ground],
        cx: i32,
        cy: i32,
        rad: i32,
    ) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for y in cy - rad..=cy + rad {
            for x in cx - rad..=cx + rad {
                if g_at(map, g, x, y) == Ground::Rock || self.is_full(map, x, y) {
                    continue;
                }
                let covered = self
                    .offs
                    .iter()
                    .any(|(dx, dy)| self.is_full(map, x + dx, y + dy));
                if !covered {
                    out.push((x, y));
                }
            }
        }
        out
    }
}

/// A rock bump on a floor edge, radius 1 to 3, smoothed into the rock it
/// stands on: the floor left in the corners where it meets the rock, too
/// tight for the width rule, becomes rock as well. Undone if the width rule
/// would take floor any further away, which means the bump narrowed a
/// passage below the setting.
fn bump(
    map: &Map,
    g: &mut [Ground],
    width: &mut Width,
    keep: &[bool],
    rng: &mut Rng,
    t: usize,
    tw: i32,
) {
    let Some(((ex, ey), _)) = edge_cell(map, g, keep, rng, t) else {
        return;
    };
    let b = rng.range(1, 3);
    let cells: Vec<usize> = disk_offsets(b)
        .into_iter()
        .map(|(dx, dy)| (ex + dx, ey + dy))
        .filter(|&(x, y)| map.inside(x, y))
        .map(|(x, y)| map.idx(x, y))
        .collect();
    if cells.iter().any(|&i| keep[i] || g[i] == Ground::Dirt) {
        return;
    }
    let changed: Vec<usize> = cells
        .into_iter()
        .filter(|&i| g[i] == Ground::Floor)
        .collect();
    for &i in &changed {
        g[i] = Ground::Rock;
    }
    let closed = width.close(map, &changed);
    let tight = width.uncovered_near(map, g, ex, ey, b + tw - 1);
    let reach = (b + 3) * (b + 3);
    let fillet = tight.iter().all(|&(x, y)| {
        (x - ex).pow(2) + (y - ey).pow(2) <= reach
            && !keep[map.idx(x, y)]
            && g[map.idx(x, y)] == Ground::Floor
    });
    if fillet {
        // No fully open disk holds these cells, so closing them changes
        // no disk.
        for (x, y) in tight {
            let i = map.idx(x, y);
            g[i] = Ground::Rock;
        }
    } else {
        for &i in &changed {
            g[i] = Ground::Floor;
        }
        width.reopen(&closed);
    }
}

/// A rock speck, radius 1 or 2, standing in open floor at least a Tunnel
/// Width from anything else.
fn speck(
    map: &Map,
    g: &mut [Ground],
    width: &mut Width,
    keep: &[bool],
    rng: &mut Rng,
    t: usize,
    tw: i32,
) {
    for _ in 0..40 {
        let (x, y) = sector_cell(map, rng, t);
        let s = rng.range(1, 2);
        let clear = disk_offsets(s + tw).iter().all(|(dx, dy)| {
            let (px, py) = (x + dx, y + dy);
            g_at(map, g, px, py) == Ground::Floor && !keep[map.idx(px, py)]
        });
        if clear {
            let cells: Vec<usize> = disk_offsets(s)
                .into_iter()
                .map(|(dx, dy)| map.idx(x + dx, y + dy))
                .collect();
            for &i in &cells {
                g[i] = Ground::Rock;
            }
            width.close(map, &cells);
            return;
        }
    }
}

/// Fills floor that cannot be reached from home chamber 0, even by digging.
fn seal_unreachable(map: &mut Map) {
    let home = map.home(0).clone();
    let reach = flood(map, home.cx, home.cy, true);
    for i in 0..map.ground.len() {
        if !reach[i] {
            map.ground[i] = Ground::Rock;
        }
    }
    let ground = map.ground.clone();
    let w = map.w;
    map.nodes.retain(|n| {
        let (x, y) = n.pos.cell();
        ground[(y * w + x) as usize] != Ground::Rock
    });
}

/// Cells reachable from (x, y) through floor, and through dirt if `dig`.
pub fn flood(map: &Map, x: i32, y: i32, dig: bool) -> Vec<bool> {
    let mut seen = vec![false; map.ground.len()];
    let mut stack = vec![(x, y)];
    while let Some((cx, cy)) = stack.pop() {
        if !map.inside(cx, cy) {
            continue;
        }
        let i = map.idx(cx, cy);
        if seen[i] {
            continue;
        }
        let g = map.ground[i];
        if g == Ground::Rock || (g == Ground::Dirt && !dig) {
            continue;
        }
        seen[i] = true;
        stack.push((cx + 1, cy));
        stack.push((cx - 1, cy));
        stack.push((cx, cy + 1));
        stack.push((cx, cy - 1));
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Format;

    const TYPES: [MapType; 3] = [MapType::Tunnels, MapType::Open, MapType::Maze];

    fn settings(format: Format) -> MatchSettings {
        MatchSettings {
            format,
            map_type: MapType::Tunnels,
            ..MatchSettings::default()
        }
    }

    fn typed(format: Format, map_type: MapType) -> MatchSettings {
        MatchSettings {
            format,
            map_type,
            ..MatchSettings::default()
        }
    }

    /// The default settings and a few corners: small and large maps, small
    /// and large chambers, narrow and wide tunnels.
    fn variants(format: Format, map_type: MapType) -> Vec<MatchSettings> {
        let mut v = vec![typed(format, map_type)];
        v.push(MatchSettings {
            map_size: Level::Low,
            chamber_size: Level::High,
            tunnel_width: crate::settings::TUNNEL_MAX,
            ..typed(format, map_type)
        });
        v.push(MatchSettings {
            map_size: Level::High,
            chamber_size: Level::Low,
            tunnel_width: crate::settings::TUNNEL_MIN,
            resource_density: Level::High,
            ..typed(format, map_type)
        });
        v
    }

    fn map_hash(m: &Map) -> u64 {
        use crate::dice::{fnv, fnv_i64, FNV_START};
        let mut h = FNV_START;
        for g in &m.ground {
            h = fnv(h, &[*g as u8]);
        }
        for n in &m.nodes {
            h = fnv_i64(h, n.pos.x as i64);
            h = fnv_i64(h, n.pos.y as i64);
            h = fnv_i64(h, n.amount as i64);
        }
        for c in &m.chambers {
            h = fnv_i64(h, (c.cx * 1000 + c.cy) as i64);
        }
        h
    }

    #[test]
    fn tunnels_maps_are_frozen() {
        // Tunnels maps for these seeds, frozen so a change to them is seen.
        // Captured again for TRIAGE rows 47 and 50-52: every chamber has
        // every resource, new node sizes, and each sector decorated on its
        // own (the hashes taken for TRIAGE row 33 no longer hold).
        let want: [u64; 8] = [
            2178450767473459997,
            11323575075085791838,
            13029420177463473568,
            9794210519413522520,
            15101770702980332516,
            16372915244465898842,
            17975477603746795572,
            15496232582336606526,
        ];
        let mut got = Vec::new();
        for f in [Format::OneVOne, Format::FreeForAll] {
            for seed in [0u64, 1, 7, 42] {
                got.push(map_hash(&generate(seed, &settings(f))));
            }
        }
        assert_eq!(got, want);
    }

    #[test]
    fn the_same_seed_gives_the_same_map() {
        for t in TYPES {
            for f in [Format::OneVOne, Format::FreeForAll] {
                let s = typed(f, t);
                assert_eq!(generate(9, &s), generate(9, &s), "{t:?} {f:?}");
            }
        }
    }

    #[test]
    fn different_seeds_give_different_maps() {
        for t in TYPES {
            let s = typed(Format::OneVOne, t);
            assert_ne!(generate(1, &s).ground, generate(2, &s).ground, "{t:?}");
        }
    }

    #[test]
    fn map_types_differ() {
        let a = generate(3, &typed(Format::OneVOne, MapType::Tunnels));
        let b = generate(3, &typed(Format::OneVOne, MapType::Open));
        let c = generate(3, &typed(Format::OneVOne, MapType::Maze));
        assert_ne!(a.ground, b.ground);
        assert_ne!(b.ground, c.ground);
        // Open and Maze are mostly open ground; Tunnels mostly rock.
        let floor = |m: &Map| {
            m.ground.iter().filter(|g| **g == Ground::Floor).count() * 100 / m.ground.len()
        };
        assert!(floor(&a) < 50, "Tunnels floor {}%", floor(&a));
        assert!(floor(&b) > 55, "Open floor {}%", floor(&b));
        assert!(floor(&c) > 55, "Maze floor {}%", floor(&c));
    }

    #[test]
    fn open_and_maze_have_dirt_and_caches() {
        for t in [MapType::Open, MapType::Maze] {
            for seed in 0..6 {
                let m = generate(seed, &typed(Format::OneVOne, t));
                let dirt = m.ground.iter().filter(|g| **g == Ground::Dirt).count();
                assert!(dirt > 0, "{t:?} seed {seed}: no dirt");
                let reach = flood(&m, m.home(0).cx, m.home(0).cy, false);
                for n in m.nodes.iter().filter(|n| n.cache) {
                    let (x, y) = n.pos.cell();
                    assert!(
                        !reach[m.idx(x, y)],
                        "{t:?} seed {seed}: cache at {x},{y} is not behind dirt"
                    );
                }
            }
        }
    }

    /// The cell a node or chamber centre of seat 0's sector lands on in
    /// sector `t`.
    fn turned(m: &Map, t: usize, p: Pos) -> Pos {
        rotate_pos(m.w, m.h, m.seats, t, p)
    }

    #[test]
    fn the_layout_is_rotated_but_the_ground_is_not() {
        // TRIAGE row 50: chambers, homes and nodes are still exact
        // rotations (fair by construction), but each sector is decorated on
        // its own, so the ground is no longer a rotated copy.
        for t in TYPES {
            for (f, seed) in [
                (Format::OneVOne, 3),
                (Format::FreeForAll, 4),
                (Format::OneVOne, 11),
            ] {
                for (k, s) in variants(f, t).into_iter().enumerate() {
                    let m = generate(seed, &s);
                    for n in &m.nodes {
                        let q = turned(&m, 1, n.pos);
                        assert!(
                            m.nodes.iter().any(|o| o.pos == q
                                && o.resource == n.resource
                                && o.amount == n.amount),
                            "{t:?} {f:?}: node at {:?} has no rotated twin",
                            n.pos.cell()
                        );
                    }
                    // The centre chamber is its own rotation (to within a
                    // cell); every other chamber has a rotated twin.
                    for c in m.chambers.iter().filter(|c| c.cx != m.w / 2) {
                        let (rx, ry) = rotate(m.w, m.h, m.seats, 1, c.cx, c.cy);
                        assert!(
                            m.chambers
                                .iter()
                                .any(|o| (o.cx, o.cy, o.r) == (rx, ry, c.r)),
                            "{t:?} {f:?}: chamber at {},{}",
                            c.cx,
                            c.cy
                        );
                    }
                    let differ = (0..m.h)
                        .flat_map(|y| (0..m.w).map(move |x| (x, y)))
                        .filter(|&(x, y)| {
                            let (rx, ry) = rotate(m.w, m.h, m.seats, 1, x, y);
                            m.at(x, y) != m.at(rx, ry)
                        })
                        .count();
                    // The default settings: at least 0.3% of the map. The
                    // corner settings (tiny maps of huge chambers and wide
                    // tunnels) leave little room, but still some.
                    let least = if k == 0 {
                        m.ground.len() * 3 / 1000
                    } else {
                        20
                    };
                    assert!(
                        differ >= least,
                        "{t:?} {f:?} seed {seed} {}x{} tw {}: only {differ} cells differ from the rotated map", m.w, m.h, s.tunnel_width
                    );
                }
            }
        }
    }

    #[test]
    fn walking_distances_count_ten_a_step_and_fourteen_a_diagonal() {
        // Worked by hand: in an open 5 x 5 room from (1, 1), (4, 1) is three
        // straight steps (30) and (4, 4) three diagonal ones (42).
        let mut m = generate(0, &settings(Format::OneVOne));
        for g in m.ground.iter_mut() {
            *g = Ground::Rock;
        }
        for y in 1..=5 {
            for x in 1..=5 {
                let i = m.idx(x, y);
                m.ground[i] = Ground::Floor;
            }
        }
        // A rock post at (3, 3): the diagonal must go round it.
        let i = m.idx(3, 3);
        m.ground[i] = Ground::Rock;
        let d = walk_distances(&m, 1, 1);
        assert_eq!(d[m.idx(4, 1)], 30);
        assert_eq!(d[m.idx(2, 2)], 14);
        // To (4, 4) the post is in the way and no step may cut past its
        // corners: one diagonal to (2, 2), then four straight steps round
        // it, 14 + 4 x 10 = 54.
        assert_eq!(d[m.idx(4, 4)], 54);
        assert_eq!(d[m.idx(3, 3)], u32::MAX);
        assert_eq!(d[m.idx(0, 0)], u32::MAX);
    }

    /// Nodes by seat: those in the seat's home chamber, and those nearer
    /// its home than any other (its sector), the centre's left out.
    fn seat_resources(m: &Map) -> (Vec<[i32; 3]>, Vec<[i32; 3]>) {
        let mut home = vec![[0; 3]; m.seats];
        let mut sector = vec![[0; 3]; m.seats];
        let c = &m.chambers[m.centre];
        for n in &m.nodes {
            let (x, y) = n.pos.cell();
            if (x - c.cx).pow(2) + (y - c.cy).pow(2) <= (c.r + 4).pow(2) {
                continue;
            }
            let d = |s: usize| {
                let h = m.home(s);
                (x - h.cx).pow(2) + (y - h.cy).pow(2)
            };
            let s = (0..m.seats).min_by_key(|&s| (d(s), s)).unwrap();
            let h = m.home(s);
            sector[s][n.resource.index()] += n.amount;
            if d(s) <= h.r * h.r {
                home[s][n.resource.index()] += n.amount;
            }
        }
        (home, sector)
    }

    #[test]
    fn every_seat_is_equally_well_placed() {
        // TRIAGE row 50. Per seat, walking without digging: home to the
        // centre, to every other home, and to its three nearest chambers
        // are within 5% of the other seats'; home nodes are identical and
        // each sector's nodes within 5%.
        for t in TYPES {
            for seed in 0..8 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    for s in variants(f, t) {
                        let m = generate(seed, &s);
                        let d = seat_distances(&m);
                        assert!(
                            distances_fair(&d, 5),
                            "{t:?} seed {seed} {f:?}: distances {d:?}"
                        );
                        let (home, sector) = seat_resources(&m);
                        assert!(
                            home.iter().all(|h| *h == home[0]),
                            "{t:?} seed {seed} {f:?}: home nodes {home:?}"
                        );
                        for r in 0..3 {
                            let lo = sector.iter().map(|v| v[r]).min().unwrap();
                            let hi = sector.iter().map(|v| v[r]).max().unwrap();
                            assert!(
                                hi * 100 <= lo * 105,
                                "{t:?} seed {seed} {f:?}: sector nodes {sector:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_seat_has_enough_for_a_full_army_and_more() {
        // TRIAGE row 47. A 300-Pop Cost army of Tier 2 ants, a third each
        // by Pop Cost of Soldier, Acid and Turtle Ants, costs (from the
        // species tables) 100 x 55 + 50 x 50 + 50 x 70 = 11,500 Food and
        // 50 x 40 + 50 x 30 = 3,500 Nectar. A 1v1 seat's share of the
        // map's nodes pays for it one and a half times over, leaving room
        // for workers, structures, upgrades and losses.
        use crate::rules::Species;
        let mut army = [0i32; 3];
        for (s, n) in [
            (Species::Soldier, 100),
            (Species::Acid, 50),
            (Species::Turtle, 50),
        ] {
            for (slot, c) in army.iter_mut().zip(s.stats().cost) {
                *slot += c * n;
            }
        }
        assert_eq!(army, [11_500, 3_500, 0]);
        for t in TYPES {
            for seed in 0..6 {
                let m = generate(seed, &typed(Format::OneVOne, t));
                let mut share = [0i32; 3];
                for n in &m.nodes {
                    share[n.resource.index()] += n.amount / m.seats as i32;
                }
                for r in 0..2 {
                    assert!(
                        share[r] * 2 >= army[r] * 3,
                        "{t:?} seed {seed}: a seat has {share:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_chamber_has_every_resource() {
        // TRIAGE row 51: Food, Nectar and Leaves in every chamber.
        for t in TYPES {
            for seed in 0..6 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    let m = generate(seed, &typed(f, t));
                    for (i, c) in m.chambers.iter().enumerate() {
                        for res in Resource::ALL {
                            let has = m.nodes.iter().any(|n| {
                                let (x, y) = n.pos.cell();
                                n.resource == res
                                    && !n.cache
                                    && m.is_floor(x, y)
                                    && (x - c.cx).pow(2) + (y - c.cy).pow(2) <= c.r * c.r
                            });
                            assert!(has, "{t:?} seed {seed} {f:?} chamber {i} lacks {res:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_home_chamber_has_all_three_nodes() {
        for t in TYPES {
            for seed in 0..20 {
                for f in [Format::OneVOne, Format::TwoVTwo, Format::FreeForAll] {
                    let m = generate(seed, &typed(f, t));
                    for seat in 0..m.seats {
                        let home = m.home(seat);
                        for res in Resource::ALL {
                            let near = m.nodes.iter().any(|n| {
                                let (x, y) = n.pos.cell();
                                n.resource == res
                                    && !n.cache
                                    && m.is_floor(x, y)
                                    && (x - home.cx).pow(2) + (y - home.cy).pow(2)
                                        <= home.r * home.r
                            });
                            assert!(near, "{t:?} seed {seed} {f:?} seat {seat} lacks {res:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_home_and_the_centre_are_reachable_without_digging() {
        for t in TYPES {
            for seed in 0..20 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    let s = if seed % 4 == 3 {
                        variants(f, t)[(seed as usize / 4) % 3].clone()
                    } else {
                        typed(f, t)
                    };
                    let m = generate(seed, &s);
                    let h0 = m.home(0).clone();
                    let reach = flood(&m, h0.cx, h0.cy, false);
                    for seat in 0..m.seats {
                        let h = m.home(seat);
                        assert!(
                            reach[m.idx(h.cx, h.cy)],
                            "{t:?} seed {seed} {f:?} seat {seat}"
                        );
                    }
                    let c = &m.chambers[m.centre];
                    assert!(reach[m.idx(c.cx, c.cy)], "{t:?} seed {seed} {f:?} centre");
                    // Every chamber can be walked to (expansions, scouting).
                    for (i, c) in m.chambers.iter().enumerate() {
                        assert!(
                            reach[m.idx(c.cx, c.cy)],
                            "{t:?} seed {seed} {f:?} chamber {i}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn open_and_maze_need_no_rescue_passage() {
        // The generators' clearances alone keep the open ground connected;
        // `connect_homes` is a safety net that should not be needed.
        for t in [MapType::Open, MapType::Maze] {
            for seed in 0..20 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    for s in variants(f, t) {
                        let plain = generate_open(seed, &s, false);
                        let m = generate_open(seed, &s, true);
                        assert!(
                            m.ground == plain.ground,
                            "{t:?} seed {seed} {f:?} needed a rescue passage"
                        );
                    }
                }
            }
        }
    }

    /// The reachability, fairness, home-node and no-rescue checks over many
    /// seeds: `cargo test --release -p tool_core --lib map::tests::many_seeds
    /// -- --ignored`.
    #[test]
    #[ignore]
    fn many_seeds() {
        for t in TYPES {
            for seed in 100..400 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    for s in variants(f, t) {
                        let m = generate(seed, &s);
                        if t != MapType::Tunnels {
                            let plain = generate_open(seed, &s, false);
                            let rescued = generate_open(seed, &s, true);
                            assert!(
                                rescued.ground == plain.ground,
                                "{t:?} seed {seed} {f:?} rescue"
                            );
                        }
                        let d = seat_distances(&m);
                        assert!(distances_fair(&d, 5), "{t:?} seed {seed} {f:?} {d:?}");
                        let h0 = m.home(0).clone();
                        let reach = flood(&m, h0.cx, h0.cy, false);
                        for (i, c) in m.chambers.iter().enumerate() {
                            assert!(
                                reach[m.idx(c.cx, c.cy)],
                                "{t:?} seed {seed} {f:?} chamber {i}"
                            );
                        }
                        for seat in 0..m.seats {
                            let home = m.home(seat);
                            for res in Resource::ALL {
                                assert!(
                                    m.nodes.iter().any(|n| {
                                        let (x, y) = n.pos.cell();
                                        n.resource == res
                                            && !n.cache
                                            && m.is_floor(x, y)
                                            && (x - home.cx).pow(2) + (y - home.cy).pow(2)
                                                <= home.r * home.r
                                    }),
                                    "{t:?} seed {seed} {f:?} seat {seat} lacks {res:?}"
                                );
                            }
                        }
                        // Decoration never opens a cache. (A Tunnels cache
                        // can be open from the start, when its alcove's
                        // connector runs through a tunnel.)
                        let base = if t == MapType::Tunnels {
                            generate_tunnels(seed, &s)
                        } else {
                            generate_open(seed, &s, true)
                        };
                        let b0 = base.home(0).clone();
                        let base_reach = flood(&base, b0.cx, b0.cy, false);
                        for n in m.nodes.iter().filter(|n| n.cache) {
                            let (x, y) = n.pos.cell();
                            let open = reach[m.idx(x, y)];
                            assert!(
                                !open || (t == MapType::Tunnels && base_reach[m.idx(x, y)]),
                                "{t:?} seed {seed} {f:?} open cache"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn home_chambers_are_equally_far_from_the_centre() {
        for t in TYPES {
            let m = generate(5, &typed(Format::FreeForAll, t));
            let c = &m.chambers[m.centre];
            let d: Vec<i32> = (0..4)
                .map(|s| {
                    let h = m.home(s);
                    (h.cx - c.cx).pow(2) + (h.cy - c.cy).pow(2)
                })
                .collect();
            let spread = d.iter().max().unwrap() - d.iter().min().unwrap();
            assert!(spread <= 2 * (c.cx + c.cy), "{t:?} {d:?}");
        }
    }

    #[test]
    fn chambers_are_laid_out_by_rotation_for_windfalls() {
        // world.rs picks a Windfall chamber in sector 0 and its rotations at
        // `pick + t * per`; the centre is last.
        for t in TYPES {
            for f in [Format::OneVOne, Format::FreeForAll] {
                let m = generate(8, &typed(f, t));
                assert_eq!(m.centre, m.chambers.len() - 1);
                let per = (m.chambers.len() - 1) / m.seats;
                for i in 0..per {
                    for k in 1..m.seats {
                        let a = &m.chambers[i];
                        let b = &m.chambers[i + k * per];
                        assert_eq!(
                            rotate(m.w, m.h, m.seats, k, a.cx, a.cy),
                            (b.cx, b.cy),
                            "{t:?} {f:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn narrowest_tunnel_is_at_least_the_setting() {
        // Every open cell lies inside a fully open disk of radius
        // (width - 2) / 2: a passage that narrow at any angle would fail.
        for t in TYPES {
            for seed in 0..6 {
                for f in [Format::OneVOne, Format::FreeForAll] {
                    for s in variants(f, t) {
                        let m = generate(seed, &s);
                        let r = (s.tunnel_width - 2) / 2;
                        let offs = disk_offsets(r);
                        let open = |x: i32, y: i32| m.at(x, y) != Ground::Rock;
                        let mut covered = vec![false; m.ground.len()];
                        for y in 0..m.h {
                            for x in 0..m.w {
                                if offs.iter().all(|(dx, dy)| open(x + dx, y + dy)) {
                                    for (dx, dy) in &offs {
                                        covered[m.idx(x + dx, y + dy)] = true;
                                    }
                                }
                            }
                        }
                        for y in 0..m.h {
                            for x in 0..m.w {
                                assert!(
                                    !open(x, y) || covered[m.idx(x, y)],
                                    "{t:?} seed {seed} {f:?}: cell {x},{y} is in a passage narrower than {}",
                                    s.tunnel_width
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// ASCII previews: `cargo test -p tool_core --lib map::tests::preview --
    /// --ignored`, with MAP_PREVIEW naming the file to write (optional).
    #[test]
    #[ignore]
    fn preview() {
        let mut out = String::new();
        for t in TYPES {
            for (f, seed) in [
                (Format::OneVOne, 13),
                (Format::OneVOne, 1),
                (Format::FreeForAll, 2),
            ] {
                let m = generate(seed, &typed(f, t));
                out.push_str(&format!("{t:?} {f:?} seed {seed} {}x{}\n", m.w, m.h));
                for y in (0..m.h).step_by(2) {
                    for x in (0..m.w).step_by(2) {
                        let node = m.nodes.iter().find(|n| {
                            let (nx, ny) = n.pos.cell();
                            nx / 2 == x / 2 && ny / 2 == y / 2
                        });
                        let ch = match node {
                            Some(n) if n.cache => '$',
                            Some(n) => match n.resource {
                                Resource::Food => 'F',
                                Resource::Nectar => 'N',
                                Resource::Leaves => 'L',
                            },
                            None => match m.at(x, y) {
                                Ground::Rock => '#',
                                Ground::Dirt => '%',
                                Ground::Floor => {
                                    if m.chambers
                                        .iter()
                                        .any(|c| c.cx / 2 == x / 2 && c.cy / 2 == y / 2)
                                    {
                                        '+'
                                    } else {
                                        ' '
                                    }
                                }
                            },
                        };
                        out.push(ch);
                    }
                    out.push('\n');
                }
            }
        }
        panic!("\n{out}");
    }
}
