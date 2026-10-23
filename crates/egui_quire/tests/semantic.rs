//! egui_kittest suite for semantic tokens through the widget: the request event, a
//! synchronous host's answer, a delta request after an edit naming the previous result, and the
//! release of the replaced result.

use egui::Vec2;
use egui_kittest::Harness;
use egui_quire::{CodeEditor, EditorEvent};
use quire_core::semantic_tokens::{
    SemanticTokens, SemanticTokensLegend, SemanticTokensReply, SemanticTokensSource,
};
use quire_core::syntax::TokenType;
use quire_core::{Document, EditorView, SourceId};

struct St {
    doc: Document,
    view: EditorView,
    requests: Vec<Option<(SourceId, String)>>,
    released: Vec<Option<String>>,
    rect: egui::Rect,
}

fn app(ui: &mut egui::Ui, st: &mut St) {
    let rect = ui.available_rect_before_wrap();
    st.rect = rect;
    let out = CodeEditor::new(&mut st.doc, &mut st.view, "ed", "Source").show_in(ui, rect);
    for e in out.events {
        match e {
            EditorEvent::SemanticTokensRequested { id, previous, .. } => {
                let n = st.requests.len() + 1;
                st.requests.push(previous);
                // `foo` is a class
                let start = st.doc.text().to_string().find("foo").unwrap() as u32;
                let reply = SemanticTokensReply::Tokens(SemanticTokens {
                    result_id: Some(format!("r{n}")),
                    data: vec![0, start, 3, 0, 0],
                });
                st.doc
                    .set_semantic_tokens(id, vec![(SourceId::new("ls"), reply)])
                    .unwrap();
            }
            EditorEvent::SemanticTokensReleased { result_id, .. } => st.released.push(result_id),
            _ => {}
        }
    }
}

#[test]
fn semantic_tokens_are_asked_and_follow_edits() {
    let mut doc = Document::new("let foo = 1;", Default::default());
    doc.set_semantic_tokens_sources(vec![SemanticTokensSource {
        id: SourceId::new("ls"),
        legend: SemanticTokensLegend {
            token_types: vec!["class".into()],
            token_modifiers: vec![],
        },
    }]);
    let view = EditorView::new(&doc);
    let st = St {
        doc,
        view,
        requests: Vec::new(),
        released: Vec::new(),
        rect: egui::Rect::NOTHING,
    };
    let mut h = Harness::builder()
        .with_size(Vec2::new(600.0, 200.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st);
    for _ in 0..4 {
        h.step();
    }
    h.run();
    assert_eq!(h.state().requests, vec![None]);
    let class_at = |st: &St| -> Vec<u32> {
        st.doc
            .line_tokens(0)
            .iter()
            .filter(|t| t.style.ty() == TokenType::Class)
            .map(|t| t.start)
            .collect()
    };
    assert_eq!(class_at(h.state()), vec![4]);
    // an edit at the start: the token moves at once, and is asked again after the debounce
    {
        let st = h.state_mut();
        let len = st.doc.text().len_bytes();
        let cs = quire_core::ChangeSet::replace(len, [(0..0, "// ")]).unwrap();
        let tr = quire_core::Transaction::new(st.doc.version(), cs);
        st.doc.apply(tr, &mut st.view).unwrap();
    }
    h.run();
    assert_eq!(h.state().doc.text().to_string(), "// let foo = 1;");
    assert_eq!(class_at(h.state()), vec![7]);
    for _ in 0..60 {
        h.step();
    }
    h.run();
    assert_eq!(
        h.state().requests,
        vec![None, Some((SourceId::new("ls"), "r1".to_string()))]
    );
    assert_eq!(h.state().released, vec![Some("r1".to_string())]);
    assert_eq!(class_at(h.state()), vec![7]);
}
