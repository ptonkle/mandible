//! AI difficulty ladder (PLAN decision 16): in seeded headless 1v1 matches,
//! Imago beats Pupa and Pupa beats Larva most of the time. Slow, so it is
//! ignored by default; run it with
//!
//!     cargo test --release -p tool_core --test ai -- --ignored --nocapture
//!
//! MAP_TYPE=open or MAP_TYPE=maze runs every test here on that Map Type
//! (default Tunnels).

use tool_core::rules::TICKS_PER_SECOND;
use tool_core::settings::{Difficulty, MapType, MatchSettings, SeatKind};
use tool_core::world::World;

/// The default Match Settings, with the Map Type from MAP_TYPE.
fn settings() -> MatchSettings {
    let map_type = match std::env::var("MAP_TYPE").as_deref() {
        Ok("open") => MapType::Open,
        Ok("maze") => MapType::Maze,
        _ => MapType::Tunnels,
    };
    MatchSettings {
        map_type,
        ..MatchSettings::default()
    }
}

/// 20 minutes of game time: no match should get near it.
const LIMIT: i32 = 20 * 60 * TICKS_PER_SECOND;
/// Matches are designed to last about 5 minutes; half of them must be over
/// within 10.
const MEDIAN_LIMIT: i32 = 10 * 60 * TICKS_PER_SECOND;

/// Plays one match; returns the winning seat (or None) and the tick it ended.
fn play(seed: u64, a: Difficulty, b: Difficulty) -> (Option<u8>, i32) {
    let mut w = World::new(seed, settings(), &[SeatKind::Ai(a), SeatKind::Ai(b)]);
    while w.winner.is_none() && w.tick < LIMIT {
        w.step(&[], &[]);
    }
    (w.winner, w.tick)
}

fn ladder(strong: Difficulty, weak: Difficulty) -> (i32, i32, Vec<i32>) {
    let mut wins = 0;
    let mut games = 0;
    let mut lengths = Vec::new();
    // SEED0 shifts the seeds, to check the ladder on maps it was not tuned on.
    let first: u64 = std::env::var("SEED0")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    for seed in first..first + 10 {
        // Alternate seats so neither side always has seat 0.
        let (winner, t) = if seed % 2 == 0 {
            let (w, t) = play(seed, strong, weak);
            (w.map(|s| s == 0), t)
        } else {
            let (w, t) = play(seed, weak, strong);
            (w.map(|s| s == 1), t)
        };
        games += 1;
        if winner == Some(true) {
            wins += 1;
        }
        // Lengths print negative for the matches the stronger side lost.
        lengths.push(if winner == Some(true) { t } else { -t });
    }
    (wins, games, lengths)
}

#[test]
#[ignore]
fn imago_beats_pupa_and_pupa_beats_larva() {
    eprintln!("AI: Map Type {:?}", settings().map_type);
    let (w1, g1, l1) = ladder(Difficulty::Imago, Difficulty::Pupa);
    eprintln!("AI: Imago beat Pupa {w1}/{g1}, lengths {l1:?}");
    let (w2, g2, l2) = ladder(Difficulty::Pupa, Difficulty::Larva);
    eprintln!("AI: Pupa beat Larva {w2}/{g2}, lengths {l2:?}");
    for (name, l) in [("Imago v Pupa", &l1), ("Pupa v Larva", &l2)] {
        let mut t: Vec<i32> = l.iter().map(|x| x.abs()).collect();
        t.sort();
        assert!(
            t.iter().all(|&x| x < LIMIT),
            "{name}: a match reached the limit: {l:?}"
        );
        let median = t[t.len() / 2];
        eprintln!(
            "AI: {name} median {median} ticks = {}:{:02}",
            median / TICKS_PER_SECOND / 60,
            median / TICKS_PER_SECOND % 60
        );
        assert!(
            t[t.len() / 2] <= MEDIAN_LIMIT,
            "{name}: median length {} over {MEDIAN_LIMIT}",
            t[t.len() / 2]
        );
    }
    assert!(w1 >= 7, "Imago won only {w1} of {g1}");
    assert!(w2 >= 7, "Pupa won only {w2} of {g2}");
}

