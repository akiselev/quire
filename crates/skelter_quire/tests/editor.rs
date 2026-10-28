//! The editor engine end to end on skelter types, with skelter-text's bundled test fonts (Noto Sans:
//! proportional, so these take the general shaping path; the fast path has unit tests in
//! `shape.rs`).

use std::sync::Arc;

use quire_core::{Diagnostic, Document, EditorConfig, EditorView, Severity, SourceId, WordWrap};
use skelter_base::color::Color;
use skelter_base::commit::FastCommit;
use skelter_base::display::Prim;
use skelter_base::event::{
    ImeEvent, KeyEvent, KeyLocation, KeyState, Modifiers, MouseButton, PointerType, ScrollDelta,
};
use skelter_base::geom::{Affine, Point, Rect, Size, Vec2};
use skelter_base::ids::{InputSeq, LayerId, NodeId, PropNodeId, Revision, RunId, WindowId};
use skelter_base::input::TextInputClient;
use skelter_base::key::Key;
use skelter_base::text::GlyphRun;
use skelter_base::time::{Span, Time};
use skelter_core::PointerInput;
use skelter_quire::editor::{EditorEvent, EditorOptions, SkelterEditor};
use skelter_quire::ime::{ImeClient, ImeTarget};
use skelter_quire::layers::{EditorLayerIds, EditorLayers};
use skelter_quire::{EditorTheme, OperatingSystem, RunSink};

fn fonts() -> Arc<skelter_text::FontStore> {
    skelter_text::FontStore::new(skelter_text::FontConfig::testing())
}

fn editor(os: OperatingSystem) -> SkelterEditor {
    let theme = EditorTheme {
        font_family: skelter_text::test_fonts::SANS_FAMILY.into(),
        ..EditorTheme::default()
    };
    SkelterEditor::new(
        fonts(),
        theme,
        EditorOptions {
            os,
            ..EditorOptions::default()
        },
    )
}

struct Fixture {
    doc: Document,
    view: EditorView,
    ed: SkelterEditor,
}

fn fixture(text: &str, size: Size) -> Fixture {
    fixture_with(text, size, EditorConfig::default())
}

fn fixture_with(text: &str, size: Size, cfg: EditorConfig) -> Fixture {
    let doc = Document::new(text, cfg);
    let mut view = EditorView::new(&doc);
    let mut ed = editor(OperatingSystem::Linux);
    ed.layout(&doc, &mut view, size, 1.0);
    ed.focus(&doc, &mut view, true);
    ed.layout(&doc, &mut view, size, 1.0);
    Fixture { doc, view, ed }
}

impl Fixture {
    fn relayout(&mut self) {
        let size = self.ed.geom().size;
        self.ed.layout(&self.doc, &mut self.view, size, 1.0);
    }
    fn key(&mut self, k: Key, mods: Modifiers, text: Option<&str>) -> bool {
        let ev = KeyEvent {
            key: Some(k),
            physical: 0,
            text: text.map(str::to_owned),
            state: KeyState::Pressed,
            repeat: false,
            location: KeyLocation::Standard,
            modifiers: mods,
        };
        let r = self.ed.key(&mut self.doc, &mut self.view, &ev);
        self.relayout();
        r
    }
    fn press(&mut self, at: Point, t_ms: u64) {
        self.ed.set_now(Time(t_ms * 1_000_000));
        let p = pointer(at, 1, Some(MouseButton::Primary));
        self.ed.pointer_down(&mut self.doc, &mut self.view, &p);
        self.ed.pointer_up(
            &mut self.doc,
            &mut self.view,
            &pointer(at, 0, Some(MouseButton::Primary)),
        );
        self.relayout();
    }
    fn text(&self) -> String {
        self.doc.text().to_string()
    }
    fn head(&self) -> usize {
        self.view.selection().primary().head
    }
    /// The local point of the middle of line `line`, `x` points into the text.
    fn at_line(&self, line: usize, x: f32) -> Point {
        let g = self.ed.geom();
        Point::new(
            g.text_left() + x,
            skelter_quire::editor::PAD_Y + line as f32 * g.row_h + g.row_h * 0.5,
        )
    }
}

