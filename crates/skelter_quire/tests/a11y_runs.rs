//! Cradle CQ-2: `code-editor` exposes its text as AccessKit `TextRun` children (skelter-core
//! virtual children) with the selection pointing into them, as egui_quire does, and sends them
//! again after an edit.

use std::sync::Arc;

use quire_core::inlay_hints::{InlayHint, InlayHintsOutput};
use quire_core::{EditorConfig, EditorView, SelRange, Selection, SourceId, WordWrap};
use skelter_base::accesskit::{Action, Node, NodeId as AxId, Role, TreeUpdate};
use skelter_base::event::{
    InputEnvelope, InputEvent, KeyEvent, KeyLocation, KeyState, Modifiers, MouseButton,
    PointerEvent, PointerPhase, PointerType, ScrollDelta, WheelEvent,
};
use skelter_base::geom::{Point, Scale, Size, Vec2};
use skelter_base::ids::{InputSeq, NodeId, WindowId};
use skelter_base::key::Key;
use skelter_base::time::Time;
use skelter_core::testkinds::{BoxKind, FixedText};
use skelter_core::{Document, Kind, KindRegistry, StyleInput, Value};
use skelter_quire::EditorTheme;
use skelter_quire::a11y::TextRuns;
use skelter_quire::editor::{EditorOptions, SkelterEditor};
use skelter_quire::kind::{CodeEditorKind, QuireHost};

const W: WindowId = WindowId(1);

fn envelope(event: InputEvent, t_ms: u64, seq: u64) -> InputEnvelope {
    InputEnvelope {
        seq: InputSeq(seq),
        window: W,
        os_time: Time(t_ms * 1_000_000),
        recv_time: Time(t_ms * 1_000_000),
        event,
    }
}

fn pointer(phase: PointerPhase, p: Point, t_ms: u64, seq: u64) -> InputEnvelope {
    let e = PointerEvent {
        phase,
        pointer: 0,
        kind: PointerType::Mouse,
        button: matches!(phase, PointerPhase::Down | PointerPhase::Up)
            .then_some(MouseButton::Primary),
        buttons: if phase == PointerPhase::Down { 1 } else { 0 },
        pos: p,
        modifiers: Modifiers::default(),
        pressure: 0.5,
        tilt: Vec2::ZERO,
        history: Default::default(),
    };
    envelope(InputEvent::Pointer(e), t_ms, seq)
}

fn typed(k: Key, text: &str, t_ms: u64, seq: u64) -> InputEnvelope {
    let e = KeyEvent {
        key: Some(k),
        physical: 0,
        text: Some(text.into()),
        state: KeyState::Pressed,
        repeat: false,
        location: KeyLocation::Standard,
        modifiers: Modifiers::default(),
    };
    envelope(InputEvent::Key(e), t_ms, seq)
}

fn editor(text: &str) -> (Document, NodeId) {
    editor_with(text, EditorConfig::default(), None)
}

/// An editor on buffer `main` holding `text` with `cfg`; `root_style` styles the window's root box
/// (a padding moves the editor away from the window's origin).
fn editor_with(text: &str, cfg: EditorConfig, root_style: Option<&str>) -> (Document, NodeId) {
    let (mut d, root) = window(text, cfg, root_style);
    let n = add_editor(&mut d, root);
    (d, n)
}

fn window(text: &str, cfg: EditorConfig, root_style: Option<&str>) -> (Document, NodeId) {
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let editor = reg.register::<CodeEditorKind>().expect("code-editor");
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = skelter_text::test_fonts::SANS_FAMILY.into();
    host.insert_buffer("main", quire_core::Document::new(text, cfg));
    d.set_service(host);
    let root = d.create_window(W, boxed, Size::new(500.0, 400.0), Scale(1.0));
    if let Some(style) = root_style {
        d.set_style(
            root,
            StyleInput {
                inline: Some(style.into()),
                ..Default::default()
            },
        );
    }
    let _ = editor;
    (d, root)
}

