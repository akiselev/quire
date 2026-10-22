//! The suggest session of one view: VS Code's `SuggestModel` (trigger, refilter, retrigger,
//! cancel) and the suggest widget's list state (focus, scroll, details), as a state machine
//! driven by the view's edits and the host's answers. Translated from
//! `editor/contrib/suggest/browser/suggestModel.ts` (microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`).
//!
//! Departures (ours): a quick suggestion is requested on the keystroke itself (VS Code waits
//! `quickSuggestionsDelay`, 10 ms; quire-core has no clock, and typing within a session only
//! refilters); no word-distance locality bonus (`suggest.localityBonus`, default off).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use super::model::{CompletionModel, FilteredItem, LineContext, ModelOptions, SuggestItem, compare_items};
use super::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionRefusal, CompletionRequest,
    CompletionRequestId, CompletionTriggerKind, FuzzyScoreOptions, SnippetSuggestions,
    SuggestOutput,
};
use crate::change::{Bias, ChangeSet};
use crate::diagnostics::SourceId;
use crate::document::Document;
use crate::selection::Selection;
use crate::syntax::TokenType;
use crate::text::Text;
use crate::version::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Rows the list shows at most (VS Code's suggest widget: 12).
pub const VISIBLE_ROWS: usize = 12;

/// How the cursor or text changed, as the session sees it (VS Code's cursor change reason
/// and source).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChangeKind {
    /// Typing (reason NotSet, source keyboard).
    Typed,
    /// Backspace (source deleteLeft).
    DeletedLeft,
    /// A cursor key moved an empty selection (reason Explicit).
    CursorMoved,
    /// An IME composition ended (VS Code `onDidCompositionEnd`).
    CompositionEnd,
    /// Anything else (mouse, paste, undo, commands): cancel.
    Other,
}

/// What the widget shows (VS Code's suggest widget states).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WidgetState {
    /// Nothing.
    Hidden,
    /// Waiting for a manual request's answer ("Loading...").
    Loading,
    /// A manual request found nothing ("No suggestions.").
    Empty,
    /// The list.
    Open,
    /// The list of a manual request, kept while the typed word matches nothing.
    Frozen,
}

/// What the session asks for when it triggers (VS Code `SuggestTriggerOptions`).
#[derive(Clone, Debug)]
pub(crate) struct TriggerOptions {
    pub auto: bool,
    pub kind: CompletionTriggerKind,
    pub retrigger: bool,
    pub sources: Option<Vec<SourceId>>,
    pub reuse: BTreeMap<SourceId, Vec<Arc<SuggestItem>>>,
}

impl TriggerOptions {
    pub(crate) fn new(auto: bool) -> Self {
        TriggerOptions {
            auto,
            kind: CompletionTriggerKind::Invoke,
            retrigger: false,
            sources: None,
            reuse: BTreeMap::new(),
        }
    }
}

/// The word around a position (VS Code `IWordAtPosition`): byte columns.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct WordAt {
    pub word: String,
    pub start: usize,
    pub end: usize,
}

/// The cursor's line context (VS Code suggestModel's `LineContext`).
#[derive(Clone, Debug)]
pub(crate) struct Ctx {
    pub line: usize,
    pub column: usize,
    pub leading_line_content: String,
    pub leading_word: WordAt,
}

fn is_word_char(c: char, seps: &str) -> bool {
    !c.is_whitespace() && !seps.contains(c)
}

/// The word containing or ending at `col` of `line` (VS Code `getWordAtPosition`), as runs of
/// characters that are neither whitespace nor word separators.
pub(crate) fn word_at(line: &str, col: usize, seps: &str) -> Option<WordAt> {
    let col = col.min(line.len());
    let mut start = col;
    for (i, c) in line[..col].char_indices().rev() {
        if !is_word_char(c, seps) {
            break;
        }
        start = i;
    }
    let mut end = col;
    for (i, c) in line[col..].char_indices() {
        if !is_word_char(c, seps) {
            break;
        }
        end = col + i + c.len_utf8();
    }
    (start < end).then(|| WordAt {
        word: line[start..end].to_string(),
        start,
        end,
    })
}

/// VS Code `getWordUntilPosition`: the word's part before `col` (empty at `col` when none).
pub(crate) fn word_until(line: &str, col: usize, seps: &str) -> WordAt {
    match word_at(line, col, seps) {
        Some(w) if w.start < col => WordAt {
            word: line[w.start..col].to_string(),
            start: w.start,
            end: col,
        },
        _ => WordAt {
            word: String::new(),
            start: col,
            end: col,
        },
    }
}

