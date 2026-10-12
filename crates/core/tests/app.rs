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
                assert!(
                    !label.ends_with('.') && label.split(' ').count() <= 3,
                    "{label}"
                );
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
        tool_core::app::to_pixels(&g.view, tool_core::fixed::Pos::of_cell(x, y))
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

// ---- play-test fixes (TRIAGE rows 2, 5, 6, 7, 8, 9, 13, 15, 20, 22, 25, 28) ----

fn notices(a: &mut App) -> Vec<String> {
    a.ui()["match"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["text"].as_str().unwrap().to_string())
        .collect()
}

fn button(a: &mut App, tab: usize, i: usize) -> Value {
    a.act(&format!("tab.{tab}"), "", false, false);
    a.ui()["match"]["grid"][i].clone()
}

fn rows_of(b: &Value) -> Vec<(String, String)> {
    b["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r[0].as_str().unwrap().to_string(),
                r[1].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn the_queue_bar_groups_a_species_per_queen_with_a_count_and_a_click_cancels_one() {
    let mut a = app();
    start_match(&mut a);
    // Three Harvester Ants: 300 - 3 x 50 = 150 Food, all from the one queen.
    for _ in 0..3 {
        a.act("grid.0", "", false, false);
    }
    a.advance(150);
    let q = a.ui()["match"]["queue"].clone();
    let q = q.as_array().unwrap();
    assert_eq!(q.len(), 1, "one item for the species: {q:?}");
    assert_eq!(q[0]["label"], "Harvester Ant ×3");
    assert_eq!(q[0]["count"], "×3");
    assert_eq!(a.game.as_ref().unwrap().world.colonies[0].stock[0], 150);
    // A click cancels one, refunded: 200 Food, and ×2 left.
    let id = q[0]["id"].as_str().unwrap().to_string();
    a.act(&id, "", false, false);
    a.advance(300);
    let q = a.ui()["match"]["queue"].clone();
    assert_eq!(q[0]["count"], "×2");
    assert_eq!(a.game.as_ref().unwrap().world.colonies[0].stock[0], 200);
}

#[test]
fn every_play_vs_ai_match_without_a_seed_gets_a_new_map() {
    let mut a = app();
    start_match(&mut a);
    let first = a.game.as_ref().unwrap().world.seed;
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    let second = a.game.as_ref().unwrap().world.seed;
    assert_ne!(first, second, "a fresh seed for each match");
    // A typed Seed still gives the same map every time.
    a.act("go.main_menu", "", false, false);
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.seed", "42", false, false);
    a.act("pva.start", "", false, false);
    assert_eq!(a.game.as_ref().unwrap().world.seed, 42);
}

#[test]
fn a_plain_click_replaces_waypoints_and_shift_click_adds_and_stays() {
    let mut a = app();
    start_match(&mut a);
    a.resize(1000, 800);
    a.act("tab.2", "", false, false);
    a.act("grid.1", "", false, false); // Soldier Ant
    let soldier = Species::Soldier.index();
    let count = |a: &App| a.game.as_ref().unwrap().world.colonies[0].waypoints[soldier].len();
    a.mouse("mousedown", 0, 470, 400, true);
    a.advance(150);
    a.mouse("mousedown", 0, 530, 400, true);
    a.advance(300);
    assert_eq!(count(&a), 2, "Shift + click adds");
    assert_eq!(
        a.game.as_ref().unwrap().mode,
        tool_core::app::Mode::Waypoint(Some(Species::Soldier)),
        "Shift + click stays in waypoint mode"
    );
    a.mouse("mousedown", 0, 500, 440, false);
    a.advance(450);
    assert_eq!(count(&a), 1, "a plain click replaces");
    assert_eq!(a.game.as_ref().unwrap().mode, tool_core::app::Mode::Idle);
    // The Hotkey row says how.
    let b = button(&mut a, 2, 1);
    let hotkey = rows_of(&b).into_iter().find(|r| r.0 == "Hotkey").unwrap().1;
    assert_eq!(
        hotkey,
        "W · Click: Set Waypoint · Shift Click: Add Waypoint"
    );
}

#[test]
fn seat_one_as_an_ai_makes_a_spectated_match() {
    let mut a = app();
    a.act("go.play_vs_ai", "", false, false);
    a.act("ai.0", "imago", false, false);
    a.act("pva.start", "", false, false);
    let g = a.game.as_ref().unwrap();
    assert!(g.spectate);
    assert!(g
        .world
        .colonies
        .iter()
        .all(|c| matches!(c.controller, tool_core::world::Controller::Ai(_))));
    let ui = a.ui();
    let m = &ui["match"];
    assert_eq!(m["spectate"], true);
    assert!(m["grid"].as_array().unwrap().is_empty(), "no action grid");
    let ids: Vec<&str> = m["controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["spectate.vision", "time.pause", "time.scale"]);
    // The whole map first, then Red's and Blue's Fog of War, then back.
    let mut seen = Vec::new();
    for _ in 0..3 {
        a.act("spectate.vision", "", false, false);
        seen.push(a.game.as_ref().unwrap().vision);
    }
    assert_eq!(seen, [Some(0), Some(1), None]);
    // A spectator issues nothing: a grid hotkey trains no ant.
    a.key("KeyQ", true, false, false);
    assert!(a.game.as_ref().unwrap().pending.is_empty());
    // You again for the next match.
    a.act("go.main_menu", "", false, false);
    a.act("go.play_vs_ai", "", false, false);
    a.act("ai.0", "you", false, false);
    a.act("pva.start", "", false, false);
    assert!(!a.game.as_ref().unwrap().spectate);
}

#[test]
fn pause_and_time_scale_run_a_local_sandbox_match_slower_or_faster() {
    let mut a = app();
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.sandbox", "on", false, false);
    a.act("pva.start", "", false, false);
    let tick = |a: &App| a.game.as_ref().unwrap().world.tick;
    let mut now = 0;
    let mut run = |a: &mut App, ms: i64| {
        for _ in 0..ms / 100 {
            now += 100;
            a.advance(now);
        }
    };
    // Normal Game Speed is 20 ticks per second, so 1 s at ×1 is 20 ticks.
    run(&mut a, 1000);
    assert_eq!(tick(&a), 20);
    a.act("time.scale", "", false, false); // ×2: 40 ticks
    run(&mut a, 1000);
    assert_eq!(tick(&a), 60);
    a.act("time.scale", "", false, false); // ×4: 80 ticks
    run(&mut a, 1000);
    assert_eq!(tick(&a), 140);
    a.act("time.scale", "", false, false); // ×½: 10 ticks
    run(&mut a, 1000);
    assert_eq!(tick(&a), 150);
    a.act("time.pause", "", false, false);
    run(&mut a, 1000);
    assert_eq!(tick(&a), 150, "Pause stops time");
    // A plain Play vs AI match can Pause (button, P, or the open Menu), but
    // has no Time Scale.
    let mut b = app();
    start_match(&mut b);
    let ids: Vec<String> = b.ui()["match"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["time.pause"]);
    b.key("KeyP", true, false, false);
    assert!(b.game.as_ref().unwrap().paused);
    run(&mut b, 1000);
    assert_eq!(tick(&b), 0, "P pauses a plain Play vs AI match");
    b.key("KeyP", true, false, false);
    b.key("Escape", true, false, false);
    run(&mut b, 1000);
    assert_eq!(tick(&b), 0, "the open Menu pauses too");
}

#[test]
fn hill_announcements_give_the_reward_and_the_winner_by_colour() {
    let mut a = app();
    start_match(&mut a);
    {
        let w = &mut a.game.as_mut().unwrap().world;
        w.announce("announce.hill", None, None);
        w.announce("announce.hill_won.1", None, None);
        // An open Hill: 32 s left, Blue (team 2) has held it longest.
        w.events.hill = Some(tool_core::world::Hill {
            x: 0,
            y: 0,
            r: 4,
            ends: w.tick + 32 * tool_core::rules::TICKS_PER_SECOND,
            hold: vec![2, 5],
        });
    }
    let n = notices(&mut a);
    assert!(n.contains(&"Hill Won: Blue · +300 Food · +150 Nectar · +150 Leaves".to_string()));
    assert!(n.contains(&"Event: Hill · Reward: 300 Food · 150 Nectar · 150 Leaves".to_string()));
    let stats = a.ui()["match"]["hud"]["stats"].clone();
    let hill = stats
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "hill")
        .expect("the Hill shows in the top bar")
        .clone();
    assert_eq!(hill["label"], "Hill");
    assert_eq!(hill["value"], "0:32 · Blue");
}

#[test]
fn a_2v2_hill_winner_is_named_by_team() {
    let mut a = app();
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.format", "two_v_two", false, false);
    a.act("pva.start", "", false, false);
    a.game
        .as_mut()
        .unwrap()
        .world
        .announce("announce.hill_won.1", None, None);
    let n = notices(&mut a);
    assert!(n[0].starts_with("Hill Won: Team 2 · +300 Food"), "{n:?}");
}

#[test]
fn host_fills_in_a_random_room_code_and_typed_codes_are_upper_cased() {
    let mut a = app();
    a.act("go.online", "", false, false);
    a.act("online.room", "k7mxp4", false, false);
    assert_eq!(a.online.room, "K7MXP4");
    a.act("online.room", "", false, false);
    a.act("online.host", "", false, false);
    let code = a.online.room.clone();
    assert_eq!(code.len(), 6);
    assert!(
        code.chars()
            .all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c)),
        "{code}"
    );
    a.act("lobby.leave", "", false, false);
    a.act("online.room", "", false, false);
    a.act("online.host", "", false, false);
    assert_ne!(a.online.room, code, "each Host gets a new code");
    // Join with no code says so, as label and value.
    a.act("lobby.leave", "", false, false);
    a.act("online.room", "", false, false);
    a.act("online.join", "", false, false);
    assert_eq!(a.online.status, "Room Code: None");
}