fn pointer(local: Point, buttons: u32, button: Option<MouseButton>) -> PointerInput {
    PointerInput {
        pointer: 0,
        kind: PointerType::Mouse,
        button,
        buttons,
        pos: local,
        local,
        modifiers: Modifiers::default(),
        pressure: 0.5,
        count: 1,
        history: Vec::new(),
    }
}

#[derive(Default)]
struct Runs(Vec<Arc<GlyphRun>>);

impl RunSink for Runs {
    fn add_run(&mut self, run: Arc<GlyphRun>) -> RunId {
        self.0.push(run);
        RunId(self.0.len() as u64)
    }
}

#[test]
fn only_the_viewport_and_overscan_are_laid_out() {
    let text = "line\n".repeat(10_000);
    let f = fixture(&text, Size::new(400.0, 200.0));
    let rows = (200.0 / f.ed.geom().row_h).ceil() as usize + 1;
    let laid = f.ed.laid_lines();
    assert_eq!(laid.first().map(|l| l.line), Some(0));
    assert!(
        laid.len() <= rows + f.ed.options.overscan_rows + 1,
        "{} lines laid out",
        laid.len()
    );
    assert!(laid.len() >= rows);
}

#[test]
fn typed_text_goes_through_the_core() {
    let mut f = fixture("", Size::new(400.0, 200.0));
    assert!(f.key(Key::A, Modifiers::default(), Some("a")));
    assert!(f.key(Key::B, Modifiers::SHIFT, Some("B")));
    assert_eq!(f.text(), "aB");
    let out = f.ed.take_output();
    assert_eq!(out.applied.len(), 2);
}

#[test]
fn ctrl_chords_do_not_type() {
    let mut f = fixture("x", Size::new(400.0, 200.0));
    f.key(Key::Q, Modifiers::CTRL, Some("q"));
    assert_eq!(f.text(), "x");
}

#[test]
fn save_is_a_host_event_and_cmd_is_primary_on_macos() {
    let doc = Document::new("x", EditorConfig::default());
    let mut view = EditorView::new(&doc);
    let mut ed = editor(OperatingSystem::Mac);
    ed.layout(&doc, &mut view, Size::new(400.0, 200.0), 1.0);
    let mut f = Fixture { doc, view, ed };
    f.key(Key::S, Modifiers::SUPER, Some("s"));
    let out = f.ed.take_output();
    assert!(
        out.events.contains(&EditorEvent::SaveRequested),
        "{:?}",
        out.events
    );
    assert_eq!(f.text(), "x", "Cmd+S does not type");
}

#[test]
fn copy_and_paste_go_through_the_host() {
    let mut f = fixture("hello world", Size::new(400.0, 200.0));
    f.key(Key::A, Modifiers::CTRL, None);
    f.key(Key::C, Modifiers::CTRL, None);
    f.key(Key::V, Modifiers::CTRL, None);
    let out = f.ed.take_output();
    assert!(
        out.events.contains(&EditorEvent::CopyText {
            text: "hello world".into()
        }),
        "{:?}",
        out.events
    );
    assert!(out.events.contains(&EditorEvent::PasteRequested));
    f.ed.paste(&mut f.doc, &mut f.view, "bye");
    assert_eq!(f.text(), "bye");
}

#[test]
fn a_click_places_the_caret_on_the_clicked_line() {
    let mut f = fixture("aaaa\nbbbb\ncccc", Size::new(400.0, 200.0));
    let p = f.at_line(1, 0.0);
    f.press(p, 1_000);
    assert_eq!(f.head(), 5, "the start of line 1");
    // Right of line 2's text but inside the widget: a press outside the widget's rect is not
    // the editor's (pointer_down returns false), so x stays within the text area.
    let g = f.ed.geom();
    let p = f.at_line(2, g.size.w - g.text_left() - 1.0);
    f.press(p, 3_000);
    assert_eq!(f.head(), 14, "past the end of line 2");
}

#[test]
fn a_double_click_selects_the_word() {
    let mut f = fixture("foo barbaz qux", Size::new(400.0, 200.0));
    let shaped = f.ed.laid_lines()[0].rows.shaped.clone();
    let x = shaped.x_of(6);
    let p = f.at_line(0, x);
    f.press(p, 1_000);
    f.press(p, 1_100);
    assert_eq!(f.view.selection().primary().range(), 4..10);
}

