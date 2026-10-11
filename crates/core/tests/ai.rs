//! AI difficulty ladder (PLAN decision 16): in seeded headless 1v1 matches,
//! Imago beats Pupa and Pupa beats Larva most of the time. Slow, so it is
//! ignored by default; run it with
//!
//!     cargo test --release -p tool_core --test ai -- --ignored --nocapture

use tool_core::settings::{Difficulty, MatchSettings, SeatKind};
use tool_core::world::World;

const LIMIT: i32 = 12_000; // 20 minutes of game time: no match should get near it

/// Plays one match; returns the winning seat (or None) and the tick it ended.
fn play(seed: u64, a: Difficulty, b: Difficulty) -> (Option<u8>, i32) {
    let mut w = World::new(
        seed,
        MatchSettings::default(),
        &[SeatKind::Ai(a), SeatKind::Ai(b)],
    );
    while w.winner.is_none() && w.tick < LIMIT {
        w.step(&[], &[]);
    }
    (w.winner, w.tick)
}

fn ladder(strong: Difficulty, weak: Difficulty) -> (i32, i32, Vec<i32>) {
    let mut wins = 0;
    let mut games = 0;
    let mut lengths = Vec::new();
    for seed in 0..10u64 {
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
        lengths.push(t);
    }
    (wins, games, lengths)
}

#[test]
#[ignore]
fn imago_beats_pupa_and_pupa_beats_larva() {
    let (w1, g1, l1) = ladder(Difficulty::Imago, Difficulty::Pupa);
    eprintln!("AI: Imago beat Pupa {w1}/{g1}, lengths {l1:?}");
    let (w2, g2, l2) = ladder(Difficulty::Pupa, Difficulty::Larva);
    eprintln!("AI: Pupa beat Larva {w2}/{g2}, lengths {l2:?}");
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
    let mut w = World::new(
        seed,
        MatchSettings::default(),
        &[SeatKind::Ai(diff("A")), SeatKind::Ai(diff("B"))],
    );
    while w.winner.is_none() && w.tick < 9000 {
        w.step(&[], &[]);
        if w.tick % 600 == 0 {
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
}

#[test]
#[ignore]
fn trace_gather() {
    use tool_core::rules::Species;
    use tool_core::world::Job;
    let mut w = World::new(
        0,
        MatchSettings::default(),
        &[SeatKind::Human, SeatKind::Human],
    );
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
