//! Match History (TRIAGE row 88): every finished match is recorded once,
//! newest first, kept to 50, handed to the page as text to store, and read
//! back on load. Expected values are worked out from the request: a
//! conceded 1v1 against one Pupa AI is "Conceded", 1v1, two colonies, Red
//! is You and Blue is AI · Pupa.

use serde_json::Value;
use tool_core::app::{App, Screen};
use tool_core::history::{MatchResult, Stored, HISTORY_KEEP, HISTORY_VERSION};

fn app() -> App {
    let strings = include_str!("../../../data/strings.json");
    let glossary = include_str!("../../../data/glossary.json");
    App::new(strings, glossary, "", "", "me").expect("data files parse")
}

fn run(a: &mut App, ms: i64) {
    for _ in 0..ms / 100 {
        let now = a.now_ms + 100;
        a.advance(now);
    }
}

fn start_match(a: &mut App) {
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.start", "", false, false);
    assert_eq!(a.screen, Screen::Match);
}

fn concede(a: &mut App) {
    a.key("Escape", true, false, false);
    a.act("menu.concede", "", false, false);
    a.act("menu.concede_yes", "", false, false);
    run(a, 300);
}

/// Ends the current match with every colony but Red's defeated.
fn red_wins(a: &mut App) {
    let g = a.game.as_mut().unwrap();
    for c in 1..g.world.colonies.len() as u8 {
        g.world.defeat(c);
    }
    g.world.step(&[], &[]);
    assert!(g.world.winner.is_some());
    run(a, 200);
}

/// The text the page would store, from one `ui()` call.
fn saved(ui: &Value) -> Option<String> {
    ui["save_history"].as_str().map(|s| s.to_string())
}

fn panel_texts(ui: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(&ui["panels"], &mut out);
    out
}

#[test]
fn a_conceded_match_is_recorded_once_with_its_summary() {
    let mut a = app();
    start_match(&mut a);
    concede(&mut a);
    let ui = a.ui();
    assert_eq!(ui["match"]["end"]["title"], "Defeat");
    let text = saved(&ui).expect("the page is handed the history to store");
    // Later frames record nothing more.
    for _ in 0..5 {
        assert!(saved(&a.ui()).is_none());
    }
    let s = Stored::parse(&text);
    assert_eq!(s.version, HISTORY_VERSION);
    assert_eq!(s.matches.len(), 1);
    let m = &s.matches[0];
    assert_eq!(m.number, 1);
    assert_eq!(m.result, MatchResult::Conceded);
    assert_eq!((m.format.as_str(), m.online), ("one_v_one", false));
    assert_eq!(m.colonies.len(), 2);
    assert!(m.colonies[0].you && m.colonies[0].ai.is_none());
    assert_eq!(m.colonies[1].ai.as_deref(), Some("pupa"));
    // The summary numbers are the end screen's: Trained counts nothing
    // before the first ant is trained.
    assert_eq!(m.colonies[0].trained, 0);
    assert_eq!(m.winner, Some(m.colonies[1].team));
    assert!(m.ticks > 0);
}

#[test]
fn a_win_and_a_spectated_match_have_their_results() {
    let mut a = app();
    start_match(&mut a);
    red_wins(&mut a);
    let s = Stored::parse(&saved(&a.ui()).unwrap());
    assert_eq!(s.matches[0].result, MatchResult::Victory);

    a.act("end.main_menu", "", false, false);
    a.act("go.play_vs_ai", "", false, false);
    a.act("ai.0", "imago", false, false);
    a.act("pva.start", "", false, false);
    red_wins(&mut a);
    let s = Stored::parse(&saved(&a.ui()).unwrap());
    assert_eq!(s.matches.len(), 2);
    let m = &s.matches[0];
    assert_eq!((m.number, m.result), (2, MatchResult::Spectate));
    assert_eq!(m.colonies[0].ai.as_deref(), Some("imago"));
    assert!(!m.colonies[0].you);

    a.act("end.main_menu", "", false, false);
    a.act("go.match_history", "", false, false);
    let ui = a.ui();
    assert_eq!(ui["screen"], "match_history");
    let texts = panel_texts(&ui);
    // Newest first; the spectated match names its winner.
    let at = |t: &str| texts.iter().position(|x| x == t).unwrap();
    assert!(at("Match 2") < at("Match 1"));
    assert!(texts.contains(&"Victory: Red".to_string()), "{texts:?}");
    assert!(texts.contains(&"Red · AI · Imago".to_string()), "{texts:?}");
    assert!(texts.contains(&"Spectate".to_string()));
    // Clicking Match 1 shows its summary.
    a.act("history.show.1", "", false, false);
    let texts = panel_texts(&a.ui());
    assert!(texts.contains(&"Red · You".to_string()), "{texts:?}");
    assert!(texts.contains(&"Blue · AI · Pupa".to_string()), "{texts:?}");
    assert!(texts.contains(&"Play vs AI".to_string()));
}

