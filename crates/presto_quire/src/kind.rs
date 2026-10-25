//! `code-editor`: the editor as a presto-core [`Kind`], for svelters' `<code-editor>` element.
//!
//! The host registers [`CodeEditorKind`] and lends the kinds a [`QuireHost`] service: the font store,
//! the theme, the documents by name (a node shows the one its `buffer` prop names; several nodes
//! may show one document) and the outbox of [`EditorEvent`]s the host drains each frame.
//!
//! A kind records one display list per node, so here the editor's four lists are composed into
//! it: the content under a clip, translated by the scroll offset, the gutter by its y offset, the
//! primary caret drawn in its blink phase (wake-ups at each blink transition). Scrolling therefore
//! re-records this node (a paint change) instead of moving a compositor layer; [`crate::layers`]
//! is the layer path for hosts that own layers, and moving `code-editor` onto it needs a presto-core
//! hook for kinds that own layers (docs/PRESTO-QUIRE.md, question 1).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use presto_base::change::ChangeClass;
use presto_base::geom::{Affine, Point, Rect, RoundedRect, Vec2};
use presto_base::ids::{NodeId, RunId};
use presto_base::input::{InputPurpose, TextInputClient, Utf16Range};
use presto_base::text::GlyphRun;
use presto_base::time::Time;
use presto_core::{
    A11yCx, Event, EventCx, FocusReason, Handled, Kind, KindSpec, MeasureCx, PrepareCx, PropCx,
    PropError, PropId, PropSpec, RecordCx, SignalFlags, SignalSpec, Value, ValueType,
};
use quire_core::{Document, EditorView};

use crate::editor::{EditorEvent, EditorOptions, PrestoEditor};
use crate::ime::ImeTarget;
use crate::layers::translate_prim;
use crate::scene::RunSink;
use crate::theme::EditorTheme;

/// What `code-editor` nodes share, lent through `Document::set_service`.
pub struct QuireHost {
    pub fonts: Arc<presto_text::FontStore>,
    pub theme: EditorTheme,
    /// Options new editors start with.
    pub options: EditorOptions,
    /// Documents by the name a node's `buffer` prop gives.
    pub buffers: HashMap<String, Rc<RefCell<Document>>>,
    /// Events of every editor, with the node that produced them; the host drains it.
    pub events: Vec<(NodeId, EditorEvent)>,
    /// Device pixels per point of the windows the editors are in.
    pub scale: f32,
}

impl std::fmt::Debug for QuireHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuireHost")
            .field("buffers", &self.buffers.keys().collect::<Vec<_>>())
            .field("events", &self.events.len())
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl QuireHost {
    pub fn new(fonts: Arc<presto_text::FontStore>) -> Self {
        QuireHost {
            fonts,
            theme: EditorTheme::default(),
            options: EditorOptions::default(),
            buffers: HashMap::new(),
            events: Vec::new(),
            scale: 1.0,
        }
    }

    /// Adds (or replaces) the document named `name`.
    pub fn insert_buffer(
        &mut self,
        name: impl Into<String>,
        doc: Document,
    ) -> Rc<RefCell<Document>> {
        let d = Rc::new(RefCell::new(doc));
        self.buffers.insert(name.into(), d.clone());
        d
    }
}

pub const PROPS: &[PropSpec] = &[
    PropSpec::new("buffer", ValueType::Str, ChangeClass::Paint)
        .doc("The name of the document (in the host's QuireHost) this editor shows."),
    PropSpec::new("readonly", ValueType::Bool, ChangeClass::Paint)
        .doc("Refuse edits (selection and find still work)."),
    PropSpec::new("gutter", ValueType::Bool, ChangeClass::Layout)
        .default(presto_core::PropDefault::Bool(true))
        .doc("Show line numbers, fold markers and diagnostic marks."),
];

pub const SIGNALS: &[SignalSpec] = &[
    SignalSpec::new("change", None, SignalFlags::PASSIVE)
        .doc("The document's text changed through this editor."),
    SignalSpec::new("selectionchange", None, SignalFlags::PASSIVE)
        .doc("The selection changed through this editor."),
];

pub static SPEC: KindSpec = KindSpec {
    props: PROPS,
    signals: SIGNALS,
    role: presto_base::accesskit::Role::MultilineTextInput,
    focusable: true,
    doc: "The quire code editor (presto_quire).",
    ..KindSpec::new("code-editor")
};