#[test]
fn wheel_scrolls_and_stays_in_range() {
    let text = "x\n".repeat(1_000);
    let mut f = fixture(&text, Size::new(400.0, 200.0));
    assert!(f.ed.wheel(
        &f.doc,
        &mut f.view,
        ScrollDelta::Pixels(Vec2::new(0.0, -50.0)),
        Modifiers::default()
    ));
    assert_eq!(f.ed.scroll().y, 50.0);
    assert!(
        !f.ed.wheel(
            &f.doc,
            &mut f.view,
            ScrollDelta::Pixels(Vec2::new(0.0, 1_000.0)),
            Modifiers::default()
        ) || f.ed.scroll().y == 0.0
    );
    assert_eq!(f.ed.scroll().y, 0.0, "clamped at the top");
}

#[test]
fn the_record_draws_text_and_line_numbers() {
    let mut f = fixture("fn main() {}\nlet x = 1;", Size::new(400.0, 200.0));
    let mut runs = Runs::default();
    let sc = f.ed.record(&f.doc, &mut f.view, &mut runs, None);
    let glyphs = |l: &skelter_base::display::DisplayList| {
        l.items
            .iter()
            .filter(|p| matches!(p, Prim::Glyphs(_)))
            .count()
    };
    assert!(glyphs(&sc.content) >= 2, "a run per line at least");
    assert!(glyphs(&sc.gutter) >= 2, "line numbers");
    assert!(
        sc.caret.is_some(),
        "focused: the primary caret is the compositor's"
    );
    assert!(!runs.0.is_empty());
}

#[test]
fn diagnostics_are_wavy_lines() {
    let mut f = fixture("let x = 1;", Size::new(400.0, 200.0));
    let v = f.doc.version();
    f.doc
        .set_diagnostics(
            SourceId::new("t"),
            v,
            vec![Diagnostic::new(4..5, Severity::Error, "bad")],
        )
        .unwrap();
    f.relayout();
    let sc = f.ed.record(&f.doc, &mut f.view, &mut Runs::default(), None);
    assert!(
        sc.content
            .items
            .iter()
            .any(|p| matches!(p, Prim::Line(l) if l.wavy))
    );
}

#[test]
fn soft_wrap_down_moves_by_row() {
    let cfg = EditorConfig {
        word_wrap: WordWrap::On,
        ..EditorConfig::default()
    };
    let long = "word ".repeat(60);
    let mut f = fixture_with(&format!("{long}\nnext"), Size::new(200.0, 200.0), cfg);
    let rows = f.ed.laid_lines()[0].rows.n_rows();
    assert!(rows > 2, "the long line wraps ({rows} rows)");
    f.key(Key::ArrowDown, Modifiers::default(), None);
    let t = f.doc.text();
    assert_eq!(
        t.byte_to_line(f.head()),
        0,
        "still on the first line, its second row"
    );
    assert!(f.head() > 0);
}

#[test]
fn the_caret_blinks_and_stops_when_idle() {
    let f = fixture("x", Size::new(400.0, 200.0));
    let t0 = f.ed.last_interaction_time();
    let on = f.ed.theme.blink_on;
    assert!(f.ed.caret_visible(t0));
    assert!(!f.ed.caret_visible(t0 + on + Span::from_millis(1)));
    assert_eq!(f.ed.next_blink(t0), Some(t0 + on));
    let idle = t0 + f.ed.theme.blink_idle_stop;
    assert!(f.ed.caret_visible(idle));
    assert_eq!(f.ed.next_blink(idle), None);
}

#[test]
fn ime_preedit_is_injected_and_committed() {
    let mut f = fixture("ab", Size::new(400.0, 200.0));
    f.key(Key::ArrowRight, Modifiers::default(), None);
    f.ed.ime(
        &mut f.doc,
        &mut f.view,
        &ImeEvent::Preedit {
            text: "かな".into(),
            cursor: Some((6, 6)),
        },
    );
    f.relayout();
    assert_eq!(f.ed.laid_lines()[0].display.text, "aかなb");
    assert_eq!(f.text(), "ab", "the preedit is not in the document");
    f.ed.ime(&mut f.doc, &mut f.view, &ImeEvent::Commit("仮名".into()));
    assert_eq!(f.text(), "a仮名b");
}