#[test]
fn the_history_survives_a_reload_through_the_page() {
    let mut a = app();
    start_match(&mut a);
    concede(&mut a);
    let text = saved(&a.ui()).unwrap();
    // A new visit: the page passes the stored text back.
    let mut b = app();
    b.act("history.load", &text, false, false);
    start_match(&mut b);
    concede(&mut b);
    let s = Stored::parse(&saved(&b.ui()).unwrap());
    let numbers: Vec<u32> = s.matches.iter().map(|m| m.number).collect();
    assert_eq!(numbers, [2, 1]);
}

#[test]
fn unreadable_or_other_version_text_gives_an_empty_history() {
    for text in ["", "not json", r#"{"version":999,"next":4,"matches":[]}"#] {
        let s = Stored::parse(text);
        assert!(s.matches.is_empty() && s.next == 1, "{text}");
    }
}

#[test]
fn only_the_newest_fifty_are_kept_and_clear_keeps_counting() {
    let mut a = app();
    for _ in 0..HISTORY_KEEP + 3 {
        start_match(&mut a);
        concede(&mut a);
        a.ui();
        a.act("end.main_menu", "", false, false);
    }
    let s = &a.match_history.stored;
    assert_eq!(s.matches.len(), HISTORY_KEEP);
    assert_eq!(s.matches[0].number, 53);
    assert_eq!(s.matches[HISTORY_KEEP - 1].number, 4);

    // Ten to a page: Older and Newer turn pages.
    a.act("go.match_history", "", false, false);
    let first = panel_texts(&a.ui());
    assert!(first.contains(&"Match 44".to_string()) && !first.contains(&"Match 43".to_string()));
    a.act("history.older", "", false, false);
    let second = panel_texts(&a.ui());
    assert!(second.contains(&"Match 43".to_string()) && !second.contains(&"Match 44".to_string()));

    // Clear asks first; Back keeps everything.
    a.act("history.clear", "", false, false);
    assert!(panel_texts(&a.ui()).contains(&"Clear the Match History?".to_string()));
    a.act("history.clear_no", "", false, false);
    assert_eq!(a.match_history.stored.matches.len(), HISTORY_KEEP);
    a.act("history.clear", "", false, false);
    a.act("history.clear_yes", "", false, false);
    let s = Stored::parse(&saved(&a.ui()).unwrap());
    assert!(s.matches.is_empty());
    a.act("go.main_menu", "", false, false);
    start_match(&mut a);
    concede(&mut a);
    let s = Stored::parse(&saved(&a.ui()).unwrap());
    assert_eq!(s.matches[0].number, 54);
}

#[test]
fn leaving_a_2v2_after_conceding_records_it() {
    let mut a = app();
    a.act("go.play_vs_ai", "", false, false);
    a.act("pva.format", "two_v_two", false, false);
    a.act("pva.start", "", false, false);
    concede(&mut a);
    // The ally fights on: no winner yet, nothing recorded.
    assert!(a.game.as_ref().unwrap().world.winner.is_none());
    assert!(saved(&a.ui()).is_none());
    a.key("Escape", true, false, false);
    a.act("end.main_menu", "", false, false);
    let s = Stored::parse(&saved(&a.ui()).unwrap());
    assert_eq!(s.matches.len(), 1);
    assert_eq!(s.matches[0].result, MatchResult::Conceded);
    assert_eq!(s.matches[0].format, "two_v_two");
    assert_eq!(s.matches[0].colonies.len(), 4);
}

#[test]
fn the_main_menu_opens_match_history() {
    let mut a = app();
    let ui = a.ui();
    let items = ui["panels"][0]["items"].as_array().unwrap();
    let last = items.last().unwrap();
    assert_eq!(
        (last["id"].as_str(), last["text"].as_str()),
        (Some("go.match_history"), Some("Match History"))
    );
    a.act("go.match_history", "", false, false);
    let texts = panel_texts(&a.ui());
    // Empty: the table head, Match: None, and Clear is disabled.
    assert!(texts.contains(&"None".to_string()));
    a.act("go.main_menu", "", false, false);
    assert_eq!(a.screen, Screen::MainMenu);
}
