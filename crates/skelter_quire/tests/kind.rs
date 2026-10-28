//! `code-editor` in a skelter-core document: it lays out, records, takes focus on a click, types,
//! and reports to the host's outbox and to JS.

use std::sync::Arc;

use quire_core::EditorConfig;
use skelter_base::event::{
    InputEnvelope, InputEvent, KeyEvent, KeyLocation, KeyState, Modifiers, MouseButton,
    PointerEvent, PointerPhase, PointerType,
};
use skelter_base::geom::{Point, Scale, Size, Vec2};
use skelter_base::ids::{InputSeq, WindowId};
use skelter_base::key::Key;
use skelter_base::time::Time;
use skelter_core::testkinds::{BoxKind, FixedText};
use skelter_core::{Document, Kind, KindRegistry, ListenFlags, StyleInput, Value};
use skelter_quire::EditorEvent;
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

#[test]
fn records_types_and_reports() {
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let editor = reg.register::<CodeEditorKind>().expect("code-editor");
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = skelter_text::test_fonts::SANS_FAMILY.into();
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
    // JS listens to `change` and `selectionchange`; skelter-core queues a kind's signal for JS
    // only when a listener is on its path
    let change = spec.signal_named("change").unwrap();
    let selectionchange = spec.signal_named("selectionchange").unwrap();
    d.listen(n, change, ListenFlags::default());
    d.listen(n, selectionchange, ListenFlags::default());

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
            .any(|p| matches!(p, skelter_base::display::Prim::Glyphs(_))),
        "text is drawn"
    );
    assert!(!c.runs.is_empty());

    // a click focuses it and puts the caret on the first line; typing goes into the document.
    // Each dispatch returns the JS-facing events it produced.
    let mut events = Vec::new();
    events.extend(
        d.dispatch(&pointer(
            PointerPhase::Down,
            Point::new(100.0, 8.0),
            1_000,
            1,
        ))
        .events,
    );
    events.extend(
        d.dispatch(&pointer(PointerPhase::Up, Point::new(100.0, 8.0), 1_050, 2))
            .events,
    );
    assert_eq!(d.focused(W), Some(n));
    events.extend(d.dispatch(&typed(Key::X, "x", 1_100, 3)).events);
    assert!(
        buf.borrow().text().to_string().contains('x'),
        "{:?}",
        buf.borrow().text().to_string()
    );
    events.extend(d.take_events());
    assert!(
        events.iter().any(|e| e.target == n && e.signal == change),
        "change reaches JS"
    );
    assert!(
        events
            .iter()
            .any(|e| e.target == n && e.signal == selectionchange),
        "selectionchange reaches JS"
    );
    let host = d.service::<QuireHost>().unwrap();
    assert!(
        host.events
            .iter()
            .any(|(id, e)| *id == n && *e == EditorEvent::FocusChanged(true)),
        "{:?}",
        host.events
    );
}

/// A host's own kind under the tag `code-editor` (cadatomic's gallery registers quire's editor as
/// `gallery-source` beside it).
#[derive(Default)]
struct HostEditor;

static HOST_EDITOR: skelter_core::KindSpec = skelter_core::KindSpec {
    props: &[skelter_core::PropSpec::new(
        "buffer",
        skelter_core::ValueType::Str,
        skelter_base::change::ChangeClass::Paint,
    )],
    ..skelter_core::KindSpec::new("code-editor")
};

impl Kind for HostEditor {
    fn spec() -> &'static skelter_core::KindSpec {
        &HOST_EDITOR
    }
    fn record(&self, _cx: &mut skelter_core::RecordCx) {}
}

/// Puts an editor node of kind `kind` showing `buffer` into `root`, readonly or not, and
/// returns it.
fn editor_node(
    d: &mut Document,
    root: skelter_base::ids::NodeId,
    kind: skelter_core::KindId,
    top: f32,
    buffer: &str,
    readonly: bool,
) -> skelter_base::ids::NodeId {
    let n = d.create(kind);
    d.set_style(
        n,
        StyleInput {
            inline: Some(
                format!("position: absolute; left: 0px; top: {top}px; width: 400px; height: 150px")
                    .into(),
            ),
            ..Default::default()
        },
    );
    d.insert(root, n, None, None).unwrap();
    let spec = d.registry().get(kind).unwrap().spec;
    d.set_prop(
        n,
        spec.prop_named("buffer").unwrap(),
        Value::Str(buffer.into()),
    )
    .unwrap();
    d.set_prop(
        n,
        spec.prop_named("readonly").unwrap(),
        Value::Bool(readonly),
    )
    .unwrap();
    n
}

