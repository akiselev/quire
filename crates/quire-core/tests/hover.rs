//! Hover: VS Code's model-level hover tests (`src/vs/editor/contrib/hover/test/browser/
//! contentHover.test.ts` and `hoverUtils.test.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`, translated) and quire's own tests of the delayed
//! computation, diagnostics in the hover and the host seam.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::hover::{
    AnswerRefusal, EventModifiers, Hover, HoverAnchor, HoverEnabled as E, HoverOutput, HoverPart,
    HoverRequest, HoverStartMode, HoverStartSource, LineCol, MultiCursorModifier as M,
    compute_hover_positions, is_position_within, is_trigger_modifier_pressed, should_show_hover,
};
use quire_core::{
    ChangeSet, Diagnostic, Document, EditorCommand as C, EditorConfig, EditorView, Origin,
    Severity, SourceId, Transaction,
};

// ---- contentHover.test.ts -------------------------------------------------------------------

/// VS Code's 1-based (line, column) as quire's 0-based line and byte column.
fn lc(line: usize, col: usize) -> LineCol {
    LineCol::new(line - 1, col - 1)
}

#[test]
fn hover_shows_up_in_the_wrong_place_151235() {
    let (show, secondary) =
        compute_hover_positions(lc(5, 5)..lc(5, 5), 0, &[(lc(4, 1)..lc(5, 6), false)]);
    assert_eq!(show, lc(5, 5));
    assert_eq!(secondary, lc(5, 5));
}

#[test]
fn hover_placement_with_word_wrap_95328() {
    // 'just some text' wrapped at column 6 ("just " | "some " | "text"): the anchor at column 8
    // is on the visual row starting at column 6 (the widget's wrap rows give that column).
    let row_start = lc(1, 6).col;
    let (show, secondary) =
        compute_hover_positions(lc(1, 8)..lc(1, 8), row_start, &[(lc(1, 1)..lc(1, 15), false)]);
    assert_eq!(show, lc(1, 8));
    assert_eq!(secondary, lc(1, 6));
}

// ---- hoverUtils.test.ts ---------------------------------------------------------------------

fn mods(ctrl: bool, alt: bool, meta: bool) -> EventModifiers {
    EventModifiers { ctrl, alt, meta }
}

#[test]
fn should_show_hover_on() {
    assert!(should_show_hover(E::On, M::Alt, mods(false, false, false)));
}

#[test]
fn should_show_hover_off() {
    assert!(!should_show_hover(E::Off, M::Alt, mods(false, false, false)));
}

#[test]
fn should_show_hover_ctrl_with_alt_modifier() {
    assert!(should_show_hover(E::OnKeyboardModifier, M::Alt, mods(true, false, false)));
}

#[test]
fn should_show_hover_no_ctrl_with_alt_modifier() {
    assert!(!should_show_hover(E::OnKeyboardModifier, M::Alt, mods(false, false, false)));
}

#[test]
fn should_show_hover_meta_with_alt_modifier() {
    assert!(should_show_hover(E::OnKeyboardModifier, M::Alt, mods(false, false, true)));
}

#[test]
fn should_show_hover_alt_with_ctrl_modifier() {
    assert!(should_show_hover(E::OnKeyboardModifier, M::Ctrl, mods(false, true, false)));
}

#[test]
fn should_show_hover_no_alt_with_ctrl_modifier() {
    assert!(!should_show_hover(E::OnKeyboardModifier, M::Ctrl, mods(false, false, false)));
}

#[test]
fn should_show_hover_alt_with_meta_modifier() {
    assert!(should_show_hover(E::OnKeyboardModifier, M::Meta, mods(false, true, false)));
}

#[test]
fn should_show_hover_ignores_alt_with_alt_modifier() {
    assert!(!should_show_hover(E::OnKeyboardModifier, M::Alt, mods(false, true, false)));
}

#[test]
fn should_show_hover_ignores_ctrl_with_ctrl_modifier() {
    assert!(!should_show_hover(E::OnKeyboardModifier, M::Ctrl, mods(true, false, false)));
}

/// The element of `isMousePositionWithinElement`'s tests: (left, top, width, height).
fn within(el: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    is_position_within(el.0, el.1, el.2, el.3, x, y)
}

