//! The `CodeEditor` widget.
//!
//! It takes the shared `Document` and this view's `EditorView`, applies input transactions in the
//! frame they arrive (SEAM-CHANGES item 2, so typed text appears the same frame), and returns the
//! `Applied` records, the commands it dispatched and its events.
//!
//! Identity: everything the widget stores in egui memory is keyed by the editor id (from the
//! caller's id salt) or ids derived from it (E-2), never `ui.id()`.

use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{
    Color32, Context, Event, EventFilter, Id, ImeEvent, Key, Pos2, Rect, Response, Sense, Ui, Vec2,
    pos2, vec2,
};
use quire_core::{
    Applied, Document, EditorCommand, EditorView, HostRequest, SelRange, Selection, Version,
};

use crate::command::{KeyAction, WidgetAction};
use crate::find_bar::FindBar;
use crate::galley_cache::GalleyCache;
use crate::glue;
use crate::height::HeightIndex;
use crate::keymap::{KeyLookup, Keymap, WhenContext};
use crate::layout::{self, LineShaper};
use crate::theme::{EditorText, EditorTheme};
use crate::wrap::{self, VisualLine, Wrap, WrappedRows};

/// How keys reach the editor.
#[derive(Clone, Debug)]
pub enum KeyHandling {
    /// The widget maps chords to commands through the keymap and consumes the keys it handles.
    Builtin(Keymap),
    /// The host dispatches commands; the widget consumes only text input, IME and clipboard
    /// events while focused.
    Host,
}

impl Default for KeyHandling {
    fn default() -> Self {
        Self::Builtin(default_keymap().clone())
    }
}

/// The bundled VS Code keymap, parsed once.
pub fn default_keymap() -> &'static Keymap {
    static MAP: OnceLock<Keymap> = OnceLock::new();
    MAP.get_or_init(Keymap::vscode_default)
}

/// Something the host may act on.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorEvent {
    /// The Save command ran; the host writes the file and calls `Document::mark_saved`.
    SaveRequested,
    /// Undo in `UndoMode::Host`: the host answers with a transaction of origin `Undo`.
    UndoRequested,
    /// Redo in `UndoMode::Host`.
    RedoRequested,
    FocusChanged(bool),
    /// The hover asks the host's providers for content at `pos` (VS Code `provideHover`; the
    /// mouse rested there for half of `editor.hover.delay`, or `editor.action.showHover` ran).
    /// Answer, now or later, with `EditorView::set_hover(doc, id, version, hovers)`.
    HoverAt {
        id: quire_core::hover::HoverRequestId,
        pos: usize,
        version: Version,
    },
    /// A link in a hover or in the parameter hints was clicked; the host opens `href`.
    HoverLinkActivated {
        href: String,
    },
    /// The parameter hints ask the host's providers for signature help (VS Code
    /// `provideSignatureHelp`). Answer with `EditorView::set_signature_help(doc, id, version,
    /// help)`.
    SignatureHelpRequested {
        id: quire_core::parameter_hints::SignatureHelpRequestId,
        /// The primary cursor, in bytes at `version`.
        pos: usize,
        version: Version,
        context: quire_core::parameter_hints::SignatureHelpContext,
    },
    /// Ctrl+click (Cmd+click on macOS) on the word `range`.
    LinkActivated {
        range: Range<usize>,
        version: Version,
    },
    /// Secondary click at `pos`; the host draws the menu.
    ContextMenu {
        pos: usize,
    },
    /// A diagnostic's action was chosen.
    DiagnosticAction {
        command_id: String,
        args: serde_json::Value,
    },
    /// The suggest session asks the host's providers for items (VS Code
    /// `provideCompletionItems`). Answer, now or later, with
    /// `EditorView::set_completions(doc, id, version, lists)`.
    CompletionRequested {
        id: quire_core::completion::CompletionRequestId,
        /// The cursor, in bytes at `version`.
        pos: usize,
        version: Version,
        trigger: quire_core::completion::CompletionTriggerKind,
        /// `false` for Ctrl+Space.
        auto: bool,
        /// The sources to ask (`None`: all).
        sources: Option<Vec<quire_core::SourceId>>,
    },
    /// The focused item wants resolving (VS Code `resolveCompletionItem`); answer with
    /// `EditorView::resolve_completion_item`.
    CompletionResolveRequested {
        id: quire_core::completion::CompletionRequestId,
        source: quire_core::SourceId,
        index: usize,
        item: Arc<quire_core::completion::CompletionItem>,
    },
    /// An accepted item's command, for the host's registry.
    CompletionCommand {
        command_id: String,
        args: serde_json::Value,
    },
}

/// What one frame of the editor produced.
#[derive(Debug)]
pub struct EditorOutput {
    pub response: Response,
    /// Transactions the widget applied this frame, in order.
    pub applied: Vec<Applied>,
    /// Commands the widget resolved from input this frame (for macros and recording), in order,
    /// whether or not they changed anything.
    pub commands: Vec<EditorCommand>,
    pub events: Vec<EditorEvent>,
    /// Primary caret in screen coordinates (anchors, tutorials, IME placement).
    pub caret_rect: Option<Rect>,
    /// False while the widget has work in flight (a drag selection) or the document is not
    /// settled (pending parse or debounce). Caret blink does not count (R8).
    pub settled: bool,
    /// The document has changes since it was last marked saved.
    pub dirty: bool,
    /// Lines laid out and painted this frame.
    pub visible_lines: Range<usize>,
}

/// The widget. Build with [`CodeEditor::new`], then call [`CodeEditor::show`] or
/// [`CodeEditor::show_in`].
#[derive(Debug)]
pub struct CodeEditor<'a> {
    doc: &'a mut Document,
    view: &'a mut EditorView,
    id: Id,
    theme: Option<&'a EditorTheme>,
    text: Option<&'a EditorText>,
    keys: KeyHandling,
    a11y_name: &'a str,
    read_only: bool,
    author: quire_core::AuthorId,
}

/// Widget-side state kept in egui memory under the editor id.
#[derive(Debug, Default)]
pub(crate) struct Mem {
    pub galleys: GalleyCache,
    /// Rows of wrapped lines.
    pub wraps: GalleyCache<Arc<WrappedRows>>,
    /// Carets drawn at the end of a wrapped row rather than the next row's start: (hash of the
    /// selection and document version they belong to, the heads).
    pub upstream: Option<(u64, Vec<usize>)>,
    pub heights: HeightIndex,
    /// Hash of (row height, folds, wrap, prefix length) the height index was built for.
    pub heights_sig: u64,
    pub scroll: Vec2,
    pub last_interaction: f64,
    pub focused: bool,
    pub last_press: Option<(f64, Pos2, u32)>,
    pub drag: Option<Drag>,
    /// The mouse side of the hover (VS Code `ContentHoverController`).
    pub hover_mouse: HoverMouse,
    /// The hover's rectangle and size last frame, and its scroll offset.
    pub hover_rect: Option<Rect>,
    pub hover_size: Option<Vec2>,
    pub hover_offset: Vec2,
    /// The parameter hints' size last frame.
    pub parameter_hints_size: Option<Vec2>,
    pub find: FindBar,
    pub a11y_page_start: usize,
    /// Live-region text announced after the last page shift.
    pub a11y_announce: Option<String>,
    pub scroll_to_caret: bool,
    /// Widest line laid out since the text last changed (wide glyphs exceed column widths).
    pub max_seen_width: f32,
    /// Document version `max_seen_width` belongs to.
    pub seen_version: Option<Version>,
    /// Visible columns per line, for the horizontal extent.
    pub widths: crate::widths::LineWidths,
    /// Last frame's text area and primary caret (layer coordinates), for anchor queries.
    pub last_text_rect: Option<Rect>,
    pub last_caret: Option<Rect>,
    /// Selection ranges last reported with `TextSelectionSettled` (Linux PRIMARY).
    pub reported_selection: Option<Vec<Range<usize>>>,
    /// First chord of a two-chord binding, waiting for the second.
    pub pending_chord: Option<crate::chord::Chord>,
    /// The last key event was a press the keymap consumed: the `Text` it produced is dropped.
    pub key_consumed: bool,
    /// The manual suggest request waiting since (for VS Code's "Loading..." delay).
    pub suggest_loading_since: Option<(quire_core::completion::CompletionRequestId, f64)>,
    /// `theme.galley_hash()` of this frame.
    pub style_salt: u64,
    /// Reused per frame so clean frames do not allocate (E1).
    pub line_buf: String,
    pub runs_buf: Vec<(usize, Color32)>,
    pub vis_buf: Vec<crate::paint::VisLine>,
    /// Monospace metrics with what they were measured for: font, pixels per point, and a cheap
    /// probe (advance of 'M', row height) that changes when the font definitions do.
    pub mono_cache: Option<(egui::FontId, f32, (f32, f32), layout::Mono)>,
    /// The font atlas the cached galleys point into; when egui rebuilds it, the caches go.
    pub atlas: Option<crate::ascii::AtlasState>,
    /// Column selection over visual rows (soft wrap; VS Code's view `IColumnSelectData`): the
    /// selection key it belongs to, and its (from, to) positions.
    pub column: Option<(u64, (RowCol, RowCol))>,
    /// `CodeEditor::measure`'s last wrapped row count: (hash of what it depends on, rows).
    pub measured: Option<(u64, usize)>,
}

/// VS Code `ContentHoverController`'s mouse state.
#[derive(Clone, Debug, Default)]
pub(crate) struct HoverMouse {
    /// The last pointer position and modifiers seen over the editor.
    pub last: Option<(Pos2, egui::Modifiers)>,
    /// `_reactToEditorMouseMoveRunner`: react to the last mouse move at this time.
    pub runner: Option<f64>,
    /// The scroll offset the hover was shown at (scrolling hides it).
    pub scroll: Option<Vec2>,
}

#[derive(Clone, Debug)]
pub(crate) struct Drag {
    pub anchor: Range<usize>,
    pub unit: Unit,
    /// Alt+Shift+drag: box selection from an anchor to a head (row 0 without wrap).
    pub column: Option<(RowCol, RowCol)>,
}

/// A motion by visual rows (soft wrap).
#[derive(Clone, Copy, Debug)]
enum RowMove {
    Vertical(isize),
    Home,
    End,
}

/// A position on a visual row, as VS Code's view positions: document line, row within it, and
/// visible column (x in columns of the editor font, the wrapping indent included). Without
/// wrap the row is 0 and the column is core's visual column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RowCol {
    pub line: usize,
    pub row: usize,
    pub vis: u32,
}

/// A column-select step by keyboard (soft wrap).
#[derive(Clone, Copy, Debug)]
enum ColumnStep {
    Left,
    Right,
    Rows(isize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Char,
    Word,
    Line,
}

type SharedMem = Arc<Mutex<Mem>>;

fn load_mem(ctx: &Context, id: Id) -> SharedMem {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SharedMem>(id.with("quire.mem"))
            .clone()
    })
}

