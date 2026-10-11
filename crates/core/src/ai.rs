//! The AI players. They run inside the simulation on every peer, so they send
//! nothing over the network, and they issue the same commands a player does.
//! They see only what their team sees, plus the home chambers, which every
//! player knows from the map's symmetry.
//!
//! Difficulty shapes the plan: Larva decides slowly, keeps one nest, skips
//! upgrades and attacks late; Pupa follows a standard plan with one
//! expansion and upgrades; Imago decides often, builds a bigger economy,
//! expands twice, counters what it has seen, defends, and retreats from
//! fights it is losing.

use serde::{Deserialize, Serialize};

use crate::fixed::{Pos, CELL};
use crate::rules::{evolve_cost, Role, Species, StructureKind, Upgrade, POP_CAP};
use crate::settings::Difficulty;
use crate::world::{Amount, Command, Focus, Work, World};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AiState {
    pub attacking: bool,
    pub wave: i32,
    pub retreat_until: i32,
}

pub fn interval(d: Difficulty) -> i32 {
    match d {
        Difficulty::Larva => 30,
        Difficulty::Pupa => 14,
        Difficulty::Imago => 6,
    }
}

pub fn due(tick: i32, colony: usize, d: Difficulty) -> bool {
    (tick + colony as i32 * 3) % interval(d) == 0
}

struct Plan {
    harvesters: i32,
    carpenters: i32,
    expansions: i32,
    upgrades: bool,
    attack_pop: i32,
    t2_after: i32,
    t3_after: i32,
}

