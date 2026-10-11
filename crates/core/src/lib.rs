//! tool_core: every rule of the ant colony game.
//!
//! Builder requirement: this crate performs no input or output. It takes
//! values and returns values, so `cargo test` reaches every rule without a
//! browser. `tests/boundary.rs` enforces that, along with the dependency
//! allowlist and the reference ban on floats, hash maps and clocks.

// Resources are [Food, Nectar, Leaves] arrays indexed in parallel, which
// these two lints fight; every other clippy lint stays an error.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod ai;
pub mod ants;
pub mod api;
pub mod dice;
pub mod fixed;
pub mod fog;
pub mod lockstep;
pub mod map;
pub mod nav;
pub mod page;
pub mod rules;
pub mod settings;
pub mod world;
