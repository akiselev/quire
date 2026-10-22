//! [`EditorView`]: one view of a [`Document`] (selection, folds, find, scroll, focus, IME
//! preedit). See the [`document`](crate::document) module docs for how views stay in step.

use crate::change::{Bias, ChangeSet};
use crate::diagnostics::map_range;
use crate::document::{Applied, Document};
use crate::find::{FindQuery, FindState};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::Effect;
use crate::version::Version;
use std::ops::Range;

/// An IME composition in progress. It renders at each cursor and changes no text.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Preedit {
    /// The composing text.
    pub text: String,
    /// The active (underlined) range within `text`, in bytes.
    pub active: Option<Range<usize>>,
}

/// Scroll position, kept as a text anchor so edits above it do not move the visible text.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Scroll {
    /// A byte offset in the first visible line.
    pub anchor: usize,
    /// Pixels the first visible line is scrolled past (the widget's unit).
    pub offset: f32,
    /// Horizontal scroll in pixels.
    pub horizontal: f32,
}

/// A pending scroll request from an effect; the widget takes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reveal {
    /// Make a byte position visible.
    Pos {
        /// The position.
        pos: usize,
        /// Centre it.
        center: bool,
    },
    /// Make a line visible.
    Line(usize),
}

/// A closing character inserted by auto-close: `close` is its byte range, `open_start` where
/// the opening character starts.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AutoClosed {
    /// Start of the opening character(s).
    pub open_start: usize,
    /// The closing character(s).
    pub close: Range<usize>,
}

/// View state that lives only while the selection stays what a command set (VS Code's
/// column-select data and the add-next-match session). Carried by
/// [`Effect::ViewSession`](crate::Effect::ViewSession); its contents are internal.
#[derive(Clone, PartialEq, Debug)]
pub struct ViewSession {
    pub(crate) kind: SessionKind,
    pub(crate) at: Selection,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum SessionKind {
    Column(ColumnSelectData),
    NextMatch(NextMatchSession),
}

/// VS Code `IColumnSelectData` (0-based lines, visual columns).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct ColumnSelectData {
    pub from_line: usize,
    pub from_vis: u32,
    pub to_line: usize,
    pub to_vis: u32,
}

/// VS Code `MultiCursorSession`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct NextMatchSession {
    pub search: String,
    pub whole_word: bool,
    pub match_case: bool,
    /// The range added last (VS Code's last selection in insertion order).
    pub last_added: SelRange,
}

/// How [`EditorView::sync`] caught up.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SyncOutcome {
    /// Already at the document's version.
    UpToDate,
    /// Mapped forward through the recent changes.
    Mapped,
    /// Older than the window: positions clamped, find marked stale.
    Clamped,
}

/// One view of a document.
#[derive(Clone, Debug)]
pub struct EditorView {
    selection: Selection,
    selection_version: u64,
    synced_at: Version,
    folds: Vec<Range<usize>>,
    find: Option<FindState>,
    scroll: Scroll,
    reveal: Option<Reveal>,
    focused: bool,
    preedit: Option<Preedit>,
    auto_closed: Vec<AutoClosed>,
    session: Option<ViewSession>,
}

impl EditorView {
    /// A view of `doc` with a cursor at the start.
    pub fn new(doc: &Document) -> Self {
        EditorView {
            selection: Selection::point(0),
            selection_version: 0,
            synced_at: doc.version(),
            folds: Vec::new(),
            find: None,
            scroll: Scroll::default(),
            reveal: None,
            focused: false,
            preedit: None,
            auto_closed: Vec::new(),
            session: None,
        }
    }

    /// The selection (valid at [`EditorView::synced_at`]).
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Bumps whenever the selection changes.
    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    /// The document version the view's positions belong to.
    pub fn synced_at(&self) -> Version {
        self.synced_at
    }

