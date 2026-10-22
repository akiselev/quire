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
    /// Keymap for editor 0 (default: the bundled VS Code keymap).
    keymap: Option<egui_quire::Keymap>,
    /// Per frame, per view: events.
    events: Vec<EditorEvent>,
    applied: usize,
    /// Keys still pressed in egui input after the editors ran.
    left_keys: Vec<Key>,
    left_text: Vec<String>,
    /// `TextSelectionSettled` texts sent (accumulated; tests clear it).
    primary: Vec<String>,
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
            keymap: None,
            events: Vec::new(),
            applied: 0,
            left_keys: Vec::new(),
            left_text: Vec::new(),
            primary: Vec::new(),
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
        } else if let Some(map) = st.keymap.clone().filter(|_| i == 0) {
            ed = ed.key_handling(KeyHandling::Builtin(map));
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
    ui.ctx().output(|o| {
        for c in &o.commands {
            if let egui::OutputCommand::TextSelectionSettled(t) = c {
                st.primary.push(t.clone());
            }
        }
    });
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
    // Every command in the default keymap resolves against quire-core.
    assert_eq!(unresolved, Vec::<&str>::new());
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

/// ARCH-QUIRE §3.9 "Idle: 0 frames after the blink stops": after typing, with the pointer
/// resting over the text (so the hover timer ran too) and diagnostics shown, no frame of the
/// next 5 s of idle asks for a repaint, at any delay.
#[test]
fn idle_requests_no_frames_after_the_blink_stops() {
    let text: String = (0..200).map(|i| format!("let x{i} = {i};\n")).collect();
    let mut st = St::new(&text);
    st.doc
        .set_diagnostics(
            quire_core::SourceId::new("test"),
            st.doc.version(),
            vec![quire_core::Diagnostic::new(
                4..6,
                quire_core::Severity::Error,
                "e",
            )],
        )
        .unwrap();
    let mut h = Harness::builder()
        .with_size(Vec2::new(600.0, 300.0))
        .with_step_dt(0.1)
        .build_ui_state(app, st);
    h.ctx.all_styles_mut(|s| s.visuals.text_cursor.blink = true);
    h.run();
    focus(&mut h);
    for c in ["a", "b", "c"] {
        h.event(Event::Text(c.into()));
        h.step();
    }
    let r = h.state().rects[0];
    h.event(Event::PointerMoved(pos2(r.center().x, r.top() + 30.0)));
    // The blink stops 5 s after the last interaction; the hover fires after 0.5 s.
    h.run_steps(60);
    assert!(
        h.state()
            .events
            .iter()
            .any(|e| matches!(e, EditorEvent::HoverAt { .. }))
    );
    let mut requested = 0;
    for _ in 0..50 {
        h.step();
        if root_repaint_delay(&h) != std::time::Duration::MAX {
            requested += 1;
        }
    }
    assert_eq!(requested, 0, "frames requested while idle");
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
                tab_size: 4,
            };
            let line = "fn main() { let x = 42; }";
            let fast = sh.shape(
                line,
                &[(0, egui::Color32::RED), (3, egui::Color32::GREEN)],
                1,
                egui::Color32::WHITE,
            );
            assert!(fast.mono.is_some());
            let slow = fast.without_fast_path();
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

#[test]
fn a11y_selection_maps_and_page_follows_caret() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(St::new(&text));
    h.run();
    focus(&mut h);
    let id = CodeEditor::id_for(("ed", 0usize));
    h.key_press_modifiers(cmd(), Key::End);
    h.run();
    let head = h.state().sel(0).1;
    assert_eq!(
        h.state().doc.text().byte_to_line(head),
        200,
        "caret on the last (empty) line"
    );
    let node = h.get_by_label("Source");
    let runs: Vec<_> = node
        .accesskit_node()
        .children()
        .filter(|c| c.role() == Role::TextRun)
        .collect();
    assert_eq!(runs.len(), 1, "page 200..250 holds only the last line");
    let sel = node
        .accesskit_node()
        .data()
        .text_selection()
        .copied()
        .expect("selection mapped");
    assert_eq!(
        sel.focus.node,
        egui_quire::a11y_run_id(id, 200, 0).accesskit_id()
    );
    assert_eq!(sel.focus.character_index, 0);
    // Back up a few lines: the page shifts to 150..200.
    for _ in 0..3 {
        h.key_press(Key::ArrowUp);
    }
    h.run();
    let node = h.get_by_label("Source");
    let runs: Vec<_> = node
        .accesskit_node()
        .children()
        .filter(|c| c.role() == Role::TextRun)
        .collect();
    assert_eq!(runs.len(), 50);
    assert_eq!(runs[0].value().as_deref(), Some("line 150\n"));
}

#[test]
fn a11y_page_is_500_lines_with_a_screen_reader() {
    let text: String = (0..800).map(|i| format!("{i}\n")).collect();
    let mut h = harness(St::new(&text));
    h.ctx.options_mut(|o| o.screen_reader = true);
    h.run();
    let node = h.get_by_label("Source");
    assert_eq!(
        node.accesskit_node()
            .children()
            .filter(|c| c.role() == Role::TextRun)
            .count(),
        500
    );
}

