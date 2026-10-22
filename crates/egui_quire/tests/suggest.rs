//! egui_kittest suite for the suggest widget: trigger (Ctrl+Space and typing), filtering as you
//! type, accepting with Enter and Tab, Escape, the details toggle and the list's AccessKit roles.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Vec2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_quire::{CodeEditor, EditorEvent};
use quire_core::completion::{
    CompletionItem, CompletionItemKind as K, CompletionList, CompletionSource,
};
use quire_core::{Document, EditorView, SourceId};

struct St {
    doc: Document,
    view: EditorView,
    labels: Vec<&'static str>,
    requests: usize,
    caret: Option<egui::Rect>,
    rect: egui::Rect,
}

impl St {
    fn new(text: &str, labels: Vec<&'static str>) -> Self {
        let mut doc = Document::new(text, Default::default());
        doc.set_completion_sources(vec![CompletionSource {
            id: SourceId::new("test"),
            trigger_characters: vec![".".into()],
        }]);
        let view = EditorView::new(&doc);
        St {
            doc,
            view,
            labels,
            requests: 0,
            caret: None,
            rect: egui::Rect::NOTHING,
        }
    }

    fn text(&self) -> String {
        self.doc.text().to_string()
    }
}

/// The word before `pos` (the default suggest range).
fn word_start(doc: &Document, pos: usize) -> usize {
    let t = doc.text().slice(0..pos).to_string();
    t.rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1)
}

fn app(ui: &mut egui::Ui, st: &mut St) {
    let rect = ui.available_rect_before_wrap();
    st.rect = rect;
    let out = CodeEditor::new(&mut st.doc, &mut st.view, "ed", "Source").show_in(ui, rect);
    st.caret = out.caret_rect;
    // A synchronous host: answer every request at once.
    for e in out.events {
        if let EditorEvent::CompletionRequested {
            id, pos, version, ..
        } = e
        {
            st.requests += 1;
            let start = word_start(&st.doc, pos);
            let items = st
                .labels
                .iter()
                .map(|l| {
                    let mut c = CompletionItem::new(*l, K::Variable, start..pos);
                    c.detail = Some(format!("detail of {l}"));
                    c.documentation = Some(format!("Docs for {l}."));
                    c
                })
                .collect();
            let _ = st.view.set_completions(
                &st.doc,
                id,
                version,
                vec![CompletionList {
                    source: SourceId::new("test"),
                    items,
                    incomplete: false,
                }],
            );
        }
    }
}

fn harness(st: St) -> Harness<'static, St> {
    Harness::builder()
        .with_size(Vec2::new(800.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st)
}

fn focus(h: &mut Harness<'_, St>) {
    let r = h.state().rect;
    let p = pos2(r.right() - 20.0, r.bottom() - 20.0);
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

fn type_text(h: &mut Harness<'_, St>, s: &str) {
    for ch in s.chars() {
        h.event(Event::Text(ch.to_string()));
        h.run();
    }
}

fn options(h: &Harness<'_, St>) -> Vec<String> {
    h.query_all_by_role(Role::ListBoxOption)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect()
}

fn list_visible(h: &Harness<'_, St>) -> bool {
    h.query_by_role(Role::ListBox).is_some()
}

#[test]
fn ctrl_space_opens_the_list_with_a11y_roles() {
    let mut h = harness(St::new("let x = ", vec!["console", "const", "constructor"]));
    focus(&mut h);
    assert!(!list_visible(&h));
    h.key_press_modifiers(Modifiers::CTRL, Key::Space);
    h.run();
    assert_eq!(h.state().requests, 1);
    let list = h.get_by_role(Role::ListBox);
    assert_eq!(list.accesskit_node().label().as_deref(), Some("Suggest"));
    assert_eq!(options(&h), ["console", "const", "constructor"]);
    let first = h.get_by_role_and_label(Role::ListBoxOption, "console");
    assert_eq!(first.accesskit_node().is_selected(), Some(true));
    let second = h.get_by_role_and_label(Role::ListBoxOption, "const");
    assert_eq!(second.accesskit_node().is_selected(), Some(false));
}

#[test]
fn typing_triggers_and_filters_as_you_type() {
    let mut h = harness(St::new("", vec!["console", "const", "parse", "process"]));
    focus(&mut h);
    type_text(&mut h, "c");
    assert!(list_visible(&h), "a quick suggestion opens the list");
    assert_eq!(options(&h), ["console", "const"]);
    type_text(&mut h, "ons");
    assert_eq!(options(&h), ["console", "const"]);
    type_text(&mut h, "o");
    assert_eq!(options(&h), ["console"]);
    assert_eq!(
        h.state().requests,
        1,
        "typing in a session filters, it does not ask again"
    );
    type_text(&mut h, "z");
    assert!(
        !list_visible(&h),
        "nothing matches: the automatic list closes"
    );
}

#[test]
fn enter_and_tab_accept() {
    let mut h = harness(St::new("", vec!["console", "const"]));
    focus(&mut h);
    type_text(&mut h, "co");
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        h.get_by_role_and_label(Role::ListBoxOption, "const")
            .accesskit_node()
            .is_selected(),
        Some(true)
    );
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text(), "const");
    assert!(!list_visible(&h));

    type_text(&mut h, " co");
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(h.state().text(), "const console");
}

#[test]
fn escape_hides_and_keeps_the_text() {
    let mut h = harness(St::new("", vec!["console"]));
    focus(&mut h);
    type_text(&mut h, "con");
    assert!(list_visible(&h));
    h.key_press(Key::Escape);
    h.run();
    assert!(!list_visible(&h));
    assert_eq!(h.state().text(), "con");
    // Escape went to the list, not to the host.
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(
        h.state().text(),
        "con\n",
        "Enter without the list types a line break"
    );
}

#[test]
fn click_accepts_and_details_toggle() {
    let mut h = harness(St::new("", vec!["alpha", "alphabet"]));
    focus(&mut h);
    h.key_press_modifiers(Modifiers::CTRL, Key::Space);
    h.run();
    // Ctrl+Space with the list open toggles the details panel.
    assert!(h.query_by_label("Docs for alpha.").is_none());
    h.key_press_modifiers(Modifiers::CTRL, Key::Space);
    h.run();
    assert!(h.query_by_label("Docs for alpha.").is_some());
    let row = h.get_by_role_and_label(Role::ListBoxOption, "alphabet");
    row.click();
    h.run();
    assert_eq!(h.state().text(), "alphabet");
    assert!(!list_visible(&h));
}