#[test]
fn the_text_input_client_counts_utf16_over_the_virtual_text() {
    let mut f = fixture("😀x", Size::new(400.0, 200.0));
    f.key(Key::End, Modifiers::default(), None);
    let mut client = ImeClient::new(ImeTarget {
        editor: &mut f.ed,
        doc: &mut f.doc,
        view: &mut f.view,
        to_window: Default::default(),
    });
    assert_eq!(client.selected_range(), Some(3..3), "😀 is two code units");
    client.replace_and_mark_text_in_range(None, "ab", Some(1..1));
    assert_eq!(client.marked_range(), Some(3..5));
    assert_eq!(client.text_for_range(0..5).as_deref(), Some("😀xab"));
    assert_eq!(
        client.selected_range(),
        Some(4..4),
        "the composition's cursor"
    );
    client.unmark_text();
    assert_eq!(client.marked_range(), None);
    assert_eq!(f.text(), "😀xab");
}

fn layer_ids() -> EditorLayerIds {
    EditorLayerIds {
        window: WindowId(1),
        parent_layer: None,
        parent_space: None,
        background: LayerId(10),
        content: LayerId(11),
        gutter: LayerId(12),
        overlay: LayerId(13),
        box_space: PropNodeId(20),
        text_clip: PropNodeId(21),
        gutter_clip: PropNodeId(22),
        content_scroll: PropNodeId(23),
        content_space: PropNodeId(24),
        gutter_scroll: PropNodeId(25),
        gutter_space: PropNodeId(26),
        background_node: NodeId(30),
        content_node: NodeId(31),
        gutter_node: NodeId(32),
        overlay_node: NodeId(33),
    }
}

#[test]
fn layers_scroll_by_offset_inside_the_overscan() {
    let text = "x\n".repeat(1_000);
    let mut f = fixture(&text, Size::new(400.0, 200.0));
    let ids = layer_ids();
    let mut layers = EditorLayers::new(ids, 1 << 48);
    let c1 = layers.commit(
        &mut f.ed,
        &f.doc,
        &mut f.view,
        Default::default(),
        Revision(1),
        InputSeq(1),
    );
    assert_eq!(c1.lists.len(), 4);
    assert!(
        c1.layers
            .iter()
            .any(|l| matches!(l, skelter_base::layer::LayerDelta::Add(_)))
    );
    f.ed.wheel(
        &f.doc,
        &mut f.view,
        ScrollDelta::Pixels(Vec2::new(0.0, -40.0)),
        Modifiers::default(),
    );
    f.relayout();
    let c2 = layers.commit(
        &mut f.ed,
        &f.doc,
        &mut f.view,
        Default::default(),
        Revision(2),
        InputSeq(2),
    );
    assert_eq!(c2.lists.len(), 1, "only the overlay re-records");
    assert!(c2.props.ops.iter().any(|o| matches!(o, skelter_base::props::PropOp::Scroll(n, s) if *n == ids.content_scroll && s.offset.y == 40.0)));
    let fast = layers.scroll_commits(&f.ed, Revision(2), InputSeq(2));
    assert!(
        matches!(fast[1], skelter_base::commit::FastCommit::ScrollOffset { offset, .. } if offset == Vec2::new(0.0, 40.0))
    );
    // far away: the recorded lines no longer cover the viewport
    f.ed.wheel(
        &f.doc,
        &mut f.view,
        ScrollDelta::Pixels(Vec2::new(0.0, -5_000.0)),
        Modifiers::default(),
    );
    f.relayout();
    let c3 = layers.commit(
        &mut f.ed,
        &f.doc,
        &mut f.view,
        Default::default(),
        Revision(3),
        InputSeq(3),
    );
    assert_eq!(c3.lists.len(), 4);
    assert!(!c3.dropped_runs.is_empty(), "the old record's runs go");
}