#[test]
fn the_online_screen_shows_pasted_codes_only_with_the_codes_flag() {
    let ids = |a: &mut App| a.ui()["panels"].to_string();
    let mut a = app();
    a.act("go.online", "", false, false);
    let plain = ids(&mut a);
    assert!(plain.contains("online.room") && plain.contains("online.host"));
    assert!(!plain.contains("online.invite") && !plain.contains("online.host_code"));
    a.act("flag.codes", "on", false, false);
    let codes = ids(&mut a);
    assert!(codes.contains("online.invite") && codes.contains("online.host_code"));
}

#[test]
fn queen_and_brood_cell_tooltips_have_an_effect_row_and_build_buttons_a_tier() {
    let mut a = app();
    start_match(&mut a);
    let queen = button(&mut a, 1, 0);
    assert!(rows_of(&queen).contains(&(
        "Effect".into(),
        "Train Harvester Ant, Soldier Ant, Carpenter Ant · Drop-off · Housing +20".into()
    )));
    let brood = button(&mut a, 1, 1);
    assert!(rows_of(&brood).contains(&("Effect".into(), "Housing +8".into())));
    // Tier badges: Queen 1, Acid Ant Hatchery (D) 2, Dinosaur Ant Hatchery 3.
    assert_eq!(queen["tier"], "T1");
    assert_eq!(button(&mut a, 1, 6)["tier"], "T2");
    assert_eq!(button(&mut a, 1, 11)["tier"], "T3");
    // Ants and Upgrades too: Fire Ant is Tier 2; Damage I is Tier 1.
    assert_eq!(button(&mut a, 0, 4)["tier"], "T2");
    assert_eq!(button(&mut a, 3, 0)["tier"], "T1");
    // A Queen costs 250 Food: with 100 the button is greyed and says why.
    a.game.as_mut().unwrap().world.colonies[0].stock[0] = 100;
    let queen = button(&mut a, 1, 0);
    assert_eq!(queen["state"], "unaffordable");
    assert_eq!(queen["state_text"], "Not Enough Food");
}