fn plan(d: Difficulty) -> Plan {
    match d {
        Difficulty::Larva => Plan {
            harvesters: 9,
            carpenters: 2,
            expansions: 0,
            upgrades: false,
            attack_pop: 30,
            t2_after: 2000,
            t3_after: i32::MAX,
        },
        Difficulty::Pupa => Plan {
            harvesters: 16,
            carpenters: 3,
            expansions: 1,
            upgrades: true,
            attack_pop: 34,
            t2_after: 900,
            t3_after: 1900,
        },
        Difficulty::Imago => Plan {
            harvesters: 22,
            carpenters: 4,
            expansions: 2,
            upgrades: true,
            attack_pop: 30,
            t2_after: 600,
            t3_after: 1400,
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
    let mut mix = vec![(Species::Soldier, 4)];
    if tier >= 2 {
        match d {
            Difficulty::Larva => mix.push((Species::Fire, 3)),
            _ => {
                mix.push((Species::Acid, 4));
                mix.push((Species::Turtle, 2));
                mix.push((Species::Nurse, 1));
                mix.push((Species::Fire, 2));
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
}

fn enemy_seen(w: &World, colony: u8) -> Seen {
    let vis = w.team_visible(colony);
    let mut s = Seen::default();
    let home = w.nearest_queen(colony, Pos::default()).map(|q| q.pos);
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
                if w.queens(colony).any(|q| q.pos.dist(a.pos) < 24 * CELL)
                    || a.pos.dist(h) < 24 * CELL
                {
                    s.near_home += 1;
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
    s
}

/// Where to attack: the nearest known structure of a living enemy; failing
/// that, the nearest enemy ants in sight; failing that, the nearest enemy
/// home chamber that still has a queen standing, else our own home (where
/// an enemy army with nothing left to destroy is likely heading).
fn attack_point(w: &World, colony: u8) -> Option<Pos> {
    let from = w
        .nearest_queen(colony, Pos::default())
        .map(|q| q.pos)
        .or_else(|| {
            let h = w.map.home(colony as usize);
            Some(Pos::of_cell(h.cx, h.cy))
        })?;
    let explored = w.team_explored(colony);
    let visible = w.team_visible(colony);
    let enemy = |c: u8| !w.allied(c, colony) && !w.colonies[c as usize].defeated;
    let nearest = |ps: Vec<Pos>| ps.into_iter().min_by_key(|p| (p.dist2(from), p.x, p.y));
    let known: Vec<Pos> = w
        .structures
        .iter()
        .filter(|s| {
            enemy(s.colony) && {
                let (x, y) = s.pos.cell();
                explored.get(w.map.idx(x, y))
            }
        })
        .map(|s| s.pos)
        .collect();
    if let Some(p) = nearest(known) {
        return Some(p);
    }
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
    if let Some(p) = nearest(seen) {
        return Some(p);
    }
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
    // Hunting: an enemy is alive but nowhere known. Search chambers nobody
    // on the team has seen, nearest first; then sweep the chambers out of
    // sight, one after another.
    let my_team = w.colonies[colony as usize].team;
    let unseen: Vec<Pos> = w
        .map
        .chambers
        .iter()
        .filter(|ch| {
            ch.home
                .map(|h| w.colonies[h as usize].team != my_team)
                .unwrap_or(true)
        })
        .filter(|ch| !explored.get(w.map.idx(ch.cx, ch.cy)))
        .map(|ch| Pos::of_cell(ch.cx, ch.cy))
        .collect();
    if let Some(p) = nearest(unseen) {
        return Some(p);
    }
    let dark: Vec<Pos> = w
        .map
        .chambers
        .iter()
        .filter(|ch| !visible.get(w.map.idx(ch.cx, ch.cy)))
        .map(|ch| Pos::of_cell(ch.cx, ch.cy))
        .collect();
    if dark.is_empty() {
        return Some(from);
    }
    Some(dark[(w.tick / 300) as usize % dark.len()])
}

pub fn decide(w: &World, colony: u8, d: Difficulty) -> Vec<Command> {
    let mut out = Vec::new();
    let ci = colony as usize;
    let c = &w.colonies[ci];
    let p = plan(d);
    let n = counts(w, colony);
    let queens: Vec<Pos> = w.queens(colony).map(|q| q.pos).collect();
    let Some(&home) = queens.first() else {
        return out;
    };
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
    // The army a difficulty keeps at home before spending on anything else.
    let floor = match d {
        Difficulty::Larva => 0,
        Difficulty::Pupa => w.tick / 100,
        Difficulty::Imago => w.tick / 90,
    }
    .min(24);
    let safe = army_pop >= floor;

    // 1. Housing: never stall on Housing Full.
    let pending_housing = blueprints
        .iter()
        .filter(|k| matches!(k, StructureKind::BroodCell | StructureKind::Queen))
        .count() as i32;
    if housing < POP_CAP && pop + 6 >= housing + pending_housing * 15 {
        let cost = StructureKind::BroodCell.stats().cost;
        if afford(&stock, &cost) {
            if let Some((x, y)) = find_spot(w, colony, StructureKind::BroodCell, home, 6) {
                out.push(Command::Place {
                    kind: StructureKind::BroodCell,
                    x,
                    y,
                    x2: x,
                    y2: y,
                });
                spend(&mut stock, &cost);
            }
        }
    }

    // 2. Workers come first: one queued per queen while short.
    let harvester_goal = p.harvesters + (queens.len() as i32 - 1) * 8;
    let queued_harvesters = w
        .queens(colony)
        .flat_map(|q| q.lane.iter())
        .filter(|q| matches!(q.work, Work::Train(Species::Harvester)))
        .count();
    let workers_short = n[Species::Harvester.index()] < harvester_goal;
    if workers_short && queued_harvesters < queens.len() {
        let cost = Species::Harvester.stats().cost;
        if afford(&stock, &cost) {
            out.push(Command::Train {
                species: Species::Harvester,
                amount: Amount::One,
            });
            spend(&mut stock, &cost);
        }
    }
    if n[Species::Carpenter.index()] < p.carpenters {
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
    let mut train_army = |stock: &mut [i32; 3], out: &mut Vec<Command>| {
        let total: i32 = mix.iter().map(|(_, wt)| *wt).sum::<i32>().max(1);
        let army_now: i32 = Species::ALL
            .iter()
            .filter(|s| s.is_combat())
            .map(|s| n[s.index()])
            .sum::<i32>()
            .max(1);
        let mut best: Option<(i64, Species)> = None;
        for (s, wt) in &mix {
            if w.can_train(colony, *s).is_err() || !afford(stock, &s.stats().cost) {
                continue;
            }
            let gap = (*wt as i64) * army_now as i64 - n[s.index()] as i64 * total as i64;
            if best.map(|b| gap > b.0).unwrap_or(true) {
                best = Some((gap, *s));
            }
        }
        if let Some((_, s)) = best {
            let cost = s.stats().cost;
            if pop + s.stats().pop <= housing + 8 {
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
    if !safe && (lane_room || !workers_short) {
        train_army(&mut stock, &mut out);
    } else {
        // 5. Tech: Evolve, hatcheries, expansions, upgrades.
        if c.tier < 3 && !w.evolving(colony) {
            let after = if c.tier == 1 { p.t2_after } else { p.t3_after };
            let (cost, _) = evolve_cost(c.tier + 1);
            if w.tick >= after && afford(&stock, &cost) {
                out.push(Command::Evolve);
                spend(&mut stock, &cost);
            }
        }
        for (s, _) in &mix {
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
            }
            break; // one new hatchery at a time
        }
        let have_expansions = queens.len() as i32 - 1
            + blueprints
                .iter()
                .filter(|k| **k == StructureKind::Queen)
                .count() as i32;
        let expand_at = match d {
            Difficulty::Imago => 1100 + have_expansions * 1000,
            _ => 1600,
        };
        if have_expansions < p.expansions && w.tick >= expand_at {
            let cost = StructureKind::Queen.stats().cost;
            if afford(&stock, &cost) {
                if let Some((x, y)) = expansion_spot(w, colony, home) {
                    out.push(Command::Place {
                        kind: StructureKind::Queen,
                        x,
                        y,
                        x2: x,
                        y2: y,
                    });
                    spend(&mut stock, &cost);
                }
            }
        }
        if p.upgrades {
            let wanted = [
                Upgrade::GatherRate,
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
        // 6. The rest goes to the army.
        if lane_room || !workers_short {
            train_army(&mut stock, &mut out);
        }
    }

    // 7. Army control.
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
            if cur.map(|p| p.dist(tp) > 8 * CELL).unwrap_or(true) {
                out.push(Command::SetWaypoint {
                    species: None,
                    x,
                    y,
                });
            }
        }
        return out;
    }
    if seen.near_home >= 3 && d != Difficulty::Larva {
        // Defending: everyone waits at the nest.
        if has_waypoint {
            clear_all(&mut out);
        }
        return out;
    }
    if army_pop >= attack_pop || (w.tick > 2400 && army_pop >= 16) {
        if let Some(tp) = attack_point(w, colony) {
            let (x, y) = tp.cell();
            out.push(Command::SetWaypoint {
                species: None,
                x,
                y,
            });
            out.push(Command::AiState {
                attacking: true,
                wave: c.ai.wave,
            });
        }
        return out;
    }
    // Scouting while building up: Soldier Ants look at the nearest chamber
    // the team has not seen, so expansions can be found.
    if p.expansions > 0 && !has_waypoint && w.tick > 250 {
        let explored = w.team_explored(colony);
        let target = w
            .map
            .chambers
            .iter()
            .filter(|ch| ch.home.is_none() && !explored.get(w.map.idx(ch.cx, ch.cy)))
            .min_by_key(|ch| (Pos::of_cell(ch.cx, ch.cy).dist2(home), ch.cx, ch.cy));
        if let Some(ch) = target {
            out.push(Command::SetWaypoint {
                species: Some(Species::Soldier),
                x: ch.cx,
                y: ch.cy,
            });
        }
    } else if let Some(h) = &w.events.hill {
        if d == Difficulty::Imago && !has_waypoint && army_pop >= 10 {
            out.push(Command::SetWaypoint {
                species: None,
                x: h.x,
                y: h.y,
            });
        }
    }
    out
}

/// The nearest free chamber with nodes, for a new queen.
fn expansion_spot(w: &World, colony: u8, home: Pos) -> Option<(i32, i32)> {
    let explored = w.team_explored(colony);
    let mut chambers: Vec<(i64, usize)> = w
        .map
        .chambers
        .iter()
        .enumerate()
        .filter(|(_, c)| c.home.is_none())
        .map(|(i, c)| (Pos::of_cell(c.cx, c.cy).dist2(home), i))
        .collect();
    chambers.sort();
    for (_, i) in chambers.into_iter().take(4) {
        let c = &w.map.chambers[i];
        if !explored.get(w.map.idx(c.cx, c.cy)) {
            continue;
        }
        let rich = w
            .nodes
            .iter()
            .any(|n| n.amount > 0 && n.pos.dist(Pos::of_cell(c.cx, c.cy)) < (c.r + 4) * CELL);
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
