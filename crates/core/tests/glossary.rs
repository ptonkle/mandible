//! The glossary guard (PLAN decision 23): on-screen words come from
//! GLOSSARY.md. These tests read GLOSSARY.md, data/strings.json and
//! data/glossary.json.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(p: &str) -> String {
    fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Table rows of GLOSSARY.md as cells, without header and rule lines.
fn rows() -> Vec<Vec<String>> {
    read("GLOSSARY.md")
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| ---"))
        .map(|l| {
            l.trim_matches('|')
                .split(" | ")
                .map(|c| c.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|c| c.len() >= 3 && c[0] != "Term" && c[0] != "Species" && c[0] != "Name")
        .collect()
}

/// Every term: the first cell, split on ", " and " / ".
fn terms() -> BTreeSet<String> {
    let mut t = BTreeSet::new();
    for r in rows() {
        for part in r[0].split(" / ").flat_map(|p| p.split(", ")) {
            let p = part.trim().trim_matches('*');
            if !p.is_empty() {
                t.insert(p.to_string());
            }
        }
    }
    t
}

fn strings() -> Vec<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(&read("data/strings.json")).unwrap();
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn on_screen_names_are_glossary_terms() {
    let terms = terms();
    let species: Vec<String> = strings()
        .into_iter()
        .filter(|(k, _)| k.starts_with("species."))
        .map(|(_, v)| v)
        .collect();
    let named = [
        "species.",
        "structure.",
        "upgrade.",
        "tab.",
        "role.",
        "resource.",
        "difficulty.",
        "action.",
        "focus.",
        "stat.",
        "state.",
        "format.",
        "biome.",
        "color.",
        "end.",
        "time.",
        "short.",
        "event.",
    ];
    let mut bad = Vec::new();
    for (k, v) in strings() {
        if !named.iter().any(|p| k.starts_with(p)) {
            continue;
        }
        let composed = species.iter().any(|sp| {
            v.strip_prefix(sp.as_str())
                .map(|rest| {
                    let rest = rest.trim();
                    rest.is_empty() || terms.contains(rest)
                })
                .unwrap_or(false)
        });
        if !terms.contains(&v) && !composed {
            bad.push(format!("{k} = {v:?}"));
        }
    }
    assert!(bad.is_empty(), "not in GLOSSARY.md:\n{}", bad.join("\n"));
}

#[test]
fn every_hint_term_is_in_the_glossary() {
    let terms = terms();
    let hints: serde_json::Value = serde_json::from_str(&read("data/glossary.json")).unwrap();
    let bad: Vec<&String> = hints
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| !terms.contains(*k))
        .collect();
    assert!(
        bad.is_empty(),
        "term hints for words not in GLOSSARY.md: {bad:?}"
    );
}

/// Plain never-say words: single words or phrases without notes in
/// parentheses, that are not themselves glossary terms.
fn never_say() -> BTreeSet<String> {
    let terms: BTreeSet<String> = terms().into_iter().map(|t| t.to_lowercase()).collect();
    let mut out = BTreeSet::new();
    for r in rows() {
        let Some(cell) = r.last() else { continue };
        for w in cell.split(", ") {
            let w = w.trim();
            if w.is_empty() || w.contains('(') || w.contains(';') || w.contains('"') {
                continue;
            }
            let lw = w.to_lowercase();
            if !terms.contains(&lw)
                && lw
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == ' ' || c == '-')
            {
                out.insert(lw);
            }
        }
    }
    out
}

#[test]
fn strings_avoid_never_say_words() {
    let banned = never_say();
    assert!(
        banned.contains("unit") && banned.contains("base"),
        "the parser reads the lists"
    );
    // Whole glossary terms are removed first: "Carry Capacity" is a term,
    // even though "carry" alone is a never-say word.
    let mut terms: Vec<String> = terms().into_iter().map(|t| t.to_lowercase()).collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let mut bad = Vec::new();
    for (k, v) in strings() {
        let mut rest = format!(" {} ", v.to_lowercase());
        for t in &terms {
            rest = remove_word(&rest, t);
        }
        let words: Vec<String> = rest
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| !w.is_empty())
            .map(|w| w.to_string())
            .collect();
        let text = format!(" {} ", words.join(" "));
        for b in &banned {
            if text.contains(&format!(" {b} ")) {
                bad.push(format!("{k} = {v:?} uses \"{b}\""));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "never-say words on screen:\n{}",
        bad.join("\n")
    );
}

/// Removes `term` from `text` wherever it stands as whole words.
fn remove_word(text: &str, term: &str) -> String {
    let edge = |c: Option<char>| c.map(|c| !c.is_alphanumeric()).unwrap_or(true);
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(term) {
        let before = rest[..i].chars().last();
        let after = rest[i + term.len()..].chars().next();
        if edge(before) && edge(after) {
            out.push_str(&rest[..i]);
            out.push(' ');
        } else {
            out.push_str(&rest[..i + term.len()]);
        }
        rest = &rest[i + term.len()..];
    }
    out.push_str(rest);
    out
}

#[test]
fn removal_respects_word_edges() {
    assert_eq!(remove_word(" waiting on ", "ai"), " waiting on ");
    assert_eq!(remove_word(" ai pupa ", "ai"), "   pupa ");
}