#[test]
fn all_ants_shows_all_and_auto_scout_toggles_on_the_colony_tab() {
    let mut a = app();
    start_match(&mut a);
    let all = button(&mut a, 2, 0);
    assert_eq!(all["icon_text"], "ALL");
    assert_eq!(all["icon"], "");
    let scout = button(&mut a, 4, 9);
    assert_eq!(scout["label"], "Auto-Scout");
    // Auto-Scout is on from the start (TRIAGE row 45); a click turns it off.
    assert_eq!(scout["active"], true);
    a.act("grid.9", "", false, false);
    a.advance(150);
    assert!(!a.game.as_ref().unwrap().world.colonies[0].auto_scout);
    let scout = button(&mut a, 4, 9);
    assert_eq!(scout["active"], false);
    assert!(rows_of(&scout).contains(&("Status".into(), "Off".into())));
}

#[test]
fn map_revealed_and_sandbox_are_match_settings() {
    let mut a = app();
    a.act("go.match_settings", "", false, false);
    a.act("def.map_revealed", "on", false, false);
    a.act("def.sandbox", "on", false, false);
    assert!(a.defaults.map_revealed && a.defaults.sandbox);
    let form = a.ui()["panels"].to_string();
    assert!(form.contains("def.map_revealed") && form.contains("def.sandbox"));
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    let s = &a.game.as_ref().unwrap().world.settings;
    assert!(s.map_revealed && s.sandbox);
}