#[test]
fn host_undo_mode_emits_undo_requested() {
    let mut st = St::new("abc");
    st.doc = Document::new("abc", Default::default()).with_undo_mode(quire_core::UndoMode::Host);
    st.views = vec![EditorView::new(&st.doc), EditorView::new(&st.doc)];
    let mut h = harness(st);
    h.run();
    focus(&mut h);
    h.event(Event::Text("d".into()));
    h.run();
    assert_eq!(h.state().text(), "abcd");
    h.key_press_modifiers(cmd(), Key::Z);
    h.key_press_modifiers(cmd(), Key::Y);
    h.run();
    assert!(h.state().events.contains(&EditorEvent::UndoRequested));
    assert!(h.state().events.contains(&EditorEvent::RedoRequested));
    assert_eq!(
        h.state().text(),
        "abcd",
        "the host answers undo, not the widget"
    );
}

#[test]
fn local_undo_redo_and_dirty() {
    let mut h = harness(St::new("abc"));
    h.run();
    focus(&mut h);
    h.event(Event::Text("d".into()));
    h.run();
    assert!(h.state().doc.is_dirty());
    h.key_press_modifiers(cmd(), Key::Z);
    h.run();
    assert_eq!(h.state().text(), "abc");
    h.key_press_modifiers(cmd(), Key::Y);
    h.run();
    assert_eq!(h.state().text(), "abcd");
}

#[test]
fn default_theme_colours_tokens() {
    use quire_core::syntax::TokenType;
    let t = egui_quire::EditorTheme::default();
    assert_ne!(t.token_color(TokenType::Keyword as usize), t.foreground);
    assert_eq!(t.token_color(TokenType::Text as usize), t.foreground);
    assert_eq!(t.token_colors.len(), TokenType::ALL.len());
}

#[test]
fn cut_with_empty_selection_copies_the_line_it_deletes() {
    let mut h = harness(St::new("one\ntwo\nthree"));
    h.run();
    // Put the caret on line 1 ("two"): click line 0, then one row below it.
    let r = h.state().rects[0];
    click_at(&mut h, pos2(r.right() - 30.0, r.top() + 8.0));
    let c0 = h.state().caret.unwrap();
    click_at(
        &mut h,
        pos2(r.right() - 30.0, c0.bottom() + c0.height() * 0.5),
    );
    assert_eq!(h.state().sel(0), (7, 7));
    h.event(Event::Cut);
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
    assert_eq!(copied, vec!["two\n".to_owned()]);
    assert_eq!(h.state().text(), "one\nthree");
}

// ---------------------------------------------------------------------------------------------
// Tabs at tab stops.

/// Shape `line` with `tab_size` and call `f` with the layout (fast path) and the advance.
fn with_shaped(line: &str, tab_size: u32, f: impl FnOnce(&egui_quire::layout::LineLayout, f32)) {
    with_shaped_font(line, tab_size, egui::FontId::monospace(13.0), f);
}

fn with_shaped_font(
    line: &str,
    tab_size: u32,
    font: egui::FontId,
    f: impl FnOnce(&egui_quire::layout::LineLayout, f32),
) {
    let mut f = Some(f);
    let line = line.to_owned();
    let mut h = Harness::builder().build_ui(move |ui| {
        let mono = egui_quire::layout::mono(ui.ctx(), &font);
        let mut cache = egui_quire::galley_cache::GalleyCache::default();
        let mut sh = egui_quire::layout::LineShaper {
            cache: &mut cache,
            ctx: ui.ctx(),
            font: &font,
            pixels_per_point: ui.ctx().pixels_per_point(),
            mono,
            tab_size,
        };
        let ll = sh.shape(&line, &[], 1, egui::Color32::WHITE);
        if let Some(f) = f.take() {
            f(&ll, mono.advance());
        }
    });
    h.run();
}

#[test]
fn tabs_advance_to_tab_stops() {
    for (line, tab_size, col_of_x) in [
        ("\tx", 4, 4.0),
        ("a\tx", 4, 4.0),
        ("abc\tx", 4, 4.0),
        ("abcd\tx", 4, 8.0),
        ("\t\tx", 4, 8.0),
        ("ab\tx", 2, 4.0),
        ("a\tx", 8, 8.0),
    ] {
        with_shaped(line, tab_size, |ll, adv| {
            let col = line.len() - 1;
            assert!(ll.mono.is_some());
            let fast = ll.x_of(col);
            assert!(
                (fast - col_of_x * adv).abs() < 1e-3,
                "{line:?} ts {tab_size}: {fast} vs {}",
                col_of_x * adv
            );
            // The galley itself places the glyph there (egui's tab advance corrected).
            let slow = ll.without_fast_path();
            assert!(
                (slow.x_of(col) - fast).abs() <= 1.0,
                "{line:?}: galley {} vs fast {fast}",
                slow.x_of(col)
            );
            // Hit-testing inside the tab: left half → before it, right half → after it.
            let tab_at = line.find('\t').unwrap();
            let (x0, x1) = (ll.x_of(tab_at), ll.x_of(tab_at + 1));
            assert_eq!(ll.col_at(x0 + (x1 - x0) * 0.3), tab_at, "{line:?}");
            assert_eq!(ll.col_at(x0 + (x1 - x0) * 0.7), tab_at + 1, "{line:?}");
            assert_eq!(slow.col_at(x0 + (x1 - x0) * 0.7), tab_at + 1, "{line:?}");
        });
    }
}