fn prop(name: &str) -> PropId {
    SPEC.prop_named(name).expect("code-editor prop")
}

struct Inner {
    doc: Rc<RefCell<Document>>,
    view: EditorView,
    editor: PrestoEditor,
    /// Local → window.
    to_window: Affine,
    now: Time,
}

/// The `code-editor` kind.
#[derive(Default)]
pub struct CodeEditorKind {
    inner: RefCell<Option<Inner>>,
    /// The `buffer` the inner state was built for.
    buffer: Option<String>,
    read_only: bool,
    gutter: bool,
}

impl std::fmt::Debug for CodeEditorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeEditorKind")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

/// [`RunSink`] over a node's [`RecordCx`].
struct CxSink<'c, 'a>(&'c mut RecordCx<'a>);

impl RunSink for CxSink<'_, '_> {
    fn add_run(&mut self, run: Arc<GlyphRun>) -> RunId {
        self.0.add_run(run)
    }
}

impl CodeEditorKind {
    /// Builds the editor state once the host's service and the named buffer are there.
    fn ensure(&self, host: Option<&mut QuireHost>) {
        let mut slot = self.inner.borrow_mut();
        if slot.is_some() {
            return;
        }
        let (Some(host), Some(name)) = (host, self.buffer.as_deref()) else {
            return;
        };
        let Some(doc) = host.buffers.get(name).cloned() else {
            return;
        };
        let mut options = host.options.clone();
        options.read_only |= self.read_only;
        options.gutter = self.gutter;
        let editor = PrestoEditor::new(host.fonts.clone(), host.theme.clone(), options);
        let view = EditorView::new(&doc.borrow());
        *slot = Some(Inner {
            doc,
            view,
            editor,
            to_window: Affine::IDENTITY,
            now: Time::ZERO,
        });
    }

    /// Hands the editor's output to the host and JS: events to the outbox, `change` and
    /// `selectionchange` to listeners.
    fn flush(&self, cx: &mut EventCx) {
        let node = cx.node();
        let out = match self.inner.borrow_mut().as_mut() {
            Some(i) => i.editor.take_output(),
            None => return,
        };
        let changed = out.applied.iter().any(|a| a.changed_text());
        let selected = !out.applied.is_empty();
        if let Some(host) = cx.service::<QuireHost>() {
            host.events
                .extend(out.events.into_iter().map(|e| (node, e)));
        }
        if changed && let Some(s) = SPEC.signal_named("change") {
            cx.emit(s, None);
        }
        if selected && let Some(s) = SPEC.signal_named("selectionchange") {
            cx.emit(s, None);
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        self.inner.borrow_mut().as_mut().map(f)
    }

    fn ime<R>(&self, f: impl FnOnce(&mut ImeTarget) -> R) -> Option<R> {
        let mut slot = self.inner.borrow_mut();
        let i = slot.as_mut()?;
        let mut doc = i.doc.borrow_mut();
        let mut t = ImeTarget {
            editor: &mut i.editor,
            doc: &mut doc,
            view: &mut i.view,
            to_window: i.to_window,
        };
        Some(f(&mut t))
    }
}

impl Kind for CodeEditorKind {
    fn spec() -> &'static KindSpec {
        &SPEC
    }

    fn prop_changed(
        &mut self,
        cx: &mut PropCx,
        p: PropId,
        value: Option<&Value>,
    ) -> Result<(), PropError> {
        if p == prop("buffer") {
            let name = value.and_then(Value::as_str).map(str::to_owned);
            if name != self.buffer {
                self.buffer = name;
                *self.inner.get_mut() = None;
            }
        } else if p == prop("readonly") {
            self.read_only = value.and_then(Value::as_bool).unwrap_or(false);
            if let Some(i) = self.inner.get_mut() {
                i.editor.options.read_only = self.read_only;
            }
        } else if p == prop("gutter") {
            self.gutter = value.and_then(Value::as_bool).unwrap_or(true);
            if let Some(i) = self.inner.get_mut() {
                i.editor.options.gutter = self.gutter;
            }
        }
        cx.request_paint();
        Ok(())
    }

    fn measure(
        &self,
        _cx: &mut MeasureCx,
        known: presto_core::taffy::Size<Option<f32>>,
        _avail: presto_core::taffy::Size<presto_core::taffy::AvailableSpace>,
    ) -> presto_core::taffy::Size<f32> {
        // the editor fills what CSS gives it; alone it has no intrinsic size
        presto_core::taffy::Size {
            width: known.width.unwrap_or(0.0),
            height: known.height.unwrap_or(0.0),
        }
    }