#[test]
#[ignore]
fn trace() {
    let diff = |k: &str| match std::env::var(k).as_deref() {
        Ok("larva") => Difficulty::Larva,
        Ok("imago") => Difficulty::Imago,
        _ => Difficulty::Pupa,
    };
    use tool_core::rules::{Species, StructureKind};
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let every: i32 = std::env::var("EVERY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60 * TICKS_PER_SECOND);
    let mut w = World::new(
        seed,
        settings(),
        &[SeatKind::Ai(diff("A")), SeatKind::Ai(diff("B"))],
    );
    // DEBUG=from-to prints seat 0's AI commands in that tick range.
    let debug: Option<(i32, i32)> = std::env::var("DEBUG").ok().and_then(|s| {
        let (a, b) = s.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });
    while w.winner.is_none() && w.tick < 15 * 60 * TICKS_PER_SECOND {
        if let Some((from, to)) = debug {
            let d = diff("A");
            if (from..to).contains(&(w.tick + 1)) && tool_core::ai::due(w.tick + 1, 0, d) {
                let cmds = tool_core::ai::decide(&w, 0, d);
                if !cmds.is_empty() {
                    eprintln!("d{} {:?} {:?}", w.tick + 1, w.colonies[0].stock, cmds);
                }
            }
        }
        w.step(&[], &[]);
        if w.tick % every == 0 {
            for c in 0..2u8 {
                let col = &w.colonies[c as usize];
                let n = |s: Species| {
                    w.ants
                        .iter()
                        .filter(|a| a.colony == c && a.species == s)
                        .count()
                };
                let army: i32 = w
                    .ants
                    .iter()
                    .filter(|a| a.colony == c && a.species.is_combat())
                    .map(|a| a.species.stats().pop)
                    .sum();
                let st: Vec<String> = w
                    .structures
                    .iter()
                    .filter(|s| s.colony == c && s.kind != StructureKind::Wall)
                    .map(|s| format!("{:?}{}", s.kind, if s.complete { "" } else { "*" }))
                    .collect();
                let wp = col.waypoints.iter().filter(|v| !v.is_empty()).count();
                let mut comp = std::collections::BTreeMap::new();
                for a in w
                    .ants
                    .iter()
                    .filter(|a| a.colony == c && a.species.is_combat())
                {
                    *comp.entry(a.species.key()).or_insert(0) += 1;
                }
                let wpos: Vec<(i32, i32)> = col
                    .waypoints
                    .iter()
                    .filter_map(|v| v.first())
                    .map(|p| p.cell())
                    .take(1)
                    .collect();
                eprintln!(
                    "t{} c{} T{} stock{:?} harv{} army{} pop{}/{} wp{}{:?} {:?} {:?} k{} l{} g{:?}",
                    w.tick,
                    c,
                    col.tier,
                    col.stock,
                    n(Species::Harvester),
                    army,
                    w.population(c),
                    w.housing(c),
                    wp,
                    wpos,
                    comp,
                    st,
                    col.tally.killed,
                    col.tally.lost,
                    col.tally.gathered
                );
            }
        }
    }
    eprintln!("winner {:?} at {}", w.winner, w.tick);
    if w.winner.is_none() {
        for s in w
            .structures
            .iter()
            .filter(|s| s.kind != StructureKind::Wall)
        {
            let (x, y) = s.pos.cell();
            eprintln!(
                "left c{} {:?} at {:?} complete {} chamber {:?}",
                s.colony,
                s.kind,
                (x, y),
                s.complete,
                w.map.chamber_at(x, y)
            );
        }
        for a in &w.ants {
            let (x, y) = a.pos.cell();
            let ch = w.map.chamber_at(x, y);
            eprintln!(
                "left c{} {:?} at {:?} chamber {:?}",
                a.colony,
                a.species,
                (x, y),
                ch.map(|i| (w.map.chambers[i].cx, w.map.chambers[i].cy))
            );
        }
    }
}

#[test]
#[ignore]
fn trace_gather() {
    use tool_core::rules::Species;
    use tool_core::world::Job;
    let mut w = World::new(0, settings(), &[SeatKind::Human, SeatKind::Human]);
    for t in [5, 50, 150, 300] {
        while w.tick < t {
            w.step(&[], &[]);
        }
        let jobs: Vec<String> = w
            .ants
            .iter()
            .filter(|a| a.colony == 0 && a.species == Species::Harvester)
            .map(|a| match a.job {
                Job::Gather(n) => {
                    let nd = w.nodes.iter().find(|x| x.id == n).unwrap();
                    format!(
                        "G{:?}d{}inv{:?}",
                        nd.resource,
                        a.pos.dist(nd.pos) / 256,
                        a.inventory
                    )
                }
                j => format!("{j:?}inv{:?}", a.inventory),
            })
            .collect();
        eprintln!("t{t} {jobs:?} stock {:?}", w.colonies[0].stock);
    }
    let q = w.queens(0).next().unwrap().pos;
    for n in w.nodes.iter().take(3) {
        eprintln!(
            "node {} {:?} dist-to-queen {} amount {}",
            n.id,
            n.resource,
            n.pos.dist(q) / 256,
            n.amount
        );
    }
}

/// A fight in the centre chamber between two armies given as
/// "species:count,..." in env X and Y; prints what is left.
#[test]
#[ignore]
fn duel() {
    use tool_core::fixed::Pos;
    use tool_core::rules::Species;
    use tool_core::world::Command;
    let parse = |k: &str| -> Vec<(Species, i32)> {
        std::env::var(k)
            .unwrap_or_default()
            .split(',')
            .filter_map(|p| {
                let (s, n) = p.split_once(':')?;
                Some((Species::from_key(s)?, n.parse().ok()?))
            })
            .collect()
    };
    let mut w = World::new(0, settings(), &[SeatKind::Human, SeatKind::Human]);
    let ch = w.map.chambers[w.map.centre].clone();
    let mut ids: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for (colony, key, dx) in [(0u8, "X", -5), (1u8, "Y", 5)] {
        for (s, n) in parse(key) {
            for _ in 0..n {
                let id = w.spawn_ant(colony, s, Pos::of_cell(ch.cx + dx, ch.cy));
                ids[colony as usize].push(id);
            }
        }
    }
    let alive = |w: &World, c: usize| -> (usize, i32) {
        let v: Vec<_> = w.ants.iter().filter(|a| ids[c].contains(&a.id)).collect();
        (v.len(), v.iter().map(|a| a.health).sum())
    };
    let charge = |colony: u8, dx: i32| {
        (
            colony,
            Command::SetWaypoint {
                species: None,
                x: ch.cx + dx,
                y: ch.cy,
                add: false,
            },
        )
    };
    w.step(&[charge(0, 5), charge(1, -5)], &[]);
    while w.tick < 150 * TICKS_PER_SECOND && alive(&w, 0).0 > 0 && alive(&w, 1).0 > 0 {
        w.step(&[], &[]);
    }
    eprintln!("t{} X {:?} Y {:?}", w.tick, alive(&w, 0), alive(&w, 1));
}

/// Lists the nodes of a seed's 1v1 map with their distance from seat 0's
/// home chamber.
#[test]
#[ignore]
fn map_nodes() {
    use tool_core::fixed::Pos;
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let w = World::new(seed, settings(), &[SeatKind::Human, SeatKind::Human]);
    let h = w.map.home(0);
    let hp = Pos::of_cell(h.cx, h.cy);
    eprintln!("map {}x{} home0 {:?}", w.map.w, w.map.h, (h.cx, h.cy));
    let mut v: Vec<_> = w
        .nodes
        .iter()
        .map(|n| (n.pos.dist(hp) / 256, n.resource, n.amount, n.pos.cell()))
        .collect();
    v.sort();
    for n in v {
        eprintln!("{n:?}");
    }
}

/// Prints a seed's map (MAP_TYPE, SEED, SEATS=2 or 4) one character per
/// cell: '#' rock, '%' dirt, ' ' floor, '$' cache, 'F'/'N'/'L' nodes, 'H'
/// home centres, '+' other chamber centres.
#[test]
#[ignore]
fn map_dump() {
    use tool_core::map::Ground;
    use tool_core::rules::Resource;
    use tool_core::settings::Format;
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let format = match std::env::var("SEATS").as_deref() {
        Ok("4") => Format::FreeForAll,
        _ => Format::OneVOne,
    };
    let s = MatchSettings {
        format,
        ..settings()
    };
    let m = tool_core::map::generate(seed, &s);
    let mut rows: Vec<Vec<char>> = (0..m.h)
        .map(|y| {
            (0..m.w)
                .map(|x| match m.at(x, y) {
                    Ground::Rock => '#',
                    Ground::Dirt => '%',
                    Ground::Floor => ' ',
                })
                .collect()
        })
        .collect();
    for c in &m.chambers {
        rows[c.cy as usize][c.cx as usize] = if c.home.is_some() { 'H' } else { '+' };
    }
    for n in &m.nodes {
        let (x, y) = n.pos.cell();
        rows[y as usize][x as usize] = if n.cache {
            '$'
        } else {
            match n.resource {
                Resource::Food => 'F',
                Resource::Nectar => 'N',
                Resource::Leaves => 'L',
            }
        };
    }
    eprintln!("MAP {} {}", m.w, m.h);
    for r in rows {
        eprintln!("{}", r.into_iter().collect::<String>());
    }
}

/// Map generation time per Map Type, Format and Map Size, in release:
/// `cargo test --release -p tool_core --test ai -- --ignored --nocapture
/// map_generation_time`. Every map must take well under 50 ms.
#[test]
#[ignore]
fn map_generation_time() {
    use tool_core::settings::{Format, Level};
    for map_type in [MapType::Tunnels, MapType::Open, MapType::Maze] {
        for format in [Format::OneVOne, Format::FreeForAll] {
            for map_size in [Level::Normal, Level::High] {
                let s = MatchSettings {
                    format,
                    map_size,
                    map_type,
                    ..MatchSettings::default()
                };
                let mut worst = 0u128;
                let mut total = 0u128;
                for seed in 0..10 {
                    let t0 = std::time::Instant::now();
                    let m = tool_core::map::generate(seed, &s);
                    let us = t0.elapsed().as_micros();
                    assert!(m.w > 0);
                    worst = worst.max(us);
                    total += us;
                }
                eprintln!(
                    "map {map_type:?} {format:?} {map_size:?}: mean {} us, worst {worst} us",
                    total / 10
                );
                assert!(
                    worst < 50_000,
                    "{map_type:?} {format:?} {map_size:?}: {worst} us"
                );
            }
        }
    }
}
