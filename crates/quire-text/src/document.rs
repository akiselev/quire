//! [`TextDocument`]: the shared model of one buffer (text, version, history, recent changes), and
//! its one mutation path.
//!
//! **The split.** A `TextDocument` holds what belongs to the buffer; a [`TextView`] holds what
//! belongs to one view of it (selection, focus, IME preedit). A single-line field is one document
//! and one view; quire's editor is one document and any number of views, each with its own carets.
//!
//! **Applying.** [`TextDocument::apply`]`(tr, acting_view)` is the only mutation path for local
//! edits and [`TextDocument::apply_remote`] the path for foreign ones. The acting view is synced
//! first (mapped forward through the recent changes, or clamped if it fell out of the window)
//! rather than refused; the transaction carries its own `base` and is rebased or refused on that.
//! Other views catch up with [`TextView::sync`] before use.
//!
//! **Atomicity.** A transaction is validated completely (base, lengths, char boundaries, no `\r`,
//! the content rules, selection bounds) against the new text before anything changes; a refusal
//! leaves document and view untouched.
//!
//! **Versions** bump only when text changes. Selection-only transactions bump the view's
//! `selection_version` only.
//!
//! **Cheap to create.** `TextDocument::new` allocates only the text: the history is allocated on
//! the first edit and the recent-changes ring grows on the first change.

use crate::change::{Bias, ChangeSet};
use crate::history::{History, Record, UndoGroupClosed, UndoMode};
use crate::recent::{LineEdit, RecentChanges, Stale};
use crate::rules::DocRules;
use crate::selection::Selection;
use crate::text::Text;
use crate::transaction::{AuthorId, DocEffect, NoEffect, Origin, Transaction};
use crate::version::Version;
use crate::view::TextView;
use smallvec::SmallVec;
use std::sync::Arc;

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
    /// Inserted text contains a line break and the document is single-line.
    LineBreak,
    /// The text would be longer than [`DocRules::max_chars`].
    MaxChars,
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
    /// Undo groups this transaction closed (the host's `text.edit` boundaries in host mode).
    pub closed: SmallVec<[UndoGroupClosed; 1]>,
}

impl Applied {
    /// Whether text changed.
    pub fn changed_text(&self) -> bool {
        self.version != self.prev_version
    }
}

/// A persistent position: a byte offset at a version with a bias. Resolve it with
/// [`TextDocument::resolve_anchor`], which maps it through the recent changes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Anchor {
    /// The offset at `version`.
    pub pos: usize,
    /// How it maps through insertions at its position.
    pub bias: Bias,
    /// The version `pos` belongs to.
    pub version: Version,
}

/// How [`TextDocument::replace_all`] treats history.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ExternalSet {
    /// A text set from outside starts a new history (GTK `set_text`, Qt `setText`).
    ResetHistory,
    /// A foreign change (quire's reload): not undoable; the stacks are rebased over it.
    Foreign,
    /// An undoable change of its own (an app-level revert).
    Undoable,
}

static EMPTY_LOCAL: History = History::new(UndoMode::Local);
static EMPTY_HOST: History = History::new(UndoMode::Host);

/// One buffer's shared model. See the module docs.
#[derive(Debug, Clone)]
pub struct TextDocument {
    text: Text,
    version: Version,
    /// Allocated on the first edit.
    history: Option<Box<History>>,
    recent: RecentChanges,
    rules: Arc<DocRules>,
}

impl TextDocument {
    /// A document holding `text` (line endings normalised to `\n`).
    pub fn new(text: &str, rules: Arc<DocRules>) -> Self {
        TextDocument::from_text(Text::from(text), rules)
    }

