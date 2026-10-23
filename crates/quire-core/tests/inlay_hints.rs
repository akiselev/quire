//! Inlay hints through `EditorView` (quire's own tests: VS Code has no test suite for
//! `InlayHintsController`; the behaviour follows `inlayHintsController.ts` and `inlayHints.ts`
//! at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`).

use quire_core::hover::AnswerRefusal;
use quire_core::injected::CursorStops;
use quire_core::inlay_hints::{
    InlayHint, InlayHintEdit, InlayHintLabelPart, InlayHintsEnabled, InlayHintsOutput,
    InlayHintsRequest,
};
use quire_core::{
    Document, EditorCommand as C, EditorConfig, EditorView, SelRange, Selection, SourceId,
};

struct Ed {
    doc: Document,
    view: EditorView,
    now: u64,
}

impl Ed {
    fn new(text: &str) -> Self {
        Self::with(text, EditorConfig::default())
    }

    fn with(text: &str, cfg: EditorConfig) -> Self {
        let mut doc = Document::new(text, cfg);
        doc.set_inlay_hint_sources(vec![SourceId::new("ls")]);
        let view = EditorView::new(&doc);
        Ed {
            doc,
            view,
            now: 1000,
        }
    }

    fn tick(&mut self, ms: u64) {
        self.now += ms;
        self.view.tick(&self.doc, self.now);
    }

    fn requests(&mut self) -> Vec<InlayHintsRequest> {
        self.view
            .take_inlay_hints_outputs()
            .into_iter()
            .map(|InlayHintsOutput::Request(r)| r)
            .collect()
    }

    /// Ticks until a request comes out, and returns it.
    fn request(&mut self) -> InlayHintsRequest {
        for _ in 0..2000 {
            self.tick(1);
            if let Some(r) = self.requests().pop() {
                return r;
            }
        }
        panic!("no request");
    }

    fn answer(&mut self, r: &InlayHintsRequest, hints: Vec<InlayHint>) {
        self.view
            .set_inlay_hints(&self.doc, r.id, r.version, hints)
            .unwrap();
    }

    /// Line `line` with its hints spliced in as `[..]`.
    fn shown(&mut self, line: usize) -> String {
        let text = self.doc.text();
        let lr = text.line_range(line);
        let s = text.slice(lr.clone()).to_string();
        let hints = self.view.line_inlay_hints(&self.doc, line);
        let mut out = String::new();
        let mut at = lr.start;
        for h in hints {
            out.push_str(&s[at - lr.start..h.pos - lr.start]);
            at = h.pos;
            out.push('[');
            for p in &h.pieces {
                out.push_str(&p.text);
            }
            out.push(']');
        }
        out.push_str(&s[at - lr.start..]);
        out
    }

    fn set_cursor(&mut self, pos: usize) {
        self.view
            .set_selection(&self.doc, Selection::new([SelRange::point(pos)], 0));
    }

    fn typ(&mut self, t: &str) {
        self.doc
            .execute(&mut self.view, &C::Type { text: t.into() }, None)
            .unwrap();
    }
}

#[test]
fn nothing_is_asked_without_providers_or_when_off() {
    let mut doc = Document::new("let x = 1;", EditorConfig::default());
    let mut view = EditorView::new(&doc);
    view.tick(&doc, 0);
    view.tick(&doc, 1000);
    assert!(view.take_inlay_hints_outputs().is_empty());
    let mut cfg = EditorConfig::default();
    cfg.inlay_hints.enabled = InlayHintsEnabled::Off;
    doc.set_config(cfg);
    doc.set_inlay_hint_sources(vec![SourceId::new("ls")]);
    view.tick(&doc, 2000);
    view.tick(&doc, 3000);
    assert!(view.take_inlay_hints_outputs().is_empty());
}

#[test]
fn first_request_asks_the_whole_document_and_hints_show() {
    let mut e = Ed::new("let x = 1;\nfoo(1, 2);");
    let r = e.request();
    assert_eq!(r.ranges, vec![0..21]);
    // type hint after `x`, parameter hints before the arguments
    e.answer(
        &r,
        vec![
            InlayHint {
                kind: Some(quire_core::inlay_hints::InlayHintKind::Type),
                ..InlayHint::new(5, ": number")
            },
            InlayHint::new(15, "a:"),
            InlayHint {
                padding_right: true,
                ..InlayHint::new(18, "b:")
            },
        ],
    );
    assert_eq!(e.shown(0), "let x[: number] = 1;");
    assert_eq!(e.shown(1), "foo([a:]1, [b: ]2);");
    let hs = e.view.line_inlay_hints(&e.doc, 1);
    assert_eq!(hs[0].pieces[0].cursor_stops, CursorStops::Right);
    assert_eq!(hs[1].pieces[0].cursor_stops, CursorStops::None);
    assert!(hs[1].pieces[1].padding);
    assert_eq!(hs[1].pieces[1].cursor_stops, CursorStops::Right);
}

