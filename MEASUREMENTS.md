# Measurements

Stage 4, recorded by the agent before building. `B2 Require step-by-step justification`.

The starting state, measured rather than recalled, so later changes can be compared with it.

| What | Command | Result | When |
| --- | --- | --- | --- |
| Toolchain | `rustc --version; wasm-bindgen --version` | rustc 1.99.0, wasm-bindgen 0.2.127, wasm32 target installed; 16 cores, 30 GB RAM (WSL2) | 2026-10-10 |
| Core tests | `cargo test --workspace` | template: 7 unit + 11 boundary tests pass (after `cargo clean`: stale `target/` pointed at an old path `~/game`) | 2026-10-10 |
| Full check | `./scripts/check.sh` | BUILDER CHECK PASSED in chromium, firefox, webkit; 44 s wall time from clean | 2026-10-10 |
| Simulation, 1,200 ants (Free-for-all, tier 3 mix, all marching to the centre) | `cargo test --release -p tool_core --test perf -- --ignored --nocapture` | 2.8 ms average tick, 8.4 ms worst (native, release); budget 8 ms average | 2026-10-11 |
| Draw list, 1,240 ants | renderer agent's ignored test | 13 µs per frame native (8 µs with fog); terrain image 39 ms once per match | 2026-10-10 |
| Benchmark in the browser | `tests/browser/check.py` (Player Settings → Benchmark) | Chromium headless, software rendering: 60 frames per second with the whole map in view, tick 5.0 ms average and 6.4 ms worst in wasm, draw 0.65 ms: Within budget. Zoomed to 8–16 px per cell: 40–52 frames per second (canvas rasterisation, not the core) | 2026-10-11 |
| AI ladder | `cargo test --release -p tool_core --test ai -- --ignored --nocapture` | Imago beat Pupa 10/10 (median about 4,000 ticks, 6.7 min); Pupa beat Larva 10/10 (median about 3,900 ticks) | 2026-10-11 |
| Full check | `./scripts/check.sh` | BUILDER CHECK PASSED (fmt, clippy, tests, wasm build, import audit, three engines) | 2026-10-11 |
| Online check | `./scripts/check-online.sh` | ONLINE CHECK PASSED and ONLINE MATCH CHECK PASSED in chromium, firefox, webkit | 2026-10-11 |
| After the triage pass (20 ticks per second) | ladder, perf, check.sh, check-online.sh | Imago beat Pupa 10/10 (median 6,788 ticks = 5:39), Pupa beat Larva 10/10 (median 5,869 = 4:53); 2.7 ms average tick, 7.5 ms worst, 1,200 ants; BUILDER CHECK PASSED; ONLINE CHECK PASSED in three engines | 2026-10-11 |