#[test]
fn trailing_tab_extends_line_width() {
    with_shaped("ab\t", 4, |ll, adv| {
        assert!((ll.x_of(3) - 4.0 * adv).abs() < 1e-3);
        assert!((ll.width() - 4.0 * adv).abs() < 1e-3);
        let slow = ll.without_fast_path();
        assert!((slow.x_of(3) - 4.0 * adv).abs() <= 1.0);
    });
}

#[test]
fn tabs_after_non_ascii_use_glyph_positions() {
    // 'é' is one column in the monospace font; the galley path must still reach the stop.
    with_shaped("é\tx", 4, |ll, adv| {
        assert!(ll.mono.is_none());
        let x = ll.x_of("é\t".len());
        assert!((x - 4.0 * adv).abs() <= 1.0, "{x} vs {}", 4.0 * adv);
    });
    // A proportional font (glyphs of uneven width): the text after the tab lands on a stop.
    with_shaped_font("Wé\tx", 4, egui::FontId::proportional(13.0), |ll, adv| {
        let x = ll.x_of("Wé\t".len());
        let stop = 4.0 * adv;
        let k = (x / stop).round();
        assert!(
            k >= 1.0 && (x - k * stop).abs() <= 1.0,
            "{x} not on a stop of {stop}"
        );
        assert!(x > ll.x_of("Wé".len()));
    });
}

#[test]
fn editor_caret_and_clicks_honour_tab_size() {
    let cfg = quire_core::EditorConfig {
        tab_size: 4,
        ..Default::default()
    };
    let mut st = St::new("abcdefgh\nab\n\tfoo\n");
    st.doc.set_config(cfg);
    let mut h = harness(st);
    h.run();
    let (left, adv, row0) = measure(&mut h, 8, 2);
    let row_h = row0.height();
    let y2 = row0.top() + row_h * 2.5;
    // Caret at the end of "\tfoo": 4 + 3 columns.
    click_at(&mut h, pos2(left + 30.0 * adv, y2));
    let start2 = "abcdefgh\nab\n".len();
    assert_eq!(h.state().sel(0).1, start2 + 4);
    let c = h.state().caret.unwrap();
    assert!(
        (c.left() - (left + 7.0 * adv)).abs() < 0.6,
        "{} vs {}",
        c.left(),
        left + 7.0 * adv
    );
    // Click a column inside the tab: left half → before it, right half → after it.
    click_at(&mut h, pos2(left + 1.0 * adv, y2));
    assert_eq!(h.state().sel(0).1, start2);
    click_at(&mut h, pos2(left + 3.0 * adv, y2));
    assert_eq!(h.state().sel(0).1, start2 + 1);
    // Tab size 8 moves "foo" to column 8.
    let mut cfg = h.state().doc.config().clone();
    cfg.tab_size = 8;
    h.state_mut().doc.set_config(cfg);
    click_at(&mut h, pos2(left + 30.0 * adv, y2));
    let c = h.state().caret.unwrap();
    assert!((c.left() - (left + 11.0 * adv)).abs() < 0.6);
}

// ---------------------------------------------------------------------------------------------
// Multiple cursors and selections (drawn from the view's `Selection`).

/// Filled rects of `color` in the last frame's shapes.
fn rects_of(h: &Harness<'_, St>, color: egui::Color32) -> Vec<Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.fill == color => Some(r.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn multiple_cursors_and_selections_are_drawn_and_typed_into() {
    let mut h = harness(St::new("one\ntwo\nthree\n"));
    h.run();
    focus(&mut h);
    {
        let St { doc, views, .. } = h.state_mut();
        let sel = quire_core::Selection::new(
            [
                quire_core::SelRange::point(1),
                quire_core::SelRange::point(5),
                quire_core::SelRange::new(8, 13),
            ],
            0,
        );
        let tr = quire_core::Transaction::select(doc.version(), doc.text().len_bytes(), sel);
        doc.apply(tr, &mut views[0]).unwrap();
    }
    h.run();
    let theme = egui_quire::EditorTheme::from_visuals(&h.ctx.global_style().visuals);
    let carets = rects_of(&h, theme.caret);
    assert_eq!(carets.len(), 3, "one caret per range: {carets:?}");
    let mut tops: Vec<f32> = carets.iter().map(|r| r.top()).collect();
    tops.sort_by(f32::total_cmp);
    assert!(tops[0] < tops[1] && tops[1] < tops[2], "on three lines");
    let sels = rects_of(&h, theme.selection);
    assert_eq!(sels.len(), 1, "one non-empty range: {sels:?}");
    assert!(sels[0].width() > 0.0);
    // Typing goes to every cursor and replaces the selection.
    h.event(Event::Text("X".into()));
    h.run();
    assert_eq!(h.state().text(), "oXne\ntXwo\nX\n");
    assert_eq!(rects_of(&h, theme.caret).len(), 3);
}

