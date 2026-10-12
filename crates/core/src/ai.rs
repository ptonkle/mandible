//! The AI players. They run inside the simulation on every peer, so they send
//! nothing over the network, and they issue the same commands a player does.
//! They see only what their team sees, plus the home chambers, which every
//! player knows from the map's symmetry.
//!
//! Difficulty shapes the plan: Larva decides slowly, keeps one nest, skips
//! upgrades and attacks late; Pupa follows a standard plan with a second
//! nest and upgrades. Imago decides often and plays to the economy: Food is
//! the scarce resource, so it trains only the Harvester Ants its nodes can
//! use, scouts every chamber on its side of the map, saves for Tier 2 and
//! its first hatcheries instead of spending on Soldier Ants, fields Turtle
//! and Acid Ants with the upgrades that suit them, meets intruders with the
//! whole army, and retreats from fights it is losing.
//!
//! Every difficulty finishes a match: the army attacks known queens and
//! hatcheries first, the army needed to attack shrinks as the match runs
//! long, and when the enemy is nowhere known the army splits to sweep the
//! map.

use serde::{Deserialize, Serialize};

use crate::fixed::{Pos, CELL};
use crate::rules::{
    evolve_cost, ticks, Resource, Species, StructureKind, Upgrade, POP_CAP, TICKS_PER_SECOND,
};
use crate::settings::Difficulty;
use crate::world::{Amount, Command, Focus, Work, World};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AiState {
    pub attacking: bool,
    pub wave: i32,
    pub retreat_until: i32,
}

/// Ticks between decisions.
pub fn interval(d: Difficulty) -> i32 {
    ticks(match d {
        Difficulty::Larva => 30,
        Difficulty::Pupa => 14,
        Difficulty::Imago => 6,
    })
}

/// Match time in tenths of a second: every timing in the AI's plan is
/// written in these, so it keeps its meaning whatever the tick rate.
fn tenths(w: &World) -> i32 {
    w.tick * 10 / TICKS_PER_SECOND
}

pub fn due(tick: i32, colony: usize, d: Difficulty) -> bool {
    (tick + colony as i32 * 3) % interval(d) == 0
}

struct Plan {
    harvesters: i32,
    carpenters: i32,
    nests: i32,
    upgrades: bool,
    attack_pop: i32,
    t2_after: i32,
    t3_after: i32,
}

fn plan(d: Difficulty) -> Plan {
    match d {
        Difficulty::Larva => Plan {
            harvesters: 9,
            carpenters: 4,
            nests: 0,
            upgrades: false,
            attack_pop: 30,
            t2_after: 2000,
            t3_after: i32::MAX,
        },
        Difficulty::Pupa => Plan {
            harvesters: 16,
            carpenters: 4,
            nests: 1,
            upgrades: true,
            attack_pop: 34,
            t2_after: 900,
            t3_after: 1900,
        },
        Difficulty::Imago => Plan {
            harvesters: 14,
            carpenters: 4,
            nests: 2,
            upgrades: true,
            attack_pop: 33,
            t2_after: 500,
            t3_after: 5000,
        },
    }
}

/// Counts of a colony's ants by species, including queued ones.
fn counts(w: &World, colony: u8) -> [i32; 12] {
    let mut c = [0; 12];
    for a in w.ants.iter().filter(|a| a.colony == colony) {
        c[a.species.index()] += 1;
    }
    for s in w.structures.iter().filter(|s| s.colony == colony) {
        for q in &s.lane {
            if let Work::Train(sp) = q.work {
                c[sp.index()] += 1;
            }
        }
    }
    c
}

/// A spot for a structure near `near`, searched outward ring by ring.
pub fn find_spot(
    w: &World,
    colony: u8,
    kind: StructureKind,
    near: Pos,
    from: i32,
) -> Option<(i32, i32)> {
    let (nx, ny) = near.cell();
    for r in from..from + 14 {
        let steps = 8 + r * 2;
        for k in 0..steps {
            let a = (k * 64 / steps) as usize;
            let x = nx + r * crate::fixed::cos64(a) / crate::fixed::TRIG_ONE;
            let y = ny + r * crate::fixed::sin64(a) / crate::fixed::TRIG_ONE;
            if w.check_place(colony, kind, x, y, x, y).is_ok() {
                return Some((x, y));
            }
        }
    }
    None
}

