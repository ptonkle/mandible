//! Navigation by flow fields. One distance field per target and mode, shared
//! by every ant heading to the same place, built with Dijkstra over the cell
//! grid. The cache is pure: a field depends only on the terrain, so the world
//! never needs to save it and every peer gets the same answers.

use std::collections::BTreeMap;

use crate::map::Ground;

/// How an ant moves through the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// Gatherers and Builders: floor and own walls only.
    Walk,
    /// Combat ants: also through dirt and enemy walls, at a cost, because
    /// they dig and break them.
    Dig,
    /// Flying Ants: over any wall; rock and dirt still block.
    Fly,
}

pub const UNREACHED: u16 = u16::MAX;
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
const DIG_DIRT: u32 = 60;
const DIG_WALL: u32 = 120;
/// The dearest single step: a diagonal into an enemy wall.
const MAX_STEP: u32 = DIAGONAL + DIG_WALL;
/// Buckets in the search's ring: more than any one step.
const RING: usize = MAX_STEP as usize + 1;
/// A cell the search may not enter.
const CLOSED: u8 = u8::MAX;

/// The terrain a field is built over. `wall` holds the owning colony of a
/// complete wall on each cell, or `NO_WALL`. `teams` gives each colony's
/// team: a wall lets its whole team through (a gate), so allies never dig
/// or break each other's walls. Empty `teams` means every colony is its own
/// team.
pub struct Terrain<'a> {
    pub w: i32,
    pub h: i32,
    pub ground: &'a [Ground],
    pub wall: &'a [u8],
    pub teams: &'a [u8],
}

pub const NO_WALL: u8 = u8::MAX;

