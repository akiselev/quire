//! egui_kittest suite for the `CodeEditor` widget (QE1).

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_quire::{CodeEditor, EditorEvent, EditorOutput, KeyHandling};
use quire_core::{Document, EditorView};

/// Test state: one document, two views, what the editors and the host saw.
struct St {
    doc: Document,
    views: Vec<EditorView>,
    /// How many views to show (side by side).
    shown: usize,
    host_mode: bool,
    /// Per frame, per view: events.
    events: Vec<EditorEvent>,
    applied: usize,
    /// Keys still pressed in egui input after the editors ran.
    left_keys: Vec<Key>,
    left_text: Vec<String>,
    rects: Vec<Rect>,
    caret: Option<Rect>,
    settled: bool,
}

impl St {
    fn new(text: &str) -> Self {
        let doc = Document::new(text, Default::default());
        let views = vec![EditorView::new(&doc), EditorView::new(&doc)];
        Self {
            doc,
            views,
            shown: 1,
            host_mode: false,
            events: Vec::new(),
            applied: 0,
            left_keys: Vec::new(),
            left_text: Vec::new(),
            rects: Vec::new(),
            caret: None,
            settled: true,
        }
    }

    fn text(&self) -> String {
        self.doc.text().to_string()
    }

    fn sel(&self, view: usize) -> (usize, usize) {
        let r = self.views[view].selection().primary();
        (r.anchor, r.head)
    }
}

fn app(ui: &mut egui::Ui, st: &mut St) {
    st.rects.clear();
    let full = ui.available_rect_before_wrap();
    let w = full.width() / st.shown as f32;
    for i in 0..st.shown {
        let rect = Rect::from_min_size(
            pos2(full.left() + w * i as f32, full.top()),
            vec2(w, full.height()),
        );
        let name = if i == 0 { "Source" } else { "Source 2" };
        let mut ed = CodeEditor::new(&mut st.doc, &mut st.views[i], ("ed", i), name);
        if st.host_mode {
            ed = ed.key_handling(KeyHandling::Host);
        }
        let out: EditorOutput = ed.show_in(ui, rect);
        st.applied += out.applied.len();
        st.events.extend(out.events);
        st.rects.push(rect);
        if i == 0 {
            st.caret = out.caret_rect;
            st.settled = out.settled;
        }
    }
    // What the host would still see (accumulated; tests clear it).
    ui.input(|inp| {
        for e in &inp.events {
            match e {
                Event::Key {
                    key, pressed: true, ..
                } => st.left_keys.push(*key),
                Event::Text(t) => st.left_text.push(t.clone()),
                _ => {}
            }
        }
    });
}

