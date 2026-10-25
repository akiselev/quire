//! `code-editor` in a presto-core document: it lays out, records, takes focus on a click, types,
//! and reports to the host's outbox and to JS.

use std::sync::Arc;

use presto_base::event::{
    InputEnvelope, InputEvent, KeyEvent, KeyLocation, KeyState, Modifiers, MouseButton,
    PointerEvent, PointerPhase, PointerType,
};
use presto_base::geom::{Point, Scale, Size, Vec2};
use presto_base::ids::{InputSeq, WindowId};
use presto_base::key::Key;
use presto_base::time::Time;
use presto_core::testkinds::{BoxKind, FixedText};
use presto_core::{Document, Kind, KindRegistry, StyleInput, Value};
use presto_quire::EditorEvent;
use presto_quire::kind::{CodeEditorKind, QuireHost};
use quire_core::EditorConfig;

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

#[test]
fn records_types_and_reports() {
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let editor = reg.register::<CodeEditorKind>().expect("code-editor");
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = presto_text::FontStore::new(presto_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = presto_text::test_fonts::SANS_FAMILY.into();
    let buf = host.insert_buffer(
        "main",
        quire_core::Document::new("hello\nworld", EditorConfig::default()),
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

    let c = d.frame(W, Time(0)).expect("a first commit");
    let list = c
        .lists
        .iter()
        .find(|(id, _)| *id == n)
        .map(|(_, l)| l.clone())
        .expect("the editor recorded");
    assert!(
        list.items
            .iter()
            .any(|p| matches!(p, presto_base::display::Prim::Glyphs(_))),
        "text is drawn"
    );
    assert!(!c.runs.is_empty());

    // a click focuses it and puts the caret on the first line; typing goes into the document
    d.dispatch(&pointer(
        PointerPhase::Down,
        Point::new(100.0, 8.0),
        1_000,
        1,
    ));
    d.dispatch(&pointer(PointerPhase::Up, Point::new(100.0, 8.0), 1_050, 2));
    assert_eq!(d.focused(W), Some(n));
    d.dispatch(&typed(Key::X, "x", 1_100, 3));
    assert!(
        buf.borrow().text().to_string().contains('x'),
        "{:?}",
        buf.borrow().text().to_string()
    );
    let events = d.take_events();
    assert!(!events.is_empty(), "change / selectionchange reach JS");
    let host = d.service::<QuireHost>().unwrap();
    assert!(
        host.events
            .iter()
            .any(|(id, e)| *id == n && *e == EditorEvent::FocusChanged(true)),
        "{:?}",
        host.events
    );
}
