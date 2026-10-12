//! Performance budget (PLAN decision 24). Ignored by default because timing
//! depends on the machine; run it with
//!
//!     cargo test --release -p tool_core --test perf -- --ignored --nocapture
//!
//! and record the result in MEASUREMENTS.md.

use std::time::Instant;

use tool_core::fixed::Pos;
use tool_core::rules::Species;
use tool_core::settings::{Format, MatchSettings, SeatKind};
use tool_core::world::{Command, World};

#[test]
#[ignore]
fn twelve_hundred_ants_tick_within_budget() {
    let s = MatchSettings {
        format: Format::FreeForAll,
        events: false,
        ..MatchSettings::default()
    };
    let mut w = World::new(31, s, &[SeatKind::Human; 4]);
    let mix = [
        (Species::Harvester, 40),
        (Species::Soldier, 120),
        (Species::Acid, 40),
        (Species::Turtle, 30),
        (Species::Fire, 50),
        (Species::Flying, 20),
    ];
    for seat in 0..4u8 {
        w.colonies[seat as usize].tier = 3;
        let h = w.map.home(seat as usize).clone();
        for (sp, n) in mix {
            for _ in 0..n {
                w.spawn_ant(seat, sp, Pos::of_cell(h.cx, h.cy));
            }
        }
    }
    let c = w.map.chambers[w.map.centre].clone();
    let mut cmds = Vec::new();
    for seat in 0..4u8 {
        cmds.push((
            seat,
            Command::SetWaypoint {
                species: None,
                x: c.cx,
                y: c.cy,
                add: false,
            },
        ));
    }
    w.step(&cmds, &[]);
    assert!(w.ants.len() >= 1200, "{} ants", w.ants.len());
    let mut worst = 0u128;
    let mut total = 0u128;
    let ticks = 300;
    for _ in 0..ticks {
        let t0 = Instant::now();
        w.step(&[], &[]);
        let us = t0.elapsed().as_micros();
        worst = worst.max(us);
        total += us;
    }
    let avg = total / ticks as u128;
    eprintln!(
        "PERF: {} ants left, average tick {} us, worst {} us",
        w.ants.len(),
        avg,
        worst
    );
    assert!(avg < 8_000, "average tick {avg} us is over the 8 ms budget");
}

/// Plays an AI match in `format` (four Imago colonies) and reports tick
/// times per minute of game time; returns (average, worst) in us over the
/// whole match.
fn ai_match_ticks(format: Format, seed: u64, minutes: i32) -> (u128, u128) {
    use tool_core::rules::TICKS_PER_SECOND;
    use tool_core::settings::Difficulty;
    let s = MatchSettings {
        format,
        ..MatchSettings::default()
    };
    let mut w = World::new(seed, s, &[SeatKind::Ai(Difficulty::Imago); 4]);
    let minute = 60 * TICKS_PER_SECOND;
    let (mut total, mut worst, mut n) = (0u128, 0u128, 0u128);
    let (mut m_total, mut m_worst) = (0u128, 0u128);
    // Ticks on which dirt was dug: each repaints the terrain image.
    let mut dirt = tool_core::render::terrain_stamp(&w);
    let mut digs = 0;
    while w.winner.is_none() && w.tick < minutes * minute {
        let t0 = Instant::now();
        w.step(&[], &[]);
        let us = t0.elapsed().as_micros();
        total += us;
        n += 1;
        worst = worst.max(us);
        m_total += us;
        m_worst = m_worst.max(us);
        let now = tool_core::render::terrain_stamp(&w);
        if now != dirt {
            dirt = now;
            digs += 1;
        }
        if w.tick % minute == 0 {
            eprintln!(
                "PERF {format:?} minute {}: {} ants, {} structures, average {} us, worst {} us, {digs} ticks with dirt dug",
                w.tick / minute,
                w.ants.len(),
                w.structures.len(),
                m_total / minute as u128,
                m_worst
            );
            digs = 0;
            m_total = 0;
            m_worst = 0;
        }
    }
    let avg = total / n.max(1);
    eprintln!(
        "PERF {format:?}: {} ticks, winner {:?}, average {avg} us, worst {worst} us",
        w.tick, w.winner
    );
    (avg, worst)
}

