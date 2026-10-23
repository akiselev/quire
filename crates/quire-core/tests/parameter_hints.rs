//! Parameter hints: VS Code's model-level tests
//! (`src/vs/editor/contrib/parameterHints/test/browser/parameterHintsModel.test.ts` at
//! microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`, translated) and quire's own
//! tests of cycling, closing and the host seam.
//!
//! VS Code's faked timers become explicit host times: typing and commands carry a time, and
//! `EditorView::tick` runs the delayer. The provider is the test: it reads the requests the view
//! emits and answers them.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::hover::AnswerRefusal;
use quire_core::parameter_hints::{
    Documentation, ParameterHintsOutput, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureHelpRequest, SignatureHelpSource, SignatureHelpTriggerKind as K, SignatureInformation,
};
use quire_core::{Document, EditorCommand as C, EditorConfig, EditorView, Selection, SourceId};

struct Ed {
    doc: Document,
    view: EditorView,
}

fn source(triggers: &[&str], retriggers: &[&str]) -> SignatureHelpSource {
    SignatureHelpSource {
        id: SourceId::new("test"),
        trigger_characters: triggers.iter().map(|s| s.to_string()).collect(),
        retrigger_characters: retriggers.iter().map(|s| s.to_string()).collect(),
    }
}

impl Ed {
    /// VS Code `createMockEditor` with a registered provider; `delay` as the model's.
    fn new(text: &str, sources: Vec<SignatureHelpSource>, delay: Option<u64>) -> Self {
        let mut cfg = EditorConfig::default();
        // The tests type brackets; VS Code's test editor has no auto-closing pairs.
        cfg.auto_closing_pairs.clear();
        let mut doc = Document::new(text, cfg);
        doc.set_signature_help_sources(sources);
        let mut view = EditorView::new(&doc);
        if let Some(d) = delay {
            view.set_parameter_hints_delay(d);
        }
        Ed { doc, view }
    }

    /// `editor.trigger('keyboard', Handler.Type, { text })` at host time `t`.
    fn type_at(&mut self, text: &str, t: u64) {
        self.doc
            .execute(&mut self.view, &C::Type { text: text.into() }, Some(t))
            .unwrap();
    }

    fn cmd(&mut self, c: C, t: u64) {
        self.doc.execute(&mut self.view, &c, Some(t)).unwrap();
    }

    fn tick(&mut self, t: u64) {
        self.view.tick(&self.doc, t);
    }

    fn requests(&mut self) -> Vec<SignatureHelpRequest> {
        self.view
            .take_parameter_hints_outputs()
            .into_iter()
            .map(|o| match o {
                ParameterHintsOutput::Request(r) => r,
            })
            .collect()
    }

    fn one_request(&mut self) -> SignatureHelpRequest {
        let mut r = self.requests();
        assert_eq!(r.len(), 1, "{r:?}");
        r.remove(0)
    }

    fn answer(
        &mut self,
        r: &SignatureHelpRequest,
        h: Option<SignatureHelp>,
    ) -> Result<(), AnswerRefusal> {
        self.view.set_signature_help(&self.doc, r.id, r.version, h)
    }
}

/// VS Code's `emptySigHelp`.
fn empty_sig_help() -> SignatureHelp {
    SignatureHelp {
        signatures: vec![SignatureInformation::new("none", vec![])],
        active_signature: 0,
        active_parameter: 0,
    }
}

fn trig(c: &str) -> K {
    K::TriggerCharacter(c.into())
}

#[test]
fn provider_should_get_trigger_character_on_type() {
    let mut e = Ed::new("", vec![source(&["("], &[])], None);
    e.type_at("(", 0);
    e.tick(119);
    assert!(e.requests().is_empty(), "the default delay is 120 ms");
    e.tick(120);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("("));
}

#[test]
fn provider_should_be_retriggered_if_already_active() {
    let mut e = Ed::new("", vec![source(&["("], &[])], None);
    e.type_at("(", 0);
    e.tick(120);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("("));
    assert!(!r.context.is_retrigger);
    assert_eq!(r.context.active_signature_help, None);
    e.answer(&r, Some(empty_sig_help())).unwrap();
    // Retrigger.
    e.type_at("(", 120);
    e.tick(240);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("("));
    assert!(r.context.is_retrigger);
    assert_eq!(r.context.active_signature_help, Some(empty_sig_help()));
}