#[test]
fn tooltips_show_real_seconds_whatever_the_tick_rate() {
    let mut a = app();
    start_match(&mut a);
    a.act("tab.0", "", false, false);
    let ui = a.ui();
    let soldier = &ui["match"]["grid"][1];
    let rows = soldier["rows"].as_array().unwrap();
    let get = |label: &str| {
        rows.iter()
            .find(|r| r[0] == label)
            .map(|r| r[1].as_str().unwrap().to_string())
            .unwrap_or_default()
    };
    // A Soldier Ant attacks once a second (rules.rs: ticks(10)).
    assert_eq!(get("Attack Speed"), "1.0s");
    // A Harvester Ant gathers one resource every 0.3 s: 3.3 per second.
    let harvester = &ui["match"]["grid"][0];
    let hr: Vec<_> = harvester["rows"].as_array().unwrap().clone();
    let rate = hr.iter().find(|r| r[0] == "Gather Rate").unwrap()[1]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(rate, "3.3/s");
}

#[test]
fn map_type_is_a_saved_match_setting_that_reaches_the_match() {
    let mut a = app();
    a.act("go.match_settings", "", false, false);
    a.act("def.map_type", "maze", false, false);
    let saved = a.ui()["save"]["defaults"].as_str().unwrap().to_string();
    assert!(saved.contains("\"map_type\":\"maze\""), "{saved}");
    a.act("def.map_type", "nonsense", false, false);
    assert_eq!(a.defaults.map_type, tool_core::settings::MapType::Maze);
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    assert_eq!(
        a.game.as_ref().unwrap().world.settings.map_type,
        tool_core::settings::MapType::Maze
    );
}

// ---- play-test fixes (TRIAGE rows 39, 48, 53, 55, 56, 57) ----

