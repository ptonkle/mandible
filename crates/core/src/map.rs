//! The map: a grid of rock, floor and dirt, generated from a seed.
//!
//! Fairness comes from symmetry. The generator lays out one sector of the map
//! (chambers, tunnels, dirt and nodes), then copies it by exact grid rotation:
//! 180° for two seats, 90° for four. Every seat therefore gets the same map.

use serde::{Deserialize, Serialize};

use crate::dice::Rng;
use crate::fixed::{cos64, sin64, Pos, CELL, TRIG_ONE};
use crate::rules::Resource;
use crate::settings::{Level, MatchSettings};

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
pub fn generate(seed: u64, settings: &MatchSettings) -> Map {
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
    let (hx, hy) = if seats == 2 {
        (w * 17 / 100, h / 2 + rng.range(-(h / 6), h / 6))
    } else {
        (
            w * 18 / 100 + rng.range(-3, 3),
            h * 18 / 100 + rng.range(-3, 3),
        )
    };
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
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
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
    let home = &reps[0];
    let a0 = rng.range(0, 63) as usize;
    let home_amounts = [2200, 1300, 1600];
    for (k, res) in Resource::ALL.into_iter().enumerate() {
        let a = (a0 + k * 21) % 64;
        let d = home.r * 6 / 10;
        rep_nodes.push(NodeSpot {
            pos: Pos::new(
                home.cx * CELL + CELL / 2 + d * CELL * cos64(a) / TRIG_ONE,
                home.cy * CELL + CELL / 2 + d * CELL * sin64(a) / TRIG_ONE,
            ),
            resource: res,
            amount: home_amounts[k] * density / 100,
            cache: false,
        });
    }
    for c in reps.iter().skip(1) {
        let count = if rng.chance(density as u32 / 2) { 2 } else { 1 };
        for _ in 0..count {
            let roll = rng.below(100);
            let res = if roll < 45 {
                Resource::Food
            } else if roll < 75 {
                Resource::Nectar
            } else {
                Resource::Leaves
            };
            let a = rng.range(0, 63) as usize;
            let d = rng.range(0, c.r / 2);
            rep_nodes.push(NodeSpot {
                pos: Pos::new(
                    c.cx * CELL + CELL / 2 + d * CELL * cos64(a) / TRIG_ONE,
                    c.cy * CELL + CELL / 2 + d * CELL * sin64(a) / TRIG_ONE,
                ),
                resource: res,
                amount: rng.range(1400, 2000) * density / 100,
                cache: false,
            });
        }
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
            amount: 700 * density / 100,
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
    let centre = Pos::new(w * CELL / 2, h * CELL / 2);
    for (k, res) in Resource::ALL.into_iter().enumerate() {
        if seats == 2 {
            // Two of each, mirrored, so both seats are equally far.
            let a = k * 21 + 5;
            let d = (base_r / 2) * CELL;
            let p = Pos::new(
                centre.x + d * cos64(a) / TRIG_ONE,
                centre.y + d * sin64(a) / TRIG_ONE,
            );
            for t in 0..2 {
                nodes.push(NodeSpot {
                    pos: rotate_pos(w, h, seats, t, p),
                    resource: res,
                    amount: 1500 * density / 100,
                    cache: false,
                });
            }
        } else {
            let a = k * 16 + 8;
            let d = (base_r / 2) * CELL;
            let p = Pos::new(
                centre.x + d * cos64(a) / TRIG_ONE,
                centre.y + d * sin64(a) / TRIG_ONE,
            );
            for t in 0..4 {
                nodes.push(NodeSpot {
                    pos: rotate_pos(w, h, seats, t, p),
                    resource: res,
                    amount: 900 * density / 100,
                    cache: false,
                });
            }
        }
    }

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

/// Morphological opening with a disk: keeps only open cells (floor or dirt)
/// that lie in a fully open disk of radius `(width - 2) / 2`, so no passage,
/// at any angle, is narrower than the Tunnel Width setting allows.
fn open_to_width(map: &mut Map, width: i32) {
    let r = (width - 2) / 2;
    let offsets = disk_offsets(r);
    let (w, h) = (map.w, map.h);
    let open = |x: i32, y: i32, g: &[Ground]| -> bool {
        x >= 0 && y >= 0 && x < w && y < h && g[(y * w + x) as usize] != Ground::Rock
    };
    let mut keep = vec![false; map.ground.len()];
    for y in 0..h {
        for x in 0..w {
            if !open(x, y, &map.ground) {
                continue;
            }
            if offsets
                .iter()
                .all(|(dx, dy)| open(x + dx, y + dy, &map.ground))
            {
                for (dx, dy) in &offsets {
                    keep[((y + dy) * w + x + dx) as usize] = true;
                }
            }
        }
    }
    for (i, g) in map.ground.iter_mut().enumerate() {
        if !keep[i] {
            *g = Ground::Rock;
        }
    }
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

    fn settings(format: Format) -> MatchSettings {
        MatchSettings {
            format,
            ..MatchSettings::default()
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_map() {
        let s = settings(Format::OneVOne);
        assert_eq!(generate(9, &s), generate(9, &s));
    }

    #[test]
    fn different_seeds_give_different_maps() {
        let s = settings(Format::OneVOne);
        assert_ne!(generate(1, &s).ground, generate(2, &s).ground);
    }

    #[test]
    fn maps_are_rotationally_symmetric() {
        for (f, seed) in [(Format::OneVOne, 3), (Format::FreeForAll, 4)] {
            let m = generate(seed, &settings(f));
            for y in 0..m.h {
                for x in 0..m.w {
                    let (rx, ry) = rotate(m.w, m.h, m.seats, 1, x, y);
                    assert_eq!(m.at(x, y), m.at(rx, ry), "{f:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn every_home_chamber_has_all_three_nodes() {
        for seed in 0..20 {
            for f in [Format::OneVOne, Format::TwoVTwo, Format::FreeForAll] {
                let m = generate(seed, &settings(f));
                for seat in 0..m.seats {
                    let home = m.home(seat);
                    for res in Resource::ALL {
                        let near = m.nodes.iter().any(|n| {
                            let (x, y) = n.pos.cell();
                            n.resource == res
                                && !n.cache
                                && (x - home.cx).pow(2) + (y - home.cy).pow(2) <= home.r * home.r
                        });
                        assert!(near, "seed {seed} {f:?} seat {seat} lacks {res:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_home_and_the_centre_are_reachable_without_digging() {
        for seed in 0..20 {
            for f in [Format::OneVOne, Format::FreeForAll] {
                let m = generate(seed, &settings(f));
                let h0 = m.home(0).clone();
                let reach = flood(&m, h0.cx, h0.cy, false);
                for seat in 0..m.seats {
                    let h = m.home(seat);
                    assert!(reach[m.idx(h.cx, h.cy)], "seed {seed} {f:?} seat {seat}");
                }
                let c = &m.chambers[m.centre];
                assert!(reach[m.idx(c.cx, c.cy)], "seed {seed} {f:?} centre");
            }
        }
    }

    #[test]
    fn home_chambers_are_equally_far_from_the_centre() {
        let m = generate(5, &settings(Format::FreeForAll));
        let c = &m.chambers[m.centre];
        let d: Vec<i32> = (0..4)
            .map(|s| {
                let h = m.home(s);
                (h.cx - c.cx).pow(2) + (h.cy - c.cy).pow(2)
            })
            .collect();
        let spread = d.iter().max().unwrap() - d.iter().min().unwrap();
        assert!(spread <= 2 * (c.cx + c.cy), "{d:?}");
    }

    #[test]
    fn narrowest_tunnel_is_at_least_the_setting() {
        // Every open cell lies inside a fully open disk of radius
        // (width - 2) / 2: a passage that narrow at any angle would fail.
        for seed in 0..6 {
            for f in [Format::OneVOne, Format::FreeForAll] {
                let s = settings(f);
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
                            "seed {seed} {f:?}: cell {x},{y} is in a passage narrower than {}",
                            s.tunnel_width
                        );
                    }
                }
            }
        }
    }
}