/// VS Code expects `isRetrigger: true` here only because its provider runs synchronously before
/// the model records the pending request; quire's host answers asynchronously, so a cancel
/// before the answer starts a fresh session (ours, approved).
#[test]
fn provider_not_retriggered_if_previous_help_is_canceled_first() {
    let mut e = Ed::new("", vec![source(&["("], &[])], None);
    e.type_at("(", 0);
    e.tick(120);
    let r = e.one_request();
    assert!(!r.context.is_retrigger);
    // Cancel and retrigger.
    e.view.cancel_parameter_hints();
    e.type_at("(", 130);
    e.tick(250);
    let r2 = e.one_request();
    assert_eq!(r2.context.trigger, trig("("));
    assert!(!r2.context.is_retrigger);
    assert_eq!(r2.context.active_signature_help, None);
    assert_eq!(
        e.answer(&r, Some(empty_sig_help())),
        Err(AnswerRefusal::Superseded)
    );
}

#[test]
fn provider_should_get_last_trigger_character_when_triggered_multiple_times() {
    let mut e = Ed::new("", vec![source(&["a", "b", "c"], &[])], Some(5));
    e.type_at("a", 0);
    e.type_at("b", 0);
    e.type_at("c", 0);
    e.tick(5);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("c"));
    assert!(!r.context.is_retrigger);
    // Give some time to allow for later triggers.
    e.tick(55);
    assert!(e.requests().is_empty(), "invoked once");
}

#[test]
fn provider_should_be_retriggered_if_already_active_after_delay() {
    let mut e = Ed::new("", vec![source(&["a", "b"], &[])], Some(5));
    e.type_at("a", 0);
    e.tick(5);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("a"));
    e.answer(&r, Some(empty_sig_help())).unwrap();
    // Retrigger after a delay for the widget to show up.
    e.type_at("b", 55);
    e.tick(60);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("b"));
    assert!(r.context.is_retrigger);
    e.answer(&r, Some(empty_sig_help())).unwrap();
    e.tick(200);
    assert!(e.requests().is_empty(), "no unexpected invoke");
}

#[test]
fn should_cancel_existing_request_when_new_request_comes_in() {
    let mut e = Ed::new("abc def", vec![source(&[], &[])], None);
    e.cmd(C::TriggerParameterHints, 0);
    let r0 = e.one_request();
    // The provider retriggers on its first request.
    e.cmd(C::TriggerParameterHints, 0);
    let r1 = e.one_request();
    let label = |s: &str| SignatureHelp {
        signatures: vec![SignatureInformation::new(s, vec![])],
        active_signature: 0,
        active_parameter: 0,
    };
    // Request 0 was cancelled: its answer is not taken.
    assert_eq!(
        e.answer(&r0, Some(label("0"))),
        Err(AnswerRefusal::Superseded)
    );
    e.answer(&r1, Some(label("1"))).unwrap();
    assert_eq!(
        e.view.parameter_hints().active().unwrap().signatures[0].label,
        "1"
    );
}

#[test]
fn provider_should_be_retriggered_by_retrigger_character() {
    let mut e = Ed::new("", vec![source(&["a"], &["b"])], Some(5));
    // This should not trigger anything.
    e.type_at("b", 0);
    e.tick(10);
    assert!(e.requests().is_empty());
    // But a trigger character should.
    e.type_at("a", 10);
    e.tick(15);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("a"));
    e.answer(&r, Some(empty_sig_help())).unwrap();
    e.type_at("b", 65);
    e.tick(70);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("b"));
    assert!(r.context.is_retrigger);
}

#[test]
fn should_use_first_result_from_multiple_providers() {
    let mut e = Ed::new("", vec![source(&["a"], &[]), source(&["a"], &[])], Some(5));
    // The host asks its providers in order and answers with the first result
    // (VS Code `provideSignatureHelp`).
    let first = |r: &SignatureHelpRequest| {
        (!r.context.is_retrigger).then(|| SignatureHelp {
            active_parameter: 0,
            active_signature: 0,
            signatures: vec![SignatureInformation::new(
                "firstProvider",
                vec![ParameterInformation::new("parameter")],
            )],
        })
    };
    let second = |r: &SignatureHelpRequest| {
        r.context.is_retrigger.then(|| {
            let prev = r.context.active_signature_help.as_ref();
            SignatureHelp {
                active_parameter: 0,
                active_signature: prev.map_or(0, |p| p.active_signature + 1),
                signatures: vec![SignatureInformation::new(
                    "secondProvider",
                    prev.map_or(vec![], |p| p.signatures[0].parameters.clone()),
                )],
            }
        })
    };
    let provide = |r: &SignatureHelpRequest| first(r).or_else(|| second(r));
    e.type_at("a", 0);
    e.tick(5);
    let r = e.one_request();
    e.answer(&r, provide(&r)).unwrap();
    let h = e.view.parameter_hints().active().unwrap().clone();
    assert_eq!(h.signatures[0].label, "firstProvider");
    assert_eq!(h.active_signature, 0);
    assert_eq!(
        h.signatures[0].parameters[0].label,
        ParameterLabel::Text("parameter".into())
    );
    // Retrigger after a delay for the widget to show up.
    e.type_at("a", 55);
    e.tick(60);
    let r = e.one_request();
    e.answer(&r, provide(&r)).unwrap();
    let h = e.view.parameter_hints().active().unwrap().clone();
    assert_eq!(h.signatures[0].label, "secondProvider");
    assert_eq!(h.active_signature, 1);
    assert_eq!(
        h.signatures[0].parameters[0].label,
        ParameterLabel::Text("parameter".into())
    );
}