#[test]
#[ignore]
fn two_v_two_ai_match_ticks_within_budget() {
    let (avg, _) = ai_match_ticks(Format::TwoVTwo, 3, 12);
    assert!(avg < 8_000, "average tick {avg} us is over the 8 ms budget");
}

#[test]
#[ignore]
fn free_for_all_ai_match_ticks_within_budget() {
    let (avg, _) = ai_match_ticks(Format::FreeForAll, 4, 12);
    assert!(avg < 8_000, "average tick {avg} us is over the 8 ms budget");
}

/// What the page asks the core for on every animation frame of a 2v2
/// against the AI (the player's seat played by an Imago AI too, seen
/// through its Fog of War), timed per part at 60 frames per second.
#[test]
#[ignore]
fn two_v_two_frames_within_budget() {
    use tool_core::app::App;
    use tool_core::settings::Difficulty;
    use tool_core::world::Controller;
    let strings = include_str!("../../../data/strings.json");
    let glossary = include_str!("../../../data/glossary.json");
    let mut a = App::new(strings, glossary, "", "", "me").expect("data files parse");
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.format", "two_v_two", false, false);
    a.act("pva.seed", "3", false, false);
    a.act("pva.start", "", false, false);
    a.resize(1920, 1080);
    a.game.as_mut().unwrap().world.colonies[0].controller = Controller::Ai(Difficulty::Imago);
    let names = ["advance", "ui", "frame", "fog", "minimap", "terrain"];
    let mut total = [0u128; 6];
    let mut worst = [0u128; 6];
    let mut frame_worst = 0u128;
    let (mut fog_stamp, mut terrain_stamp) = (-1, 0);
    let frames = 60 * 60 * 6;
    let mut now = 0i64;
    for f in 0..frames {
        now += 1000 / 60 + (f % 3 == 0) as i64;
        let mut us = [0u128; 6];
        let t = Instant::now();
        a.advance(now);
        us[0] = t.elapsed().as_micros();
        let t = Instant::now();
        let _ = a.ui().to_string();
        us[1] = t.elapsed().as_micros();
        let t = Instant::now();
        let _ = a.frame();
        us[2] = t.elapsed().as_micros();
        if a.fog_stamp() != fog_stamp {
            fog_stamp = a.fog_stamp();
            let t = Instant::now();
            let _ = a.fog();
            us[3] = t.elapsed().as_micros();
            let t = Instant::now();
            let _ = a.minimap(200);
            us[4] = t.elapsed().as_micros();
        }
        if a.terrain_stamp() != terrain_stamp {
            terrain_stamp = a.terrain_stamp();
            let t = Instant::now();
            let _ = a.terrain();
            us[5] = t.elapsed().as_micros();
        }
        for k in 0..6 {
            total[k] += us[k];
            worst[k] = worst[k].max(us[k]);
        }
        frame_worst = frame_worst.max(us.iter().sum());
        if a.game.as_ref().unwrap().world.winner.is_some() {
            break;
        }
    }
    let g = a.game.as_ref().unwrap();
    eprintln!(
        "FRAMES 2v2: {} ticks, {} ants, map {}x{}",
        g.world.tick,
        g.world.ants.len(),
        g.world.map.w,
        g.world.map.h
    );
    let mut sum = 0;
    for k in 0..6 {
        sum += total[k];
        eprintln!(
            "FRAMES 2v2 {}: average {} us, worst {} us",
            names[k],
            total[k] / frames as u128,
            worst[k]
        );
    }
    let avg = sum / frames as u128;
    eprintln!("FRAMES 2v2 all: average {avg} us per frame, worst {frame_worst} us");
    assert!(
        avg < 8_000,
        "average frame {avg} us is over the 8 ms budget"
    );
}

