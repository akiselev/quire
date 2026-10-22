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
use crate::history::{History, Record, UndoGroupClosed, UndoMode};
use crate::recent::{LineEdit, RecentChanges, Stale};
use crate::selection::Selection;
use crate::syntax::{PendingWork, TokenRuns, TokenStore};
use crate::text::Text;
use crate::transaction::{Origin, Transaction};
use crate::version::Version;
use crate::view::EditorView;
use smallvec::SmallVec;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// An immutable view of the text at a version. Cloning is O(1); `Send + Sync`, for background
/// work (parsing, compiling, saving).
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// The text.
    pub text: Text,
    /// Its version.
    pub version: Version,
}

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
    /// Whether the acting view's selection changed.
    pub selection_changed: bool,
    /// Undo groups this transaction closed (the host's `text.edit` boundaries in host mode).
    pub closed: SmallVec<[UndoGroupClosed; 1]>,
}

impl Applied {
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

/// A persistent position: a byte offset at a version with a bias. Resolve it with
/// [`Document::resolve_anchor`], which maps it through the recent changes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Anchor {
    /// The offset at `version`.
    pub pos: usize,
    /// How it maps through insertions at its position.
    pub bias: Bias,
    /// The version `pos` belongs to.
    pub version: Version,
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
    text: Text,
    version: Version,
    history: History,
    recent: RecentChanges,
    tokens: TokenStore,
    diagnostics: Diagnostics,
    saved_at: Option<Version>,
    saved_text: Option<Text>,
    dirty: bool,
    config: EditorConfig,
    pending: PendingWork,
    open_group: Arc<AtomicBool>,
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
            recent: RecentChanges::new(config.recent_changes_window, Version::INITIAL),
            tokens: TokenStore::new(lines),
            diagnostics: Diagnostics::default(),
            history: History::new(UndoMode::Local),
            saved_at: Some(Version::INITIAL),
            saved_text: Some(text.clone()),
            dirty: false,
            version: Version::INITIAL,
            text,
            config,
            pending: PendingWork::default(),
            open_group: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The same document in another undo mode.
    #[must_use]
    pub fn with_undo_mode(mut self, mode: UndoMode) -> Self {
        self.set_undo_mode(mode);
        self
    }

    /// Switches undo mode: closes the open group (returned) and clears the local stacks.
    pub fn set_undo_mode(&mut self, mode: UndoMode) -> Option<UndoGroupClosed> {
        let c = self.history.set_mode(mode);
        self.update_probe();
        c
    }

    /// The undo mode.
    pub fn undo_mode(&self) -> UndoMode {
        self.history.mode()
    }

    /// The text.
    pub fn text(&self) -> &Text {
        &self.text
    }