#[test]
fn within_inside_bounds() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(within(el, 150.0, 150.0));
    assert!(within(el, 200.0, 150.0));
    assert!(within(el, 250.0, 180.0));
}

#[test]
fn within_on_edges() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(!within(el, 100.0, 100.0));
    assert!(!within(el, 300.0, 100.0));
    assert!(!within(el, 100.0, 200.0));
    assert!(!within(el, 300.0, 200.0));
}

#[test]
fn within_left_of_element() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(!within(el, 99.0, 150.0));
    assert!(!within(el, 50.0, 150.0));
}

#[test]
fn within_right_of_element() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(!within(el, 301.0, 150.0));
    assert!(!within(el, 400.0, 150.0));
}

#[test]
fn within_above_element() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(!within(el, 200.0, 99.0));
    assert!(!within(el, 200.0, 50.0));
}

#[test]
fn within_below_element() {
    let el = (100.0, 100.0, 200.0, 100.0);
    assert!(!within(el, 200.0, 201.0));
    assert!(!within(el, 200.0, 300.0));
}

#[test]
fn within_element_at_origin() {
    let el = (0.0, 0.0, 100.0, 100.0);
    assert!(!within(el, 0.0, 0.0));
    assert!(within(el, 50.0, 50.0));
    assert!(!within(el, 100.0, 100.0));
    assert!(!within(el, 101.0, 101.0));
}

#[test]
fn within_small_element() {
    let el = (100.0, 100.0, 1.0, 1.0);
    assert!(!within(el, 100.0, 100.0));
    assert!(!within(el, 101.0, 101.0));
    assert!(!within(el, 102.0, 102.0));
}

#[test]
fn trigger_ctrl_with_alt_modifier() {
    assert!(is_trigger_modifier_pressed(M::Alt, mods(true, false, false)));
}

#[test]
fn trigger_meta_with_alt_modifier() {
    assert!(is_trigger_modifier_pressed(M::Alt, mods(false, false, true)));
}

#[test]
fn trigger_ctrl_and_meta_with_alt_modifier() {
    assert!(is_trigger_modifier_pressed(M::Alt, mods(true, false, true)));
}

#[test]
fn trigger_none_with_alt_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Alt, mods(false, false, false)));
}

#[test]
fn trigger_alt_with_alt_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Alt, mods(false, true, false)));
}

#[test]
fn trigger_alt_with_ctrl_modifier() {
    assert!(is_trigger_modifier_pressed(M::Ctrl, mods(false, true, false)));
}

#[test]
fn trigger_none_with_ctrl_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Ctrl, mods(false, false, false)));
}

#[test]
fn trigger_ctrl_with_ctrl_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Ctrl, mods(true, false, false)));
}

#[test]
fn trigger_alt_with_meta_modifier() {
    assert!(is_trigger_modifier_pressed(M::Meta, mods(false, true, false)));
}

#[test]
fn trigger_none_with_meta_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Meta, mods(false, false, false)));
}

#[test]
fn trigger_meta_with_meta_modifier() {
    assert!(!is_trigger_modifier_pressed(M::Meta, mods(false, false, true)));
}

// ---- quire's own tests: HoverOperation timing, participants, the host seam ------------------

struct Ed {
    doc: Document,
    view: EditorView,
}

impl Ed {
    fn new(text: &str, providers: bool) -> Self {
        let mut doc = Document::new(text, EditorConfig::default());
        if providers {
            doc.set_hover_sources(vec![SourceId::new("lsp")]);
        }
        let view = EditorView::new(&doc);
        Ed { doc, view }
    }

    fn mouse(&mut self, pos: usize, t: u64) -> bool {
        self.view.start_hover(
            &self.doc,
            Some(HoverAnchor { range: pos..pos }),
            HoverStartMode::Delayed,
            HoverStartSource::Mouse,
            false,
            false,
            Some(t),
        )
    }

    fn requests(&mut self) -> Vec<HoverRequest> {
        self.view
            .take_hover_outputs()
            .into_iter()
            .map(|o| match o {
                HoverOutput::Request(r) => r,
            })
            .collect()
    }

