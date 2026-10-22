//! [`TextView`]: one view of a [`TextDocument`] (selection, focus, IME preedit). See the
//! [`document`](crate::document) module docs for how views stay in step.

use crate::change::{Bias, ChangeSet};
use crate::document::{Applied, TextDocument};
use crate::selection::Selection;
use crate::text::Text;
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

/// How [`TextView::sync`] caught up.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SyncOutcome {
    /// Already at the document's version.
    UpToDate,
    /// Mapped forward through the recent changes.
    Mapped,
    /// Older than the window: positions clamped.
    Clamped,
}

/// One step of [`TextView::sync_with`], for a wrapper that maps its own view state.
#[derive(Copy, Clone, Debug)]
pub enum SyncStep<'a> {
    /// One change was mapped (the selection is already mapped through it).
    Mapped(&'a ChangeSet),
    /// The view was older than the window; clamp to this text (the selection is already
    /// clamped).
    Clamped(&'a Text),
}

/// One view of a document.
#[derive(Clone, Debug)]
pub struct TextView {
    selection: Selection,
    selection_version: u64,
    synced_at: Version,
    focused: bool,
    preedit: Option<Preedit>,
}

impl TextView {
    /// A view of `doc` with a cursor at the start.
    pub fn new(doc: &TextDocument) -> Self {
        TextView {
            selection: Selection::point(0),
            selection_version: 0,
            synced_at: doc.version(),
            focused: false,
            preedit: None,
        }
    }

    /// The selection (valid at [`TextView::synced_at`]).
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

    /// Whether the document moved on since the view last synced.
    pub fn is_behind(&self, doc: &TextDocument) -> bool {
        self.synced_at != doc.version()
    }

    /// Whether the view has focus.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Sets focus; returns whether it changed. On a change the caller closes the document's undo
    /// group ([`TextDocument::close_undo_group`]), one of Monaco's closing rules. Blur ends the
    /// preedit.
    pub fn set_focused(&mut self, f: bool) -> bool {
        let changed = self.focused != f;
        self.focused = f;
        if !f {
            self.preedit = None;
        }
        changed
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
    pub fn set_selection(&mut self, doc: &TextDocument, sel: Selection) {
        self.sync(doc);
        self.set_selection_unchecked(sel.clamped(doc.text()));
    }

    /// Sets the selection without clamping: its positions must be valid at
    /// [`TextView::synced_at`]. For wrappers that already validated them.
    pub fn set_selection_unchecked(&mut self, sel: Selection) {
        if sel != self.selection {
            self.selection = sel;
            self.selection_version += 1;
        }
    }

    /// Catches up with the document: maps the selection through the recent changes since
    /// [`TextView::synced_at`]; if the view is older than the window, clamps it.
    pub fn sync(&mut self, doc: &TextDocument) -> SyncOutcome {
        self.sync_with(doc, |_, _| {})
    }

    /// [`TextView::sync`], calling `step` after each change the selection was mapped through (or
    /// once after clamping) with the step and the selection now, so a wrapper maps its own state
    /// in the same order.
    pub fn sync_with(
        &mut self,
        doc: &TextDocument,
        mut step: impl FnMut(SyncStep<'_>, &Selection),
    ) -> SyncOutcome {
        if self.synced_at == doc.version() {
            return SyncOutcome::UpToDate;
        }
        match doc.recent().since(self.synced_at) {
            Ok(changes) => {
                for e in changes {
                    let sel = self.selection.map(&e.changes, Bias::Before);
                    self.set_selection_unchecked(sel);
                    step(SyncStep::Mapped(&e.changes), &self.selection);
                }
                self.synced_at = doc.version();
                SyncOutcome::Mapped
            }
            Err(_) => {
                let sel = self.selection.clamped(doc.text());
                self.set_selection_unchecked(sel);
                step(SyncStep::Clamped(doc.text()), &self.selection);
                self.synced_at = doc.version();
                SyncOutcome::Clamped
            }
        }
    }

    /// Maps the view through one applied record (for hosts that fan out). Returns `false` and
    /// does nothing if the view is not at `applied.prev_version`; call [`TextView::sync`] then.
    pub fn map_through(&mut self, applied: &Applied) -> bool {
        if !applied.changed_text() {
            return self.synced_at == applied.version;
        }
        if self.synced_at != applied.prev_version {
            return false;
        }
        let sel = self.selection.map(&applied.changes, Bias::Before);
        self.set_selection_unchecked(sel);
        self.synced_at = applied.version;
        true
    }

    /// After a local change made through this view.
    pub(crate) fn after_local_change(&mut self, version: Version, sel: Selection) {
        self.set_selection_unchecked(sel);
        self.synced_at = version;
    }

    /// Marks the view synced at `version` (a wrapper that mapped it through one applied record
    /// itself, as [`TextView::map_through`] does).
    pub fn set_synced_at(&mut self, version: Version) {
        self.synced_at = version;
    }
}
