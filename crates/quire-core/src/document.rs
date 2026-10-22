//! [`Document`]: the shared model of one file, and its one mutation path.
//!
//! **The split (SEAM-CHANGES items 1 and 6).** A `Document` holds what belongs to the file: text,
//! version, history, undo mode, recent changes, tokens, diagnostics, `saved_at`, config. An
//! [`EditorView`] holds what belongs to one view of it: selection, folds, find state, scroll,
//! focus, IME preedit. Two views of one document keep separate carets.
//!
//! **Applying.** [`Document::apply`]`(tr, acting_view)` is the only mutation path for local edits
//! and [`Document::apply_remote`] the path for foreign ones. The acting view is **synced first**
//! (mapped forward through the recent changes, or clamped if it fell out of the window; see
//! [`EditorView::sync`]) rather than refused, so a view that missed edits made through another
//! view can still act; the transaction itself carries its own `base` and is rebased or refused
//! on that. Other views do not need the host to fan out every [`Applied`]: each view records the
//! version it last synced at and calls [`EditorView::sync`] before use (the widget does it in
//! `show_in`, [`Document::command`] works on a synced copy). [`EditorView::map_through`] is there
//! for hosts that do fan out.
//!
//! **Atomicity.** A transaction is validated completely (base, lengths, char boundaries, no `\r`,
//! read-only, selection bounds) against the new text before anything changes; a refusal leaves
//! document and view untouched.
//!
//! **Versions** bump only when text changes. Selection-only transactions bump the view's
//! `selection_version` only.

use crate::change::{Bias, ChangeSet};
use crate::command::{CommandOutput, EditorCommand, HostRequest};
use crate::config::EditorConfig;
use crate::diagnostics::{Diagnostic, Diagnostics, SourceId};
use crate::history::{UndoGroupClosed, UndoMode};
use crate::large::Degraded;
use crate::recent::{LineEdit, RecentChanges, Stale};
use crate::syntax::{FoldRange, PendingWork, TokenRuns, TokenStore};
use crate::text::Text;
use crate::transaction::{AuthorId, Effect, Origin, Transaction};
use crate::version::Version;
use crate::view::EditorView;
use quire_text::{DocRules, EditRules, TextDocument, WordClassifier, WordRules};
use smallvec::SmallVec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub use quire_text::{Anchor, Snapshot};

/// Why a transaction was refused. Nothing changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The transaction's base is older than the recent-changes window (or newer than the
    /// document), so it cannot be rebased.
    Stale(Stale),
    /// The change set's length does not match the text at the base.
    LengthMismatch {
        /// The text length.
        expected: usize,
        /// The change set's length.
        got: usize,
    },
    /// An edit boundary or selection end falls inside a UTF-8 sequence or past the end.
    NotCharBoundary(usize),
    /// Inserted text contains `\r` (normalise line endings first; the text is LF in memory).
    CarriageReturn,
    /// The document is read-only (foreign changes still apply).
    ReadOnly,
    /// A change set could not be rebased.
    Rebase(crate::change::ChangeError),
    /// A find regex did not compile.
    InvalidQuery(String),
    /// Inserted text contains a line break and the document is single-line (not reachable from
    /// an [`EditorConfig`]).
    LineBreak,
    /// The text would exceed the character limit (not reachable from an [`EditorConfig`]).
    MaxChars,
}