#[test]
fn layers_caret_commit_carries_the_caret_colour_and_box_space() {
    let mut f = fixture("hello", Size::new(400.0, 200.0));
    let red = Color::from_rgba8(255, 0, 0, 255);
    f.ed.theme.caret = red;
    let ids = layer_ids();
    let mut layers = EditorLayers::new(ids, 1 << 48);
    let at = Affine::translate(5.0, 7.0);
    let r = Rect::new(10.0, 0.0, 12.0, 18.0);
    let caret = |c: FastCommit| match c {
        FastCommit::Caret {
            rect,
            color,
            transform,
            ..
        } => (rect, color, transform),
        other => panic!("not a caret: {other:?}"),
    };
    // before the layers are committed the compositor doesn't know the box node: window space
    let (before, color, transform) = caret(layers.caret_commit(Some(r), &f.ed, at, Revision(1)));
    assert_eq!(color, red, "the editor's caret colour");
    assert_eq!(transform, PropNodeId::default(), "window space");
    let before = before.expect("shown");
    layers.commit(&mut f.ed, &f.doc, &mut f.view, at, Revision(2), InputSeq(2));
    // then in the editor's box space, without the box offset
    let (after, color, transform) = caret(layers.caret_commit(Some(r), &f.ed, at, Revision(3)));
    assert_eq!(color, red);
    assert_eq!(transform, ids.box_space, "the editor's own transform node");
    let after = after.expect("shown");
    assert_eq!(before, after.translate(Vec2::new(5.0, 7.0)));
    let (hidden, color, _) = caret(layers.caret_commit(None, &f.ed, at, Revision(4)));
    assert_eq!(hidden, None);
    assert_eq!(color, red);
}

/// The last drawn line's glyph box and the horizontal scrollbar's thumb, in local space.
fn last_line_and_hbar(f: &mut Fixture) -> (Rect, Rect) {
    let mut runs = Runs::default();
    let sc = f.ed.record(&f.doc, &mut f.view, &mut runs, None);
    let g = *f.ed.geom();
    let dy = sc.text_origin.y - sc.scroll.y;
    let base = sc
        .content
        .items
        .iter()
        .filter_map(|p| match p {
            Prim::Glyphs(gl) => Some(gl.origin.y + dy),
            _ => None,
        })
        .fold(f32::MIN, f32::max);
    let glyphs = Rect::new(0.0, base - g.ascent, g.size.w, base + g.descent);
    let bar = sc
        .overlay
        .items
        .iter()
        .find_map(|p| match p {
            Prim::Quad(q) if q.rect.min.y >= g.size.h - skelter_quire::editor::SCROLLBAR_W => {
                Some(q.rect)
            }
            _ => None,
        })
        .expect("the horizontal scrollbar is drawn");
    (glyphs, bar)
}

/// VS Code's layout (Q-P10): with no wrap and content wider than the view, the horizontal
/// scrollbar overlays the bottom of the view; scrolled to the end, the last line is clear of it.
#[test]
fn the_last_line_scrolls_clear_of_the_horizontal_scrollbar() {
    let line = "abcdefghij".repeat(20);
    let text = vec![line.as_str(); 400].join("\n");
    let size = Size::new(400.0, 200.0);
    let last = 399;

    // the wheel to the end (scrollBeyondLastLine: the last line goes to the top)
    let mut f = fixture(&text, size);
    f.ed.wheel(
        &f.doc,
        &mut f.view,
        ScrollDelta::Pixels(Vec2::new(0.0, -1.0e6)),
        Modifiers::default(),
    );
    f.relayout();
    assert!(
        f.ed.visible_lines().contains(&last),
        "{:?}",
        f.ed.visible_lines()
    );
    let (glyphs, bar) = last_line_and_hbar(&mut f);
    assert!(glyphs.max.y <= bar.min.y, "wheel: {glyphs:?} under {bar:?}");

    // Ctrl+End: the caret's line is revealed at the bottom of the view, above the bar
    let mut f = fixture(&text, size);
    f.key(Key::End, Modifiers::CTRL, None);
    assert_eq!(f.head(), f.doc.text().len_bytes());
    let (glyphs, bar) = last_line_and_hbar(&mut f);
    assert!(
        glyphs.max.y <= bar.min.y,
        "ctrl+end: {glyphs:?} under {bar:?}"
    );
    assert!(
        glyphs.max.y > bar.min.y - f.ed.geom().row_h,
        "a minimal reveal: at the bottom"
    );

    // arrow down from the top to the last line: each step reveals minimally
    let mut f = fixture(&text, size);
    for _ in 0..last {
        f.key(Key::ArrowDown, Modifiers::default(), None);
    }
    let (glyphs, bar) = last_line_and_hbar(&mut f);
    assert!(
        glyphs.max.y <= bar.min.y,
        "arrow down: {glyphs:?} under {bar:?}"
    );
}