/// The army this difficulty wants, as weights per species.
fn army_mix(w: &World, colony: u8, d: Difficulty) -> Vec<(Species, i32)> {
    let tier = w.colonies[colony as usize].tier;
    let soldiers = if d == Difficulty::Imago && tier >= 2 {
        1
    } else {
        4
    };
    let mut mix = vec![(Species::Soldier, soldiers)];
    if tier >= 2 {
        match d {
            Difficulty::Larva => mix.push((Species::Fire, 3)),
            Difficulty::Imago => {
                // Turtle Ants shrug off small hits (flat Armor); Acid Ants
                // stand behind them; Nurse Ants keep both alive.
                mix.push((Species::Acid, 4));
                mix.push((Species::Turtle, 4));
                mix.push((Species::Nurse, 1));
                // Siege Ants break the queens and hatcheries the army is
                // sent to.
                mix.push((Species::Siege, 1));
            }
            _ => {
                mix.push((Species::Acid, 4));
                mix.push((Species::Turtle, 2));
                mix.push((Species::Nurse, 1));
                mix.push((Species::Fire, 2));
                mix.push((Species::Siege, 1));
            }
        }
    }
    if tier >= 3 && d != Difficulty::Larva {
        mix.push((Species::TrapJaw, 3));
        mix.push((Species::Dinosaur, 1));
        if d == Difficulty::Imago {
            // Counter what it has seen: many small ants call for splash,
            // big ants for Trap-jaw Ants, walls for Flying Ants.
            let seen = enemy_seen(w, colony);
            if seen.small > 20 {
                mix.push((Species::Exploding, 3));
                mix.push((Species::Dinosaur, 2));
            }
            if seen.walls > 0 {
                mix.push((Species::Flying, 3));
            }
            if seen.big > 6 {
                mix.push((Species::TrapJaw, 3));
            }
        } else {
            mix.push((Species::Flying, 1));
        }
    }
    mix.retain(|(s, _)| !w.settings.is_disabled(*s));
    mix
}

#[derive(Default)]
struct Seen {
    small: i32,
    big: i32,
    walls: i32,
    army_pop: i32,
    near_home: i32,
    /// The enemy ant nearest one of our queens, among those near a nest.
    intruder: Option<Pos>,
}

fn enemy_seen(w: &World, colony: u8) -> Seen {
    let vis = w.team_visible(colony);
    let mut s = Seen::default();
    let home = w.nearest_queen(colony, Pos::default()).map(|q| q.pos);
    let mut best: Option<(i32, Pos)> = None;
    for a in &w.ants {
        if w.allied(a.colony, colony) {
            continue;
        }
        let (x, y) = a.pos.cell();
        if !vis.get(w.map.idx(x, y)) {
            continue;
        }
        if a.species.is_combat() {
            let st = a.species.stats();
            s.army_pop += st.pop;
            if st.pop <= 1 {
                s.small += 1;
            } else {
                s.big += 1;
            }
            if let Some(h) = home {
                let d = w
                    .queens(colony)
                    .map(|q| q.pos.dist(a.pos))
                    .min()
                    .unwrap_or(i32::MAX)
                    .min(a.pos.dist(h));
                if d < 24 * CELL {
                    s.near_home += 1;
                    if best.map(|b: (i32, Pos)| d < b.0).unwrap_or(true) {
                        best = Some((d, a.pos));
                    }
                }
            }
        }
    }
    for st in &w.structures {
        if st.kind == StructureKind::Wall && !w.allied(st.colony, colony) {
            let (x, y) = st.pos.cell();
            if vis.get(w.map.idx(x, y)) {
                s.walls += 1;
            }
        }
    }
    s.intruder = best.map(|b| b.1);
    s
}