impl From<quire_text::Refusal> for Refusal {
    fn from(r: quire_text::Refusal) -> Self {
        use quire_text::Refusal as R;
        match r {
            R::Stale(s) => Refusal::Stale(s),
            R::LengthMismatch { expected, got } => Refusal::LengthMismatch { expected, got },
            R::NotCharBoundary(p) => Refusal::NotCharBoundary(p),
            R::CarriageReturn => Refusal::CarriageReturn,
            R::ReadOnly => Refusal::ReadOnly,
            R::Rebase(e) => Refusal::Rebase(e),
            R::LineBreak => Refusal::LineBreak,
            R::MaxChars => Refusal::MaxChars,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Stale(s) => write!(f, "{s}"),
            Refusal::LengthMismatch { expected, got } => {
                write!(f, "change set for length {got}, text has {expected}")
            }
            Refusal::NotCharBoundary(p) => write!(f, "position {p} is not a char boundary"),
            Refusal::CarriageReturn => f.write_str("inserted text contains a carriage return"),
            Refusal::ReadOnly => f.write_str("the document is read-only"),
            Refusal::Rebase(e) => write!(f, "rebase failed: {e}"),
            Refusal::InvalidQuery(e) => write!(f, "invalid query: {e}"),
            Refusal::LineBreak => f.write_str("the document is single-line"),
            Refusal::MaxChars => f.write_str("the text would exceed the character limit"),
        }
    }
}
impl std::error::Error for Refusal {}

/// The record of an applied transaction.
#[derive(Clone, PartialEq, Debug)]
pub struct Applied {
    /// The version before.
    pub prev_version: Version,
    /// The version after (equal to `prev_version` for selection-only transactions).
    pub version: Version,
    /// The text changes as applied (rebased if the base was old).
    pub changes: ChangeSet,
    /// The origin.
    pub origin: Origin,
    /// Who made it ([`Transaction::author`]).
    pub author: AuthorId,
    /// Whether the acting view's selection changed.
    pub selection_changed: bool,
    /// Whether an effect changed the document's config ([`Effect::SetInsertSpaces`],
    /// [`Effect::SetWordWrap`]); a host
    /// that persists editor options reads [`Document::config`] then.
    pub config_changed: bool,
    /// Undo groups this transaction closed (the host's `text.edit` boundaries in host mode).
    pub closed: SmallVec<[UndoGroupClosed; 1]>,
}

impl Applied {
    fn from_text(a: quire_text::Applied, selection_changed: bool, config_changed: bool) -> Self {
        Applied {
            prev_version: a.prev_version,
            version: a.version,
            changes: a.changes,
            origin: a.origin,
            author: a.author,
            selection_changed,
            config_changed,
            closed: a.closed,
        }
    }

    /// Whether text changed.
    pub fn changed_text(&self) -> bool {
        self.version != self.prev_version
    }
}

/// The result of [`Document::execute`].
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Executed {
    /// The applied transaction, if the command made one.
    pub applied: Option<Applied>,
    /// A request for the host, if the command made one.
    pub request: Option<HostRequest>,
}

/// A cheap `Send + Sync` handle that answers "is the document settled?" from another thread
/// (svelters' `Widget::busy()`, SEAM-CHANGES item 4).
#[derive(Clone, Debug)]
pub struct SettledProbe {
    pending: PendingWork,
    open_group: Arc<AtomicBool>,
}

impl SettledProbe {
    /// No pending background work and no undo group waiting to close in host mode.
    pub fn is_settled(&self) -> bool {
        self.pending.count() == 0 && !self.open_group.load(Ordering::Acquire)
    }
}

/// One file's shared model. See the module docs.
#[derive(Debug)]
pub struct Document {
    /// Text, version, history and recent changes (quire-text).
    doc: TextDocument,
    /// The command rules the config implies, with its word classifier (rebuilt only in
    /// `set_config`).
    edit_rules: EditRules,
    tokens: TokenStore,
    diagnostics: Diagnostics,
    saved_at: Option<Version>,
    saved_text: Option<Text>,
    dirty: bool,
    config: EditorConfig,
    pending: PendingWork,
    open_group: Arc<AtomicBool>,
    syntax_folds: Option<Vec<FoldRange>>,
    indent_folds: OnceLock<Vec<FoldRange>>,
    /// Indentation levels behind `indent_folds`, followed incrementally once computed.
    indent_levels: OnceLock<crate::folding::IndentLevels>,
    degraded: Option<Degraded>,
}

impl Document {
    /// A document from file contents (BOM and line ending detected and recorded; see
    /// [`Text::from_file`]). It starts saved, in local undo mode.
    pub fn new(contents: &str, config: EditorConfig) -> Self {
        Document::from_text(Text::from_file(contents), config)
    }

