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
use quire_core::{Applied, Document, EditorCommand, EditorView, HostRequest, Selection, Version};

use crate::command::{KeyAction, WidgetAction};
use crate::find_bar::FindBar;
use crate::galley_cache::GalleyCache;
use crate::glue;
use crate::height::HeightIndex;
use crate::keymap::{Keymap, WhenContext};
use crate::layout::{self, LineLayout, LineShaper};
use crate::theme::{EditorText, EditorTheme};

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
    /// The pointer rested over `pos` (host may answer with hover content).
    HoverAt {
        pos: usize,
        version: Version,
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
}

/// Widget-side state kept in egui memory under the editor id.
#[derive(Debug, Default)]
pub(crate) struct Mem {
    pub galleys: GalleyCache,
    pub heights: HeightIndex,
    pub scroll: Vec2,
    pub last_interaction: f64,
    pub focused: bool,
    pub last_press: Option<(f64, Pos2, u32)>,
    pub drag: Option<Drag>,
    pub hover: Option<(usize, f64, bool)>,
    pub find: FindBar,
    pub a11y_page_start: usize,
    pub scroll_to_caret: bool,
    pub max_seen_width: f32,
    pub diag_hover: Option<Range<usize>>,
    /// Last frame's text area and primary caret (layer coordinates), for anchor queries.
    pub last_text_rect: Option<Rect>,
    pub last_caret: Option<Rect>,
    /// `theme.galley_hash()` of this frame.
    pub style_salt: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Drag {
    pub anchor: Range<usize>,
    pub unit: Unit,
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
}

impl Geom {
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

    /// Size for layout without drawing: all lines × line height; width fills `available.x` when
    /// finite, else gutter plus 80 columns.
    pub fn measure(&self, ctx: &Context, available: Vec2) -> Vec2 {
        let theme = self
            .theme
            .cloned()
            .unwrap_or_else(|| EditorTheme::from_visuals(&ctx.global_style().visuals));
        let row_h = layout::row_height(ctx, &theme.font);
        let adv = layout::mono(ctx, &theme.font).advance();
        let lines = glue::text(self.doc).len_lines().max(1);
        let w = if available.x.is_finite() {
            available.x
        } else {
            gutter_width(lines, adv) + 80.0 * adv
        };
        vec2(w, lines as f32 * row_h + PAD_Y * 2.0)
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
        let default_text;
        let text = match text {
            Some(t) => t,
            None => {
                default_text = EditorText::default();
                &default_text
            }
        };
        glue::sync(doc, view);
        glue::refresh_find(doc, view);

        let shared = load_mem(&ctx, id);
        let mut mem = shared.lock().unwrap_or_else(|e| e.into_inner());

        let row_h = layout::row_height(&ctx, &theme.font);
        let mono = layout::mono(&ctx, &theme.font);
        let adv = mono.advance();
        let n_lines = glue::text(doc).len_lines().max(1);
        if mem.heights.len() != n_lines || mem.heights.get(0) != row_h {
            mem.heights = HeightIndex::uniform(n_lines, row_h);
        }
        let gw = gutter_width(n_lines, adv);
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
            pad_left: (adv * 0.5 * mono.pixels_per_point).round() / mono.pixels_per_point,
        };

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
            if !focused && glue::preedit(frame.view).is_some() {
                glue::set_preedit(frame.view, None);
            }
        }

        // Keyboard, text, IME, clipboard.
        if focused {
            frame.handle_events(ui, &keys);
        }
        // Accessibility actions (SetValue / ReplaceSelectedText) arrive without focus.
        frame.handle_accesskit_actions(ui);

        // Mouse.
        frame.handle_pointer(ui, &response);
        frame.handle_scroll(ui, &response);

        // Paint.
        let caret_rect = crate::paint::paint(&mut frame, ui);
        crate::paint::scrollbars(&mut frame, ui);

        // Find bar (drawn over the text, after the editor so it wins hit tests).
        if frame.mem.find.open {
            crate::find_bar::show(&mut frame, ui, text);
        }

        // Diagnostics hover.
        frame.diagnostic_hover(ui, &response);

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
            let escape = glue::preedit(frame.view).is_some() || frame.mem.find.open;
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

