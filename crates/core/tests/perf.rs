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