    /// A document from a [`Text`].
    pub fn from_text(text: Text, config: EditorConfig) -> Self {
        let lines = text.len_lines();
        Document {
            tokens: TokenStore::new(lines),
            diagnostics: Diagnostics::default(),
            saved_at: Some(Version::INITIAL),
            saved_text: Some(text.clone()),
            dirty: false,
            pending: PendingWork::default(),
            open_group: Arc::new(AtomicBool::new(false)),
            syntax_folds: None,
            indent_folds: OnceLock::new(),
            indent_levels: OnceLock::new(),
            degraded: crate::large::check(&text, &config.large_file),
            doc: TextDocument::from_text(text, Arc::new(doc_rules(&config, UndoMode::Local))),
            edit_rules: edit_rules(&config),
            config,
        }
    }

    /// The quire-text command rules the config implies (VS Code's word rules with the config's
    /// separators, smart home, the empty-selection clipboard), built once per config.
    pub fn edit_rules(&self) -> &EditRules {
        &self.edit_rules
    }

    /// The quire-text document underneath (text, version, history, recent changes).
    pub fn text_document(&self) -> &TextDocument {
        &self.doc
    }

    /// The same document in another undo mode.
    #[must_use]
    pub fn with_undo_mode(mut self, mode: UndoMode) -> Self {
        self.set_undo_mode(mode);
        self
    }

    /// Switches undo mode: closes the open group (returned) and clears the local stacks.
    pub fn set_undo_mode(&mut self, mode: UndoMode) -> Option<UndoGroupClosed> {
        let c = self.doc.set_undo_mode(mode);
        self.update_probe();
        c
    }

    /// The undo mode.
    pub fn undo_mode(&self) -> UndoMode {
        self.doc.undo_mode()
    }

    /// The text.
    pub fn text(&self) -> &Text {
        self.doc.text()
    }

    /// The version.
    pub fn version(&self) -> Version {
        self.doc.version()
    }

    /// An O(1) snapshot of the text and version.
    pub fn snapshot(&self) -> Snapshot {
        self.doc.snapshot()
    }

    /// The config.
    pub fn config(&self) -> &EditorConfig {
        &self.config
    }

    /// Replaces the config. A changed recent-changes window applies to later changes.
    pub fn set_config(&mut self, config: EditorConfig) {
        if config.recent_changes_window != self.config.recent_changes_window
            || config.undo_pause_ms != self.config.undo_pause_ms
            || config.read_only != self.config.read_only
        {
            let rules = doc_rules(&config, self.doc.undo_mode());
            self.doc.set_rules(Arc::new(rules));
        }
        if config.word_separators != self.config.word_separators
            || config.smart_home != self.config.smart_home
            || config.empty_selection_clipboard != self.config.empty_selection_clipboard
        {
            self.edit_rules = edit_rules(&config);
        }
        if config.large_file != self.config.large_file {
            self.degraded = crate::large::check(self.doc.text(), &config.large_file);
        }
        if config.tab_size != self.config.tab_size {
            self.indent_folds = OnceLock::new();
            self.indent_levels = OnceLock::new();
        }
        self.config = config;
    }

    /// Why the document is past the large-file thresholds, if it is (decided at load and on a
    /// reload, as in Monaco).
    pub fn degraded(&self) -> Option<&Degraded> {
        self.degraded.as_ref()
    }

    /// Stores fold ranges from a syntax provider, computed at version `at` (mapped forward by
    /// lines if older; refused outside the recent-changes window). They replace the indentation
    /// ranges until [`Document::clear_fold_ranges`].
    pub fn set_fold_ranges(
        &mut self,
        at: Version,
        mut ranges: Vec<FoldRange>,
    ) -> Result<(), Stale> {
        for e in self.doc.recent().since(at)? {
            crate::folding::map_fold_ranges(&mut ranges, &e.line_edits);
        }
        let last = self.doc.text().len_lines().saturating_sub(1);
        ranges.retain(|r| r.start_line < r.end_line && r.end_line <= last);
        ranges.sort_by_key(|r| (r.start_line, std::cmp::Reverse(r.end_line)));
        self.syntax_folds = Some(ranges);
        Ok(())
    }

