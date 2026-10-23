//! egui_kittest suite for the hover and the parameter hints: opening (mouse rest,
//! `editor.action.showHover`, trigger characters, Ctrl+Shift+Space), contents (diagnostics with
//! Markdown, the copy button, links, the active parameter), Escape, AccessKit roles and overload
//! cycling.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Vec2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_quire::{CodeEditor, EditorEvent};
use quire_core::hover::Hover;
use quire_core::parameter_hints::{
    Documentation, ParameterInformation, SignatureHelp, SignatureHelpSource, SignatureInformation,
};
use quire_core::{Diagnostic, Document, EditorView, Selection, Severity, SourceId};

struct St {
    doc: Document,
    view: EditorView,
    hover_requests: usize,
    sig_requests: Vec<quire_core::parameter_hints::SignatureHelpContext>,
    /// What the signature help provider answers (None: the cursor left the call).
    sig_answer: Option<SignatureHelp>,
    links: Vec<String>,
    caret: Option<egui::Rect>,
    rect: egui::Rect,
}

fn overloads() -> SignatureHelp {
    let mut a = SignatureInformation::new(
        "add(a: i32, b: i32) -> i32",
        vec![
            ParameterInformation {
                label: quire_core::parameter_hints::ParameterLabel::Text("a: i32".into()),
                documentation: Some(Documentation::Plain("the first".into())),
            },
            ParameterInformation::new("b: i32"),
        ],
    );
    a.documentation = Some(Documentation::Markdown("Adds **two** numbers.".into()));
    let b = SignatureInformation::new(
        "add(a: f64, b: f64) -> f64",
        vec![
            ParameterInformation::new("a: f64"),
            ParameterInformation::new("b: f64"),
        ],
    );
    SignatureHelp {
        signatures: vec![a, b],
        active_signature: 0,
        active_parameter: 0,
    }
}

impl St {
    fn new(text: &str) -> Self {
        let mut doc = Document::new(text, Default::default());
        doc.set_hover_sources(vec![SourceId::new("lsp")]);
        doc.set_signature_help_sources(vec![SignatureHelpSource {
            id: SourceId::new("lsp"),
            trigger_characters: vec!["(".into(), ",".into()],
            retrigger_characters: vec![")".into()],
        }]);
        doc.set_diagnostics(
            SourceId::new("check"),
            doc.version(),
            vec![Diagnostic::new(
                4..7,
                Severity::Error,
                "unused variable `foo`",
            )],
        )
        .unwrap();
        let view = EditorView::new(&doc);
        St {
            doc,
            view,
            hover_requests: 0,
            sig_requests: Vec::new(),
            sig_answer: Some(overloads()),
            links: Vec::new(),
            caret: None,
            rect: egui::Rect::NOTHING,
        }
    }
}

fn app(ui: &mut egui::Ui, st: &mut St) {
    let rect = ui.available_rect_before_wrap();
    st.rect = rect;
    let out = CodeEditor::new(&mut st.doc, &mut st.view, "ed", "Source").show_in(ui, rect);
    st.caret = out.caret_rect;
    // A synchronous host.
    for e in out.events {
        match e {
            EditorEvent::HoverAt { id, version, .. } => {
                st.hover_requests += 1;
                let h = Hover {
                    source: SourceId::new("lsp"),
                    contents: vec![
                        "```rust\nlet foo: i32\n```".into(),
                        "The **foo** binding. See [docs](https://example.com/foo).".into(),
                    ],
                    range: Some(4..7),
                };
                st.view.set_hover(&st.doc, id, version, vec![h]).unwrap();
            }
            EditorEvent::SignatureHelpRequested {
                id,
                version,
                context,
                ..
            } => {
                st.sig_requests.push(context);
                let a = st.sig_answer.clone();
                st.view.set_signature_help(&st.doc, id, version, a).unwrap();
            }
            EditorEvent::HoverLinkActivated { href } => st.links.push(href),
            _ => {}
        }
    }
}

fn harness(st: St) -> Harness<'static, St> {
    Harness::builder()
        .with_size(Vec2::new(800.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st)
}

