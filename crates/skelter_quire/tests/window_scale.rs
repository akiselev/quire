//! Cradle CQ-1: `code-editor` lays out at the scale of the window its node is in
//! (`NodeView::scale`), not at one scale per host, so editors in windows of different scales, and
//! an editor whose window changes scale, shape for their own device pixels.

use std::sync::Arc;

use quire_core::EditorConfig;
use skelter_base::display::Prim;
use skelter_base::geom::{Scale, Size};
use skelter_base::ids::{NodeId, WindowId};
use skelter_base::time::Time;
use skelter_core::testkinds::{BoxKind, FixedText};
use skelter_core::{Document, Kind, KindRegistry, StyleInput, Value};
use skelter_quire::kind::{CodeEditorKind, QuireHost};

/// A document with one window at `scale` holding one editor over `int x = 1;`.
fn editor_in_window(window: WindowId, scale: f32) -> (Document, NodeId) {
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
        quire_core::Document::new("int x = 1;", EditorConfig::default()),
    );
    d.set_service(host);
    let root = d.create_window(window, boxed, Size::new(500.0, 400.0), Scale(scale));
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

/// The origins of the glyph runs node `n` recorded in its last list.
fn glyph_origins(c: &skelter_base::commit::Commit, n: NodeId) -> Vec<(f32, f32)> {
    let list = c
        .lists
        .iter()
        .find(|(id, _)| *id == n)
        .map(|(_, l)| l)
        .expect("the editor recorded");
    list.items
        .iter()
        .filter_map(|p| match p {
            Prim::Glyphs(g) => Some((g.origin.x, g.origin.y)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_editor_lays_out_at_its_windows_scale() {
    let w = WindowId(1);
    let (mut d1, n1) = editor_in_window(w, 1.0);
    let at_1 = glyph_origins(&d1.frame(w, Time(0)).expect("a commit"), n1);
    let (mut d2, n2) = editor_in_window(w, 2.0);
    let at_2 = glyph_origins(&d2.frame(w, Time(0)).expect("a commit"), n2);
    assert!(!at_1.is_empty() && !at_2.is_empty(), "text is drawn");
    assert_ne!(
        at_1, at_2,
        "the text origin snaps to the window's device pixels (QuireHost has no scale to follow)"
    );

    // the window moves to a screen of scale 2: the editor re-lays out as one created there
    d1.resize_window(w, Size::new(500.0, 400.0), Scale(2.0));
    let moved = glyph_origins(&d1.frame(w, Time(16_000_000)).expect("a commit"), n1);
    assert_eq!(moved, at_2, "after the scale change the editor draws at scale 2");
}