    /// Drops syntax fold ranges (back to indentation ranges).
    pub fn clear_fold_ranges(&mut self) {
        self.syntax_folds = None;
    }

    /// The fold ranges: the syntax provider's if set, else ranges from indentation (computed
    /// once per version). Empty when the document is degraded past syntax features.
    pub fn fold_ranges(&self) -> &[FoldRange] {
        if self.degraded.as_ref().is_some_and(Degraded::syntax_off) {
            return &[];
        }
        if let Some(r) = &self.syntax_folds {
            return r;
        }
        self.indent_folds.get_or_init(|| {
            self.indent_levels
                .get_or_init(|| {
                    crate::folding::IndentLevels::compute(self.doc.text(), self.config.tab_size)
                })
                .ranges()
        })
    }

    /// The history.
    pub fn history(&self) -> &crate::history::History {
        self.doc.history()
    }

    /// The recent changes.
    pub fn recent(&self) -> &RecentChanges {
        self.doc.recent()
    }

    /// The token store.
    pub fn tokens(&self) -> &TokenStore {
        &self.tokens
    }

    /// The diagnostics.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// The version last marked saved.
    pub fn saved_at(&self) -> Option<Version> {
        self.saved_at
    }

    /// Records that the text at `at` was saved. If `at` is older than the current version (the
    /// user typed while saving), the document stays dirty.
    pub fn mark_saved(&mut self, at: Version) {
        self.saved_at = Some(at);
        if at == self.doc.version() {
            self.saved_text = Some(self.doc.text().clone());
            self.dirty = false;
        } else {
            self.saved_text = None;
            self.dirty = true;
        }
    }

    /// Whether the text differs from the last save. Undo or redo back to the saved text makes
    /// the document clean again.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The pending-work counter the syntax driver and host debounces register with.
    pub fn pending_work(&self) -> &PendingWork {
        &self.pending
    }

    /// No pending background work, and (in host mode) no undo group waiting to close.
    pub fn is_settled(&self) -> bool {
        self.settled_probe().is_settled()
    }

    /// A `Send + Sync` handle for [`Document::is_settled`].
    pub fn settled_probe(&self) -> SettledProbe {
        SettledProbe {
            pending: self.pending.clone(),
            open_group: self.open_group.clone(),
        }
    }

    fn update_probe(&self) {
        let open = self.doc.undo_mode() == UndoMode::Host && self.doc.has_open_group();
        self.open_group.store(open, Ordering::Release);
    }

    /// Closes the open undo group (focus change, explicit stop).
    pub fn close_undo_group(&mut self) -> Option<UndoGroupClosed> {
        let c = self.doc.close_undo_group();
        self.update_probe();
        c
    }