fn click(h: &mut Harness<'_, St>, p: egui::Pos2) {
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

fn focus(h: &mut Harness<'_, St>) {
    let r = h.state().rect;
    click(h, pos2(r.right() - 20.0, r.bottom() - 20.0));
}

/// Screen position of byte `pos` (the caret placed there, then restored).
fn point_of(h: &mut Harness<'_, St>, pos: usize) -> egui::Pos2 {
    let st = h.state_mut();
    let old = st.view.selection().clone();
    st.view.set_selection(&st.doc, Selection::point(pos));
    h.run();
    let c = h.state().caret.unwrap();
    let st = h.state_mut();
    st.view.set_selection(&st.doc, old);
    h.run();
    c.center()
}

/// Lets `secs` of (simulated) time pass.
fn wait(h: &mut Harness<'_, St>, secs: f32) {
    for _ in 0..(secs * 60.0).ceil() as usize {
        h.step();
    }
    h.run();
}

fn tooltips(h: &Harness<'_, St>) -> Vec<String> {
    h.query_all_by_role(Role::Tooltip)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect()
}

fn hover_label(h: &Harness<'_, St>) -> Option<String> {
    tooltips(h).into_iter().find(|l| l.contains("foo"))
}

fn type_text(h: &mut Harness<'_, St>, s: &str) {
    for ch in s.chars() {
        h.event(Event::Text(ch.to_string()));
        h.run();
    }
}

#[test]
fn hover_shows_diagnostics_and_markdown_with_copy_buttons() {
    let mut h = harness(St::new("let foo = 1;\n"));
    focus(&mut h);
    let p = point_of(&mut h, 5);
    h.event(Event::PointerMoved(p));
    h.run();
    wait(&mut h, 0.1);
    assert!(hover_label(&h).is_none(), "not before editor.hover.delay");
    wait(&mut h, 0.4);
    assert_eq!(h.state().hover_requests, 1);
    let label = hover_label(&h).expect("hover shown");
    // Diagnostics first, then the provider's parts.
    let diag = label.find("unused variable").unwrap();
    let md = label.find("The **foo** binding").unwrap();
    assert!(diag < md, "{label}");
    assert!(label.contains("let foo: i32"));
    // The rendered Markdown (strong text, the code block).
    assert!(h.query_all_by_label("foo").count() >= 1);
    assert!(h.query_all_by_label_contains("let foo: i32").count() >= 1);
    // One copy button per part.
    assert_eq!(h.query_all_by_label("Copy").count(), 2);
}

#[test]
fn hover_copy_button_copies_the_part() {
    let mut h = harness(St::new("let foo = 1;\n"));
    focus(&mut h);
    let p = point_of(&mut h, 5);
    h.event(Event::PointerMoved(p));
    wait(&mut h, 0.5);
    let button = h.query_all_by_label("Copy").next().unwrap();
    // Through AccessKit (see docs/handoff/p1-hover.md: pointer clicks on hover buttons).
    button.click_accesskit();
    // The copy is in the output of the frame that saw the click.
    let mut copied = false;
    for _ in 0..4 {
        h.step();
        copied |=
            h.output().platform_output.commands.iter().any(
                |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "unused variable `foo`"),
            );
    }
    h.run();
    assert!(copied);
    assert!(hover_label(&h).is_some(), "clicking on the hover keeps it");
}

#[test]
fn hover_link_emits_hover_link_activated_and_escape_closes() {
    let mut h = harness(St::new("let foo = 1;\n"));
    focus(&mut h);
    let p = point_of(&mut h, 5);
    h.event(Event::PointerMoved(p));
    wait(&mut h, 0.5);
    h.get_by_label("docs").click_accesskit();
    h.run();
    assert_eq!(h.state().links, ["https://example.com/foo"]);
    h.key_press(Key::Escape);
    h.run();
    assert!(hover_label(&h).is_none());
}