    fn md(&self) -> Vec<String> {
        self.view
            .hover()
            .shown()
            .map(|r| {
                r.parts
                    .iter()
                    .map(|p| match p {
                        HoverPart::Diagnostic { diagnostic, .. } => format!("diag:{}", diagnostic.message),
                        HoverPart::Markdown { contents, .. } => contents.join("|"),
                        HoverPart::Loading { .. } => "Loading...".into(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn hover(s: &str, range: Option<std::ops::Range<usize>>) -> Hover {
    Hover {
        source: SourceId::new("lsp"),
        contents: vec![s.to_string()],
        range,
    }
}

#[test]
fn delayed_hover_asks_at_half_delay_and_shows_at_full_delay() {
    let mut e = Ed::new("let foo = 1;", true);
    assert!(e.mouse(5, 1000));
    e.view.tick(&e.doc, 1149);
    assert!(e.requests().is_empty());
    e.view.tick(&e.doc, 1150);
    let r = e.requests();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].pos, 5);
    // An early answer is held until the hover time.
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("**foo**: i32", Some(4..7))])
        .unwrap();
    assert!(!e.view.hover().is_visible());
    e.view.tick(&e.doc, 1300);
    assert_eq!(e.md(), vec!["**foo**: i32"]);
    assert!(e.view.hover().shown().unwrap().complete);
}

#[test]
fn loading_shows_after_three_delays() {
    let mut e = Ed::new("let foo = 1;", true);
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r = e.requests();
    e.view.tick(&e.doc, 300);
    assert!(!e.view.hover().is_visible(), "nothing to show yet");
    e.view.tick(&e.doc, 900);
    assert_eq!(e.md(), vec!["Loading..."]);
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("doc", None)])
        .unwrap();
    assert_eq!(e.md(), vec!["doc"]);
    assert_eq!(e.view.next_deadline(), None);
}

#[test]
fn diagnostics_come_first_in_the_same_hover() {
    let mut e = Ed::new("let foo = 1;", true);
    let mut d = Diagnostic::new(4..7, Severity::Error, "unused variable");
    d.code = Some("unused".into());
    e.doc
        .set_diagnostics(SourceId::new("check"), e.doc.version(), vec![d])
        .unwrap();
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r = e.requests();
    e.view.tick(&e.doc, 300);
    // Diagnostics show at the hover time; the hover is not complete yet.
    assert_eq!(e.md(), vec!["diag:unused variable"]);
    assert!(!e.view.hover().shown().unwrap().complete);
    e.view
        .set_hover(
            &e.doc,
            r[0].id,
            r[0].version,
            vec![hover("first", None), hover("  ", None), hover("second", None)],
        )
        .unwrap();
    assert_eq!(e.md(), vec!["diag:unused variable", "first", "second"]);
}

#[test]
fn without_providers_only_diagnostics_show() {
    let mut e = Ed::new("let foo = 1;", false);
    e.doc
        .set_diagnostics(
            SourceId::new("check"),
            e.doc.version(),
            vec![Diagnostic::new(4..7, Severity::Warning, "w")],
        )
        .unwrap();
    e.mouse(10, 0);
    e.view.tick(&e.doc, 300);
    assert!(e.requests().is_empty());
    assert!(!e.view.hover().is_visible(), "no diagnostic at 10");
    e.mouse(6, 400);
    e.view.tick(&e.doc, 700);
    assert_eq!(e.md(), vec!["diag:w"]);
    assert!(e.view.hover().shown().unwrap().complete);
}

#[test]
fn same_anchor_does_not_restart_and_moving_on_the_line_filters() {
    let mut e = Ed::new("let foo = bar;", true);
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r = e.requests();
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("foo", Some(4..7))])
        .unwrap();
    e.view.tick(&e.doc, 300);
    assert_eq!(e.md(), vec!["foo"]);
    e.mouse(5, 310);
    e.view.tick(&e.doc, 500);
    assert!(e.requests().is_empty(), "same anchor");
    // Within the part's range: kept while the new computation runs.
    e.mouse(6, 600);
    assert_eq!(e.md(), vec!["foo"]);
    // Outside it: filtered out, so the hover hides until the new answer.
    e.mouse(11, 610);
    assert!(!e.view.hover().is_visible());
}

