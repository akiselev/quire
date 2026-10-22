//! Robustness (QE4): random `EditorCommand` streams, undo and redo included, through one
//! `Document` and two `EditorView`s. After every step both views (synced) must hold:
//! - every selection range inside the text and on char boundaries, sorted and non-overlapping,
//!   with a valid primary index;
//! - folds and fresh find matches inside the text, and the document's (incremental) indentation
//!   fold ranges equal to a fresh computation;
//! - the document version never goes back, and moves whenever the text changes;
//! - `is_dirty()` is true whenever the text differs from the saved (original) text.
//!
//! At the end, undoing everything restores the original text, and as many redos as undos restore
//! the text before the undos. `QUIRE_ROBUST_CASES=n` runs more cases.

use proptest::prelude::*;
use quire_core::*;

const ALPHABET: &[&str] = &[
    "a", "b", "word", " ", "  ", "\t", "\n", "\n    ", "é", "😀", "(", ")", "{", "}", "\"", "'",
    "//", "x_1", ".",
];

fn text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..40).prop_map(|v| v.concat())
}

fn motion() -> impl Strategy<Value = Motion> {
    prop_oneof![
        Just(Motion::Left),
        Just(Motion::Right),
        Just(Motion::Up),
        Just(Motion::Down),
        Just(Motion::WordLeft),
        Just(Motion::WordStartLeft),
        Just(Motion::WordEndLeft),
        Just(Motion::WordStartRight),
        Just(Motion::WordEndRight),
        Just(Motion::LineStart),
        Just(Motion::LineEnd),
        Just(Motion::DocumentStart),
        Just(Motion::DocumentEnd),
        (1u32..4).prop_map(|lines| Motion::PageUp { lines }),
        (1u32..4).prop_map(|lines| Motion::PageDown { lines }),
    ]
}

fn snippet() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..4).prop_map(|v| v.concat())
}

fn query() -> impl Strategy<Value = FindQuery> {
    (
        prop::sample::select(&["a", "word", "é", "\\w+", "(", "^", "$", "a|", ""][..]),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(s, is_regex, match_case, whole_word)| FindQuery {
            search_string: s.to_owned(),
            is_regex,
            match_case,
            whole_word,
        })
}

/// A command, with selections given as seeds (resolved against the text at the step).
#[derive(Clone, Debug)]
enum Step {
    Cmd(EditorCommand),
    /// SetSelection: (anchor seed, head seed) per range, primary seed. Seeds may fall off char
    /// boundaries and past the end; the document must refuse or clamp, never panic.
    Select(Vec<(usize, usize)>, usize),
}

fn command() -> impl Strategy<Value = Step> {
    use EditorCommand::*;
    let simple = prop::sample::select(vec![
        SelectAll,
        ExpandLineSelection,
        RemoveSecondaryCursors,
        Cut,
        DeleteLeft,
        DeleteRight,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteWordStartLeft,
        DeleteWordEndLeft,
        DeleteWordStartRight,
        DeleteWordEndRight,
        Tab,
        Outdent,
        IndentLines,
        OutdentLines,
        CommentLine,
        AddCommentLine,
        RemoveCommentLine,
        BlockComment,
        DeleteAllRight,
        InsertLineAfter,
        InsertLineBefore,
        DeleteLines,
        InsertCursorAbove,
        InsertCursorBelow,
        AddNextMatch,
        MoveLinesUp,
        MoveLinesDown,
        CopyLinesUp,
        CopyLinesDown,
        DeleteAllLeft,
        ColumnSelectLeft,
        ColumnSelectRight,
        ColumnSelectUp,
        ColumnSelectDown,
        Fold,
        Unfold,
        ToggleFold,
        FoldAll,
        UnfoldAll,
        FindNext,
        FindPrevious,
        CloseFind,
        Save,
    ]);
    prop_oneof![
        4 => simple.prop_map(Step::Cmd),
        3 => (motion(), any::<bool>()).prop_map(|(to, select)| Step::Cmd(CursorMove { to, select })),
        4 => snippet().prop_map(|text| Step::Cmd(Type { text })),
        1 => (snippet(), any::<bool>()).prop_map(|(text, per_cursor)| Step::Cmd(Paste { text, per_cursor })),
        2 => (prop::collection::vec((0usize..200, 0usize..200), 1..4), 0usize..4)
            .prop_map(|(r, p)| Step::Select(r, p)),
        1 => (1u32..4).prop_map(|lines| Step::Cmd(ColumnSelectPageUp { lines })),
        1 => (1u32..4).prop_map(|lines| Step::Cmd(ColumnSelectPageDown { lines })),
        1 => (0usize..8, 0u32..12, 0usize..8, 0u32..12).prop_map(|(al, ac, hl, hc)| Step::Cmd(ColumnSelect {
            anchor_line: al,
            anchor_column: ac,
            head_line: hl,
            head_column: hc,
        })),
        1 => (snippet(), 0usize..3, 0usize..3).prop_map(|(text, p, n)| Step::Cmd(CompositionType {
            text,
            replace_prev_chars: p,
            replace_next_chars: n,
        })),
        3 => Just(Step::Cmd(Undo)),
        2 => Just(Step::Cmd(Redo)),
        1 => query().prop_map(|query| Step::Cmd(Find { query })),
        1 => snippet().prop_map(|replacement| Step::Cmd(ReplaceOne { replacement })),
        1 => snippet().prop_map(|replacement| Step::Cmd(ReplaceAll { replacement })),
        1 => (0usize..10).prop_map(|line| Step::Cmd(RevealLine { line })),
    ]
}