fn harness(st: St) -> Harness<'static, St> {
    Harness::builder()
        .with_size(Vec2::new(800.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(app, st)
}

/// Click at a point (press + release) and run.
fn click_at(h: &mut Harness<'_, St>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// Text geometry of editor 0 measured through the caret: (text left x, char advance, row rect of
/// line 0). Clicks the end of line 0 and of line 1, so line 1 must be shorter than line 0 and
/// both lines ASCII. Leaves the caret at the end of line 1.
fn measure(h: &mut Harness<'_, St>, len0: usize, len1: usize) -> (f32, f32, Rect) {
    let r = h.state().rects[0];
    click_at(h, pos2(r.right() - 30.0, r.top() + 8.0));
    let c0 = h.state().caret.unwrap();
    click_at(h, pos2(r.right() - 30.0, c0.bottom() + c0.height() * 0.5));
    let c1 = h.state().caret.unwrap();
    let adv = (c0.left() - c1.left()) / (len0 - len1) as f32;
    let left = c1.left() - adv * len1 as f32;
    (
        left,
        adv,
        Rect::from_min_max(pos2(left, c0.top()), pos2(c0.left(), c0.bottom())),
    )
}

/// Focus the first editor by clicking into its text area.
fn focus(h: &mut Harness<'_, St>) {
    let r = h.state().rects[0];
    click_at(h, pos2(r.right() - 20.0, r.bottom() - 20.0));
}

fn cmd() -> Modifiers {
    Modifiers::COMMAND
}

#[test]
fn a11y_node_has_name_role_and_first_page() {
    let text: String = (0..120).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(St::new(&text));
    h.run();
    let node = h.get_by_label("Source");
    assert_eq!(node.accesskit_node().role(), Role::MultilineTextInput);
    let runs: Vec<_> = node
        .accesskit_node()
        .children()
        .filter(|c| c.role() == Role::TextRun)
        .collect();
    assert_eq!(
        runs.len(),
        50,
        "first page is 50 lines without a screen reader"
    );
    assert_eq!(runs[0].value().as_deref(), Some("line 0\n"));
}

#[test]
fn typing_appears_the_same_frame() {
    let mut h = harness(St::new(""));
    h.run();
    focus(&mut h);
    h.event(Event::Text("hi".into()));
    h.step();
    assert_eq!(h.state().text(), "hi");
    // The accessibility tree of that same frame already shows it.
    let node = h.get_by_label("Source");
    let run = node
        .accesskit_node()
        .children()
        .find(|c| c.role() == Role::TextRun)
        .unwrap();
    assert_eq!(run.value().as_deref(), Some("hi"));
    assert!(h.state().left_text.is_empty(), "text event consumed");
}

#[test]
fn handled_keys_are_consumed_unhandled_left() {
    let mut h = harness(St::new("abc"));
    h.run();
    focus(&mut h);
    h.state_mut().left_keys.clear();
    h.key_press(Key::ArrowLeft);
    h.step();
    assert!(!h.state().left_keys.contains(&Key::ArrowLeft));
    h.key_press(Key::F12);
    h.step();
    assert!(h.state().left_keys.contains(&Key::F12));
}

#[test]
fn backspace_and_enter() {
    let mut h = harness(St::new("ab"));
    h.run();
    focus(&mut h);
    h.key_press(Key::Backspace);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text(), "a\n");
}

#[test]
fn clipboard_copy_cut_paste() {
    let mut h = harness(St::new("hello world"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::A);
    h.run();
    assert_eq!(h.state().sel(0), (0, 11));
    h.event(Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "hello world"));
    assert!(copied, "copy puts the selection on the clipboard");
    h.event(Event::Cut);
    h.step();
    assert_eq!(h.state().text(), "");
    h.event(Event::Paste("pasted".into()));
    h.step();
    assert_eq!(h.state().text(), "pasted");
}

#[test]
fn ime_preedit_then_commit() {
    let mut h = harness(St::new("x"));
    h.run();
    focus(&mut h);
    h.event(Event::Ime(egui::ImeEvent::Preedit {
        text: "ni".into(),
        active_range_chars: Some(2..2),
    }));
    h.step();
    assert_eq!(h.state().text(), "x", "preedit changes no buffer");
    assert!(h.state().views[0].composing());
    let v = h.state().doc.version();
    h.event(Event::Ime(egui::ImeEvent::Commit("ü".into())));
    h.step();
    assert_eq!(h.state().text(), "xü");
    assert!(!h.state().views[0].composing());
    assert_eq!(
        h.state().doc.version(),
        quire_core::Version(v.0 + 1),
        "commit is one transaction"
    );
    // IME output is written while focused.
    assert!(h.output().platform_output.ime.is_some());
}

#[test]
fn double_click_selects_word_triple_selects_line() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 16, 6);
    assert_eq!(h.state().sel(0), (23, 23), "click places the caret");
    let p = pos2(left + adv * 7.5, row0.center().y);
    for _ in 0..2 {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
    assert_eq!(h.state().sel(0), (6, 10), "double click selects the word");
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(
        h.state().sel(0),
        (0, 17),
        "triple click selects the line with its break"
    );
}

