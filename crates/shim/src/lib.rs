//! tool_shim: forwards values between the page and `tool_core::api`.
//!
//! Builder requirement: the shim decides nothing. Each method is one call
//! into the core. `crates/core/tests/boundary.rs` fails if a branch, a loop or
//! the `?` operator appears here. Casting the browser's floating-point mouse
//! positions and timestamps to integers is the only translation.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Game {
    s: tool_core::api::Session,
}

#[wasm_bindgen]
impl Game {
    #[wasm_bindgen(constructor)]
    pub fn new(
        strings: &str,
        glossary: &str,
        defaults: &str,
        player: &str,
        client_id: &str,
        entropy: f64,
    ) -> Game {
        Game {
            s: tool_core::api::Session::new(
                strings,
                glossary,
                defaults,
                player,
                client_id,
                entropy as u64,
            ),
        }
    }

    pub fn ui(&mut self) -> String {
        self.s.ui()
    }

    pub fn hints(&self) -> String {
        self.s.hints()
    }

    pub fn act(&mut self, id: &str, value: &str, ctrl: bool, shift: bool) {
        self.s.act(id, value, ctrl, shift)
    }

    pub fn key(&mut self, code: &str, down: bool, ctrl: bool, shift: bool) {
        self.s.key(code, down, ctrl, shift)
    }

    pub fn mouse(&mut self, kind: &str, button: i32, x: f64, y: f64, shift: bool) {
        self.s.mouse(kind, button, x as i32, y as i32, shift)
    }

    pub fn wheel(&mut self, dy: f64) {
        self.s.wheel(dy as i32)
    }

    pub fn resize(&mut self, w: f64, h: f64) {
        self.s.resize(w as i32, h as i32)
    }

    pub fn jump(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.s.jump(x as i32, y as i32, w as i32, h as i32)
    }

    pub fn advance(&mut self, now_ms: f64) {
        self.s.advance(now_ms as i64)
    }

    pub fn net_event(&mut self, kind: &str, peer: &str, text: &str, host: bool) {
        self.s.net_event(kind, peer, text, host)
    }

    pub fn net_code(&mut self, which: &str, code: &str) {
        self.s.net_code(which, code)
    }

    pub fn outbox(&mut self) -> String {
        self.s.outbox()
    }

    pub fn frame(&self) -> Vec<i32> {
        self.s.frame()
    }

    pub fn icons(&self) -> String {
        self.s.icons()
    }

    pub fn terrain_stamp(&self) -> u32 {
        self.s.terrain_stamp()
    }

    pub fn terrain_size(&self) -> Vec<i32> {
        self.s.terrain_size()
    }

    pub fn terrain(&self) -> Vec<u8> {
        self.s.terrain()
    }

    pub fn fog_stamp(&self) -> i32 {
        self.s.fog_stamp()
    }

    pub fn fog_size(&self) -> Vec<i32> {
        self.s.fog_size()
    }

    pub fn fog(&self) -> Vec<u8> {
        self.s.fog()
    }

    pub fn minimap(&self, size: f64) -> Vec<u8> {
        self.s.minimap(size as i32)
    }
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
