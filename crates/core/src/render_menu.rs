//! Menu Background (TRIAGE row 89): the dim, still picture behind every menu
//! screen.
//!
//! Each screen gets its own scene, fixed by the screen's key: a generated
//! map's ground (tunnels, chambers, dirt and the biome's props, painted by
//! `render::terrain_image`), seen through a dark layer so the menus stay
//! readable, with trails of ant silhouettes walking between chambers. Nothing
//! moves: the page paints the image once per screen change and replays the
//! draw list again only when the window is resized.
//!
//! `MenuBackground::new(screen)` paints the image; `draw(view_w, view_h)`
//! lays out the draw list for a view (sixteenths of a pixel, as
//! `render::frame`), with its own palette (`palette_json`).

use crate::dice::Rng;
use crate::fixed::{heading16, CELL};
use crate::map::{walk_distances, Map};
use crate::render::{ant_shape_id, op, terrain_image, IMAGE_TERRAIN, SCALE_MAX, SCALE_ONE, SUB_PX};
use crate::rules::Species;
use crate::settings::{Biome, Difficulty, Level, MapType, MatchSettings, SeatKind};
use crate::world::World;

/// Image pixels per cell of the background's ground.
pub const IMAGE_SCALE: i32 = 6;

/// The colour of the dark layer over the ground, and how much of it covers
/// the ground, in percent. With `DIM_PERCENT` this high no part of the
/// ground is brighter than `MAX_LUMA` (checked by a test).
const DIM_RGB: [u8; 3] = [14, 10, 7];
const DIM_PERCENT: i32 = 72;
/// Picnic's blanket is the brightest ground, so it is dimmed further.
const DIM_PICNIC_PERCENT: i32 = 78;
/// Ant silhouettes: a dark shade over the dimmed ground.
const SILHOUETTE_RGB: [u8; 3] = [6, 4, 2];
const SILHOUETTE_PERCENT: i32 = 42;
/// Darker bands at the top and bottom edges, so the title sits on calm
/// ground: `EDGE_STEPS` bands, each this dark, stacked into a soft fade.
const EDGE_PERCENT: i32 = 9;
const EDGE_STEPS: i32 = 4;
/// The brightest any background pixel may be, as luma out of 255. The
/// menus' text (#f4ead8) and headings (#f0c24b) keep a contrast of at least
/// 4.5 : 1 (WCAG AA) on ground this bright: about 6.7 and 4.8.
pub const MAX_LUMA: i32 = 80;
/// The most the background may average, as luma out of 255.
pub const MEAN_LUMA: i32 = 40;

// Palette indices of the background's own palette.
const C_CLEAR: i32 = 0;
const C_DIM: i32 = 1;
const C_SILHOUETTE: i32 = 2;
const C_EDGE: i32 = 3;
const C_DIM_PICNIC: i32 = 4;

/// The background's palette, CSS colours by index.
pub fn palette() -> Vec<String> {
    let rgba = |c: [u8; 3], percent: i32| {
        format!(
            "rgba({},{},{},{}.{:02})",
            c[0],
            c[1],
            c[2],
            percent / 100,
            percent % 100
        )
    };
    vec![
        format!("rgb({},{},{})", DIM_RGB[0], DIM_RGB[1], DIM_RGB[2]),
        rgba(DIM_RGB, DIM_PERCENT),
        rgba(SILHOUETTE_RGB, SILHOUETTE_PERCENT),
        rgba(DIM_RGB, EDGE_PERCENT),
        rgba(DIM_RGB, DIM_PICNIC_PERCENT),
    ]
}

/// The palette as JSON text, for the page's renderer.
pub fn palette_json() -> String {
    serde_json::to_string(&palette()).unwrap_or_default()
}

/// One screen's scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scene {
    seed: u64,
    map_type: MapType,
    biome: Biome,
    /// How far the view is zoomed into the map, in percent of "the map
    /// covers the view".
    zoom: i32,
    /// Which chamber the view centres on (index mod the chamber count).
    focus: usize,
    /// Trails of ants: from one chamber to another (indices mod the count).
    trails: [(usize, usize); 2],
    /// The species walking the trails.
    species: [Species; 3],
    /// Ants standing about in each trail's first chamber.
    milling: u32,
}