/// Where to attack, nearest the army (its All Ants waypoint) first: enemy
/// ants in sight close to the army; then known enemy queens and hatcheries,
/// which keep a colony alive, then its other structures; then the enemy
/// home chambers not yet seen; then enemy ants in sight anywhere; then the
/// chambers the team has never seen. None means every chamber has been seen
/// and nothing is in sight, so the army must sweep (see `sweep`).
fn attack_point(w: &World, colony: u8) -> Option<Pos> {
    let from = w
        .nearest_queen(colony, Pos::default())
        .map(|q| q.pos)
        .or_else(|| {
            let h = w.map.home(colony as usize);
            Some(Pos::of_cell(h.cx, h.cy))
        })?;
    let army_at = w.colonies[colony as usize].waypoints[Species::Fire.index()]
        .first()
        .copied()
        .unwrap_or(from);
    // With Map Revealed every cell is Explored; what the team has really
    // seen is what counts.
    let explored = w.team_sighted(colony);
    let visible = w.team_visible(colony);
    let enemy = |c: u8| !w.allied(c, colony) && !w.colonies[c as usize].defeated;
    let nearest_to = |at: Pos, ps: Vec<Pos>| ps.into_iter().min_by_key(|p| (p.dist2(at), p.x, p.y));
    let seen: Vec<Pos> = w
        .ants
        .iter()
        .filter(|a| {
            enemy(a.colony) && {
                let (x, y) = a.pos.cell();
                visible.get(w.map.idx(x, y))
            }
        })
        .map(|a| a.pos)
        .collect();
    let close: Vec<Pos> = seen
        .iter()
        .copied()
        .filter(|p| p.dist(army_at) < 16 * CELL)
        .collect();
    if let Some(p) = nearest_to(army_at, close) {
        return Some(p);
    }
    let known = |makes_ants: bool| -> Vec<Pos> {
        w.structures
            .iter()
            .filter(|s| {
                enemy(s.colony)
                    && s.kind.makes_ants() == makes_ants
                    && s.kind != StructureKind::Wall
                    && {
                        let (x, y) = s.pos.cell();
                        explored.get(w.map.idx(x, y))
                    }
            })
            .map(|s| s.pos)
            .collect()
    };
    if let Some(p) = nearest_to(army_at, known(true)) {
        return Some(p);
    }
    if let Some(p) = nearest_to(army_at, known(false)) {
        return Some(p);
    }
    let nearest = |ps: Vec<Pos>| nearest_to(from, ps);
    let homes: Vec<Pos> = w
        .colonies
        .iter()
        .filter(|c| enemy(c.seat))
        .map(|c| {
            let h = w.map.home(c.seat as usize);
            Pos::of_cell(h.cx, h.cy)
        })
        .filter(|p| {
            let (x, y) = p.cell();
            !explored.get(w.map.idx(x, y))
        })
        .collect();
    if let Some(p) = nearest(homes) {
        return Some(p);
    }
    if let Some(p) = nearest_to(army_at, seen) {
        return Some(p);
    }
    // Hunting: an enemy is alive but nowhere known. Search chambers nobody
    // on the team has seen, nearest first; then sweep the chambers out of
    // sight, one after another.
    if let Some(&i) = unseen_chambers(w, colony, from).first() {
        let ch = &w.map.chambers[i];
        return Some(Pos::of_cell(ch.cx, ch.cy));
    }
    // Every chamber has been seen and nothing is in sight: sweep.
    None
}

/// Chambers nobody on the team has had in sight, except the team's own
/// home chambers, nearest `from` first.
pub fn unseen_chambers(w: &World, colony: u8, from: Pos) -> Vec<usize> {
    let my_team = w.colonies[colony as usize].team;
    let seen = w.team_sighted(colony);
    let mut v: Vec<(i64, usize)> = w
        .map
        .chambers
        .iter()
        .enumerate()
        .filter(|(_, ch)| {
            ch.home
                .map(|h| w.colonies[h as usize].team != my_team)
                .unwrap_or(true)
        })
        .filter(|(_, ch)| !seen.get(w.map.idx(ch.cx, ch.cy)))
        .map(|(i, ch)| (Pos::of_cell(ch.cx, ch.cy).dist2(from), i))
        .collect();
    v.sort();
    v.into_iter().map(|(_, i)| i).collect()
}

/// Every chamber in a serpentine tour of the map, band by band (the order
/// `sweep_tour` visits them in).
fn chamber_tour(w: &World) -> Vec<usize> {
    let band = 30;
    let mut v: Vec<(i32, i32, i32, usize)> = w
        .map
        .chambers
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let b = ch.cy / band;
            (b, if b % 2 == 0 { ch.cx } else { -ch.cx }, ch.cy, i)
        })
        .collect();
    v.sort();
    v.into_iter().map(|t| t.3).collect()
}

/// Where Auto-Scout's scout may go next, best first: chambers the team has
/// never seen, nearest first; then, once every chamber has been seen, the
/// chambers out of the team's sight, in tour order after `last`, so the
/// scout keeps sweeping instead of going back and forth.
pub fn scout_candidates(w: &World, colony: u8, from: Pos, last: Option<u32>) -> Vec<usize> {
    let unseen = unseen_chambers(w, colony, from);
    if !unseen.is_empty() {
        return unseen;
    }
    let my_team = w.colonies[colony as usize].team;
    let tour = chamber_tour(w);
    let n = tour.len();
    let start = last
        .and_then(|l| tour.iter().position(|&i| i as u32 == l))
        .map(|k| k + 1)
        .unwrap_or(0);
    (0..n)
        .map(|s| tour[(start + s) % n])
        .filter(|&i| Some(i as u32) != last)
        .filter(|&i| {
            let ch = &w.map.chambers[i];
            let own_home = ch
                .home
                .map(|h| w.colonies[h as usize].team == my_team)
                .unwrap_or(false);
            !own_home && !w.team_sees(colony, ch.cx, ch.cy)
        })
        .collect()
}

/// Every chamber centre and a sample of tunnel cells, in a serpentine tour
/// of the map, band by band.
fn sweep_tour(w: &World) -> Vec<Pos> {
    let band = 30;
    let step = 6;
    let mut spots: Vec<(i32, i32)> = w.map.chambers.iter().map(|ch| (ch.cx, ch.cy)).collect();
    for y in (step / 2..w.map.h).step_by(step as usize) {
        for x in (step / 2..w.map.w).step_by(step as usize) {
            if w.map.is_floor(x, y) && w.map.chamber_at(x, y).is_none() {
                spots.push((x, y));
            }
        }
    }
    let mut tour: Vec<(i32, i32, i32)> = spots
        .into_iter()
        .map(|(x, y)| {
            let b = y / band;
            (b, if b % 2 == 0 { x } else { -x }, y)
        })
        .collect();
    tour.sort();
    tour.into_iter()
        .map(|(b, x, y)| Pos::of_cell(if b % 2 == 0 { x } else { -x }, y))
        .collect()
}

