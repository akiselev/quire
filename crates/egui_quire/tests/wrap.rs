//! kittests for soft wrap (QE4 / ED2) and long-line prefixes (E3 F2).

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Vec2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_quire::CodeEditor;
use quire_core::{
    Document, EditorCommand, EditorConfig, EditorView, Selection, WordWrap, WrappingIndent,
};

struct St {
    doc: Document,
    view: EditorView,
    caret: Option<Rect>,
    rect: Rect,
    id: egui::Id,
}

impl St {
    fn new(text: &str, cfg: EditorConfig) -> Self {
        let doc = Document::new(text, cfg);
        let view = EditorView::new(&doc);
        Self {
            doc,
            view,
            caret: None,
            rect: Rect::NOTHING,
            id: egui::Id::NULL,
        }
    }

    fn head(&self) -> usize {
        self.view.selection().primary().head
    }

    fn select(&mut self, at: usize) {
        self.doc
            .execute(
                &mut self.view,
                &EditorCommand::SetSelection {
                    selection: Selection::point(at),
                },
                None,
            )
            .unwrap();
    }
}

fn wrap_cfg() -> EditorConfig {
    EditorConfig {
        word_wrap: WordWrap::On,
        ..Default::default()
    }
}

fn harness(st: St) -> Harness<'static, St> {
    let mut h = Harness::builder()
        .with_size(Vec2::new(420.0, 300.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, st: &mut St| {
                let rect = ui.available_rect_before_wrap();
                let ed = CodeEditor::new(&mut st.doc, &mut st.view, "wrap", "Source");
                st.id = ed.editor_id();
                let out = ed.show_in(ui, rect);
                st.caret = out.caret_rect;
                st.rect = rect;
            },
            st,
        );
    h.run();
    h
}

fn click_at(h: &mut Harness<'_, St>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
}

fn caret(h: &Harness<'_, St>) -> Rect {
    h.state().caret.expect("caret")
}

fn press(h: &mut Harness<'_, St>, key: Key) {
    h.key_press(key);
    h.run();
}

/// "word " × n: breaks only after spaces.
fn words(n: usize) -> String {
    "word ".repeat(n).trim_end().to_owned()
}

/// Put the caret at `at`, focus the editor without moving it, and return the caret rect.
fn caret_at(h: &mut Harness<'_, St>, at: usize) -> Rect {
    h.state_mut().select(at);
    h.run();
    caret(h)
}

fn focus(h: &mut Harness<'_, St>) {
    let r = h.state().rect;
    // The last line is short, so a click at the bottom right lands at the document end.
    click_at(h, pos2(r.right() - 20.0, r.bottom() - 10.0));
}

#[test]
fn long_line_wraps_into_rows_and_the_next_line_follows() {
    let text = format!("{}\nnext", words(40));
    let mut h = harness(St::new(&text, wrap_cfg()));
    let c0 = caret_at(&mut h, 0);
    let row_h = c0.height();
    let end = caret_at(&mut h, words(40).len());
    assert!(
        end.top() >= c0.top() + row_h * 3.0,
        "200 chars at ~45 cols: 4+ rows"
    );
    // Every row stays inside the editor (no horizontal overflow).
    assert!(end.right() < h.state().rect.right());
    let rows = ((end.top() - c0.top()) / row_h).round() + 1.0;
    let next = caret_at(&mut h, words(40).len() + 1);
    assert!((next.top() - (c0.top() + rows * row_h)).abs() < 0.5);
    // The gutter's line anchor of line 1 starts below all rows of line 0.
    let id = h.state().id;
    let l1 = CodeEditor::anchor_rect(&h.ctx, id, "line[1]").unwrap();
    assert!((l1.top() - next.top()).abs() < 0.5);
    let l0 = CodeEditor::anchor_rect(&h.ctx, id, "line[0]").unwrap();
    assert!((l0.height() - rows * row_h).abs() < 0.5, "measured height");
    // A row breaks after a space: the second row starts with a word.
    let t = h.state().doc.text().to_string();
    click_at(&mut h, pos2(c0.left() + 1.0, c0.top() + row_h * 1.5));
    let at = h.state().head();
    assert!(at > 0 && &t[at..at + 4] == "word", "{at}");
    assert_eq!(&t[at - 1..at], " ");
}