#[test]
fn hints_follow_their_word() {
    let mut e = Ed::new("let x = 1;");
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(5, ": number")]);
    // typing at the end of the word grows the anchor: the hint stays after the word
    e.set_cursor(5);
    e.typ("y");
    assert_eq!(e.shown(0), "let xy[: number] = 1;");
    // typing before the line moves it along
    e.set_cursor(0);
    e.typ("  ");
    assert_eq!(e.shown(0), "  let xy[: number] = 1;");
    // deleting the word collapses the anchor: the hint hides
    e.view
        .set_selection(&e.doc, Selection::new([SelRange::new(6, 8)], 0));
    e.typ("z");
    assert_eq!(e.shown(0), "  let z = 1;");
}

#[test]
fn answers_are_refused_when_superseded_or_stale() {
    let mut e = Ed::new("let x = 1;");
    let r = e.request();
    // the text changed: the request in flight is dropped
    e.set_cursor(0);
    e.typ("a");
    e.tick(1);
    assert_eq!(
        e.view.set_inlay_hints(&e.doc, r.id, r.version, vec![]),
        Err(AnswerRefusal::Superseded)
    );
    let r2 = e.request();
    assert!(r2.id > r.id);
    // an answer for an older version is mapped forward
    e.typ("b");
    let mut e2 = Ed::new("let x = 1;");
    let r = e2.request();
    e2.set_cursor(0);
    e2.doc
        .execute(&mut e2.view, &C::Type { text: "  ".into() }, None)
        .unwrap();
    // not ticked yet: the pending request still stands
    e2.view
        .set_inlay_hints(&e2.doc, r.id, r.version, vec![InlayHint::new(5, ": T")])
        .unwrap();
    assert_eq!(e2.shown(0), "  let x[: T] = 1;");
}

#[test]
fn maximum_length_cuts_the_hints_of_a_line() {
    let mut cfg = EditorConfig::default();
    cfg.inlay_hints.maximum_length = 10;
    let mut e = Ed::with("f(a, b, c)", cfg);
    let r = e.request();
    e.answer(
        &r,
        vec![
            InlayHint::new(2, "first:"),
            InlayHint::new(5, "second:"),
            InlayHint::new(8, "third:"),
        ],
    );
    // 6 + 7 = 13 > 10: the second is cut by 3 and gets `…`; the third is dropped
    assert_eq!(e.shown(0), "f([first:]a, [seco…]b, c)");
}

#[test]
fn label_parts_are_pieces() {
    let mut e = Ed::new("let x = 1;");
    let r = e.request();
    e.answer(
        &r,
        vec![InlayHint {
            position: 5,
            label: vec![
                InlayHintLabelPart::text(": "),
                InlayHintLabelPart {
                    location: Some(quire_core::inlay_hints::InlayHintLocation {
                        uri: "lib.d.ts".into(),
                        range: 10..16,
                    }),
                    ..InlayHintLabelPart::text("number")
                },
            ],
            padding_left: true,
            ..Default::default()
        }],
    );
    let hs = e.view.line_inlay_hints(&e.doc, 0);
    let parts: Vec<(String, Option<usize>)> = hs[0]
        .pieces
        .iter()
        .map(|p| (p.text.clone(), p.part))
        .collect();
    assert_eq!(
        parts,
        vec![
            (" ".to_string(), None),
            (": ".to_string(), Some(0)),
            ("number".to_string(), Some(1))
        ]
    );
    let h = e.view.inlay_hints().hint(hs[0].hint).unwrap();
    assert!(h.label[1].location.is_some());
}

#[test]
fn typing_debounces_and_refetches_when_the_pause_ends() {
    let mut e = Ed::new("let x = 1;");
    let r = e.request();
    e.answer(&r, vec![]);
    e.set_cursor(10);
    e.typ(" ");
    // the scheduler waits its delay (37 ms by default, then what the provider took)
    let delay = e.view.inlay_hints().delay();
    e.tick(1);
    assert!(e.requests().is_empty());
    e.tick(delay);
    let r = e.requests();
    assert_eq!(r.len(), 1);
    e.answer(&r[0], vec![]);
    // 800 ms after the change, one more request without delay
    e.tick(400);
    assert!(e.requests().is_empty());
    e.tick(400);
    e.tick(1);
    assert_eq!(e.requests().len(), 1);
}