    /// The version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// An O(1) snapshot of the text and version.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            version: self.version,
        }
    }

    /// The config.
    pub fn config(&self) -> &EditorConfig {
        &self.config
    }

    /// Replaces the config. A changed recent-changes window applies to later changes.
    pub fn set_config(&mut self, config: EditorConfig) {
        if config.recent_changes_window != self.recent.capacity() {
            let mut r = RecentChanges::new(config.recent_changes_window, self.version);
            // Keep what fits.
            if let Ok(it) = self.recent.since(self.recent.oldest()) {
                let entries: Vec<_> = it.cloned().collect();
                r = RecentChanges::new(
                    config.recent_changes_window,
                    entries.first().map_or(self.version, |e| e.from),
                );
                for e in entries {
                    r.push(e.changes, e.line_edits);
                }
            }
            self.recent = r;
        }
        self.config = config;
    }

    /// The history.
    pub fn history(&self) -> &crate::history::History {
        &self.history
    }

    /// The recent changes.
    pub fn recent(&self) -> &RecentChanges {
        &self.recent
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
        if at == self.version {
            self.saved_text = Some(self.text.clone());
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
        let open = self.history.mode() == UndoMode::Host && self.history.has_open_group();
        self.open_group.store(open, Ordering::Release);
    }

    /// Closes the open undo group (focus change, explicit stop).
    pub fn close_undo_group(&mut self) -> Option<UndoGroupClosed> {
        let c = self.history.close();
        self.update_probe();
        c
    }

    /// Closes the open undo group if the pause since its last edit exceeds
    /// `EditorConfig::undo_pause_ms` at the host's clock `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Option<UndoGroupClosed> {
        let c = self.history.tick(now_ms, self.config.undo_pause_ms);
        self.update_probe();
        c
    }

    /// An anchor at `pos` now.
    pub fn anchor(&self, pos: usize, bias: Bias) -> Anchor {
        Anchor {
            pos,
            bias,
            version: self.version,
        }
    }

    /// Where an anchor is now.
    pub fn resolve_anchor(&self, a: &Anchor) -> Result<usize, Stale> {
        self.recent
            .map_pos(a.version, a.pos, a.bias)
            .map(|p| self.text.clamp_to_boundary(p))
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
        for e in self.recent.since(at)? {
            for d in &mut diags {
                d.map(&e.changes);
            }
        }
        let len = self.text.len_bytes();
        for d in &mut diags {
            d.clamp(len);
            d.range.start = self.text.clamp_to_boundary(d.range.start);
            d.range.end = self.text.clamp_to_boundary(d.range.end);
        }
        self.diagnostics.replace(source, diags);
        Ok(())
    }

    fn len_at(&self, at: Version) -> Result<usize, Stale> {
        let mut it = self.recent.since(at)?;
        Ok(it.next().map_or(self.text.len_bytes(), |e| e.changes.len()))
    }

    /// Stores tokens computed at version `at`. Stale runs are shifted by whole lines past edits
    /// before them and cut at the first line an edit touched; refused outside the window.
    pub fn set_tokens(&mut self, at: Version, runs: TokenRuns) -> Result<(), Stale> {
        let since: Vec<&[LineEdit]> = self
            .recent
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
        if view.synced_at() != self.version {
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
        view.sync(self);
        match self.command(view, cmd)? {
            CommandOutput::Transaction(mut tr) => {
                tr.time_ms = time_ms.or(tr.time_ms);
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
        self.apply_inner(tr, Some(view))
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
        let tr = Transaction::new(base, changes).with_origin(origin);
        self.apply_inner(tr, None)
    }

    fn rebase(&self, tr: &mut Transaction) -> Result<(), Refusal> {
        if tr.base == self.version {
            return Ok(());
        }
        let r = self
            .recent
            .composed_since(tr.base)
            .map_err(Refusal::Stale)?
            .expect("base differs from version");
        if tr.changes.len() != r.len() {
            return Err(Refusal::LengthMismatch {
                expected: r.len(),
                got: tr.changes.len(),
            });
        }
        let changes = tr.changes.map(&r, false).map_err(Refusal::Rebase)?;
        let r_after = r.map(&tr.changes, true).map_err(Refusal::Rebase)?;
        if let Some(sel) = &tr.selection {
            tr.selection = Some(sel.map(&r_after, Bias::After));
        }
        tr.changes = changes;
        tr.base = self.version;
        Ok(())
    }

    fn validate(&self, tr: &Transaction) -> Result<(), Refusal> {
        let len = self.text.len_bytes();
        if tr.changes.len() != len {
            return Err(Refusal::LengthMismatch {
                expected: len,
                got: tr.changes.len(),
            });
        }
        if !tr.changes.is_empty() && self.config.read_only && !tr.origin.is_foreign() {
            return Err(Refusal::ReadOnly);
        }
        for e in tr.changes.edits() {
            for p in [e.range.start, e.range.end] {
                if !self.text.is_char_boundary(p) {
                    return Err(Refusal::NotCharBoundary(p));
                }
            }
            if e.insert.contains('\r') {
                return Err(Refusal::CarriageReturn);
            }
        }
        Ok(())
    }

    fn apply_inner(
        &mut self,
        mut tr: Transaction,
        mut view: Option<&mut EditorView>,
    ) -> Result<Applied, Refusal> {
        if tr.base > self.version {
            return Err(Refusal::Stale(Stale {
                at: tr.base,
                oldest: self.recent.oldest(),
                current: self.version,
            }));
        }
        self.rebase(&mut tr)?;
        self.validate(&tr)?;
        let new_text = if tr.changes.is_empty() {
            None
        } else {
            let mut t = self.text.clone();
            tr.changes.apply_to(&mut t);
            Some(t)
        };
        let check_text = new_text.as_ref().unwrap_or(&self.text);
        if let Some(sel) = &tr.selection {
            for r in sel.ranges() {
                for p in [r.anchor, r.head] {
                    if !check_text.is_char_boundary(p) {
                        return Err(Refusal::NotCharBoundary(p));
                    }
                }
            }
        }

        // ---- commit ----
        let prev_version = self.version;
        let mut closed: SmallVec<[UndoGroupClosed; 1]> = SmallVec::new();
        let selection_before = view.as_ref().map(|v| v.selection().clone());
        if let Some(new_text) = new_text {
            let inverse = tr.changes.invert(&self.text);
            let line_edits = LineEdit::compute(&tr.changes, &self.text);
            let old_text = std::mem::replace(&mut self.text, new_text);
            self.version = self.version.next();
            self.tokens.shift(&line_edits);
            self.recent.push(tr.changes.clone(), line_edits);
            self.diagnostics.map(&tr.changes);

            let sel_before = selection_before
                .clone()
                .unwrap_or_else(|| Selection::point(0));
            let sel_after = match (&tr.selection, &view) {
                (Some(s), _) => s.clone(),
                (None, Some(v)) => v.selection().map(&tr.changes, Bias::After),
                (None, None) => Selection::point(0),
            };
            let local = self.history.mode() == UndoMode::Local;
            let recorded_specially = match &tr.origin {
                Origin::Undo => {
                    closed.extend(self.history.close());
                    !local || self.history.applied_undo(&tr.changes, &old_text)
                }
                Origin::Redo => {
                    closed.extend(self.history.close());
                    !local || self.history.applied_redo(&tr.changes, &old_text)
                }
                Origin::Remote | Origin::Reload => {
                    closed.extend(self.history.applied_foreign(&tr.changes));
                    true
                }
                _ => false,
            };
            if !recorded_specially {
                let origin = match &tr.origin {
                    Origin::Undo => Origin::Command("undo".into()),
                    Origin::Redo => Origin::Command("redo".into()),
                    o => o.clone(),
                };
                closed.extend(self.history.record(Record {
                    origin: &origin,
                    group: tr.group,
                    time_ms: tr.time_ms,
                    pause_ms: self.config.undo_pause_ms,
                    from: prev_version,
                    to: self.version,
                    changes: &tr.changes,
                    inverse: &inverse,
                    selection_before: &sel_before,
                    selection_after: &sel_after,
                }));
            }
            // Dirty tracking: compare with the saved text only when an undo/redo may have
            // returned to it (cheap length check first).
            self.dirty = match &self.saved_text {
                Some(saved)
                    if matches!(tr.origin, Origin::Undo | Origin::Redo)
                        && saved.len_bytes() == self.text.len_bytes() =>
                {
                    *saved != self.text
                }
                _ => true,
            };
            if let Some(v) = view.as_deref_mut() {
                v.after_local_change(&tr.changes, self.version, sel_after);
            }
        } else if let (Some(v), Some(sel)) = (view.as_deref_mut(), tr.selection.clone()) {
            v.set_selection_raw(sel);
        }
        let mut selection_changed = false;
        if let Some(v) = view {
            for e in &tr.effects {
                v.apply_effect(e, self);
            }
            selection_changed = selection_before.as_ref() != Some(v.selection());
        }
        self.update_probe();
        Ok(Applied {
            prev_version,
            version: self.version,
            changes: tr.changes,
            origin: tr.origin,
            selection_changed,
            closed,
        })
    }
}