/// One flow field over a 2v2 map, built from scratch.
#[test]
#[ignore]
fn flow_field_build_time() {
    use tool_core::nav::{FieldKey, Mode, Nav, Terrain};
    let s = MatchSettings {
        format: Format::TwoVTwo,
        ..MatchSettings::default()
    };
    let w = World::new(3, s, &[SeatKind::Human; 4]);
    let teams = [0u8, 1, 0, 1, 104, 105, 106, 107];
    let t = Terrain {
        w: w.map.w,
        h: w.map.h,
        ground: &w.map.ground,
        wall: &w.wall,
        teams: &teams,
    };
    let mut nav = Nav::default();
    let runs = 200;
    let t0 = Instant::now();
    for k in 0..runs {
        let c = &w.map.chambers[k as usize % w.map.chambers.len()];
        let key = FieldKey {
            colony: (k % 2) as u8,
            mode: if k % 3 == 0 { Mode::Walk } else { Mode::Dig },
            x: c.cx,
            y: c.cy,
            r: 2 + k % 5,
        };
        nav.invalidate();
        let _ = nav.field(&t, key);
    }
    let us = t0.elapsed().as_micros() / runs as u128;
    eprintln!(
        "NAV: one field over {}x{} cells in {us} us",
        w.map.w, w.map.h
    );
}

/// What one dug cell costs in a 2v2 three minutes in: repairing every
/// cached flow field (before: every field was rebuilt) and repainting the
/// terrain round it (before: the whole image).
#[test]
#[ignore]
fn a_dug_cell_costs_little() {
    use tool_core::map::Ground;
    use tool_core::nav::Terrain;
    use tool_core::render::{terrain_image, terrain_region, TERRAIN_REACH, TERRAIN_SCALE};
    use tool_core::rules::TICKS_PER_SECOND;
    use tool_core::settings::Difficulty;
    // Open Ground: dirt rings round the caches.
    let s = MatchSettings {
        format: Format::TwoVTwo,
        map_type: tool_core::settings::MapType::Open,
        ..MatchSettings::default()
    };
    let mut w = World::new(3, s, &[SeatKind::Ai(Difficulty::Imago); 4]);
    while w.tick < 3 * 60 * TICKS_PER_SECOND {
        w.step(&[], &[]);
    }
    let fields = w.nav.len();
    let mut teams = [0u8; 8];
    for (i, t) in teams.iter_mut().enumerate() {
        *t = w.colonies.get(i).map_or(100 + i as u8, |c| c.team);
    }
    let dirt: Vec<usize> = (0..w.map.ground.len())
        .filter(|&i| w.map.ground[i] == Ground::Dirt)
        .collect();
    let runs = 40.min(dirt.len());
    let mut repair = 0u128;
    let mut patch = 0u128;
    for &i in dirt.iter().take(runs) {
        let (x, y) = (i as i32 % w.map.w, i as i32 / w.map.w);
        w.map.ground[i] = Ground::Floor;
        let t = Terrain {
            w: w.map.w,
            h: w.map.h,
            ground: &w.map.ground,
            wall: &w.wall,
            teams: &teams,
        };
        let t0 = Instant::now();
        w.nav.opened(&t, &[(x, y)]);
        repair += t0.elapsed().as_micros();
        let r = TERRAIN_REACH;
        let t0 = Instant::now();
        let _ = terrain_region(&w, TERRAIN_SCALE, x - r, y - r, x + r + 1, y + r + 1);
        patch += t0.elapsed().as_micros();
    }
    let t0 = Instant::now();
    let _ = terrain_image(&w, TERRAIN_SCALE);
    let whole = t0.elapsed().as_micros();
    eprintln!(
        "DIG: {fields} cached fields repaired in {} us; terrain patch {} us (whole image {whole} us)",
        repair / runs as u128,
        patch / runs as u128
    );
}