fn stat(a: &mut App, key: &str) -> String {
    let ui = a.ui();
    ui["match"]["hud"]["stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .map(|s| s["value"].as_str().unwrap().to_string())
        .unwrap_or_default()
}

/// Runs the match `ms` milliseconds of real time, 100 ms per frame.
fn run(a: &mut App, ms: i64) {
    for _ in 0..ms / 100 {
        let now = a.now_ms + 100;
        a.advance(now);
    }
}

#[test]
fn the_top_bar_counts_combat_ants_out_now_apart_from_the_queue() {
    let mut a = app();
    start_match(&mut a);
    // A colony starts with 2 Soldier Ants, Pop Cost 1 each; Harvester and
    // Carpenter Ants are not combat ants.
    assert_eq!(stat(&mut a, "combat_ants"), "2 · Pop Cost 2");
    // A queued Soldier Ant is not counted until it is trained.
    a.act("tab.0", "", false, false);
    a.act("grid.1", "", false, false);
    run(&mut a, 300);
    assert_eq!(a.ui()["match"]["queue"].as_array().unwrap().len(), 1);
    assert_eq!(stat(&mut a, "combat_ants"), "2 · Pop Cost 2");
    run(&mut a, 20_000);
    assert_eq!(stat(&mut a, "combat_ants"), "3 · Pop Cost 3");
}

#[test]
fn queued_upgrades_show_rank_and_stat_and_tell_colony_from_species() {
    let mut a = app();
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.sandbox", "on", false, false);
    a.act("pva.start", "", false, false);
    a.act("tab.3", "", false, false);
    a.act("grid.0", "", false, false); // Damage: a Colony Upgrade
    a.act("grid.1", "", false, false); // Soldier Ant Health: a Species Upgrade
    run(&mut a, 200);
    let q = a.ui()["match"]["queue"].as_array().unwrap().clone();
    let find = |label: &str| {
        q.iter()
            .find(|i| i["label"] == label)
            .unwrap_or_else(|| panic!("{label} in {q:?}"))
            .clone()
    };
    let damage = find("Damage I");
    assert_eq!(damage["kind"], "colony_upgrade");
    assert_eq!(damage["rank"], "I");
    assert_eq!(damage["short"], "Damage");
    let health = find("Soldier Ant Health I");
    assert_eq!(health["kind"], "species_upgrade");
    assert_eq!(health["rank"], "I");
    assert_eq!(health["short"], "Health");
    let rows = rows_of(&damage);
    assert!(rows.contains(&("Rank".into(), "I".into())), "{rows:?}");
    assert!(rows.contains(&("Upgrade".into(), "Colony Upgrade".into())));
    assert!(rows_of(&health).contains(&("Upgrade".into(), "Species Upgrade".into())));
}

#[test]
fn tab_sets_the_all_ants_waypoint_from_any_grid_tab_and_can_be_remapped() {
    let mut a = app();
    start_match(&mut a);
    a.resize(1000, 800);
    a.act("tab.0", "", false, false);
    a.key("Tab", true, false, false);
    {
        let g = a.game.as_ref().unwrap();
        assert_eq!(g.mode, tool_core::app::Mode::Waypoint(None));
        assert_eq!(g.tab, 0, "the Ants tab stays open");
    }
    assert_eq!(a.ui()["match"]["mode"], "waypoint");
    a.mouse("mousedown", 0, 400, 400, false);
    run(&mut a, 200);
    let c = &a.game.as_ref().unwrap().world.colonies[0];
    // Every combat species gets the waypoint; Harvester Ants do not.
    assert!(!c.waypoints[Species::Soldier.index()].is_empty());
    assert!(!c.waypoints[Species::Fire.index()].is_empty());
    assert!(c.waypoints[Species::Harvester.index()].is_empty());
    // The All Ants button's Hotkey row names Tab too.
    let b = button(&mut a, 2, 0);
    let hotkey = rows_of(&b)
        .into_iter()
        .find(|(l, _)| l == "Hotkey")
        .unwrap()
        .1;
    assert!(hotkey.starts_with("Q · Tab · "), "{hotkey}");
    // Remapped in Keybinds, and saved.
    a.act("go.main_menu", "", false, false);
    a.act("go.player_settings", "", false, false);
    a.act("bind.all_ants", "", false, false);
    assert_eq!(a.ui()["keys_captured"], true);
    a.key("KeyG", true, false, false);
    assert_eq!(a.player.key_of("all_ants"), "KeyG");
    let ui = a.ui();
    assert_eq!(ui["keys_captured"], false);
    assert!(ui["save"]["player"].as_str().unwrap().contains("KeyG"));
}

#[test]
fn edge_pan_speed_is_a_saved_player_setting_that_scales_edge_pan() {
    // The view's move over 100 ms with the pointer at the left or right
    // edge, toward the middle of the map so it never meets the map's side.
    let moved = |speed: &str| {
        let mut a = app();
        a.act("go.player_settings", "", false, false);
        a.act("player.edge_pan_speed", speed, false, false);
        let saved = a.ui()["save"]["player"].as_str().unwrap().to_string();
        assert!(saved.contains("\"edge_pan_speed\""), "{saved}");
        a.act("go.main_menu", "", false, false);
        start_match(&mut a);
        a.resize(1000, 800);
        let g = a.game.as_ref().unwrap();
        let left_half = g.view.centre_x < g.world.map.w * tool_core::fixed::CELL / 2;
        let x = if left_half { 999 } else { 1 };
        a.mouse("mousemove", 0, x, 400, false);
        let before = a.game.as_ref().unwrap().view.centre_x;
        run(&mut a, 100);
        (a.game.as_ref().unwrap().view.centre_x - before).abs()
    };
    let slow = moved("very_slow");
    let fast = moved("very_fast");
    // Very Slow is 300 pixels a second and Very Fast 1,800: six times as far.
    assert!(slow > 0);
    assert!((fast - 6 * slow).abs() <= 6, "slow {slow}, fast {fast}");
    // An unknown value changes nothing.
    let mut a = app();
    a.act("player.edge_pan_speed", "warp", false, false);
    assert_eq!(a.player.edge_pan_speed, 2);
}

#[test]
fn keybinds_name_tabs_and_buttons_as_in_the_match() {
    let mut a = app();
    a.act("go.player_settings", "", false, false);
    let ui = a.ui();
    let items = ui["panels"][0]["items"].as_array().unwrap();
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    for want in [
        "Action Button Q",
        "Action Button D",
        "Action Button V",
        "Grid Tab: Ants",
        "Grid Tab: Build",
        "Grid Tab: Waypoints",
        "Grid Tab: Upgrades",
        "Grid Tab: Colony",
        "Set Waypoint: All Ants",
        "Edge Pan Speed",
    ] {
        assert!(labels.contains(&want), "{want} in {labels:?}");
    }
    assert!(!labels.iter().any(|l| l.starts_with("Grid Tab 1")));
    // The All Ants hotkey is listed right after the five tabs.
    let at = |l: &str| labels.iter().position(|x| *x == l).unwrap();
    assert_eq!(at("Set Waypoint: All Ants"), at("Grid Tab: Colony") + 1);
}

#[test]
fn the_end_screen_graphs_every_colony_and_summarises_each() {
    let mut a = app();
    start_match(&mut a);
    // 35 seconds of match: Samples at 10, 20 and 30 s.
    {
        let g = a.game.as_mut().unwrap();
        for _ in 0..tool_core::world::SAMPLE_TICKS * 7 / 2 {
            g.world.step(&[], &[]);
        }
    }
    a.key("Escape", true, false, false);
    a.act("menu.concede", "", false, false);
    a.act("menu.concede_yes", "", false, false);
    run(&mut a, 300);
    let ui = a.ui();
    let end = &ui["match"]["end"];
    assert_eq!(end["title"], "Defeat");
    let tabs: Vec<&str> = end["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        tabs,
        [
            "Food",
            "Nectar",
            "Leaves",
            "Army",
            "Population",
            "Killed",
            "Lost"
        ]
    );
    assert_eq!(end["tabs"][0]["active"], true);
    let graph = &end["graph"];
    assert_eq!(graph["title"], "Gathered: Food");
    let (gw, gh) = (tool_core::app::GRAPH_W, tool_core::app::GRAPH_H);
    assert_eq!(graph["view_box"], format!("0 0 {gw} {gh}"));
    // One line per colony, Red then Blue, in its player color.
    let lines = graph["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["colour"], "#f0302a");
    assert_eq!(lines[1]["colour"], "#2f7cff");
    for l in lines {
        let d = l["d"].as_str().unwrap();
        assert!(d.starts_with("M "), "{d}");
        // The start (0 Food gathered at 0:00) and three Samples.
        let nums: Vec<i32> = d.split(' ').filter_map(|t| t.parse().ok()).collect();
        assert_eq!(nums.len(), 8, "{d}");
        for p in nums.chunks(2) {
            assert!((0..=gw).contains(&p[0]) && (0..=gh).contains(&p[1]), "{d}");
        }
        // Time runs left to right.
        assert!(nums[0] < nums[2] && nums[2] < nums[4] && nums[4] < nums[6]);
    }
    // Every colony's summary, You marked.
    let cols: Vec<&str> = end["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap())
        .collect();
    assert_eq!(cols, ["Red · You", "Blue"]);
    let summary = end["summary"].as_array().unwrap();
    let labels: Vec<&str> = summary
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "Trained",
            "Killed",
            "Lost",
            "Gathered: Food",
            "Gathered: Nectar",
            "Gathered: Leaves"
        ]
    );
    assert!(summary
        .iter()
        .all(|r| r["values"].as_array().unwrap().len() == 2));
    // The Population tab: a round scale (1, 2 or 5 times a power of ten)
    // at least the largest Population in the match.
    a.act("end.graph.4", "", false, false);
    let ui = a.ui();
    let end = &ui["match"]["end"];
    assert_eq!(end["tabs"][4]["active"], true);
    assert_eq!(end["graph"]["title"], "Population");
    let g = a.game.as_ref().unwrap();
    let most = g
        .world
        .colonies
        .iter()
        .flat_map(|c| c.history.iter().map(|s| s.population))
        .max()
        .unwrap();
    // The scale's labels sit left of the plot, right-aligned.
    let scale: Vec<i32> = end["graph"]["labels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["anchor"] == "end" && l["x"].as_i64().unwrap() < 60)
        .map(|l| l["text"].as_str().unwrap().parse().unwrap())
        .collect();
    let top = *scale.iter().max().unwrap();
    assert!(top >= most && top < most * 5, "scale {top} for {most}");
    // Half the scale is marked only when it is a whole number.
    assert_eq!(scale.contains(&(top / 2)), top % 2 == 0, "{scale:?}");
    let digits = top.to_string();
    assert!(["1", "2", "5"].contains(&&digits[..1]) && digits[1..].chars().all(|c| c == '0'));
    a.act("end.graph.3", "", false, false);
    assert_eq!(a.ui()["match"]["end"]["graph"]["title"], "Army: Pop Cost");
}

#[test]
fn training_with_no_housing_room_plays_a_warning_once() {
    let mut a = app();
    start_match(&mut a);
    {
        // Fill the queen's 20 housing: 12 ants now + 10 queued.
        let g = a.game.as_mut().unwrap();
        g.world.colonies[0].stock = [5000, 0, 0];
    }
    a.act("tab.0", "", false, false);
    a.act("grid.1", "", true, false); // Train 10 Soldier Ants: 12 < 20 when pressed
    a.advance(100);
    assert!(
        a.ui().get("sounds").is_none(),
        "no warning while there is room"
    );
    a.act("grid.1", "", false, false); // one more: no room
    let ui = a.ui();
    assert_eq!(ui["sounds"], serde_json::json!(["warning"]));
    // Pressed again at once: the cooldown keeps it from repeating.
    a.act("grid.1", "", false, false);
    assert!(a.ui().get("sounds").is_none());
    // The sound itself is a WAV file at the Player Settings volume.
    assert_eq!(&a.sound("warning")[0..4], b"RIFF");
    a.act("player.volume", "0", false, false);
    assert!(
        a.sound("warning")[44..].iter().all(|b| *b == 0),
        "Volume Off is silent"
    );
}

#[test]
fn volume_is_a_slider_in_player_settings_and_the_match_menu() {
    let mut a = app();
    a.act("go.player_settings", "", false, false);
    let ui = a.ui();
    let sliders: Vec<&serde_json::Value> = ui["panels"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["items"].as_array().unwrap().iter())
        .filter(|i| i["type"] == "slider")
        .collect();
    assert_eq!(sliders.len(), 1);
    assert_eq!(sliders[0]["id"], "player.volume");
    assert_eq!(sliders[0]["value"], 70, "Normal by default");
    a.act("player.volume", "45", false, false);
    assert_eq!(a.player.volume, 45);
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    a.key("Escape", true, false, false);
    let ui = a.ui();
    let menu = ui["match"]["menu"]["items"].as_array().unwrap();
    assert!(
        menu.iter()
            .any(|i| i["type"] == "slider" && i["value"] == 45),
        "the in-match Menu has the Volume slider: {menu:?}"
    );
    // Settings saved with the old 4-step Volume convert to percent.
    let old = tool_core::keys::PlayerSettings::parse(r#"{"volume": 1}"#).unwrap();
    assert_eq!(old.volume, 35);
}
