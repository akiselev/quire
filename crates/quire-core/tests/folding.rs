//! Folding (fold ranges, fold commands, folds across edits) and the large-file policy.

use quire_core::syntax::{FoldKind, FoldRange};
use quire_core::{
    DegradedReason, Document, EditorCommand, EditorConfig, EditorView, LargeFilePolicy, Motion,
    Selection,
};

const SRC: &str = "fn a() {\n    one;\n    two;\n}\nfn b() {\n    if x {\n        y;\n    }\n}\n";

fn setup(src: &str) -> (Document, EditorView) {
    let doc = Document::new(src, EditorConfig::default());
    let view = EditorView::new(&doc);
    (doc, view)
}

fn run(doc: &mut Document, view: &mut EditorView, cmd: EditorCommand) {
    doc.execute(view, &cmd, None).expect("command");
}

fn lines(r: &[FoldRange]) -> Vec<(usize, usize)> {
    r.iter().map(|r| (r.start_line, r.end_line)).collect()
}

#[test]
fn indentation_ranges_from_the_document() {
    let (doc, _) = setup(SRC);
    assert_eq!(lines(doc.fold_ranges()), vec![(0, 2), (4, 7), (5, 6)]);
}

#[test]
fn fold_unfold_toggle_all() {
    let (mut doc, mut view) = setup(SRC);
    // Cursor on line 6 ("y;"): fold takes the innermost range (5..6), then the outer one.
    let p = doc.text().line_to_byte(6) + 8;
    view.set_selection(&doc, Selection::point(p));
    run(&mut doc, &mut view, EditorCommand::Fold);
    assert_eq!(view.folds().len(), 1);
    assert!(view.is_line_hidden(doc.text(), 6));
    assert!(!view.is_line_hidden(doc.text(), 5));
    assert!(view.fold_at_line(doc.text(), 5).is_some());
    // The cursor moved out of the hidden text, to the header line's end.
    assert_eq!(
        view.selection().primary().head,
        doc.text().line_range(5).end
    );
    run(&mut doc, &mut view, EditorCommand::Fold);
    assert_eq!(view.folds().len(), 2);
    assert!(view.is_line_hidden(doc.text(), 5));
    run(&mut doc, &mut view, EditorCommand::Unfold);
    assert_eq!(view.folds().len(), 1);
    // The cursor is on line 4 now (moved out of the outer fold): toggle refolds (4..7), then
    // unfolds it again.
    run(&mut doc, &mut view, EditorCommand::ToggleFold);
    assert_eq!(view.folds().len(), 2);
    run(&mut doc, &mut view, EditorCommand::ToggleFold);
    assert_eq!(view.folds().len(), 1);
    run(&mut doc, &mut view, EditorCommand::FoldAll);
    assert_eq!(view.folds().len(), 3);
    run(&mut doc, &mut view, EditorCommand::UnfoldAll);
    assert!(view.folds().is_empty());
    // The ids round-trip.
    assert_eq!(
        EditorCommand::from_vscode_id("editor.toggleFold", 10),
        Some(EditorCommand::ToggleFold)
    );
}

#[test]
fn folds_follow_edits_and_vertical_motion_skips_them() {
    let (mut doc, mut view) = setup(SRC);
    run(&mut doc, &mut view, EditorCommand::Fold); // cursor at 0: folds lines 1..2
    assert!(view.is_line_hidden(doc.text(), 1));
    // Down from line 0 lands on line 3, past the hidden lines.
    run(
        &mut doc,
        &mut view,
        EditorCommand::CursorMove {
            to: Motion::Down,
            select: false,
        },
    );
    assert_eq!(doc.text().byte_to_line(view.selection().primary().head), 3);
    // Insert a line at the top through another view: the fold maps with it.
    let mut other = EditorView::new(&doc);
    run(
        &mut doc,
        &mut other,
        EditorCommand::Type {
            text: "// x\n".into(),
        },
    );
    view.sync(&doc);
    assert!(view.is_line_hidden(doc.text(), 2));
    assert!(view.is_line_hidden(doc.text(), 3));
    assert!(!view.is_line_hidden(doc.text(), 1));
    // Indentation ranges are recomputed for the new version.
    assert_eq!(lines(doc.fold_ranges())[0], (1, 3));
}

#[test]
fn syntax_ranges_replace_indentation_and_map_forward() {
    let (mut doc, mut view) = setup(SRC);
    let v0 = doc.version();
    run(
        &mut doc,
        &mut view,
        EditorCommand::Type { text: "\n".into() },
    );
    // Ranges computed at v0 arrive late: they are shifted by the inserted line.
    doc.set_fold_ranges(
        v0,
        vec![FoldRange {
            start_line: 4,
            end_line: 8,
            kind: FoldKind::Other,
        }],
    )
    .expect("in window");
    assert_eq!(lines(doc.fold_ranges()), vec![(5, 9)]);
    doc.clear_fold_ranges();
    assert_eq!(lines(doc.fold_ranges())[0], (1, 3));
}

#[test]
fn large_file_policy() {
    let cfg = EditorConfig {
        large_file: LargeFilePolicy {
            max_bytes: 1 << 30,
            max_lines: 5,
            max_line_len: 1000,
        },
        ..EditorConfig::default()
    };
    let doc = Document::new(SRC, cfg.clone());
    let d = doc.degraded().expect("degraded");
    assert_eq!(d.reasons, vec![DegradedReason::LineCount { lines: 10 }]);
    assert!(doc.fold_ranges().is_empty());
    let doc = Document::new("a\nb", cfg);
    assert!(doc.degraded().is_none());
}
