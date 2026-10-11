//! Player Settings: Keybinds. Hotkeys follow the keyboard's shape: the action
//! grid's 3 x 4 buttons sit on Q W E R / A S D F / Z X C V and its tabs on
//! 1 2 3 4 5, the row above. Keys are `KeyboardEvent.code` values (physical
//! keys), so the shape holds on any layout.

use serde::{Deserialize, Serialize};

/// Every rebindable slot, in Keybinds-screen order.
pub const SLOTS: [&str; 22] = [
    "grid.0",
    "grid.1",
    "grid.2",
    "grid.3",
    "grid.4",
    "grid.5",
    "grid.6",
    "grid.7",
    "grid.8",
    "grid.9",
    "grid.10",
    "grid.11",
    "tab.0",
    "tab.1",
    "tab.2",
    "tab.3",
    "tab.4",
    "menu",
    "home",
    "pan.left",
    "pan.right",
    "pan.up",
];

pub const EXTRA_SLOTS: [&str; 2] = ["pan.down", "pause"];

fn default_key(slot: &str) -> &'static str {
    match slot {
        "grid.0" => "KeyQ",
        "grid.1" => "KeyW",
        "grid.2" => "KeyE",
        "grid.3" => "KeyR",
        "grid.4" => "KeyA",
        "grid.5" => "KeyS",
        "grid.6" => "KeyD",
        "grid.7" => "KeyF",
        "grid.8" => "KeyZ",
        "grid.9" => "KeyX",
        "grid.10" => "KeyC",
        "grid.11" => "KeyV",
        "tab.0" => "Digit1",
        "tab.1" => "Digit2",
        "tab.2" => "Digit3",
        "tab.3" => "Digit4",
        "tab.4" => "Digit5",
        "menu" => "Escape",
        "home" => "Space",
        "pan.left" => "ArrowLeft",
        "pan.right" => "ArrowRight",
        "pan.up" => "ArrowUp",
        "pan.down" => "ArrowDown",
        "pause" => "KeyP",
        _ => "",
    }
}

pub fn all_slots() -> Vec<&'static str> {
    SLOTS.iter().chain(EXTRA_SLOTS.iter()).copied().collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PlayerSettings {
    /// (slot, key code) pairs; slots missing here use their default.
    pub keys: Vec<(String, String)>,
    pub edge_pan: bool,
    /// Player Name, shown in the Lobby and in Waiting On; empty means the
    /// default ("Player").
    pub name: String,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        PlayerSettings {
            keys: Vec::new(),
            edge_pan: true,
            name: String::new(),
        }
    }
}

impl PlayerSettings {
    pub fn parse(json: &str) -> Result<PlayerSettings, String> {
        let p: PlayerSettings = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for (slot, _) in &p.keys {
            if !all_slots().contains(&slot.as_str()) {
                return Err(format!("Keybinds: unknown slot {slot}"));
            }
        }
        Ok(p)
    }

    pub fn key_of(&self, slot: &str) -> String {
        self.keys
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, k)| k.clone())
            .unwrap_or_else(|| default_key(slot).to_string())
    }

    /// The slot a key triggers, if any.
    pub fn slot_of(&self, code: &str) -> Option<&'static str> {
        all_slots().into_iter().find(|s| self.key_of(s) == code)
    }

    /// Binds a key to a slot. A key already used elsewhere moves: the other
    /// slot takes this slot's old key, so no two slots share a key.
    pub fn bind(&mut self, slot: &str, code: &str) {
        let old = self.key_of(slot);
        if let Some(other) = self.slot_of(code) {
            if other != slot {
                self.set(other, &old);
            }
        }
        self.set(slot, code);
    }

    fn set(&mut self, slot: &str, code: &str) {
        self.keys.retain(|(s, _)| s != slot);
        if default_key(slot) != code {
            self.keys.push((slot.to_string(), code.to_string()));
        }
        self.keys.sort();
    }

    pub fn reset(&mut self) {
        self.keys.clear();
    }
}

/// The letter shown on a button for a key code: "KeyQ" → "Q", "Digit1" → "1".
pub fn key_label(code: &str) -> String {
    if let Some(r) = code.strip_prefix("Key") {
        return r.to_string();
    }
    if let Some(r) = code.strip_prefix("Digit") {
        return r.to_string();
    }
    match code {
        "Escape" => "Esc".into(),
        "Space" => "Space".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_follow_the_keyboard_shape() {
        let p = PlayerSettings::default();
        // From the brief: bottom-left is Z, bottom-right is V, and the 2nd
        // row's 3rd column is D, in a 3 x 4 grid.
        assert_eq!(p.key_of("grid.8"), "KeyZ");
        assert_eq!(p.key_of("grid.11"), "KeyV");
        assert_eq!(p.key_of("grid.6"), "KeyD");
        assert_eq!(p.slot_of("KeyD"), Some("grid.6"));
    }

    #[test]
    fn remapped_keys_fire_their_slot_and_never_collide() {
        let mut p = PlayerSettings::default();
        p.bind("grid.0", "KeyJ");
        assert_eq!(p.slot_of("KeyJ"), Some("grid.0"));
        assert_eq!(p.slot_of("KeyQ"), None);
        // Binding W (grid.1's key) to grid.0 swaps them.
        p.bind("grid.0", "KeyW");
        assert_eq!(p.key_of("grid.0"), "KeyW");
        assert_eq!(p.key_of("grid.1"), "KeyJ");
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(PlayerSettings::parse(&json).unwrap(), p);
        p.reset();
        assert_eq!(p.key_of("grid.1"), "KeyW");
    }

    #[test]
    fn unknown_slots_are_rejected() {
        assert!(PlayerSettings::parse(r#"{"keys": [["grid.99", "KeyQ"]]}"#).is_err());
    }

    #[test]
    fn key_labels_are_short() {
        assert_eq!(key_label("KeyQ"), "Q");
        assert_eq!(key_label("Digit3"), "3");
        assert_eq!(key_label("Escape"), "Esc");
    }
}