/// A `code-editor` on buffer `main`, the last child of `root`.
fn add_editor(d: &mut Document, root: NodeId) -> NodeId {
    let kind = d
        .registry()
        .by_name(<CodeEditorKind as Kind>::spec().name)
        .expect("code-editor is registered")
        .id;
    let n = d.create(kind);
    d.set_style(
        n,
        StyleInput {
            inline: Some("width: 400px; height: 300px".into()),
            ..Default::default()
        },
    );
    d.insert(root, n, None, None).unwrap();
    let spec = <CodeEditorKind as Kind>::spec();
    d.set_prop(
        n,
        spec.prop_named("buffer").unwrap(),
        Value::Str("main".into()),
    )
    .unwrap();
    n
}

/// The editor's node in `u`, and its `TextRun` children in order.
fn runs(u: &TreeUpdate, n: NodeId) -> (&Node, Vec<(AxId, &Node)>) {
    let find = |id: AxId| u.nodes.iter().find(|(i, _)| *i == id).map(|(_, n)| n);
    let node = find(AxId(n.0)).expect("the editor is in the update");
    let kids = node
        .children()
        .iter()
        .filter_map(|&c| find(c).map(|k| (c, k)))
        .filter(|(_, k)| k.role() == Role::TextRun)
        .collect();
    (node, kids)
}

#[test]
fn the_text_is_exposed_as_text_runs_with_the_selection_in_them() {
    let (mut d, n) = editor("fn main() {}\n\nlet x = 1;");
    let c = d.frame(W, Time(0)).expect("a first commit");
    let u = c.a11y.as_ref().expect("the first frame sends the tree");
    let (node, kids) = runs(u, n);
    assert_eq!(node.role(), Role::MultilineTextInput);
    let values: Vec<_> = kids.iter().map(|(_, k)| k.value().unwrap_or("")).collect();
    assert_eq!(
        values,
        ["fn main() {}\n", "\n", "let x = 1;"],
        "one run per row, a line feed ending each line but the last"
    );
    let (_, first) = kids[0];
    assert_eq!(first.character_lengths().len(), 13);
    assert_eq!(first.character_positions().map(<[f32]>::len), Some(13));
    assert_eq!(first.character_widths().map(<[f32]>::len), Some(13));
    // `fn main() {}`: words start at `main` (3)
    assert_eq!(first.word_starts(), &[3]);
    let b0 = first.bounds().expect("bounds");
    let b2 = kids[2].1.bounds().expect("bounds");
    assert!(b2.y0 > b0.y0, "the third line is below the first");
    let sel = node.text_selection().expect("the selection names the runs");
    assert_eq!(sel.focus.node, kids[0].0);
    assert_eq!(sel.focus.character_index, 0);
    assert_eq!(sel.anchor, sel.focus);

    // a click at the end of the first line and a typed char: the runs are sent again with the
    // new text and the caret after it
    d.dispatch(&pointer(
        PointerPhase::Down,
        Point::new(390.0, 8.0),
        1_000,
        1,
    ));
    d.dispatch(&pointer(PointerPhase::Up, Point::new(390.0, 8.0), 1_050, 2));
    d.frame(W, Time(1_100_000_000));
    d.dispatch(&typed(Key::X, "x", 1_200, 3));
    let c = d
        .frame(W, Time(1_300_000_000))
        .expect("a commit after typing");
    let u = c
        .a11y
        .as_ref()
        .expect("the editor's accessibility is sent again");
    let (node, kids) = runs(u, n);
    assert_eq!(kids[0].1.value(), Some("fn main() {}x\n"));
    let sel = node.text_selection().expect("selection");
    assert_eq!(sel.focus.node, kids[0].0);
    assert_eq!(sel.focus.character_index, 13, "the caret is after `x`");
}