// ---------------------------------------------------------------------------------------------
// Horizontal extent from the longest line.

fn wheel(h: &mut Harness<'_, St>, delta: Vec2) {
    let r = h.state().rects[0];
    h.event(Event::PointerMoved(r.center()));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(40);
}

#[test]
fn horizontal_extent_is_the_longest_line_even_off_screen() {
    // The long line is far below the viewport: never laid out, still scrollable to.
    let mut text: String = (0..300).map(|i| format!("{i}\n")).collect();
    text.push_str(&"x".repeat(400));
    text.push('\n');
    let mut h = harness(St::new(&text));
    h.run();
    wheel(&mut h, vec2(-100_000.0, 0.0));
    let x = h.state().views[0].scroll().horizontal;
    let adv = egui_quire::layout::mono(&h.ctx, &egui::FontId::monospace(13.0)).advance();
    let view_w = h.state().rects[0].width();
    // 400 columns plus margin, minus the visible text width (gutter excluded): at least
    // 400 columns minus the whole editor width.
    assert!(
        x > 400.0 * adv - view_w && x < 410.0 * adv,
        "scroll x {x} (adv {adv})"
    );
    // Another party deletes the long line; the extent shrinks and the scroll clamps.
    let start = h.state().doc.text().line_to_byte(300);
    let len = h.state().doc.text().len_bytes();
    {
        let st = h.state_mut();
        let cs = quire_core::ChangeSet::replace(len, [(start..len, "")]).unwrap();
        let tr = quire_core::Transaction::new(st.doc.version(), cs);
        let St { doc, views, .. } = st;
        doc.apply(tr, &mut views[1]).unwrap();
    }
    h.run();
    assert_eq!(h.state().views[0].scroll().horizontal, 0.0);
}

// ---------------------------------------------------------------------------------------------
// Two-chord bindings.

fn chord_state(text: &str) -> St {
    let mut st = St::new(text);
    let cfg = quire_core::EditorConfig {
        line_comment: Some("//".into()),
        ..Default::default()
    };
    st.doc.set_config(cfg);
    st.keymap = Some(
        egui_quire::Keymap::from_json(
            r#"[
                { "key": "ctrl+k ctrl+c", "mac": "cmd+k cmd+c", "command": "editor.action.commentLine" },
                { "key": "ctrl+k", "mac": "cmd+k", "command": "cursorEnd" },
                { "key": "left", "command": "cursorLeft" }
            ]"#,
        )
        .unwrap(),
    );
    st
}

#[test]
fn two_chord_binding_runs_after_both_chords() {
    let mut h = harness(chord_state("let a;"));
    h.run();
    focus(&mut h);
    h.state_mut().left_keys.clear();
    h.key_press_modifiers(cmd(), Key::K);
    h.run_steps(5);
    assert_eq!(h.state().text(), "let a;", "the prefix alone does nothing");
    assert!(
        h.state().left_keys.is_empty(),
        "the prefix is consumed (and wins over the one-chord binding of the same keys)"
    );
    h.key_press_modifiers(cmd(), Key::C);
    h.run();
    assert_eq!(h.state().text(), "// let a;");
    assert!(h.state().left_keys.is_empty());
}

#[test]
fn escape_or_another_key_cancels_a_pending_chord() {
    let mut h = harness(chord_state("let a;"));
    h.run();
    focus(&mut h);
    let caret = h.state().sel(0);
    // Escape cancels and is consumed; focus stays.
    h.key_press_modifiers(cmd(), Key::K);
    h.run();
    h.state_mut().left_keys.clear();
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().left_keys.contains(&Key::Escape));
    assert!(
        h.ctx
            .memory(|m| m.has_focus(CodeEditor::id_for(("ed", 0usize))))
    );
    h.key_press_modifiers(cmd(), Key::C);
    h.run();
    assert_eq!(
        h.state().text(),
        "let a;",
        "second chord alone is not the sequence"
    );
    // A key that does not complete the sequence cancels it and is swallowed, not run.
    h.key_press_modifiers(cmd(), Key::K);
    h.run();
    h.key_press(Key::ArrowLeft);
    h.run();
    assert_eq!(
        h.state().sel(0),
        caret,
        "Left was swallowed by the cancelled sequence"
    );
    h.key_press(Key::ArrowLeft);
    h.run();
    assert_ne!(h.state().sel(0), caret, "and works again afterwards");
}

#[test]
fn pending_chord_waits_indefinitely_but_not_across_focus_loss() {
    let mut h = harness(chord_state("let a;"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::K);
    h.run();
    h.run_steps(600); // 10 s at 60 Hz: still pending, as in VS Code
    h.key_press_modifiers(cmd(), Key::C);
    h.run();
    assert_eq!(h.state().text(), "// let a;");
    // Focus loss cancels.
    h.key_press_modifiers(cmd(), Key::K);
    h.run();
    h.ctx
        .memory_mut(|m| m.surrender_focus(CodeEditor::id_for(("ed", 0usize))));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::C);
    h.run();
    assert_eq!(h.state().text(), "// let a;");
}

