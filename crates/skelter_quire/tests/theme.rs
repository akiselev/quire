//! Cradle batch 10 (adoption Q3): a theme the host sets on `QuireHost` after the editors exist
//! reaches them on the next frame (the host calls `Document::repaint_all` after changing the
//! service, as for the widgets' theme).

use std::sync::Arc;

use quire_core::EditorConfig;
use skelter_base::color::{Color, Fill};
use skelter_base::display::Prim;
use skelter_base::geom::{Scale, Size};
use skelter_base::ids::{NodeId, WindowId};
use skelter_base::time::Time;
use skelter_core::testkinds::{BoxKind, FixedText};
use skelter_core::{Document, Kind, KindRegistry, StyleInput, Value};
use skelter_quire::kind::{CodeEditorKind, QuireHost};

const W: WindowId = WindowId(1);

fn editor() -> (Document, NodeId) {
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

/// The solid fills node `n` recorded.
fn fills(c: &skelter_base::commit::Commit, n: NodeId) -> Vec<Color> {
    let list = c
        .lists
        .iter()
        .find(|(id, _)| *id == n)
        .map(|(_, l)| l)
        .expect("the editor recorded");
    list.items
        .iter()
        .filter_map(|p| match p {
            Prim::Quad(q) => match q.fill {
                Fill::Solid(c) => Some(c),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn a_theme_set_on_the_host_reaches_an_existing_editor() {
    let (mut d, n) = editor();
    let before = d.service::<QuireHost>().unwrap().theme.background;
    let c = d.frame(W, Time(0)).expect("a first commit");
    // the first fill is the editor's background
    assert_eq!(
        fills(&c, n).first(),
        Some(&before),
        "drawn on the first theme"
    );

    let dark = Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 1.0,
    };
    assert_ne!(dark, before);
    d.service::<QuireHost>().unwrap().theme.background = dark;
    d.repaint_all();
    let c = d
        .frame(W, Time(16_000_000))
        .expect("a commit after the theme change");
    assert_eq!(
        fills(&c, n).first(),
        Some(&dark),
        "the editor draws on the new background"
    );
}