#[test]
fn show_hover_command_opens_then_focuses_and_escape_closes() {
    let mut h = harness(St::new("let foo = 1;\n"));
    focus(&mut h);
    {
        let st = h.state_mut();
        st.view.set_selection(&st.doc, Selection::point(5));
    }
    h.run();
    // Ctrl+K Ctrl+I (Cmd on macOS): shows at once at the cursor.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run();
    assert!(hover_label(&h).is_some());
    assert!(!h.state().view.hover().is_focused());
    // Again: focus the hover; arrows scroll it instead of moving the cursor.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run();
    assert!(h.state().view.hover().is_focused());
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(h.state().view.selection().primary().head, 5);
    h.key_press(Key::Escape);
    h.run();
    assert!(hover_label(&h).is_none());
    // Typing hides a shown (unfocused) hover.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run();
    assert!(hover_label(&h).is_some());
    h.key_press(Key::ArrowRight);
    h.run();
    assert!(hover_label(&h).is_none());
}

fn hints_label(h: &Harness<'_, St>) -> Option<String> {
    tooltips(h).into_iter().find(|l| l.starts_with("add("))
}

fn hints_description(h: &Harness<'_, St>) -> Option<String> {
    h.query_all_by_role(Role::Tooltip)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("add("))
        })
        .and_then(|n| n.accesskit_node().description())
}

#[test]
fn trigger_character_opens_parameter_hints_with_active_parameter() {
    let mut h = harness(St::new("add\n"));
    focus(&mut h);
    {
        let st = h.state_mut();
        st.view.set_selection(&st.doc, Selection::point(3));
    }
    h.run();
    type_text(&mut h, "(");
    assert!(hints_label(&h).is_none(), "VS Code waits 120 ms");
    wait(&mut h, 0.2);
    assert_eq!(h.state().sig_requests.len(), 1);
    assert_eq!(
        h.state().sig_requests[0].trigger,
        quire_core::parameter_hints::SignatureHelpTriggerKind::TriggerCharacter("(".into())
    );
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: i32, b: i32) -> i32")
    );
    assert_eq!(
        hints_description(&h).as_deref(),
        Some("a: i32, the first, Adds **two** numbers., hint")
    );
    // The counter and the documentation are drawn.
    assert!(h.query_by_label("1/2").is_some());
    assert!(h.query_by_label("the first").is_some());
}

#[test]
fn up_down_cycle_overloads_and_escape_closes() {
    let mut h = harness(St::new("add\n"));
    focus(&mut h);
    {
        let st = h.state_mut();
        st.view.set_selection(&st.doc, Selection::point(3));
    }
    h.run();
    // Ctrl+Shift+Space (Cmd on macOS).
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Space);
    h.run();
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: i32, b: i32) -> i32")
    );
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: f64, b: f64) -> f64")
    );
    assert!(h.query_by_label("2/2").is_some());
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: i32, b: i32) -> i32"),
        "cycles"
    );
    h.key_press(Key::ArrowUp);
    h.run();
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: f64, b: f64) -> f64")
    );
    // The arrows of the widget do the same.
    h.get_by_label("Previous signature").click_accesskit();
    h.run();
    assert_eq!(
        hints_label(&h).as_deref(),
        Some("add(a: i32, b: i32) -> i32")
    );
    assert_eq!(
        h.state().view.selection().primary().head,
        3,
        "the cursor stays"
    );
    h.key_press(Key::Escape);
    h.run();
    assert!(hints_label(&h).is_none());
    assert!(h.state().view.focused(), "the editor keeps focus");
}

#[test]
fn leaving_the_call_closes_parameter_hints() {
    let mut h = harness(St::new("add\n"));
    focus(&mut h);
    {
        let st = h.state_mut();
        st.view.set_selection(&st.doc, Selection::point(3));
    }
    h.run();
    type_text(&mut h, "(");
    wait(&mut h, 0.2);
    assert!(hints_label(&h).is_some());
    // The provider finds no call at the cursor any more.
    h.state_mut().sig_answer = None;
    // Auto-close inserted ")": typing it types over it and retriggers.
    type_text(&mut h, ")");
    wait(&mut h, 0.2);
    assert!(h.state().sig_requests.len() >= 2);
    assert!(h.state().sig_requests.last().unwrap().is_retrigger);
    assert!(hints_label(&h).is_none());
}
