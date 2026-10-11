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

    pub fn net_event(&mut self, kind: &str, peer: &str, text: &str) {
        self.s.net_event(kind, peer, text)
    }

    pub fn net_code(&mut self, which: &str, code: &str) {
        self.s.net_code(which, code)
    }

    pub fn outbox(&mut self) -> String {
        self.s.outbox()
    }
}