/// Geometry of one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geom {
    pub rect: Rect,
    pub gutter: Rect,
    pub text: Rect,
    pub row_h: f32,
    pub adv: f32,
    pub mono: layout::Mono,
    pub pad_left: f32,
    /// Soft wrap of this frame (None: off).
    pub wrap: Option<Wrap>,
    /// Lines with more chars than this render a prefix (`stop_rendering_line_after`).
    pub stop_after: usize,
}

impl Geom {
    /// The fold-marker column (right edge of the gutter).
    pub fn fold_col(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.gutter.right() - fold_w(self.adv), self.gutter.top()),
            self.gutter.max,
        )
    }

    /// Screen y of the top of `line`.
    pub fn line_top(&self, mem: &Mem, line: usize) -> f32 {
        self.text.top() + mem.heights.y_of(line) - mem.scroll.y
    }

    /// Screen x of the start of text.
    pub fn text_left(&self, mem: &Mem) -> f32 {
        self.text.left() + self.pad_left - mem.scroll.x
    }
}

impl<'a> CodeEditor<'a> {
    /// `id_salt` identifies this editor in egui memory (caret, scroll, IME ownership survive
    /// reparenting when the salt is stable). `a11y_name` is the accessible name (required: an
    /// unnamed text input fails accessibility checks).
    pub fn new(
        doc: &'a mut Document,
        view: &'a mut EditorView,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        a11y_name: &'a str,
    ) -> Self {
        Self {
            doc,
            view,
            id: Self::id_for(id_salt),
            theme: None,
            text: None,
            keys: KeyHandling::default(),
            a11y_name,
            read_only: false,
            author: quire_core::AuthorId::USER,
        }
    }

    /// Use exactly this id (for hosts that already hold a salted node id).
    pub fn id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }

    pub fn theme(mut self, theme: &'a EditorTheme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn text(mut self, text: &'a EditorText) -> Self {
        self.text = Some(text);
        self
    }

    pub fn key_handling(mut self, keys: KeyHandling) -> Self {
        self.keys = keys;
        self
    }

    /// Who this editor's input is made by (default [`quire_core::AuthorId::USER`]): every
    /// transaction the widget applies carries it, so the host can tell its users apart.
    pub fn author(mut self, author: quire_core::AuthorId) -> Self {
        self.author = author;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// The editor's egui id.
    pub fn editor_id(&self) -> Id {
        self.id
    }

    /// The egui id an editor built with `id_salt` uses.
    pub fn id_for(id_salt: impl std::hash::Hash + std::fmt::Debug) -> Id {
        Id::unique(id_salt)
    }

    /// Rect of a sub-anchor of the editor `id` as of its last frame (layer coordinates):
    /// `caret`, or `line[N]` (0-based line). None if the editor has not been shown.
    pub fn anchor_rect(ctx: &Context, id: Id, anchor: &str) -> Option<Rect> {
        let shared = ctx.data(|d| d.get_temp::<SharedMem>(id.with("quire.mem")))?;
        let mem = shared.lock().unwrap_or_else(|e| e.into_inner());
        if anchor == "caret" {
            return mem.last_caret;
        }
        let n: usize = anchor
            .strip_prefix("line[")?
            .strip_suffix(']')?
            .parse()
            .ok()?;
        if n >= mem.heights.len() {
            return None;
        }
        let t = mem.last_text_rect?;
        let top = t.top() + mem.heights.y_of(n) - mem.scroll.y;
        Some(Rect::from_min_size(
            pos2(t.left(), top),
            vec2(t.width(), mem.heights.get(n)),
        ))
    }

    /// (galleys cached, galleys laid out since creation) of the editor `id`; for tests and perf.
    #[doc(hidden)]
    pub fn galley_stats(ctx: &Context, id: Id) -> Option<(usize, u64)> {
        let shared = ctx.data(|d| d.get_temp::<SharedMem>(id.with("quire.mem")))?;
        let mem = shared.lock().unwrap_or_else(|e| e.into_inner());
        Some((mem.galleys.len(), mem.galleys.misses))
    }

    /// Size for layout without drawing; width fills `available.x` when finite, else gutter plus
    /// 80 columns. The height is what [`CodeEditor::show_in`] renders in a rect that wide: every
    /// line not folded away, with soft wrap its rows at that width (the rows are counted as
    /// the widget lays them out, so measure equals render; svelters' measure, §4.2). The result
    /// is cached per document version, width and style.
    pub fn measure(&self, ctx: &Context, available: Vec2) -> Vec2 {
        let theme = self
            .theme
            .cloned()
            .unwrap_or_else(|| EditorTheme::from_visuals(&ctx.global_style().visuals));
        let row_h = layout::row_height(ctx, &theme.font);
        let mono = layout::mono(ctx, &theme.font);
        let adv = mono.advance();
        let doc: &Document = &*self.doc;
        let text = glue::text(doc);
        let lines = text.len_lines().max(1);
        let gw = gutter_width(lines, adv);
        let w = if available.x.is_finite() {
            available.x
        } else {
            gw + 80.0 * adv
        };
        let cfg = doc.config();
        let wrap = wrap_for(cfg, w, gw, pad_left(adv, &mono), adv);
        let stop_after = cfg.stop_rendering_line_after.max(1);
        let ppp = ctx.pixels_per_point();
        let style_salt = theme.galley_hash();
        let view: &EditorView = &*self.view;
        let folds = glue::folds(view);
        let key = crate::galley_cache::hash_one((
            glue::version(doc),
            wrap,
            stop_after,
            row_h.to_bits(),
            adv.to_bits(),
            ppp.to_bits(),
            &theme.font,
            style_salt,
            folds,
        ));
        // Folded lines (spans merged: folds nest).
        let mut hidden = glue::hidden_line_spans(view, doc);
        hidden.sort_unstable();
        let mut shown = vec![];
        let mut next = 0usize;
        for (a, b) in hidden {
            let b = b.min(lines - 1);
            if a > b || b < next {
                continue;
            }
            let a = a.max(next);
            if a > next {
                shown.push(next..a);
            }
            next = b + 1;
        }
        if next < lines {
            shown.push(next..lines);
        }
        let rows = match wrap {
            None => shown.iter().map(|r| r.len()).sum::<usize>(),
            Some(_) => {
                let shared = load_mem(ctx, self.id);
                let mut mem = shared.lock().unwrap_or_else(|e| e.into_inner());
                match mem.measured {
                    Some((k, rows)) if k == key => rows,
                    _ => {
                        let rows = count_doc_rows(
                            ctx, doc, &theme, &mut mem, mono, wrap, stop_after, style_salt, &shown,
                        );
                        mem.measured = Some((key, rows));
                        rows
                    }
                }
            }
        };
        vec2(w, rows as f32 * row_h + PAD_Y * 2.0)
    }

    /// Allocate the available space and show.
    pub fn show(self, ui: &mut Ui) -> EditorOutput {
        let size = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        self.show_in(ui, rect)
    }

    /// Draw and interact inside `rect`.
    pub fn show_in(self, ui: &mut Ui, rect: Rect) -> EditorOutput {
        let Self {
            doc,
            view,
            id,
            theme,
            text,
            keys,
            a11y_name,
            read_only,
            author,
        } = self;
        let ctx = ui.ctx().clone();
        let owned_theme;
        let theme = match theme {
            Some(t) => t,
            None => {
                owned_theme = EditorTheme::from_visuals(ui.visuals());
                &owned_theme
            }
        };
        let text = match text {
            Some(t) => t,
            None => {
                // Built once: a clean frame allocates nothing for the default strings (E1).
                static DEFAULT_TEXT: OnceLock<EditorText> = OnceLock::new();
                DEFAULT_TEXT.get_or_init(EditorText::default)
            }
        };
        glue::sync(doc, view);
        glue::refresh_find(doc, view);

        let shared = load_mem(&ctx, id);
        let mut mem = shared.lock().unwrap_or_else(|e| e.into_inner());

        // Galleys hold atlas coordinates: drop them when egui rebuilt the atlas.
        let atlas = crate::ascii::AtlasState::now(&ctx);
        if mem.atlas.is_some_and(|a| a.rebuilt_by(&atlas)) {
            mem.galleys.clear();
            mem.wraps.clear();
        }
        mem.atlas = Some(atlas);
        let row_h = layout::row_height(&ctx, &theme.font);
        let ppp = ctx.pixels_per_point();
        let probe = ctx.fonts_mut(|f| (f.glyph_width(&theme.font, 'M'), f.row_height(&theme.font)));
        let mono = match &mem.mono_cache {
            Some((font, p, pr, m)) if *font == theme.font && *p == ppp && *pr == probe => *m,
            _ => {
                let m = layout::mono(&ctx, &theme.font);
                mem.mono_cache = Some((theme.font.clone(), ppp, probe, m));
                m
            }
        };
        let adv = mono.advance();
        let n_lines = glue::text(doc).len_lines().max(1);
        let gw = gutter_width(n_lines, adv);
        let pad_left = pad_left(adv, &mono);
        let cfg = doc.config();
        let stop_after = cfg.stop_rendering_line_after.max(1);
        let wrap = wrap_for(cfg, rect.width(), gw, pad_left, adv);
        mem.widths.sync(doc);
        let splices = std::mem::take(&mut mem.widths.splices);
        let rebuilt = std::mem::take(&mut mem.widths.rebuilt);
        let sig =
            crate::galley_cache::hash_one((row_h.to_bits(), glue::folds(view), wrap, stop_after));
        // With wrap on, a line-count change splices the height index the way the width table
        // was spliced (W1): O(edit), not a rebuild of every line's estimate.
        if wrap.is_some() && mem.heights_sig == sig && !rebuilt && mem.heights.len() != n_lines {
            for (start, removed, inserted) in splices {
                if start + removed > mem.heights.len() {
                    break;
                }
                // Placeholder heights; the inserted lines are in `widths.changed` and get their
                // estimate below.
                mem.heights.splice(start..start + removed, inserted, row_h);
            }
        }
        if mem.heights_sig != sig || mem.heights.len() != n_lines {
            mem.heights_sig = sig;
            mem.heights = match wrap {
                None => HeightIndex::uniform(n_lines, row_h),
                // Rows estimated from each line's columns; laying a line out measures it.
                Some(w) => {
                    let widths = &mem.widths;
                    HeightIndex::from_fn(n_lines, |l| {
                        estimate_rows(widths.cols(l), w.cols, stop_after) as f32 * row_h
                    })
                }
            };
            // Folded lines are zero-height: line_at() never lands on them and painting skips them.
            for (a, b) in glue::hidden_line_spans(view, doc) {
                for l in a..=b.min(n_lines - 1) {
                    mem.heights.set(l, 0.0);
                }
            }
        } else if let Some(w) = wrap {
            // Lines edited since the last frame get a fresh estimate (visible ones are measured
            // when painted).
            let changed = std::mem::take(&mut mem.widths.changed);
            for l in changed {
                if l < mem.heights.len() && mem.heights.get(l) != 0.0 {
                    let h = estimate_rows(mem.widths.cols(l), w.cols, stop_after) as f32 * row_h;
                    mem.heights.set(l, h);
                }
            }
        }
        mem.widths.changed.clear();
        let geom = Geom {
            rect,
            gutter: Rect::from_min_max(rect.min, pos2(rect.left() + gw, rect.bottom())),
            text: Rect::from_min_max(
                pos2(rect.left() + gw, rect.top() + PAD_Y),
                pos2(rect.right(), rect.bottom()),
            ),
            row_h,
            adv,
            mono,
            pad_left,
            wrap,
            stop_after,
        };

        if mem.seen_version != Some(glue::version(doc)) {
            mem.seen_version = Some(glue::version(doc));
            mem.max_seen_width = 0.0;
        }
        mem.scroll = glue::scroll_px(view, doc, &mem.heights);
        mem.style_salt = theme.galley_hash();
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let mut frame = FrameCx {
            ctx: &ctx,
            doc,
            view,
            id,
            theme,
            geom,
            mem: &mut mem,
            out: Out::default(),
            read_only,
            author,
        };

        // Focus.
        if response.clicked() || response.drag_started() || response.secondary_clicked() {
            response.request_focus();
        }
        let focused = ctx.memory(|m| m.has_focus(id));
        if focused != frame.mem.focused {
            frame.mem.focused = focused;
            frame.out.events.push(EditorEvent::FocusChanged(focused));
            frame.mem.last_interaction = ctx.input(|i| i.time);
            glue::set_focused(frame.view, focused);
            frame.mem.pending_chord = None;
            frame.mem.key_consumed = false;
            if !focused && glue::preedit(frame.view).is_some() {
                glue::set_preedit(frame.view, None);
            }
            if !focused {
                // VS Code cancels suggest when the editor loses focus.
                frame.view.cancel_suggest();
            }
        }

        // Keyboard, text, IME, clipboard.
        if focused {
            frame.handle_events(ui, &keys);
        }
        // Accessibility actions (SetValue / ReplaceSelectedText) arrive without focus.
        frame.handle_accesskit_actions(ui);

        // Mouse.
        frame.handle_middle_click_paste(ui, &response);
        frame.handle_pointer(ui, &response);
        frame.handle_scroll(ui, &response);

        // Paint.
        let caret_rect = crate::paint::paint(&mut frame, ui);
        crate::paint::scrollbars(&mut frame, ui);

        // Find bar (drawn over the text, after the editor so it wins hit tests).
        if frame.mem.find.open {
            crate::find_bar::show(&mut frame, ui, text);
        }

        // Hover and parameter hints timers run on the host clock (egui's input time).
        frame.hover_scroll_check();
        let now_ms = (ctx.input(|i| i.time) * 1000.0) as u64;
        frame.view.tick(frame.doc, now_ms);

        // The parameter hints (above the caret) and the suggest list (below it).
        if focused {
            crate::parameter_hints_widget::show(&mut frame, ui, caret_rect, text);
            crate::suggest_widget::show(&mut frame, ui, caret_rect, text);
        }
        frame.drain_suggest();

        // The hover (diagnostics and the providers' content).
        crate::hover_widget::show(&mut frame, ui, text);
        frame.drain_hints();
        match frame.view.next_deadline() {
            Some(0) => ctx.request_repaint(),
            Some(t) => ctx.request_repaint_after(std::time::Duration::from_millis(
                t.saturating_sub(now_ms).max(1),
            )),
            None => {}
        }

        // IME output, like TextEdit (screen coordinates).
        if focused
            && ctx.memory(|m| m.owns_ime_events(id))
            && let Some(caret) = caret_rect
        {
            let to_global = ctx
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default();
            ctx.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect: to_global * geom.text,
                    cursor_rect: to_global * caret,
                    should_interrupt_composition: false,
                });
            });
        }

        // Focus filter for the next frame's events, from the state after this frame's input:
        // Escape stays with the editor only while it has a use for it (preedit, find bar).
        if ctx.memory(|m| m.has_focus(id)) {
            let escape = glue::preedit(frame.view).is_some()
                || frame.mem.find.open
                || frame.mem.pending_chord.is_some()
                || frame.view.suggest().is_visible()
                || frame.view.snippet().is_some()
                || frame.view.parameter_hints().is_visible()
                || frame.view.hover().is_visible();
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape,
                    },
                )
            });
        }

        frame.report_settled_selection(ui);
        crate::a11y::update(&mut frame, ui, a11y_name, text);
        glue::set_scroll_px(frame.view, frame.doc, &frame.mem.heights, frame.mem.scroll);

        let settled = frame.mem.drag.is_none() && glue::is_settled(frame.doc);
        let dirty = frame.doc.is_dirty();
        let visible_lines = frame.visible_lines();
        frame.mem.last_text_rect = Some(geom.text);
        frame.mem.last_caret = caret_rect;
        let caret_rect = caret_rect.map(|r| {
            ctx.layer_transform_to_global(ui.layer_id())
                .unwrap_or_default()
                * r
        });
        let Out {
            applied,
            commands,
            events,
        } = frame.out;
        let mut response = response;
        if !applied.is_empty() {
            response.mark_changed();
        }
        EditorOutput {
            response,
            applied,
            commands,
            events,
            caret_rect,
            settled,
            dirty,
            visible_lines,
        }
    }
}