        crate::a11y::update(&mut frame, ui, a11y_name);
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

/// Gutter width for `lines` lines: diagnostic mark, digits, padding.
fn gutter_width(lines: usize, adv: f32) -> f32 {
    let digits = (lines.max(1).ilog10() + 1).max(2) as f32;
    MARK_W + digits * adv + adv * 1.5
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
        let time_ms = (self.now() * 1000.0) as u64;
        match glue::run(self.doc, self.view, &cmd, time_ms) {
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
        WhenContext {
            find_widget_visible: self.mem.find.open,
            multiple_selections: glue::selection(self.view).len() > 1,
        }
    }

    /// Keyboard, text, IME and clipboard events, in arrival order. Handled events are removed
    /// from egui's input (R11), so the host sees only what is left.
    fn handle_events(&mut self, ui: &Ui, keys: &KeyHandling) {
        let os = self.ctx.os();
        let owns_ime = self.ctx.memory(|m| m.owns_ime_events(self.id));
        let events = ui.input_mut(|i| std::mem::take(&mut i.events));
        let mut kept = Vec::with_capacity(events.len());
        for ev in events {
            let handled = match &ev {
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
                        match map
                            .lookup(os, *key, *modifiers, &self.when_context())
                            .map(|b| b.action.clone())
                        {
                            Some(KeyAction::Command(c)) => {
                                self.dispatch(c);
                                true
                            }
                            Some(KeyAction::Widget(a)) => {
                                self.widget_action(a);
                                true
                            }
                            None => false,
                        }
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

    /// Copy the selection (ranges joined by newlines). Returns whether anything was copied.
    fn copy(&mut self) -> bool {
        let text = glue::text(self.doc);
        let parts: Vec<String> = glue::ranges(glue::selection(self.view))
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| text.slice(r.range()).to_string())
            .collect();
        if parts.is_empty() {
            return false;
        }
        self.ctx.copy_text(parts.join("\n"));
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
                    self.dispatch(glue::type_cmd(text));
                }
            }
            ImeEvent::DeleteSurrounding {
                before_chars,
                after_chars,
            } => {
                if self.read_only {
                    return;
                }
                let t = glue::text(self.doc);
                let head = glue::selection(self.view).primary().head;
                let mut from = head;
                for _ in 0..*before_chars {
                    if from == 0 {
                        break;
                    }
                    from = t.prev_char_boundary(from);
                }
                let mut to = head;
                for _ in 0..*after_chars {
                    if to >= t.len_bytes() {
                        break;
                    }
                    to = t.next_char_boundary(to);
                }
                if from < to
                    && let Some(a) = glue::replace(self.doc, self.view, from..to, "", true)
                {
                    self.push_applied(a);
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
                if let Some(a) = glue::replace(self.doc, self.view, range, &v, false) {
                    self.push_applied(a);
                }
            }
        }
    }

    /// Byte offset under a screen position.
    pub fn pos_at(&mut self, p: Pos2) -> usize {
        let line = self
            .mem
            .heights
            .line_at(p.y - self.geom.text.top() + self.mem.scroll.y);
        let text = glue::text(self.doc);
        let line = line.min(text.len_lines().saturating_sub(1));
        let ll = self.layout_line(line);
        let col = ll.col_at(p.x - self.geom.text_left(self.mem));
        glue::text(self.doc).line_to_byte(line) + col
    }

    /// Lay out line `line` through the cache.
    pub fn layout_line(&mut self, line: usize) -> LineLayout {
        let text = glue::text(self.doc);
        let s = glue::line_str(text, line);
        let mut runs = Vec::new();
        let style_hash =
            glue::line_runs(self.doc, line, self.theme, &mut runs) ^ self.mem.style_salt;
        let mut shaper = LineShaper {
            cache: &mut self.mem.galleys,
            ctx: self.ctx,
            font: &self.theme.font,
            pixels_per_point: self.ctx.pixels_per_point(),
            mono: self.geom.mono,
        };
        shaper.shape(&s, &runs, style_hash, self.theme.foreground)
    }

    /// The unit range (char: empty at pos; word; line incl. its break) around `pos`.
    fn unit_range(&self, pos: usize, unit: Unit) -> Range<usize> {
        let text = glue::text(self.doc);
        match unit {
            Unit::Char => pos..pos,
            Unit::Line => {
                let l = text.byte_to_line(pos);
                let r = text.line_range(l);
                let end = if l + 1 < text.len_lines() {
                    text.line_to_byte(l + 1)
                } else {
                    r.end
                };
                r.start..end
            }
            Unit::Word => {
                let l = text.byte_to_line(pos);
                let start = text.line_to_byte(l);
                let s = glue::line_str(text, l);
                let col = pos - start;
                let r = word_at(&s, col);
                start + r.start..start + r.end
            }
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
        let Some(ptr) = ptr else { return };
        let in_text = self.geom.text.contains(ptr) || self.geom.gutter.contains(ptr);

        if response.secondary_clicked() {
            let pos = self.pos_at(ptr);
            self.out.events.push(EditorEvent::ContextMenu { pos });
        }

        if pressed && response.hovered() && in_text {
            self.touch();
            let pos = self.pos_at(ptr);
            if mods.command && !mods.shift && !mods.alt {
                let range = self.unit_range(pos, Unit::Word);
                self.out.events.push(EditorEvent::LinkActivated {
                    range,
                    version: glue::version(self.doc),
                });
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
            });
            let new = if anchor_range.is_empty() || mods.shift {
                glue::single(anchor_range.start, head)
            } else {
                glue::single(anchor_range.start, anchor_range.end)
            };
            self.set_selection(new);
        } else if down
            && let Some(drag) = self.mem.drag.clone()
            && (response.dragged() || response.is_pointer_button_down_on())
        {
            let pos = self.pos_at(ptr);
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

        // Hover: emit HoverAt once the pointer has rested over a position for 0.5 s.
        if response.hovered() && self.geom.text.contains(ptr) && !down {
            let pos = self.pos_at(ptr);
            match self.mem.hover {
                Some((p, since, emitted)) if p == pos => {
                    if !emitted && time - since >= 0.5 {
                        self.out.events.push(EditorEvent::HoverAt {
                            pos,
                            version: glue::version(self.doc),
                        });
                        self.mem.hover = Some((p, since, true));
                    } else if !emitted {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_secs_f64(
                                (0.5 - (time - since)).max(0.01),
                            ));
                    }
                }
                _ => {
                    self.mem.hover = Some((pos, time, false));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
        } else {
            self.mem.hover = None;
        }
    }

    /// Mouse selection: recorded as `setSelection`, applied as a selection-only transaction (the
    /// transaction that command resolves to).
    fn set_selection(&mut self, sel: Selection) {
        self.touch();
        self.out.commands.push(glue::set_selection_cmd(sel.clone()));
        if let Some(a) = glue::select(self.doc, self.view, sel) {
            self.push_applied(a);
        }
    }

    fn content_size(&self) -> Vec2 {
        // Width: widest laid-out line seen is not tracked yet; allow scrolling to the longest line
        // measured in characters (monospace estimate).
        let h = self.mem.heights.total() + self.geom.text.height() * 0.5;
        vec2(self.max_line_width(), h)
    }

    fn max_line_width(&self) -> f32 {
        // O(lines) scan of line lengths is avoided: use the longest visible line plus a margin.
        self.mem.galleys_max_width().max(self.geom.text.width()) + self.geom.adv * 4.0
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
        let top = self.mem.heights.y_of(line);
        let bottom = top + self.geom.row_h;
        let view_h = self.geom.text.height();
        if center {
            self.mem.scroll.y = top - (view_h - self.geom.row_h) * 0.5;
        } else if top < self.mem.scroll.y {
            self.mem.scroll.y = top;
        } else if bottom > self.mem.scroll.y + view_h {
            self.mem.scroll.y = bottom - view_h;
        }
        let x = self.layout_line(line).x_of(col) + self.geom.pad_left;
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

/// (colour, message, actions as (title, command id, args JSON)).
type HoverItem = (Color32, String, Vec<(String, String, String)>);

impl FrameCx<'_> {
    /// Hover text for diagnostics under the pointer, with their actions as buttons. The hover
    /// target stays alive while its tooltip is open so the buttons can be reached.
    fn diagnostic_hover(&mut self, ui: &mut Ui, response: &Response) {
        let wid = self.id.with("quire.diag");
        let ptr = ui.input(|i| i.pointer.hover_pos());
        let mut target = None;
        if let Some(p) = ptr
            && response.hovered()
            && self.geom.text.contains(p)
            && self.mem.drag.is_none()
        {
            let pos = self.pos_at(p);
            target = glue::diagnostics_at(self.doc, pos)
                .first()
                .map(|d| d.range.clone());
        }
        if target.is_none() && egui::Tooltip::was_tooltip_open_last_frame(self.ctx, wid) {
            target = self.mem.diag_hover.clone();
        }
        self.mem.diag_hover = target.clone();
        let Some(range) = target else { return };
        let text = glue::text(self.doc);
        let start = range.start.min(text.len_bytes());
        let line = text.byte_to_line(start);
        let ls = text.line_to_byte(line);
        let line_len = glue::line_str(text, line).len();
        let end = range.end.clamp(start, ls + line_len);
        let ll = self.layout_line(line);
        let left = self.geom.text_left(self.mem);
        let top = self.geom.line_top(self.mem, line);
        let x0 = left + ll.x_of(start - ls);
        let x1 = (left + ll.x_of(end - ls)).max(x0 + self.geom.adv);
        let rect = Rect::from_min_max(pos2(x0, top), pos2(x1, top + self.geom.row_h));
        let items: Vec<HoverItem> = glue::diagnostics_at(self.doc, start)
            .into_iter()
            .map(|d| {
                let c = match d.severity {
                    glue::Severity::Error => self.theme.error,
                    glue::Severity::Warning => self.theme.warning,
                    glue::Severity::Info => self.theme.info,
                    glue::Severity::Hint => self.theme.hint,
                };
                let acts = d
                    .actions
                    .iter()
                    .map(|a| (a.title.clone(), a.command_id.clone(), a.args_json.clone()))
                    .collect();
                (c, d.message.clone(), acts)
            })
            .collect();
        let mut chosen = None;
        ui.interact(rect, wid, Sense::hover()).on_hover_ui(|ui| {
            for (c, msg, acts) in &items {
                ui.colored_label(*c, msg);
                if !acts.is_empty() {
                    ui.horizontal(|ui| {
                        for (title, command_id, args) in acts {
                            if ui.button(title).clicked() {
                                chosen = Some((command_id.clone(), args.clone()));
                            }
                        }
                    });
                }
            }
        });
        if let Some((command_id, args)) = chosen {
            let args = serde_json::from_str(&args).unwrap_or(serde_json::Value::Null);
            self.out
                .events
                .push(EditorEvent::DiagnosticAction { command_id, args });
        }
    }
}

impl Mem {
    pub(crate) fn galleys_max_width(&self) -> f32 {
        self.max_seen_width
    }
}

/// The word (or run of punctuation / whitespace) at byte column `col` of `line`.
pub(crate) fn word_at(line: &str, col: usize) -> Range<usize> {
    #[derive(PartialEq)]
    enum K {
        Word,
        Space,
        Punct,
    }
    fn kind(c: char) -> K {
        if c.is_alphanumeric() || c == '_' {
            K::Word
        } else if c.is_whitespace() {
            K::Space
        } else {
            K::Punct
        }
    }
    let col = col.min(line.len());
    // Prefer the char at col; at end of a word (col after it) take the char before.
    let at = line[col..].chars().next().map(kind);
    let before = line[..col].chars().next_back().map(kind);
    let k = match (&at, &before) {
        (Some(K::Word), _) => K::Word,
        (_, Some(K::Word)) => K::Word,
        (Some(_), _) => at.unwrap(),
        (None, Some(_)) => before.unwrap(),
        (None, None) => return col..col,
    };
    let start = line[..col]
        .char_indices()
        .rev()
        .take_while(|(_, c)| kind(*c) == k)
        .last()
        .map_or(col, |(i, _)| i);
    let end = col
        + line[col..]
            .chars()
            .take_while(|c| kind(*c) == k)
            .map(char::len_utf8)
            .sum::<usize>();
    start..end
}

#[cfg(test)]
mod tests {
    use super::word_at;

    #[test]
    fn words() {
        assert_eq!(word_at("let foo_bar = 1;", 5), 4..11);
        assert_eq!(word_at("let foo_bar = 1;", 11), 4..11);
        assert_eq!(word_at("a  b", 2), 1..3);
        assert_eq!(word_at("", 0), 0..0);
        assert_eq!(word_at("x(", 2), 1..2);
    }
}
