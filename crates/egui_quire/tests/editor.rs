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
        let rect = Rect::from_min_size(pos2(full.left() + w * i as f32, full.top()), vec2(w, full.height()));
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
    ui.input(|inp| {
        st.left_keys = inp
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Key { key, pressed: true, .. } => Some(*key),
                _ => None,
            })
            .collect();
        st.left_text = inp
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
    });
}

fn harness(st: St) -> Harness<'static, St> {
    Harness::builder().with_size(Vec2::new(800.0, 400.0)).build_ui_state(app, st)
}

/// Click at a point (press + release) and run.
fn click_at(h: &mut Harness<'_, St>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
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
    let runs: Vec<_> = node.accesskit_node().children().filter(|c| c.role() == Role::TextRun).collect();
    assert_eq!(runs.len(), 50, "first page is 50 lines without a screen reader");
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
    let run = node.accesskit_node().children().find(|c| c.role() == Role::TextRun).unwrap();
    assert_eq!(run.value().as_deref(), Some("hi"));
    assert!(h.state().left_text.is_empty(), "text event consumed");
}

#[test]
fn handled_keys_are_consumed_unhandled_left() {
    let mut h = harness(St::new("abc"));
    h.run();
    focus(&mut h);
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
    let copied = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "hello world"));
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
    h.event(Event::Ime(egui::ImeEvent::Preedit { text: "ni".into(), active_range_chars: Some(2..2) }));
    h.step();
    assert_eq!(h.state().text(), "x", "preedit changes no buffer");
    assert!(h.state().views[0].composing());
    let v = h.state().doc.version();
    h.event(Event::Ime(egui::ImeEvent::Commit("你".into())));
    h.step();
    assert_eq!(h.state().text(), "x你");
    assert!(!h.state().views[0].composing());
    assert_eq!(h.state().doc.version(), quire_core::Version(v.0 + 1), "commit is one transaction");
    // IME output is written while focused.
    assert!(h.output().platform_output.ime.is_some());
}

#[test]
fn double_click_selects_word_triple_selects_line() {
    let mut h = harness(St::new("alpha beta gamma\nsecond"));
    h.run();
    focus(&mut h);
    // Find "beta" on line 0: use the caret rect after placing the caret at column 7.
    let r = h.state().rects[0];
    // Click into the first line well inside "beta" by measuring: put the caret at 0 first.
    h.key_press_modifiers(cmd(), Key::Home);
    h.run();
    let caret0 = h.state().caret.unwrap();
    // Monospace: column width from typing nothing; take column 7 as 7 advances.
    h.key_press(Key::End);
    h.run();
    let caret_end = h.state().caret.unwrap();
    let adv = (caret_end.left() - caret0.left()) / 16.0;
    let p = pos2(caret0.left() + adv * 7.5, caret0.center().y);
    assert!(r.contains(p));
    for _ in 0..2 {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    }
    h.run();
    assert_eq!(h.state().sel(0), (6, 10), "double click selects the word");
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(h.state().sel(0), (0, 17), "triple click selects the line with its break");
}

#[test]
fn ctrl_click_activates_link() {
    let mut h = harness(St::new("foo bar"));
    h.run();
    focus(&mut h);
    let caret = h.state().caret.unwrap();
    let p = pos2(caret.left() + 2.0, caret.center().y);
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: cmd() });
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: cmd() });
    h.run();
    assert!(h.state().events.iter().any(|e| matches!(e, EditorEvent::LinkActivated { range, .. } if *range == (0..3))));
}

#[test]
fn find_bar_finds_and_moves_selection() {
    let mut h = harness(St::new("one two one two one"));
    h.run();
    focus(&mut h);
    h.key_press_modifiers(cmd(), Key::F);
    h.run();
    h.get_by_placeholder("Find").type_text("two");
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
    assert!(h.query_by_placeholder("Find").is_none(), "escape closes the bar");
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
    h.key_press(Key::ArrowLeft);
    h.step();
    assert!(h.state().left_keys.contains(&Key::ArrowLeft), "host mode does not consume keys");
    h.key_press_modifiers(cmd(), Key::S);
    h.step();
    assert!(!h.state().events.contains(&EditorEvent::SaveRequested));
    h.event(Event::Text("z".into()));
    h.step();
    assert!(h.state().text().contains('z'), "text input is still applied");
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
    assert_eq!(h.state().sel(0).1, 12, "view 0 maps forward through the other view's edit");
}

#[test]
fn settled_when_idle() {
    let mut h = harness(St::new("x"));
    h.run();
    assert!(h.state().settled);
    assert_eq!(h.state().applied, 0);
}