const PAD_Y: f32 = 2.0;

/// Width of the scrollbars.
pub(crate) const SCROLLBAR_W: f32 = 8.0;

/// Estimated rows of a line of `cols` columns wrapped at `wrap` columns (exact for lines of one
/// column per char that break anywhere; laying the line out measures it).
fn estimate_rows(cols: u32, wrap: u32, stop_after: usize) -> u32 {
    let cols = cols.min(stop_after.min(u32::MAX as usize - 3) as u32 + 3);
    cols.div_ceil(wrap.max(1)).max(1)
}

/// The rendered part of a document line (`slice`, without its break): all of it, or its first
/// `stop` chars when it may be longer (E3 F2). A line split across rope chunks is joined in
/// `buf`.
fn rendered_str<'t>(slice: quire_core::TextSlice<'t>, stop: usize, buf: &'t mut String) -> &'t str {
    if slice.len_bytes() > stop {
        // Possibly more chars than the prefix: copy at most `stop` of them.
        buf.clear();
        let mut chars = 0usize;
        for c in slice.chunks() {
            match c.char_indices().nth(stop - chars) {
                Some((b, _)) => {
                    buf.push_str(&c[..b]);
                    break;
                }
                None => {
                    buf.push_str(c);
                    chars += c.chars().count();
                }
            }
        }
        return buf.as_str();
    }
    match slice.as_str() {
        Some(s) => s,
        None => {
            buf.clear();
            slice.chunks().for_each(|c| buf.push_str(c));
            buf.as_str()
        }
    }
}

/// Left padding of the text area.
fn pad_left(adv: f32, mono: &layout::Mono) -> f32 {
    (adv * 0.5 * mono.pixels_per_point).round() / mono.pixels_per_point
}

/// Soft wrap for an editor `width` wide (None: off). VS Code's wrapping column: the columns that
/// fit beside the vertical scrollbar.
fn wrap_for(
    cfg: &quire_core::EditorConfig,
    width: f32,
    gutter: f32,
    pad: f32,
    adv: f32,
) -> Option<Wrap> {
    (cfg.word_wrap == quire_core::WordWrap::On).then(|| Wrap {
        cols: (((width - gutter - pad - SCROLLBAR_W - 2.0) / adv).floor() as u32).max(1),
        indent: cfg.wrapping_indent,
    })
}

/// Rows of the lines in `shown` as `FrameCx::layout_line` lays them out (with `wrap`), through
/// the editor's caches (`CodeEditor::measure`).
#[allow(clippy::too_many_arguments)]
fn count_doc_rows(
    ctx: &Context,
    doc: &Document,
    theme: &EditorTheme,
    mem: &mut Mem,
    mono: layout::Mono,
    wrap: Option<Wrap>,
    stop_after: usize,
    style_salt: u64,
    shown: &[Range<usize>],
) -> usize {
    let text = glue::text(doc);
    let mut buf = std::mem::take(&mut mem.line_buf);
    let mut runs = std::mem::take(&mut mem.runs_buf);
    let mut rows = 0;
    let Mem { galleys, wraps, .. } = &mut *mem;
    let mut shaper = LineShaper {
        cache: galleys,
        ctx,
        font: &theme.font,
        pixels_per_point: ctx.pixels_per_point(),
        mono,
        tab_size: doc.config().tab_size,
    };
    for line in shown.iter().flat_map(|r| r.clone()) {
        let s = rendered_str(text.line(line), stop_after, &mut buf);
        let style_hash = glue::line_runs(doc, line, theme, &mut runs) ^ style_salt;
        rows += wrap::count_rows(
            &mut shaper,
            wraps,
            s,
            &runs,
            style_hash,
            theme.foreground,
            wrap,
        );
    }
    mem.line_buf = buf;
    mem.runs_buf = runs;
    rows
}

/// Gutter width for `lines` lines: diagnostic mark, digits, padding, fold markers.
fn gutter_width(lines: usize, adv: f32) -> f32 {
    let digits = (lines.max(1).ilog10() + 1).max(2) as f32;
    MARK_W + digits * adv + adv * 1.5 + fold_w(adv)
}

/// Width of the fold-marker column at the gutter's right edge.
pub(crate) fn fold_w(adv: f32) -> f32 {
    (adv * 1.6).max(12.0)
}

const MARK_W: f32 = 8.0;

#[derive(Debug, Default)]
pub(crate) struct Out {
    pub applied: Vec<Applied>,
    pub commands: Vec<EditorCommand>,
    pub events: Vec<EditorEvent>,
}

/// Everything one frame works on.
#[derive(Debug)]
pub(crate) struct FrameCx<'f> {
    pub ctx: &'f Context,
    pub doc: &'f mut Document,
    pub view: &'f mut EditorView,
    pub id: Id,
    pub theme: &'f EditorTheme,
    pub geom: Geom,
    pub mem: &'f mut Mem,
    pub out: Out,
    pub read_only: bool,
    /// Who the widget's transactions are made by.
    pub author: quire_core::AuthorId,
}