fn draws_glyphs(c: &skelter_base::commit::Commit, n: skelter_base::ids::NodeId) -> bool {
    c.lists.iter().any(|(id, l)| {
        *id == n
            && l.items
                .iter()
                .any(|p| matches!(p, skelter_base::display::Prim::Glyphs(_)))
    })
}

/// Two registrations of the editor under different tags in one registry: each node takes its own
/// props (`buffer`, `readonly`) and renders.
#[test]
fn register_as_two_tags_take_their_props() {
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let plain = CodeEditorKind::register(&mut reg).expect("code-editor");
    let gallery = CodeEditorKind::register_as(&mut reg, "gallery-source").expect("gallery-source");
    assert_ne!(plain, gallery);
    assert_eq!(
        reg.get(gallery).unwrap().spec.name,
        "gallery-source",
        "the copy carries the new tag"
    );
    // registering a tag twice is refused, as for any kind
    assert!(CodeEditorKind::register_as(&mut reg, "gallery-source").is_err());
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = skelter_text::test_fonts::SANS_FAMILY.into();
    let a = host.insert_buffer(
        "a",
        quire_core::Document::new("alpha", EditorConfig::default()),
    );
    let b = host.insert_buffer(
        "b",
        quire_core::Document::new("beta", EditorConfig::default()),
    );
    d.set_service(host);
    let root = d.create_window(W, boxed, Size::new(500.0, 400.0), Scale(1.0));
    let na = editor_node(&mut d, root, plain, 0.0, "a", false);
    let nb = editor_node(&mut d, root, gallery, 200.0, "b", true);
    let c = d.frame(W, Time(0)).expect("a first commit");
    assert!(draws_glyphs(&c, na), "code-editor renders its buffer");
    assert!(draws_glyphs(&c, nb), "gallery-source renders its buffer");

    // click and type into each: `readonly` reached the gallery node only
    let mut seq = 0;
    let mut type_into = |d: &mut Document, y: f32, t: u64| {
        seq += 3;
        d.dispatch(&pointer(PointerPhase::Down, Point::new(100.0, y), t, seq));
        d.dispatch(&pointer(
            PointerPhase::Up,
            Point::new(100.0, y),
            t + 50,
            seq + 1,
        ));
        d.dispatch(&typed(Key::X, "x", t + 100, seq + 2));
    };
    type_into(&mut d, 8.0, 1_000);
    assert_eq!(d.focused(W), Some(na));
    type_into(&mut d, 208.0, 2_000);
    assert_eq!(d.focused(W), Some(nb));
    assert!(
        a.borrow().text().to_string().contains('x'),
        "code-editor is editable"
    );
    assert_eq!(
        b.borrow().text().to_string(),
        "beta",
        "gallery-source is readonly"
    );

    // the gallery node's signals are its own tag's
    let host = d.service::<QuireHost>().unwrap();
    assert!(
        host.events
            .iter()
            .any(|(id, e)| *id == nb && *e == EditorEvent::FocusChanged(true)),
        "{:?}",
        host.events
    );
}

/// A host's own `code-editor` kind and quire's editor as `gallery-source` in one registry.
#[test]
fn register_as_beside_a_host_code_editor() {
    let mut reg = KindRegistry::new();
    let boxed = reg.register::<BoxKind>().expect("box");
    let theirs = reg
        .register::<HostEditor>()
        .expect("the host's code-editor");
    let gallery = CodeEditorKind::register_as(&mut reg, "gallery-source").expect("gallery-source");
    // quire's own tag is taken
    assert!(CodeEditorKind::register(&mut reg).is_err());
    let mut d = Document::new(Arc::new(reg), Box::new(FixedText));
    d.set_text_shaper(Box::new(FixedText));
    let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
    let mut host = QuireHost::new(fonts);
    host.theme.font_family = skelter_text::test_fonts::SANS_FAMILY.into();
    host.insert_buffer(
        "src",
        quire_core::Document::new("let x = 1;", EditorConfig::default()),
    );
    d.set_service(host);
    let root = d.create_window(W, boxed, Size::new(500.0, 400.0), Scale(1.0));
    let other = d.create(theirs);
    d.insert(root, other, None, None).unwrap();
    d.set_prop(
        other,
        HOST_EDITOR.prop_named("buffer").unwrap(),
        Value::Str("src".into()),
    )
    .unwrap();
    let ng = editor_node(&mut d, root, gallery, 0.0, "src", true);
    let c = d.frame(W, Time(0)).expect("a first commit");
    assert!(draws_glyphs(&c, ng), "gallery-source renders");
    assert!(!draws_glyphs(&c, other), "the host's kind is its own");
}
