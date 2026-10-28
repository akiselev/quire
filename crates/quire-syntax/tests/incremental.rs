//! Incremental-edit fixtures: after each edit, the incremental parse (previous tree + change set)
//! highlights, folds and injects exactly like a parse from scratch.
//!
//! tree-sitter's error recovery may legitimately build a different tree incrementally than from
//! scratch; such steps (some layer's S-expression differs) are counted and skipped, and must
//! stay a small minority. Everything above the root tree (layers, injections, lazy shifts,
//! highlighting) is ours and must match exactly.

mod support;

use quire_core::syntax::{CancelFlag, Highlighter};
use quire_core::{ChangeSet, Snapshot, Text, Version};
use quire_syntax::{Lang, SyntaxTree, TreeSitterHighlighter};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const SNIPPETS: &[&str] = &[
    "x",
    "a",
    " ",
    "\n",
    "{",
    "}",
    "(",
    ")",
    "<p>",
    "</p>",
    "{n}",
    "{#if a}",
    "{/if}",
    "'",
    "\"",
    "`",
    "/*",
    "*/",
    "//",
    ";",
    "let y = 2;\n",
    "<b>{a + 1}</b>\n",
    ".c { color: red; }\n",
    "lang=\"ts\"",
    "\n\n",
    "  ",
    "=>",
    "<",
    ">",
    "${",
    "css`",
    "é",
];

fn injections(t: &SyntaxTree) -> String {
    format!("{:?}", t.injections())
}

fn check(lang: Lang, src: &str, steps: usize, seed: u64) -> (usize, usize) {
    let h = TreeSitterHighlighter::new(lang).unwrap();
    let mut rng = Rng(seed);
    let mut text = Text::from(src);
    let cancel = CancelFlag::new();
    let mut snap = Snapshot {
        text: text.clone(),
        version: Version(0),
    };
    let mut tree = h.parse(&snap, None, &cancel).unwrap();
    let (mut compared, mut skipped) = (0, 0);
    for step in 0..steps {
        // One to three non-overlapping edits per change set.
        let len = text.len_bytes();
        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let n = 1 + rng.below(3);
        let mut points: Vec<usize> = (0..n * 2)
            .map(|_| text.clamp_to_boundary(rng.below(len + 1)))
            .collect();
        points.sort();
        for k in 0..n {
            let (a, b) = (points[2 * k], points[2 * k + 1]);
            let b = if rng.below(3) == 0 { b.min(a + 12) } else { a };
            let b = text.clamp_to_boundary(b.max(a));
            let ins = if rng.below(4) == 0 {
                String::new()
            } else {
                SNIPPETS[rng.below(SNIPPETS.len())].to_string()
            };
            if b == a && ins.is_empty() {
                continue;
            }
            if edits.last().is_some_and(|(r, _)| r.end > a) {
                continue;
            }
            edits.push((a..b, ins));
        }
        if edits.is_empty() {
            continue;
        }
        let changes =
            ChangeSet::replace(len, edits.iter().map(|(r, s)| (r.clone(), s.as_str()))).unwrap();
        text = changes.apply(&text).unwrap();
        snap = Snapshot {
            text: text.clone(),
            version: Version(step as u64 + 1),
        };
        let inc = h.parse(&snap, Some((&tree, &changes)), &cancel).unwrap();
        let full = h.parse(&snap, None, &cancel).unwrap();
        let all = 0..text.len_lines();
        let same_tree = inc_tree_eq(&inc, &full);
        if !same_tree {
            skipped += 1;
        } else {
            compared += 1;
            let s = text.to_string();
            assert_eq!(
                injections(&inc),
                injections(&full),
                "{lang} step {step} injections; edits {edits:?}\n{s}"
            );
            assert_eq!(
                h.highlights(&inc, &snap, all.clone()),
                h.highlights(&full, &snap, all.clone()),
                "{lang} step {step} highlights; edits {edits:?}\n{s}"
            );
            assert_eq!(
                h.folds(&inc),
                h.folds(&full),
                "{lang} step {step} folds; edits {edits:?}"
            );
            assert_eq!(
                h.brackets(&inc, all.clone()),
                h.brackets(&full, all),
                "{lang} step {step} brackets"
            );
        }
        tree = inc;
    }
    (compared, skipped)
}