impl Terrain<'_> {
    /// Whether a wall owned by `owner` lets `colony`'s ants through.
    pub fn friendly(&self, owner: u8, colony: u8) -> bool {
        owner == colony
            || matches!(
                (self.teams.get(owner as usize), self.teams.get(colony as usize)),
                (Some(a), Some(b)) if a == b
            )
    }

    /// The first colony on `colony`'s team: every colony of a team gets the
    /// same fields (walls are gates for the whole team), so allies share
    /// them.
    pub fn lead(&self, colony: u8) -> u8 {
        let Some(team) = self.teams.get(colony as usize) else {
            return colony;
        };
        self.teams
            .iter()
            .position(|t| t == team)
            .map_or(colony, |i| i as u8)
    }

    /// Cost to step onto a cell, or `None` if the cell is closed.
    fn enter(&self, i: usize, colony: u8, mode: Mode) -> Option<u32> {
        let g = self.ground[i];
        let wall = self.wall[i];
        match g {
            Ground::Rock => None,
            Ground::Dirt => (mode == Mode::Dig).then_some(DIG_DIRT),
            Ground::Floor => {
                if wall == NO_WALL || self.friendly(wall, colony) || mode == Mode::Fly {
                    Some(0)
                } else if mode == Mode::Dig {
                    Some(DIG_WALL)
                } else {
                    None
                }
            }
        }
    }

    /// Whether an ant of this colony and mode may stand on a cell.
    pub fn standable(&self, x: i32, y: i32, colony: u8, mode: Mode) -> bool {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return false;
        }
        let i = (y * self.w + x) as usize;
        match self.ground[i] {
            Ground::Floor => {
                let wall = self.wall[i];
                wall == NO_WALL || self.friendly(wall, colony) || mode == Mode::Fly
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldKey {
    pub colony: u8,
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    /// Every standable cell within this radius of (x, y) is a goal.
    pub r: i32,
}

pub struct Field {
    pub dist: Vec<u16>,
    version: u32,
    used: u64,
    /// Every goal cell is under a wall (no open goal cell existed).
    walled_goals: bool,
}

impl Field {
    pub fn at(&self, w: i32, x: i32, y: i32) -> u16 {
        if x < 0 || y < 0 || x >= w {
            return UNREACHED;
        }
        self.dist
            .get((y * w + x) as usize)
            .copied()
            .unwrap_or(UNREACHED)
    }
}

/// The field cache. `version` changes whenever terrain changes.
#[derive(Default)]
pub struct Nav {
    fields: BTreeMap<FieldKey, Field>,
    pub version: u32,
    clock: u64,
}

const CACHE_LIMIT: usize = 192;

impl Nav {
    pub fn invalidate(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The field for a key, built if missing or stale.
    pub fn field(&mut self, t: &Terrain, key: FieldKey) -> &Field {
        let key = FieldKey {
            colony: t.lead(key.colony),
            ..key
        };
        self.clock += 1;
        let fresh = matches!(self.fields.get(&key), Some(f) if f.version == self.version);
        if !fresh {
            if self.fields.len() >= CACHE_LIMIT {
                if let Some(oldest) = self
                    .fields
                    .iter()
                    .min_by_key(|(k, f)| (f.used, **k))
                    .map(|(k, _)| *k)
                {
                    self.fields.remove(&oldest);
                }
            }
            let (dist, walled_goals) = build(t, key);
            self.fields.insert(
                key,
                Field {
                    dist,
                    version: self.version,
                    used: self.clock,
                    walled_goals,
                },
            );
        }
        let f = self.fields.get_mut(&key).expect("just inserted");
        f.used = self.clock;
        f
    }

    /// Updates every cached field after cells became easier to cross and
    /// nothing became harder (dirt dug to floor, a wall destroyed): `t` is
    /// the terrain after the change. Distances can only fall, so each field
    /// is repaired from the changed cells outward instead of rebuilt; the
    /// result is exactly what a fresh build gives. A field whose goals were
    /// all under walls is dropped and rebuilt when next asked for (an open
    /// goal cell would replace them).
    pub fn opened(&mut self, t: &Terrain, cells: &[(i32, i32)]) {
        let version = self.version;
        self.fields
            .retain(|_, f| f.version == version && !f.walled_goals);
        for (key, f) in self.fields.iter_mut() {
            repair(t, *key, &mut f.dist, cells);
        }
    }
}

/// Whether (x, y) is a goal cell of `key`: `Some(walled)` inside the goal
/// radius where the ant may stand (or, digging, enter), else `None`.
fn goal_cell(t: &Terrain, key: FieldKey, x: i32, y: i32) -> Option<bool> {
    let dx = x - key.x;
    let dy = y - key.y;
    if dx * dx + dy * dy > key.r * key.r + key.r {
        return None;
    }
    if !(t.standable(x, y, key.colony, key.mode)
        || (key.mode == Mode::Dig && x >= 0 && y >= 0 && x < t.w && y < t.h && {
            let i = (y * t.w + x) as usize;
            t.enter(i, key.colony, key.mode).is_some()
        }))
    {
        return None;
    }
    Some(t.wall[(y * t.w + x) as usize] != NO_WALL)
}

const STEPS8: [(i32, i32, u32); 8] = [
    (1, 0, STRAIGHT),
    (-1, 0, STRAIGHT),
    (0, 1, STRAIGHT),
    (0, -1, STRAIGHT),
    (1, 1, DIAGONAL),
    (1, -1, DIAGONAL),
    (-1, 1, DIAGONAL),
    (-1, -1, DIAGONAL),
];

/// The cost of the step from (x, y) by (dx, dy), entering the cell there,
/// or `None` if the step is closed (as in `build`).
fn step_cost(t: &Terrain, key: FieldKey, x: i32, y: i32, dx: i32, dy: i32, c: u32) -> Option<u32> {
    let (nx, ny) = (x + dx, y + dy);
    if nx < 0 || ny < 0 || nx >= t.w || ny >= t.h {
        return None;
    }
    let extra = t.enter((ny * t.w + nx) as usize, key.colony, key.mode)?;
    if dx != 0 && dy != 0 {
        t.enter((y * t.w + nx) as usize, key.colony, key.mode)?;
        t.enter((ny * t.w + x) as usize, key.colony, key.mode)?;
    }
    Some(c + extra)
}

/// Lowers a field's distances after `cells` opened (see `Nav::opened`):
/// new goals and every cell round an opened one take their best distance
/// from their neighbours, then the drop spreads outward, Dijkstra-style.
fn repair(t: &Terrain, key: FieldKey, dist: &mut [u16], cells: &[(i32, i32)]) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let w = t.w;
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    let lower = |dist: &mut [u16], heap: &mut BinaryHeap<Reverse<(u32, u32)>>, i: usize, d: u32| {
        if d < dist[i] as u32 && d < UNREACHED as u32 {
            dist[i] = d as u16;
            heap.push(Reverse((d, i as u32)));
        }
    };
    for &(cx, cy) in cells {
        for (ax, ay) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (cx + dx, cy + dy))) {
            if ax < 0 || ay < 0 || ax >= t.w || ay >= t.h {
                continue;
            }
            let i = (ay * w + ax) as usize;
            if goal_cell(t, key, ax, ay) == Some(false) {
                lower(dist, &mut heap, i, 0);
                continue;
            }
            // The best step into (ax, ay) from any neighbour.
            for (dx, dy, c) in STEPS8 {
                let (bx, by) = (ax - dx, ay - dy);
                if bx < 0 || by < 0 || bx >= t.w || by >= t.h {
                    continue;
                }
                let from = dist[(by * w + bx) as usize];
                if from == UNREACHED {
                    continue;
                }
                if let Some(cost) = step_cost(t, key, bx, by, dx, dy, c) {
                    lower(dist, &mut heap, i, from as u32 + cost);
                }
            }
        }
    }
    while let Some(Reverse((d, i))) = heap.pop() {
        let i = i as usize;
        if d > dist[i] as u32 {
            continue;
        }
        let (x, y) = ((i as i32) % w, (i as i32) / w);
        for (dx, dy, c) in STEPS8 {
            if let Some(cost) = step_cost(t, key, x, y, dx, dy, c) {
                let ni = ((y + dy) * w + x + dx) as usize;
                lower(dist, &mut heap, ni, d + cost);
            }
        }
    }
}

