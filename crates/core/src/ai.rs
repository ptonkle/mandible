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

/// Where to attack: the nearest structure of the nearest living enemy, or its
/// home chamber.
fn attack_point(w: &World, colony: u8) -> Option<Pos> {
    let from = w.nearest_queen(colony, Pos::default())?.pos;
    let explored = w.team_explored(colony);
    let mut best: Option<(i64, Pos)> = None;
    for s in &w.structures {
        if w.allied(s.colony, colony) || w.colonies[s.colony as usize].defeated {
            continue;
        }
        let (x, y) = s.pos.cell();
        if !explored.get(w.map.idx(x, y)) {
            continue;
        }
        let d = s.pos.dist2(from);
        if best.map(|b| d < b.0).unwrap_or(true) {
            best = Some((d, s.pos));
        }
    }
    if let Some((_, p)) = best {
        return Some(p);
    }
    w.colonies
        .iter()
        .filter(|c| !c.defeated && c.team != w.colonies[colony as usize].team)
        .map(|c| {
            let h = w.map.home(c.seat as usize);
            Pos::of_cell(h.cx, h.cy)
        })
        .min_by_key(|p| p.dist2(from))
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

    // Housing first: never stall on Housing Full.
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

    // Workers.
    let harvester_goal = p.harvesters + (queens.len() as i32 - 1) * 8;
    let workers_short = n[Species::Harvester.index()] < harvester_goal;
    if workers_short {
        let cost = Species::Harvester.stats().cost;
        let lanes_busy = w.queens(colony).filter(|q| q.lane.len() >= 2).count() == queens.len();
        if afford(&stock, &cost) && !lanes_busy {
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

    // Gather Focus follows what is short.
    let focus = if !blueprints.is_empty() && stock[2] < 80 {
        Focus::Leaves
    } else if c.tier >= 2 && stock[1] < 100 {
        Focus::Nectar
    } else {
        Focus::Balanced
    };
    if focus != c.focus {
        out.push(Command::Focus { focus });
    }

    // Evolve.
    if c.tier < 3 && !w.evolving(colony) {
        let after = if c.tier == 1 { p.t2_after } else { p.t3_after };
        let (cost, _) = evolve_cost(c.tier + 1);
        if w.tick >= after
            && n[Species::Harvester.index()] * 10 >= harvester_goal * 7
            && afford(&stock, &cost)
        {
            out.push(Command::Evolve);
            spend(&mut stock, &cost);
        }
    }

    // Expansions.
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

    // Hatcheries for the army mix.
    let mix = army_mix(w, colony, d);
    for (s, _) in &mix {
        if s.from_queen() || w.has_hatchery(colony, *s) {
            continue;
        }
        let kind = StructureKind::Hatchery(*s);
        if blueprints.contains(&kind) {
            continue;
        }
        let cost = kind.stats().cost;
        if afford(&stock, &cost) && c.tier >= s.stats().tier {
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

    // Upgrades.
    if p.upgrades && n.iter().sum::<i32>() > 18 {
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
                // Keep a reserve for the army.
                if (0..3).all(|i| stock[i] >= cost[i] + 100) {
                    out.push(Command::Research { upgrade: u });
                    spend(&mut stock, &cost);
                    break;
                }
            }
        }
    }

    // Army: spend what is left by the mix weights.
    if !workers_short || stock[0] > 300 {
        let total: i32 = mix.iter().map(|(_, wt)| *wt).sum::<i32>().max(1);
        let army_now: i32 = Species::ALL
            .iter()
            .filter(|s| s.is_combat())
            .map(|s| n[s.index()])
            .sum::<i32>()
            .max(1);
        // The species furthest below its share.
        let mut best: Option<(i64, Species)> = None;
        for (s, wt) in &mix {
            if w.can_train(colony, *s).is_err() {
                continue;
            }
            let want = (*wt as i64) * army_now as i64;
            let have = n[s.index()] as i64 * total as i64;
            let gap = want - have;
            if best.map(|b| gap > b.0).unwrap_or(true) {
                best = Some((gap, *s));
            }
        }
        if let Some((_, s)) = best {
            let cost = s.stats().cost;
            if afford(&stock, &cost) && pop + s.stats().pop <= housing + 10 {
                let amount = if d == Difficulty::Larva {
                    Amount::One
                } else if (0..3).all(|i| stock[i] >= cost[i] * 4) {
                    Amount::Ten
                } else {
                    Amount::One
                };
                out.push(Command::Train { species: s, amount });
            }
        }
    }

    // Army control.
    let army_pop: i32 = w
        .ants
        .iter()
        .filter(|a| {
            a.colony == colony && a.species.stats().role != Role::Gatherer && a.species.is_combat()
        })
        .map(|a| a.species.stats().pop)
        .sum();
    let seen = enemy_seen(w, colony);
    let has_waypoint = c.waypoints.iter().any(|v| !v.is_empty());
    if d == Difficulty::Imago {
        let losing = seen.army_pop > army_pop * 5 / 3 && seen.army_pop > 10;
        if losing != c.retreat {
            out.push(Command::Retreat { on: losing });
            if losing {
                return out;
            }
        }
    }
    if seen.near_home >= 3 && d != Difficulty::Larva {
        // Defend: everyone to the nest under attack.
        if has_waypoint {
            for wps in &c.waypoints {
                for wp in wps {
                    let (x, y) = wp.cell();
                    out.push(Command::ClearWaypoint { x, y });
                }
            }
        }
        return out;
    }
    // Scouting: while there is no attack, Soldier Ants look at the nearest
    // chamber nobody on the team has seen, so expansions can be found.
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
    }
    let attack_pop = p.attack_pop + c.ai.wave * 6;
    if army_pop >= attack_pop || (w.tick > 2400 && army_pop >= 12) {
        if let Some(tp) = attack_point(w, colony) {
            let (x, y) = tp.cell();
            let current = c
                .waypoints
                .iter()
                .find_map(|v| v.first().copied())
                .map(|p| p.cell());
            let near = current
                .map(|(cx, cy)| (cx - x).abs() + (cy - y).abs() <= 6)
                .unwrap_or(false);
            if !near {
                out.push(Command::SetWaypoint {
                    species: None,
                    x,
                    y,
                });
            }
        }
    } else if army_pop < attack_pop / 3 && has_waypoint {
        for wps in &c.waypoints {
            for wp in wps {
                let (x, y) = wp.cell();
                out.push(Command::ClearWaypoint { x, y });
            }
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