    fn prepare(&mut self, cx: &mut PrepareCx) {
        if self.buffer.is_none() {
            self.buffer = cx.prop_str(prop("buffer")).map(str::to_owned);
            self.gutter = cx
                .prop(prop("gutter"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            self.read_only = cx.prop_bool(prop("readonly"));
        }
        let size = cx.size();
        let scale = cx.service::<QuireHost>().map_or(1.0, |h| h.scale);
        self.ensure(cx.service::<QuireHost>());
        self.with(|i| {
            let doc = i.doc.borrow();
            i.editor.layout(&doc, &mut i.view, size, scale);
        });
    }

    fn record(&self, cx: &mut RecordCx) {
        let mut slot = self.inner.borrow_mut();
        let Some(i) = slot.as_mut() else { return };
        let doc = i.doc.borrow();
        let sc = i.editor.record(&doc, &mut i.view, &mut CxSink(cx), None);
        let caret_on = i.editor.caret_visible(i.now);
        let put = |cx: &mut RecordCx, list: &presto_base::display::DisplayList, d: Vec2| {
            for p in &list.items {
                let (q, ink) = translate_prim(p, d);
                cx.push(q, ink);
            }
        };
        put(cx, &sc.background, Vec2::ZERO);
        let content_d = Vec2::new(
            sc.text_origin.x - sc.scroll.x,
            sc.text_origin.y - sc.scroll.y,
        );
        cx.push_clip(RoundedRect {
            rect: sc.text_clip,
            radii: Default::default(),
        });
        put(cx, &sc.content, content_d);
        if caret_on && let Some((p, ink)) = sc.caret_prim() {
            let (q, ink2) = translate_prim(&p, content_d);
            let _ = ink;
            cx.push(q, ink2);
        }
        cx.pop_clip();
        if sc.gutter_width > 0.0 {
            cx.push_clip(RoundedRect {
                rect: Rect::new(0.0, 0.0, sc.gutter_width, sc.text_clip.max.y),
                radii: Default::default(),
            });
            put(
                cx,
                &sc.gutter,
                Vec2::new(0.0, sc.text_origin.y - sc.scroll.y),
            );
            cx.pop_clip();
        }
        put(cx, &sc.overlay, Vec2::ZERO);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &Event) -> Handled {
        self.ensure(cx.service::<QuireHost>());
        let node = cx.node();
        let now = cx.time();
        let origin = cx
            .document()
            .world_rect(node)
            .map_or(Point::ZERO, |r| r.min);
        let handled = self
            .with(|i| {
                i.now = now;
                i.to_window = Affine::translate(origin.x, origin.y);
                i.editor.set_now(now);
                let mut doc = i.doc.borrow_mut();
                let (e, view) = (&mut i.editor, &mut i.view);
                match ev {
                    Event::PointerDown(p) => {
                        e.focus(&doc, view, true);
                        e.pointer_down(&mut doc, view, p)
                    }
                    Event::PointerMove(p) => e.pointer_move(&mut doc, view, p),
                    Event::PointerUp(p) => e.pointer_up(&mut doc, view, p),
                    Event::PointerCancel => e.pointer_up(&mut doc, view, &cancel_input()),
                    Event::HoverLeave => {
                        e.pointer_leave(&doc, view);
                        false
                    }
                    Event::Wheel(w) => e.wheel(&doc, view, w.delta, w.modifiers),
                    Event::Key(k) => e.key(&mut doc, view, k),
                    Event::Ime(m) => {
                        e.ime(&mut doc, view, m);
                        true
                    }
                    Event::FocusIn(_) => {
                        e.focus(&doc, view, true);
                        false
                    }
                    Event::FocusOut => {
                        e.focus(&doc, view, false);
                        false
                    }
                    Event::Timer => {
                        e.tick(&mut doc, view, now);
                        true
                    }
                    Event::A11y(req) => a11y_action(e, &mut doc, view, req),
                    Event::Click(_) | Event::HoverEnter => false,
                }
            })
            .unwrap_or(false);
        if matches!(ev, Event::PointerDown(_)) {
            cx.focus(FocusReason::Pointer);
            cx.capture_pointer();
        }
        if matches!(ev, Event::PointerUp(_) | Event::PointerCancel) {
            cx.release_pointer();
        }
        if matches!(ev, Event::PointerMove(_) | Event::HoverEnter) {
            cx.set_cursor(presto_base::cursor::CursorIcon::Text);
        }
        // the next wake-up: hover and hint deadlines, the caret's next blink
        let next = self.with(|i| {
            let mut doc = i.doc.borrow_mut();
            i.editor.tick(&mut doc, &mut i.view, now)
        });
        match next.flatten() {
            Some(t) => cx.wake_at(t),
            None => cx.cancel_wake(),
        }
        self.flush(cx);
        cx.request_paint();
        if handled { Handled::Yes } else { Handled::No }
    }

    fn text_input(&mut self) -> Option<&mut dyn TextInputClient> {
        self.inner.get_mut().as_ref()?;
        Some(self)
    }

    fn a11y(&self, cx: &mut A11yCx) {
        cx.default_a11y();
        let slot = self.inner.borrow();
        let Some(i) = slot.as_ref() else { return };
        let doc = i.doc.borrow();
        let t = doc.text();
        // the whole value for documents up to a page of text; larger ones expose their name only
        // (the TextRun projection egui_quire has needs child a11y nodes: docs/PRESTO-QUIRE.md)
        if t.len_bytes() <= A11Y_VALUE_MAX {
            cx.out.set_value(t.to_string());
        }
        if i.editor.options.read_only {
            cx.out.set_read_only();
        }
    }
}

/// Documents up to this size expose their text as the accessible value.
pub const A11Y_VALUE_MAX: usize = 64 * 1024;

fn cancel_input() -> presto_core::PointerInput {
    presto_core::PointerInput {
        pointer: 0,
        kind: presto_base::event::PointerType::Mouse,
        button: None,
        buttons: 0,
        pos: Point::ZERO,
        local: Point::ZERO,
        modifiers: Default::default(),
        pressure: 0.0,
        count: 0,
    }
}

/// `SetValue` replaces the text; `ReplaceSelectedText` the selection (egui_quire's
/// `handle_accesskit_actions`).
fn a11y_action(
    e: &mut PrestoEditor,
    doc: &mut Document,
    view: &mut EditorView,
    req: &presto_base::accesskit::ActionRequest,
) -> bool {
    use presto_base::accesskit::{Action, ActionData};
    if e.options.read_only {
        return false;
    }
    let Some(ActionData::Value(v)) = &req.data else {
        return false;
    };
    let len = doc.text().len_bytes();
    let range = match req.action {
        Action::SetValue => 0..len,
        Action::ReplaceSelectedText => view.selection().primary().range(),
        _ => return false,
    };
    let author = e.options.author;
    if let Some(a) = crate::editor::replace_range(doc, view, range, v, false, author) {
        e.note_applied(a);
    }
    true
}

impl TextInputClient for CodeEditorKind {
    fn text_for_range(&self, range: Utf16Range) -> Option<String> {
        self.ime(|t| t.text_for_range(range)).flatten()
    }
    fn selected_range(&self) -> Option<Utf16Range> {
        self.ime(|t| t.selected_range()).flatten()
    }
    fn marked_range(&self) -> Option<Utf16Range> {
        self.ime(|t| t.marked_range()).flatten()
    }
    fn replace_text_in_range(&mut self, range: Option<Utf16Range>, text: &str) {
        self.ime(|t| t.replace_text_in_range(range, text));
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Utf16Range>,
        text: &str,
        new_selected: Option<Utf16Range>,
    ) {
        self.ime(|t| t.replace_and_mark_text_in_range(range, text, new_selected));
    }
    fn unmark_text(&mut self) {
        self.ime(|t| t.unmark_text());
    }
    fn bounds_for_range(&self, range: Utf16Range) -> Option<Rect> {
        self.ime(|t| t.bounds_for_range(range)).flatten()
    }
    fn character_index_for_point(&self, p: Point) -> Option<usize> {
        self.ime(|t| t.character_index_for_point(p)).flatten()
    }
    fn surrounding_text(&self, max_bytes: usize) -> Option<(String, std::ops::Range<usize>)> {
        self.ime(|t| t.surrounding_text(max_bytes)).flatten()
    }
    fn purpose(&self) -> InputPurpose {
        InputPurpose::Code
    }
}
