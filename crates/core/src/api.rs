//! The one door into the core. The shim forwards each call here unchanged;
//! values cross as strings, integers and JSON text.

use serde_json::json;

use crate::app::App;

/// A running game application: menus, settings and the match.
pub struct Session {
    app: Option<App>,
    error: String,
}

impl Session {
    /// Builds the application from the text files and saved settings. A bad
    /// strings or glossary file never panics: `ui()` then reports it.
    pub fn new(
        strings_json: &str,
        glossary_json: &str,
        saved_defaults: &str,
        saved_player: &str,
        client_id: &str,
        entropy: u64,
    ) -> Session {
        match App::new(
            strings_json,
            glossary_json,
            saved_defaults,
            saved_player,
            client_id,
        ) {
            Ok(mut app) => {
                app.entropy = entropy.max(1);
                Session {
                    app: Some(app),
                    error: String::new(),
                }
            }
            Err(e) => Session {
                app: None,
                error: format!("data/strings.json or data/glossary.json: {e}"),
            },
        }
    }

    /// The view model for the page (see `App::ui`), or `{"error": ...}`.
    pub fn ui(&mut self) -> String {
        match &mut self.app {
            Some(a) => a.ui().to_string(),
            None => json!({ "error": self.error }).to_string(),
        }
    }

    /// The term hints, `{term: hint}`, for hovering glossary words.
    pub fn hints(&self) -> String {
        match &self.app {
            Some(a) => serde_json::to_string(&a.hints).expect("serialises"),
            None => "{}".into(),
        }
    }

    pub fn act(&mut self, id: &str, value: &str, ctrl: bool, shift: bool) {
        if let Some(a) = &mut self.app {
            a.act(id, value, ctrl, shift);
        }
    }

    pub fn key(&mut self, code: &str, down: bool, ctrl: bool, shift: bool) {
        if let Some(a) = &mut self.app {
            a.key(code, down, ctrl, shift);
        }
    }

    pub fn mouse(&mut self, kind: &str, button: i32, x: i32, y: i32, shift: bool) {
        if let Some(a) = &mut self.app {
            a.mouse(kind, button, x, y, shift);
        }
    }

    pub fn wheel(&mut self, dy: i32) {
        if let Some(a) = &mut self.app {
            a.wheel(dy);
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        if let Some(a) = &mut self.app {
            a.resize(w, h);
        }
    }

    pub fn jump(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if let Some(a) = &mut self.app {
            a.jump(x, y, w, h);
        }
    }

    pub fn advance(&mut self, now_ms: i64) {
        if let Some(a) = &mut self.app {
            a.advance(now_ms);
        }
    }

    pub fn net_event(&mut self, kind: &str, peer: &str, text: &str, host: bool) {
        if let Some(a) = &mut self.app {
            a.net_event(kind, peer, text, host);
        }
    }

    pub fn net_code(&mut self, which: &str, code: &str) {
        if let Some(a) = &mut self.app {
            a.net_code(which, code);
        }
    }

    /// Messages for the transport: `[{"to": peer | null, "text": ...}]`.
    pub fn outbox(&mut self) -> String {
        match &mut self.app {
            Some(a) => a.take_outbox().to_string(),
            None => "[]".into(),
        }
    }

    pub fn frame(&self) -> Vec<i32> {
        self.app.as_ref().map(|a| a.frame()).unwrap_or_default()
    }

    /// The interface icons for this match (see `App::icons`).
    pub fn icons(&self) -> String {
        self.app
            .as_ref()
            .map(|a| a.icons().to_string())
            .unwrap_or_else(|| "{}".into())
    }

    pub fn terrain_stamp(&self) -> u32 {
        self.app.as_ref().map(|a| a.terrain_stamp()).unwrap_or(0)
    }

    pub fn terrain_size(&self) -> Vec<i32> {
        self.app
            .as_ref()
            .map(|a| a.terrain_size())
            .unwrap_or_default()
    }

    pub fn terrain(&mut self) -> Vec<u8> {
        self.app.as_mut().map(|a| a.terrain()).unwrap_or_default()
    }

    pub fn terrain_patch(&mut self) -> Vec<i32> {
        self.app
            .as_mut()
            .map(|a| a.terrain_patch())
            .unwrap_or_default()
    }

    pub fn terrain_patch_pixels(&mut self) -> Vec<u8> {
        self.app
            .as_mut()
            .map(|a| a.terrain_patch_pixels())
            .unwrap_or_default()
    }

    pub fn fog_stamp(&self) -> i32 {
        self.app.as_ref().map(|a| a.fog_stamp()).unwrap_or(-1)
    }

    pub fn fog_size(&self) -> Vec<i32> {
        self.app.as_ref().map(|a| a.fog_size()).unwrap_or_default()
    }

    pub fn fog(&self) -> Vec<u8> {
        self.app.as_ref().map(|a| a.fog()).unwrap_or_default()
    }

    pub fn minimap(&self, size: i32) -> Vec<u8> {
        self.app
            .as_ref()
            .map(|a| a.minimap(size))
            .unwrap_or_default()
    }

    /// A sound's WAV bytes (see `App::sound`); empty for an unknown key.
    pub fn sound(&self, key: &str) -> Vec<u8> {
        self.app.as_ref().map(|a| a.sound(key)).unwrap_or_default()
    }

    pub fn app(&mut self) -> Option<&mut App> {
        self.app.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings() -> String {
        include_str!("../../../data/strings.json").to_string()
    }

    fn glossary() -> String {
        include_str!("../../../data/glossary.json").to_string()
    }

    #[test]
    fn a_bad_strings_file_is_reported_not_panicked() {
        let mut s = Session::new("not json", "{}", "", "", "c", 1);
        assert!(s.ui().starts_with(r#"{"error":"data/strings.json"#));
    }

    #[test]
    fn the_main_menu_offers_the_four_choices_from_the_brief() {
        let mut s = Session::new(&strings(), &glossary(), "", "", "c", 1);
        let ui: serde_json::Value = serde_json::from_str(&s.ui()).unwrap();
        assert_eq!(ui["screen"], "main_menu");
        let items: Vec<String> = ui["panels"][0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            items,
            vec![
                "Play vs AI",
                "Online Multiplayer",
                "Match Settings",
                "Player Settings",
                "Match History"
            ]
        );
    }
}
