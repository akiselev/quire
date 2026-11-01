//! `code-editor`: the editor as a skelter-core [`Kind`], for svelters' `<code-editor>` element.
//!
//! The host registers [`CodeEditorKind`] and lends the kinds a [`QuireHost`] service: the font store,
//! the theme, the documents by name (a node shows the one its `buffer` prop names; several nodes
//! may show one document) and the outbox of [`EditorEvent`]s the host drains each frame.
//!
//! A kind records one display list per node, so here the editor's four lists are composed into
//! it: the content under a clip, translated by the scroll offset, the gutter by its y offset, the
//! primary caret drawn in its blink phase (wake-ups at each blink transition). Scrolling therefore
//! re-records this node (a paint change) instead of moving a compositor layer; [`crate::layers`]
//! is the layer path for hosts that own layers, and moving `code-editor` onto it needs a skelter-core
//! hook for kinds that own layers (docs/SKELTER-QUIRE.md, question 1).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use quire_core::{Document, EditorView};
use skelter_base::change::ChangeClass;
use skelter_base::geom::{Affine, Point};
use skelter_base::ids::NodeId;
use skelter_base::input::TextInputClient;
use skelter_base::time::Time;
use skelter_core::{
    A11yCx, Event, EventCx, Handled, Kind, KindId, KindRegistry, KindSpec, MeasureCx, NodeView,
    PrepareCx, PropCx, PropError, PropId, PropSpec, RecordCx, RegistryError, SignalFlags,
    SignalSpec, Value, ValueType,
};

use crate::editor::{EditorEvent, EditorOptions, SkelterEditor};
use crate::ime::ImeTarget;
use crate::kind_support::{ImeHost, input_effects, record_scene, route_event, schedule_wake};
use crate::theme::EditorTheme;

/// What `code-editor` nodes share, lent through `Document::set_service`.
pub struct QuireHost {
    pub fonts: Arc<skelter_text::FontStore>,
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
    pub fn new(fonts: Arc<skelter_text::FontStore>) -> Self {
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
        .default(skelter_core::PropDefault::Bool(true))
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
    role: skelter_base::accesskit::Role::MultilineTextInput,
    focusable: true,
    doc: "The quire code editor (skelter_quire).",
    ..KindSpec::new("code-editor")
};

// Props and signals by their index in [`PROPS`] and [`SIGNALS`]. A node resolves them through the
// registry entry it was created from, so one kind serves every tag it is registered under
// ([`CodeEditorKind::register_as`]).
const BUFFER: usize = 0;
const READONLY: usize = 1;
const GUTTER: usize = 2;
const CHANGE: usize = 0;
const SELECTIONCHANGE: usize = 1;

/// The id of prop `ix` in the spec the node was registered with.
fn prop(view: &NodeView, ix: usize) -> PropId {
    view.kind().prop_ids()[ix]
}

struct Inner {
    doc: Rc<RefCell<Document>>,
    view: EditorView,
    editor: SkelterEditor,
    /// Local → window.
    to_window: Affine,
    now: Time,
}

/// The `code-editor` kind.
pub struct CodeEditorKind {
    inner: RefCell<Option<Inner>>,
    /// The `buffer` the inner state was built for.
    buffer: Option<String>,
    read_only: bool,
    gutter: bool,
}

impl Default for CodeEditorKind {
    fn default() -> Self {
        CodeEditorKind {
            inner: RefCell::new(None),
            buffer: None,
            read_only: false,
            gutter: true,
        }
    }
}

impl std::fmt::Debug for CodeEditorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeEditorKind")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

impl CodeEditorKind {
    /// Registers the kind under its own tag, `code-editor` (as `reg.register::<CodeEditorKind>()`).
    pub fn register(reg: &mut KindRegistry) -> Result<KindId, RegistryError> {
        reg.register::<CodeEditorKind>()
    }

    /// Registers the kind under the tag `name` (a gallery's `gallery-source`, or `code-editor`
    /// beside a host's own `code-editor` kind): a copy of [`SPEC`] with that name, so the props and
    /// signals are the same and their ids are the new tag's. A node resolves its props through the
    /// spec it was created with, so every registration takes its own props. Each call keeps one
    /// `KindSpec` for the life of the process (the registry holds `&'static` specs); a registry
    /// is built once per host, so this is a few hundred bytes per tag.
    pub fn register_as(
        reg: &mut KindRegistry,
        name: &'static str,
    ) -> Result<KindId, RegistryError> {
        if name == SPEC.name {
            return Self::register(reg);
        }
        let spec: &'static KindSpec = Box::leak(Box::new(KindSpec { name, ..SPEC }));
        reg.register_with(spec, || Box::<CodeEditorKind>::default())
    }

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
        let editor = SkelterEditor::new(host.fonts.clone(), host.theme.clone(), options);
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
        let signals = cx
            .view(node)
            .map(|v| v.kind().signal_ids().to_vec())
            .unwrap_or_default();
        if changed && let Some(&s) = signals.get(CHANGE) {
            cx.emit(s, None);
        }
        if selected && let Some(&s) = signals.get(SELECTIONCHANGE) {
            cx.emit(s, None);
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        self.inner.borrow_mut().as_mut().map(f)
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
        if p == prop(cx, BUFFER) {
            let name = value.and_then(Value::as_str).map(str::to_owned);
            if name != self.buffer {
                self.buffer = name;
                *self.inner.get_mut() = None;
            }
        } else if p == prop(cx, READONLY) {
            self.read_only = value.and_then(Value::as_bool).unwrap_or(false);
            if let Some(i) = self.inner.get_mut() {
                i.editor.options.read_only = self.read_only;
            }
        } else if p == prop(cx, GUTTER) {
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
        known: skelter_core::taffy::Size<Option<f32>>,
        _avail: skelter_core::taffy::Size<skelter_core::taffy::AvailableSpace>,
    ) -> skelter_core::taffy::Size<f32> {
        // the editor fills what CSS gives it; alone it has no intrinsic size
        skelter_core::taffy::Size {
            width: known.width.unwrap_or(0.0),
            height: known.height.unwrap_or(0.0),
        }
    }

    fn prepare(&mut self, cx: &mut PrepareCx) {
        if self.buffer.is_none() {
            self.buffer = cx.prop_str(prop(cx, BUFFER)).map(str::to_owned);
            self.gutter = cx
                .prop(prop(cx, GUTTER))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            self.read_only = cx.prop_bool(prop(cx, READONLY));
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
        record_scene(cx, &mut i.editor, &doc, &mut i.view, i.now);
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
                let mut doc = i.doc.borrow_mut();
                route_event(&mut i.editor, &mut doc, &mut i.view, ev, now)
            })
            .unwrap_or(false);
        input_effects(cx, ev);
        // the next wake-up: hover and hint deadlines, the caret's next blink
        let next = self.with(|i| {
            let mut doc = i.doc.borrow_mut();
            i.editor.tick(&mut doc, &mut i.view, now)
        });
        schedule_wake(cx, next.flatten());
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
        // (the TextRun projection egui_quire has needs child a11y nodes: docs/SKELTER-QUIRE.md)
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

impl ImeHost for CodeEditorKind {
    fn with_ime<R>(&self, f: impl FnOnce(&mut ImeTarget<'_>) -> R) -> Option<R> {
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

crate::text_input_client!(CodeEditorKind);