    /// Whether the view is behind the document.
    pub fn is_behind(&self, doc: &Document) -> bool {
        self.synced_at != doc.version()
    }

    /// Folded byte ranges (sorted).
    pub fn folds(&self) -> &[Range<usize>] {
        &self.folds
    }

    /// Replaces the folded ranges (positions at the document's current version; sync first).
    pub fn set_folds(&mut self, mut folds: Vec<Range<usize>>) {
        folds.sort_by_key(|r| r.start);
        self.folds = folds;
    }

    /// The find state.
    pub fn find(&self) -> Option<&FindState> {
        self.find.as_ref()
    }

    /// The index of the find match the primary selection is on ([`FindState::current_match`]).
    pub fn find_current_match(&self) -> Option<usize> {
        self.find.as_ref()?.current_match(&self.selection.primary())
    }

    /// The scroll position.
    pub fn scroll(&self) -> Scroll {
        self.scroll
    }

    /// Sets the scroll position (widget).
    pub fn set_scroll(&mut self, s: Scroll) {
        self.scroll = s;
    }

    /// Takes the pending scroll request.
    pub fn take_reveal(&mut self) -> Option<Reveal> {
        self.reveal.take()
    }

    /// Whether the view has focus.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Sets focus; returns whether it changed. On a change the caller closes the document's undo
    /// group ([`Document::close_undo_group`]), one of Monaco's closing rules.
    pub fn set_focused(&mut self, f: bool) -> bool {
        let changed = self.focused != f;
        self.focused = f;
        if !f {
            self.preedit = None;
        }
        changed
    }

    /// Auto-inserted closing characters still tracked (VS Code's `autoClosedCharacters`).
    pub fn auto_closed(&self) -> &[AutoClosed] {
        &self.auto_closed
    }

    /// Whether an IME composition is active (the host must not route Enter or Escape elsewhere).
    pub fn composing(&self) -> bool {
        self.preedit.is_some()
    }

    /// The active preedit.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// Sets or clears the preedit.
    pub fn set_preedit(&mut self, p: Option<Preedit>) {
        self.preedit = p.filter(|p| !p.text.is_empty());
    }

    /// Sets the selection directly (view state; no document change). Positions are clamped to the
    /// document's text; the view is synced first.
    pub fn set_selection(&mut self, doc: &Document, sel: Selection) {
        self.sync(doc);
        self.set_selection_raw(sel.clamped(doc.text()));
    }

