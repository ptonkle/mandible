//! tool_shim: forwards strings between the page and `tool_core::api`.
//!
//! Builder requirement: the shim decides nothing. Each function is one call
//! into the core. `crates/core/tests/boundary.rs` fails if a branch, a loop or
//! the `?` operator appears here; if a feature seems to need one, the rule
//! belongs in the core.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn load(strings_json: &str) -> String {
    tool_core::api::load(strings_json)
}

/// The draw list's assets (op codes, palette, shapes, scales) as JSON text.
#[wasm_bindgen]
pub fn render_assets() -> String {
    tool_core::render::assets_json()
}

/// The Benchmark: the performance-test scenario, ticked and drawn live.
/// Browser timestamps are milliseconds as floats; they cross into the core
/// as whole microseconds, and pixel positions as whole pixels.
#[wasm_bindgen]
pub struct Bench {
    inner: tool_core::bench::Bench,
}

#[wasm_bindgen]
impl Bench {
    #[wasm_bindgen(constructor)]
    pub fn new(view_w: f64, view_h: f64, query: &str) -> Bench {
        Bench {
            inner: tool_core::bench::Bench::new(view_w as i32, view_h as i32, query),
        }
    }

    pub fn fog_size(&self) -> Vec<i32> {
        self.inner.fog_size()
    }

    pub fn fog(&self) -> Vec<u8> {
        self.inner.fog()
    }

    pub fn terrain_size(&self) -> Vec<i32> {
        self.inner.terrain_size()
    }

    pub fn terrain(&self) -> Vec<u8> {
        self.inner.terrain()
    }

    pub fn terrain_stamp(&self) -> u32 {
        self.inner.terrain_stamp()
    }

    pub fn resize(&mut self, view_w: f64, view_h: f64) {
        self.inner.resize(view_w as i32, view_h as i32)
    }

    pub fn frame_start(&mut self, now_ms: f64) {
        self.inner.frame_start((now_ms * 1000.0) as i64)
    }

    pub fn tick_due(&self) -> bool {
        self.inner.tick_due()
    }

    pub fn tick(&mut self) {
        self.inner.tick()
    }

    pub fn tick_done(&mut self, start_ms: f64, end_ms: f64) {
        self.inner
            .tick_done((start_ms * 1000.0) as i64, (end_ms * 1000.0) as i64)
    }

    pub fn draw(&self) -> Vec<i32> {
        self.inner.draw()
    }

    pub fn draw_done(&mut self, start_ms: f64, end_ms: f64) {
        self.inner
            .draw_done((start_ms * 1000.0) as i64, (end_ms * 1000.0) as i64)
    }

    pub fn panel(&self) -> String {
        self.inner.panel_json()
    }

    pub fn wheel(&mut self, delta: f64, x: f64, y: f64) {
        self.inner.wheel(delta as i32, x as i32, y as i32)
    }

    pub fn drag_start(&mut self, x: f64, y: f64) {
        self.inner.drag_start(x as i32, y as i32)
    }

    pub fn drag_move(&mut self, x: f64, y: f64) {
        self.inner.drag_move(x as i32, y as i32)
    }

    pub fn drag_end(&mut self) {
        self.inner.drag_end()
    }
}