#[test]
fn drag_selects_and_shift_click_extends() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 16, 6);
    let y = row0.center().y;
    let a = pos2(left + adv * 1.0, y);
    let b = pos2(left + adv * 8.0, y);
    h.event(Event::PointerMoved(a));
    h.event(Event::PointerButton {
        pos: a,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerMoved(pos2(left + adv * 4.0, y)));
    h.event(Event::PointerMoved(b));
    h.event(Event::PointerButton {
        pos: b,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().sel(0), (1, 8));
    let c = pos2(left + adv * 12.0, y);
    h.event(Event::PointerMoved(c));
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::SHIFT,
    });
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::SHIFT,
    });
    h.run();
    assert_eq!(
        h.state().sel(0),
        (1, 12),
        "shift+click extends from the anchor"
    );
}

#[test]
fn ctrl_click_activates_link() {
    let mut h = harness(St::new("foo bar"));
    h.run();
    focus(&mut h);
    // The caret is at the end (col 7), on "bar".
    let caret = h.state().caret.unwrap();
    let before = h.state().sel(0);
    let p = pos2(caret.left() - 2.0, caret.center().y);
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: cmd(),
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: cmd(),
    });
    h.run();
    assert!(
        h.state()
            .events
            .iter()
            .any(|e| matches!(e, EditorEvent::LinkActivated { range, .. } if *range == (4..7)))
    );
    assert_eq!(
        h.state().sel(0),
        before,
        "ctrl+click does not move the caret"
    );
}