fn build(t: &Terrain, key: FieldKey) -> (Vec<u16>, bool) {
    let n = (t.w * t.h) as usize;
    // Goal cells: open cells within the radius. Ants pass through friendly
    // walls but never stop on one (TRIAGE row 38: retreating ants used to
    // stop on the walls round a nest, looking stuck), so wall cells are
    // goals only when the whole goal area is walled.
    let mut goals: Vec<usize> = Vec::new();
    let mut walled: Vec<usize> = Vec::new();
    for y in key.y - key.r..=key.y + key.r {
        for x in key.x - key.r..=key.x + key.r {
            match goal_cell(t, key, x, y) {
                Some(false) => goals.push((y * t.w + x) as usize),
                Some(true) => walled.push((y * t.w + x) as usize),
                None => {}
            }
        }
    }
    let walled_goals = goals.is_empty() && !walled.is_empty();
    if goals.is_empty() {
        goals = walled;
    }
    // Entry cost of every cell, worked out once (`CLOSED` where the ant
    // cannot enter), in a grid padded by one closed cell on every side so
    // the search needs no bounds checks.
    let pw = (t.w + 2) as usize;
    let ph = (t.h + 2) as usize;
    let mut cost = vec![CLOSED; pw * ph];
    for y in 0..t.h {
        for x in 0..t.w {
            let i = (y * t.w + x) as usize;
            if let Some(c) = t.enter(i, key.colony, key.mode) {
                cost[(y as usize + 1) * pw + x as usize + 1] = c as u8;
            }
        }
    }
    // Dijkstra with a bucket queue (Dial's algorithm): every step costs at
    // most `MAX_STEP`, so a ring of `RING` buckets holds every distance
    // still waiting. The distances are exactly a heap's.
    let mut pdist = vec![u32::MAX; pw * ph];
    let mut ring: Vec<Vec<u32>> = vec![Vec::new(); RING];
    let mut waiting = 0usize;
    for i in goals {
        let pi = (i / t.w as usize + 1) * pw + i % t.w as usize + 1;
        if pdist[pi] != 0 {
            pdist[pi] = 0;
            ring[0].push(pi as u32);
            waiting += 1;
        }
    }
    let pwi = pw as isize;
    let steps: [(isize, u32, isize, isize); 8] = [
        (1, STRAIGHT, 0, 0),
        (-1, STRAIGHT, 0, 0),
        (pwi, STRAIGHT, 0, 0),
        (-pwi, STRAIGHT, 0, 0),
        (pwi + 1, DIAGONAL, 1, pwi),
        (-pwi + 1, DIAGONAL, 1, -pwi),
        (pwi - 1, DIAGONAL, -1, pwi),
        (-pwi - 1, DIAGONAL, -1, -pwi),
    ];
    let mut d = 0u32;
    let mut bucket: Vec<u32> = Vec::new();
    while waiting > 0 {
        let slot = d as usize % RING;
        std::mem::swap(&mut bucket, &mut ring[slot]);
        waiting -= bucket.len();
        for &pi in &bucket {
            let pi = pi as usize;
            if pdist[pi] != d {
                continue;
            }
            for &(off, c, side_a, side_b) in &steps {
                let ni = (pi as isize + off) as usize;
                let extra = cost[ni];
                if extra == CLOSED {
                    continue;
                }
                // No cutting corners past closed cells.
                if side_a != 0
                    && (cost[(pi as isize + side_a) as usize] == CLOSED
                        || cost[(pi as isize + side_b) as usize] == CLOSED)
                {
                    continue;
                }
                let nd = d + c + extra as u32;
                // Distances from UNREACHED up are never stored, so they
                // are never spread either (`repair` relies on it).
                if nd < pdist[ni] && nd < UNREACHED as u32 {
                    pdist[ni] = nd;
                    ring[nd as usize % RING].push(ni as u32);
                    waiting += 1;
                }
            }
        }
        bucket.clear();
        d += 1;
    }
    let mut out = Vec::with_capacity(n);
    for y in 0..t.h as usize {
        for x in 0..t.w as usize {
            let d = pdist[(y + 1) * pw + x + 1];
            out.push(if d >= UNREACHED as u32 {
                UNREACHED
            } else {
                d as u16
            });
        }
    }
    (out, walled_goals)
}