#[test]
fn superseded_and_stale_answers_are_refused() {
    let mut e = Ed::new("let foo = 1;", true);
    let mut cfg = e.doc.config().clone();
    cfg.recent_changes_window = 1;
    e.doc.set_config(cfg);
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r1 = e.requests();
    e.mouse(9, 160);
    e.view.tick(&e.doc, 310);
    let r2 = e.requests();
    assert_eq!(
        e.view.set_hover(&e.doc, r1[0].id, r1[0].version, vec![hover("x", None)]),
        Err(AnswerRefusal::Superseded)
    );
    for i in 0..3 {
        let tr = Transaction::new(
            e.doc.version(),
            ChangeSet::insert_at(e.doc.text().len_bytes(), 0, "x").unwrap(),
        )
        .with_origin(Origin::Command(format!("t{i}").into()));
        e.doc.apply(tr, &mut e.view).unwrap();
    }
    assert_eq!(
        e.view.set_hover(&e.doc, r2[0].id, r2[0].version, vec![hover("x", None)]),
        Err(AnswerRefusal::Stale)
    );
}

#[test]
fn answers_map_forward_through_edits() {
    let mut e = Ed::new("let foo = 1;", true);
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r = e.requests();
    let tr = Transaction::new(e.doc.version(), ChangeSet::insert_at(e.doc.text().len_bytes(), 0, "//").unwrap());
    e.doc.apply(tr, &mut e.view).unwrap();
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("foo", Some(4..7))])
        .unwrap();
    e.view.tick(&e.doc, 300);
    let shown = e.view.hover().shown().unwrap();
    assert_eq!(shown.parts[0].range(), &(6..9));
    assert_eq!(shown.anchor.range, 7..7);
}

#[test]
fn show_hover_command_is_immediate_and_focuses_when_shown() {
    let mut e = Ed::new("let foo = 1;", true);
    e.doc
        .execute(
            &mut e.view,
            &C::SetSelection { selection: quire_core::Selection::point(5) },
            Some(0),
        )
        .unwrap();
    e.doc.execute(&mut e.view, &C::ShowHover, Some(10)).unwrap();
    let r = e.requests();
    assert_eq!(r.len(), 1, "asked at once");
    assert_eq!(r[0].pos, 5);
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("foo", None)])
        .unwrap();
    assert_eq!(e.md(), vec!["foo"]);
    assert_eq!(e.view.hover().shown().unwrap().source, HoverStartSource::Keyboard);
    assert!(!e.view.hover().is_focused());
    e.doc.execute(&mut e.view, &C::ScrollDownHover, Some(20)).unwrap();
    assert_eq!(e.view.take_hover_scroll(), None, "precondition: hover focused");
    e.doc.execute(&mut e.view, &C::ShowHover, Some(30)).unwrap();
    assert!(e.view.hover().is_focused());
    e.doc.execute(&mut e.view, &C::ScrollDownHover, Some(40)).unwrap();
    assert_eq!(
        e.view.take_hover_scroll(),
        Some(quire_core::hover::HoverScroll::Down)
    );
    e.doc.execute(&mut e.view, &C::HideHover, Some(50)).unwrap();
    assert!(!e.view.hover().is_visible());
    assert!(!e.view.hover().is_focused());
}

#[test]
fn empty_answers_hide_and_blur_hides() {
    let mut e = Ed::new("let foo = 1;", true);
    e.mouse(5, 0);
    e.view.tick(&e.doc, 150);
    let r = e.requests();
    e.view.set_hover(&e.doc, r[0].id, r[0].version, vec![]).unwrap();
    e.view.tick(&e.doc, 300);
    assert!(!e.view.hover().is_visible());
    e.mouse(9, 400);
    e.view.tick(&e.doc, 550);
    let r = e.requests();
    e.view
        .set_hover(&e.doc, r[0].id, r[0].version, vec![hover("one", None)])
        .unwrap();
    e.view.tick(&e.doc, 700);
    assert!(e.view.hover().is_visible());
    e.view.set_focused(true);
    e.view.set_focused(false);
    assert!(!e.view.hover().is_visible());
}