#[test]
fn find_bar_finds_and_moves_selection() {
    let mut h = harness(St::new("one two one two one"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::F);
    h.run();
    h.get_by_label("Find").type_text("two");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let (a, b) = h.state().sel(0);
    assert_eq!((a.min(b), a.max(b)), (4, 7));
    h.key_press(Key::Enter);
    h.run();
    let (a, b) = h.state().sel(0);
    assert_eq!((a.min(b), a.max(b)), (12, 15));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Find").is_none(), "escape closes the bar");
}

#[test]
fn save_emits_save_requested() {
    let mut h = harness(St::new("x"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::S);
    h.run();
    assert!(h.state().events.contains(&EditorEvent::SaveRequested));
}

#[test]
fn host_mode_types_but_leaves_keys() {
    let mut st = St::new("abc");
    st.host_mode = true;
    let mut h = harness(st);
    h.run();
    focus(&mut h);
    h.state_mut().left_keys.clear();
    h.key_press(Key::ArrowLeft);
    h.step();
    assert!(
        h.state().left_keys.contains(&Key::ArrowLeft),
        "host mode does not consume keys"
    );
    h.key_press_modifiers(cmd(), Key::S);
    h.step();
    assert!(!h.state().events.contains(&EditorEvent::SaveRequested));
    h.event(Event::Text("z".into()));
    h.step();
    assert!(
        h.state().text().contains('z'),
        "text input is still applied"
    );
}

#[test]
fn two_views_keep_separate_carets() {
    let mut st = St::new("0123456789");
    st.shown = 2;
    let mut h = harness(st);
    h.run();
    // Focus view 0, go to end; view 1 stays at 0.
    focus(&mut h);
    h.key_press(Key::End);
    h.run();
    assert_eq!(h.state().sel(0).1, 10);
    assert_eq!(h.state().sel(1).1, 0);
    // Type in view 0 at the end; view 1's caret stays at 0, view 0 moves on.
    h.event(Event::Text("!".into()));
    h.run();
    assert_eq!(h.state().sel(0).1, 11);
    assert_eq!(h.state().sel(1).1, 0);
    // Focus view 1, type: it inserts at its own caret.
    let r1 = h.state().rects[1];
    click_at(&mut h, pos2(r1.left() + 60.0, r1.top() + 6.0));
    h.key_press_modifiers(cmd(), Key::Home);
    h.event(Event::Text(">".into()));
    h.run();
    assert_eq!(h.state().text(), ">0123456789!");
    assert_eq!(
        h.state().sel(0).1,
        12,
        "view 0 maps forward through the other view's edit"
    );
}

#[test]
fn settled_when_idle() {
    let mut h = harness(St::new("x"));
    h.run();
    assert!(h.state().settled);
    assert_eq!(h.state().applied, 0);
}

#[test]
fn copy_puts_selection_on_clipboard() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 16, 6);
    let p = pos2(left + adv * 7.5, row0.center().y);
    for _ in 0..2 {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
    h.event(Event::Copy);
    h.step();
    let copied: Vec<_> = h
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(copied, vec!["beta".to_owned()]);
    assert_eq!(
        h.state().text(),
        "alpha beta gamma\nsecond",
        "copy changes nothing"
    );
}

#[test]
fn ime_preedit_renders_without_changing_the_buffer() {
    let mut h = harness(St::new("x"));
    h.run();
    focus(&mut h);
    let v = h.state().doc.version();
    h.event(Event::Ime(egui::ImeEvent::Preedit {
        text: "ni".into(),
        active_range_chars: Some(2..2),
    }));
    h.step();
    assert_eq!(h.state().text(), "x", "preedit changes no buffer");
    assert_eq!(h.state().doc.version(), v);
    assert!(h.state().views[0].composing());
    assert!(
        h.output().platform_output.ime.is_some(),
        "IME output written while focused"
    );
    // While composing, Escape cancels the preedit only and is consumed.
    h.state_mut().left_keys.clear();
    h.key_press(Key::Escape);
    h.step();
    assert!(!h.state().views[0].composing());
    assert!(!h.state().left_keys.contains(&Key::Escape));
    assert_eq!(h.state().text(), "x");
}

#[test]
fn widget_keys_consumed_unbound_keys_left() {
    let mut h = harness(St::new("one two one"));
    h.run();
    focus(&mut h);
    h.state_mut().left_keys.clear();
    h.key_press(Key::F12);
    h.step();
    assert!(
        h.state().left_keys.contains(&Key::F12),
        "unbound key is left for the host"
    );
    h.state_mut().left_keys.clear();
    h.key_press_modifiers(cmd(), Key::F);
    h.run();
    assert!(
        !h.state().left_keys.contains(&Key::F),
        "find chord consumed"
    );
    assert!(h.query_by_label("Find").is_some(), "find bar open");
}

#[test]
fn find_bar_counts_matches_and_closes() {
    let mut h = harness(St::new("one two one two one"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::F);
    h.run();
    h.get_by_label("Find").type_text("one");
    h.run();
    assert!(
        h.query_by_label_contains("of 3").is_some()
            || h.query_by_label_contains("? of 3").is_some()
    );
    let n = h.state().views[0].find().map(|f| f.matches.len());
    assert_eq!(n, Some(3));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Find").is_none(), "escape closes the bar");
    assert!(h.state().views[0].find().is_none());
}

#[test]
fn diagnostics_paint_and_mark_settled() {
    let mut st = St::new("let x = 1;\nlet y = 2;");
    let v = st.doc.version();
    st.doc
        .set_diagnostics(
            "test".into(),
            v,
            vec![quire_core::Diagnostic::new(
                4..5,
                quire_core::Severity::Error,
                "unused x",
            )],
        )
        .unwrap();
    let mut h = harness(st);
    h.run();
    assert!(h.state().settled);
}

#[test]
fn keymap_resolves_against_core_commands() {
    let map = egui_quire::Keymap::vscode_default();
    let mut unresolved: Vec<&str> = map.unresolved().iter().map(|(id, _)| id.as_str()).collect();
    unresolved.sort_unstable();
    unresolved.dedup();
    // Commands quire-core does not have yet (ED2 / later); not bound until they exist.
    assert_eq!(
        unresolved,
        vec![
            "deleteAllLeft",
            "editor.action.addSelectionToNextFindMatch",
            "editor.action.copyLinesDownAction",
            "editor.action.copyLinesUpAction",
            "editor.action.insertCursorAbove",
            "editor.action.insertCursorBelow",
            "editor.action.moveLinesDownAction",
            "editor.action.moveLinesUpAction",
        ]
    );
    let os = egui::os::OperatingSystem::Nix;
    let ctx = egui_quire::keymap::WhenContext::default();
    let b = map
        .lookup(os, Key::ArrowLeft, Modifiers::CTRL | Modifiers::SHIFT, &ctx)
        .unwrap();
    assert_eq!(b.command_id, "cursorWordLeftSelect");
    let mac = egui::os::OperatingSystem::Mac;
    assert_eq!(
        map.lookup(mac, Key::S, Modifiers::MAC_CMD, &ctx)
            .unwrap()
            .command_id,
        "workbench.action.files.save"
    );
    assert!(map.lookup(mac, Key::S, Modifiers::CTRL, &ctx).is_none());
    // Escape closes find only while the bar is visible.
    assert!(map.lookup(os, Key::Escape, Modifiers::NONE, &ctx).is_none());
    let find = egui_quire::keymap::WhenContext {
        find_widget_visible: true,
        ..Default::default()
    };
    assert_eq!(
        map.lookup(os, Key::Escape, Modifiers::NONE, &find)
            .unwrap()
            .command_id,
        "closeFindWidget"
    );
}

fn root_repaint_delay(h: &Harness<'_, St>) -> std::time::Duration {
    h.output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(std::time::Duration::MAX, |v| v.repaint_delay)
}

#[test]
fn hover_rest_emits_hover_at() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 16, 6);
    h.state_mut().events.clear();
    let p = pos2(left + adv * 2.2, row0.center().y);
    h.event(Event::PointerMoved(p));
    h.run_steps(45);
    let hovers: Vec<_> = h
        .state()
        .events
        .iter()
        .filter(|e| matches!(e, EditorEvent::HoverAt { .. }))
        .collect();
    assert_eq!(
        hovers.len(),
        1,
        "one HoverAt per resting position: {hovers:?}"
    );
    assert!(matches!(hovers[0], EditorEvent::HoverAt { pos: 2, .. }));
}