fn resolve(step: &Step) -> EditorCommand {
    match step {
        Step::Cmd(c) => c.clone(),
        Step::Select(rs, p) => EditorCommand::SetSelection {
            selection: Selection::new(rs.iter().map(|&(a, h)| SelRange::new(a, h)), *p),
        },
    }
}

/// Checks one view's invariants against the document (on a synced copy).
fn check_view(doc: &Document, view: &EditorView, who: usize) -> Result<(), TestCaseError> {
    let mut v = view.clone();
    v.sync(doc);
    let text = doc.text();
    let len = text.len_bytes();
    let sel = v.selection();
    prop_assert!(!sel.ranges().is_empty(), "view {who}: empty selection");
    prop_assert!(
        sel.primary_index() < sel.len(),
        "view {who}: primary out of range"
    );
    for r in sel.ranges() {
        for p in [r.anchor, r.head] {
            prop_assert!(p <= len, "view {who}: {p} past the end {len} in {:?}", sel);
            prop_assert!(
                text.is_char_boundary(p),
                "view {who}: {p} not on a char boundary in {:?}",
                text.to_string()
            );
        }
    }
    for w in sel.ranges().windows(2) {
        let (a, b) = (w[0], w[1]);
        let ok = b.from() > a.to() || (b.from() == a.to() && !a.is_empty() && !b.is_empty());
        prop_assert!(ok, "view {who}: ranges overlap or are unsorted: {:?}", sel);
    }
    for f in v.folds() {
        prop_assert!(
            f.start <= f.end && f.end <= len,
            "view {who}: fold {f:?} outside {len}"
        );
    }
    if let Some(find) = v.find()
        && !find.stale
        && find.version == doc.version()
    {
        for m in &find.matches {
            prop_assert!(
                m.start <= m.end && m.end <= len,
                "view {who}: match {m:?} outside {len}"
            );
            prop_assert!(text.is_char_boundary(m.start) && text.is_char_boundary(m.end));
        }
    }
    Ok(())
}

fn run(original: &str, steps: &[(u8, Step, u64)]) -> Result<(), TestCaseError> {
    let mut doc = Document::new(original, EditorConfig::default());
    let mut views = [EditorView::new(&doc), EditorView::new(&doc)];
    let mut now = 0u64;
    let mut version = doc.version();
    for (who, step, dt) in steps {
        now += dt;
        let who = *who as usize % 2;
        let before = doc.text().to_string();
        let cmd = resolve(step);
        let res = doc.execute(&mut views[who], &cmd, Some(now));
        if let Err(e) = &res {
            // Refusals are fine for out-of-range selections and invalid regexes only.
            let expected = match (step, e) {
                (Step::Select(..), Refusal::NotCharBoundary(_)) => true,
                (Step::Cmd(EditorCommand::Find { query }), Refusal::InvalidQuery(_)) => {
                    query.is_regex
                }
                _ => false,
            };
            prop_assert!(expected, "{:?} refused: {:?} on {:?}", cmd, e, before);
        }
        let after = doc.text().to_string();
        prop_assert!(doc.version() >= version, "version went back after {cmd:?}");
        if after != before {
            prop_assert!(
                doc.version() > version,
                "text changed without a new version: {cmd:?}"
            );
        }
        version = doc.version();
        // Dirty whenever the text differs from the saved text. (Editing back to the saved
        // text by hand stays dirty, as in VS Code; only undo/redo back to it cleans.)
        if after != original {
            prop_assert!(
                doc.is_dirty(),
                "clean but changed after {:?}: {:?}",
                cmd,
                after
            );
        }
        // The incrementally kept indentation folds equal a fresh computation.
        prop_assert_eq!(
            doc.fold_ranges(),
            &quire_core::folding::indent_fold_ranges(doc.text(), doc.config().tab_size)[..],
            "fold ranges after {:?}",
            cmd
        );
        for (i, v) in views.iter().enumerate() {
            check_view(&doc, v, i)?;
        }
    }
    // Undo everything (through either view), then redo everything.
    let end = doc.text().to_string();
    let mut undos = 0;
    while doc.history().can_undo() {
        undos += 1;
        prop_assert!(undos < 10_000, "undo does not terminate");
        let v = &mut views[undos % 2];
        doc.execute(v, &EditorCommand::Undo, Some(now)).unwrap();
        for (i, v) in views.iter().enumerate() {
            check_view(&doc, v, i)?;
        }
    }
    prop_assert_eq!(doc.text().to_string(), original, "undo-all");
    prop_assert!(!doc.is_dirty());
    // As many redos as undos (the redo stack may also hold groups undone during the stream).
    for _ in 0..undos {
        prop_assert!(doc.history().can_redo(), "fewer redo groups than undos");
        doc.execute(&mut views[0], &EditorCommand::Redo, Some(now))
            .unwrap();
        check_view(&doc, &views[0], 0)?;
    }
    prop_assert_eq!(doc.text().to_string(), end, "redo-all");
    Ok(())
}

