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

/// Slots added after the first release, listed after the others.
/// `all_ants` (TRIAGE row 55): the All Ants waypoint from any grid tab.
pub const EXTRA_SLOTS: [&str; 3] = ["pan.down", "pause", "all_ants"];

/// Edge Pan Speed (TRIAGE row 56): the word key shown in Player Settings,
/// and the pixels per second Edge Pan moves the view at each.
pub const EDGE_PAN_SPEEDS: [(&str, i32); 5] = [
    ("very_slow", 300),
    ("slow", 600),
    ("normal", 900),
    ("fast", 1350),
    ("very_fast", 1800),
];

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
        "all_ants" => "Tab",
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
    /// An index into `EDGE_PAN_SPEEDS`; settings saved before it was added
    /// read as Normal.
    pub edge_pan_speed: usize,
    /// Sound volume in percent, 0–100, set with a slider. Settings saved
    /// when it was a 4-step choice (0–3) are converted on load.
    pub volume: u8,
    /// Player Name, shown in the Lobby and in Waiting On; empty means the
    /// default ("Player").
    pub name: String,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        PlayerSettings {
            keys: Vec::new(),
            edge_pan: true,
            edge_pan_speed: 2,
            volume: crate::sound::DEFAULT_VOLUME,
            name: String::new(),
        }
    }
}

impl PlayerSettings {
    pub fn parse(json: &str) -> Result<PlayerSettings, String> {
        let mut p: PlayerSettings = serde_json::from_str(json).map_err(|e| e.to_string())?;
        // The old 4-step Volume (Off, Low, Normal, High) as percent.
        if p.volume <= 3 {
            p.volume = crate::sound::volume_percent(p.volume) as u8;
        }
        p.volume = p.volume.min(100);
        for (slot, _) in &p.keys {
            if !all_slots().contains(&slot.as_str()) {
                return Err(format!("Keybinds: unknown slot {slot}"));
            }
        }
        if p.edge_pan_speed >= EDGE_PAN_SPEEDS.len() {
            return Err("Edge Pan Speed: out of range".into());
        }
        Ok(p)
    }

    /// Pixels per second Edge Pan moves the view.
    pub fn edge_pan_px(&self) -> i32 {
        EDGE_PAN_SPEEDS[self.edge_pan_speed.min(EDGE_PAN_SPEEDS.len() - 1)].1
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

/// The label of a slot's default key: grid slots are named by it in
/// Keybinds ("Action Button Q"), wherever they are bound now.
pub fn default_label(slot: &str) -> String {
    key_label(default_key(slot))
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
        assert!(PlayerSettings::parse(r#"{"edge_pan_speed": 9}"#).is_err());
    }

    #[test]
    fn tab_sets_the_all_ants_waypoint_and_old_settings_still_read() {
        let p = PlayerSettings::default();
        assert_eq!(p.slot_of("Tab"), Some("all_ants"));
        assert_eq!(key_label("Tab"), "Tab");
        // Saved before Edge Pan Speed existed: Normal, 900 pixels a second.
        let old = PlayerSettings::parse(r#"{"keys": [], "edge_pan": false}"#).unwrap();
        assert_eq!(old.edge_pan_px(), 900);
    }

    #[test]
    fn key_labels_are_short() {
        assert_eq!(key_label("KeyQ"), "Q");
        assert_eq!(key_label("Digit3"), "3");
        assert_eq!(key_label("Escape"), "Esc");
    }
}