#[test]
fn keymap_parses_two_chord_entries() {
    let map = egui_quire::Keymap::vscode_default();
    let os = egui::os::OperatingSystem::Nix;
    let ctx = egui_quire::keymap::WhenContext::default();
    // ctrl+k starts the fold and comment chords.
    let k = egui_quire::Chord::parse("ctrl+k").unwrap();
    assert!(matches!(
        map.lookup_seq(os, None, Key::K, Modifiers::CTRL, &ctx),
        egui_quire::KeyLookup::Prefix(c) if c == k
    ));
    match map.lookup_seq(os, Some(k), Key::C, Modifiers::CTRL, &ctx) {
        egui_quire::KeyLookup::Bound(b) => assert_eq!(b.command_id, "editor.action.addCommentLine"),
        other => panic!("{other:?}"),
    }
    match map.lookup_seq(os, Some(k), Key::Num0, Modifiers::CTRL, &ctx) {
        egui_quire::KeyLookup::Bound(b) => assert_eq!(b.command_id, "editor.foldAll"),
        other => panic!("{other:?}"),
    }
    let custom = chord_state("").keymap.unwrap();
    assert!(matches!(
        custom.lookup_seq(os, None, Key::K, Modifiers::CTRL, &ctx),
        egui_quire::KeyLookup::Prefix(c) if c == k
    ));
    match custom.lookup_seq(os, Some(k), Key::C, Modifiers::CTRL, &ctx) {
        egui_quire::KeyLookup::Bound(b) => assert_eq!(b.command_id, "editor.action.commentLine"),
        other => panic!("{other:?}"),
    }
    // The one-step lookup still sees the one-chord binding.
    assert_eq!(
        custom
            .lookup(os, Key::K, Modifiers::CTRL, &ctx)
            .unwrap()
            .command_id,
        "cursorEnd"
    );
}

// ---------------------------------------------------------------------------------------------
// X11/Wayland PRIMARY selection.

fn settled_selections(h: &mut Harness<'_, St>) -> Vec<String> {
    std::mem::take(&mut h.state_mut().primary)
}

#[test]
fn settled_selection_is_reported_once_for_primary() {
    let mut h = harness(St::new("alpha beta\n"));
    h.run();
    focus(&mut h);
    settled_selections(&mut h);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowLeft);
    h.step();
    assert_eq!(settled_selections(&mut h), vec!["\n".to_owned()]);
    h.step();
    assert!(
        settled_selections(&mut h).is_empty(),
        "not re-sent while unchanged"
    );
    h.key_press_modifiers(cmd(), Key::A);
    h.step();
    assert_eq!(settled_selections(&mut h), vec!["alpha beta\n".to_owned()]);
    // While a drag is in progress nothing is sent; on release the final selection is.
    let r = h.state().rects[0];
    let p0 = pos2(r.right() - 30.0, r.top() + 8.0);
    h.event(Event::PointerMoved(p0));
    h.event(Event::PointerButton {
        pos: p0,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    let p1 = pos2(r.left() + 60.0, r.top() + 8.0);
    for i in 1..=4 {
        h.event(Event::PointerMoved(p0 + (p1 - p0) * (i as f32 / 4.0)));
        h.step();
    }
    assert!(
        settled_selections(&mut h).is_empty(),
        "nothing while dragging"
    );
    h.event(Event::PointerButton {
        pos: p1,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let (a, b) = h.state().sel(0);
    assert!(a != b);
    let expect = h.state().text()[b.min(a)..a.max(b)].to_owned();
    assert_eq!(settled_selections(&mut h), vec![expect]);
}

#[test]
fn middle_click_pastes_primary_at_the_pointer() {
    let mut h = harness(St::new("abcdefgh\nab\n"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 8, 2);
    let p = pos2(left + 4.0 * adv, row0.center().y);
    h.event(Event::PointerMoved(p));
    h.event(Event::MiddleClickPaste {
        pos: p,
        text: "XY".into(),
    });
    h.run();
    assert_eq!(h.state().text(), "abcdXYefgh\nab\n");
    assert_eq!(h.state().sel(0), (6, 6));
}

// ---------------------------------------------------------------------------------------------
// Page-shift announcement.

fn announcement(h: &Harness<'_, St>) -> Option<(String, Option<egui::accesskit::Live>)> {
    let node = h.get_by_label("Source");
    let n = node
        .accesskit_node()
        .children()
        .find(|c| c.role() == Role::Status)?;
    Some((n.label().unwrap_or_default(), n.data().live()))
}

#[test]
fn page_shift_announces_the_line() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(St::new(&text));
    h.run();
    focus(&mut h);
    assert_eq!(announcement(&h), None, "nothing to announce before a shift");
    h.key_press_modifiers(cmd(), Key::Home);
    h.run();
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        announcement(&h),
        None,
        "moving within the page announces nothing"
    );
    h.key_press_modifiers(cmd(), Key::End);
    h.run();
    assert_eq!(
        announcement(&h),
        Some(("Line 201".to_owned(), Some(egui::accesskit::Live::Polite)))
    );
    // Line 201 → 200 leaves page 200..250 for 150..200.
    h.key_press(Key::ArrowUp);
    h.run();
    assert_eq!(announcement(&h).unwrap().0, "Line 200");
}

// ---------------------------------------------------------------------------------------------
// Core ED2 commands through the default keymap (multi-cursor, lines, column select, folds).

fn is_mac(h: &Harness<'_, St>) -> bool {
    matches!(
        h.ctx.os(),
        egui::os::OperatingSystem::Mac | egui::os::OperatingSystem::IOS
    )
}

/// Press the default keymap's keys for `command_id` on the harness's platform. Returns false
/// (pressing nothing) when the command has no binding there.
fn press_command(h: &mut Harness<'_, St>, command_id: &str) -> bool {
    let os = h.ctx.os();
    let mac = is_mac(h);
    let Some(keys) = egui_quire::Keymap::vscode_default().keys_for(os, command_id) else {
        return false;
    };
    for c in std::iter::once(keys.first).chain(keys.second) {
        h.key_press_modifiers(c.modifiers(mac), c.key);
        h.run();
    }
    true
}

fn caret_to(h: &mut Harness<'_, St>, pos: usize) {
    let St { doc, views, .. } = h.state_mut();
    let tr = quire_core::Transaction::select(
        doc.version(),
        doc.text().len_bytes(),
        quire_core::Selection::point(pos),
    );
    doc.apply(tr, &mut views[0]).unwrap();
    h.run();
}

fn ranges(h: &Harness<'_, St>) -> Vec<(usize, usize)> {
    h.state().views[0]
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.anchor, r.head))
        .collect()
}

