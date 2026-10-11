//! Navigation by flow fields. One distance field per target and mode, shared
//! by every ant heading to the same place, built with Dijkstra over the cell
//! grid. The cache is pure: a field depends only on the terrain, so the world
//! never needs to save it and every peer gets the same answers.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

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

/// The terrain a field is built over. `wall` holds the owning colony of a
/// complete wall on each cell, or `NO_WALL`.
pub struct Terrain<'a> {
    pub w: i32,
    pub h: i32,
    pub ground: &'a [Ground],
    pub wall: &'a [u8],
}

pub const NO_WALL: u8 = u8::MAX;

impl Terrain<'_> {
    /// Cost to step onto a cell, or `None` if the cell is closed.
    fn enter(&self, i: usize, colony: u8, mode: Mode) -> Option<u32> {
        let g = self.ground[i];
        let wall = self.wall[i];
        match g {
            Ground::Rock => None,
            Ground::Dirt => (mode == Mode::Dig).then_some(DIG_DIRT),
            Ground::Floor => {
                if wall == NO_WALL || wall == colony || mode == Mode::Fly {
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
                wall == NO_WALL || wall == colony || mode == Mode::Fly
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
            let dist = build(t, key);
            self.fields.insert(
                key,
                Field {
                    dist,
                    version: self.version,
                    used: self.clock,
                },
            );
        }
        let f = self.fields.get_mut(&key).expect("just inserted");
        f.used = self.clock;
        f
    }
}

fn build(t: &Terrain, key: FieldKey) -> Vec<u16> {
    let n = (t.w * t.h) as usize;
    let mut dist = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    for y in key.y - key.r..=key.y + key.r {
        for x in key.x - key.r..=key.x + key.r {
            let dx = x - key.x;
            let dy = y - key.y;
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
                dist[i] = 0;
                heap.push(Reverse((0u32, i as u32)));
            }
        }
    }
    const STEPS: [(i32, i32, u32); 8] = [
        (1, 0, STRAIGHT),
        (-1, 0, STRAIGHT),
        (0, 1, STRAIGHT),
        (0, -1, STRAIGHT),
        (1, 1, DIAGONAL),
        (1, -1, DIAGONAL),
        (-1, 1, DIAGONAL),
        (-1, -1, DIAGONAL),
    ];
    while let Some(Reverse((d, i))) = heap.pop() {
        let i = i as usize;
        if d > dist[i] {
            continue;
        }
        let x = (i as i32) % t.w;
        let y = (i as i32) / t.w;
        for (dx, dy, c) in STEPS {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= t.w || ny >= t.h {
                continue;
            }
            let ni = (ny * t.w + nx) as usize;
            let Some(extra) = t.enter(ni, key.colony, key.mode) else {
                continue;
            };
            if dx != 0 && dy != 0 {
                // No cutting corners past closed cells.
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
        .map(|d| {
            if d >= UNREACHED as u32 {
                UNREACHED
            } else {
                d as u16
            }
        })
        .collect()
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