#[test]
fn a_long_document_exposes_a_page_around_the_caret() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (mut d, n) = editor(&text);
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (_, kids) = runs(c.a11y.as_ref().expect("tree"), n);
    assert_eq!(
        kids.len(),
        50,
        "one page of lines (skelter_quire::a11y::PAGE_LINES)"
    );
    assert_eq!(kids[0].1.value(), Some("line 0\n"));
}

fn wheel(p: Point, lines: f32, t_ms: u64, seq: u64) -> InputEnvelope {
    let e = WheelEvent {
        pos: p,
        delta: ScrollDelta::Lines(Vec2::new(0.0, lines)),
        phase: None,
        momentum: None,
        modifiers: Modifiers::default(),
    };
    envelope(InputEvent::Wheel(e), t_ms, seq)
}

/// A click (down and up) at `p`.
fn click(d: &mut Document, p: Point, t_ms: u64, seq: u64) {
    d.dispatch(&pointer(PointerPhase::Down, p, t_ms, seq));
    d.dispatch(&pointer(PointerPhase::Up, p, t_ms + 50, seq + 1));
}

fn values<'a>(kids: &[(AxId, &'a Node)]) -> Vec<&'a str> {
    kids.iter().map(|(_, k)| k.value().unwrap_or("")).collect()
}

// ---- the runs of one editor, without a skelter-core document ----

struct Fx {
    doc: quire_core::Document,
    view: EditorView,
    ed: SkelterEditor,
}

fn fx(text: &str, cfg: EditorConfig, size: Size) -> Fx {
    let doc = quire_core::Document::new(text, cfg);
    let mut view = EditorView::new(&doc);
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let theme = EditorTheme {
        font_family: skelter_text::test_fonts::SANS_FAMILY.into(),
        ..EditorTheme::default()
    };
    let mut ed = SkelterEditor::new(fonts, theme, EditorOptions::default());
    ed.layout(&doc, &mut view, size, 1.0);
    Fx { doc, view, ed }
}

impl Fx {
    fn select(&mut self, sel: Selection) {
        self.view.set_selection(&self.doc, sel);
        self.relayout();
    }
    fn relayout(&mut self) {
        let size = self.ed.geom().size;
        self.ed.layout(&self.doc, &mut self.view, size, 1.0);
    }
    fn runs(&mut self, page_start: usize) -> TextRuns {
        self.ed.a11y_runs(
            &self.doc,
            &mut self.view,
            page_start,
            skelter_core::a11y::MAX_VIRTUAL_CHILDREN,
        )
    }
    fn line_start(&self, line: usize) -> usize {
        self.doc.text().line_to_byte(line)
    }
}

/// T1: the page follows the caret by whole pages and stays put while the caret is inside it.
#[test]
fn the_page_moves_by_whole_pages_with_the_caret() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut f = fx(&text, EditorConfig::default(), Size::new(400.0, 300.0));
    let at = f.line_start(120);
    f.select(Selection::point(at));
    let r = f.runs(0);
    assert_eq!(r.page_start, 100, "(120 / 50) * 50");
    assert_eq!(r.runs.len(), 50);
    assert_eq!(r.runs[0].text, "line 100\n");
    assert_eq!(r.focus, Some((20, 0)), "the caret on the page's 21st run");
    // inside the page: it stays
    let at = f.line_start(130);
    f.select(Selection::point(at));
    assert_eq!(f.runs(100).page_start, 100);
    // before it: the page that holds the caret
    let at = f.line_start(99);
    f.select(Selection::point(at));
    let r = f.runs(100);
    assert_eq!(r.page_start, 50);
    assert_eq!(r.runs[0].text, "line 50\n");
}

