//! The application layer: menus, settings, keybinds, the action grid and
//! running a match from the menus.

use serde_json::Value;
use tool_core::app::{App, Screen};
use tool_core::rules::Species;

fn app() -> App {
    let strings = include_str!("../../../data/strings.json");
    let glossary = include_str!("../../../data/glossary.json");
    App::new(strings, glossary, "", "", "me").expect("data files parse")
}

fn start_match(a: &mut App) {
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.start", "", false, false);
    assert_eq!(a.screen, Screen::Match);
}

#[test]
fn every_tooltip_is_label_value_pairs() {
    let mut a = app();
    start_match(&mut a);
    for tab in 0..5 {
        a.act(&format!("tab.{tab}"), "", false, false);
        let ui = a.ui();
        for b in ui["match"]["grid"].as_array().unwrap() {
            if b["empty"] == Value::Bool(true) {
                continue;
            }
            let rows = b["rows"].as_array().unwrap();
            assert!(!rows.is_empty(), "tab {tab} {} has a tooltip", b["label"]);
            for r in rows {
                let pair = r.as_array().unwrap();
                assert_eq!(pair.len(), 2, "{r}");
                let label = pair[0].as_str().unwrap();
                // A label is a short name, never a sentence.
                assert!(!label.ends_with('.') && label.split(' ').count() <= 3, "{label}");
            }
        }
    }
}

#[test]
fn the_hotkey_q_trains_a_harvester_and_ctrl_trains_ten() {
    let mut a = app();
    start_match(&mut a);
    a.key("Digit1", true, false, false);
    a.key("KeyQ", true, false, false);
    a.advance(200);
    let g = a.game.as_ref().unwrap();
    // 300 Food - 50 for one Harvester Ant.
    assert_eq!(g.world.colonies[0].stock[0], 250);
    a.key("KeyQ", true, true, false);
    a.advance(400);
    let g = a.game.as_ref().unwrap();
    // 250 / 50 = 5 more: Train 10 queues what can be paid for.
    assert_eq!(g.world.colonies[0].stock[0], 0);
}

#[test]
fn remapped_keys_fire_their_action_and_are_saved() {
    let mut a = app();
    a.act("go.player_settings", "", false, false);
    a.act("bind.grid.0", "", false, false);
    a.key("KeyJ", true, false, false);
    let ui = a.ui();
    let saved = ui["save"]["player"].as_str().expect("settings saved");
    assert!(saved.contains("KeyJ"));
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    a.key("Digit1", true, false, false);
    a.key("KeyJ", true, false, false);
    a.advance(200);
    assert_eq!(a.game.as_ref().unwrap().world.colonies[0].stock[0], 250);
    // A new App built from the saved string keeps the binding.
    let strings = include_str!("../../../data/strings.json");
    let glossary = include_str!("../../../data/glossary.json");
    let b = App::new(strings, glossary, "", saved, "me").unwrap();
    assert_eq!(b.player.key_of("grid.0"), "KeyJ");
}

#[test]
fn default_match_settings_are_saved_and_used_by_play_vs_ai() {
    let mut a = app();
    a.act("go.match_settings", "", false, false);
    a.act("def.format", "free_for_all", false, false);
    a.act("def.disabled.acid", "on", false, false);
    let ui = a.ui();
    let saved = ui["save"]["defaults"].as_str().unwrap().to_string();
    assert!(saved.contains("free_for_all") && saved.contains("acid"));
    // An invalid value is refused and nothing changes.
    a.act("def.tunnel_width", "3", false, false);
    assert_eq!(a.defaults.tunnel_width, 12);
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    let g = a.game.as_ref().unwrap();
    assert_eq!(g.world.colonies.len(), 4);
    assert!(g.world.settings.is_disabled(Species::Acid));
}

#[test]
fn placing_a_brood_cell_by_mouse_makes_a_blueprint() {
    let mut a = app();
    start_match(&mut a);
    a.resize(1000, 800);
    a.act("tab.1", "", false, false);
    a.act("grid.1", "", false, false); // Brood Cell
    // A free spot near the queen, clicked at its screen position.
    let (sx, sy) = {
        let g = a.game.as_ref().unwrap();
        let q = g.world.queens(0).next().unwrap().pos;
        let (x, y) = tool_core::ai::find_spot(
            &g.world,
            0,
            tool_core::rules::StructureKind::BroodCell,
            q,
            6,
        )
        .unwrap();
        g.view.to_screen(tool_core::fixed::Pos::of_cell(x, y))
    };
    a.mouse("mousemove", 0, sx, sy, false);
    a.mouse("mousedown", 0, sx, sy, false);
    a.advance(150);
    let g = a.game.as_ref().unwrap();
    assert!(
        g.world.structures.iter().any(|s| !s.complete),
        "a blueprint was placed"
    );
}

#[test]
fn right_click_clears_a_waypoint() {
    let mut a = app();
    start_match(&mut a);
    a.resize(1000, 800);
    a.act("tab.2", "", false, false);
    a.act("grid.0", "", false, false); // All Ants
    a.mouse("mousedown", 0, 400, 400, false);
    a.advance(150);
    assert!(a.game.as_ref().unwrap().world.colonies[0]
        .waypoints
        .iter()
        .any(|w| !w.is_empty()));
    a.mouse("mousedown", 2, 400, 400, false);
    a.advance(300);
    assert!(a.game.as_ref().unwrap().world.colonies[0]
        .waypoints
        .iter()
        .all(|w| w.is_empty()));
}

#[test]
fn escape_cancels_placement_before_opening_the_menu() {
    let mut a = app();
    start_match(&mut a);
    a.act("tab.1", "", false, false);
    a.act("grid.1", "", false, false);
    a.key("Escape", true, false, false);
    assert!(!a.game.as_ref().unwrap().menu_open);
    a.key("Escape", true, false, false);
    assert!(a.game.as_ref().unwrap().menu_open);
}