#[test]
fn quick_typing_should_use_the_first_trigger_character() {
    let mut e = Ed::new("", vec![source(&["a"], &[])], Some(50));
    e.type_at("a", 0);
    e.type_at("x", 0);
    e.tick(50);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig("a"));
    e.answer(&r, Some(empty_sig_help())).unwrap();
    assert!(e.view.parameter_hints().is_visible());
    e.tick(500);
    assert!(e.requests().is_empty(), "no unexpected invoke");
}

#[test]
fn retrigger_while_pending_resolve_preserves_last_active_signature_96702() {
    let mut e = Ed::new("", vec![source(&["a"], &["b"])], Some(50));
    e.type_at("a", 0);
    e.tick(50);
    let r1 = e.one_request();
    assert_eq!(r1.context.trigger, trig("a"));
    e.answer(&r1, Some(empty_sig_help())).unwrap();
    e.type_at("b", 100);
    e.tick(150);
    let r2 = e.one_request();
    // Trigger again while the provider still works on request 2.
    e.type_at("b", 200);
    e.tick(250);
    let r3 = e.one_request();
    // In a retrigger during a pending resolve, the old active signature is still there.
    assert_eq!(r3.context.active_signature_help, Some(empty_sig_help()));
    assert_eq!(
        e.answer(&r2, Some(empty_sig_help())),
        Err(AnswerRefusal::Superseded)
    );
    e.answer(&r3, Some(empty_sig_help())).unwrap();
}

// ---- quire's own tests ----------------------------------------------------------------------

fn overloads(n: usize) -> SignatureHelp {
    SignatureHelp {
        signatures: (0..n)
            .map(|i| SignatureInformation::new(format!("f{i}(a, b)"), vec![]))
            .collect(),
        active_signature: 0,
        active_parameter: 0,
    }
}

#[test]
fn next_and_previous_cycle_through_overloads() {
    let mut e = Ed::new("f", vec![source(&["("], &[])], None);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    assert_eq!(r.context.trigger, K::Invoke);
    e.answer(&r, Some(overloads(3))).unwrap();
    let active = |e: &Ed| e.view.parameter_hints().active().unwrap().active_signature;
    e.cmd(C::ShowNextParameterHint, 1);
    assert_eq!(active(&e), 1);
    e.cmd(C::ShowNextParameterHint, 2);
    e.cmd(C::ShowNextParameterHint, 3);
    assert_eq!(active(&e), 0, "cycle wraps");
    e.cmd(C::ShowPrevParameterHint, 4);
    assert_eq!(active(&e), 2);
    assert!(e.requests().is_empty(), "cycling asks nothing");
}

#[test]
fn next_without_cycle_closes_at_the_last_overload() {
    let mut e = Ed::new("f", vec![source(&["("], &[])], None);
    let mut cfg = e.doc.config().clone();
    cfg.parameter_hints.cycle = false;
    e.doc.set_config(cfg);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    e.answer(&r, Some(overloads(2))).unwrap();
    e.cmd(C::ShowNextParameterHint, 1);
    assert!(e.view.parameter_hints().is_visible());
    e.cmd(C::ShowNextParameterHint, 2);
    assert!(!e.view.parameter_hints().is_visible());
}

#[test]
fn single_signature_disables_cycling_commands() {
    let mut e = Ed::new("f", vec![source(&["("], &[])], None);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    e.answer(&r, Some(overloads(1))).unwrap();
    e.cmd(C::ShowNextParameterHint, 1);
    assert!(
        e.view.parameter_hints().is_visible(),
        "precondition: multiple signatures"
    );
    e.cmd(C::CloseParameterHints, 2);
    assert!(!e.view.parameter_hints().is_visible());
}

