//! egui_kittest suite for inlay hints as injected text: the request event and a synchronous
//! host, carets and clicks around the injected text, AccessKit text without the hints,
//! Ctrl+click on a label part and a double click applying the hint's edits.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Vec2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_quire::{CodeEditor, EditorEvent};
use quire_core::inlay_hints::{InlayHint, InlayHintEdit, InlayHintLabelPart, InlayHintLocation};
use quire_core::{Document, EditorView, Selection, SourceId};

struct St {
    doc: Document,
    view: EditorView,
    /// What the provider answers.
    hints: Vec<InlayHint>,
    requests: usize,
    events: Vec<EditorEvent>,
    caret: Option<egui::Rect>,
    rect: egui::Rect,
}

impl St {
    fn new(text: &str, hints: Vec<InlayHint>) -> Self {
        let mut doc = Document::new(text, Default::default());
        doc.set_inlay_hint_sources(vec![SourceId::new("lsp")]);
        let view = EditorView::new(&doc);
        St {
            doc,
            view,
            hints,
            requests: 0,
            events: Vec::new(),
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
    for e in out.events {
        match e {
            EditorEvent::InlayHintsRequested { id, version, .. } => {
                st.requests += 1;
                let hints = st.hints.clone();
                st.view
                    .set_inlay_hints(&st.doc, id, version, hints)
                    .unwrap();
            }
            e => st.events.push(e),
        }
    }
}

fn harness(st: St) -> Harness<'static, St> {
    let mut h = Harness::builder()
        .with_size(Vec2::new(800.0, 300.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st);
    // the first request goes out at once
    for _ in 0..6 {
        h.step();
    }
    h.run();
    h
}

fn press(h: &mut Harness<'_, St>, p: egui::Pos2, modifiers: Modifiers) {
    h.event(Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    h.run();
}

/// The caret's rect with the caret at byte `pos`.
fn caret_at(h: &mut Harness<'_, St>, pos: usize) -> egui::Rect {
    let st = h.state_mut();
    st.view.set_selection(&st.doc, Selection::point(pos));
    h.run();
    h.state().caret.unwrap()
}

fn focus(h: &mut Harness<'_, St>) {
    let r = h.state().rect;
    press(
        h,
        pos2(r.right() - 20.0, r.bottom() - 20.0),
        Modifiers::NONE,
    );
}

fn type_hint() -> InlayHint {
    InlayHint::new(5, ": number")
}

#[test]
fn hints_are_asked_and_shift_the_text_after_them() {
    // `let x = 1;` with `: number` after `x` (byte 5)
    let mut h = harness(St::new("let x = 1;", vec![type_hint()]));
    assert_eq!(h.state().requests, 1);
    focus(&mut h);
    let c3 = caret_at(&mut h, 3).left();
    let c4 = caret_at(&mut h, 4).left();
    let adv = c4 - c3;
    assert!(adv > 1.0);
    // at the hint: before it (VS Code's PositionAffinity.None)
    let c5 = caret_at(&mut h, 5).left();
    assert!((c5 - c4 - adv).abs() < 0.5, "{c4} {c5} {adv}");
    // after it: shifted by the 8 characters of the label
    let c6 = caret_at(&mut h, 6).left();
    assert!((c6 - c5 - 9.0 * adv).abs() < 0.5, "{c5} {c6} {adv}");
}

#[test]
fn clicks_on_and_after_the_hint() {
    let mut h = harness(St::new("let x = 1;", vec![type_hint()]));
    focus(&mut h);
    let c5 = caret_at(&mut h, 5);
    let c6 = caret_at(&mut h, 6);
    // on the hint's text: the hint's position
    let mid = pos2((c5.left() + c6.left()) / 2.0, c5.center().y);
    press(&mut h, mid, Modifiers::NONE);
    assert_eq!(h.state().view.selection().primary().head, 5);
    // just after it: the next character
    press(
        &mut h,
        pos2(c6.left() + 1.0, c6.center().y),
        Modifiers::NONE,
    );
    assert_eq!(h.state().view.selection().primary().head, 6);
}

#[test]
fn accesskit_text_has_no_hints() {
    let h = harness(St::new("let x = 1;", vec![type_hint()]));
    let editor = h.get_by_label("Source");
    let run = editor
        .accesskit_node()
        .children()
        .find(|c| c.role() == Role::TextRun)
        .expect("a text run");
    assert_eq!(run.value().as_deref(), Some("let x = 1;"));
}

#[test]
fn ctrl_click_on_a_part_with_a_location() {
    let location = InlayHintLocation {
        uri: "lib.d.ts".into(),
        range: 10..16,
    };
    let hint = InlayHint {
        position: 5,
        label: vec![
            InlayHintLabelPart::text(": "),
            InlayHintLabelPart {
                location: Some(location.clone()),
                ..InlayHintLabelPart::text("number")
            },
        ],
        ..Default::default()
    };
    let mut h = harness(St::new("let x = 1;", vec![hint]));
    focus(&mut h);
    let c5 = caret_at(&mut h, 5);
    let c6 = caret_at(&mut h, 6);
    // the `number` part: the last third of the hint
    let x = c6.left() - (c6.left() - c5.left()) / 6.0;
    let cmd = Modifiers::COMMAND;
    h.state_mut().events.clear();
    press(&mut h, pos2(x, c5.center().y), cmd);
    assert!(
        h.state()
            .events
            .contains(&EditorEvent::InlayHintLocationActivated { location }),
        "{:?}",
        h.state().events
    );
    assert!(
        !h.state()
            .events
            .iter()
            .any(|e| matches!(e, EditorEvent::LinkActivated { .. })),
    );
}

#[test]
fn double_click_applies_the_hints_edits() {
    let hint = InlayHint {
        text_edits: vec![InlayHintEdit {
            range: 5..5,
            text: ": number".into(),
        }],
        ..type_hint()
    };
    let mut h = harness(St::new("let x = 1;", vec![hint]));
    focus(&mut h);
    let c5 = caret_at(&mut h, 5);
    let c6 = caret_at(&mut h, 6);
    let mid = pos2((c5.left() + c6.left()) / 2.0, c5.center().y);
    h.event(Event::PointerMoved(mid));
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: mid,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
    }
    h.run();
    assert_eq!(h.state().doc.text().to_string(), "let x: number = 1;");
}

#[test]
fn the_mouse_over_a_hint_shows_its_tooltip() {
    let hint = InlayHint {
        tooltip: Some(quire_core::parameter_hints::Documentation::Plain(
            "the type of x".into(),
        )),
        ..type_hint()
    };
    let mut h = harness(St::new("let x = 1;", vec![hint]));
    focus(&mut h);
    let c5 = caret_at(&mut h, 5);
    let c6 = caret_at(&mut h, 6);
    let mid = pos2((c5.left() + c6.left()) / 2.0, c5.center().y);
    h.event(Event::PointerMoved(mid));
    for _ in 0..60 {
        h.step();
    }
    h.run();
    let found = h
        .query_all_by_role(Role::Tooltip)
        .any(|n| format!("{:?}", n.accesskit_node()).contains("the type of x"))
        || h.query_by_label_contains("the type of x").is_some();
    assert!(found, "tooltip shown");
}

#[test]
fn wrapped_lines_with_long_hints() {
    let text = "let value = compute(first_argument, second_argument, third_argument);";
    let hints = vec![
        InlayHint::new(9, ": SomeVeryLongTypeName<WithGenerics>"),
        InlayHint::new(20, "first:"),
        InlayHint::new(36, "second:"),
        InlayHint::new(53, "third:"),
    ];
    let mut st = St::new(text, hints);
    let mut cfg = st.doc.config().clone();
    cfg.word_wrap = quire_core::WordWrap::On;
    st.doc.set_config(cfg);
    let mut h = Harness::builder()
        .with_size(Vec2::new(260.0, 300.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st);
    for _ in 0..6 {
        h.step();
    }
    h.run();
    focus(&mut h);
    // every caret position lays out, in order down the rows
    let mut last = (f32::MIN, f32::MIN);
    for pos in 0..=text.len() {
        let c = caret_at(&mut h, pos);
        let at = (c.top(), c.left());
        assert!(at.0 >= last.0, "{pos}: {at:?} after {last:?}");
        last = at;
    }
    assert!(last.0 > 30.0, "the line wraps");
}