fn line_text(text: &Text, line: usize) -> String {
    text.line(line).to_string()
}

pub(crate) fn ctx_at(doc: &Document, pos: usize) -> Ctx {
    let text = doc.text();
    let (line, column) = text.byte_to_line_col(pos);
    let l = line_text(text, line);
    let column = column.min(l.len());
    Ctx {
        line,
        column,
        leading_line_content: l[..column].to_string(),
        leading_word: word_until(&l, column, &doc.config().word_separators),
    }
}

fn is_number(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && (s.parse::<f64>().is_ok()
            || s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit())))
}

/// VS Code `LineContext.shouldAutoTrigger`: the primary cursor ends a word (or follows its first
/// character) that is not a number.
pub fn should_auto_trigger(doc: &Document, sel: &Selection) -> bool {
    let p = sel.primary().head;
    let text = doc.text();
    let (line, col) = text.byte_to_line_col(p);
    let l = line_text(text, line);
    let Some(w) = word_at(&l, col, &doc.config().word_separators) else {
        return false;
    };
    let first_len = w.word.chars().next().map_or(0, char::len_utf8);
    if w.end != col && w.start + first_len != col {
        return false;
    }
    !is_number(&w.word)
}

fn leading_whitespace(s: &str) -> &str {
    let n = s.len() - s.trim_start_matches([' ', '\t']).len();
    &s[..n]
}

/// One active session (VS Code: the suggest model between trigger and cancel).
#[derive(Clone, Debug)]
pub struct SuggestSession {
    /// The current request.
    pub id: CompletionRequestId,
    /// An automatic session (quick suggestions, trigger characters).
    pub auto: bool,
    /// The trigger of the current request.
    pub trigger: CompletionTriggerKind,
    version: Version,
    /// The request position, mapped through later edits (bias before).
    anchor: usize,
    /// The character right of the request position, mapped (bias after); `None` at line end
    /// (VS Code `LineSuffix`).
    suffix: Option<usize>,
    ctx: Ctx,
    model: Option<CompletionModel>,
    reuse: BTreeMap<SourceId, Vec<Arc<SuggestItem>>>,
    focused: usize,
    scroll_top: usize,
    frozen: bool,
    shown: bool,
    resolve_requested: BTreeSet<(SourceId, usize)>,
    /// The previous list, shown while a retrigger waits: model, focus, scroll, frozen.
    stale: Option<Box<(CompletionModel, usize, usize, bool)>>,
}

impl SuggestSession {
    /// The document version of the current request.
    pub fn request_version(&self) -> Version {
        self.version
    }

    /// The model (once the host answered).
    pub fn model(&self) -> Option<&CompletionModel> {
        self.model.as_ref()
    }
}

/// A view's suggest state: the session, the details toggle (kept across sessions, as VS Code
/// stores it) and what the host should do.
#[derive(Clone, Debug, Default)]
pub struct SuggestState {
    session: Option<SuggestSession>,
    next_id: u64,
    details_visible: bool,
    outbox: Vec<SuggestOutput>,
    generation: u64,
}

impl SuggestState {
    /// The session, if any.
    pub fn session(&self) -> Option<&SuggestSession> {
        self.session.as_ref()
    }

    /// Bumps whenever what the widget shows may have changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// What the widget shows.
    pub fn widget_state(&self) -> WidgetState {
        let Some(s) = &self.session else {
            return WidgetState::Hidden;
        };
        if s.model.is_none()
            && let Some(st) = &s.stale
        {
            return if st.3 { WidgetState::Frozen } else { WidgetState::Open };
        }
        match &s.model {
            None if s.auto => WidgetState::Hidden,
            None => WidgetState::Loading,
            Some(_) if !s.shown => {
                if s.auto {
                    WidgetState::Hidden
                } else {
                    WidgetState::Loading
                }
            }
            Some(m) if m.cached_items().is_empty() => {
                if s.auto {
                    WidgetState::Hidden
                } else {
                    WidgetState::Empty
                }
            }
            Some(_) if s.frozen => WidgetState::Frozen,
            Some(_) => WidgetState::Open,
        }
    }

