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
use quire_text::{SyncStep, TextView};
use std::ops::Range;

pub use quire_text::{Preedit, SyncOutcome};

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

/// One view of a document.
#[derive(Clone, Debug)]
pub struct EditorView {
    /// Selection, selection version, synced version, focus and preedit (quire-text).
    text: TextView,
    folds: Vec<Range<usize>>,
    find: Option<FindState>,
    scroll: Scroll,
    reveal: Option<Reveal>,
    auto_closed: Vec<AutoClosed>,
    session: Option<ViewSession>,
    /// Byte ranges of the current text changed since the find matches were computed (`None`:
    /// unknown, search everything), so a refresh re-searches only those lines.
    find_dirty: Option<Vec<Range<usize>>>,
    /// The compiled find query, reused across refreshes (compiling costs hundreds of
    /// allocations).
    find_regex: Option<(FindQuery, regex_automata::meta::Regex)>,
    /// The suggest session and list state.
    suggest: crate::completion::SuggestState,
    /// The snippet session (snippet mode).
    snippet: Option<crate::completion::SnippetSession>,
}

impl EditorView {
    /// A view of `doc` with a cursor at the start.
    pub fn new(doc: &Document) -> Self {
        EditorView {
            text: TextView::new(doc.text_document()),
            folds: Vec::new(),
            find: None,
            scroll: Scroll::default(),
            reveal: None,
            auto_closed: Vec::new(),
            session: None,
            find_dirty: Some(Vec::new()),
            find_regex: None,
            suggest: crate::completion::SuggestState::default(),
            snippet: None,
        }
    }

    /// The selection (valid at [`EditorView::synced_at`]).
    pub fn selection(&self) -> &Selection {
        self.text.selection()
    }

    /// The quire-text view underneath (selection, focus, preedit).
    pub fn text_view(&self) -> &TextView {
        &self.text
    }

    pub(crate) fn text_view_mut(&mut self) -> &mut TextView {
        &mut self.text
    }

    /// Bumps whenever the selection changes.
    pub fn selection_version(&self) -> u64 {
        self.text.selection_version()
    }

    /// The document version the view's positions belong to.
    pub fn synced_at(&self) -> Version {
        self.text.synced_at()
    }

    /// Whether the view is behind the document.
    pub fn is_behind(&self, doc: &Document) -> bool {
        self.text.synced_at() != doc.version()
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

    /// Whether `line` is hidden inside a fold (the header line of a fold stays visible).
    pub fn is_line_hidden(&self, text: &Text, line: usize) -> bool {
        if self.folds.is_empty() || line == 0 || line >= text.len_lines() {
            return false;
        }
        let start = text.line_to_byte(line);
        let end = text.line_range(line).end;
        let i = self.folds.partition_point(|f| f.start < start);
        self.folds[..i].iter().any(|f| end <= f.end)
    }

    /// The fold whose header is `line`, if that line is folded.
    pub fn fold_at_line(&self, text: &Text, line: usize) -> Option<Range<usize>> {
        if line >= text.len_lines() {
            return None;
        }
        let at = text.line_range(line).end;
        self.folds
            .iter()
            .filter(|f| f.start == at)
            .max_by_key(|f| f.end)
            .cloned()
    }

    /// The find state.
    pub fn find(&self) -> Option<&FindState> {
        self.find.as_ref()
    }

    /// The index of the find match the primary selection is on ([`FindState::current_match`]).
    pub fn find_current_match(&self) -> Option<usize> {
        self.find
            .as_ref()?
            .current_match(&self.text.selection().primary())
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
        self.text.focused()
    }

    /// Sets focus; returns whether it changed. On a change the caller closes the document's undo
    /// group ([`Document::close_undo_group`]), one of Monaco's closing rules.
    pub fn set_focused(&mut self, f: bool) -> bool {
        self.text.set_focused(f)
    }

    /// Auto-inserted closing characters still tracked (VS Code's `autoClosedCharacters`).
    pub fn auto_closed(&self) -> &[AutoClosed] {
        &self.auto_closed
    }

    /// Whether an IME composition is active (the host must not route Enter or Escape elsewhere).
    pub fn composing(&self) -> bool {
        self.text.composing()
    }

    /// The active preedit.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.text.preedit()
    }

