//! The editor engine: one [`EditorView`] of a shared [`Document`] laid out, scrolled and driven by
//! presto input, independent of where its display lists go ([`crate::scene`] records them,
//! [`crate::layers`] commits them as compositor layers, [`crate::kind`] hosts them in a presto-core
//! node).
//!
//! The behaviour is egui_quire's (`editor.rs`): the same commands, keymap, multi-click and drag
//! selection, Alt+click cursors, Alt+Shift box selection, fold toggles in the gutter, IME preedit as
//! injected text, the same host events. What differs is the input and geometry vocabulary
//! (presto's events, points in the editor's local space) and vertical motion under soft wrap,
//! which goes through quire-text's [`VisualLines`](quire_text::VisualLines) over the wrapped rows
//! ([`crate::visual`]).
//!
//! Coordinates: the editor's box is local space, origin top-left. The text area starts at
//! [`Geom::text_left`]; *content space* is the scrolled text: line `l` starts at y
//! `heights.y_of(l)`, x 0 is the text area's left edge. Local = content − scroll + text origin.

use std::ops::Range;
use std::sync::Arc;

use presto_base::event::{ImeEvent, KeyEvent, KeyState, Modifiers, MouseButton, ScrollDelta};
use presto_base::geom::{Point, Rect, Size, Vec2};
use presto_base::key::Key;
use presto_base::time::{Span, Time};
use presto_core::PointerInput;
use quire_core::{
    Applied, AuthorId, ChangeSet, Document, EditorCommand, EditorView, GroupHint, HostRequest,
    InputKind, Origin, SelRange, Selection, Transaction, UndoMode, Version, WordWrap,
};

use crate::cache::{GenCache, RowsKey, hash_one};
use crate::chord::Chord;
use crate::command::{KeyAction, WidgetAction};
use crate::height::HeightIndex;
use crate::keymap::{KeyLookup, Keymap, OperatingSystem, WhenContext};
use crate::lines::{self, DisplayLine, PieceKind};
use crate::shape::LineShaper;
use crate::theme::EditorTheme;
use crate::widths::LineWidths;
use crate::wrap::{LineRows, Wrap};

/// Who handles keys.
#[derive(Clone, Debug)]
pub enum KeyHandling {
    /// The editor resolves keys through this keymap.
    Builtin(Keymap),
    /// The host resolves keys (and dispatches commands with [`PrestoEditor::dispatch`]); typed text
    /// still types.
    Host,
}

impl Default for KeyHandling {
    fn default() -> Self {
        KeyHandling::Builtin(default_keymap().clone())
    }
}

/// The bundled VS Code keymap, parsed once.
pub fn default_keymap() -> &'static Keymap {
    static MAP: std::sync::OnceLock<Keymap> = std::sync::OnceLock::new();
    MAP.get_or_init(Keymap::vscode_default)
}

/// What the host may act on (egui_quire's `EditorEvent`, plus the clipboard and find bar, which the
/// egui widget handled through egui itself).
#[derive(Clone, Debug, PartialEq)]
pub enum EditorEvent {
    /// The Save command ran; the host writes the file and calls `Document::mark_saved`.
    SaveRequested,
    /// Undo in `UndoMode::Host`: the host answers with a transaction of origin `Undo`.
    UndoRequested,
    RedoRequested,
    FocusChanged(bool),
    /// The hover asks the host's providers for content at `pos`; answer with
    /// `EditorView::set_hover(doc, id, version, hovers)`.
    HoverAt {
        id: quire_core::hover::HoverRequestId,
        pos: usize,
        version: Version,
    },
    HoverLinkActivated {
        href: String,
    },
    SignatureHelpRequested {
        id: quire_core::parameter_hints::SignatureHelpRequestId,
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
    DiagnosticAction {
        command_id: String,
        args: serde_json::Value,
    },
    CompletionRequested {
        id: quire_core::completion::CompletionRequestId,
        pos: usize,
        version: Version,
        trigger: quire_core::completion::CompletionTriggerKind,
        auto: bool,
        sources: Option<Vec<quire_core::SourceId>>,
    },
    CompletionResolveRequested {
        id: quire_core::completion::CompletionRequestId,
        source: quire_core::SourceId,
        index: usize,
        item: Arc<quire_core::completion::CompletionItem>,
    },
    CompletionCommand {
        command_id: String,
        args: serde_json::Value,
    },
    InlayHintsRequested {
        id: quire_core::inlay_hints::InlayHintsRequestId,
        ranges: Vec<Range<usize>>,
        version: Version,
    },
    InlayHintCommand {
        command_id: String,
        args: serde_json::Value,
    },
    InlayHintLocationActivated {
        location: quire_core::inlay_hints::InlayHintLocation,
    },
    SemanticTokensRequested {
        id: quire_core::semantic_tokens::SemanticTokensRequestId,
        version: Version,
        previous: Option<(quire_core::SourceId, String)>,
    },
    SemanticTokensReleased {
        source: quire_core::SourceId,
        result_id: Option<String>,
    },
    /// Copy or cut: the host puts `text` on the clipboard.
    CopyText {
        text: String,
    },
    /// Paste was pressed: the host reads the clipboard and calls [`PrestoEditor::paste`].
    PasteRequested,
    /// `actions.find` / the replace action: the host shows its find bar (a presto text field) and
    /// drives the search with [`PrestoEditor::set_find`].
    FindWidgetRequested {
        replace: bool,
        seed: Option<String>,
    },
    /// `closeFindWidget`.
    FindWidgetClosed,
}

/// What the editor produced since the host last took it.
#[derive(Debug, Default)]
pub struct EditorOutput {
    /// Transactions applied, in order.
    pub applied: Vec<Applied>,
    /// Commands resolved from input (for macros and recording), in order.
    pub commands: Vec<EditorCommand>,
    pub events: Vec<EditorEvent>,
}

/// View options the host sets.
#[derive(Clone, Debug)]
pub struct EditorOptions {
    pub keys: KeyHandling,
    pub os: OperatingSystem,
    pub read_only: bool,
    pub author: AuthorId,
    /// Show the gutter (line numbers, fold markers, diagnostic marks).
    pub gutter: bool,
    /// Rows laid out and recorded above and below the viewport, so short scrolls are compositor
    /// offsets only.
    pub overscan_rows: usize,
    /// Show inlay hints as injected text.
    pub inlay_hints: bool,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            keys: KeyHandling::default(),
            os: OperatingSystem::current(),
            read_only: false,
            author: AuthorId::USER,
            gutter: true,
            overscan_rows: 20,
            inlay_hints: true,
        }
    }
}

/// Vertical padding above the first row.
pub const PAD_Y: f32 = 2.0;
/// Width of the scrollbars.
pub const SCROLLBAR_W: f32 = 8.0;
/// The diagnostic mark column at the gutter's left edge.
pub const MARK_W: f32 = 8.0;
/// Clicks closer than this in time and space count as one multi-click (egui's defaults).
pub const DOUBLE_CLICK: Span = Span::from_millis(300);
pub const CLICK_SLOP: f32 = 6.0;

/// The editor's geometry for one layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub size: Size,
    pub scale: f32,
    pub row_h: f32,
    /// One column of the editor font.
    pub column: f32,
    pub ascent: f32,
    pub descent: f32,
    pub gutter_w: f32,
    /// Padding between the gutter and the text.
    pub pad_left: f32,
    pub wrap: Wrap,
}

impl Geom {
    /// The text area's left edge in local space.
    pub fn text_left(&self) -> f32 {
        self.gutter_w + self.pad_left
    }

    /// The text area in local space.
    pub fn text_rect(&self) -> Rect {
        Rect::new(self.gutter_w, PAD_Y, self.size.w, self.size.h)
    }

    pub fn gutter_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.gutter_w, self.size.h)
    }

    /// The fold marker column at the gutter's right edge.
    pub fn fold_col(&self) -> Rect {
        let w = fold_w(self.column);
        Rect::new(self.gutter_w - w, 0.0, self.gutter_w, self.size.h)
    }

    /// The baseline offset of a row from its top, rounded to device pixels.
    pub fn baseline(&self) -> f32 {
        let y = (self.row_h - (self.ascent + self.descent)) * 0.5 + self.ascent;
        (y * self.scale).round() / self.scale
    }
}