#[test]
fn leaving_the_call_closes_the_hints() {
    let mut e = Ed::new("f", vec![source(&["("], &[")"])], None);
    e.cmd(
        C::CursorMove {
            to: quire_core::Motion::LineEnd,
            select: false,
        },
        0,
    );
    e.type_at("(", 0);
    e.tick(120);
    let r = e.one_request();
    e.answer(&r, Some(overloads(1))).unwrap();
    // Typing a retrigger character asks again; the provider says the cursor left the call.
    e.type_at(")", 200);
    e.tick(320);
    let r = e.one_request();
    assert_eq!(r.context.trigger, trig(")"));
    e.answer(&r, None).unwrap();
    assert!(!e.view.parameter_hints().is_visible());
    assert!(!e.view.parameter_hints().is_triggered());
}

#[test]
fn moving_the_cursor_retriggers_with_content_change() {
    let mut e = Ed::new("f(a, b)", vec![source(&["("], &[])], None);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    e.answer(&r, Some(overloads(1))).unwrap();
    e.cmd(
        C::CursorMove {
            to: quire_core::Motion::Right,
            select: false,
        },
        10,
    );
    e.tick(130);
    let r = e.one_request();
    assert_eq!(r.context.trigger, K::ContentChange);
    assert!(r.context.is_retrigger);
    assert_eq!(r.pos, 1);
}

#[test]
fn a_mouse_selection_and_blur_cancel() {
    let mut e = Ed::new("f(a, b)", vec![source(&["("], &[])], None);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    e.answer(&r, Some(overloads(1))).unwrap();
    // The widget's mouse selection (egui_quire `glue::select`).
    let tr = quire_core::Transaction::select(
        e.doc.version(),
        e.doc.text().len_bytes(),
        Selection::point(3),
    )
    .with_origin(quire_core::Origin::Command("setSelection".into()));
    e.doc.apply(tr, &mut e.view).unwrap();
    assert!(!e.view.parameter_hints().is_visible());
    e.cmd(C::TriggerParameterHints, 20);
    let r = e.one_request();
    e.answer(&r, Some(overloads(1))).unwrap();
    e.view.set_focused(true);
    e.view.set_focused(false);
    assert!(!e.view.parameter_hints().is_visible());
}

#[test]
fn no_provider_no_trigger() {
    let mut e = Ed::new("f", vec![], None);
    e.type_at("(", 0);
    e.cmd(C::TriggerParameterHints, 0);
    e.tick(1000);
    assert!(e.requests().is_empty());
}

#[test]
fn disabled_hints_do_not_trigger_on_type() {
    let mut e = Ed::new("f", vec![source(&["("], &[])], None);
    let mut cfg = e.doc.config().clone();
    cfg.parameter_hints.enabled = false;
    e.doc.set_config(cfg);
    e.type_at("(", 0);
    e.tick(1000);
    assert!(e.requests().is_empty());
    e.cmd(C::TriggerParameterHints, 1000);
    assert_eq!(e.requests().len(), 1, "the command still asks");
}

#[test]
fn stale_answers_are_refused() {
    let mut e = Ed::new("f", vec![source(&["("], &[])], None);
    let mut cfg = e.doc.config().clone();
    cfg.recent_changes_window = 2;
    e.doc.set_config(cfg);
    e.cmd(C::TriggerParameterHints, 0);
    let r = e.one_request();
    assert_eq!(
        e.view
            .set_signature_help(&e.doc, r.id, quire_core::Version(r.version.0 + 5), None),
        Err(AnswerRefusal::Stale)
    );
}

#[test]
fn parameter_label_offsets() {
    let mut s = SignatureInformation::new(
        "foo(a, ab: number, b)",
        vec![
            ParameterInformation::new("a"),
            ParameterInformation::new("b"),
            ParameterInformation::new("ab: number"),
            ParameterInformation::new("zz"),
            ParameterInformation {
                label: ParameterLabel::Offsets(7..9),
                documentation: Some(Documentation::Plain("doc".into())),
            },
        ],
    );
    assert_eq!(
        s.parameter_label_offsets(0),
        4..5,
        "whole word, first occurrence"
    );
    assert_eq!(s.parameter_label_offsets(1), 19..20, "not inside `ab`");
    assert_eq!(s.parameter_label_offsets(2), 7..17);
    assert_eq!(s.parameter_label_offsets(3), 0..0, "not found");
    assert_eq!(s.parameter_label_offsets(4), 7..9);
    assert_eq!(s.parameter_label_offsets(9), 0..0, "no such parameter");
    s.label = "x".into();
    assert_eq!(s.parameter_label_offsets(4), 1..1, "offsets clamped");
}