    /// Sets or clears the preedit.
    pub fn set_preedit(&mut self, p: Option<Preedit>) {
        self.text.set_preedit(p);
    }

    /// Sets the selection directly (view state; no document change). Positions are clamped to the
    /// document's text; the view is synced first.
    pub fn set_selection(&mut self, doc: &Document, sel: Selection) {
        self.sync(doc);
        self.set_selection_raw(sel.clamped(doc.text()));
        self.suggest.cancel();
        if self
            .snippet
            .as_ref()
            .is_some_and(|s| !s.contains_selection(self.text.selection()))
        {
            self.snippet = None;
        }
    }

    pub(crate) fn set_selection_raw(&mut self, sel: Selection) {
        self.text.set_selection_unchecked(sel);
        self.after_selection_set();
    }

    /// What a selection change does to the view state that depends on it.
    fn after_selection_set(&mut self) {
        prune_auto_closed(&mut self.auto_closed, self.text.selection());
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.at != *self.text.selection())
        {
            self.session = None;
        }
    }

    fn after_selection_set_to(&mut self, sel: &Selection) {
        prune_auto_closed(&mut self.auto_closed, sel);
        if self.session.as_ref().is_some_and(|s| s.at != *sel) {
            self.session = None;
        }
    }

    /// The session state, if the selection is still the one it was recorded with.
    pub(crate) fn session(&self) -> Option<&SessionKind> {
        self.session
            .as_ref()
            .filter(|s| s.at == *self.text.selection())
            .map(|s| &s.kind)
    }

    /// Catches up with the document: maps selection, folds, find matches and scroll anchor
    /// through the recent changes since [`EditorView::synced_at`]; if the view is older than the
    /// window, clamps them to valid positions and marks find stale.
    pub fn sync(&mut self, doc: &Document) -> SyncOutcome {
        let mut text = std::mem::replace(&mut self.text, TextView::new(doc.text_document()));
        // Per step, as before the split: the secondary state is mapped, then the state that
        // depends on the selection is pruned against the mapped selection.
        let outcome = text.sync_with(doc.text_document(), |step, sel| {
            match step {
                SyncStep::Mapped(c) => self.map_secondary(c),
                SyncStep::Clamped(t) => self.clamp_secondary(t),
            }
            self.after_selection_set_to(sel);
        });
        self.text = text;
        outcome
    }

    /// Maps the view through one applied record (for hosts that fan out). Returns `false` and
    /// does nothing if the view is not at `applied.prev_version`; call [`EditorView::sync`] then.
    pub fn map_through(&mut self, applied: &Applied) -> bool {
        if !applied.changed_text() {
            return self.text.synced_at() == applied.version;
        }
        if self.text.synced_at() != applied.prev_version {
            return false;
        }
        self.map_changes(&applied.changes, Bias::Before);
        self.text.set_synced_at(applied.version);
        true
    }

    fn map_changes(&mut self, c: &ChangeSet, cursor_bias: Bias) {
        self.map_secondary(c);
        let sel = self.text.selection().map(c, cursor_bias);
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
            if let Some(dirty) = &mut self.find_dirty {
                for d in dirty.iter_mut() {
                    *d = map_range(d, c);
                }
                // What this change inserted, in its after-text coordinates.
                let mut delta = 0isize;
                for e in c.edits() {
                    let start = (e.range.start as isize + delta) as usize;
                    dirty.push(start..start + e.insert.len());
                    delta += e.insert.len() as isize - e.range.len() as isize;
                }
                if dirty.len() > 256 {
                    self.find_dirty = None;
                }
            }
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
        self.suggest.map(c);
        if let Some(sn) = &mut self.snippet {
            sn.map(c);
        }
        self.scroll.anchor = c.map_pos(self.scroll.anchor, Bias::Before);
        if let Some(Reveal::Pos { pos, .. }) = &mut self.reveal {
            *pos = c.map_pos(*pos, Bias::Before);
        }
    }

    fn clamp_secondary(&mut self, text: &Text) {
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
        self.find_dirty = None;
        self.scroll.anchor = text.clamp_to_boundary(self.scroll.anchor);
        self.reveal = None;
        self.auto_closed.clear();
        self.suggest.cancel();
        self.snippet = None;
    }

    /// After the document applied a transaction made through this view (the selection is
    /// already set): maps the view state quire-text does not know about.
    pub(crate) fn after_apply(&mut self, applied: &quire_text::Applied) {
        if applied.changed_text() {
            self.map_secondary(&applied.changes);
        }
        self.after_selection_set();
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
            Effect::SetFolds(f) => self.set_folds(f.clone()),
            Effect::AutoClosed(list) => {
                self.auto_closed.extend(list.iter().cloned());
                prune_auto_closed(&mut self.auto_closed, self.text.selection());
            }
            // Applied by the document.
            Effect::SetInsertSpaces(_) | Effect::SetWordWrap(_) => {}
            Effect::Suggest(e) => {
                use crate::completion::session::TriggerOptions;
                use crate::transaction::SuggestEffect as S;
                let sel = self.text.selection().clone();
                match e {
                    S::Trigger => self.suggest.trigger(doc, &sel, TriggerOptions::new(false)),
                    S::Hide => self.suggest.cancel(),
                    S::Select { delta, page } => self.suggest.select(*delta, *page),
                    S::ToggleDetails => self.suggest.toggle_details(),
                    S::Accepted { retrigger, command } => {
                        self.suggest.cancel();
                        if let Some(c) = command {
                            self.suggest
                                .push_output(crate::completion::SuggestOutput::Command(c.clone()));
                        }
                        if *retrigger {
                            self.suggest.trigger(doc, &sel, TriggerOptions::new(true));
                        }
                    }
                }
            }
            Effect::Snippet(s) => self.snippet = s.clone(),
        }
    }

    /// The suggest state (session, list, details).
    pub fn suggest(&self) -> &crate::completion::SuggestState {
        &self.suggest
    }

    /// Takes what the suggest session wants from the host: requests, resolves, commands.
    pub fn take_suggest_outputs(&mut self) -> Vec<crate::completion::SuggestOutput> {
        self.suggest.take_outputs()
    }

    /// The host's answer to a [`CompletionRequest`](crate::completion::CompletionRequest):
    /// one list per source, ranges in bytes at `at`. Mapped forward if the text moved on;
    /// refused when stale or superseded (nothing changes then).
    pub fn set_completions(
        &mut self,
        doc: &Document,
        id: crate::completion::CompletionRequestId,
        at: crate::Version,
        lists: Vec<crate::completion::CompletionList>,
    ) -> Result<(), crate::completion::CompletionRefusal> {
        self.sync(doc);
        let sel = self.text.selection().clone();
        self.suggest.set_completions(doc, &sel, id, at, lists)
    }

    /// The host's answer to a resolve request: detail, documentation and additional edits of
    /// `resolved` replace the item's. `false` when the request is no longer current.
    pub fn resolve_completion_item(
        &mut self,
        id: crate::completion::CompletionRequestId,
        source: &crate::SourceId,
        index: usize,
        resolved: crate::completion::CompletionItem,
    ) -> bool {
        self.suggest.resolve_item(id, source, index, resolved)
    }

    /// Starts a suggest request at the primary cursor (VS Code `SuggestController.triggerSuggest`;
    /// `auto`: as quick suggestions do, an empty answer closes it).
    pub fn trigger_suggest(&mut self, doc: &Document, auto: bool) {
        self.sync(doc);
        let sel = self.text.selection().clone();
        self.suggest.trigger(
            doc,
            &sel,
            crate::completion::session::TriggerOptions::new(auto),
        );
    }

    /// Focuses a row of the suggest list (the mouse).
    pub fn suggest_focus_row(&mut self, row: usize) {
        self.suggest.focus_row(row);
    }

    /// Scrolls the suggest list (the mouse wheel).
    pub fn suggest_scroll_to(&mut self, top: usize) {
        self.suggest.set_scroll_top(top);
    }

    /// Closes the suggest list (focus lost; VS Code cancels on blur).
    pub fn cancel_suggest(&mut self) {
        self.suggest.cancel();
    }

    /// The snippet session (snippet mode: `inSnippetMode`).
    pub fn snippet(&self) -> Option<&crate::completion::SnippetSession> {
        self.snippet.as_ref()
    }

    /// After a transaction through this view that is not a suggest or snippet command: the
    /// suggest session follows the edit, the snippet session ends when the selection leaves
    /// its placeholders.
    pub(crate) fn after_change_for_sessions(
        &mut self,
        doc: &Document,
        origin: &crate::transaction::Origin,
        changed_text: bool,
    ) {
        use crate::completion::session::ChangeKind;
        use crate::transaction::{InputKind as K, Origin as O};
        let kind = match origin {
            O::Input(K::Typing | K::TypingFirstSpace | K::TypingConsecutiveSpace) => {
                ChangeKind::Typed
            }
            O::Input(K::Ime) => ChangeKind::CompositionEnd,
            O::Input(K::DeletingLeft) => ChangeKind::DeletedLeft,
            O::Command(id) if id == "cursorMove" => ChangeKind::CursorMoved,
            _ => ChangeKind::Other,
        };
        let sel = self.text.selection().clone();
        if let Some(sn) = &self.snippet
            && (matches!(origin, O::Undo | O::Redo) || !sn.contains_selection(&sel))
        {
            self.snippet = None;
        }
        let composing = self.text.composing();
        let in_snippet = self.snippet.is_some();
        self.suggest
            .after_change(doc, &sel, kind, changed_text, composing, in_snippet);
    }

    /// Sets (and runs) or clears the find query.
    pub fn set_find(&mut self, doc: &Document, q: Option<FindQuery>) {
        self.find =
            q.map(|q| FindState::new(q, doc.text(), doc.version(), &doc.config().word_separators));
        self.find_dirty = Some(Vec::new());
    }

    /// Re-runs a stale find query at the document's version.
    pub fn refresh_find(&mut self, doc: &Document) {
        if let Some(f) = &mut self.find
            && (f.stale || f.version != doc.version())
        {
            // Re-search only the edited lines when the matches were mapped up to now.
            if let Some(mut dirty) = self.find_dirty.take()
                && self.text.synced_at() == doc.version()
            {
                dirty.sort_by_key(|d| d.start);
                if self.find_regex.as_ref().is_none_or(|(q, _)| *q != f.query) {
                    self.find_regex = crate::find::build_regex(&f.query)
                        .ok()
                        .map(|re| (f.query.clone(), re));
                }
                let ws = &doc.config().word_separators;
                if let Some((_, re)) = &self.find_regex
                    && crate::find::refresh_lines(f, re, doc.text(), &dirty, doc.version(), ws)
                {
                    self.find_dirty = Some(Vec::new());
                    return;
                }
            }
            let q = f.query.clone();
            self.set_find(doc, Some(q));
        }
    }
}

fn prune_auto_closed(auto_closed: &mut Vec<AutoClosed>, sel: &Selection) {
    if auto_closed.is_empty() {
        return;
    }
    auto_closed.retain(|a| {
        a.close.start < a.close.end
            && sel
                .ranges()
                .iter()
                .any(|r| r.head > a.open_start && r.head <= a.close.start)
    });
}