#[test]
fn the_debounce_follows_the_provider() {
    let mut e = Ed::new("x");
    assert_eq!(e.view.inlay_hints().delay(), 37);
    let r = e.request();
    e.tick(300);
    e.answer(&r, vec![]);
    assert_eq!(e.view.inlay_hints().delay(), 300);
    e.view.refresh_inlay_hints();
    let r = e.request();
    e.tick(2000);
    e.answer(&r, vec![]);
    // average of 300 and 2000, at most 625
    assert_eq!(e.view.inlay_hints().delay(), 625);
}

#[test]
fn hints_before_the_caret_keep_their_length_while_typing() {
    let mut e = Ed::new("foo(1)");
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(4, "value:")]);
    assert_eq!(e.shown(0), "foo([value:]1)");
    e.set_cursor(5);
    e.typ("2");
    let r = e.request();
    // the new answer is longer, and comes before the pause ends: it keeps the old length
    e.answer(&r, vec![InlayHint::new(4, "valueLonger:")]);
    assert_eq!(e.shown(0), "foo([value…]12)");
    // a shorter one is filled up
    e.typ("3");
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(4, "v:")]);
    assert_eq!(e.shown(0), "foo([v:    ]123)");
}

#[test]
fn visible_lines_pick_the_ranges() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut e = Ed::new(&text);
    e.view.set_visible_lines(100..120);
    let r = e.request();
    let t = e.doc.text().clone();
    // one viewport (20 lines) and 30 more on each side
    assert_eq!(r.ranges, vec![t.line_to_byte(50)..t.line_range(169).end]);
    // a scroll asks again
    e.answer(&r, vec![]);
    e.view.set_visible_lines(110..130);
    let r = e.request();
    assert_eq!(r.ranges, vec![t.line_to_byte(60)..t.line_range(179).end]);
}

#[test]
fn an_answer_replaces_the_hints_in_its_ranges_only() {
    let text: String = (0..200).map(|i| format!("v{i} = 1\n")).collect();
    let mut e = Ed::new(&text);
    let t = e.doc.text().clone();
    let r = e.request();
    e.answer(
        &r,
        vec![
            InlayHint::new(t.line_to_byte(0) + 2, ":A"),
            InlayHint::new(t.line_to_byte(150) + 4, ":B"),
        ],
    );
    e.view.set_visible_lines(0..10);
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(t.line_to_byte(0) + 2, ":C")]);
    assert_eq!(e.shown(0), "v0[:C] = 1");
    assert_eq!(e.shown(150), "v150[:B] = 1");
}

#[test]
fn unless_pressed_modes() {
    let mut cfg = EditorConfig::default();
    cfg.inlay_hints.enabled = InlayHintsEnabled::OffUnlessPressed;
    let mut e = Ed::with("let x = 1;", cfg);
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(5, ": T")]);
    assert_eq!(e.shown(0), "let x = 1;");
    e.view.set_inlay_hints_modifier(&e.doc, true);
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(5, ": T")]);
    assert_eq!(e.shown(0), "let x[: T] = 1;");
    e.view.set_inlay_hints_modifier(&e.doc, false);
    assert_eq!(e.shown(0), "let x = 1;");
}

#[test]
fn double_click_applies_the_text_edits() {
    let mut e = Ed::new("let x = 1;");
    let r = e.request();
    e.answer(
        &r,
        vec![InlayHint {
            text_edits: vec![InlayHintEdit {
                range: 5..5,
                text: ": number".into(),
            }],
            ..InlayHint::new(5, ": number")
        }],
    );
    let i = e.view.line_inlay_hints(&e.doc, 0)[0].hint;
    let tr = e.view.inlay_hint_edits(&e.doc, i).unwrap();
    e.doc.apply(tr, &mut e.view).unwrap();
    assert_eq!(e.doc.text().to_string(), "let x: number = 1;");
    // and the hints are asked again at once
    e.tick(1);
    assert_eq!(e.requests().len(), 1);
}

#[test]
fn hints_attach_to_tokens_without_a_word() {
    // `(` at 3 has no word: the hint before `)` attaches to the token around it
    let mut e = Ed::new("f( )");
    let r = e.request();
    e.answer(&r, vec![InlayHint::new(3, "x")]);
    assert_eq!(e.shown(0), "f( [x])");
}