#[test]
fn up_down_move_by_visual_row() {
    let text = format!("{}\nnext", words(40));
    let mut h = harness(St::new(&text, wrap_cfg()));
    focus(&mut h);
    let c0 = caret_at(&mut h, 2);
    let row_h = c0.height();
    press(&mut h, Key::ArrowDown);
    let c1 = caret(&h);
    assert!((c1.top() - (c0.top() + row_h)).abs() < 0.5, "one row down");
    assert!((c1.left() - c0.left()).abs() < 1.0, "same column");
    let head = h.state().head();
    assert!(
        head > 2 && head < words(40).len(),
        "still in line 0: {head}"
    );
    press(&mut h, Key::ArrowUp);
    assert_eq!(h.state().head(), 2);
    // Down through all rows reaches line 1, keeping the goal column.
    for _ in 0..10 {
        press(&mut h, Key::ArrowDown);
    }
    assert_eq!(h.state().head(), text.len(), "line 1 is shorter: its end");
    // Shift+Down selects by row.
    let c = caret_at(&mut h, 0);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    h.run();
    let sel = h.state().view.selection().primary();
    assert_eq!(sel.anchor, 0);
    assert!((caret(&h).top() - (c.top() + row_h)).abs() < 0.5);
}

#[test]
fn home_and_end_go_to_the_row_first_then_the_line() {
    let line = words(40);
    let text = format!("{line}\nnext");
    let mut h = harness(St::new(&text, wrap_cfg()));
    focus(&mut h);
    let c0 = caret_at(&mut h, 2);
    let row_h = c0.height();
    // End: end of the first row (the caret stays on that row), then the line's end.
    press(&mut h, Key::End);
    let e1 = caret(&h);
    let row_end = h.state().head();
    assert!(row_end < line.len(), "row end first: {row_end}");
    assert!((e1.top() - c0.top()).abs() < 0.5, "drawn at the row's end");
    assert!(e1.left() > c0.left());
    press(&mut h, Key::End);
    assert_eq!(h.state().head(), line.len());
    // Home on the last row: that row's start, then the line start.
    press(&mut h, Key::Home);
    let row_start = h.state().head();
    assert!(row_start > 0 && row_start < line.len());
    let hs = caret(&h);
    assert!(hs.top() > c0.top() + row_h * 0.5);
    press(&mut h, Key::Home);
    assert_eq!(h.state().head(), 0);
}

#[test]
fn click_past_a_row_end_puts_the_caret_at_that_row_end() {
    let text = words(40);
    let mut h = harness(St::new(&text, wrap_cfg()));
    let c0 = caret_at(&mut h, 0);
    let r = h.state().rect;
    click_at(&mut h, pos2(r.right() - 12.0, c0.center().y));
    let c = caret(&h);
    assert!((c.top() - c0.top()).abs() < 0.5, "same row");
    let head = h.state().head();
    assert!(head > 0 && head < text.len());
    assert_eq!(&text[head - 1..head], " ", "after the row's trailing space");
}

#[test]
fn continuation_rows_keep_the_indentation() {
    let line = format!("        {}", words(30));
    let mut h = harness(St::new(&line, wrap_cfg()));
    let first = caret_at(&mut h, 8);
    let row_h = first.height();
    let r = h.state().rect;
    click_at(&mut h, pos2(r.left() + 60.0, first.top() + row_h * 1.5));
    let c = caret(&h);
    assert!(
        (c.left() - first.left()).abs() < 1.0,
        "row 2 starts at the indent"
    );
    // With wrappingIndent none it starts at column 0.
    let cfg = EditorConfig {
        wrapping_indent: WrappingIndent::None,
        ..wrap_cfg()
    };
    let mut h = harness(St::new(&line, cfg));
    let first = caret_at(&mut h, 8);
    let zero = caret_at(&mut h, 0);
    click_at(&mut h, pos2(r.left() + 60.0, first.top() + row_h * 1.5));
    assert!(caret(&h).left() < first.left() - 1.0);
    let _ = zero;
}

#[test]
fn folds_and_wrap_together() {
    let text = format!("fn f() {{ {}\n    body\n    body\n}}\nafter", words(30));
    let mut h = harness(St::new(&text, wrap_cfg()));
    focus(&mut h);
    let c0 = caret_at(&mut h, 0);
    let row_h = c0.height();
    let header_rows = {
        let id = h.state().id;
        let r = CodeEditor::anchor_rect(&h.ctx, id, "line[0]").unwrap();
        (r.height() / row_h).round()
    };
    assert!(header_rows >= 3.0);
    // Fold the body: line 3 ("}") follows the header's rows.
    {
        let st = h.state_mut();
        st.doc
            .execute(&mut st.view, &EditorCommand::Fold, None)
            .unwrap();
    }
    h.run();
    assert!(!h.state().view.folds().is_empty(), "folded");
    let close = text.find("\n}").unwrap() + 1;
    let c = caret_at(&mut h, close);
    assert!((c.top() - (c0.top() + header_rows * row_h)).abs() < 0.5);
    // Down from the header's last row skips the folded lines.
    caret_at(&mut h, words(30).len());
    press(&mut h, Key::ArrowDown);
    let head = h.state().head();
    assert!(head >= close, "past the fold: {head}");
}