    /// A document holding a [`Text`].
    pub fn from_text(text: Text, rules: Arc<DocRules>) -> Self {
        TextDocument {
            recent: RecentChanges::new(rules.recent_window, Version::INITIAL),
            history: None,
            version: Version::INITIAL,
            text,
            rules,
        }
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

    /// The rules.
    pub fn rules(&self) -> &Arc<DocRules> {
        &self.rules
    }

    /// Replaces the rules. A changed recent-changes window keeps the entries that fit; a changed
    /// undo mode switches modes as [`TextDocument::set_undo_mode`] does.
    pub fn set_rules(&mut self, rules: Arc<DocRules>) {
        if rules.recent_window != self.recent.capacity() {
            let mut r = RecentChanges::new(rules.recent_window, self.version);
            // Keep what fits.
            if let Ok(it) = self.recent.since(self.recent.oldest()) {
                let entries: Vec<_> = it.cloned().collect();
                r = RecentChanges::new(
                    rules.recent_window,
                    entries.first().map_or(self.version, |e| e.from),
                );
                for e in entries {
                    r.push(e.changes, e.line_edits, e.author);
                }
            }
            self.recent = r;
        }
        let mode = rules.undo_mode;
        self.rules = rules;
        if mode != self.undo_mode() {
            self.set_undo_mode(mode);
        }
    }

    /// The history (an empty one until the first edit).
    pub fn history(&self) -> &History {
        match &self.history {
            Some(h) => h,
            None => match self.rules.undo_mode {
                UndoMode::Local => &EMPTY_LOCAL,
                UndoMode::Host => &EMPTY_HOST,
            },
        }
    }

    fn history_mut(&mut self) -> &mut History {
        let r = &self.rules;
        let (mode, rules, limit) = (r.undo_mode, r.undo, r.undo_limit);
        self.history
            .get_or_insert_with(|| Box::new(History::with_rules(mode, rules, limit)))
    }

    /// The recent changes.
    pub fn recent(&self) -> &RecentChanges {
        &self.recent
    }

    /// Who owns undo.
    pub fn undo_mode(&self) -> UndoMode {
        self.history().mode()
    }

    /// Switches undo mode: closes the open group (returned) and clears the local stacks.
    pub fn set_undo_mode(&mut self, mode: UndoMode) -> Option<UndoGroupClosed> {
        if self.rules.undo_mode != mode {
            Arc::make_mut(&mut self.rules).undo_mode = mode;
        }
        match &mut self.history {
            Some(h) => h.set_mode(mode),
            None => None,
        }
    }

    /// Whether `undo` would do something (local mode).
    pub fn can_undo(&self) -> bool {
        self.history().can_undo()
    }

    /// Whether `redo` would do something (local mode).
    pub fn can_redo(&self) -> bool {
        self.history().can_redo()
    }

    /// Whether an undo group is open.
    pub fn has_open_group(&self) -> bool {
        self.history().has_open_group()
    }

    /// Closes the open undo group (focus change, explicit stop).
    pub fn close_undo_group(&mut self) -> Option<UndoGroupClosed> {
        self.history.as_mut()?.close()
    }

    /// Closes the open undo group if the pause since its last edit exceeds
    /// [`DocRules::undo_pause_ms`] at the host's clock `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Option<UndoGroupClosed> {
        let pause = self.rules.undo_pause_ms.unwrap_or(u64::MAX);
        self.history.as_mut()?.tick(now_ms, pause)
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

    /// Applies a transaction made through `view` (see the module docs: the view is synced first,
    /// the transaction rebased if its base is old).
    pub fn apply(&mut self, tr: Transaction, view: &mut TextView) -> Result<Applied, Refusal> {
        self.apply_with(tr, view).map(|(a, _)| a)
    }

    /// [`TextDocument::apply`] for a wrapper with its own effects: applies text, selection and
    /// history, and hands the effects back for the wrapper to apply.
    pub fn apply_with<E: DocEffect>(
        &mut self,
        tr: Transaction<E>,
        view: &mut TextView,
    ) -> Result<(Applied, SmallVec<[E; 2]>), Refusal> {
        view.sync(self);
        self.apply_inner(tr, Some(view))
    }

    /// Applies a foreign change (a collaborator's, merged by the host; or a reload). Not recorded
    /// in local history; the stacks are rebased over it. Views catch up on their next `sync`.
    pub fn apply_remote(
        &mut self,
        base: Version,
        changes: ChangeSet,
        origin: Origin,
        author: AuthorId,
    ) -> Result<Applied, Refusal> {
        let tr: Transaction<NoEffect> = Transaction::new(base, changes)
            .with_origin(origin)
            .with_author(author);
        self.apply_inner(tr, None).map(|(a, _)| a)
    }

    /// Replaces the whole text from outside (a prop write, a reload, a revert). The change is
    /// the differing middle (common prefix and suffix kept; with the `diff` feature, a reload uses
    /// a line diff), so carets outside it keep their place.
    pub fn replace_all(
        &mut self,
        text: &str,
        mode: ExternalSet,
        view: Option<&mut TextView>,
    ) -> Result<Applied, Refusal> {
        let new = crate::text::normalize_line_endings(text);
        let changes = match mode {
            #[cfg(feature = "diff")]
            ExternalSet::Foreign => ChangeSet::diff(&self.text, &Text::from(&*new)),
            _ => ChangeSet::between_trimmed(&self.text, &new),
        };
        let base = self.version;
        match mode {
            ExternalSet::ResetHistory | ExternalSet::Foreign => {
                let tr: Transaction<NoEffect> = Transaction::new(base, changes).with_origin(Origin::Reload);
                let r = self.apply_inner(tr, view).map(|(a, _)| a);
                if mode == ExternalSet::ResetHistory
                    && let Some(h) = &mut self.history
                {
                    h.reset();
                }
                r
            }
            ExternalSet::Undoable => {
                let tr: Transaction<NoEffect> = Transaction::new(base, changes)
                    .with_origin(Origin::Command("replaceAll".into()))
                    .with_group(crate::transaction::GroupHint::Isolated);
                self.apply_inner(tr, view).map(|(a, _)| a)
            }
        }
    }

    fn rebase<E>(&self, tr: &mut Transaction<E>) -> Result<(), Refusal> {
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

    fn validate<E: DocEffect>(&self, tr: &Transaction<E>) -> Result<(), Refusal> {
        let len = self.text.len_bytes();
        if tr.changes.len() != len {
            return Err(Refusal::LengthMismatch {
                expected: len,
                got: tr.changes.len(),
            });
        }
        let foreign = tr.origin.is_foreign();
        let writes = tr.effects.iter().any(DocEffect::writes_document);
        if (!tr.changes.is_empty() || writes) && self.rules.read_only && !foreign {
            return Err(Refusal::ReadOnly);
        }
        let single = matches!(
            self.rules.lines,
            crate::rules::LineMode::Single { breaks } if breaks != crate::rules::LineBreaks::Keep
        );
        let mut inserted_chars = 0usize;
        let mut deleted_bytes = Vec::new();
        for e in tr.changes.edits() {
            for p in [e.range.start, e.range.end] {
                if !self.text.is_char_boundary(p) {
                    return Err(Refusal::NotCharBoundary(p));
                }
            }
            if e.insert.contains('\r') {
                return Err(Refusal::CarriageReturn);
            }
            if !foreign {
                if single && e.insert.contains('\n') {
                    return Err(Refusal::LineBreak);
                }
                if self.rules.max_chars.is_some() {
                    inserted_chars += e.insert.chars().count();
                    deleted_bytes.push(e.range.clone());
                }
            }
        }
        if let Some(max) = self.rules.max_chars
            && !foreign
            && inserted_chars > 0
        {
            let deleted: usize = deleted_bytes
                .iter()
                .map(|r| self.text.byte_to_char(r.end) - self.text.byte_to_char(r.start))
                .sum();
            if self.text.len_chars() - deleted + inserted_chars > max as usize {
                return Err(Refusal::MaxChars);
            }
        }
        Ok(())
    }

    fn apply_inner<E: DocEffect>(
        &mut self,
        mut tr: Transaction<E>,
        mut view: Option<&mut TextView>,
    ) -> Result<(Applied, SmallVec<[E; 2]>), Refusal> {
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
            let line_edits = if self.rules.track_line_edits {
                LineEdit::compute(&tr.changes, &self.text)
            } else {
                Vec::new()
            };
            let old_text = std::mem::replace(&mut self.text, new_text);
            self.version = self.version.next();
            self.recent.push(tr.changes.clone(), line_edits, tr.author);

            let sel_before = selection_before
                .clone()
                .unwrap_or_else(|| Selection::point(0));
            let sel_after = match (&tr.selection, &view) {
                (Some(s), _) => s.clone(),
                (None, Some(v)) => v.selection().map(&tr.changes, Bias::After),
                (None, None) => Selection::point(0),
            };
            let pause_ms = self.rules.undo_pause_ms.unwrap_or(u64::MAX);
            let version = self.version;
            let new_text = self.text.clone();
            let history = self.history_mut();
            let local = history.mode() == UndoMode::Local;
            let native = history.is_native();
            let recorded_specially = match &tr.origin {
                Origin::Undo | Origin::Redo if native => {
                    let undo = matches!(tr.origin, Origin::Undo);
                    let own = history.applied_native_step(&old_text, &sel_before, &new_text, undo);
                    if !own {
                        history.record_native(&tr.origin, &old_text, &new_text, &tr.changes, &sel_before);
                    }
                    true
                }
                Origin::Input(_) | Origin::Command(_) if native => {
                    history.record_native(&tr.origin, &old_text, &new_text, &tr.changes, &sel_before);
                    true
                }
                Origin::Undo => {
                    closed.extend(history.close());
                    !local || history.applied_undo(&tr.changes, &old_text)
                }
                Origin::Redo => {
                    closed.extend(history.close());
                    !local || history.applied_redo(&tr.changes, &old_text)
                }
                Origin::Remote | Origin::Reload => {
                    closed.extend(history.applied_foreign(&tr.changes));
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
                closed.extend(history.record(Record {
                    origin: &origin,
                    author: tr.author,
                    group: tr.group,
                    time_ms: tr.time_ms,
                    pause_ms,
                    from: prev_version,
                    to: version,
                    changes: &tr.changes,
                    inverse: &inverse,
                    selection_before: &sel_before,
                    selection_after: &sel_after,
                }));
            }
            if let Some(v) = view.as_deref_mut() {
                v.after_local_change(self.version, sel_after);
            }
        } else if let (Some(v), Some(sel)) = (view.as_deref_mut(), tr.selection.clone()) {
            v.set_selection_unchecked(sel);
            if let (Some(h), Some(before)) = (self.history.as_deref_mut(), &selection_before)
                && h.is_native()
                && before != v.selection()
            {
                h.moved_native(&self.text, before, v.selection());
            }
        }
        let selection_changed = match (&view, &selection_before) {
            (Some(v), Some(b)) => b != v.selection(),
            _ => false,
        };
        Ok((
            Applied {
                prev_version,
                version: self.version,
                changes: tr.changes,
                origin: tr.origin,
                author: tr.author,
                selection_changed,
                closed,
            },
            tr.effects,
        ))
    }
}