#[test]
fn secondary_click_emits_context_menu() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 16, 6);
    let p = pos2(left + adv * 6.2, row0.center().y);
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(
        h.state()
            .events
            .contains(&EditorEvent::ContextMenu { pos: 6 })
    );
}

#[test]
fn diagnostic_hover_shows_message_and_action() {
    let mut st = St::new("let x = 1;\nlet y;");
    let v = st.doc.version();
    let mut d =
        quire_core::Diagnostic::new(4..5, quire_core::Severity::Warning, "unused variable x");
    d.actions.push(quire_core::DiagAction {
        title: "Prefix with _".into(),
        command_id: "fix.prefix".into(),
        args_json: r#"{"at":4}"#.into(),
    });
    st.doc.set_diagnostics("test".into(), v, vec![d]).unwrap();
    let mut h = harness(st);
    h.run();
    let (left, adv, row0) = measure(&mut h, 10, 6);
    // Hover the `x` (column 4).
    let p = pos2(left + adv * 4.5, row0.center().y);
    h.event(Event::PointerMoved(p));
    h.run_steps(60);
    let btn = h.get_by_label("Prefix with _");
    assert!(
        h.query_by_label("unused variable x").is_some(),
        "message in the hover"
    );
    btn.click();
    h.run();
    assert!(h.state().events.iter().any(|e| matches!(e,
        EditorEvent::DiagnosticAction { command_id, args } if command_id == "fix.prefix" && args["at"] == 4)));
}

#[test]
fn virtualised_lines_and_wheel_scroll() {
    let text: String = (0..10_000).map(|i| format!("line number {i}\n")).collect();
    let mut h = harness(St::new(&text));
    h.run();
    let id = CodeEditor::id_for(("ed", 0usize));
    let (cached, misses) = CodeEditor::galley_stats(&h.ctx, id).unwrap();
    assert!(
        misses < 120,
        "only visible lines and the a11y page are shaped: {misses}"
    );
    assert!(cached as u64 <= misses);
    let r = h.state().rects[0];
    h.event(Event::PointerMoved(r.center()));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -3000.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    // egui animates smooth scrolling over several frames.
    h.run_steps(40);
    let first = h.state().views[0].scroll().anchor;
    assert!(
        first > 0,
        "wheel scrolls the view (scroll lives in the EditorView)"
    );
    let (_, misses2) = CodeEditor::galley_stats(&h.ctx, id).unwrap();
    assert!(
        misses2 - misses < 400,
        "scrolling shapes only lines it passes: {}",
        misses2 - misses
    );
    // Idle frames reuse the cached galleys.
    h.run_steps(3);
    let (_, misses3) = CodeEditor::galley_stats(&h.ctx, id).unwrap();
    assert_eq!(misses3, misses2, "an idle frame shapes nothing");
    // line[N] anchors follow the scroll.
    let l = h.state().doc.text().byte_to_line(first);
    let a = CodeEditor::anchor_rect(&h.ctx, id, &format!("line[{l}]")).unwrap();
    let b = CodeEditor::anchor_rect(&h.ctx, id, &format!("line[{}]", l + 1)).unwrap();
    assert!((b.top() - a.bottom()).abs() < 0.01);
    assert!(a.top() <= r.top() + 4.0 && a.bottom() > r.top());
}

