//! A headless egui rig for the E1 and E3 benches: one `Context`, one `Document`, one
//! `EditorView`, frames driven by synthetic `RawInput`, the editor's own time and allocations
//! measured around `CodeEditor::show_in`.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use egui::{Context, Event, Modifiers, MouseWheelUnit, Pos2, RawInput, Rect, pos2, vec2};
use egui_quire::CodeEditor;
use quire_core::{Document, EditorCommand, EditorConfig, EditorView, Selection};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

/// The bench binary's global allocator (counts every allocation; wraps `System`).
pub type Counting = StatsAlloc<std::alloc::System>;
pub static COUNTING: &Counting = &INSTRUMENTED_SYSTEM;

/// The window size (points).
pub const SCREEN_W: f32 = 1280.0;
pub const SCREEN_H: f32 = 800.0;

/// What one frame cost.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// `CodeEditor::show_in` only.
    pub editor: Duration,
    /// The whole `Context::run_ui` (egui's pass bookkeeping plus the editor).
    pub pass: Duration,
    /// Allocations made inside `show_in`.
    pub editor_allocs: usize,
    /// Bytes allocated inside `show_in`.
    pub editor_bytes: usize,
    /// Allocations made by the whole pass.
    pub pass_allocs: usize,
    /// Lines the editor laid out and painted.
    pub visible: usize,
}

pub struct Rig {
    pub ctx: Context,
    pub doc: Document,
    pub view: EditorView,
    pub time: f64,
    pub salt: &'static str,
}

/// A frame's synthetic input on top of the time and screen.
pub fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(SCREEN_W, SCREEN_H))),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        focused: true,
        events,
        ..Default::default()
    }
}

/// Where the pointer rests (inside the text area).
pub const POINTER: Pos2 = pos2(640.0, 400.0);

pub fn wheel_lines(lines: f32) -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, -lines),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    }
}

impl Rig {
    /// A warmed context (fonts loaded) with no document shown yet.
    pub fn warm_context() -> Context {
        let ctx = Context::default();
        // egui smooths wheel input over a few frames; with one wheel event per frame the scroll
        // speed settles at the wheel delta per frame.
        let _ = ctx.run_ui(input(0.0, vec![]), |ui| {
            ui.monospace("warm up the monospace font");
        });
        ctx
    }

    pub fn new(ctx: Context, text: &str) -> Self {
        Self::with_config(ctx, text, EditorConfig::default())
    }

    pub fn with_config(ctx: Context, text: &str, config: EditorConfig) -> Self {
        let doc = Document::new(text, config);
        let view = EditorView::new(&doc);
        Self {
            ctx,
            doc,
            view,
            time: 1.0,
            salt: "bench-editor",
        }
    }

    /// Runs one frame with `events` and measures it.
    pub fn frame(&mut self, events: Vec<Event>) -> Frame {
        self.time += 1.0 / 60.0;
        let mut f = Frame::default();
        let (doc, view, salt) = (&mut self.doc, &mut self.view, self.salt);
        let pass_region = Region::new(COUNTING);
        let t_pass = Instant::now();
        let out = self.ctx.run_ui(input(self.time, events), |ui| {
            let rect = ui.max_rect();
            let region = Region::new(COUNTING);
            let t = Instant::now();
            let out = CodeEditor::new(doc, view, salt, "Bench").show_in(ui, rect);
            f.editor = t.elapsed();
            let st = region.change();
            f.editor_allocs = st.allocations + st.reallocations;
            f.editor_bytes = st.bytes_allocated;
            f.visible = out.visible_lines.len();
        });
        f.pass = t_pass.elapsed();
        let st = pass_region.change();
        f.pass_allocs = st.allocations + st.reallocations;
        // The host would tessellate and render this; the bench drops it.
        drop(out);
        f
    }

    /// Clicks into the editor so it has focus (two frames).
    pub fn focus(&mut self) {
        let press = |pressed| Event::PointerButton {
            pos: POINTER,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(POINTER), press(true)]);
        self.frame(vec![press(false)]);
        self.frame(vec![]);
    }

    /// Puts the caret at `pos` and reveals it (through commands, as a host would).
    pub fn caret_to(&mut self, pos: usize) {
        let line = self.doc.text().byte_to_line(pos);
        self.doc
            .execute(
                &mut self.view,
                &EditorCommand::SetSelection {
                    selection: Selection::point(pos),
                },
                None,
            )
            .unwrap();
        self.doc
            .execute(&mut self.view, &EditorCommand::RevealLine { line }, None)
            .unwrap();
        self.frame(vec![]);
        self.frame(vec![]);
    }

    /// Scrolls so that `line` is the first visible line.
    pub fn scroll_to_line(&mut self, line: usize) {
        let mut s = self.view.scroll();
        s.anchor = self.doc.text().line_to_byte(line);
        s.offset = 0.0;
        self.view.set_scroll(s);
    }
}

/// p50, p99 and max of a sample (sorted in place).
pub fn percentiles(xs: &mut [Duration]) -> (Duration, Duration, Duration) {
    xs.sort();
    let at = |q: f64| xs[((xs.len() - 1) as f64 * q).round() as usize];
    (at(0.5), at(0.99), *xs.last().unwrap())
}

pub fn ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1e3)
}