    /// Closes the open undo group if the pause since its last edit exceeds
    /// `EditorConfig::undo_pause_ms` at the host's clock `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Option<UndoGroupClosed> {
        let c = self.doc.tick(now_ms);
        self.update_probe();
        c
    }

    /// An anchor at `pos` now.
    pub fn anchor(&self, pos: usize, bias: Bias) -> Anchor {
        self.doc.anchor(pos, bias)
    }

    /// Where an anchor is now.
    pub fn resolve_anchor(&self, a: &Anchor) -> Result<usize, Stale> {
        self.doc.resolve_anchor(a)
    }

    /// Replaces one source's diagnostics, computed at version `at`. Mapped forward if the text
    /// moved on; refused as stale outside the window. An empty list clears the source.
    pub fn set_diagnostics(
        &mut self,
        source: SourceId,
        at: Version,
        mut diags: Vec<Diagnostic>,
    ) -> Result<(), Stale> {
        let len_at = self.len_at(at)?;
        for d in &mut diags {
            d.clamp(len_at);
        }
        for e in self.doc.recent().since(at)? {
            for d in &mut diags {
                d.map(&e.changes);
            }
        }
        let text = self.doc.text();
        let len = text.len_bytes();
        for d in &mut diags {
            d.clamp(len);
            d.range.start = text.clamp_to_boundary(d.range.start);
            d.range.end = text.clamp_to_boundary(d.range.end);
        }
        self.diagnostics.replace(source, diags);
        Ok(())
    }

    fn len_at(&self, at: Version) -> Result<usize, Stale> {
        let mut it = self.doc.recent().since(at)?;
        Ok(it
            .next()
            .map_or(self.doc.text().len_bytes(), |e| e.changes.len()))
    }

    /// Stores tokens computed at version `at`. Stale runs are shifted by whole lines past edits
    /// before them and cut at the first line an edit touched; refused outside the window.
    pub fn set_tokens(&mut self, at: Version, runs: TokenRuns) -> Result<(), Stale> {
        let since: Vec<&[LineEdit]> = self
            .doc
            .recent()
            .since(at)?
            .map(|e| e.line_edits.as_slice())
            .collect();
        if let Some(runs) = crate::syntax::map_runs(runs, since.into_iter()) {
            self.tokens.set(runs);
        }
        Ok(())
    }

    /// Marks lines from `line` as needing tokens (e.g. the highlighter changed).
    pub fn invalidate_tokens_from(&mut self, line: usize) {
        self.tokens.invalidate_from(line);
    }

    /// Resolves a command against a view without applying it (pure). The view is synced on a
    /// copy first if it is behind.
    pub fn command(
        &self,
        view: &EditorView,
        cmd: &EditorCommand,
    ) -> Result<CommandOutput, Refusal> {
        if view.synced_at() != self.doc.version() {
            let mut v = view.clone();
            v.sync(self);
            return crate::commands::run(self, &v, cmd);
        }
        crate::commands::run(self, view, cmd)
    }

    /// Resolves and applies a command. `time_ms` is the host clock for undo grouping.
    pub fn execute(
        &mut self,
        view: &mut EditorView,
        cmd: &EditorCommand,
        time_ms: Option<u64>,
    ) -> Result<Executed, Refusal> {
        self.execute_as(view, cmd, time_ms, AuthorId::USER)
    }

    /// [`Document::execute`] on behalf of `author`: the transaction carries it, except an undo
    /// or redo, which keeps the author of the group it reverts.
    pub fn execute_as(
        &mut self,
        view: &mut EditorView,
        cmd: &EditorCommand,
        time_ms: Option<u64>,
        author: AuthorId,
    ) -> Result<Executed, Refusal> {
        view.sync(self);
        match self.command(view, cmd)? {
            CommandOutput::Transaction(mut tr) => {
                tr.time_ms = time_ms.or(tr.time_ms);
                if !matches!(tr.origin, Origin::Undo | Origin::Redo) {
                    tr.author = author;
                }
                Ok(Executed {
                    applied: Some(self.apply(tr, view)?),
                    request: None,
                })
            }
            CommandOutput::Request(r) => Ok(Executed {
                applied: None,
                request: Some(r),
            }),
            CommandOutput::Nothing => Ok(Executed::default()),
        }
    }

    /// Applies a transaction made through `view` (see the module docs: the view is synced first,
    /// the transaction rebased if its base is old).
    pub fn apply(&mut self, tr: Transaction, view: &mut EditorView) -> Result<Applied, Refusal> {
        view.sync(self);
        let selection_before = view.selection().clone();
        let (applied, effects) = self.doc.apply_with(tr, view.text_view_mut())?;
        self.after_apply(&applied);
        view.after_apply(&applied);
        let config_changed = self.apply_doc_effects(&effects);
        for e in &effects {
            view.apply_effect(e, self);
        }
        let selection_changed = selection_before != *view.selection();
        self.update_probe();
        Ok(Applied::from_text(
            applied,
            selection_changed,
            config_changed,
        ))
    }

    /// Applies a foreign change (a collaborator's, merged by the host; or a reload, e.g.
    /// `ChangeSet::diff(old, new)` with `Origin::Reload`). Not recorded in local history; the
    /// stacks are rebased over it. Views catch up on their next `sync`.
    pub fn apply_remote(
        &mut self,
        base: Version,
        changes: ChangeSet,
        origin: Origin,
    ) -> Result<Applied, Refusal> {
        self.apply_remote_by(base, changes, origin, AuthorId::USER)
    }

    /// [`Document::apply_remote`] made by `author` (a collaborator, a tool, a reload).
    pub fn apply_remote_by(
        &mut self,
        base: Version,
        changes: ChangeSet,
        origin: Origin,
        author: AuthorId,
    ) -> Result<Applied, Refusal> {
        let applied = self.doc.apply_remote(base, changes, origin, author)?;
        self.after_apply(&applied);
        self.update_probe();
        Ok(Applied::from_text(applied, false, false))
    }

    /// What the document keeps beyond quire-text, after a text change: tokens, folds,
    /// diagnostics, the large-file check on reload, dirty tracking.
    fn after_apply(&mut self, applied: &quire_text::Applied) {
        if !applied.changed_text() {
            return;
        }
        let line_edits = &self
            .doc
            .recent()
            .latest()
            .expect("a text change is recorded")
            .line_edits;
        self.tokens.shift(line_edits);
        if let Some(f) = &mut self.syntax_folds {
            crate::folding::map_fold_ranges(f, line_edits);
        }
        // Indentation folds: follow the levels incrementally, and the ranges around the
        // edited lines only (E1 P3).
        match self.indent_levels.get_mut() {
            Some(lv) => match lv.update(self.doc.text(), line_edits) {
                crate::folding::LevelChange::None => {}
                crate::folding::LevelChange::Lines { old, new } => {
                    if let Some(r) = self.indent_folds.get_mut() {
                        lv.update_ranges(r, old, new);
                    }
                }
                crate::folding::LevelChange::All => self.indent_folds = OnceLock::new(),
            },
            None => self.indent_folds = OnceLock::new(),
        }
        self.diagnostics.map(&applied.changes);
        if matches!(applied.origin, Origin::Reload) {
            self.degraded = crate::large::check(self.doc.text(), &self.config.large_file);
        }
        // Dirty tracking: compare with the saved text only when an undo/redo may have
        // returned to it (cheap length check first).
        let text = self.doc.text();
        self.dirty = match &self.saved_text {
            Some(saved)
                if matches!(applied.origin, Origin::Undo | Origin::Redo)
                    && saved.len_bytes() == text.len_bytes() =>
            {
                saved != text
            }
            _ => true,
        };
    }

    /// Effects that act on the document; returns whether the config changed.
    fn apply_doc_effects(&mut self, effects: &[Effect]) -> bool {
        let mut config_changed = false;
        for e in effects {
            match e {
                Effect::SetInsertSpaces(b) if self.config.insert_spaces != *b => {
                    self.config.insert_spaces = *b;
                    config_changed = true;
                }
                Effect::SetWordWrap(w) if self.config.word_wrap != *w => {
                    self.config.word_wrap = *w;
                    config_changed = true;
                }
                _ => {}
            }
        }
        config_changed
    }
}

/// The quire-text command rules an [`EditorConfig`] implies.
fn edit_rules(config: &EditorConfig) -> EditRules {
    EditRules {
        words: WordRules::VsCode(WordClassifier::new(&config.word_separators)),
        smart_home: config.smart_home,
        empty_selection_clipboard: config.empty_selection_clipboard,
        ..EditRules::default()
    }
}

/// The quire-text rules an [`EditorConfig`] implies.
fn doc_rules(config: &EditorConfig, undo_mode: UndoMode) -> DocRules {
    DocRules {
        undo_mode,
        undo_pause_ms: Some(config.undo_pause_ms),
        read_only: config.read_only,
        recent_window: config.recent_changes_window,
        track_line_edits: true,
        ..DocRules::default()
    }
}