#[test]
fn host_mode_leaves_command_keys() {
    let mut st = St::new("abc");
    st.host_mode = true;
    let mut h = harness(st);
    h.run();
    focus(&mut h);
    h.state_mut().left_keys.clear();
    h.key_press(Key::ArrowLeft);
    h.key_press_modifiers(cmd(), Key::F);
    h.key_press_modifiers(cmd(), Key::S);
    h.run();
    let left = &h.state().left_keys;
    assert!(
        left.contains(&Key::ArrowLeft) && left.contains(&Key::F) && left.contains(&Key::S),
        "{left:?}"
    );
    assert!(h.query_by_label("Find").is_none());
    assert!(!h.state().events.contains(&EditorEvent::SaveRequested));
}

#[test]
fn caret_blink_stops_after_idle() {
    // kittest turns cursor blinking off; turn it back on for this test.
    let mut h = Harness::builder()
        .with_size(Vec2::new(600.0, 200.0))
        .with_step_dt(0.25)
        .build_ui_state(app, St::new("x"));
    h.ctx.all_styles_mut(|s| s.visuals.text_cursor.blink = true);
    h.run();
    focus(&mut h);
    h.step();
    assert!(h.state().events.contains(&EditorEvent::FocusChanged(true)));
    assert!(
        root_repaint_delay(&h) < std::time::Duration::from_secs(2),
        "blinking while recently active: {:?}",
        root_repaint_delay(&h)
    );
    h.run_steps(30);
    assert_eq!(
        root_repaint_delay(&h),
        std::time::Duration::MAX,
        "no repaint requests once idle"
    );
    assert!(h.state().settled);
}

#[test]
fn mono_fast_path_matches_galley_positions() {
    for ppp in [1.0, 1.25, 1.5, 2.0] {
        mono_fast_path_at(ppp);
    }
}

fn mono_fast_path_at(ppp: f32) {
    let mut checked = false;
    let mut h = Harness::builder()
        .with_pixels_per_point(ppp)
        .build_ui(|ui| {
            let font = egui::FontId::monospace(13.0);
            let mono = egui_quire::layout::mono(ui.ctx(), &font);
            let mut cache = egui_quire::galley_cache::GalleyCache::default();
            let mut sh = egui_quire::layout::LineShaper {
                cache: &mut cache,
                ctx: ui.ctx(),
                font: &font,
                pixels_per_point: ui.ctx().pixels_per_point(),
                mono,
            };
            let line = "fn main() { let x = 42; }";
            let fast = sh.shape(
                line,
                &[(0, egui::Color32::RED), (3, egui::Color32::GREEN)],
                1,
                egui::Color32::WHITE,
            );
            assert!(fast.mono.is_some());
            let slow = egui_quire::layout::LineLayout {
                galley: fast.galley.clone(),
                mono: None,
            };
            for col in 0..=line.len() {
                // Within one physical pixel of egui's pixel-snapped glyph origins.
                assert!(
                    (fast.x_of(col) - slow.x_of(col)).abs() <= 1.0 / ppp + 1e-3,
                    "ppp {ppp} col {col}: {} vs {}",
                    fast.x_of(col),
                    slow.x_of(col)
                );
            }
            assert_eq!(slow.col_at(fast.x_of(7) + 1.0), 7);
            checked = true;
        });
    h.run();
    drop(h);
    assert!(checked);
}