/// Hunting an enemy that is alive but nowhere known: the army splits over
/// four waypoints spread around the sweep tour. Each waypoint moves on along
/// the tour once its spot is in sight, so the waypoints themselves remember
/// where the sweep has got to.
fn sweep(w: &World, colony: u8, from: Pos) -> Vec<Command> {
    const GROUPS: usize = 4;
    let tour = sweep_tour(w);
    let n = tour.len();
    if n == 0 {
        return Vec::new();
    }
    let visible = w.team_visible(colony);
    let seen = |k: usize| {
        let (x, y) = tour[k % n].cell();
        visible.get(w.map.idx(x, y))
    };
    let index_of = |p: Pos| (0..n).min_by_key(|&k| (tour[k].dist2(p), k)).unwrap_or(0);
    let next_dark = |k: usize| (0..n).map(|s| (k + s) % n).find(|&j| !seen(j));
    let cur = &w.colonies[colony as usize].waypoints[Species::Fire.index()];
    let mut want: Vec<usize> = if cur.len() == GROUPS {
        cur.iter().map(|p| index_of(*p)).collect()
    } else {
        let k0 = index_of(from);
        (0..GROUPS).map(|q| (k0 + q * n / GROUPS) % n).collect()
    };
    for k in want.iter_mut() {
        if cur.len() != GROUPS || seen(*k) {
            if let Some(j) = next_dark(*k) {
                *k = j;
            }
        }
    }
    let unchanged = cur.len() == GROUPS
        && cur
            .iter()
            .zip(&want)
            .all(|(p, k)| p.dist(tour[*k]) <= 6 * CELL);
    if unchanged {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (q, k) in want.iter().enumerate() {
        let (x, y) = tour[*k].cell();
        if q == 0 {
            out.push(Command::SetWaypoint {
                species: None,
                x,
                y,
                add: false,
            });
        } else {
            for s in Species::ALL.into_iter().filter(|s| s.is_combat()) {
                out.push(Command::SetWaypoint {
                    species: Some(s),
                    x,
                    y,
                    add: true,
                });
            }
        }
    }
    out
}

pub fn decide(w: &World, colony: u8, d: Difficulty) -> Vec<Command> {
    let mut out = Vec::new();
    let ci = colony as usize;
    let c = &w.colonies[ci];
    let p = plan(d);
    let now = tenths(w);
    let n = counts(w, colony);
    let queens: Vec<Pos> = w.queens(colony).map(|q| q.pos).collect();
    let home = queens.first().copied().unwrap_or_else(|| {
        let h = w.map.home(ci);
        Pos::of_cell(h.cx, h.cy)
    });
    let pop = w.population(colony) + w.queued_pop(colony);
    let housing = w.housing(colony);
    let blueprints: Vec<StructureKind> = w
        .structures
        .iter()
        .filter(|s| s.colony == colony && !s.complete)
        .map(|s| s.kind)
        .collect();
    let mut stock = c.stock;
    let afford = |stock: &[i32; 3], cost: &[i32; 3]| (0..3).all(|i| stock[i] >= cost[i]);
    let spend = |stock: &mut [i32; 3], cost: &[i32; 3]| {
        for i in 0..3 {
            stock[i] -= cost[i];
        }
    };
    let army_pop: i32 = w
        .ants
        .iter()
        .filter(|a| a.colony == colony && a.species.is_combat())
        .map(|a| a.species.stats().pop)
        .sum();
    let hatcheries = w
        .structures
        .iter()
        .filter(|s| {
            s.colony == colony && s.complete && matches!(s.kind, StructureKind::Hatchery(_))
        })
        .count();
    // The army a difficulty keeps at home before spending on anything else.
    let floor = match d {
        Difficulty::Larva => 0,
        Difficulty::Pupa => now / 100,
        // Imago spends the opening on Harvester Ants, Tier 2 and its first
        // hatcheries, with a small guard of Soldier Ants; it trains in
        // earnest once the hatcheries stand.
        Difficulty::Imago if hatcheries < 2 => ((now - 600) / 120).clamp(0, 8),
        Difficulty::Imago => ((now - 900) / 70).max(0),
    }
    .min(24);
    let threat = enemy_seen(w, colony).near_home;
    let safe = army_pop >= floor && !(d == Difficulty::Imago && threat >= 3 && army_pop < 40);

    // Without a queen there is no economy to run, but the army still
    // fights on.
    if !queens.is_empty() {
        // 1. Housing: never stall on Housing Full.
        let pending_housing = blueprints
            .iter()
            .filter(|k| matches!(k, StructureKind::BroodCell | StructureKind::Queen))
            .count() as i32;
        // A brood cell adds 8 housing (TRIAGE row 71), so up to two are
        // placed at once when the army grows fast.
        let cell = StructureKind::BroodCell.stats();
        let mut planned = pending_housing * cell.housing;
        let mut placed: Option<(i32, i32)> = None;
        for k in 0..2 {
            if housing >= POP_CAP || pop + 8 < housing + planned || !afford(&stock, &cell.cost) {
                break;
            }
            // The second spot is searched farther out, clear of the first.
            let Some((x, y)) = find_spot(w, colony, StructureKind::BroodCell, home, 6 + 4 * k)
            else {
                break;
            };
            if placed.is_some_and(|(px, py)| (px - x).abs() + (py - y).abs() < 4) {
                break;
            }
            placed = Some((x, y));
            out.push(Command::Place {
                kind: StructureKind::BroodCell,
                x,
                y,
                x2: x,
                y2: y,
            });
            spend(&mut stock, &cell.cost);
            planned += cell.housing;
        }

        // 2. Harvester Ants come first: one queued per queen while short.
        let harvester_goal = if d == Difficulty::Imago {
            // Enough to saturate every node near a nest.
            node_room(w, colony).clamp(p.harvesters, 64)
        } else {
            p.harvesters + (queens.len() as i32 - 1) * 8
        };
        let queued_harvesters = w
            .queens(colony)
            .flat_map(|q| q.lane.iter())
            .filter(|q| matches!(q.work, Work::Train(Species::Harvester)))
            .count();
        let harvesters_short = n[Species::Harvester.index()] < harvester_goal;
        if harvesters_short && queued_harvesters < queens.len() {
            let cost = Species::Harvester.stats().cost;
            if afford(&stock, &cost) {
                out.push(Command::Train {
                    species: Species::Harvester,
                    amount: Amount::One,
                });
                spend(&mut stock, &cost);
            }
        }
        let big_blueprints = blueprints
            .iter()
            .filter(|k| !matches!(k, StructureKind::BroodCell | StructureKind::Wall))
            .count() as i32;
        let carpenter_goal = if d == Difficulty::Imago {
            (p.carpenters + big_blueprints).min(8)
        } else {
            p.carpenters
        };
        if n[Species::Carpenter.index()] < carpenter_goal {
            let cost = Species::Carpenter.stats().cost;
            if afford(&stock, &cost) {
                out.push(Command::Train {
                    species: Species::Carpenter,
                    amount: Amount::One,
                });
                spend(&mut stock, &cost);
            }
        }

        // 3. Gather Focus: the resource furthest below a wanted ratio, which
        // leans to Nectar once higher tiers need it.
        let want = if c.tier >= 2 { [3, 2, 1] } else { [3, 1, 1] };
        let short = (0..3).min_by_key(|&i| (stock[i] / want[i], i)).unwrap_or(0);
        let focus = if !blueprints.is_empty() && stock[2] < 60 {
            Focus::Leaves
        } else if stock[short] / want[short] >= 150 {
            Focus::Balanced
        } else {
            [Focus::Food, Focus::Nectar, Focus::Leaves][short]
        };
        if focus != c.focus {
            out.push(Command::Focus { focus });
        }

        let mix = army_mix(w, colony, d);
        let train_army = |stock: &mut [i32; 3], out: &mut Vec<Command>, queen_ok: bool| {
            let total: i32 = mix.iter().map(|(_, wt)| *wt).sum::<i32>().max(1);
            let army_now: i32 = Species::ALL
                .iter()
                .filter(|s| s.is_combat())
                .map(|s| n[s.index()])
                .sum::<i32>()
                .max(1);
            let mut best: Option<(i64, Species)> = None;
            for (s, wt) in &mix {
                // Imago waits for the species it most wants rather than
                // buying whatever is cheapest, so dear ants still get made.
                let skip_unaffordable = d != Difficulty::Imago;
                if w.can_train(colony, *s).is_err()
                    || (skip_unaffordable && !afford(stock, &s.stats().cost))
                    || (s.from_queen() && !queen_ok)
                {
                    continue;
                }
                let gap = (*wt as i64) * army_now as i64 - n[s.index()] as i64 * total as i64;
                if best.map(|b| gap > b.0).unwrap_or(true) {
                    best = Some((gap, *s));
                }
            }
            if let Some((_, s)) = best {
                let cost = s.stats().cost;
                if pop + s.stats().pop <= housing + 8 && afford(stock, &cost) {
                    let ten = d != Difficulty::Larva && (0..3).all(|i| stock[i] >= cost[i] * 5);
                    out.push(Command::Train {
                        species: s,
                        amount: if ten { Amount::Ten } else { Amount::One },
                    });
                    let k = if ten { 5 } else { 1 };
                    for i in 0..3 {
                        stock[i] -= cost[i] * k;
                    }
                }
            }
        };

        // 4. Below the floor, the army comes before anything else, but never
        // crowds Harvester Ants out of a queen's lane.
        let lane_room = w.queens(colony).any(|q| q.lane.len() < 3);
        if !safe && (lane_room || !harvesters_short) {
            train_army(&mut stock, &mut out, true);
        } else {
            // 5. Tech: Evolve, hatcheries, new nests, upgrades. Imago saves
            // up for the next step of its plan instead of spending the
            // difference on more Soldier Ants.
            let mut saving = false;
            if c.tier < 3 && !w.evolving(colony) {
                let after = if c.tier == 1 { p.t2_after } else { p.t3_after };
                let (cost, _) = evolve_cost(c.tier + 1);
                if now >= after {
                    if afford(&stock, &cost) {
                        out.push(Command::Evolve);
                        spend(&mut stock, &cost);
                    } else if d == Difficulty::Imago {
                        saving = true;
                    }
                }
            }
            for (s, _) in mix.iter().filter(|_| !saving) {
                if s.from_queen() || w.has_hatchery(colony, *s) {
                    continue;
                }
                let kind = StructureKind::Hatchery(*s);
                if blueprints.contains(&kind) || c.tier < s.stats().tier {
                    continue;
                }
                let cost = kind.stats().cost;
                if afford(&stock, &cost) {
                    if let Some((x, y)) = find_spot(w, colony, kind, home, 7) {
                        out.push(Command::Place {
                            kind,
                            x,
                            y,
                            x2: x,
                            y2: y,
                        });
                        spend(&mut stock, &cost);
                    }
                } else if d == Difficulty::Imago {
                    saving = true;
                }
                break; // one new hatchery at a time
            }
            if !saving {
                try_new_nest(w, colony, d, home, &mut stock, &mut out);
            }
            if p.upgrades && !saving && d != Difficulty::Imago {
                let wanted = [
                    Upgrade::GatherRate,
                    Upgrade::TrainingRate,
                    Upgrade::Damage,
                    Upgrade::Armor,
                    Upgrade::Health,
                    Upgrade::Species(Species::Acid),
                    Upgrade::Species(Species::Soldier),
                    Upgrade::CarryCapacity,
                ];
                for u in wanted {
                    if let Ok(rank) = w.can_research(colony, u) {
                        let cost = u.cost(rank);
                        if (0..3).all(|i| stock[i] >= cost[i] + 60) {
                            out.push(Command::Research { upgrade: u });
                            spend(&mut stock, &cost);
                            break;
                        }
                    }
                }
            }
            if d == Difficulty::Imago && !saving && hatcheries >= 2 {
                // Combat upgrades for the army it fields, in order; once there
                // is an army to protect the wait, it saves for the next one.
                let wanted = [
                    Upgrade::Species(Species::Turtle),
                    Upgrade::TrainingRate,
                    Upgrade::Armor,
                    Upgrade::Damage,
                    Upgrade::Species(Species::Acid),
                    Upgrade::Health,
                ];
                if let Some((u, rank)) = wanted
                    .into_iter()
                    .find_map(|u| w.can_research(colony, u).ok().map(|r| (u, r)))
                {
                    let cost = u.cost(rank);
                    if afford(&stock, &cost) {
                        out.push(Command::Research { upgrade: u });
                        spend(&mut stock, &cost);
                    } else if army_pop >= 30 {
                        saving = true;
                    }
                }
            }
            // 6. The rest goes to the army.
            if d == Difficulty::Imago {
                if !saving {
                    train_army(&mut stock, &mut out, !harvesters_short);
                }
            } else if lane_room || !harvesters_short {
                train_army(&mut stock, &mut out, true);
            }
        }
    }

    // 7. Army control. With no queen and no hatchery left, the last ants
    // all fight: a lone scout wandering the map would only drag the match
    // out.
    if queens.is_empty() && hatcheries == 0 && c.auto_scout {
        out.push(Command::AutoScout { on: false });
    }
    let seen = enemy_seen(w, colony);
    let has_waypoint = c.waypoints.iter().any(|v| !v.is_empty());
    let clear_all = |out: &mut Vec<Command>| {
        for wps in &c.waypoints {
            for wp in wps {
                let (x, y) = wp.cell();
                out.push(Command::ClearWaypoint { x, y });
            }
        }
    };
    if d == Difficulty::Imago {
        let losing = seen.army_pop > army_pop * 2 && seen.army_pop > 12;
        if losing != c.retreat {
            out.push(Command::Retreat { on: losing });
            if losing {
                return out;
            }
        }
    }
    let attack_pop = p.attack_pop + c.ai.wave.min(4) * 6;
    // An attack keeps its All Ants waypoint on the enemy until the army is
    // broken; then the army regroups at home and the next wave is bigger.
    if c.ai.attacking {
        let home_hit = seen.near_home >= 5 && d != Difficulty::Larva;
        if army_pop < attack_pop / 4 || home_hit {
            clear_all(&mut out);
            out.push(Command::AiState {
                attacking: false,
                wave: c.ai.wave + 1,
            });
        } else if let Some(tp) = attack_point(w, colony) {
            // Follow the nearest enemy structure as targets fall.
            let (x, y) = tp.cell();
            let cur = c.waypoints.iter().find_map(|v| v.first().copied());
            let split = c.waypoints[Species::Fire.index()].len() > 1;
            if split || cur.map(|p| p.dist(tp) > 3 * CELL).unwrap_or(true) {
                out.push(Command::SetWaypoint {
                    species: None,
                    x,
                    y,
                    add: false,
                });
            }
        } else {
            out.extend(sweep(w, colony, home));
        }
        return out;
    }
    if seen.near_home >= 3 && d != Difficulty::Larva {
        if let (Difficulty::Imago, Some(tp)) = (d, seen.intruder) {
            // Defending: the whole army meets the intruders together,
            // before they reach the Harvester Ants.
            let cur = c.waypoints.iter().find_map(|v| v.first().copied());
            if cur.map(|p| p.dist(tp) > 6 * CELL).unwrap_or(true) {
                let (x, y) = tp.cell();
                out.push(Command::SetWaypoint {
                    species: None,
                    x,
                    y,
                    add: false,
                });
            }
        } else if has_waypoint {
            // Defending: everyone waits at the nest.
            clear_all(&mut out);
        }
        return out;
    }
    let all_ants_wp = c.waypoints[Species::Fire.index()].first().copied();
    let defence_wp = all_ants_wp
        .map(|p| queens.iter().any(|q| q.dist(p) < 26 * CELL))
        .unwrap_or(false);
    if d == Difficulty::Imago && defence_wp && seen.near_home == 0 {
        // The defence is over: back to the nest.
        clear_all(&mut out);
        return out;
    }
    let late = if d == Difficulty::Imago {
        (3200, 20)
    } else {
        (3000, 20)
    };
    // Late in a match the army needed to attack shrinks, so that a match
    // where both sides are out of Food still ends. With no queen and no
    // hatchery left there is nothing to defend and
    // nothing to wait for: whatever army is left goes hunting.
    let nothing_to_lose = queens.is_empty() && hatcheries == 0;
    if army_pop >= attack_pop
        || (now > late.0 && army_pop >= (late.1 - (now - late.0) / 300).max(6))
        || (nothing_to_lose && army_pop > 0)
    {
        match attack_point(w, colony) {
            Some(tp) => {
                let (x, y) = tp.cell();
                out.push(Command::SetWaypoint {
                    species: None,
                    x,
                    y,
                    add: false,
                });
            }
            None => out.extend(sweep(w, colony, home)),
        }
        out.push(Command::AiState {
            attacking: true,
            wave: c.ai.wave,
        });
        return out;
    }
    // Scouting while building up: Soldier Ants look at the nearest chamber
    // the team has not seen, so chambers for new nests can be found. Imago moves on to
    // the next one once a chamber has been seen, so it learns where every
    // Crumb Pile is before its own run out.
    let explored = w.team_explored(colony);
    let scout_wp = c.waypoints[Species::Soldier.index()].first().copied();
    let scouted = scout_wp
        .map(|p| {
            let (x, y) = p.cell();
            all_ants_wp.is_none() && explored.get(w.map.idx(x, y))
        })
        .unwrap_or(false);
    let rescout = d == Difficulty::Imago && scouted;
    if p.nests > 0 && (!has_waypoint || rescout) && now > 250 {
        let target = w
            .map
            .chambers
            .iter()
            .filter(|ch| ch.home.is_none() && !explored.get(w.map.idx(ch.cx, ch.cy)))
            .filter(|ch| d != Difficulty::Imago || on_our_side(w, colony, ch.cx, ch.cy))
            .min_by_key(|ch| (Pos::of_cell(ch.cx, ch.cy).dist2(home), ch.cx, ch.cy));
        if let Some(ch) = target {
            if let (true, Some(sp)) = (rescout, scout_wp) {
                let (x, y) = sp.cell();
                out.push(Command::ClearWaypoint { x, y });
            }
            out.push(Command::SetWaypoint {
                species: Some(Species::Soldier),
                x: ch.cx,
                y: ch.cy,
                add: false,
            });
        }
    } else if let Some(h) = &w.events.hill {
        if d == Difficulty::Imago && !has_waypoint && army_pop >= 10 {
            out.push(Command::SetWaypoint {
                species: None,
                x: h.x,
                y: h.y,
                add: false,
            });
        }
    }
    out
}

/// Places a queen blueprint for a new nest when the plan wants one. Returns
/// true when one is wanted now but the stockpile cannot pay for it yet.
fn try_new_nest(
    w: &World,
    colony: u8,
    d: Difficulty,
    home: Pos,
    stock: &mut [i32; 3],
    out: &mut Vec<Command>,
) -> bool {
    let p = plan(d);
    let queens = w.queens(colony).count() as i32;
    let pending = w
        .structures
        .iter()
        .filter(|s| s.colony == colony && !s.complete && s.kind == StructureKind::Queen)
        .count() as i32;
    let have = queens - 1 + pending;
    let hatcheries = w
        .structures
        .iter()
        .filter(|s| {
            s.colony == colony && s.complete && matches!(s.kind, StructureKind::Hatchery(_))
        })
        .count();
    if d == Difficulty::Imago && hatcheries < 2 {
        return false;
    }
    let nest_at = match d {
        Difficulty::Imago => 1800 + have * 900,
        _ => 1600,
    };
    if have >= p.nests || tenths(w) < nest_at || pending > 0 {
        return false;
    }
    let Some((x, y)) = nest_spot(w, colony, home, d == Difficulty::Imago) else {
        return false;
    };
    let cost = StructureKind::Queen.stats().cost;
    if !(0..3).all(|i| stock[i] >= cost[i]) {
        return true;
    }
    out.push(Command::Place {
        kind: StructureKind::Queen,
        x,
        y,
        x2: x,
        y2: y,
    });
    for i in 0..3 {
        stock[i] -= cost[i];
    }
    false
}

/// The nearest free chamber with nodes, for a new queen; with `food_first`,
/// the nearest with a Crumb Pile when one is known.
fn nest_spot(w: &World, colony: u8, home: Pos, food_first: bool) -> Option<(i32, i32)> {
    if food_first {
        if let Some(spot) = nest_spot_where(w, colony, home, |r| r == Resource::Food) {
            return Some(spot);
        }
    }
    nest_spot_where(w, colony, home, |_| true)
}

fn nest_spot_where(
    w: &World,
    colony: u8,
    home: Pos,
    wanted: impl Fn(Resource) -> bool,
) -> Option<(i32, i32)> {
    let explored = w.team_explored(colony);
    let mut chambers: Vec<(i64, usize)> = w
        .map
        .chambers
        .iter()
        .enumerate()
        .filter(|(_, c)| c.home.is_none() && on_our_side(w, colony, c.cx, c.cy))
        .map(|(i, c)| (Pos::of_cell(c.cx, c.cy).dist2(home), i))
        .collect();
    chambers.sort();
    for (_, i) in chambers.into_iter().take(4) {
        let c = &w.map.chambers[i];
        if !explored.get(w.map.idx(c.cx, c.cy)) {
            continue;
        }
        let rich = w.nodes.iter().any(|n| {
            n.amount > 400
                && wanted(n.resource)
                && n.pos.dist(Pos::of_cell(c.cx, c.cy)) < (c.r + 4) * CELL
        });
        if !rich {
            continue;
        }
        if let Some(spot) = find_spot(w, colony, StructureKind::Queen, Pos::of_cell(c.cx, c.cy), 0)
        {
            return Some(spot);
        }
    }
    None
}

/// How many Harvester Ants the colony's nests can use: a full complement on
/// every node, still worth gathering, near one of its queens.
fn node_room(w: &World, colony: u8) -> i32 {
    let explored = w.team_explored(colony);
    let queens: Vec<Pos> = w.queens(colony).map(|q| q.pos).collect();
    w.nodes
        .iter()
        .filter(|n| {
            n.amount > 200
                && {
                    let (x, y) = n.pos.cell();
                    explored.get(w.map.idx(x, y))
                }
                && queens.iter().any(|q| q.dist(n.pos) < 22 * CELL)
        })
        .map(|n| {
            // Food is spent on everything, so its nodes get a full
            // complement; Nectar feeds the Tier 2 army; Leaves need few.
            match n.resource {
                Resource::Food => crate::rules::SATURATION as i32,
                Resource::Nectar => 5,
                Resource::Leaves => 3,
            }
        })
        .sum()
}

/// A cell nearer the colony's home chamber than any enemy's: every player
/// knows where the home chambers are from the map's symmetry.
fn on_our_side(w: &World, colony: u8, x: i32, y: i32) -> bool {
    let p = Pos::of_cell(x, y);
    let mine = w.map.home(colony as usize);
    let d = p.dist2(Pos::of_cell(mine.cx, mine.cy));
    w.colonies
        .iter()
        .filter(|c| !w.allied(c.seat, colony))
        .all(|c| {
            let h = w.map.home(c.seat as usize);
            d <= p.dist2(Pos::of_cell(h.cx, h.cy))
        })
}