fn cases() -> u32 {
    std::env::var("QUIRE_ROBUST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn random_commands_keep_invariants(
        s in text_strategy(),
        steps in prop::collection::vec((0u8..2, command(), prop::sample::select(&[0u64, 50, 400, 1500][..])), 1..60),
    ) {
        run(&s, &steps)?;
    }
}

// Bugs the proptest found, pinned.

/// Typing a closing bracket before a multi-byte char: overtype compared the next byte, which
/// sliced the rope inside `é` and panicked.
#[test]
fn closing_bracket_before_multibyte_char() {
    for (src, typed) in [("é", "}"), ("😀", ")"), ("é", "\"")] {
        let mut doc = Document::new(src, EditorConfig::default());
        let mut v = EditorView::new(&doc);
        doc.execute(&mut v, &EditorCommand::Type { text: typed.into() }, Some(0))
            .unwrap();
        assert_eq!(doc.text().to_string(), format!("{typed}{src}"));
    }
    // Auto-close looked back one byte for a two-char opener's prefix (e.g. `/*`).
    let cfg = EditorConfig {
        auto_closing_pairs: vec![("/*".into(), "*/".into())],
        ..EditorConfig::default()
    };
    let mut doc = Document::new("é", cfg);
    let mut v = EditorView::new(&doc);
    v.set_selection(&doc, Selection::point(2));
    doc.execute(&mut v, &EditorCommand::Type { text: "*".into() }, Some(0))
        .unwrap();
    assert_eq!(doc.text().to_string(), "é*");
}

/// Indent Lines with a second selection on a whitespace-only line: the "start stays" rule kept
/// the second range's old start, unmapped through the first range's edit, which landed inside
/// the emoji.
#[test]
fn indent_lines_maps_every_range_start() {
    let mut doc = Document::new("\naa😀\n    ", EditorConfig::default());
    let mut v = EditorView::new(&doc);
    let sel = Selection::new([SelRange::new(2, 3), SelRange::new(9, 10)], 0);
    v.set_selection(&doc, sel);
    doc.execute(&mut v, &EditorCommand::IndentLines, Some(0))
        .unwrap();
    assert_eq!(doc.text().to_string(), "\n    aa😀\n        ");
    let r: Vec<(usize, usize)> = v
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.anchor, r.head))
        .collect();
    assert_eq!(r, vec![(6, 7), (13, 20)]);
}

/// Move Lines Down with a selection ending at the start of the last line (which moves up): the
/// end was shifted with the block, one byte past the end of the text.
#[test]
fn move_lines_down_over_the_last_line_keeps_the_selection_in_bounds() {
    let mut doc = Document::new("ab\ncd\n", EditorConfig::default());
    let mut v = EditorView::new(&doc);
    v.set_selection(&doc, Selection::single(6, 0));
    doc.execute(&mut v, &EditorCommand::MoveLinesDown, Some(0))
        .unwrap();
    assert_eq!(doc.text().to_string(), "\nab\ncd");
    let r = v.selection().primary();
    assert_eq!((r.anchor, r.head), (6, 1));
}