/// Width of the fold-marker column.
pub fn fold_w(column: f32) -> f32 {
    (column * 1.6).max(12.0)
}

/// Gutter width for `lines` lines: diagnostic mark, digits, padding, fold markers.
pub fn gutter_width(lines: usize, column: f32) -> f32 {
    let digits = (lines.max(1).ilog10() + 1).max(2) as f32;
    MARK_W + digits * column + column * 1.5 + fold_w(column)
}

/// What a press selects by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Char,
    Word,
    Line,
}

#[derive(Clone, Debug)]
struct Drag {
    anchor: Range<usize>,
    unit: Unit,
    /// Alt+Shift box selection: (anchor, head) as (line, visual column).
    column: Option<((usize, u32), (usize, u32))>,
}

/// A laid-out line: its display text and rows (content space y from the height index).
#[derive(Clone, Debug)]
pub struct LaidLine {
    pub line: usize,
    pub display: Arc<DisplayLine>,
    pub rows: Arc<LineRows>,
}

/// The engine. One per editor view; the [`Document`] and [`EditorView`] stay the host's.
pub struct PrestoEditor {
    pub theme: EditorTheme,
    pub options: EditorOptions,
    pub(crate) shaper: LineShaper,
    rows: GenCache<RowsKey, Arc<LineRows>>,
    pub(crate) heights: HeightIndex,
    heights_sig: u64,
    widths: LineWidths,
    /// The longest line in columns (the horizontal scroll extent).
    max_cols: u32,
    pub(crate) geom: Geom,
    pub(crate) scroll: Vec2,
    focused: bool,
    drag: Option<Drag>,
    last_press: Option<(Time, Point, u32)>,
    pending_chord: Option<Chord>,
    key_consumed: bool,
    /// Heads drawn at the end of a wrapped row, for one selection (its key).
    upstream: Option<(u64, Vec<usize>)>,
    scroll_to_caret: bool,
    pub(crate) last_interaction: Time,
    now: Time,
    out: EditorOutput,
    find_open: bool,
    /// The lines last laid out (viewport plus overscan).
    pub(crate) laid: Vec<LaidLine>,
    /// The scroll offset at the last full record ([`crate::layers`]): the recorded lines cover
    /// the overscan around it.
    pub(crate) recorded_at: Option<Vec2>,
    /// Bumped whenever recorded content may differ (text, selection, theme, layout, focus).
    pub(crate) content_rev: u64,
    content_key: u64,
    hover_ptr: Option<Point>,
    /// Popups of the last record, topmost last (pointer input goes to them first).
    pub(crate) popups: Vec<crate::scene::Popup>,
}

impl std::fmt::Debug for PrestoEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrestoEditor")
            .field("geom", &self.geom)
            .field("scroll", &self.scroll)
            .field("focused", &self.focused)
            .field("laid", &self.laid.len())
            .finish_non_exhaustive()
    }
}

/// The byte budget of the wrapped-rows cache.
pub const ROWS_CACHE_BUDGET: usize = 8 << 20;

impl PrestoEditor {
    pub fn new(
        fonts: Arc<presto_text::FontStore>,
        theme: EditorTheme,
        options: EditorOptions,
    ) -> Self {
        let shaper = LineShaper::new(fonts, &theme.font_family, theme.font_size);
        PrestoEditor {
            theme,
            options,
            shaper,
            rows: GenCache::new(ROWS_CACHE_BUDGET),
            heights: HeightIndex::uniform(1, 18.0),
            heights_sig: 0,
            widths: LineWidths::default(),
            max_cols: 0,
            geom: Geom {
                size: Size::ZERO,
                scale: 1.0,
                row_h: 18.0,
                column: 8.0,
                ascent: 10.0,
                descent: 3.0,
                gutter_w: 0.0,
                pad_left: 4.0,
                wrap: Wrap::NONE,
            },
            scroll: Vec2::ZERO,
            focused: false,
            drag: None,
            last_press: None,
            pending_chord: None,
            key_consumed: false,
            upstream: None,
            scroll_to_caret: false,
            last_interaction: Time::ZERO,
            now: Time::ZERO,
            out: EditorOutput::default(),
            find_open: false,
            laid: Vec::new(),
            recorded_at: None,
            content_rev: 0,
            content_key: 0,
            hover_ptr: None,
            popups: Vec::new(),
        }
    }

    pub fn geom(&self) -> &Geom {
        &self.geom
    }

