# Measurements

Stage 4, recorded by the agent before building. `B2 Require step-by-step justification`.

The starting state, measured rather than recalled, so later changes can be compared with it.

| What | Command | Result | When |
| --- | --- | --- | --- |
| Toolchain | `rustc --version; wasm-bindgen --version` | rustc 1.99.0, wasm-bindgen 0.2.127, wasm32 target installed; 16 cores, 30 GB RAM (WSL2) | 2026-10-10 |
| Core tests | `cargo test --workspace` | template: 7 unit + 11 boundary tests pass (after `cargo clean`: stale `target/` pointed at an old path `~/game`) | 2026-10-10 |
| Full check | `./scripts/check.sh` | BUILDER CHECK PASSED in chromium, firefox, webkit; 44 s wall time from clean | 2026-10-10 |