/// T3: a line longer than AccessKit's 255-char runs is cut into chunks; the line feed counts.
#[test]
fn a_long_line_is_cut_into_runs_of_255_chars() {
    let text = format!("{}\nnext", "a".repeat(600));
    let mut f = fx(&text, EditorConfig::default(), Size::new(400.0, 300.0));
    f.select(Selection::point(300));
    let r = f.runs(0);
    let shape: Vec<_> = r
        .runs
        .iter()
        .map(|r| (r.line, r.chunk, r.chunks, r.char_start, r.lengths.len()))
        .collect();
    assert_eq!(
        shape,
        [
            (0, 0, 3, 0, 255),
            (0, 1, 3, 255, 255),
            (0, 2, 3, 510, 91),
            (1, 0, 1, 0, 4)
        ],
        "601 chars with the line feed: 255 + 255 + 91"
    );
    assert!(r.runs[2].text.ends_with('\n'));
    assert_eq!(
        r.focus,
        Some((1, 45)),
        "char 300 is char 45 of the second run"
    );
}

/// T3, through the kind: the chunks of one row are linked previous/next, and only they.
#[test]
fn the_chunks_of_a_row_are_linked_on_the_line() {
    let (mut d, n) = editor(&format!("{}\nnext", "a".repeat(600)));
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (_, kids) = runs(c.a11y.as_ref().expect("tree"), n);
    assert_eq!(kids.len(), 4);
    let id = |k: usize| kids[k].0;
    let link = |k: usize| (kids[k].1.previous_on_line(), kids[k].1.next_on_line());
    assert_eq!(link(0), (None, Some(id(1))));
    assert_eq!(link(1), (Some(id(0)), Some(id(2))));
    assert_eq!(link(2), (Some(id(1)), None));
    assert_eq!(link(3), (None, None), "the next line is not linked to it");
}

/// T4: soft wrap gives one run per visual row.
#[test]
fn a_wrapped_line_has_one_run_per_row() {
    let cfg = EditorConfig {
        word_wrap: WordWrap::On,
        ..EditorConfig::default()
    };
    let text = format!("{}\nnext", "word ".repeat(60));
    let mut f = fx(&text, cfg, Size::new(200.0, 300.0));
    let rows = f.ed.laid(0).expect("line 0 is laid out").rows.n_rows();
    assert!(rows > 2, "the line wraps ({rows} rows)");
    let r = f.runs(0);
    let line0: Vec<_> = r.runs.iter().filter(|r| r.line == 0).collect();
    assert_eq!(line0.len(), rows, "one run per row");
    for (k, run) in line0.iter().enumerate() {
        assert_eq!((run.row, run.chunk, run.chunks), (k, 0, 1));
    }
    let joined: String = line0.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, format!("{}\n", "word ".repeat(60)));
}

/// T4, through the kind: the rows of a wrapped line are not linked previous/next.
#[test]
fn the_rows_of_a_wrapped_line_are_not_linked() {
    let cfg = EditorConfig {
        word_wrap: WordWrap::On,
        ..EditorConfig::default()
    };
    let (mut d, n) = editor_with(&format!("{}\nnext", "word ".repeat(60)), cfg, None);
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (_, kids) = runs(c.a11y.as_ref().expect("tree"), n);
    assert!(
        kids.len() > 3,
        "the long line wraps into rows ({} runs)",
        kids.len()
    );
    for (_, k) in &kids {
        assert_eq!((k.previous_on_line(), k.next_on_line()), (None, None));
    }
}

/// T5: injected text (an inlay hint) is shown but is not in the runs.
#[test]
fn an_inlay_hint_is_not_in_the_runs() {
    let mut f = fx(
        "let x = 1;",
        EditorConfig::default(),
        Size::new(400.0, 300.0),
    );
    f.doc.set_inlay_hint_sources(vec![SourceId::new("ls")]);
    let mut request = None;
    for now in 0..5000 {
        f.view.tick(&f.doc, now);
        if let Some(InlayHintsOutput::Request(r)) = f.view.take_inlay_hints_outputs().pop() {
            request = Some(r);
            break;
        }
    }
    let r = request.expect("the view asks for hints");
    f.view
        .set_inlay_hints(&f.doc, r.id, r.version, vec![InlayHint::new(5, ": number")])
        .unwrap();
    f.relayout();
    let laid = f.ed.layout_line(&f.doc, &mut f.view, 0);
    assert_eq!(laid.display.text, "let x: number = 1;", "the hint is shown");
    let runs = f.runs(0);
    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].text, "let x = 1;");
    assert_eq!(runs.runs[0].lengths.len(), 10);
}