/// The neighbouring cell (or this one) with the lowest distance, preferring
/// straight steps on ties. Returns `None` when nothing is reachable.
pub fn downhill(f: &Field, w: i32, h: i32, x: i32, y: i32) -> Option<(i32, i32)> {
    let here = f.at(w, x, y);
    let mut best = (here, x, y);
    for (dx, dy) in [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ] {
        let (nx, ny) = (x + dx, y + dy);
        if nx < 0 || ny < 0 || nx >= w || ny >= h {
            continue;
        }
        let d = f.at(w, nx, ny);
        if d < best.0 {
            best = (d, nx, ny);
        }
    }
    if best.0 == UNREACHED {
        None
    } else {
        Some((best.1, best.2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain_from(rows: &[&str]) -> (i32, i32, Vec<Ground>, Vec<u8>) {
        let h = rows.len() as i32;
        let w = rows[0].len() as i32;
        let mut ground = Vec::new();
        let mut wall = Vec::new();
        for r in rows {
            for c in r.chars() {
                ground.push(match c {
                    '#' => Ground::Rock,
                    '~' => Ground::Dirt,
                    _ => Ground::Floor,
                });
                wall.push(match c {
                    'A' => 0,
                    'B' => 1,
                    _ => NO_WALL,
                });
            }
        }
        (w, h, ground, wall)
    }

    /// The search before the bucket queue: a binary heap, bounds checks
    /// and `enter` per step. The bucket queue must give the same numbers.
    fn heap_build(t: &Terrain, key: FieldKey) -> Vec<u16> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let n = (t.w * t.h) as usize;
        let mut dist = vec![u32::MAX; n];
        let mut heap = BinaryHeap::new();
        let mut goals = Vec::new();
        let mut walled = Vec::new();
        for y in key.y - key.r..=key.y + key.r {
            for x in key.x - key.r..=key.x + key.r {
                let (dx, dy) = (x - key.x, y - key.y);
                if dx * dx + dy * dy > key.r * key.r + key.r {
                    continue;
                }
                if t.standable(x, y, key.colony, key.mode)
                    || (key.mode == Mode::Dig && x >= 0 && y >= 0 && x < t.w && y < t.h && {
                        let i = (y * t.w + x) as usize;
                        t.enter(i, key.colony, key.mode).is_some()
                    })
                {
                    let i = (y * t.w + x) as usize;
                    if t.wall[i] == NO_WALL {
                        goals.push(i);
                    } else {
                        walled.push(i);
                    }
                }
            }
        }
        if goals.is_empty() {
            goals = walled;
        }
        for i in goals {
            dist[i] = 0;
            heap.push(Reverse((0u32, i as u32)));
        }
        while let Some(Reverse((d, i))) = heap.pop() {
            let i = i as usize;
            if d > dist[i] {
                continue;
            }
            let (x, y) = ((i as i32) % t.w, (i as i32) / t.w);
            for (dx, dy, c) in [
                (1, 0, STRAIGHT),
                (-1, 0, STRAIGHT),
                (0, 1, STRAIGHT),
                (0, -1, STRAIGHT),
                (1, 1, DIAGONAL),
                (1, -1, DIAGONAL),
                (-1, 1, DIAGONAL),
                (-1, -1, DIAGONAL),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= t.w || ny >= t.h {
                    continue;
                }
                let ni = (ny * t.w + nx) as usize;
                let Some(extra) = t.enter(ni, key.colony, key.mode) else {
                    continue;
                };
                if dx != 0 && dy != 0 {
                    let a = (y * t.w + nx) as usize;
                    let b = (ny * t.w + x) as usize;
                    if t.enter(a, key.colony, key.mode).is_none()
                        || t.enter(b, key.colony, key.mode).is_none()
                    {
                        continue;
                    }
                }
                let nd = d + c + extra;
                if nd < dist[ni] {
                    dist[ni] = nd;
                    heap.push(Reverse((nd, ni as u32)));
                }
            }
        }
        dist.into_iter()
            .map(|d| d.min(UNREACHED as u32) as u16)
            .collect()
    }

    #[test]
    fn the_bucket_queue_matches_a_heap_on_mixed_terrain() {
        let mut rng = crate::dice::Rng::new(7);
        for round in 0..12 {
            let (w, h) = (37 + round, 29 + 2 * round);
            let n = (w * h) as usize;
            let mut g = Vec::with_capacity(n);
            let mut wl = Vec::with_capacity(n);
            for _ in 0..n {
                g.push(match rng.below(10) {
                    0 | 1 => Ground::Rock,
                    2 => Ground::Dirt,
                    _ => Ground::Floor,
                });
                wl.push(match rng.below(12) {
                    0 => 0,
                    1 => 1,
                    2 => 2,
                    _ => NO_WALL,
                });
            }
            let teams = [0u8, 1, 0];
            let t = Terrain {
                w,
                h,
                ground: &g,
                wall: &wl,
                teams: &teams,
            };
            for mode in [Mode::Walk, Mode::Dig, Mode::Fly] {
                for colony in 0..3u8 {
                    let key = FieldKey {
                        colony,
                        mode,
                        x: rng.range(0, w - 1),
                        y: rng.range(0, h - 1),
                        r: rng.range(0, 4),
                    };
                    assert_eq!(build(&t, key).0, heap_build(&t, key), "{key:?}");
                }
            }
        }
    }

    #[test]
    fn repaired_fields_match_fresh_builds_as_dirt_is_dug_and_walls_fall() {
        // Lockstep peers that restore a snapshot build their fields fresh,
        // so a repaired field must equal a fresh one exactly.
        let mut rng = crate::dice::Rng::new(11);
        for round in 0..6 {
            let (w, h) = (31 + 3 * round, 23 + 2 * round);
            let n = (w * h) as usize;
            let mut g: Vec<Ground> = (0..n)
                .map(|_| match rng.below(10) {
                    0 | 1 => Ground::Rock,
                    2..=4 => Ground::Dirt,
                    _ => Ground::Floor,
                })
                .collect();
            // Walls stand only on floor, as in a match.
            let mut wl: Vec<u8> = (0..n)
                .map(|i| match rng.below(8) {
                    0 if g[i] == Ground::Floor => 0,
                    1 if g[i] == Ground::Floor => 1,
                    _ => NO_WALL,
                })
                .collect();
            let teams = [0u8, 1];
            let mut keys = Vec::new();
            for mode in [Mode::Walk, Mode::Dig, Mode::Fly] {
                for colony in 0..2u8 {
                    keys.push(FieldKey {
                        colony,
                        mode,
                        x: rng.range(0, w - 1),
                        y: rng.range(0, h - 1),
                        r: rng.range(0, 3),
                    });
                }
            }
            let mut nav = Nav::default();
            for step in 0..40 {
                let t = Terrain {
                    w,
                    h,
                    ground: &g,
                    wall: &wl,
                    teams: &teams,
                };
                for &key in &keys {
                    let fresh = build(&t, key).0;
                    assert_eq!(
                        nav.field(&t, key).dist,
                        fresh,
                        "round {round} step {step} {key:?}"
                    );
                }
                // Open one to three cells: dig dirt or knock down a wall.
                let mut cells = Vec::new();
                for _ in 0..1 + rng.below(3) {
                    let (x, y) = (rng.range(0, w - 1), rng.range(0, h - 1));
                    let i = (y * w + x) as usize;
                    if g[i] == Ground::Dirt {
                        g[i] = Ground::Floor;
                        cells.push((x, y));
                    } else if wl[i] != NO_WALL {
                        wl[i] = NO_WALL;
                        cells.push((x, y));
                    }
                }
                let t = Terrain {
                    w,
                    h,
                    ground: &g,
                    wall: &wl,
                    teams: &teams,
                };
                nav.opened(&t, &cells);
            }
        }
    }

    #[test]
    fn allies_share_one_field() {
        let (w, h, g, wl) = terrain_from(&[".B.", ".A.", ".B."]);
        let teams = [0u8, 1, 0];
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &teams,
        };
        let mut nav = Nav::default();
        let key = |colony| FieldKey {
            colony,
            mode: Mode::Walk,
            x: 2,
            y: 1,
            r: 0,
        };
        let a = nav.field(&t, key(0)).dist.clone();
        assert_eq!(nav.field(&t, key(2)).dist, a);
        assert_eq!(nav.len(), 1, "colony 2 is on colony 0's team");
        assert_ne!(nav.field(&t, key(1)).dist, a);
        assert_eq!(nav.len(), 2);
    }

    #[test]
    fn distance_goes_around_rock() {
        let (w, h, g, wl) = terrain_from(&[
            ".....", //
            ".###.", ".#G#.", ".#.#.", ".....",
        ]);
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &[],
        };
        let mut nav = Nav::default();
        let f = nav.field(
            &t,
            FieldKey {
                colony: 0,
                mode: Mode::Walk,
                x: 2,
                y: 2,
                r: 0,
            },
        );
        // From the top-left corner the way in is round to the bottom.
        assert_eq!(f.at(w, 2, 2), 0);
        assert_eq!(f.at(w, 2, 3), 10);
        assert!(f.at(w, 0, 0) > 40);
        assert_eq!(f.at(w, 1, 1), UNREACHED);
    }

    #[test]
    fn enemy_walls_block_but_own_walls_do_not() {
        let (w, h, g, wl) = terrain_from(&[".A.", ".A.", ".A."]);
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &[],
        };
        let mut nav = Nav::default();
        let key = |colony| FieldKey {
            colony,
            mode: Mode::Walk,
            x: 2,
            y: 1,
            r: 0,
        };
        assert_ne!(nav.field(&t, key(0)).at(w, 0, 1), UNREACHED);
        assert_eq!(nav.field(&t, key(1)).at(w, 0, 1), UNREACHED);
        // Digging ants path through the enemy wall at a cost.
        let dig = FieldKey {
            mode: Mode::Dig,
            ..key(1)
        };
        let d = nav.field(&t, dig).at(w, 0, 1);
        assert!(d != UNREACHED && d >= DIG_WALL as u16);
        // Flying Ants ignore it.
        let fly = FieldKey {
            mode: Mode::Fly,
            ..key(1)
        };
        assert_eq!(nav.field(&t, fly).at(w, 0, 1), 20);
    }

    #[test]
    fn flying_ants_cross_walls_not_rock_or_dirt() {
        let (w, h, g, wl) = terrain_from(&[".#.", ".~.", ".#."]);
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &[],
        };
        let mut nav = Nav::default();
        let f = nav.field(
            &t,
            FieldKey {
                colony: 0,
                mode: Mode::Fly,
                x: 2,
                y: 1,
                r: 0,
            },
        );
        assert_eq!(f.at(w, 0, 1), UNREACHED);
    }

    #[test]
    fn stale_fields_are_rebuilt_after_invalidation() {
        let (w, h, mut g, wl) = terrain_from(&["..~.."]);
        let key = FieldKey {
            colony: 0,
            mode: Mode::Walk,
            x: 4,
            y: 0,
            r: 0,
        };
        let mut nav = Nav::default();
        {
            let t = Terrain {
                w,
                h,
                ground: &g,
                wall: &wl,
                teams: &[],
            };
            assert_eq!(nav.field(&t, key).at(w, 0, 0), UNREACHED);
        }
        g[2] = Ground::Floor;
        nav.invalidate();
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &[],
        };
        assert_eq!(nav.field(&t, key).at(w, 0, 0), 40);
    }

    #[test]
    fn downhill_steps_toward_the_goal() {
        let (w, h, g, wl) = terrain_from(&["....."]);
        let t = Terrain {
            w,
            h,
            ground: &g,
            wall: &wl,
            teams: &[],
        };
        let mut nav = Nav::default();
        let f = nav.field(
            &t,
            FieldKey {
                colony: 0,
                mode: Mode::Walk,
                x: 4,
                y: 0,
                r: 0,
            },
        );
        assert_eq!(downhill(f, w, h, 1, 0), Some((2, 0)));
        assert_eq!(downhill(f, w, h, 4, 0), Some((4, 0)));
    }
}