    /// `suggestWidgetVisible`.
    pub fn is_visible(&self) -> bool {
        self.widget_state() != WidgetState::Hidden
    }

    /// The shown items, best first.
    pub fn items(&self) -> &[FilteredItem] {
        match self.widget_state() {
            WidgetState::Open | WidgetState::Frozen => {
                let Some(s) = &self.session else {
                    return &[];
                };
                match (&s.model, &s.stale) {
                    (Some(m), _) => m.cached_items(),
                    (None, Some(st)) => st.0.cached_items(),
                    _ => &[],
                }
            }
            _ => &[],
        }
    }

    /// The focused row (`suggestWidgetHasFocusedSuggestion`).
    pub fn focused(&self) -> Option<usize> {
        let n = self.items().len();
        let s = self.session.as_ref()?;
        let f = match (&s.model, &s.stale) {
            (None, Some(st)) => st.1,
            _ => s.focused,
        };
        (n > 0).then(|| f.min(n - 1))
    }

    /// The focused item.
    pub fn focused_item(&self) -> Option<&FilteredItem> {
        self.focused().map(|i| &self.items()[i])
    }

    /// The first visible row.
    pub fn scroll_top(&self) -> usize {
        self.session.as_ref().map_or(0, |s| match (&s.model, &s.stale) {
            (None, Some(st)) => st.2,
            _ => s.scroll_top,
        })
    }

    /// Scrolls the list (mouse wheel), clamped.
    pub fn set_scroll_top(&mut self, top: usize) {
        let n = self.items().len();
        if let Some(s) = &mut self.session {
            s.scroll_top = top.min(n.saturating_sub(VISIBLE_ROWS));
            self.generation += 1;
        }
    }

    /// `suggestWidgetDetailsVisible`.
    pub fn details_visible(&self) -> bool {
        self.details_visible
    }