/// R2: a selection end on a line hidden by a fold maps to the next visible position, not to the
/// page's end.
#[test]
fn a_selection_end_inside_a_fold_maps_to_the_next_visible_line() {
    let text: String = (0..10).map(|i| format!("l{i}\n")).collect();
    let text = text.trim_end_matches('\n');
    let mut f = fx(text, EditorConfig::default(), Size::new(400.0, 300.0));
    let anchor = f.line_start(4) + 1;
    let head = f.line_start(8) + 1;
    f.select(Selection::new([SelRange::new(anchor, head)], 0));
    let t = f.doc.text();
    let fold = t.line_range(3).end..t.line_range(5).end;
    f.view.set_folds(vec![fold]);
    f.relayout();
    let r = f.runs(0);
    let lines: Vec<_> = r.runs.iter().map(|r| r.line).collect();
    assert_eq!(lines, [0, 1, 2, 3, 6, 7, 8, 9], "lines 4 and 5 are folded");
    assert_eq!(
        r.anchor,
        Some((4, 0)),
        "line 4 is hidden: the start of line 6"
    );
    assert_eq!(r.focus, Some((6, 1)));
}

/// R3: a run's bounds are in the editor node's own space (skelter-core's virtual children carry no
/// transform; the node's transform places them), so moving the editor in the window moves the
/// node's transform and leaves the runs' bounds alone.
#[test]
fn run_bounds_are_in_the_editors_own_space() {
    let text = "fn main() {}\n\nlet x = 1;";
    let (mut d0, n0) = editor_with(text, EditorConfig::default(), None);
    let (mut d1, n1) = editor_with(
        text,
        EditorConfig::default(),
        Some("padding-left: 50px; padding-top: 60px"),
    );
    let c0 = d0.frame(W, Time(0)).expect("a first commit");
    let c1 = d1.frame(W, Time(0)).expect("a first commit");
    let (node0, kids0) = runs(c0.a11y.as_ref().expect("tree"), n0);
    let (node1, kids1) = runs(c1.a11y.as_ref().expect("tree"), n1);
    assert_ne!(
        node0.transform(),
        node1.transform(),
        "the padding moves the editor node"
    );
    let b0: Vec<_> = kids0.iter().map(|(_, k)| k.bounds()).collect();
    let b1: Vec<_> = kids1.iter().map(|(_, k)| k.bounds()).collect();
    assert_eq!(b0, b1, "the runs do not move in the node's space");
    let nb = node1.bounds().expect("node bounds");
    for b in b1.iter().flatten() {
        assert!(
            b.x0 >= nb.x0 && b.y0 >= nb.y0 && b.x1 <= nb.x1 && b.y1 <= nb.y1,
            "{b:?} inside the node's {nb:?}"
        );
    }
}

/// T2: the runs are sent again after a selection change alone (no edit, no scroll).
#[test]
fn a_click_alone_sends_the_runs_again() {
    let (mut d, n) = editor("a\nb\nc");
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (_, kids) = runs(c.a11y.as_ref().expect("tree"), n);
    let mid = |k: usize| {
        let b = kids[k].1.bounds().expect("bounds");
        Point::new(300.0, ((b.y0 + b.y1) / 2.0) as f32)
    };
    let (line0, line2) = (mid(0), mid(2));
    // focus first: the focus change alone re-sends the node
    click(&mut d, line0, 1_000, 1);
    d.frame(W, Time(1_100_000_000));
    click(&mut d, line2, 3_000, 3);
    let c = d
        .frame(W, Time(3_100_000_000))
        .expect("a commit after the click");
    let u = c
        .a11y
        .as_ref()
        .expect("the editor's accessibility is sent again");
    let (node, kids) = runs(u, n);
    let sel = node.text_selection().expect("selection");
    assert_eq!(sel.focus.node, kids[2].0, "the caret is on the third line");
}

