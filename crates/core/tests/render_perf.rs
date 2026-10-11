//! Drawing budget (PLAN decision 24): the draw list for 1,200 ants under
//! 4 ms. Ignored by default because timing depends on the machine; run it
//! with
//!
//!     cargo test --release -p tool_core --test render_perf -- --ignored --nocapture

use std::time::Instant;

use tool_core::bench::{bench_world, BENCH_SEED};
use tool_core::render::{frame, terrain_image, Camera, Overlay, TERRAIN_SCALE};
use tool_core::settings::Biome;

#[test]
#[ignore]
fn twelve_hundred_ants_draw_within_budget() {
    let mut w = bench_world(BENCH_SEED, Biome::Picnic);
    for _ in 0..100 {
        w.step(&[], &[]);
    }
    assert!(w.ants.len() >= 1000, "{} ants", w.ants.len());
    let mut cam = Camera::new(1920, 1080);
    cam.fit(w.map.w, w.map.h);
    let runs = 200;
    for (name, viewer) in [("everything", None), ("colony 0", Some(0))] {
        let t0 = Instant::now();
        let mut len = 0;
        for i in 0..runs {
            len = frame(&w, viewer, &cam, &Overlay::default(), i * 5).len();
        }
        let us = t0.elapsed().as_micros() / runs as u128;
        eprintln!(
            "DRAW ({name}, whole map): {} ants, {len} ints, {us} us per frame",
            w.ants.len()
        );
        assert!(us < 4_000, "draw list {us} us is over the 4 ms budget");
    }
    let t0 = Instant::now();
    let (iw, ih, _) = terrain_image(&w, TERRAIN_SCALE);
    eprintln!(
        "TERRAIN: {iw}x{ih} pixels in {} ms",
        t0.elapsed().as_millis()
    );
}
