//! Cradle CQ-2: `code-editor` exposes its text as AccessKit `TextRun` children (skelter-core
//! virtual children) with the selection pointing into them, as egui_quire does, and sends them
//! again after an edit.

use std::sync::Arc;

use quire_core::EditorConfig;
use skelter_base::accesskit::{Node, NodeId as AxId, Role, TreeUpdate};
use skelter_base::event::{
    InputEnvelope, InputEvent, KeyEvent, KeyLocation, KeyState, Modifiers, MouseButton,
    PointerEvent, PointerPhase, PointerType,
};
use skelter_base::geom::{Point, Scale, Size, Vec2};
use skelter_base::ids::{InputSeq, NodeId, WindowId};
use skelter_base::key::Key;
use skelter_base::time::Time;
use skelter_core::testkinds::{BoxKind, FixedText};
use skelter_core::{Document, Kind, KindRegistry, StyleInput, Value};
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
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let editor = reg.register::<CodeEditorKind>().expect("code-editor");
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = skelter_text::test_fonts::SANS_FAMILY.into();
    host.insert_buffer(
        "main",
        quire_core::Document::new(text, EditorConfig::default()),
    );
    d.set_service(host);
    let root = d.create_window(W, boxed, Size::new(500.0, 400.0), Scale(1.0));
    let n = d.create(editor);
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
    (d, n)
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