/// The scene for a screen key (`ui.screen` in the view model). A screen
/// the table does not know still gets a scene of its own, from its key.
fn scene(screen: &str) -> Scene {
    use Species::{Acid, Carpenter, Fire, Harvester, Nurse, Soldier, Turtle};
    let s = |seed, map_type, biome, zoom, focus, trails, species, milling| Scene {
        seed,
        map_type,
        biome,
        zoom,
        focus,
        trails,
        species,
        milling,
    };
    match screen {
        "main_menu" => s(
            11,
            MapType::Tunnels,
            Biome::ForestFloor,
            125,
            0,
            [(0, 2), (1, 3)],
            [Harvester, Soldier, Carpenter],
            10,
        ),
        "play_vs_ai" => s(
            23,
            MapType::Open,
            Biome::FlowerField,
            150,
            1,
            [(0, 1), (2, 0)],
            [Soldier, Fire, Harvester],
            8,
        ),
        "match_settings" => s(
            37,
            MapType::Maze,
            Biome::Riverbank,
            140,
            2,
            [(1, 2), (0, 3)],
            [Harvester, Carpenter, Turtle],
            6,
        ),
        "online" => s(
            41,
            MapType::Tunnels,
            Biome::Picnic,
            110,
            3,
            [(0, 1), (1, 0)],
            [Harvester, Soldier, Acid],
            6,
        ),
        "lobby" => s(
            53,
            MapType::Open,
            Biome::ForestFloor,
            135,
            0,
            [(0, 3), (3, 1)],
            [Soldier, Nurse, Harvester],
            12,
        ),
        "player_settings" => s(
            67,
            MapType::Tunnels,
            Biome::Riverbank,
            160,
            1,
            [(1, 0), (2, 1)],
            [Carpenter, Harvester, Soldier],
            8,
        ),
        "match_history" => s(
            79,
            MapType::Maze,
            Biome::Picnic,
            120,
            2,
            [(2, 3), (0, 2)],
            [Fire, Soldier, Harvester],
            8,
        ),
        other => {
            // Any other screen: a scene from its key (FNV-1a).
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in other.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            let types = [MapType::Tunnels, MapType::Open, MapType::Maze];
            s(
                h % 1000,
                types[(h / 7 % 3) as usize],
                Biome::ALL[(h / 11 % 4) as usize],
                110 + (h / 13 % 50) as i32,
                (h / 17 % 4) as usize,
                [(0, 1), (2, 3)],
                [Harvester, Soldier, Carpenter],
                8,
            )
        }
    }
}

/// A painted background: the ground image and what the draw list needs.
pub struct MenuBackground {
    scene: Scene,
    map: Map,
    w: i32,
    h: i32,
    rgba: Vec<u8>,
}

impl MenuBackground {
    /// Paints the background for a screen key. The same key always gives
    /// the same picture.
    pub fn new(screen: &str) -> MenuBackground {
        let scene = scene(screen);
        let settings = MatchSettings {
            map_size: Level::Low,
            biome: scene.biome,
            map_type: scene.map_type,
            ..MatchSettings::default()
        };
        let seats = [SeatKind::Ai(Difficulty::Pupa); 2];
        let world = World::new(scene.seed, settings, &seats);
        let (w, h, rgba) = terrain_image(&world, IMAGE_SCALE);
        MenuBackground {
            scene,
            map: world.map,
            w,
            h,
            rgba,
        }
    }

    pub fn width(&self) -> i32 {
        self.w
    }

    pub fn height(&self) -> i32 {
        self.h
    }