/// Same tree shapes in every layer (tree-sitter's part); positions are ours and checked
/// through highlights.
fn inc_tree_eq(a: &SyntaxTree, b: &SyntaxTree) -> bool {
    a.root_sexp() == b.root_sexp() && a.layer_sexps() == b.layer_sexps()
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// `QUIRE_FUZZ_SEEDS=n` adds n more seeds per test (longer local runs).
fn run(lang: Lang, src: &str, seeds: &[u64], steps: usize) {
    let extra: u64 = std::env::var("QUIRE_FUZZ_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let seeds: Vec<u64> = seeds
        .iter()
        .copied()
        .chain((0..extra).map(|i| 1000 + i * 7919))
        .collect();
    let (mut c, mut s) = (0, 0);
    for &seed in &seeds {
        let (a, b) = check(lang, src, steps, seed);
        c += a;
        s += b;
    }
    eprintln!("{lang}: {c} steps compared, {s} skipped (tree-sitter error recovery differs)");
    assert!(
        c >= 3 * s,
        "{lang}: too many skipped steps ({s} of {})",
        c + s
    );
}

#[test]
fn svelte_component_edits() {
    run(
        Lang::Svelte,
        &fixture("component.svelte"),
        &[1, 2, 3, 4],
        40,
    );
}

#[test]
fn svelte_generated_edits() {
    run(
        Lang::Svelte,
        &support::fixture_gen::svelte_component(400),
        &[5, 6],
        40,
    );
}

#[test]
fn typescript_edits() {
    run(Lang::TypeScript, &fixture("sample.ts"), &[7, 8, 9], 40);
}

#[test]
fn javascript_edits() {
    run(Lang::JavaScript, &fixture("sample.js"), &[10, 11], 40);
}

#[test]
fn css_edits() {
    run(Lang::Css, &fixture("sample.css"), &[12, 13], 40);
}

#[cfg(feature = "toml")]
#[test]
fn toml_edits() {
    run(Lang::Toml, &fixture("sample.toml"), &[14, 15], 40);
}

#[test]
fn lazy_shift_after_many_line_edits() {
    // Many edits above untouched layers: their lazy shifts accumulate across parses.
    let src = support::fixture_gen::svelte_component(300);
    let h = TreeSitterHighlighter::new(Lang::Svelte).unwrap();
    let cancel = CancelFlag::new();
    let mut text = Text::from(src.as_str());
    let mut tree = h
        .parse(
            &Snapshot {
                text: text.clone(),
                version: Version(0),
            },
            None,
            &cancel,
        )
        .unwrap();
    let at = src.find("<main").unwrap();
    for i in 0..30 {
        let ins = if i % 3 == 0 {
            "<p>{x}</p>\n"
        } else if i % 3 == 1 {
            "\n"
        } else {
            ""
        };
        let edit = if ins.is_empty() {
            (at..at + 1, "")
        } else {
            (at..at, ins)
        };
        let changes = ChangeSet::replace(text.len_bytes(), [edit]).unwrap();
        text = changes.apply(&text).unwrap();
        let snap = Snapshot {
            text: text.clone(),
            version: Version(i + 1),
        };
        tree = h.parse(&snap, Some((&tree, &changes)), &cancel).unwrap();
        assert!(tree.stats().reused > 0);
    }
    let snap = Snapshot {
        text: text.clone(),
        version: Version(99),
    };
    let full = h.parse(&snap, None, &cancel).unwrap();
    let all = 0..text.len_lines();
    assert_eq!(
        h.highlights(&tree, &snap, all.clone()),
        h.highlights(&full, &snap, all.clone())
    );
    assert_eq!(h.folds(&tree), h.folds(&full));
    for line in [10, 100, 250] {
        assert_eq!(
            h.indent_level(&tree, line),
            h.indent_level(&full, line),
            "line {line}"
        );
    }
}