#[test]
fn insert_cursor_below_and_above_through_keys() {
    let mut h = harness(St::new("abc\ndef\nghi"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 5);
    assert!(press_command(&mut h, "editor.action.insertCursorBelow"));
    assert!(press_command(&mut h, "editor.action.insertCursorAbove"));
    assert_eq!(h.state().views[0].selection().len(), 3);
    h.event(Event::Text("X".into()));
    h.run();
    assert_eq!(h.state().text(), "aXbc\ndXef\ngXhi");
}

#[test]
fn add_selection_to_next_find_match_through_keys() {
    let mut h = harness(St::new("foo bar foo baz foo"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 1);
    assert!(press_command(
        &mut h,
        "editor.action.addSelectionToNextFindMatch"
    ));
    assert_eq!(ranges(&h), vec![(0, 3)], "first press selects the word");
    assert!(press_command(
        &mut h,
        "editor.action.addSelectionToNextFindMatch"
    ));
    assert_eq!(ranges(&h), vec![(0, 3), (8, 11)]);
    h.event(Event::Text("q".into()));
    h.run();
    assert_eq!(h.state().text(), "q bar q baz foo");
}

#[test]
fn move_and_copy_lines_through_keys() {
    let mut h = harness(St::new("a\nb\nc"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 0);
    assert!(press_command(&mut h, "editor.action.moveLinesDownAction"));
    assert_eq!(h.state().text(), "b\na\nc");
    assert!(press_command(&mut h, "editor.action.copyLinesUpAction"));
    assert_eq!(h.state().text(), "b\na\na\nc");
    assert!(press_command(&mut h, "editor.action.moveLinesUpAction"));
    assert!(press_command(&mut h, "editor.action.copyLinesDownAction"));
    assert_eq!(h.state().text(), "a\na\nb\na\nc");
}

fn select(h: &mut Harness<'_, St>, anchor: usize, head: usize) {
    let St { doc, views, .. } = h.state_mut();
    let tr = quire_core::Transaction::select(
        doc.version(),
        doc.text().len_bytes(),
        quire_core::Selection::single(anchor, head),
    );
    doc.apply(tr, &mut views[0]).unwrap();
    h.run();
}

/// Press `keys` (VS Code notation) with this harness's platform modifiers.
fn press_keys(h: &mut Harness<'_, St>, keys: &str) {
    let mac = is_mac(h);
    let seq = egui_quire::KeySeq::parse(keys).unwrap();
    for c in std::iter::once(seq.first).chain(seq.second) {
        h.key_press_modifiers(c.modifiers(mac), c.key);
        h.run();
    }
}

#[test]
fn add_and_remove_line_comment_chords() {
    let mut h = harness(St::new("a\nb"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 0);
    assert!(press_command(&mut h, "editor.action.addCommentLine"));
    assert_eq!(h.state().text(), "// a\nb");
    // Adding again nests (addCommentLine always adds).
    assert!(press_command(&mut h, "editor.action.addCommentLine"));
    assert_eq!(h.state().text(), "// // a\nb");
    assert!(press_command(&mut h, "editor.action.removeCommentLine"));
    assert_eq!(h.state().text(), "// a\nb");
    assert!(press_command(&mut h, "editor.action.removeCommentLine"));
    assert_eq!(h.state().text(), "a\nb");
}

#[test]
fn block_comment_binding() {
    let mut h = harness(St::new("abcd"));
    h.run();
    focus(&mut h);
    select(&mut h, 1, 3);
    // shift+alt+a (ctrl+shift+a on Linux).
    assert!(press_command(&mut h, "editor.action.blockComment"));
    assert_eq!(h.state().text(), "a/* bc */d");
    assert!(press_command(&mut h, "editor.action.blockComment"));
    assert_eq!(h.state().text(), "abcd");
}

#[test]
fn delete_all_right_where_bound() {
    let mut h = harness(St::new("hello world\nnext"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 5);
    // VS Code binds deleteAllRight on macOS only: ctrl+k, and cmd+delete as the secondary.
    if !press_command(&mut h, "deleteAllRight") {
        assert!(!is_mac(&h));
        return;
    }
    assert_eq!(h.state().text(), "hello\nnext");
    caret_to(&mut h, 2);
    press_keys(&mut h, "cmd+delete");
    assert_eq!(h.state().text(), "he\nnext");
}

#[test]
fn delete_all_left_where_bound() {
    let mut h = harness(St::new("hello world"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 6);
    // VS Code binds deleteAllLeft on macOS only (cmd+backspace).
    if press_command(&mut h, "deleteAllLeft") {
        assert_eq!(h.state().text(), "world");
    } else {
        assert!(!is_mac(&h));
    }
}

#[test]
fn column_select_with_keys() {
    let mut h = harness(St::new("abcd\nefgh\nijkl"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 1);
    // VS Code binds no column-select keys on Linux (`linux: { primary: 0 }`).
    if !press_command(&mut h, "cursorColumnSelectRight") {
        assert_eq!(h.ctx.os(), egui::os::OperatingSystem::Nix);
        return;
    }
    assert!(press_command(&mut h, "cursorColumnSelectRight"));
    assert_eq!(ranges(&h), vec![(1, 3)]);
    assert!(press_command(&mut h, "cursorColumnSelectDown"));
    assert_eq!(ranges(&h), vec![(1, 3), (6, 8)]);
    assert!(press_command(&mut h, "cursorColumnSelectPageDown"));
    assert_eq!(ranges(&h), vec![(1, 3), (6, 8), (11, 13)]);
}

fn mouse(h: &mut Harness<'_, St>, p: Pos2, pressed: bool, modifiers: Modifiers) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    });
}

#[test]
fn alt_shift_drag_makes_a_box_selection_and_alt_click_adds_a_cursor() {
    let mut h = harness(St::new("abcdefgh\nab\nabcdefgh\nabcdefgh\n"));
    h.run();
    let (left, adv, row0) = measure(&mut h, 8, 2);
    let row_h = row0.height();
    let at = |col: f32, line: f32| pos2(left + col * adv, row0.top() + row_h * (line + 0.5));
    let m = Modifiers::ALT | Modifiers::SHIFT;
    mouse(&mut h, at(1.0, 0.0), true, m);
    h.step();
    for i in 1..=4 {
        let t = i as f32 / 4.0;
        h.event(Event::PointerMoved(at(1.0 + 3.0 * t, 3.0 * t)));
        h.step();
    }
    mouse(&mut h, at(4.0, 3.0), false, m);
    h.run();
    // Line 1 ("ab") is shorter than the box: its range is clipped to the line's end.
    assert_eq!(
        ranges(&h),
        vec![(1, 4), (10, 11), (13, 16), (22, 25)],
        "box selection over four lines"
    );
    // Alt+click adds a cursor.
    caret_to(&mut h, 0);
    mouse(&mut h, at(5.0, 2.0), true, Modifiers::ALT);
    mouse(&mut h, at(5.0, 2.0), false, Modifiers::ALT);
    h.run();
    assert_eq!(ranges(&h), vec![(0, 0), (17, 17)]);
}

#[test]
fn box_selection_columns_follow_wide_glyphs() {
    // Two wide CJK glyphs are four visual columns (as core counts them), whatever their drawn
    // width relative to the monospace advance.
    // The test fonts have no CJK glyphs: tofu boxes are wide glyphs whose width is not two
    // advances, which is the case the galley hit test fixes.
    let mut h = Harness::builder()
        .with_size(Vec2::new(800.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .build_ui_state(app, St::new("中文ab\nabcdefgh\n"));
    h.run();
    focus(&mut h);
    caret_to(&mut h, 9);
    let c9 = h.state().caret.expect("caret rect");
    caret_to(&mut h, 10);
    let adv = h.state().caret.unwrap().left() - c9.left();
    caret_to(&mut h, "中文".len());
    let caret = h.state().caret.expect("caret rect");
    // Anchor on the ASCII line 1 at visual column 4; head on line 0 one advance right of the
    // end of "中文" (between "a" and "b": visual column 5 there, however wide the glyphs draw).
    let x_a = caret.left();
    assert!(
        (x_a - c9.left() - 4.0 * adv).abs() > 0.5 * adv,
        "the wide glyphs must not draw exactly two advances wide, or x / advance would pass too"
    );
    let y0 = caret.center().y;
    let y1 = y0 + caret.height();
    let m = Modifiers::ALT | Modifiers::SHIFT;
    let anchor = pos2(c9.left() + 4.0 * adv, y1);
    let head = pos2(x_a + 1.1 * adv, y0);
    mouse(&mut h, anchor, true, m);
    h.step();
    h.event(Event::PointerMoved(head));
    h.step();
    mouse(&mut h, head, false, m);
    h.run();
    // Line 1 (starts at 9): columns 4..5 are bytes 13..14; line 0: "a" is bytes 6..7.
    let mut r = ranges(&h);
    r.sort_unstable();
    assert_eq!(r, vec![(6, 7), (13, 14)]);
}

#[test]
fn folds_hide_lines_and_the_gutter_marker_toggles() {
    let text = "fn a() {\n    x;\n    y;\n}\nz\n";
    let mut h = harness(St::new(text));
    h.run();
    focus(&mut h);
    caret_to(&mut h, text.find('y').unwrap());
    let id = CodeEditor::id_for(("ed", 0usize));
    let top = |h: &Harness<'_, St>, n: usize| {
        CodeEditor::anchor_rect(&h.ctx, id, &format!("line[{n}]")).unwrap()
    };
    let row_h = top(&h, 0).height();
    // Fold everything (a two-chord binding).
    assert!(press_command(&mut h, "editor.foldAll"));
    assert!(!h.state().views[0].folds().is_empty());
    assert_eq!(top(&h, 1).height(), 0.0, "hidden lines take no space");
    assert_eq!(top(&h, 2).height(), 0.0);
    assert!((top(&h, 3).top() - top(&h, 0).bottom()).abs() < 0.01);
    // The caret moved out of the hidden text, to the header's end.
    assert_eq!(h.state().sel(0).1, text.find('\n').unwrap());
    // Clicking a hidden line's old place lands on the next visible line.
    let r0 = top(&h, 0);
    click_at(&mut h, pos2(r0.left() + 1.0, r0.bottom() + row_h * 0.5));
    let l = h.state().doc.text().byte_to_line(h.state().sel(0).1);
    assert_eq!(l, 3);
    // The gutter's fold marker unfolds.
    let gutter_x = r0.left() - 6.0;
    click_at(&mut h, pos2(gutter_x, r0.center().y));
    assert!(h.state().views[0].folds().is_empty(), "marker unfolded");
    assert_eq!(top(&h, 1).height(), row_h);
    click_at(&mut h, pos2(gutter_x, r0.center().y));
    assert!(!h.state().views[0].folds().is_empty(), "marker folds again");
    assert!(press_command(&mut h, "editor.unfoldAll"));
    assert!(h.state().views[0].folds().is_empty());
}

#[test]
fn a11y_page_skips_folded_lines() {
    // Line 0 opens a block of 30 indented lines (1..=30), line 31 closes it, 100 lines follow.
    let mut text = String::from("block {\n");
    text.extend((1..=30).map(|i| format!("    in {i}\n")));
    text.push_str("}\n");
    text.extend((32..132).map(|i| format!("line {i}\n")));
    let mut h = harness(St::new(&text));
    h.run();
    focus(&mut h);
    let id = CodeEditor::id_for(("ed", 0usize));
    assert!(press_command(&mut h, "editor.foldAll"));
    h.run();
    let node = h.get_by_label("Source");
    let runs: Vec<_> = node
        .accesskit_node()
        .children()
        .filter(|c| c.role() == Role::TextRun)
        .collect();
    assert_eq!(runs.len(), 50, "50 visible lines");
    assert_eq!(runs[0].value().as_deref(), Some("block {\n"));
    assert_eq!(
        runs[1].value().as_deref(),
        Some("}\n"),
        "hidden lines get no run"
    );
    assert_eq!(runs[49].value().as_deref(), Some("line 79\n"));
    // Run ids stay keyed by document line: the caret on line 31 maps to run (31, 0).
    let close = h.state().doc.text().line_to_byte(31);
    caret_to(&mut h, close);
    let sel = h
        .get_by_label("Source")
        .accesskit_node()
        .data()
        .text_selection()
        .copied()
        .expect("selection mapped");
    assert_eq!(
        sel.focus.node,
        egui_quire::a11y_run_id(id, 31, 0).accesskit_id()
    );
    // The next visible line after the page starts the next page.
    let start = h.state().doc.text().line_to_byte(80);
    caret_to(&mut h, start);
    let node = h.get_by_label("Source");
    let first = node
        .accesskit_node()
        .children()
        .find(|c| c.role() == Role::TextRun)
        .unwrap();
    assert_eq!(first.value().as_deref(), Some("line 80\n"));
}

#[test]
fn ime_commit_goes_through_the_typing_rules() {
    let mut h = harness(St::new(""));
    h.run();
    focus(&mut h);
    h.event(Event::Ime(egui::ImeEvent::Preedit {
        text: "(".into(),
        active_range_chars: None,
    }));
    h.step();
    h.event(Event::Ime(egui::ImeEvent::Commit("(".into())));
    h.step();
    assert_eq!(
        h.state().text(),
        "()",
        "compositionType auto-closes like typing"
    );
    // DeleteSurrounding is one compositionType too.
    h.event(Event::Ime(egui::ImeEvent::DeleteSurrounding {
        before_chars: 1,
        after_chars: 0,
    }));
    h.step();
    assert_eq!(h.state().text(), ")");
}