    pub(crate) fn set_selection_raw(&mut self, sel: Selection) {
        if sel != self.selection {
            self.selection = sel;
            self.selection_version += 1;
        }
        self.prune_auto_closed();
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.at != self.selection)
        {
            self.session = None;
        }
    }

    /// The session state, if the selection is still the one it was recorded with.
    pub(crate) fn session(&self) -> Option<&SessionKind> {
        self.session
            .as_ref()
            .filter(|s| s.at == self.selection)
            .map(|s| &s.kind)
    }

    fn prune_auto_closed(&mut self) {
        if self.auto_closed.is_empty() {
            return;
        }
        let sel = &self.selection;
        self.auto_closed.retain(|a| {
            a.close.start < a.close.end
                && sel
                    .ranges()
                    .iter()
                    .any(|r| r.head > a.open_start && r.head <= a.close.start)
        });
    }

    /// Catches up with the document: maps selection, folds, find matches and scroll anchor
    /// through the recent changes since [`EditorView::synced_at`]; if the view is older than the
    /// window, clamps them to valid positions and marks find stale.
    pub fn sync(&mut self, doc: &Document) -> SyncOutcome {
        if self.synced_at == doc.version() {
            return SyncOutcome::UpToDate;
        }
        match doc.recent().since(self.synced_at) {
            Ok(changes) => {
                for e in changes {
                    self.map_changes(&e.changes, Bias::Before);
                }
                self.synced_at = doc.version();
                SyncOutcome::Mapped
            }
            Err(_) => {
                self.clamp_to(doc.text());
                self.synced_at = doc.version();
                SyncOutcome::Clamped
            }
        }
    }

    /// Maps the view through one applied record (for hosts that fan out). Returns `false` and
    /// does nothing if the view is not at `applied.prev_version`; call [`EditorView::sync`] then.
    pub fn map_through(&mut self, applied: &Applied) -> bool {
        if !applied.changed_text() {
            return self.synced_at == applied.version;
        }
        if self.synced_at != applied.prev_version {
            return false;
        }
        self.map_changes(&applied.changes, Bias::Before);
        self.synced_at = applied.version;
        true
    }

    fn map_changes(&mut self, c: &ChangeSet, cursor_bias: Bias) {
        self.map_secondary(c);
        let sel = self.selection.map(c, cursor_bias);
        self.set_selection_raw(sel);
    }

    fn map_secondary(&mut self, c: &ChangeSet) {
        self.folds = self
            .folds
            .iter()
            .map(|r| map_range(r, c))
            .filter(|r| r.start < r.end)
            .collect();
        if let Some(f) = &mut self.find {
            for m in &mut f.matches {
                *m = map_range(m, c);
            }
            f.matches.retain(|m| m.start < m.end);
            f.stale = true;
        }
        for a in &mut self.auto_closed {
            let len = a.close.end - a.close.start;
            a.open_start = c.map_pos(a.open_start, Bias::After);
            let close = map_range(&a.close, c);
            a.close = if close.end - close.start == len {
                close
            } else {
                close.start..close.start
            };
        }
        self.scroll.anchor = c.map_pos(self.scroll.anchor, Bias::Before);
        if let Some(Reveal::Pos { pos, .. }) = &mut self.reveal {
            *pos = c.map_pos(*pos, Bias::Before);
        }
    }

    fn clamp_to(&mut self, text: &Text) {
        let sel = self.selection.clamped(text);
        self.set_selection_raw(sel);
        let len = text.len_bytes();
        self.folds = self
            .folds
            .iter()
            .map(|r| text.clamp_to_boundary(r.start)..text.clamp_to_boundary(r.end.min(len)))
            .filter(|r| r.start < r.end)
            .collect();
        if let Some(f) = &mut self.find {
            f.matches.clear();
            f.stale = true;
        }
        self.scroll.anchor = text.clamp_to_boundary(self.scroll.anchor);
        self.reveal = None;
        self.auto_closed.clear();
    }

    /// After a local change made through this view.
    pub(crate) fn after_local_change(&mut self, c: &ChangeSet, version: Version, sel: Selection) {
        self.map_secondary(c);
        self.set_selection_raw(sel);
        self.synced_at = version;
    }

    pub(crate) fn apply_effect(&mut self, e: &Effect, doc: &Document) {
        match e {
            Effect::ScrollIntoView { pos, center } => {
                self.reveal = Some(Reveal::Pos {
                    pos: *pos,
                    center: *center,
                })
            }
            Effect::RevealLine(l) => self.reveal = Some(Reveal::Line(*l)),
            Effect::SetFind(q) => self.set_find(doc, q.clone()),
            Effect::ViewSession(s) => self.session = Some(s.clone()),
            Effect::AutoClosed(list) => {
                self.auto_closed.extend(list.iter().cloned());
                self.prune_auto_closed();
            }
        }
    }

    /// Sets (and runs) or clears the find query.
    pub fn set_find(&mut self, doc: &Document, q: Option<FindQuery>) {
        self.find =
            q.map(|q| FindState::new(q, doc.text(), doc.version(), &doc.config().word_separators));
    }

    /// Re-runs a stale find query at the document's version.
    pub fn refresh_find(&mut self, doc: &Document) {
        if let Some(f) = &self.find
            && (f.stale || f.version != doc.version())
        {
            let q = f.query.clone();
            self.set_find(doc, Some(q));
        }
    }
}