#[test]
fn wrap_off_scrolls_horizontally_wrap_on_does_not() {
    let text = words(60);
    let mut h = harness(St::new(&text, EditorConfig::default()));
    focus(&mut h);
    caret_at(&mut h, 0);
    press(&mut h, Key::End);
    assert!(h.state().view.scroll().horizontal > 0.0);
    let mut h = harness(St::new(&text, wrap_cfg()));
    focus(&mut h);
    caret_at(&mut h, 0);
    press(&mut h, Key::End);
    press(&mut h, Key::End);
    assert_eq!(h.state().head(), text.len());
    assert_eq!(h.state().view.scroll().horizontal, 0.0);
}

#[test]
fn typing_at_a_wrap_point_reflows() {
    let text = words(20);
    let mut h = harness(St::new(&text, wrap_cfg()));
    focus(&mut h);
    let c0 = caret_at(&mut h, 0);
    let row_h = c0.height();
    let before = caret_at(&mut h, text.len());
    for _ in 0..12 {
        h.event(Event::Text("x".into()));
    }
    h.run();
    let after = caret(&h);
    assert_eq!(h.state().doc.text().len_bytes(), text.len() + 12);
    // The caret follows the text, possibly onto a new row; never past the right edge.
    assert!(after.right() < h.state().rect.right());
    assert!(after.top() >= before.top() - 0.5);
    assert!(((after.top() - c0.top()) / row_h).fract().abs() < 0.01);
}

// ---------------------------------------------------------------------------------------------
// Long lines render a prefix (E3 F2).

fn prefix_cfg(n: usize) -> EditorConfig {
    EditorConfig {
        stop_rendering_line_after: n,
        ..Default::default()
    }
}

#[test]
fn long_line_renders_a_prefix_and_carets_past_it_sit_at_the_marker() {
    let text = format!("{}\nab", "0123456789".repeat(5));
    let mut h = harness(St::new(&text, prefix_cfg(10)));
    let c0 = caret_at(&mut h, 0);
    let c10 = caret_at(&mut h, 10);
    let adv = (c10.left() - c0.left()) / 10.0;
    // Past the prefix the caret is drawn after the marker (2.5 columns), not 40 columns on.
    let c30 = caret_at(&mut h, 30);
    let c50 = caret_at(&mut h, 50);
    assert!((c30.left() - (c10.left() + 2.5 * adv)).abs() < 1.0);
    assert_eq!(c30.left(), c50.left());
    // Clicking past the marker goes to the line end; inside the prefix to the char.
    click_at(&mut h, pos2(c10.left() + 6.0 * adv, c0.center().y));
    assert_eq!(h.state().head(), 50);
    click_at(&mut h, pos2(c0.left() + 3.0 * adv, c0.center().y));
    assert_eq!(h.state().head(), 3);
    // Editing past the prefix still edits the document.
    caret_at(&mut h, 50);
    h.event(Event::Text("Z".into()));
    h.run();
    assert!(
        h.state()
            .doc
            .text()
            .to_string()
            .starts_with(&format!("{}Z\n", "0123456789".repeat(5)))
    );
    // Home/End work on the whole line.
    press(&mut h, Key::Home);
    assert_eq!(h.state().head(), 0);
    press(&mut h, Key::End);
    assert_eq!(h.state().head(), 51);
}

#[test]
fn prefix_line_a11y_run_holds_the_prefix_only() {
    let text = format!("{}\nab", "abcdefghij".repeat(3));
    let h = harness(St::new(&text, prefix_cfg(10)));
    let run = h
        .get_all_by_role(egui::accesskit::Role::TextRun)
        .find(|n| {
            n.accesskit_node()
                .value()
                .is_some_and(|v| v.starts_with("abc"))
        })
        .expect("run");
    assert_eq!(
        run.accesskit_node().value().as_deref(),
        Some("abcdefghij\n")
    );
}

#[test]
fn prefix_and_wrap_together() {
    let text = words(400);
    let cfg = EditorConfig {
        stop_rendering_line_after: 100,
        ..wrap_cfg()
    };
    let mut h = harness(St::new(&text, cfg));
    let c0 = caret_at(&mut h, 0);
    let end = caret_at(&mut h, text.len());
    let rows = ((end.top() - c0.top()) / c0.height()).round() + 1.0;
    assert!(
        (2.0..=4.0).contains(&rows),
        "100 chars wrap into 2-4 rows: {rows}"
    );
    // Past the prefix: after the marker on the last row.
    let c100 = caret_at(&mut h, 100);
    let adv = c0.height() * 0.0 + (caret_at(&mut h, 1).left() - c0.left());
    assert_eq!(c100.top(), end.top());
    assert!(
        (end.left() - (c100.left() + 2.5 * adv)).abs() < 1.0,
        "{end:?} {c100:?}"
    );
}