    /// The scroll offset in points (x: horizontal; y: content-space top of the viewport).
    pub fn scroll(&self) -> Vec2 {
        self.scroll
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Takes what the editor produced since the last call.
    pub fn take_output(&mut self) -> EditorOutput {
        std::mem::take(&mut self.out)
    }

    /// False while a drag selection is in flight or the document is not settled.
    pub fn settled(&self, doc: &Document) -> bool {
        self.drag.is_none() && doc.is_settled()
    }

    /// Lines laid out by the last [`Self::layout`] (viewport plus overscan).
    pub fn laid_lines(&self) -> &[LaidLine] {
        &self.laid
    }

    /// The lines intersecting the viewport.
    pub fn visible_lines(&self) -> Range<usize> {
        let top = self.heights.line_at(self.scroll.y);
        let bottom = self.heights.line_at(self.scroll.y + self.geom.size.h);
        top..(bottom + 1).min(self.heights.len())
    }

    // ---- layout -------------------------------------------------------------------------------

    /// Lays the view out in a box of `size` points at `scale` device pixels per point: syncs the
    /// view, keeps the height index, applies a pending reveal, clamps the scroll and shapes the
    /// lines of the viewport plus overscan.
    pub fn layout(&mut self, doc: &Document, view: &mut EditorView, size: Size, scale: f32) {
        view.sync(doc);
        view.refresh_find(doc);
        self.shaper
            .set_font(&self.theme.font_family, self.theme.font_size);
        self.shaper.set_scale(scale);
        self.shaper.set_tab_size(doc.config().tab_size);
        let column = self.shaper.column_width();
        let (ascent, descent) = self.shaper.metrics();
        let n_lines = doc.text().len_lines().max(1);
        let gutter_w = if self.options.gutter {
            gutter_width(n_lines, column)
        } else {
            0.0
        };
        let pad_left = ((column * 0.5) * scale).round() / scale;
        let cfg = doc.config();
        let text_w = size.w - gutter_w - pad_left - SCROLLBAR_W - 2.0;
        let wrap = Wrap {
            width: (cfg.word_wrap == WordWrap::On).then_some(text_w.max(column)),
            indent: cfg.wrapping_indent,
            tab_size: cfg.tab_size,
            column,
        };
        let row_h = self.theme.row_height();
        self.geom = Geom {
            size,
            scale,
            row_h,
            column,
            ascent,
            descent,
            gutter_w,
            pad_left,
            wrap,
        };
        self.sync_heights(doc, view);
        self.scroll = self.view_scroll(doc, view);
        if let Some((pos, center)) = take_reveal(view, doc) {
            self.reveal(doc, view, pos, center);
        } else if std::mem::take(&mut self.scroll_to_caret) {
            let head = view.selection().primary().head;
            self.reveal(doc, view, head, false);
        }
        self.clamp_scroll();
        self.layout_viewport(doc, view);
        self.store_scroll(doc, view);
        view.set_visible_lines(self.visible_lines());
        let key = hash_one((
            doc.version(),
            view.selection_version(),
            view.folds(),
            view.preedit().map(|p| (&p.text, &p.active)),
            self.focused,
            hash_one(format!("{:?}", self.theme)),
            self.geom.size.w.to_bits(),
            self.geom.scale.to_bits(),
            view.find().map(|f| f.matches.len()),
        ));
        if key != self.content_key {
            self.content_key = key;
            self.content_rev += 1;
        }
    }

    /// Ends a frame of the shaping and row caches (call once per UI frame).
    pub fn end_frame(&mut self) {
        self.shaper.end_frame();
        self.rows.end_frame();
    }

    fn sync_heights(&mut self, doc: &Document, view: &EditorView) {
        let n_lines = doc.text().len_lines().max(1);
        let row_h = self.geom.row_h;
        let wrap = self.geom.wrap.width.map(f32::to_bits);
        self.widths.sync(doc);
        let splices = std::mem::take(&mut self.widths.splices);
        let rebuilt = std::mem::take(&mut self.widths.rebuilt);
        let sig = hash_one((
            row_h.to_bits(),
            view.folds(),
            wrap,
            self.geom.column.to_bits(),
        ));
        if wrap.is_some() && self.heights_sig == sig && !rebuilt && self.heights.len() != n_lines {
            for (start, removed, inserted) in splices {
                if start + removed > self.heights.len() {
                    break;
                }
                self.heights.splice(start..start + removed, inserted, row_h);
            }
        }
        let cols_per_row = self
            .geom
            .wrap
            .width
            .map(|w| ((w / self.geom.column).floor() as u32).max(1));
        if self.heights_sig != sig || self.heights.len() != n_lines {
            self.heights_sig = sig;
            self.heights = match cols_per_row {
                None => HeightIndex::uniform(n_lines, row_h),
                Some(c) => {
                    let widths = &self.widths;
                    HeightIndex::from_fn(n_lines, |l| {
                        widths.cols(l).div_ceil(c).max(1) as f32 * row_h
                    })
                }
            };
            for (a, b) in hidden_line_spans(view, doc) {
                for l in a..=b.min(n_lines - 1) {
                    self.heights.set(l, 0.0);
                }
            }
        } else if let Some(c) = cols_per_row {
            for l in std::mem::take(&mut self.widths.changed) {
                if l < self.heights.len() && self.heights.get(l) != 0.0 {
                    self.heights
                        .set(l, self.widths.cols(l).div_ceil(c).max(1) as f32 * row_h);
                }
            }
        }
        self.widths.changed.clear();
        self.max_cols = self.widths.max_cols();
    }

    /// Lays out line `line` (display text, shaping, rows) through the caches, and measures its
    /// rows into the height index.
    pub fn layout_line(&mut self, doc: &Document, view: &mut EditorView, line: usize) -> LaidLine {
        let display = Arc::new(lines::display_line(
            doc,
            view,
            line,
            self.options.inlay_hints,
        ));
        let shaped = self.shaper.shape(&display.text, &display.paints);
        let wrap = self.geom.wrap;
        let key = RowsKey {
            shape: self.shaper.key(&display.text, &display.paints),
            width_bits: wrap.width.map_or(u32::MAX, f32::to_bits),
            indent_bits: hash_one((wrap.indent, wrap.column.to_bits())) as u32,
        };
        let text = display.text.clone();
        let rows = self
            .rows
            .get_or_insert_with(key, || Arc::new(LineRows::new(&text, shaped, wrap)));
        if line < self.heights.len() && self.heights.get(line) != 0.0 {
            let h = rows.n_rows() as f32 * self.geom.row_h;
            if self.heights.get(line) != h {
                self.heights.set(line, h);
            }
        }
        LaidLine {
            line,
            display,
            rows,
        }
    }

    fn layout_viewport(&mut self, doc: &Document, view: &mut EditorView) {
        let over = self.options.overscan_rows as f32 * self.geom.row_h;
        let y0 = (self.scroll.y - over).max(0.0);
        let y1 = self.scroll.y + self.geom.size.h + over;
        let n = doc.text().len_lines();
        let mut line = self.heights.line_at(y0).min(n.saturating_sub(1));
        self.laid.clear();
        while line < n {
            let top = self.heights.y_of(line);
            if top > y1 {
                break;
            }
            if self.heights.get(line) != 0.0 {
                let l = self.layout_line(doc, view, line);
                self.laid.push(l);
            }
            line += 1;
        }
    }

    /// Whether a scroll to `scroll` stays inside what the last record covered (a compositor offset
    /// is then enough).
    pub fn covers(&self, scroll: Vec2) -> bool {
        let Some(at) = self.recorded_at else {
            return false;
        };
        let over = self.options.overscan_rows as f32 * self.geom.row_h;
        scroll.y >= (at.y - over).max(0.0)
            && scroll.y + self.geom.size.h <= at.y + self.geom.size.h + over
    }

    /// The laid-out line `line`, if it is in the viewport plus overscan.
    pub fn laid(&self, line: usize) -> Option<&LaidLine> {
        let i = self.laid.binary_search_by_key(&line, |l| l.line).ok()?;
        Some(&self.laid[i])
    }

    fn laid_or_layout(&mut self, doc: &Document, view: &mut EditorView, line: usize) -> LaidLine {
        match self.laid(line) {
            Some(l) => l.clone(),
            None => self.layout_line(doc, view, line),
        }
    }

    // ---- scroll -------------------------------------------------------------------------------

    fn view_scroll(&self, doc: &Document, view: &EditorView) -> Vec2 {
        let s = view.scroll();
        let t = doc.text();
        let line = t.byte_to_line(s.anchor.min(t.len_bytes()));
        Vec2::new(s.horizontal, self.heights.y_of(line) + s.offset)
    }

    fn store_scroll(&self, doc: &Document, view: &mut EditorView) {
        let px = self.scroll;
        let line = self
            .heights
            .line_at(px.y)
            .min(doc.text().len_lines().saturating_sub(1));
        let s = quire_core::Scroll {
            anchor: doc.text().line_to_byte(line),
            offset: px.y - self.heights.y_of(line),
            horizontal: px.x,
        };
        if view.scroll() != s {
            view.set_scroll(s);
        }
    }

    /// The content size (scroll extent) in points.
    pub fn content_size(&self) -> Size {
        let w = if self.geom.wrap.width.is_some() {
            self.geom.size.w - self.geom.text_left()
        } else {
            self.max_cols as f32 * self.geom.column + self.geom.column * 2.0
        };
        // VS Code's scrollBeyondLastLine: the last line can scroll to the top.
        let h = self.heights.total() + (self.geom.size.h - self.geom.row_h - PAD_Y).max(0.0);
        Size::new(w, h)
    }

    fn clamp_scroll(&mut self) {
        let c = self.content_size();
        let view_w = self.geom.size.w - self.geom.text_left();
        self.scroll.x = self.scroll.x.clamp(0.0, (c.w - view_w).max(0.0));
        self.scroll.y = self.scroll.y.clamp(0.0, (c.h - self.geom.size.h).max(0.0));
    }

    /// Scrolls to `offset` (clamped) as native state; returns whether it moved. A move inside the
    /// recorded range ([`Self::covers`]) needs only a compositor offset.
    pub fn set_scroll(&mut self, doc: &Document, view: &mut EditorView, offset: Vec2) -> bool {
        let before = self.scroll;
        self.scroll = offset;
        self.clamp_scroll();
        if self.scroll == before {
            return false;
        }
        self.store_scroll(doc, view);
        if view.hover().is_visible() {
            // VS Code `_onEditorScrollChanged`: scrolling hides the hover.
            view.hide_hover();
        }
        true
    }

    /// A wheel or touchpad delta; returns whether the editor scrolled.
    pub fn wheel(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        delta: ScrollDelta,
        mods: Modifiers,
    ) -> bool {
        let mut d = match delta {
            ScrollDelta::Pixels(v) => v,
            ScrollDelta::Lines(v) => v * (self.geom.row_h * 3.0),
            ScrollDelta::Pages(v) => Vec2::new(v.x * self.geom.size.w, v.y * self.geom.size.h),
        };
        if mods.contains(Modifiers::SHIFT) && d.x == 0.0 {
            d = Vec2::new(d.y, 0.0);
        }
        let to = self.scroll - d;
        self.set_scroll(doc, view, to)
    }

    /// Scrolls so `pos` is visible (`center`: in the middle).
    pub fn reveal(&mut self, doc: &Document, view: &mut EditorView, pos: usize, center: bool) {
        let (rect, _) = self.caret_content_rect(doc, view, pos, false);
        let h = self.geom.size.h - PAD_Y;
        let w = self.geom.size.w - self.geom.text_left() - SCROLLBAR_W;
        if center {
            self.scroll.y = rect.min.y - (h - self.geom.row_h) * 0.5;
        } else if rect.min.y < self.scroll.y {
            self.scroll.y = rect.min.y;
        } else if rect.max.y > self.scroll.y + h {
            self.scroll.y = rect.max.y - h;
        }
        if self.geom.wrap.width.is_none() {
            let margin = self.geom.column * 2.0;
            if rect.min.x < self.scroll.x {
                self.scroll.x = (rect.min.x - margin).max(0.0);
            } else if rect.max.x > self.scroll.x + w {
                self.scroll.x = rect.max.x - w + margin;
            }
        }
        self.clamp_scroll();
    }

    // ---- geometry -----------------------------------------------------------------------------

    /// The caret rect of model byte `pos` in content space, and its (line, row).
    pub fn caret_content_rect(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        pos: usize,
        upstream: bool,
    ) -> (Rect, (usize, usize)) {
        let t = doc.text();
        let pos = pos.min(t.len_bytes());
        let line = t.byte_to_line(pos);
        let col = pos - t.line_to_byte(line);
        let l = self.laid_or_layout(doc, view, line);
        let d = l.display.to_display(col);
        let (row, x) = l.rows.locate(d, upstream);
        let y = self.heights.y_of(line) + row as f32 * self.geom.row_h;
        let w = self.theme.caret_width;
        (Rect::new(x, y, x + w, y + self.geom.row_h), (line, row))
    }

    /// Local space → content space.
    pub fn to_content(&self, p: Point) -> Point {
        Point::new(
            p.x - self.geom.text_left() + self.scroll.x,
            p.y - PAD_Y + self.scroll.y,
        )
    }

    /// Content space → local space.
    pub fn to_local(&self, p: Point) -> Point {
        Point::new(
            p.x + self.geom.text_left() - self.scroll.x,
            p.y + PAD_Y - self.scroll.y,
        )
    }

    /// (line, row) at local y.
    fn line_row_at(&self, doc: &Document, y: f32) -> (usize, usize) {
        let cy = y - PAD_Y + self.scroll.y;
        let line = self
            .heights
            .line_at(cy)
            .min(doc.text().len_lines().saturating_sub(1));
        let row = ((cy - self.heights.y_of(line)) / self.geom.row_h)
            .floor()
            .max(0.0) as usize;
        (line, row)
    }

    /// The model byte under local point `p`, and whether it is upstream (end of a wrapped row).
    pub fn pos_at(&mut self, doc: &Document, view: &mut EditorView, p: Point) -> (usize, bool) {
        let (line, row) = self.line_row_at(doc, p.y);
        let l = self.laid_or_layout(doc, view, line);
        let x = p.x - self.geom.text_left() + self.scroll.x;
        let (d, up) = l.rows.hit(row, x);
        (doc.text().line_to_byte(line) + l.display.to_model(d), up)
    }

    /// The injected piece under local point `p`.
    fn piece_at(&mut self, doc: &Document, view: &mut EditorView, p: Point) -> Option<PieceKind> {
        let (line, row) = self.line_row_at(doc, p.y);
        let l = self.laid_or_layout(doc, view, line);
        let x = p.x - self.geom.text_left() + self.scroll.x;
        let (d, _) = l.rows.hit(row, x);
        // the boundary nearest the point can be either side of the glyph under it
        [d, d.saturating_sub(1)].into_iter().find_map(|d| {
            let piece = l.display.piece_at(d)?;
            let x0 = l.rows.x_in_row(row, piece.display.start);
            let x1 = l.rows.x_in_row(row, piece.display.end);
            (x >= x0 && x < x1).then_some(piece.kind)
        })
    }

    /// (line, visual column) under local point `p`, for box selection: past the line's end the
    /// column keeps counting whole columns, as VS Code's mouse column selection does.
    fn line_col_at(&mut self, doc: &Document, view: &mut EditorView, p: Point) -> (usize, u32) {
        let (line, row) = self.line_row_at(doc, p.y);
        let l = self.laid_or_layout(doc, view, line);
        let x = p.x - self.geom.text_left() + self.scroll.x;
        let end = l.rows.row_width(row.min(l.rows.n_rows() - 1));
        let s = lines::line_string(doc.text(), line);
        let tab = doc.config().tab_size;
        if x <= end {
            let (d, _) = l.rows.hit(row, x);
            let col = l.display.to_model(d);
            (
                line,
                quire_core::columns::visual_col(&s, col.min(s.len()), tab),
            )
        } else {
            let past = ((x - end) / self.geom.column).round().max(0.0) as u32;
            (
                line,
                quire_core::columns::visual_col(&s, s.len(), tab) + past,
            )
        }
    }

    /// Whether the caret at `head` is drawn at the end of the previous wrapped row.
    pub fn upstream(&self, doc: &Document, view: &EditorView, head: usize) -> bool {
        match &self.upstream {
            Some((key, heads)) if *key == selection_key(doc, view) => heads.contains(&head),
            _ => false,
        }
    }

    fn set_upstream(&mut self, doc: &Document, view: &EditorView, heads: Vec<usize>) {
        self.upstream = (!heads.is_empty()).then(|| (selection_key(doc, view), heads));
    }

    fn page_lines(&self) -> u32 {
        ((self.geom.size.h / self.geom.row_h).floor() as u32)
            .saturating_sub(1)
            .max(1)
    }

    // ---- commands -----------------------------------------------------------------------------

    fn touch(&mut self) {
        self.last_interaction = self.now;
    }

    fn time_ms(&self) -> u64 {
        self.now.0 / 1_000_000
    }

    fn push_applied(&mut self, a: Applied) {
        self.scroll_to_caret = true;
        self.out.applied.push(a);
    }

    /// Records a transaction applied outside the editor's own input paths (IME client, a11y) in
    /// the output, and reveals the caret.
    pub fn note_applied(&mut self, a: Applied) {
        self.push_applied(a);
    }

    /// Dispatches one editor command: Save and host-mode Undo/Redo become events; everything else
    /// goes through the core and is applied now.
    pub fn dispatch(&mut self, doc: &mut Document, view: &mut EditorView, cmd: EditorCommand) {
        self.touch();
        if self.wrapped_motion(doc, view, &cmd) {
            return;
        }
        let cmd = with_page_lines(cmd, self.page_lines());
        self.out.commands.push(cmd.clone());
        let host_undo = matches!(doc.undo_mode(), UndoMode::Host);
        match &cmd {
            EditorCommand::Save => return self.out.events.push(EditorEvent::SaveRequested),
            EditorCommand::Undo if host_undo => {
                return self.out.events.push(EditorEvent::UndoRequested);
            }
            EditorCommand::Redo if host_undo => {
                return self.out.events.push(EditorEvent::RedoRequested);
            }
            EditorCommand::CloseFind => {
                self.find_open = false;
                self.out.events.push(EditorEvent::FindWidgetClosed);
            }
            _ => {}
        }
        if self.options.read_only && changes_text(&cmd) {
            return;
        }
        // A commit character accepts the focused suggestion first (VS Code
        // CommitCharacterController), then types.
        if let EditorCommand::Type { text } = &cmd
            && view.suggest().accepts_on(doc, text)
        {
            self.dispatch(doc, view, EditorCommand::AcceptSelectedSuggestion);
        }
        let time = self.time_ms();
        if let Ok(ex) = doc.execute_as(view, &cmd, Some(time), self.options.author) {
            if let Some(a) = ex.applied {
                self.push_applied(a);
            }
            match ex.request {
                Some(HostRequest::Save) => self.out.events.push(EditorEvent::SaveRequested),
                Some(HostRequest::Undo) => self.out.events.push(EditorEvent::UndoRequested),
                Some(HostRequest::Redo) => self.out.events.push(EditorEvent::RedoRequested),
                None => {}
            }
        }
    }

    /// Under soft wrap, Up/Down/PageUp/PageDown and Home/End move by visual row: resolved through
    /// quire-text's motion over [`crate::visual::WrappedLines`] and recorded as a selection.
    fn wrapped_motion(
        &mut self,
        doc: &mut Document,
        view: &mut EditorView,
        cmd: &EditorCommand,
    ) -> bool {
        use quire_core::Motion;
        use quire_text::command::{Dir, Unit as TUnit};
        if self.geom.wrap.width.is_none() {
            return false;
        }
        let EditorCommand::CursorMove { to, select } = cmd else {
            return false;
        };
        let (unit, dir) = match to {
            Motion::Up => (TUnit::VisualLine, Dir::Back),
            Motion::Down => (TUnit::VisualLine, Dir::Forward),
            Motion::PageUp { .. } => (TUnit::Page, Dir::Back),
            Motion::PageDown { .. } => (TUnit::Page, Dir::Forward),
            Motion::LineStart | Motion::LineEnd => {
                let end = matches!(to, Motion::LineEnd);
                let sel = self.row_edges(doc, view, end, *select);
                self.out.commands.push(EditorCommand::SetSelection {
                    selection: sel.clone(),
                });
                self.set_selection(doc, view, sel);
                if end {
                    let heads = view.selection().ranges().iter().map(|r| r.head).collect();
                    self.set_upstream(doc, view, heads);
                }
                return true;
            }
            _ => return false,
        };
        let page = self.page_lines() as usize;
        let before = view.selection().clone();
        let sel = {
            let doc: &Document = doc;
            let rows = crate::visual::WrappedLines::new(self, doc, view, page);
            quire_text::ops::motion::move_selection(
                doc.text(),
                &before,
                unit,
                dir,
                *select,
                doc.edit_rules(),
                &rows,
            )
        };
        self.out.commands.push(EditorCommand::SetSelection {
            selection: sel.clone(),
        });
        self.set_selection(doc, view, sel);
        true
    }

    /// Every range moved to the start (or end) of its visual row.
    fn row_edges(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        end: bool,
        select: bool,
    ) -> Selection {
        let sel = view.selection().clone();
        let t = doc.text();
        let mut out = Vec::new();
        for r in sel.ranges() {
            let line = t.byte_to_line(r.head.min(t.len_bytes()));
            let start = t.line_to_byte(line);
            let l = self.laid_or_layout(doc, view, line);
            let up = self.upstream(doc, view, r.head);
            let k = l.rows.row_of(l.display.to_display(r.head - start), up);
            let row = &l.rows.rows[k];
            let d = if end { row.range.end } else { row.range.start };
            let head = start + l.display.to_model(d);
            out.push(SelRange::new(if select { r.anchor } else { head }, head));
        }
        let primary = sel.primary_index();
        Selection::new(out, primary)
    }

    fn set_selection(&mut self, doc: &mut Document, view: &mut EditorView, sel: Selection) {
        let tr = Transaction::select(doc.version(), doc.text().len_bytes(), sel)
            .with_origin(Origin::Command("setSelection".into()))
            .with_author(self.options.author);
        if let Ok(a) = doc.apply(tr, view) {
            self.push_applied(a);
        }
    }

    /// Pastes `text` (the host's answer to [`EditorEvent::PasteRequested`], or a drop).
    pub fn paste(&mut self, doc: &mut Document, view: &mut EditorView, text: &str) {
        self.dispatch(
            doc,
            view,
            EditorCommand::Paste {
                text: text.to_owned(),
                per_cursor: true,
            },
        );
    }

    /// Opens find with `query` (empty clears it) and moves to the nearest match.
    pub fn set_find(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        query: Option<quire_core::FindQuery>,
    ) {
        self.find_open = query.is_some();
        view.set_find(doc, query);
    }

    fn copy(&mut self, doc: &Document, view: &EditorView) -> bool {
        let text = doc.text();
        let ranges = view.selection().ranges();
        let parts: Vec<String> = if ranges.iter().all(|r| r.is_empty()) {
            if !doc.config().empty_selection_clipboard {
                return false;
            }
            let mut ls: Vec<usize> = ranges
                .iter()
                .map(|r| text.byte_to_line(r.head.min(text.len_bytes())))
                .collect();
            ls.dedup();
            ls.into_iter()
                .map(|l| lines::line_string(text, l) + "\n")
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
        let joined = if ranges.iter().all(|r| r.is_empty()) {
            parts.concat()
        } else {
            parts.join("\n")
        };
        self.out.events.push(EditorEvent::CopyText { text: joined });
        true
    }

    fn widget_action(&mut self, doc: &mut Document, view: &mut EditorView, a: WidgetAction) {
        self.touch();
        match a {
            WidgetAction::OpenFind | WidgetAction::OpenReplace => {
                let sel = view.selection().primary();
                let seed = (!sel.is_empty())
                    .then(|| doc.text().slice(sel.range()).to_string())
                    .filter(|t| !t.contains('\n'));
                self.find_open = true;
                self.out.events.push(EditorEvent::FindWidgetRequested {
                    replace: a == WidgetAction::OpenReplace,
                    seed,
                });
            }
            WidgetAction::CloseFind => {
                self.find_open = false;
                view.set_find(doc, None);
                self.out.events.push(EditorEvent::FindWidgetClosed);
            }
        }
    }

    fn when_context(&self, doc: &Document, view: &EditorView) -> WhenContext {
        let st = view.suggest();
        let snippet = view.snippet();
        let focused = st.focused().is_some();
        WhenContext {
            find_widget_visible: self.find_open,
            multiple_selections: view.selection().len() > 1,
            suggest_widget_visible: st.is_visible(),
            suggest_widget_has_focused_suggestion: focused,
            suggest_widget_multiple_suggestions: st.items().len() > 1,
            accept_suggestion_on_enter: doc.config().suggest.accept_suggestion_on_enter
                != quire_core::completion::AcceptSuggestionOnEnter::Off,
            suggestion_makes_text_edit: !focused || st.makes_text_edit(doc, view.selection()),
            in_snippet_mode: snippet.is_some(),
            has_next_tabstop: snippet.is_some_and(|s| s.has_next()),
            has_prev_tabstop: snippet.is_some_and(|s| s.has_prev()),
            parameter_hints_visible: view.parameter_hints().is_visible(),
            parameter_hints_multiple_signatures: view.parameter_hints().multiple_signatures(),
            hover_focused: view.hover().is_focused(),
        }
    }

    // ---- input --------------------------------------------------------------------------------

    /// Focus moved to or from the editor.
    pub fn focus(&mut self, doc: &Document, view: &mut EditorView, focused: bool) {
        if focused == self.focused {
            return;
        }
        self.focused = focused;
        self.out.events.push(EditorEvent::FocusChanged(focused));
        self.touch();
        view.set_focused(focused);
        self.pending_chord = None;
        self.key_consumed = false;
        if !focused {
            if view.preedit().is_some() {
                view.set_preedit(None);
            }
            // VS Code cancels suggest when the editor loses focus.
            view.cancel_suggest();
            let _ = doc;
        }
    }

    /// A key event (pressed or released). Returns whether the editor consumed it. Text the key
    /// produces types unless a binding consumed the key (VS Code: a matched keybinding prevents the
    /// keypress's default); AltGr text entry stays unbound and types.
    pub fn key(&mut self, doc: &mut Document, view: &mut EditorView, ev: &KeyEvent) -> bool {
        self.now = self.now.max(self.now);
        if ev.state != KeyState::Pressed {
            return false;
        }
        let Some(key) = ev.key else {
            return self.type_key_text(doc, view, ev);
        };
        if view.preedit().is_some() {
            // Keys belong to the composition (presto routes printable ones to the IME first);
            // Escape cancels the preedit only.
            if key == Key::Escape {
                view.set_preedit(None);
                return true;
            }
            return false;
        }
        let mods = ev.modifiers;
        let primary = mods.primary(self.options.os == OperatingSystem::Mac);
        let only_primary =
            primary && !mods.contains(Modifiers::SHIFT) && !mods.contains(Modifiers::ALT);
        // The clipboard keys egui turned into Copy/Cut/Paste events.
        let clip = match key {
            Key::Copy => Some(Clip::Copy),
            Key::Cut => Some(Clip::Cut),
            Key::Paste => Some(Clip::Paste),
            Key::C if only_primary => Some(Clip::Copy),
            Key::X if only_primary => Some(Clip::Cut),
            Key::V if only_primary => Some(Clip::Paste),
            Key::Insert if mods == Modifiers::CTRL => Some(Clip::Copy),
            Key::Insert if mods == Modifiers::SHIFT => Some(Clip::Paste),
            Key::Delete if mods == Modifiers::SHIFT => Some(Clip::Cut),
            _ => None,
        };
        if let Some(c) = clip {
            match c {
                Clip::Copy => {
                    self.copy(doc, view);
                }
                Clip::Cut => {
                    if self.copy(doc, view) {
                        self.dispatch(doc, view, EditorCommand::Cut);
                    }
                }
                Clip::Paste => self.out.events.push(EditorEvent::PasteRequested),
            }
            return true;
        }
        let consumed = match &self.options.keys {
            KeyHandling::Builtin(map) => {
                let map = map.clone();
                self.handle_key(doc, view, &map, key, mods)
            }
            KeyHandling::Host => false,
        };
        if consumed {
            return true;
        }
        self.type_key_text(doc, view, ev)
    }

    fn type_key_text(&mut self, doc: &mut Document, view: &mut EditorView, ev: &KeyEvent) -> bool {
        match ev.text.as_deref() {
            Some(t)
                if !t.is_empty()
                    && !t.chars().any(char::is_control)
                    && view.preedit().is_none() =>
            {
                let ctrl_only = ev.modifiers.contains(Modifiers::CTRL)
                    && !ev.modifiers.contains(Modifiers::ALT_GR);
                if ctrl_only && !ev.modifiers.contains(Modifiers::ALT) {
                    return false;
                }
                self.dispatch(doc, view, EditorCommand::Type { text: t.to_owned() });
                true
            }
            _ => false,
        }
    }

    /// Typed text from a source other than a key event (the platform's text input, a11y
    /// `ReplaceSelectedText`).
    pub fn type_text(&mut self, doc: &mut Document, view: &mut EditorView, text: &str) {
        if view.preedit().is_none() && !text.is_empty() {
            self.dispatch(
                doc,
                view,
                EditorCommand::Type {
                    text: text.to_owned(),
                },
            );
        }
    }

    /// One key press through the keymap, with the pending first chord of a two-chord binding.
    fn handle_key(
        &mut self,
        doc: &mut Document,
        view: &mut EditorView,
        map: &Keymap,
        key: Key,
        mods: Modifiers,
    ) -> bool {
        let pending = self.pending_chord.take();
        if pending.is_some() && key == Key::Escape {
            return true;
        }
        let ctx = self.when_context(doc, view);
        let lookup = map.lookup_seq(self.options.os, pending, key, mods, &ctx);
        let hover_cmd = matches!(&lookup, KeyLookup::Bound(b) if is_hover_command(&b.action));
        if view.hover().is_focused() {
            if hover_cmd
                && let KeyLookup::Bound(b) = &lookup
                && let KeyAction::Command(c) = b.action.clone()
            {
                self.dispatch(doc, view, c);
            } else if key == Key::Escape {
                view.hide_hover();
            } else if let KeyLookup::Prefix(c) = lookup {
                self.pending_chord = Some(c);
            }
            return true;
        }
        if view.hover().is_visible() && !hover_cmd && !matches!(lookup, KeyLookup::Prefix(_)) {
            view.hide_hover();
        }
        match lookup {
            KeyLookup::Bound(b) => match b.action.clone() {
                KeyAction::Command(c) => self.dispatch(doc, view, c),
                KeyAction::Widget(a) => self.widget_action(doc, view, a),
            },
            KeyLookup::Prefix(c) => self.pending_chord = Some(c),
            KeyLookup::Unbound => return pending.is_some(),
        }
        self.key_consumed = true;
        true
    }

    /// An IME event (presto's: preedit cursor and surrounding deletes in bytes).
    pub fn ime(&mut self, doc: &mut Document, view: &mut EditorView, ev: &ImeEvent) {
        self.touch();
        match ev {
            ImeEvent::Preedit { text, cursor } => {
                if text.is_empty() {
                    if view.preedit().is_some() {
                        view.set_preedit(None);
                    }
                    return;
                }
                if view.preedit().is_none()
                    && !view.selection().primary().is_empty()
                    && !self.options.read_only
                {
                    // Composition replaces the selection.
                    self.dispatch(
                        doc,
                        view,
                        EditorCommand::Type {
                            text: String::new(),
                        },
                    );
                }
                let active = cursor.map(|(a, b)| a.min(text.len())..b.min(text.len()));
                view.set_preedit(Some(quire_core::Preedit {
                    text: text.clone(),
                    active,
                }));
                self.scroll_to_caret = true;
            }
            ImeEvent::Commit(text) => {
                if view.preedit().is_some() {
                    view.set_preedit(None);
                }
                if !text.is_empty() {
                    self.dispatch(doc, view, composition_cmd(text, 0, 0));
                }
            }
            ImeEvent::DeleteSurrounding { before, after } => {
                // presto counts bytes, core counts chars: convert around the primary caret.
                let t = doc.text();
                let head = view.selection().primary().head.min(t.len_bytes());
                let b = t.byte_to_char(head) - t.byte_to_char(head.saturating_sub(*before));
                let a = t.byte_to_char((head + after).min(t.len_bytes())) - t.byte_to_char(head);
                if a + b > 0 {
                    self.dispatch(doc, view, composition_cmd("", b, a));
                }
            }
            ImeEvent::Enabled | ImeEvent::Disabled => {}
        }
    }

    /// The time of the input being handled (undo grouping, multi-click, caret blink).
    pub fn set_now(&mut self, now: Time) {
        self.now = now;
    }

    /// A pointer press. `p.local` is in the editor's local space; `p.count` is ignored (the editor
    /// counts clicks itself, as egui_quire, cycling 1-2-3).
    pub fn pointer_down(
        &mut self,
        doc: &mut Document,
        view: &mut EditorView,
        p: &PointerInput,
    ) -> bool {
        let at = p.local;
        let mods = p.modifiers;
        let mac = self.options.os == OperatingSystem::Mac;
        let primary_mod = mods.primary(mac);
        match p.button {
            Some(MouseButton::Secondary) => {
                let (pos, _) = self.pos_at(doc, view, at);
                self.out.events.push(EditorEvent::ContextMenu { pos });
                return true;
            }
            Some(MouseButton::Primary) | None => {}
            Some(_) => return false,
        }
        self.touch();
        if let Some(popup) = self
            .popups
            .iter()
            .rev()
            .copied()
            .find(|p| p.rect().contains(at))
        {
            if let crate::scene::Popup::Suggest { rect, top } = popup {
                let row = ((at.y - rect.min.y - crate::scene::POPUP_PAD) / self.geom.row_h)
                    .floor()
                    .max(0.0) as usize;
                view.suggest_focus_row(top + row);
                self.dispatch(doc, view, EditorCommand::AcceptSelectedSuggestion);
            }
            return true;
        }
        let in_text = self.geom.text_rect().contains(at);
        let in_gutter = self.geom.gutter_rect().contains(at);
        if !in_text && !in_gutter {
            return false;
        }
        let (pos, up) = self.pos_at(doc, view, at);
        let shift = mods.contains(Modifiers::SHIFT);
        let alt = mods.contains(Modifiers::ALT);
        // Inlay hints: Ctrl+click runs a label part's command or opens its location.
        let under_hint = if in_text {
            self.piece_at(doc, view, at)
        } else {
            None
        };
        if primary_mod
            && !shift
            && !alt
            && let Some(PieceKind::Hint { hint, piece }) = under_hint
            && self.hint_activate(view, hint, piece)
        {
            self.last_press = None;
            return true;
        }
        if primary_mod && !shift && !alt && in_text {
            let range = word_range_at(doc, pos);
            self.out.events.push(EditorEvent::LinkActivated {
                range,
                version: doc.version(),
            });
            self.last_press = None;
            return true;
        }
        if self.options.gutter && self.geom.fold_col().contains(at) {
            let (line, _) = self.line_row_at(doc, at.y);
            if !fold_headers(doc, line..line + 1).is_empty() {
                if let Some(a) = toggle_fold_at_line(doc, view, line, self.options.author) {
                    self.push_applied(a);
                }
                self.last_press = None;
                return true;
            }
        }
        if alt && shift && !primary_mod && in_text {
            let lc = self.line_col_at(doc, view, at);
            self.drag = Some(Drag {
                anchor: pos..pos,
                unit: Unit::Char,
                column: Some((lc, lc)),
            });
            self.dispatch(doc, view, column_select_cmd(lc, lc));
            self.last_press = None;
            return true;
        }
        if alt && !shift && !primary_mod && in_text {
            let sel = view.selection().push(SelRange::point(pos));
            self.set_selection(doc, view, sel);
            self.last_press = None;
            return true;
        }
        let count = match self.last_press {
            Some((t, q, c)) if self.now - t <= DOUBLE_CLICK && (q - at).length() < CLICK_SLOP => {
                c % 3 + 1
            }
            _ => 1,
        };
        self.last_press = Some((self.now, at, count));
        let unit = if in_gutter {
            Unit::Line
        } else {
            match count {
                1 => Unit::Char,
                2 => Unit::Word,
                _ => Unit::Line,
            }
        };
        let sel = view.selection().primary();
        let (anchor_range, head) = if shift && unit == Unit::Char {
            (sel.anchor..sel.anchor, pos)
        } else {
            let r = unit_range(doc, pos, unit);
            (r.clone(), r.end)
        };
        self.drag = Some(Drag {
            anchor: anchor_range.clone(),
            unit,
            column: None,
        });
        let new = if anchor_range.is_empty() || shift {
            Selection::single(anchor_range.start, head)
        } else {
            Selection::single(anchor_range.start, anchor_range.end)
        };
        self.set_selection(doc, view, new);
        if up && unit == Unit::Char {
            self.set_upstream(doc, view, vec![pos]);
        }
        // VS Code `_installDblClickGesture`: a double click on an inlay hint applies its edits.
        if count == 2
            && !self.options.read_only
            && let Some(PieceKind::Hint { hint, .. }) = under_hint
            && let Some(tr) = view.inlay_hint_edits(doc, hint)
        {
            self.drag = None;
            if let Ok(a) = doc.apply(tr.with_author(self.options.author), view) {
                self.push_applied(a);
            }
        }
        true
    }

    fn hint_activate(&mut self, view: &EditorView, hint: usize, piece: usize) -> bool {
        let Some(h) = view.inlay_hints().hint(hint) else {
            return false;
        };
        let Some(part) = h.label.get(piece) else {
            return false;
        };
        if let Some(location) = part.location.clone() {
            self.out
                .events
                .push(EditorEvent::InlayHintLocationActivated { location });
            return true;
        }
        if let Some(c) = &part.command {
            self.out.events.push(EditorEvent::InlayHintCommand {
                command_id: c.id.clone(),
                args: serde_json::from_str(&c.args_json).unwrap_or(serde_json::Value::Null),
            });
            return true;
        }
        false
    }

    /// The pointer moved (button held: drag selection, autoscroll; else hover).
    pub fn pointer_move(
        &mut self,
        doc: &mut Document,
        view: &mut EditorView,
        p: &PointerInput,
    ) -> bool {
        let at = p.local;
        if p.buttons & 1 == 0 {
            self.drag = None;
            self.hover_move(doc, view, at);
            return false;
        }
        let Some(drag) = self.drag.clone() else {
            return false;
        };
        if let Some((anchor, head)) = drag.column {
            let lc = self.line_col_at(doc, view, at);
            if lc != head {
                if let Some(d) = self.drag.as_mut() {
                    d.column = Some((anchor, lc));
                }
                self.dispatch(doc, view, column_select_cmd(anchor, lc));
            }
            return true;
        }
        let (pos, up) = self.pos_at(doc, view, at);
        let r = unit_range(doc, pos, drag.unit);
        let a = drag.anchor;
        let new = if r.start < a.start {
            Selection::single(a.end, r.start)
        } else {
            Selection::single(a.start, r.end.max(a.end))
        };
        let cur = view.selection().primary();
        if cur.range() != new.primary().range() || cur.head != new.primary().head {
            self.set_selection(doc, view, new);
            if up && drag.unit == Unit::Char {
                self.set_upstream(doc, view, vec![pos]);
            }
        }
        // Autoscroll when dragging past the edges.
        let t = self.geom.text_rect();
        let step = self.geom.row_h * 3.0;
        let dy = if at.y < t.min.y {
            -(t.min.y - at.y).min(step)
        } else if at.y > t.max.y {
            (at.y - t.max.y).min(step)
        } else {
            0.0
        };
        if dy != 0.0 {
            let to = self.scroll + Vec2::new(0.0, dy);
            self.set_scroll(doc, view, to);
        }
        true
    }

    /// The press ended.
    pub fn pointer_up(
        &mut self,
        _doc: &mut Document,
        _view: &mut EditorView,
        _p: &PointerInput,
    ) -> bool {
        self.drag.take().is_some()
    }

    /// The pointer left the editor.
    pub fn pointer_leave(&mut self, _doc: &Document, view: &mut EditorView) {
        self.hover_ptr = None;
        if view.hover().is_visible() && !view.hover().is_focused() {
            view.hide_hover();
        }
    }

    /// The mouse side of VS Code's `ContentHoverController`, simplified: resting over text starts
    /// a delayed hover at the position under the pointer.
    fn hover_move(&mut self, doc: &mut Document, view: &mut EditorView, at: Point) {
        use quire_core::hover::{HoverAnchor, HoverStartMode, HoverStartSource};
        self.hover_ptr = Some(at);
        if !self.geom.text_rect().contains(at) {
            return;
        }
        let (pos, _) = self.pos_at(doc, view, at);
        let now = self.time_ms();
        view.start_hover(
            doc,
            Some(HoverAnchor { range: pos..pos }),
            HoverStartMode::Delayed,
            HoverStartSource::Mouse,
            false,
            false,
            Some(now),
        );
    }

    /// Runs the view's timers (hover, parameter hints, semantic tokens) at `now` and drains what
    /// they ask of the host into events. Returns the next deadline.
    pub fn tick(&mut self, doc: &mut Document, view: &mut EditorView, now: Time) -> Option<Time> {
        self.now = now;
        let ms = self.time_ms();
        view.tick(doc, ms);
        doc.tick_semantic_tokens(ms);
        self.drain(doc, view);
        let semantic = doc.semantic_tokens();
        let deadline = if semantic.needs_tick() {
            Some(ms)
        } else {
            [view.next_deadline(), semantic.next_deadline()]
                .into_iter()
                .flatten()
                .min()
        };
        let blink = self.next_blink(now);
        [deadline.map(|d| Time(d.max(ms) * 1_000_000)), blink]
            .into_iter()
            .flatten()
            .min()
    }

    /// What the view's hover, hints, suggest session and the document's semantic tokens want
    /// from the host, as events.
    pub fn drain(&mut self, doc: &mut Document, view: &mut EditorView) {
        use quire_core::completion::SuggestOutput as O;
        use quire_core::hover::HoverOutput;
        use quire_core::parameter_hints::ParameterHintsOutput;
        use quire_core::semantic_tokens::SemanticTokensOutput as S;
        for o in view.take_hover_outputs() {
            let HoverOutput::Request(r) = o;
            self.out.events.push(EditorEvent::HoverAt {
                id: r.id,
                pos: r.pos,
                version: r.version,
            });
        }
        for o in doc.take_semantic_tokens_outputs() {
            self.out.events.push(match o {
                S::Request(r) => EditorEvent::SemanticTokensRequested {
                    id: r.id,
                    version: r.version,
                    previous: r.previous,
                },
                S::Release { source, result_id } => {
                    EditorEvent::SemanticTokensReleased { source, result_id }
                }
            });
        }
        for o in view.take_inlay_hints_outputs() {
            let quire_core::inlay_hints::InlayHintsOutput::Request(r) = o;
            self.out.events.push(EditorEvent::InlayHintsRequested {
                id: r.id,
                ranges: r.ranges,
                version: r.version,
            });
        }
        for o in view.take_parameter_hints_outputs() {
            let ParameterHintsOutput::Request(r) = o;
            self.out.events.push(EditorEvent::SignatureHelpRequested {
                id: r.id,
                pos: r.pos,
                version: r.version,
                context: r.context,
            });
        }
        for o in view.take_suggest_outputs() {
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

    // ---- caret blink --------------------------------------------------------------------------

    /// Whether the caret is in its visible phase at `now` (it stays on while input arrives and
    /// stops blinking, visible, after [`EditorTheme::blink_idle_stop`]).
    pub fn caret_visible(&self, now: Time) -> bool {
        if !self.focused {
            return false;
        }
        let since = now - self.last_interaction;
        if since >= self.theme.blink_idle_stop {
            return true;
        }
        let period = (self.theme.blink_on + self.theme.blink_off).0.max(1);
        since.0.rem_euclid(period) < self.theme.blink_on.0
    }

    /// The next caret blink transition after `now`, if the caret blinks.
    pub fn next_blink(&self, now: Time) -> Option<Time> {
        if !self.focused {
            return None;
        }
        let since = now - self.last_interaction;
        if since >= self.theme.blink_idle_stop {
            return None;
        }
        let period = (self.theme.blink_on + self.theme.blink_off).0.max(1);
        let phase = since.0.rem_euclid(period);
        let to = if phase < self.theme.blink_on.0 {
            self.theme.blink_on.0 - phase
        } else {
            period - phase
        };
        Some(now + Span(to))
    }
}

#[derive(Clone, Copy)]
enum Clip {
    Copy,
    Cut,
    Paste,
}

fn is_hover_command(a: &KeyAction) -> bool {
    use EditorCommand as C;
    matches!(
        a,
        KeyAction::Command(
            C::ShowHover
                | C::ScrollUpHover
                | C::ScrollDownHover
                | C::ScrollLeftHover
                | C::ScrollRightHover
                | C::PageUpHover
                | C::PageDownHover
                | C::GoToTopHover
                | C::GoToBottomHover
        )
    )
}

/// Identity of the current selection and text (upstream carets belong to it).
fn selection_key(doc: &Document, view: &EditorView) -> u64 {
    hash_one((
        doc.version(),
        view.selection()
            .ranges()
            .iter()
            .map(|r| (r.anchor, r.head))
            .collect::<Vec<_>>(),
    ))
}

fn unit_range(doc: &Document, pos: usize, unit: Unit) -> Range<usize> {
    match unit {
        Unit::Char => pos..pos,
        Unit::Word => word_range_at(doc, pos),
        Unit::Line => quire_core::words::line_range_at(doc.text(), pos.min(doc.text().len_bytes())),
    }
}

/// The word at `pos` as double-click selects it.
pub fn word_range_at(doc: &Document, pos: usize) -> Range<usize> {
    doc.edit_rules().words.word_at(doc.text(), pos)
}

/// A reveal request left by a transaction effect: (byte pos, center).
fn take_reveal(view: &mut EditorView, doc: &Document) -> Option<(usize, bool)> {
    view.take_reveal().map(|r| match r {
        quire_core::Reveal::Pos { pos, center } => (pos, center),
        quire_core::Reveal::Line(l) => (
            doc.text()
                .line_to_byte(l.min(doc.text().len_lines().saturating_sub(1))),
            true,
        ),
    })
}

/// Lines hidden by the view's folds, as (first hidden, last hidden) pairs.
pub fn hidden_line_spans(view: &EditorView, doc: &Document) -> Vec<(usize, usize)> {
    let t = doc.text();
    view.folds()
        .iter()
        .map(|f| {
            (
                t.byte_to_line(f.start.min(t.len_bytes())) + 1,
                t.byte_to_line(f.end.min(t.len_bytes())),
            )
        })
        .filter(|(a, b)| a <= b)
        .collect()
}

/// Header lines of foldable ranges within `lines` (sorted, deduplicated).
pub fn fold_headers(doc: &Document, lines: Range<usize>) -> Vec<usize> {
    let r = doc.fold_ranges();
    let lo = r.partition_point(|f| f.start_line < lines.start);
    let mut v: Vec<usize> = r[lo..]
        .iter()
        .take_while(|f| f.start_line < lines.end)
        .map(|f| f.start_line)
        .collect();
    v.dedup();
    v
}

/// Toggles the outermost foldable range headed by `line` (gutter click), as egui_quire's
/// `glue::toggle_fold_at_line`.
pub fn toggle_fold_at_line(
    doc: &mut Document,
    view: &mut EditorView,
    line: usize,
    author: AuthorId,
) -> Option<Applied> {
    let text = doc.text();
    let r = doc
        .fold_ranges()
        .iter()
        .filter(|r| r.start_line == line)
        .max_by_key(|r| r.end_line)?;
    let bytes = quire_core::folding::fold_bytes(text, r);
    let mut folds = view.folds().to_vec();
    if let Some(i) = folds.iter().position(|f| *f == bytes) {
        folds.remove(i);
    } else if let Some(open) = view.fold_at_line(text, line) {
        folds.retain(|f| *f != open);
    } else {
        folds.push(bytes);
    }
    folds.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end)));
    let hide = |p: usize| {
        folds
            .iter()
            .filter(|f| p > f.start && p <= f.end)
            .map(|f| f.start)
            .min()
            .unwrap_or(p)
    };
    let sel = view
        .selection()
        .transform(|r| SelRange::new(hide(r.anchor), hide(r.head)));
    let tr = Transaction::select(doc.version(), text.len_bytes(), sel)
        .with_origin(Origin::Command("editor.toggleFold".into()))
        .with_effect(quire_core::Effect::SetFolds(folds))
        .with_author(author);
    doc.apply(tr, view).ok()
}

/// Replace `range` with `text` as one input transaction (IME, a11y `SetValue`).
pub fn replace_range(
    doc: &mut Document,
    view: &mut EditorView,
    range: Range<usize>,
    text: &str,
    ime: bool,
    author: AuthorId,
) -> Option<Applied> {
    let len = doc.text().len_bytes();
    let changes = ChangeSet::replace(len, [(range.clone(), text)]).ok()?;
    let tr = Transaction::new(doc.version(), changes)
        .with_selection(Selection::point(range.start + text.len()))
        .with_origin(Origin::Input(if ime {
            InputKind::Ime
        } else {
            InputKind::Other
        }))
        .with_group(GroupHint::Close)
        .with_author(author);
    doc.apply(tr, view).ok()
}

/// IME commit / delete-surrounding as core's `compositionType`.
pub fn composition_cmd(text: &str, before: usize, after: usize) -> EditorCommand {
    EditorCommand::CompositionType {
        text: text.to_owned(),
        replace_prev_chars: before,
        replace_next_chars: after,
    }
}

fn column_select_cmd(anchor: (usize, u32), head: (usize, u32)) -> EditorCommand {
    EditorCommand::ColumnSelect {
        anchor_line: anchor.0,
        anchor_column: anchor.1,
        head_line: head.0,
        head_column: head.1,
    }
}

/// Gives page motions the viewport's line count.
fn with_page_lines(cmd: EditorCommand, page: u32) -> EditorCommand {
    use quire_core::Motion;
    match cmd {
        EditorCommand::CursorMove {
            to: Motion::PageUp { .. },
            select,
        } => EditorCommand::CursorMove {
            to: Motion::PageUp { lines: page },
            select,
        },
        EditorCommand::CursorMove {
            to: Motion::PageDown { .. },
            select,
        } => EditorCommand::CursorMove {
            to: Motion::PageDown { lines: page },
            select,
        },
        EditorCommand::ColumnSelectPageUp { .. } => {
            EditorCommand::ColumnSelectPageUp { lines: page }
        }
        EditorCommand::ColumnSelectPageDown { .. } => {
            EditorCommand::ColumnSelectPageDown { lines: page }
        }
        c => c,
    }
}

/// Does this command change text (refused in read-only mode)? egui_quire's list.
pub fn changes_text(cmd: &EditorCommand) -> bool {
    use EditorCommand::*;
    !matches!(
        cmd,
        CursorMove { .. }
            | SetSelection { .. }
            | SelectAll
            | ExpandLineSelection
            | RemoveSecondaryCursors
            | Save
            | Find { .. }
            | FindNext
            | FindPrevious
            | CloseFind
            | RevealLine { .. }
            | InsertCursorAbove
            | InsertCursorBelow
            | AddNextMatch
            | ColumnSelectLeft
            | ColumnSelectRight
            | ColumnSelectUp
            | ColumnSelectDown
            | ColumnSelectPageUp { .. }
            | ColumnSelectPageDown { .. }
            | ColumnSelect { .. }
            | Fold
            | Unfold
            | ToggleFold
            | FoldAll
            | UnfoldAll
            | TriggerSuggest
            | HideSuggestWidget
            | SelectNextSuggestion
            | SelectPrevSuggestion
            | SelectNextPageSuggestion
            | SelectPrevPageSuggestion
            | ToggleSuggestionDetails
            | JumpToPrevSnippetPlaceholder
            | LeaveSnippet
    )
}