impl FrameCx<'_> {
    fn now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    fn touch(&mut self) {
        self.mem.last_interaction = self.now();
    }

    /// Dispatch one editor command: Save and host-mode Undo/Redo become events; everything else
    /// goes through the core and is applied now.
    pub fn dispatch(&mut self, cmd: EditorCommand) {
        self.touch();
        if self.wrapped_motion(&cmd) {
            return;
        }
        let cmd = glue::with_page_lines(cmd, self.page_lines());
        self.out.commands.push(cmd.clone());
        match &cmd {
            // Save is a host request; so are Undo / Redo in host undo mode (the core answers
            // with a request then, but the widget must not wait on a todo-free core to know).
            EditorCommand::Save => {
                self.out.events.push(EditorEvent::SaveRequested);
                return;
            }
            EditorCommand::Undo if glue::host_undo(self.doc) => {
                self.out.events.push(EditorEvent::UndoRequested);
                return;
            }
            EditorCommand::Redo if glue::host_undo(self.doc) => {
                self.out.events.push(EditorEvent::RedoRequested);
                return;
            }
            EditorCommand::CloseFind => self.mem.find.open = false,
            _ => {}
        }
        if self.read_only && glue::changes_text(&cmd) {
            return;
        }
        // A commit character accepts the focused suggestion first (VS Code
        // CommitCharacterController), then types.
        if let EditorCommand::Type { text } = &cmd
            && self.view.suggest().accepts_on(self.doc, text)
        {
            self.dispatch(EditorCommand::AcceptSelectedSuggestion);
        }
        let time_ms = (self.now() * 1000.0) as u64;
        match glue::run(self.doc, self.view, &cmd, time_ms, self.author) {
            Ok(ran) => {
                if let Some(a) = ran.applied {
                    self.push_applied(a);
                }
                match ran.request {
                    Some(HostRequest::Save) => self.out.events.push(EditorEvent::SaveRequested),
                    Some(HostRequest::Undo) => self.out.events.push(EditorEvent::UndoRequested),
                    Some(HostRequest::Redo) => self.out.events.push(EditorEvent::RedoRequested),
                    None => {}
                }
            }
            Err(_refused) => {}
        }
    }

    /// Under soft wrap, vertical motions and Home/End move by visual row, as VS Code's
    /// view-model cursor moves do (`cursorMoveCommands.ts` `_moveToLineStart` / `_moveToLineEnd`,
    /// `cursorMoveOperations.ts` `moveUp` / `moveDown` over view lines). The core knows only
    /// document lines, so the widget resolves these to a selection and records `setSelection`,
    /// as it does for the mouse. Returns false for every other command.
    ///
    /// `InsertCursorAbove` / `Below` and the keyboard column selections also go by visual row
    /// (`cursorMoveCommands.ts` `addCursorUp/Down` with view states, `cursorColumnSelection.ts`
    /// over view lines).
    fn wrapped_motion(&mut self, cmd: &EditorCommand) -> bool {
        use quire_core::Motion;
        if self.geom.wrap.is_none() {
            return false;
        }
        let page = self.page_lines() as isize;
        let step = match cmd {
            EditorCommand::InsertCursorAbove | EditorCommand::InsertCursorBelow => {
                self.wrapped_insert_cursor(matches!(cmd, EditorCommand::InsertCursorBelow));
                return true;
            }
            EditorCommand::ColumnSelectLeft => Some(ColumnStep::Left),
            EditorCommand::ColumnSelectRight => Some(ColumnStep::Right),
            EditorCommand::ColumnSelectUp => Some(ColumnStep::Rows(-1)),
            EditorCommand::ColumnSelectDown => Some(ColumnStep::Rows(1)),
            EditorCommand::ColumnSelectPageUp { .. } => Some(ColumnStep::Rows(-page)),
            EditorCommand::ColumnSelectPageDown { .. } => Some(ColumnStep::Rows(page)),
            _ => None,
        };
        if let Some(step) = step {
            self.wrapped_column_step(step);
            return true;
        }
        let EditorCommand::CursorMove { to, select } = cmd else {
            return false;
        };
        let m = match to {
            Motion::Up => RowMove::Vertical(-1),
            Motion::Down => RowMove::Vertical(1),
            Motion::PageUp { .. } => RowMove::Vertical(-page),
            Motion::PageDown { .. } => RowMove::Vertical(page),
            Motion::LineStart => RowMove::Home,
            Motion::LineEnd => RowMove::End,
            _ => return false,
        };
        let sel = glue::selection(self.view).clone();
        let mut ups = Vec::new();
        let new = sel.transform(|r| {
            let (nr, up) = self.row_move(r, m, *select);
            if up {
                ups.push(nr.head);
            }
            nr
        });
        self.set_selection(new);
        self.set_upstream(ups);
        true
    }

    /// One range moved by visual rows; returns it and whether its head is drawn at the end of a
    /// wrapped row.
    fn row_move(&mut self, r: SelRange, m: RowMove, select: bool) -> (SelRange, bool) {
        let finish = |head: usize, goal: Option<u32>| {
            SelRange::new(if select { r.anchor } else { head }, head).with_goal(goal)
        };
        let len = glue::text(self.doc).len_bytes();
        let row_h = self.geom.row_h;
        let adv = self.geom.adv;
        match m {
            RowMove::Vertical(delta) => {
                let start = if !r.is_empty() && !select {
                    if delta < 0 { r.from() } else { r.to() }
                } else {
                    r.head
                };
                let up0 = start == r.head && self.upstream(r.head);
                let text = glue::text(self.doc);
                let line = text.byte_to_line(start.min(len));
                let col = start - text.line_to_byte(line);
                let vl = self.layout_line(line);
                let (row, x) = vl.locate(col, up0);
                let goal = match r.goal_column {
                    Some(g) if r.is_empty() || select => g,
                    _ => (x / adv).round().max(0.0) as u32,
                };
                let at_edge = if delta < 0 { start == 0 } else { start == len };
                let goal_out = (!at_edge).then_some(goal);
                let total = (self.mem.heights.total() / row_h).round() as isize;
                let cur = (self.mem.heights.y_of(line) / row_h).round() as isize + row as isize;
                let target = cur + delta;
                if target < 0 {
                    return (finish(0, goal_out), false);
                }
                if target >= total {
                    return (finish(len, goal_out), false);
                }
                let y = (target as f32 + 0.5) * row_h;
                let tl = self.mem.heights.line_at(y);
                let tl = tl.min(glue::text(self.doc).len_lines().saturating_sub(1));
                let tvl = self.layout_line(tl);
                let trow = ((y - self.mem.heights.y_of(tl)) / row_h)
                    .floor()
                    .clamp(0.0, (tvl.n_rows() - 1) as f32) as usize;
                let gx = if goal == u32::MAX {
                    f32::INFINITY
                } else {
                    goal as f32 * adv
                };
                let (c, up) = tvl.hit(trow, gx);
                let head = glue::text(self.doc).line_to_byte(tl) + c;
                (finish(head, goal_out), up)
            }
            RowMove::Home => {
                let head = r.head.min(len);
                let up = self.upstream(head);
                let text = glue::text(self.doc);
                let line = text.byte_to_line(head);
                let ls = text.line_to_byte(line);
                let col = head - ls;
                let vl = self.layout_line(line);
                let row = vl.row_of(col, up);
                let first_non_ws = |s: &str| s.find(|c| c != ' ' && c != '\t');
                if row > 0 {
                    let rs = vl.row_start(row);
                    let first = rs + first_non_ws(vl.row_layout(row).text()).unwrap_or(0);
                    if col != first {
                        return (finish(ls + first, None), false);
                    }
                }
                let target = if self.doc.config().smart_home {
                    let s = glue::line_str(glue::text(self.doc), line);
                    let first = first_non_ws(&s).unwrap_or(0);
                    if col == first { 0 } else { first }
                } else {
                    0
                };
                (finish(ls + target, None), false)
            }
            RowMove::End => {
                let head = r.head.min(len);
                let up = self.upstream(head);
                let text = glue::text(self.doc);
                let line = text.byte_to_line(head);
                let ls = text.line_to_byte(line);
                let col = head - ls;
                let vl = self.layout_line(line);
                let row = vl.row_of(col, up);
                let last = row + 1 == vl.n_rows();
                let re = vl.row_end(row);
                let at_row_end = !last && col == re && up;
                if last || at_row_end {
                    (finish(ls + vl.full_len(), Some(u32::MAX)), false)
                } else {
                    (finish(ls + re, Some(u32::MAX)), true)
                }
            }
        }
    }

    /// (line, row, visible column) of byte `b`; `up`: at a wrap point, the previous row's end.
    fn row_col_of(&mut self, b: usize, up: bool) -> RowCol {
        let text = glue::text(self.doc);
        let b = b.min(text.len_bytes());
        let line = text.byte_to_line(b);
        let col = b - text.line_to_byte(line);
        let (row, x) = self.layout_line(line).locate(col, up);
        RowCol {
            line,
            row,
            vis: (x / self.geom.adv).round().max(0.0) as u32,
        }
    }

    /// The byte at visible column `vis` of row `row` of `line` (`u32::MAX`: the row end), and
    /// whether it is the end of a row that is not the line's last.
    fn byte_at_row(&mut self, line: usize, row: usize, vis: u32) -> (usize, bool) {
        let vl = self.layout_line(line);
        let x = if vis == u32::MAX {
            f32::INFINITY
        } else {
            vis as f32 * self.geom.adv
        };
        let (c, up) = vl.hit(row, x);
        (glue::text(self.doc).line_to_byte(line) + c, up)
    }

    /// The visual row `delta` rows below (negative: above) row `row` of `line`, skipping folded
    /// lines, clamped to the first and last row; and whether it moved the whole way.
    fn step_rows(&mut self, line: usize, row: usize, delta: isize) -> ((usize, usize), bool) {
        let n = glue::text(self.doc).len_lines().min(self.mem.heights.len());
        let (mut l, mut r) = (line, row);
        for _ in 0..delta.unsigned_abs() {
            if delta > 0 {
                if r + 1 < self.layout_line(l).n_rows() {
                    r += 1;
                    continue;
                }
                let mut nl = l + 1;
                while nl < n && self.mem.heights.get(nl) == 0.0 {
                    nl += 1;
                }
                if nl >= n {
                    return ((l, r), false);
                }
                (l, r) = (nl, 0);
            } else {
                if r > 0 {
                    r -= 1;
                    continue;
                }
                let mut pl = l;
                loop {
                    if pl == 0 {
                        return ((l, r), false);
                    }
                    pl -= 1;
                    if self.mem.heights.get(pl) != 0.0 {
                        break;
                    }
                }
                (l, r) = (pl, self.layout_line(pl).n_rows() - 1);
            }
        }
        ((l, r), true)
    }

    /// Reveal `pos` instead of the primary caret (the core's `reveal` of a selection command).
    fn reveal_instead(&mut self, pos: Option<usize>) {
        if let Some(p) = pos {
            self.mem.scroll_to_caret = false;
            self.reveal(p, false);
        }
    }

    /// `addCursorUp/Down` over view lines: each selection gets a copy one visual row up or
    /// down (`translateUp/Down`: anchor and head move, the head keeps its goal column, nothing
    /// moves past the first or last row); the primary stays.
    fn wrapped_insert_cursor(&mut self, down: bool) {
        let delta = if down { 1 } else { -1 };
        let sel = glue::selection(self.view).clone();
        let mut ranges: Vec<SelRange> = sel.ranges().to_vec();
        let mut ups: Vec<usize> = ranges
            .iter()
            .map(|r| r.head)
            .filter(|&h| self.upstream(h))
            .collect();
        let mut reveal = None;
        for r in sel.ranges() {
            let a = self.row_col_of(r.anchor, r.anchor == r.head && self.upstream(r.head));
            let anchor = match self.step_rows(a.line, a.row, delta) {
                ((l, row), true) => self.byte_at_row(l, row, a.vis).0,
                _ => r.anchor,
            };
            let up = self.upstream(r.head);
            let h = self.row_col_of(r.head, up);
            let goal = r.goal_column.unwrap_or(h.vis);
            let (head, head_up) = match self.step_rows(h.line, h.row, delta) {
                ((l, row), true) => self.byte_at_row(l, row, goal),
                _ => (r.head, up),
            };
            if head_up {
                ups.push(head);
            }
            reveal = Some(match reveal {
                None => head,
                Some(p) if down => head.max(p),
                Some(p) => head.min(p),
            });
            ranges.push(SelRange::new(anchor, head).with_goal(Some(goal)));
        }
        self.set_selection(Selection::new(ranges, sel.primary_index()));
        self.set_upstream(ups);
        self.reveal_instead(reveal);
    }

    /// The column-select data in effect over visual rows: the recorded one, else from the
    /// primary range (`getCursorColumnSelectData`).
    fn column_data(&mut self) -> (RowCol, RowCol) {
        if let Some((key, d)) = self.mem.column
            && key == self.selection_key()
        {
            return d;
        }
        let p = glue::selection(self.view).primary();
        let up = self.upstream(p.head);
        let from = self.row_col_of(p.anchor, p.anchor == p.head && up);
        let to = self.row_col_of(p.head, up);
        (from, to)
    }

    /// `columnSelectLeft/Right/Up/Down` (and the paged variants) over visual rows.
    fn wrapped_column_step(&mut self, step: ColumnStep) {
        let (from, mut to) = self.column_data();
        match step {
            ColumnStep::Left => to.vis = to.vis.saturating_sub(1),
            ColumnStep::Right => {
                let mut max = 0u32;
                for (l, r) in self.rows_between(from, to) {
                    let vl = self.layout_line(l);
                    let end = if r + 1 == vl.n_rows() {
                        vl.full_len()
                    } else {
                        vl.row_end(r)
                    };
                    max = max.max((vl.x_in_row(r, end) / self.geom.adv).round() as u32);
                }
                if to.vis < max {
                    to.vis += 1;
                }
            }
            ColumnStep::Rows(d) => {
                ((to.line, to.row), _) = self.step_rows(to.line, to.row, d);
            }
        }
        self.wrapped_column_select(from, to);
    }

    /// The visual rows from `from` to `to` (either way round), in that order.
    fn rows_between(&mut self, from: RowCol, to: RowCol) -> Vec<(usize, usize)> {
        let (a, b) = ((from.line, from.row), (to.line, to.row));
        let dir = if b < a { -1 } else { 1 };
        let mut out = vec![a];
        let mut at = a;
        while at != b {
            match self.step_rows(at.0, at.1, dir) {
                (next, true) if next != at => {
                    // Past the target (it was on a folded line or a row that no longer exists).
                    if (dir > 0 && next > b) || (dir < 0 && next < b) {
                        break;
                    }
                    out.push(next);
                    at = next;
                }
                _ => break,
            }
        }
        out
    }

    /// `ColumnSelection.columnSelect` over visual rows: on each row from `from` to `to`, the
    /// range between the two visible columns; rows the box misses are skipped; when it misses
    /// every row, a cursor at each row's end. Recorded as `setSelection`; the data is kept for
    /// the next step.
    fn wrapped_column_select(&mut self, from: RowCol, to: RowCol) {
        let adv = self.geom.adv;
        let is_rtl = from.vis > to.vis;
        let is_ltr = from.vis < to.vis;
        let rows = self.rows_between(from, to);
        let reversed = (to.line, to.row) < (from.line, from.row);
        let mut ranges = Vec::new();
        let mut ups = Vec::new();
        for &(l, r) in &rows {
            let (sb, _) = self.byte_at_row(l, r, from.vis);
            let (eb, eup) = self.byte_at_row(l, r, to.vis);
            let vl = self.layout_line(l);
            let ls = glue::text(self.doc).line_to_byte(l);
            let vs = (vl.x_in_row(r, sb - ls) / adv).round() as u32;
            let ve = (vl.x_in_row(r, eb - ls) / adv).round() as u32;
            if is_ltr && (vs > to.vis || ve < from.vis) {
                continue;
            }
            if is_rtl && (ve > from.vis || vs < to.vis) {
                continue;
            }
            ranges.push(SelRange::new(sb, eb));
            if eup {
                ups.push(eb);
            }
        }
        if ranges.is_empty() {
            for &(l, r) in &rows {
                let (b, up) = self.byte_at_row(l, r, u32::MAX);
                ranges.push(SelRange::point(b));
                if up {
                    ups.push(b);
                }
            }
        }
        let heads = ranges.iter().map(|r| r.head);
        let reveal = if reversed { heads.min() } else { heads.max() };
        // The primary is the first state (the `from` row).
        self.set_selection(Selection::new(ranges, 0));
        self.set_upstream(ups);
        self.mem.column = Some((self.selection_key(), (from, to)));
        self.reveal_instead(reveal);
    }

    /// Lines per page for PageUp / PageDown.
    fn page_lines(&self) -> u32 {
        ((self.geom.text.height() / self.geom.row_h).floor() as u32)
            .saturating_sub(1)
            .max(1)
    }

    pub fn push_applied(&mut self, a: Applied) {
        self.mem.scroll_to_caret = true;
        self.out.applied.push(a);
    }

    fn widget_action(&mut self, a: WidgetAction) {
        self.touch();
        match a {
            WidgetAction::OpenFind | WidgetAction::OpenReplace => {
                let sel = glue::selection(self.view).primary();
                let seed = if !sel.is_empty() {
                    let t = glue::text(self.doc).slice(sel.range()).to_string();
                    (!t.contains('\n')).then_some(t)
                } else {
                    None
                };
                self.mem.find.open_bar(a == WidgetAction::OpenReplace, seed);
            }
            WidgetAction::CloseFind => {
                self.mem.find.open = false;
                glue::clear_find(self.doc, self.view);
            }
        }
    }

    fn when_context(&self) -> WhenContext {
        let st = self.view.suggest();
        let snippet = self.view.snippet();
        let focused = st.focused().is_some();
        WhenContext {
            find_widget_visible: self.mem.find.open,
            multiple_selections: glue::selection(self.view).len() > 1,
            suggest_widget_visible: st.is_visible(),
            suggest_widget_has_focused_suggestion: focused,
            suggest_widget_multiple_suggestions: st.items().len() > 1,
            accept_suggestion_on_enter: self.doc.config().suggest.accept_suggestion_on_enter
                != quire_core::completion::AcceptSuggestionOnEnter::Off,
            suggestion_makes_text_edit: !focused
                || st.makes_text_edit(self.doc, glue::selection(self.view)),
            in_snippet_mode: snippet.is_some(),
            has_next_tabstop: snippet.is_some_and(|s| s.has_next()),
            has_prev_tabstop: snippet.is_some_and(|s| s.has_prev()),
            parameter_hints_visible: self.view.parameter_hints().is_visible(),
            parameter_hints_multiple_signatures: self.view.parameter_hints().multiple_signatures(),
            hover_focused: self.view.hover().is_focused(),
        }
    }

    /// What the hover and the parameter hints want from the host, as events.
    pub(crate) fn drain_hints(&mut self) {
        use quire_core::hover::HoverOutput;
        use quire_core::parameter_hints::ParameterHintsOutput;
        for o in self.view.take_hover_outputs() {
            match o {
                HoverOutput::Request(r) => self.out.events.push(EditorEvent::HoverAt {
                    id: r.id,
                    pos: r.pos,
                    version: r.version,
                }),
            }
        }
        for o in self.view.take_parameter_hints_outputs() {
            match o {
                ParameterHintsOutput::Request(r) => {
                    self.out.events.push(EditorEvent::SignatureHelpRequested {
                        id: r.id,
                        pos: r.pos,
                        version: r.version,
                        context: r.context,
                    })
                }
            }
        }
    }

    /// VS Code `_onEditorScrollChanged`: scrolling the editor hides the hover.
    fn hover_scroll_check(&mut self) {
        let now = self.mem.scroll;
        if self.view.hover().is_visible() {
            match self.mem.hover_mouse.scroll {
                Some(s) if s != now => {
                    self.view.hide_hover();
                    self.mem.hover_mouse.scroll = None;
                }
                Some(_) => {}
                None => self.mem.hover_mouse.scroll = Some(now),
            }
        } else {
            self.mem.hover_mouse.scroll = None;
        }
    }

    /// What the view's suggest session wants from the host, as events.
    pub(crate) fn drain_suggest(&mut self) {
        use quire_core::completion::SuggestOutput as O;
        for o in self.view.take_suggest_outputs() {
            self.out.events.push(match o {
                O::Request(r) => EditorEvent::CompletionRequested {
                    id: r.id,
                    pos: r.pos,
                    version: r.version,
                    trigger: r.trigger,
                    auto: r.auto,
                    sources: r.sources,
                },
                O::Resolve {
                    id,
                    source,
                    index,
                    item,
                } => EditorEvent::CompletionResolveRequested {
                    id,
                    source,
                    index,
                    item,
                },
                O::Command(c) => EditorEvent::CompletionCommand {
                    command_id: c.id,
                    args: serde_json::from_str(&c.args_json).unwrap_or(serde_json::Value::Null),
                },
            });
        }
    }

    /// Keyboard, text, IME and clipboard events, in arrival order. Handled events are removed
    /// from egui's input (R11), so the host sees only what is left.
    fn handle_events(&mut self, ui: &Ui, keys: &KeyHandling) {
        let os = self.ctx.os();
        let owns_ime = self.ctx.memory(|m| m.owns_ime_events(self.id));
        let events = ui.input_mut(|i| std::mem::take(&mut i.events));
        let mut kept = Vec::with_capacity(events.len());
        // egui delivers a keypress as `Key` then the `Text` it produces (Option+Z on macOS gives
        // "Ω"), possibly in a later pass. As in VS Code, where a matched keybinding prevents the
        // keypress's default, the text of a key the keymap consumed is dropped; any other event
        // in between ends that. Unbound keys (AltGr text entry) still type.
        for ev in events {
            let drop_text = std::mem::take(&mut self.mem.key_consumed);
            let handled = match &ev {
                Event::Text(_) if drop_text => true,
                Event::Text(t) => {
                    if glue::preedit(self.view).is_none() && !t.is_empty() {
                        self.dispatch(glue::type_cmd(t));
                    }
                    true
                }
                Event::Ime(ime) if owns_ime => {
                    self.handle_ime(ime);
                    true
                }
                Event::Copy => {
                    self.copy();
                    true
                }
                Event::Cut => {
                    self.copy();
                    self.dispatch(glue::cut_cmd());
                    true
                }
                Event::Paste(t) => {
                    self.dispatch(glue::paste_cmd(t));
                    true
                }
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } if glue::preedit(self.view).is_some() => {
                    // Escape cancels the preedit only.
                    glue::set_preedit(self.view, None);
                    true
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match keys {
                    KeyHandling::Builtin(map) if glue::preedit(self.view).is_none() => {
                        let consumed = self.handle_key(map, os, *key, *modifiers);
                        self.mem.key_consumed = consumed;
                        consumed
                    }
                    _ => false,
                },
                _ => false,
            };
            if !handled {
                kept.push(ev);
            }
        }
        ui.input_mut(|i| {
            // Anything pushed meanwhile stays after what we kept.
            kept.append(&mut i.events);
            i.events = kept;
        });
    }

    /// One key press through the keymap, with the pending first chord of a two-chord binding.
    /// As in VS Code, the first chord waits indefinitely; Escape or any key that does not
    /// complete the sequence cancels it and is consumed. Losing focus also cancels it.
    fn handle_key(
        &mut self,
        map: &Keymap,
        os: egui::os::OperatingSystem,
        key: Key,
        mods: egui::Modifiers,
    ) -> bool {
        let pending = self.mem.pending_chord.take();
        if pending.is_some() && key == Key::Escape {
            return true;
        }
        let lookup = map.lookup_seq(os, pending, key, mods, &self.when_context());
        let hover_cmd = matches!(&lookup, KeyLookup::Bound(b) if is_hover_command(&b.action));
        if self.view.hover().is_focused() {
            // Keys go to the focused hover: its scroll commands, Escape hides it
            // (ContentHoverWidgetWrapper's keydown); other keys do nothing.
            if hover_cmd
                && let KeyLookup::Bound(b) = &lookup
                && let KeyAction::Command(c) = b.action.clone()
            {
                self.dispatch(c);
            } else if key == Key::Escape {
                self.view.hide_hover();
            } else if let KeyLookup::Prefix(c) = lookup {
                self.mem.pending_chord = Some(c);
            }
            return true;
        }
        // ContentHoverController._onKeyDown: any key but a chord prefix or a hover command
        // hides the hover (and still does what it does).
        if self.view.hover().is_visible() && !hover_cmd && !matches!(lookup, KeyLookup::Prefix(_)) {
            self.view.hide_hover();
        }
        match lookup {
            KeyLookup::Bound(b) => match b.action.clone() {
                KeyAction::Command(c) => self.dispatch(c),
                KeyAction::Widget(a) => self.widget_action(a),
            },
            KeyLookup::Prefix(c) => {
                self.mem.pending_chord = Some(c);
            }
            KeyLookup::Unbound => return pending.is_some(),
        }
        true
    }

    /// Hover-related input: the mouse side of VS Code's `ContentHoverController`.
    fn hover_mouse(
        &mut self,
        ui: &Ui,
        response: &Response,
        ptr: Option<Pos2>,
        pressed: bool,
        down: bool,
    ) {
        use quire_core::hover::{
            EventModifiers, HoverAnchor, HoverStartMode, HoverStartSource, MultiCursorModifier,
            should_show_hover,
        };
        let cfg = self.doc.config().hover.clone();
        let now = self.now();
        let now_ms = Some((now * 1000.0) as u64);
        let mods = ui.input(|i| i.modifiers);
        let on_hover = ptr.is_some_and(|p| crate::hover_widget::contains(self, p));
        // _onEditorMouseDown
        if pressed && !on_hover && response.contains_pointer() {
            self.view.hide_hover();
        }
        // Dragging to select hides the hover and suppresses new ones.
        if down && !on_hover && self.mem.drag.is_some() {
            self.mem.hover_mouse.runner = None;
            self.view.hide_hover();
            return;
        }
        let inside = ptr.is_some_and(|p| self.geom.rect.contains(p)) && response.contains_pointer();
        let Some(p) = ptr.filter(|_| inside || on_hover) else {
            // _onEditorMouseLeave
            if self.mem.hover_mouse.last.take().is_some() {
                self.mem.hover_mouse.runner = None;
                let keep = self.view.hover().is_focused()
                    || (cfg.sticky
                        && self
                            .view
                            .hover()
                            .shown()
                            .is_some_and(|r| r.source == HoverStartSource::Keyboard));
                if !keep {
                    self.view.hide_hover();
                }
            }
            return;
        };
        let moved = self
            .mem
            .hover_mouse
            .last
            .is_none_or(|(lp, lm)| lp != p || lm != mods);
        let prev = self.mem.hover_mouse.last.map(|(lp, _)| lp);
        self.mem.hover_mouse.last = Some((p, mods));
        let ev_mods = EventModifiers {
            ctrl: mods.ctrl,
            alt: mods.alt,
            meta: mods.mac_cmd,
        };
        let react = |f: &mut Self, p: Pos2, prev: Option<Pos2>| {
            if should_show_hover(cfg.enabled, MultiCursorModifier::Alt, ev_mods) {
                let anchor = f
                    .hover_anchor_at(p)
                    .map(|pos| HoverAnchor { range: pos..pos });
                // ContentHoverWidget.isMouseGettingCloser
                let closer = cfg.sticky
                    && match (f.mem.hover_rect, prev) {
                        (Some(r), Some(q)) => r.distance_to_pos(p) < r.distance_to_pos(q),
                        _ => false,
                    };
                let doc = &*f.doc;
                if f.view.start_hover(
                    doc,
                    anchor,
                    HoverStartMode::Delayed,
                    HoverStartSource::Mouse,
                    false,
                    closer,
                    now_ms,
                ) {
                    return;
                }
            }
            f.view.hide_hover();
        };
        if let Some(t) = self.mem.hover_mouse.runner
            && now >= t
        {
            self.mem.hover_mouse.runner = None;
            react(self, p, prev);
            return;
        }
        if !moved {
            if let Some(t) = self.mem.hover_mouse.runner {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_secs_f64(
                        (t - now).max(0.001),
                    ));
            }
            return;
        }
        // _onEditorMouseMove / _shouldKeepCurrentHover
        let visible = self.view.hover().is_visible();
        let keep = self.view.hover().is_focused()
            || (cfg.sticky && on_hover)
            || (cfg.sticky
                && self
                    .view
                    .hover()
                    .shown()
                    .is_some_and(|r| r.source == HoverStartSource::Keyboard));
        if keep {
            self.mem.hover_mouse.runner = None;
            return;
        }
        if visible && cfg.sticky && cfg.hiding_delay > 0 {
            if self.mem.hover_mouse.runner.is_none() {
                let t = now + cfg.hiding_delay as f64 / 1000.0;
                self.mem.hover_mouse.runner = Some(t);
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(cfg.hiding_delay));
            }
            return;
        }
        react(self, p, prev);
    }

    /// The position a hover at `p` is about: on text, or within half a character after a
    /// line's end (VS Code's CONTENT_TEXT / CONTENT_EMPTY targets).
    fn hover_anchor_at(&mut self, p: Pos2) -> Option<usize> {
        if !self.geom.text.contains(p) {
            return None;
        }
        let (line, _) = self.line_row_at(p.y);
        let bottom = self.geom.line_top(self.mem, line) + self.mem.heights.get(line);
        if p.y > bottom {
            return None;
        }
        let pos = self.pos_at(p);
        let (_, x, _) = self.pos_row(pos);
        let text = glue::text(self.doc);
        let line_end = text.line_range(text.byte_to_line(pos)).end;
        if pos >= line_end && p.x > x + self.geom.adv * 0.5 {
            return None;
        }
        Some(pos)
    }

    /// Copy the selection (ranges joined by newlines). With every range empty and
    /// `empty_selection_clipboard` on, copy the cursors' whole lines (VS Code), which is also
    /// what Cut then deletes. Returns whether anything was copied.
    fn copy(&mut self) -> bool {
        let text = glue::text(self.doc);
        let ranges = glue::ranges(glue::selection(self.view));
        let parts: Vec<String> = if ranges.iter().all(|r| r.is_empty()) {
            if !self.doc.config().empty_selection_clipboard {
                return false;
            }
            let mut lines: Vec<usize> = ranges
                .iter()
                .map(|r| text.byte_to_line(r.head.min(text.len_bytes())))
                .collect();
            lines.dedup();
            lines
                .into_iter()
                .map(|l| {
                    let mut s = glue::line_str(text, l).into_owned();
                    s.push('\n');
                    s
                })
                .collect()
        } else {
            ranges
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| text.slice(r.range()).to_string())
                .collect()
        };
        if parts.is_empty() {
            return false;
        }
        let joined =
            if parts.iter().all(|p| p.ends_with('\n')) && ranges.iter().all(|r| r.is_empty()) {
                parts.concat()
            } else {
                parts.join("\n")
            };
        self.ctx.copy_text(joined);
        true
    }

    fn handle_ime(&mut self, ime: &ImeEvent) {
        self.touch();
        match ime {
            ImeEvent::Preedit {
                text,
                active_range_chars,
            } => {
                if text.is_empty() {
                    if glue::preedit(self.view).is_some() {
                        glue::set_preedit(self.view, None);
                    }
                } else {
                    if glue::preedit(self.view).is_none()
                        && !glue::selection(self.view).primary().is_empty()
                        && !self.read_only
                    {
                        // Composition replaces the selection, as in TextEdit.
                        self.dispatch(glue::type_cmd(""));
                    }
                    glue::set_preedit(self.view, Some((text, active_range_chars.clone())));
                    self.mem.scroll_to_caret = true;
                }
            }
            ImeEvent::Commit(text) => {
                if glue::preedit(self.view).is_some() {
                    glue::set_preedit(self.view, None);
                }
                if !text.is_empty() {
                    self.dispatch(glue::composition_cmd(text, 0, 0));
                }
            }
            ImeEvent::DeleteSurrounding {
                before_chars,
                after_chars,
            } => {
                if *before_chars + *after_chars > 0 {
                    self.dispatch(glue::composition_cmd("", *before_chars, *after_chars));
                }
            }
            #[allow(deprecated)]
            ImeEvent::Enabled | ImeEvent::Disabled => {}
        }
    }

    fn handle_accesskit_actions(&mut self, ui: &Ui) {
        use egui::accesskit::{Action, ActionData};
        let id = self.id;
        let reqs: Vec<Option<ActionData>> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::AccessKitActionRequest(r)
                        if r.target_node == id.accesskit_id()
                            && matches!(
                                r.action,
                                Action::SetValue | Action::ReplaceSelectedText
                            ) =>
                    {
                        Some(r.data.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        if self.read_only {
            return;
        }
        for data in reqs {
            if let Some(ActionData::Value(v)) = data {
                // ReplaceSelectedText and SetValue both carry the new text; SetValue replaces all.
                let len = glue::text(self.doc).len_bytes();
                let sel = glue::selection(self.view).primary();
                let range = if sel.is_empty() { 0..len } else { sel.range() };
                if let Some(a) = glue::replace(self.doc, self.view, range, &v, false, self.author) {
                    self.push_applied(a);
                }
            }
        }
    }

    /// The (visible) line at screen y.
    fn line_at_y(&self, y: f32) -> usize {
        let line = self
            .mem
            .heights
            .line_at(y - self.geom.text.top() + self.mem.scroll.y);
        line.min(glue::text(self.doc).len_lines().saturating_sub(1))
    }

    /// The box-selection position under `p`: inside the text the column comes from the galley
    /// hit test (so wide CJK glyphs count as core counts them), elsewhere (past a row's end, in
    /// a continuation row's indent) from whole advances, as VS Code's mouse column selection
    /// does. Without wrap the column is core's visual column and may lie past the line's end;
    /// with wrap it is a visible column on the row under `p`.
    fn row_col_at(&mut self, p: Pos2) -> RowCol {
        let (line, row) = self.line_row_at(p.y);
        let x = p.x - self.geom.text_left(self.mem);
        let adv = self.geom.adv;
        let vl = self.layout_line(line);
        let row = row.min(vl.n_rows() - 1);
        let x0 = vl.row_x0(row);
        let end = x0 + vl.row_layout(row).width();
        if self.geom.wrap.is_some() {
            let vis = if x >= x0 && x <= end {
                let (col, _) = vl.hit(row, x);
                vl.x_in_row(row, col) / adv
            } else if x > end {
                (end / adv).round() + ((x - end) / adv).round()
            } else {
                x / adv
            };
            return RowCol {
                line,
                row,
                vis: vis.round().max(0.0) as u32,
            };
        }
        let s = glue::line_str(glue::text(self.doc), line);
        let tab = self.doc.config().tab_size;
        let last = row + 1 >= vl.n_rows();
        let vis = if x <= end || !last {
            let (col, _) = vl.hit(row, x);
            quire_core::columns::visual_col(&s, col.min(s.len()), tab)
        } else {
            let past = ((x - end) / adv).round().max(0.0) as u32;
            quire_core::columns::visual_col(&s, s.len(), tab) + past
        };
        RowCol { line, row: 0, vis }
    }

    /// Box selection from `anchor` to `head` (mouse): over visual rows with wrap, else the
    /// core's `columnSelect`.
    fn mouse_column_select(&mut self, anchor: RowCol, head: RowCol) {
        if self.geom.wrap.is_some() {
            self.touch();
            self.wrapped_column_select(anchor, head);
        } else {
            self.dispatch(glue::column_select_cmd(
                (anchor.line, anchor.vis),
                (head.line, head.vis),
            ));
        }
    }

    /// (visible line, row within it) at screen y.
    fn line_row_at(&mut self, y: f32) -> (usize, usize) {
        let line = self.line_at_y(y);
        let top = self.geom.line_top(self.mem, line);
        let row = ((y - top) / self.geom.row_h).floor().max(0.0) as usize;
        (line, row)
    }

    /// Byte offset under a screen position.
    pub fn pos_at(&mut self, p: Pos2) -> usize {
        self.pos_at_affine(p).0
    }

    /// Byte offset under a screen position, and whether it is the end of a wrapped row (drawn
    /// there rather than at the next row's start).
    pub fn pos_at_affine(&mut self, p: Pos2) -> (usize, bool) {
        let (line, row) = self.line_row_at(p.y);
        let vl = self.layout_line(line);
        let (col, up) = vl.hit(row, p.x - self.geom.text_left(self.mem));
        (glue::text(self.doc).line_to_byte(line) + col, up)
    }

    /// Whether the caret at `head` is drawn at the end of the previous wrapped row.
    pub fn upstream(&self, head: usize) -> bool {
        match &self.mem.upstream {
            Some((key, heads)) if *key == self.selection_key() => heads.contains(&head),
            _ => false,
        }
    }

    /// Identity of the current selection and text (upstream carets belong to it).
    fn selection_key(&self) -> u64 {
        let sel = glue::selection(self.view);
        crate::galley_cache::hash_one((
            glue::version(self.doc),
            glue::ranges(sel)
                .iter()
                .map(|r| (r.anchor, r.head))
                .collect::<Vec<_>>(),
        ))
    }

    /// Record which heads of the current selection sit at the end of a wrapped row.
    fn set_upstream(&mut self, heads: Vec<usize>) {
        self.mem.upstream = (!heads.is_empty()).then(|| (self.selection_key(), heads));
    }

    /// Lay out line `line` through the cache. A cache hit allocates nothing: a line split across
    /// rope chunks is joined in a reused buffer, and token runs go to a reused vector (E1).
    /// A line longer than `stop_rendering_line_after` chars lays out only that prefix (E3 F2).
    /// Laying out a line measures its rows into the height index.
    pub fn layout_line(&mut self, line: usize) -> VisualLine {
        let text = glue::text(self.doc);
        let slice = text.line(line);
        let full_len = slice.len_bytes();
        let stop = self.geom.stop_after;
        let mut buf = std::mem::take(&mut self.mem.line_buf);
        let mut runs = std::mem::take(&mut self.mem.runs_buf);
        let s = rendered_str(slice, stop, &mut buf);
        let style_hash =
            glue::line_runs(self.doc, line, self.theme, &mut runs) ^ self.mem.style_salt;
        let mut shaper = LineShaper {
            cache: &mut self.mem.galleys,
            ctx: self.ctx,
            font: &self.theme.font,
            pixels_per_point: self.ctx.pixels_per_point(),
            mono: self.geom.mono,
            tab_size: self.doc.config().tab_size,
        };
        let layout = wrap::shape_line(
            &mut shaper,
            &mut self.mem.wraps,
            s,
            &runs,
            style_hash,
            self.theme.foreground,
            self.geom.wrap,
            full_len,
        );
        self.mem.line_buf = buf;
        self.mem.runs_buf = runs;
        self.note_rows(line, layout.n_rows());
        layout
    }

    /// Line `line` with the IME preedit `pre` spliced in at byte `col`, underlined, through the
    /// caches (keyed by the line, the preedit and where it goes). A line longer than
    /// `stop_rendering_line_after` lays out only its prefix, with the preedit inside it: at the
    /// prefix's end when the caret is past it. Returns the layout and the byte column the
    /// preedit went to. Laying it out measures the line's rows.
    pub fn layout_preedit_line(
        &mut self,
        line: usize,
        col: usize,
        pre: &str,
    ) -> (VisualLine, usize) {
        use egui::text::TextFormat;
        let text = glue::text(self.doc);
        let slice = text.line(line);
        let full_len = slice.len_bytes();
        let mut buf = std::mem::take(&mut self.mem.line_buf);
        let s = rendered_str(slice, self.geom.stop_after, &mut buf);
        let mut at = col.min(s.len());
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        let spliced: String = [&s[..at], pre, &s[at..]].concat();
        self.mem.line_buf = buf;
        let theme = self.theme;
        let font = theme.font.clone();
        let plain = TextFormat::simple(font.clone(), theme.foreground);
        let under = TextFormat {
            underline: egui::Stroke::new(1.0, theme.preedit_underline),
            ..TextFormat::simple(font, theme.foreground)
        };
        let spans = [(at, under), (at + pre.len(), plain.clone())];
        let style = crate::galley_cache::hash_one((
            "quire.preedit",
            at,
            pre.len(),
            theme.preedit_underline,
            theme.foreground,
        )) ^ self.mem.style_salt;
        let mut shaper = LineShaper {
            cache: &mut self.mem.galleys,
            ctx: self.ctx,
            font: &self.theme.font,
            pixels_per_point: self.ctx.pixels_per_point(),
            mono: self.geom.mono,
            tab_size: self.doc.config().tab_size,
        };
        let layout = wrap::shape_formatted(
            &mut shaper,
            &mut self.mem.wraps,
            &spliced,
            &spans,
            &plain,
            style,
            self.geom.wrap,
            full_len + pre.len(),
        );
        self.note_rows(line, layout.n_rows());
        (layout, at)
    }

    /// Record a laid-out line's measured rows in the height index (not for folded lines).
    /// Returns whether its height changed.
    pub fn note_rows(&mut self, line: usize, rows: usize) -> bool {
        if self.geom.wrap.is_none() || line >= self.mem.heights.len() {
            return false;
        }
        let h = self.mem.heights.get(line);
        let want = rows as f32 * self.geom.row_h;
        if h == 0.0 || h == want {
            return false;
        }
        self.mem.heights.set(line, want);
        true
    }

    /// The unit range around `pos` (char: empty; word and line: quire-core's double- and
    /// triple-click ranges).
    fn unit_range(&self, pos: usize, unit: Unit) -> Range<usize> {
        match unit {
            Unit::Char => pos..pos,
            Unit::Word => glue::word_range_at(self.doc, pos),
            Unit::Line => glue::line_range_at(self.doc, pos),
        }
    }

    fn handle_pointer(&mut self, ui: &Ui, response: &Response) {
        let (pressed, down, mods, time, ptr) = ui.input(|i| {
            // Modifiers as they were at the press (the event carries them).
            let press_mods = i.events.iter().rev().find_map(|e| match e {
                Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                    ..
                } => Some(*modifiers),
                _ => None,
            });
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                press_mods.unwrap_or(i.modifiers),
                i.time,
                i.pointer.interact_pos(),
            )
        });
        let (click_delay, click_dist) = self.ctx.options(|o| {
            (
                o.input_options.max_double_click_delay,
                o.input_options.max_click_dist,
            )
        });
        let Some(ptr) = ptr else {
            self.hover_mouse(ui, response, None, pressed, down);
            return;
        };
        let in_text = self.geom.text.contains(ptr) || self.geom.gutter.contains(ptr);

        if response.secondary_clicked() {
            let pos = self.pos_at(ptr);
            self.out.events.push(EditorEvent::ContextMenu { pos });
        }

        if pressed && response.hovered() && in_text {
            self.touch();
            let (pos, up) = self.pos_at_affine(ptr);
            if mods.command && !mods.shift && !mods.alt {
                let range = self.unit_range(pos, Unit::Word);
                self.out.events.push(EditorEvent::LinkActivated {
                    range,
                    version: glue::version(self.doc),
                });
                self.mem.last_press = None;
                return;
            }
            // Fold marker: toggle the range headed by this line.
            if self.geom.fold_col().contains(ptr) {
                let line = self.line_at_y(ptr.y);
                if !glue::fold_headers(self.doc, line..line + 1).is_empty() {
                    if let Some(a) =
                        glue::toggle_fold_at_line(self.doc, self.view, line, self.author)
                    {
                        self.push_applied(a);
                    }
                    self.mem.last_press = None;
                    return;
                }
            }
            // Alt+Shift: box selection (drag extends it); Alt: add a cursor.
            if mods.alt && mods.shift && !mods.command && self.geom.text.contains(ptr) {
                let at = self.row_col_at(ptr);
                self.mem.drag = Some(Drag {
                    anchor: pos..pos,
                    unit: Unit::Char,
                    column: Some((at, at)),
                });
                self.mouse_column_select(at, at);
                self.mem.last_press = None;
                return;
            }
            if mods.alt && !mods.shift && !mods.command && self.geom.text.contains(ptr) {
                let sel = glue::with_cursor_added(glue::selection(self.view), pos);
                self.set_selection(sel);
                self.mem.last_press = None;
                return;
            }
            let count = match self.mem.last_press {
                Some((t, p, c)) if time - t < click_delay && p.distance(ptr) < click_dist => {
                    c % 3 + 1
                }
                _ => 1,
            };
            self.mem.last_press = Some((time, ptr, count));
            let unit = if self.geom.gutter.contains(ptr) {
                Unit::Line
            } else {
                match count {
                    1 => Unit::Char,
                    2 => Unit::Word,
                    _ => Unit::Line,
                }
            };
            let sel = glue::selection(self.view).primary();
            let (anchor_range, head) = if mods.shift && unit == Unit::Char {
                (sel.anchor..sel.anchor, pos)
            } else {
                let r = self.unit_range(pos, unit);
                (r.clone(), r.end)
            };
            self.mem.drag = Some(Drag {
                anchor: anchor_range.clone(),
                unit,
                column: None,
            });
            let new = if anchor_range.is_empty() || mods.shift {
                glue::single(anchor_range.start, head)
            } else {
                glue::single(anchor_range.start, anchor_range.end)
            };
            self.set_selection(new);
            if up && unit == Unit::Char {
                self.set_upstream(vec![pos]);
            }
        } else if down
            && let Some(Drag {
                column: Some((anchor, head)),
                ..
            }) = self.mem.drag.clone()
            && (response.dragged() || response.is_pointer_button_down_on())
        {
            let at = self.row_col_at(ptr);
            if at != head {
                if let Some(d) = self.mem.drag.as_mut() {
                    d.column = Some((anchor, at));
                }
                self.mouse_column_select(anchor, at);
            }
            ui.ctx().request_repaint();
        } else if down
            && let Some(drag) = self.mem.drag.clone()
            && (response.dragged() || response.is_pointer_button_down_on())
        {
            let (pos, up) = self.pos_at_affine(ptr);
            let r = self.unit_range(pos, drag.unit);
            let a = drag.anchor;
            let new = if r.start < a.start {
                glue::single(a.end, r.start)
            } else {
                glue::single(a.start, r.end.max(a.end))
            };
            if glue::selection(self.view).primary().range() != new.primary().range()
                || glue::selection(self.view).primary().head != new.primary().head
            {
                self.set_selection(new);
                if up && drag.unit == Unit::Char {
                    self.set_upstream(vec![pos]);
                }
            }
            // Autoscroll when dragging past the edges.
            let t = self.geom.text;
            if ptr.y < t.top() {
                self.mem.scroll.y -= (t.top() - ptr.y).min(self.geom.row_h * 3.0);
            } else if ptr.y > t.bottom() {
                self.mem.scroll.y += (ptr.y - t.bottom()).min(self.geom.row_h * 3.0);
            }
            ui.ctx().request_repaint();
        }
        if !down {
            self.mem.drag = None;
        }

        if response.hovered() && self.geom.text.contains(ptr) {
            self.ctx.set_cursor_icon(egui::CursorIcon::Text);
        }

        let hover_ptr = ui.input(|i| i.pointer.hover_pos());
        self.hover_mouse(ui, response, hover_ptr, pressed, down);
    }

    /// X11/Wayland PRIMARY paste: egui-winit turns a middle click into `MiddleClickPaste` with
    /// the PRIMARY contents; insert them where the pointer is, as `TextEdit` does (VS Code's
    /// `editor.selectionClipboard` on Linux).
    fn handle_middle_click_paste(&mut self, ui: &Ui, response: &Response) {
        if !response.contains_pointer() || self.read_only {
            return;
        }
        let Some((pos, text)) = ui.input_mut(|i| {
            let k = i
                .events
                .iter()
                .position(|e| matches!(e, Event::MiddleClickPaste { .. }))?;
            match i.events.remove(k) {
                Event::MiddleClickPaste { pos, text } => Some((pos, text)),
                _ => None,
            }
        }) else {
            return;
        };
        let pos = self
            .ctx
            .layer_transform_from_global(ui.layer_id())
            .map_or(pos, |t| t * pos);
        if !self.geom.text.contains(pos) {
            return;
        }
        response.request_focus();
        let at = self.pos_at(pos);
        self.set_selection(glue::single(at, at));
        self.dispatch(glue::paste_cmd(&text));
    }

    /// Report the selection once it settles (no pointer button down) and only when it changed,
    /// with `OutputCommand::TextSelectionSettled`; egui-winit puts it in the X11/Wayland PRIMARY
    /// selection. Ranges are joined by newlines, as Copy does.
    fn report_settled_selection(&mut self, ui: &Ui) {
        // Only the focused view reports (another view's selection moving with an edit is not
        // the user selecting).
        if !self.mem.focused || ui.input(|i| i.pointer.any_down()) {
            return;
        }
        let ranges: Vec<Range<usize>> = glue::ranges(glue::selection(self.view))
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| r.range())
            .collect();
        let now = (!ranges.is_empty()).then_some(ranges);
        if now == self.mem.reported_selection {
            return;
        }
        if let Some(ranges) = &now {
            let t = glue::text(self.doc);
            let s: Vec<String> = ranges
                .iter()
                .map(|r| t.slice(r.clone()).to_string())
                .collect();
            self.ctx
                .send_cmd(egui::OutputCommand::TextSelectionSettled(s.join("\n")));
        }
        self.mem.reported_selection = now;
    }

    /// Mouse selection: recorded as `setSelection`, applied as a selection-only transaction (the
    /// transaction that command resolves to).
    fn set_selection(&mut self, sel: Selection) {
        self.touch();
        self.out.commands.push(glue::set_selection_cmd(sel.clone()));
        if let Some(a) = glue::select(self.doc, self.view, sel, self.author) {
            self.push_applied(a);
        }
    }

    fn content_size(&mut self) -> Vec2 {
        let h = self.mem.heights.total() + self.geom.text.height() * 0.5;
        vec2(self.content_width(), h)
    }

    /// Horizontal extent: the longest line (columns × advance, or the widest galley laid out
    /// since the last edit when wide glyphs make that larger) plus a margin.
    pub fn content_width(&mut self) -> f32 {
        if self.geom.wrap.is_some() {
            return self.geom.text.width();
        }
        let cols = (self.mem.widths.max_cols() as usize).min(self.geom.stop_after + 3) as f32
            * self.geom.adv
            + self.geom.pad_left;
        cols.max(self.mem.max_seen_width) + self.geom.adv * 4.0
    }

    fn clamp_scroll(&mut self) {
        let content = self.content_size();
        let max = (content - self.geom.text.size()).max(Vec2::ZERO);
        self.mem.scroll = self.mem.scroll.clamp(Vec2::ZERO, max);
    }

    fn handle_scroll(&mut self, ui: &Ui, response: &Response) {
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta != Vec2::ZERO {
                self.mem.scroll -= delta;
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
        }
        if let Some((pos, center)) = glue::take_reveal(self.view, self.doc) {
            self.reveal(pos, center);
        } else if std::mem::take(&mut self.mem.scroll_to_caret) {
            let head = glue::selection(self.view).primary().head;
            self.reveal(head, false);
        }
        self.mem.scroll_to_caret = false;
        self.clamp_scroll();
    }

    fn reveal(&mut self, pos: usize, center: bool) {
        let text = glue::text(self.doc);
        let head = pos.min(text.len_bytes());
        let line = text.byte_to_line(head);
        let col = head - text.line_to_byte(line);
        let vl = self.layout_line(line);
        let (row, x) = vl.locate(col, self.upstream(head));
        let top = self.mem.heights.y_of(line) + row as f32 * self.geom.row_h;
        let bottom = top + self.geom.row_h;
        let view_h = self.geom.text.height();
        if center {
            self.mem.scroll.y = top - (view_h - self.geom.row_h) * 0.5;
        } else if top < self.mem.scroll.y {
            self.mem.scroll.y = top;
        } else if bottom > self.mem.scroll.y + view_h {
            self.mem.scroll.y = bottom - view_h;
        }
        let x = x + self.geom.pad_left;
        let view_w = self.geom.text.width();
        let margin = self.geom.adv * 4.0;
        if x < self.mem.scroll.x + margin {
            self.mem.scroll.x = (x - margin).max(0.0);
        } else if x > self.mem.scroll.x + view_w - margin {
            self.mem.scroll.x = x - view_w + margin;
        }
        self.mem.max_seen_width = self.mem.max_seen_width.max(x + margin);
    }

    /// Visible line range.
    pub fn visible_lines(&self) -> Range<usize> {
        let first = self.mem.heights.line_at(self.mem.scroll.y);
        let last = self
            .mem
            .heights
            .line_at(self.mem.scroll.y + self.geom.text.height());
        first..(last + 1).min(self.mem.heights.len())
    }

    pub fn caret_visible(&self, ui: &Ui) -> bool {
        let t = self.theme;
        let blink = ui.visuals().text_cursor.blink && t.blink_on > 0.0 && t.blink_off > 0.0;
        if !blink {
            return true;
        }
        let now = self.now();
        let idle = now - self.mem.last_interaction;
        if idle > t.blink_idle_stop as f64 {
            return true;
        }
        let period = (t.blink_on + t.blink_off) as f64;
        let phase = idle % period;
        let (vis, until) = if phase < t.blink_on as f64 {
            (true, t.blink_on as f64 - phase)
        } else {
            (false, period - phase)
        };
        let until_stop = t.blink_idle_stop as f64 - idle;
        self.ctx
            .request_repaint_after(std::time::Duration::from_secs_f64(
                until.min(until_stop).max(0.0),
            ));
        vis
    }
}

/// The hover commands that do not hide the hover (VS Code `_isPotentialKeyboardShortcut`).
fn is_hover_command(a: &KeyAction) -> bool {
    matches!(
        a,
        KeyAction::Command(
            EditorCommand::ShowHover
                | EditorCommand::ScrollUpHover
                | EditorCommand::ScrollDownHover
                | EditorCommand::ScrollLeftHover
                | EditorCommand::ScrollRightHover
                | EditorCommand::PageUpHover
                | EditorCommand::PageDownHover
                | EditorCommand::GoToTopHover
                | EditorCommand::GoToBottomHover
        )
    )
}