/// T2: the runs are sent again after a scroll alone (their bounds move).
#[test]
fn a_wheel_alone_sends_the_runs_again() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (mut d, n) = editor(&text);
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (_, kids) = runs(c.a11y.as_ref().expect("tree"), n);
    let y0 = kids[0].1.bounds().expect("bounds").y0;
    d.dispatch(&wheel(Point::new(200.0, 150.0), -1.0, 1_000, 1));
    let c = d
        .frame(W, Time(1_100_000_000))
        .expect("a commit after the wheel");
    let u = c
        .a11y
        .as_ref()
        .expect("the editor's accessibility is sent again");
    let (_, kids) = runs(u, n);
    assert!(
        kids[0].1.bounds().expect("bounds").y0 < y0,
        "the runs moved up with the scroll"
    );
}

/// T6: `SetValue` and `ReplaceSelectedText` only while the editor is editable.
#[test]
fn a_read_only_editor_offers_no_edit_actions() {
    let (mut d, n) = editor("let x = 1;");
    let c = d.frame(W, Time(0)).expect("a first commit");
    let (node, _) = runs(c.a11y.as_ref().expect("tree"), n);
    assert!(!node.is_read_only());
    assert!(node.supports_action(Action::SetValue));
    assert!(node.supports_action(Action::ReplaceSelectedText));
    let spec = <CodeEditorKind as Kind>::spec();
    d.set_prop(n, spec.prop_named("readonly").unwrap(), Value::Bool(true))
        .unwrap();
    let c = d.frame(W, Time(1_000_000_000)).expect("a commit");
    let (node, _) = runs(c.a11y.as_ref().expect("tree"), n);
    assert!(node.is_read_only());
    assert!(!node.supports_action(Action::SetValue));
    assert!(!node.supports_action(Action::ReplaceSelectedText));
}

/// R1: a second editor on the same buffer sends its runs again after an edit in the first.
#[test]
#[ignore = "R1, fixed in the next commit"]
fn a_second_editor_on_the_buffer_follows_an_edit_in_the_first() {
    let (mut d, root) = window("fn main() {}\nlet x = 1;", EditorConfig::default(), None);
    let a = add_editor(&mut d, root);
    let b = add_editor(&mut d, root);
    d.frame(W, Time(0)).expect("a first commit");
    // type at the start of editor A (the first; B is laid out below it)
    click(&mut d, Point::new(390.0, 8.0), 1_000, 1);
    d.frame(W, Time(1_100_000_000));
    d.dispatch(&typed(Key::X, "x", 1_200, 3));
    let c = d
        .frame(W, Time(1_300_000_000))
        .expect("a commit after typing");
    let u = c.a11y.as_ref().expect("accessibility is sent");
    let (_, ka) = runs(u, a);
    assert_eq!(values(&ka)[0], "fn main() {}x\n", "A has the edit");
    assert!(
        u.nodes.iter().any(|(i, _)| *i == AxId(b.0)),
        "B is sent again"
    );
    let (_, kb) = runs(u, b);
    assert_eq!(values(&kb)[0], "fn main() {}x\n", "B has the edit too");
}

/// R1: an edit through the platform's text-input client (an IME commit) sends the runs again.
#[test]
fn a_text_input_client_edit_sends_the_runs_again() {
    let (mut d, n) = editor("fn main() {}");
    d.frame(W, Time(0)).expect("a first commit");
    click(&mut d, Point::new(390.0, 8.0), 1_000, 1);
    d.frame(W, Time(1_100_000_000));
    d.with_text_input(W, |c| c.replace_text_in_range(None, "y"))
        .expect("the focused editor is a text-input client");
    let c = d.frame(W, Time(1_200_000_000)).expect("a commit");
    let u = c.a11y.as_ref().expect("accessibility is sent");
    let (_, kids) = runs(u, n);
    assert_eq!(values(&kids), ["fn main() {}y"]);
}