    /// Takes what the host should do.
    pub fn take_outputs(&mut self) -> Vec<SuggestOutput> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether typing `text` accepts the focused item first (its commit characters,
    /// `editor.acceptSuggestionOnCommitCharacter`).
    pub fn accepts_on(&self, doc: &Document, text: &str) -> bool {
        if !doc.config().suggest.accept_suggestion_on_commit_character
            || self.widget_state() != WidgetState::Open
        {
            return false;
        }
        let Some(last) = text.chars().last() else {
            return false;
        };
        let mut b = [0u8; 4];
        let ch = &*last.encode_utf8(&mut b);
        self.focused_item()
            .is_some_and(|f| f.item.completion.commit_characters.iter().any(|c| c == ch))
    }

    /// `suggestionMakesTextEdit`: accepting changes the text (always, unless
    /// `acceptSuggestionOnEnter` is `smart` and the word is already typed).
    pub fn makes_text_edit(&self, doc: &Document, sel: &Selection) -> bool {
        let Some(f) = self.focused_item() else {
            return true;
        };
        let Some(s) = &self.session else {
            return true;
        };
        let c = &f.item.completion;
        if doc.config().suggest.accept_suggestion_on_enter
            != super::AcceptSuggestionOnEnter::Smart
            || !s.auto
            || !c.additional_text_edits.is_empty()
            || c.insert_text_rules.insert_as_snippet
        {
            return true;
        }
        let pos = sel.primary().head;
        let (line, col) = doc.text().byte_to_line_col(pos);
        if col < f.item.edit_start || col - f.item.edit_start != c.insert_text.len() {
            return true;
        }
        let l = line_text(doc.text(), line);
        l.get(f.item.edit_start..col) != Some(c.insert_text.as_str())
    }

    // ---- lifecycle (VS Code SuggestModel) ----

    /// Cancels the session (VS Code `cancel`).
    pub(crate) fn cancel(&mut self) {
        if self.session.take().is_some() {
            self.generation += 1;
        }
    }

    /// Starts a request (VS Code `trigger`).
    pub(crate) fn trigger(&mut self, doc: &Document, sel: &Selection, o: TriggerOptions) {
        let pos = sel.primary().head;
        self.cancel();
        self.next_id += 1;
        let id = CompletionRequestId(self.next_id);
        let text = doc.text();
        let ctx = ctx_at(doc, pos);
        let reused: Vec<SourceId> = o.reuse.keys().cloned().collect();
        let line_end = text.line_range(ctx.line).end;
        let suffix = (pos < line_end).then_some(pos);
        // A retrigger keeps showing the old list until the answer (VS Code does not hide the
        // widget on a retrigger).
        let stale = if o.retrigger {
            self.session.take().and_then(|old| {
                let shown = old.shown;
                let Some(model) = old.model else {
                    return old.stale;
                };
                shown.then(|| Box::new((model, old.focused, old.scroll_top, old.frozen)))
            })
        } else {
            None
        };
        self.session = Some(SuggestSession {
            id,
            auto: o.auto,
            trigger: o.kind.clone(),
            version: doc.version(),
            anchor: pos,
            suffix,
            ctx,
            model: None,
            reuse: o.reuse,
            focused: 0,
            scroll_top: 0,
            frozen: false,
            shown: false,
            resolve_requested: BTreeSet::new(),
            stale,
        });
        self.generation += 1;
        // Providers whose items are reused are not asked again (VS Code
        // `provideSuggestionItems` with `providerItemsToReuse`).
        let sources = match o.sources {
            Some(s) => Some(s.into_iter().filter(|s| !reused.contains(s)).collect::<Vec<_>>()),
            None if !reused.is_empty() && !doc.completion_sources().is_empty() => Some(
                doc.completion_sources()
                    .iter()
                    .map(|s| s.id.clone())
                    .filter(|s| !reused.contains(s))
                    .collect(),
            ),
            None => None,
        };
        if sources.as_ref().is_some_and(Vec::is_empty) {
            // Nothing to ask: the reused items are the answer.
            let _ = self.set_completions(doc, sel, id, doc.version(), Vec::new());
            return;
        }
        self.outbox.push(SuggestOutput::Request(CompletionRequest {
            id,
            pos,
            version: doc.version(),
            trigger: o.kind,
            auto: o.auto,
            sources,
        }));
    }

    /// The host's answer to request `id`, computed at `at` (ranges in bytes at `at`).
    pub(crate) fn set_completions(
        &mut self,
        doc: &Document,
        sel: &Selection,
        id: CompletionRequestId,
        at: Version,
        lists: Vec<CompletionList>,
    ) -> Result<(), CompletionRefusal> {
        let Some(s) = &mut self.session else {
            return Err(CompletionRefusal::Superseded);
        };
        if s.id != id {
            return Err(CompletionRefusal::Superseded);
        }
        if at > doc.version() {
            return Err(CompletionRefusal::Stale);
        }
        let since: Vec<ChangeSet> = doc
            .recent()
            .since(at)
            .map_err(|_| CompletionRefusal::Stale)?
            .map(|e| e.changes.clone())
            .collect();
        let cfg = &doc.config().suggest;
        let text = doc.text();
        let (req_line, req_col) = text.byte_to_line_col(s.anchor);
        let line_range = text.line_range(req_line);
        let mut items: Vec<Arc<SuggestItem>> = Vec::new();
        for (src, reused) in std::mem::take(&mut s.reuse) {
            if lists.iter().any(|l| l.source == src) {
                continue;
            }
            items.extend(reused);
        }
        for list in lists {
            for (index, mut item) in list.items.into_iter().enumerate() {
                if cfg.snippet_suggestions == SnippetSuggestions::None
                    && item.kind == CompletionItemKind::Snippet
                {
                    continue;
                }
                for c in &since {
                    item.range.map(c);
                    for (r, _) in &mut item.additional_text_edits {
                        *r = crate::diagnostics::map_range(r, c);
                    }
                }
                if item.sort_text.is_none() {
                    item.sort_text = Some(item.label.clone());
                }
                let ins = item.range.insert().clone();
                let rep = item.range.replace().clone();
                let inside = |r: &std::ops::Range<usize>| {
                    r.start >= line_range.start && r.end <= line_range.end && r.start <= r.end
                };
                let on_line = inside(&ins) && inside(&rep) && ins.start == rep.start;
                let col = |b: usize| b.saturating_sub(line_range.start);
                items.push(Arc::new(SuggestItem::new(
                    Arc::new(item),
                    list.source.clone(),
                    index,
                    list.incomplete,
                    req_col,
                    col(ins.start),
                    col(ins.end),
                    col(rep.end),
                    on_line,
                )));
            }
        }
        let snippets = cfg.snippet_suggestions;
        items.sort_by(|a, b| compare_items(a, b, snippets));
        let options = ModelOptions {
            filter_graceful: cfg.filter_graceful,
            snippet_suggestions: snippets,
            fuzzy: FuzzyScoreOptions {
                first_match_can_be_weak: !cfg.match_on_word_start_only,
                boost_full_match: true,
            },
        };
        let now = ctx_at(doc, sel.primary().head);
        s.model = Some(CompletionModel::new(
            items,
            s.ctx.column,
            LineContext {
                leading_line_content: now.leading_line_content.clone(),
                character_count_delta: now.column as isize - s.ctx.column as isize,
            },
            options,
        ));
        self.generation += 1;
        self.on_new_context(doc, sel, now);
        Ok(())
    }

    /// Replaces a resolved item (VS Code `resolveCompletionItem`): the host's detail,
    /// documentation and additional edits.
    pub(crate) fn resolve_item(
        &mut self,
        id: CompletionRequestId,
        source: &SourceId,
        index: usize,
        resolved: CompletionItem,
    ) -> bool {
        let Some(s) = &mut self.session else {
            return false;
        };
        if s.id != id {
            return false;
        }
        let Some(m) = &mut s.model else {
            return false;
        };
        let Some(old) = m
            .all_items()
            .iter()
            .find(|i| &i.source == source && i.index == index)
            .cloned()
        else {
            return false;
        };
        let mut new = (*old).clone();
        let mut c = (*old.completion).clone();
        c.detail = resolved.detail;
        c.documentation = resolved.documentation;
        c.additional_text_edits = resolved.additional_text_edits;
        c.needs_resolve = false;
        new.completion = Arc::new(c);
        m.replace_item(&old, Arc::new(new));
        self.generation += 1;
        true
    }

    /// VS Code `_refilterCompletionItems`.
    fn refilter(&mut self, doc: &Document, sel: &Selection) {
        let ctx = ctx_at(doc, sel.primary().head);
        self.on_new_context(doc, sel, ctx);
    }

    /// VS Code `_onNewContext`.
    fn on_new_context(&mut self, doc: &Document, sel: &Selection, ctx: Ctx) {
        let Some(s) = &mut self.session else {
            return;
        };
        if ctx.line != s.ctx.line {
            // e.g. Enter while IntelliSense is computed
            self.cancel();
            return;
        }
        if leading_whitespace(&ctx.leading_line_content)
            != leading_whitespace(&s.ctx.leading_line_content)
        {
            // the line start changed (the word was outdented)
            self.cancel();
            return;
        }
        if ctx.column < s.ctx.column {
            // moved left: retrigger if still on a word
            if !ctx.leading_word.word.is_empty() {
                let auto = s.auto;
                self.trigger(
                    doc,
                    sel,
                    TriggerOptions {
                        retrigger: true,
                        ..TriggerOptions::new(auto)
                    },
                );
            } else {
                self.cancel();
            }
            return;
        }
        let Some(model) = &mut s.model else {
            // not computed yet
            return;
        };
        if !ctx.leading_word.word.is_empty() && ctx.leading_word.start > s.ctx.leading_word.start
        {
            // started a new word while the list shows: retrigger, reusing all items
            if should_auto_trigger(doc, sel) {
                let reuse = model.items_by_source();
                let auto = s.auto;
                self.trigger(
                    doc,
                    sel,
                    TriggerOptions {
                        retrigger: true,
                        reuse,
                        ..TriggerOptions::new(auto)
                    },
                );
            }
            return;
        }
        if ctx.column > s.ctx.column
            && !model.incomplete_sources().is_empty()
            && !ctx.leading_word.word.is_empty()
        {
            // moved right with an incomplete list, still on a word: ask the incomplete
            // providers again
            let mut reuse = BTreeMap::new();
            let mut filter = Vec::new();
            for (src, items) in model.items_by_source() {
                if items.first().is_some_and(|i| i.incomplete) {
                    filter.push(src);
                } else {
                    reuse.insert(src, items);
                }
            }
            let auto = s.auto;
            self.trigger(
                doc,
                sel,
                TriggerOptions {
                    auto,
                    kind: CompletionTriggerKind::TriggerForIncompleteCompletions,
                    retrigger: true,
                    sources: Some(filter),
                    reuse,
                },
            );
            return;
        }
        // moved right: update the list
        let old = model.line_context().clone();
        let mut frozen = false;
        model.set_line_context(LineContext {
            leading_line_content: ctx.leading_line_content.clone(),
            character_count_delta: ctx.column as isize - s.ctx.column as isize,
        });
        if model.items().is_empty() {
            let auto_trigger = should_auto_trigger(doc, sel);
            if auto_trigger && s.ctx.leading_word.end < ctx.leading_word.start {
                // heading into a new word
                let auto = s.auto;
                self.trigger(
                    doc,
                    sel,
                    TriggerOptions {
                        retrigger: true,
                        ..TriggerOptions::new(auto)
                    },
                );
                return;
            }
            if !s.auto {
                // freeze when requested manually
                model.set_line_context(old);
                frozen = !model.items().is_empty();
                if frozen && ctx.leading_word.word.is_empty() {
                    // results before, none now, and not on a word
                    self.cancel();
                    return;
                }
            } else {
                // nothing left
                self.cancel();
                return;
            }
        }
        // onDidSuggest: the widget shows the list, focusing the preselected or first item
        let items = model.items();
        let mut index = 0;
        if let Some(top) = items.first().map(|f| f.score.score) {
            for (i, f) in items.iter().enumerate() {
                if f.score.score != top {
                    break;
                }
                if f.item.completion.preselect {
                    index = i;
                    break;
                }
            }
        }
        s.frozen = frozen;
        s.shown = true;
        s.focused = index;
        s.scroll_top = 0;
        Self::ensure_visible(s);
        self.generation += 1;
        self.request_resolve();
    }

    fn ensure_visible(s: &mut SuggestSession) {
        if s.focused < s.scroll_top {
            s.scroll_top = s.focused;
        } else if s.focused >= s.scroll_top + VISIBLE_ROWS {
            s.scroll_top = s.focused + 1 - VISIBLE_ROWS;
        }
    }

    fn request_resolve(&mut self) {
        let Some(f) = self.focused_item().cloned() else {
            return;
        };
        if !f.item.completion.needs_resolve {
            return;
        }
        let Some(s) = &mut self.session else {
            return;
        };
        if s.resolve_requested.insert((f.item.source.clone(), f.item.index)) {
            self.outbox.push(SuggestOutput::Resolve {
                id: s.id,
                source: f.item.source.clone(),
                index: f.item.index,
                item: f.item.completion.clone(),
            });
        }
    }

    /// After a transaction through the view (not one of the suggest commands): VS Code's
    /// content-change, cursor-change, type and composition listeners.
    pub(crate) fn after_change(
        &mut self,
        doc: &Document,
        sel: &Selection,
        kind: ChangeKind,
        changed_text: bool,
        composing: bool,
        in_snippet: bool,
    ) {
        if composing {
            return;
        }
        let empty = sel.ranges().iter().all(|r| r.is_empty());
        match kind {
            ChangeKind::CompositionEnd => {
                if self.session.is_none() {
                    self.quick_suggest(doc, sel, in_snippet);
                } else {
                    self.refilter(doc, sel);
                }
                self.check_trigger_character(doc, sel);
                return;
            }
            ChangeKind::Other => {
                self.cancel();
                return;
            }
            _ => {}
        }
        if !empty {
            self.cancel();
            return;
        }
        // content changed while active: refilter
        if changed_text && self.session.is_some() {
            self.refilter(doc, sel);
        }
        match kind {
            ChangeKind::Typed => {
                if self.session.is_none() {
                    // typing moved the cursor right: quick suggest
                    self.quick_suggest(doc, sel, in_snippet);
                }
                self.check_trigger_character(doc, sel);
            }
            ChangeKind::CursorMoved if self.session.is_some() && !changed_text => {
                self.refilter(doc, sel)
            }
            _ => {}
        }
    }

    /// VS Code `_doTriggerQuickSuggest` (no delay, see the module docs).
    fn quick_suggest(&mut self, doc: &Document, sel: &Selection, in_snippet: bool) {
        let cfg = &doc.config().suggest;
        let q = cfg.quick_suggestions;
        if !q.other && !q.comments && !q.strings {
            return;
        }
        if cfg.snippets_prevent_quick_suggestions && in_snippet {
            return;
        }
        if doc.completion_sources().is_empty() || !should_auto_trigger(doc, sel) {
            return;
        }
        if !(q.other && q.comments && q.strings) {
            // the token type before the cursor decides
            let pos = sel.primary().head;
            let (line, col) = doc.text().byte_to_line_col(pos);
            let at = col.saturating_sub(1);
            let ty = if line < doc.tokens().len_lines() {
                doc.tokens().style_at(line, at).ty()
            } else {
                TokenType::Text
            };
            let on = match ty {
                TokenType::Comment => q.comments,
                TokenType::String => q.strings,
                _ => q.other,
            };
            if !on {
                return;
            }
        }
        self.trigger(doc, sel, TriggerOptions::new(true));
    }

    /// VS Code's `checkTriggerCharacter` on type.
    fn check_trigger_character(&mut self, doc: &Document, sel: &Selection) {
        if !doc.config().suggest.suggest_on_trigger_characters || doc.config().read_only {
            return;
        }
        if should_auto_trigger(doc, sel) {
            // quick suggestions cover it
            return;
        }
        let pos = sel.primary().head;
        let Some(last) = doc.text().char_before(pos) else {
            return;
        };
        let mut b = [0u8; 4];
        let ch = last.encode_utf8(&mut b).to_string();
        let supports: Vec<SourceId> = doc
            .completion_sources()
            .iter()
            .filter(|s| s.trigger_characters.iter().any(|t| *t == ch))
            .map(|s| s.id.clone())
            .collect();
        if supports.is_empty() {
            return;
        }
        // keep the items of providers that do not trigger now
        let mut reuse = BTreeMap::new();
        let mut retrigger = false;
        if let Some(m) = self.session.as_mut().and_then(|s| s.model.as_mut()) {
            retrigger = true;
            for (src, items) in m.items_by_source() {
                if !supports.contains(&src) {
                    reuse.insert(src, items);
                }
            }
        }
        self.trigger(
            doc,
            sel,
            TriggerOptions {
                auto: true,
                kind: CompletionTriggerKind::TriggerCharacter(ch),
                retrigger,
                sources: Some(supports),
                reuse,
            },
        );
    }

    // ---- the widget's list ----

    /// Moves the focus: by one row wrapping around (`selectNextSuggestion` /
    /// `selectPrevSuggestion`), or by a page, clamped (`selectNextPageSuggestion` /
    /// `selectPrevPageSuggestion`).
    pub(crate) fn select(&mut self, delta: isize, page: bool) {
        let n = self.items().len();
        let Some(s) = &mut self.session else {
            return;
        };
        if n == 0 {
            return;
        }
        let cur = s.focused.min(n - 1);
        s.focused = if page {
            let rows = VISIBLE_ROWS as isize - 1;
            if delta > 0 {
                // the last visible row, or a page further
                let last_visible = (s.scroll_top + VISIBLE_ROWS - 1).min(n - 1);
                if cur < last_visible {
                    last_visible
                } else {
                    (cur as isize + rows).min(n as isize - 1) as usize
                }
            } else if cur > s.scroll_top {
                s.scroll_top
            } else {
                (cur as isize - rows).max(0) as usize
            }
        } else {
            (cur as isize + delta).rem_euclid(n as isize) as usize
        };
        Self::ensure_visible(s);
        self.generation += 1;
        self.request_resolve();
    }

    /// Focuses a row (mouse).
    pub fn focus_row(&mut self, row: usize) {
        let n = self.items().len();
        if let Some(s) = &mut self.session
            && row < n
        {
            s.focused = row;
            Self::ensure_visible(s);
            self.generation += 1;
            self.request_resolve();
        }
    }

    /// `toggleSuggestionDetails`.
    pub(crate) fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
        self.generation += 1;
    }

    /// Maps the session's positions through a change.
    pub(crate) fn map(&mut self, c: &ChangeSet) {
        if let Some(s) = &mut self.session {
            s.anchor = c.map_pos(s.anchor, Bias::Before);
            s.suffix = s.suffix.map(|p| c.map_pos(p, Bias::After));
        }
    }

    /// The suffix delta at the cursor (VS Code `LineSuffix.delta`).
    pub(crate) fn suffix_delta(&self, doc: &Document, pos: usize) -> usize {
        let Some(s) = &self.session else {
            return 0;
        };
        let text = doc.text();
        if text.byte_to_line(s.anchor) != text.byte_to_line(pos) {
            return 0;
        }
        match s.suffix {
            Some(p) => p.saturating_sub(pos),
            None => text.line_range(text.byte_to_line(pos)).end.saturating_sub(pos),
        }
    }

    pub(crate) fn push_output(&mut self, o: SuggestOutput) {
        self.outbox.push(o);
    }
}