    /// The ground image, RGBA bytes.
    pub fn image(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// How much the dark layer covers the ground, in percent.
    pub fn dim_percent(&self) -> i32 {
        if self.scene.biome == Biome::Picnic {
            DIM_PICNIC_PERCENT
        } else {
            DIM_PERCENT
        }
    }

    /// The image rectangle (x, y, w, h in image pixels) the view shows: as
    /// much as covers the view, zoomed in by the scene's zoom, centred on
    /// the focus chamber where the edges allow.
    fn source(&self, view_w: i32, view_h: i32) -> [i32; 4] {
        let (vw, vh) = (view_w.max(1) as i64, view_h.max(1) as i64);
        let (iw, ih) = (self.w as i64, self.h as i64);
        let (mut sw, mut sh) = if iw * vh >= ih * vw {
            (ih * vw / vh, ih)
        } else {
            (iw, iw * vh / vw)
        };
        let zoom = self.scene.zoom.max(100) as i64;
        sw = (sw * 100 / zoom).max(1);
        sh = (sh * 100 / zoom).max(1);
        let c = &self.map.chambers[self.scene.focus % self.map.chambers.len()];
        let k = IMAGE_SCALE as i64;
        let fx = c.cx as i64 * k + k / 2;
        let fy = c.cy as i64 * k + k / 2;
        let sx = (fx - sw / 2).clamp(0, iw - sw);
        let sy = (fy - sh / 2).clamp(0, ih - sh);
        [sx as i32, sy as i32, sw as i32, sh as i32]
    }

    /// The draw list for a view of `view_w` × `view_h` pixels.
    pub fn draw(&self, view_w: i32, view_h: i32) -> Vec<i32> {
        let (vw, vh) = (view_w.max(1) * SUB_PX, view_h.max(1) * SUB_PX);
        let src = self.source(view_w, view_h);
        let mut out = vec![op::CLEAR, C_CLEAR, op::IMAGE, IMAGE_TERRAIN];
        out.extend_from_slice(&src);
        out.extend_from_slice(&[0, 0, vw, vh]);
        let dim = if self.scene.biome == Biome::Picnic {
            C_DIM_PICNIC
        } else {
            C_DIM
        };
        rect(&mut out, dim, 0, 0, vw, vh);
        // A soft dark fade along the top and bottom sixths.
        for step in 1..=EDGE_STEPS {
            let band = vh * step / (6 * EDGE_STEPS);
            rect(&mut out, C_EDGE, 0, 0, vw, band);
            rect(&mut out, C_EDGE, 0, vh - band, vw, band);
        }

        // Cell centre to draw-list units, and the ant scale index: ants are
        // drawn at three times their size in the match, so they read as
        // shapes.
        let k = IMAGE_SCALE as i64;
        let to_view = |cx: i32, cy: i32| -> (i32, i32) {
            let px = cx as i64 * k + k / 2 - src[0] as i64;
            let py = cy as i64 * k + k / 2 - src[1] as i64;
            (
                (px * vw as i64 / src[2] as i64) as i32,
                (py * vh as i64 / src[3] as i64) as i32,
            )
        };
        let per_cell = k * vw as i64 / src[2] as i64;
        let scale =
            (per_cell * SCALE_ONE as i64 * 3 / CELL as i64).clamp(1, SCALE_MAX as i64) as i32;
        let margin = (per_cell * 3) as i32;
        let ant = |out: &mut Vec<i32>, s: Species, cell: (i32, i32), heading: u8| {
            let (x, y) = to_view(cell.0, cell.1);
            if x < -margin || y < -margin || x > vw + margin || y > vh + margin {
                return;
            }
            out.extend_from_slice(&[
                op::FILL_SHAPE,
                ant_shape_id(s, heading, true),
                x,
                y,
                scale,
                C_SILHOUETTE,
            ]);
        };

        let n = self.map.chambers.len();
        let mut rng = Rng::new(self.scene.seed);
        for (t, &(a, b)) in self.scene.trails.iter().enumerate() {
            let (from, to) = (&self.map.chambers[a % n], &self.map.chambers[b % n]);
            let path = self.path((from.cx, from.cy), (to.cx, to.cy));
            // An ant every few cells, the species taking turns; every third
            // ant walks home the other way.
            let gap = 5 + t;
            let mut i = gap / 2;
            let mut count = 0usize;
            while i + 2 < path.len() {
                let (p, q) = (path[i], path[i + 2]);
                let back = count % 3 == 2;
                let (dx, dy) = if back {
                    (p.0 - q.0, p.1 - q.1)
                } else {
                    (q.0 - p.0, q.1 - p.1)
                };
                let heading = heading16(dx, dy).unwrap_or(0);
                let s = self.scene.species[count % self.scene.species.len()];
                ant(&mut out, s, p, heading);
                i += gap + (rng.below(3) as usize);
                count += 1;
            }
            // A few ants standing about in the trail's first chamber.
            for _ in 0..self.scene.milling {
                let r = from.r.max(2);
                let x = from.cx + rng.below((2 * r) as u32) as i32 - r;
                let y = from.cy + rng.below((2 * r) as u32) as i32 - r;
                if (x - from.cx) * (x - from.cx) + (y - from.cy) * (y - from.cy) > r * r * 2 / 3
                    || !self.map.is_floor(x, y)
                {
                    continue;
                }
                let s = self.scene.species[rng.below(3) as usize];
                ant(&mut out, s, (x, y), rng.below(16) as u8);
            }
        }
        out
    }

    /// A walking path over floor from `from` to `to`, cell by cell (empty if
    /// either is not floor or `to` cannot be reached).
    fn path(&self, from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
        let map = &self.map;
        let dist = walk_distances(map, to.0, to.1);
        let mut at = from;
        let mut out = Vec::new();
        if !map.inside(at.0, at.1) || dist[map.idx(at.0, at.1)] == u32::MAX {
            return out;
        }
        out.push(at);
        while dist[map.idx(at.0, at.1)] > 0 {
            let mut best = at;
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
                let (x, y) = (at.0 + dx, at.1 + dy);
                if map.inside(x, y) && dist[map.idx(x, y)] < dist[map.idx(best.0, best.1)] {
                    best = (x, y);
                }
            }
            if best == at {
                break;
            }
            at = best;
            out.push(at);
        }
        out
    }
}

fn rect(out: &mut Vec<i32>, colour: i32, x: i32, y: i32, w: i32, h: i32) {
    out.extend_from_slice(&[op::POLY, colour, 4, x, y, x + w, y, x + w, y + h, x, y + h]);
}

#[cfg(test)]
#[path = "render_menu_tests.rs"]
mod tests;
