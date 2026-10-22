//! Shared test helpers.
#![allow(dead_code)]

pub mod fixture_gen;

use quire_core::syntax::{CancelFlag, Highlighter, TokenRuns};
use quire_core::{Snapshot, Text, Version};
use quire_syntax::{Lang, SyntaxTree, TreeSitterHighlighter, style_name};

pub fn snap(s: &str) -> Snapshot {
    Snapshot { text: Text::from(s), version: Version(0) }
}

pub fn highlighter(lang: Lang) -> TreeSitterHighlighter {
    TreeSitterHighlighter::new(lang).expect("grammar compiled in")
}

pub fn parse(lang: Lang, s: &str) -> (TreeSitterHighlighter, Snapshot, SyntaxTree) {
    let h = highlighter(lang);
    let sn = snap(s);
    let t = h.parse(&sn, None, &CancelFlag::new()).expect("parses");
    (h, sn, t)
}

/// Every line's tokens as `type:"text"` pairs (plain text omitted), one line per source line.
pub fn render_runs(text: &str, runs: &TokenRuns) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate().skip(runs.first_line).take(runs.lines.len()) {
        let toks = &runs.lines[i - runs.first_line];
        let mut parts = Vec::new();
        for (k, t) in toks.iter().enumerate() {
            let end = toks.get(k + 1).map_or(line.len(), |n| n.start as usize);
            if t.style.0 != 0 {
                parts.push(format!("{}:{:?}", style_name(t.style.0), &line[t.start as usize..end]));
            }
        }
        out.push_str(&parts.join(" "));
        out.push('\n');
    }
    out
}

pub fn render(lang: Lang, s: &str) -> String {
    let (h, sn, t) = parse(lang, s);
    render_runs(s, &h.highlights(&t, &sn, 0..usize::MAX))
}

/// The style name of the token that covers the first occurrence of `needle` (after `after`).
pub fn style_of(lang: Lang, s: &str, needle: &str) -> String {
    style_at(lang, s, s.find(needle).unwrap_or_else(|| panic!("{needle:?} not in source")))
}

pub fn style_at(lang: Lang, s: &str, at: usize) -> String {
    let (h, sn, t) = parse(lang, s);
    let line = s[..at].matches('\n').count();
    let col = at - s[..at].rfind('\n').map_or(0, |i| i + 1);
    let runs = h.highlights(&t, &sn, line..line + 1);
    let toks = &runs.lines[0];
    let i = toks.partition_point(|t| t.start as usize <= col);
    if i == 0 { "text".into() } else { style_name(toks[i - 1].style.0) }
}

/// Compares `actual` with the expected file; `QUIRE_BLESS=1` rewrites it.
pub fn expect_file(path: &str, actual: &str) {
    let full = format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("QUIRE_BLESS").is_some() {
        std::fs::write(&full, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&full).unwrap_or_else(|_| panic!("missing {full} (QUIRE_BLESS=1 to create)"));
    if expected != actual {
        let diff: Vec<String> = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(5)
            .map(|(i, (a, b))| format!("line {}:\n  expected {a}\n  actual   {b}", i + 1))
            .collect();
        panic!("{path} differs (QUIRE_BLESS=1 to accept):\n{}", diff.join("\n"));
    }
}
