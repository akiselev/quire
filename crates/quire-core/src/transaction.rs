//! [`Transaction`]: what [`Document::apply`](crate::Document::apply) takes.

use crate::change::ChangeSet;
use crate::find::FindQuery;
use crate::selection::Selection;
use crate::version::Version;
use smallvec::SmallVec;
use std::borrow::Cow;

/// What kind of input produced a transaction; drives undo grouping (Monaco's
/// `EditOperationType`, `cursorCommon.ts`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InputKind {
    /// Typing a character other than a space.
    Typing,
    /// Typing a space after a non-space.
    TypingFirstSpace,
    /// Typing a space after a space.
    TypingConsecutiveSpace,
    /// Delete left (backspace).
    DeletingLeft,
    /// Delete right.
    DeletingRight,
    /// A paste; always its own undo group.
    Paste,
    /// An IME commit; always its own undo group.
    Ime,
    /// Any other input edit (drop, cut, a11y `SetValue`); its own undo group.
    Other,
}

impl InputKind {
    /// Whether consecutive edits of this kind may share an undo group.
    pub fn coalesces(self) -> bool {
        !matches!(self, InputKind::Paste | InputKind::Ime | InputKind::Other)
    }

    /// Monaco's `shouldPushStackElementBetween`: whether an undo stop goes between an edit of
    /// kind `self` and a following edit of kind `next`.
    pub fn stop_between(self, next: InputKind) -> bool {
        fn norm(k: InputKind) -> u8 {
            match k {
                InputKind::TypingFirstSpace | InputKind::TypingConsecutiveSpace => 0,
                InputKind::Typing => 1,
                InputKind::DeletingLeft => 2,
                InputKind::DeletingRight => 3,
                _ => 4,
            }
        }
        if !self.coalesces() || !next.coalesces() {
            return true;
        }
        let typing = |k| {
            matches!(
                k,
                InputKind::Typing | InputKind::TypingFirstSpace | InputKind::TypingConsecutiveSpace
            )
        };
        if typing(self) && !typing(next) {
            return true;
        }
        if self == InputKind::TypingFirstSpace {
            // `abc |d`: no stop; `abc  |d`: stop.
            return false;
        }
        norm(self) != norm(next)
    }
}

/// Where a transaction came from.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Origin {
    /// User input in the widget.
    Input(InputKind),
    /// A non-typing command, by its VS Code id (e.g. `"editor.action.commentLine"`). Always its
    /// own undo group.
    Command(Cow<'static, str>),
    /// An undo (local history, or the host's answer in [`UndoMode::Host`](crate::UndoMode::Host)).
    Undo,
    /// A redo.
    Redo,
    /// A collaborator's change merged by the host. Not recorded in local history; history is
    /// rebased over it.
    Remote,
    /// The file changed on disk or was written by another tool. Treated like `Remote`.
    Reload,
}

impl Origin {
    /// Whether the change is someone else's (remote or reload).
    pub fn is_foreign(&self) -> bool {
        matches!(self, Origin::Remote | Origin::Reload)
    }
}

/// How a transaction relates to the open undo group.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GroupHint {
    /// Follow the grouping rules (word boundary, cursor jump, pause, kind change).
    #[default]
    Continue,
    /// Close the open group before this transaction; it starts a new group that later typing
    /// may join.
    Close,
    /// This transaction is its own group: close before and after.
    Isolated,
}

/// A side effect a transaction carries for the acting view (or, for `SetInsertSpaces` and
/// `SetWordWrap`, the document).
#[derive(Clone, PartialEq, Debug)]
pub enum Effect {
    /// Scroll so the byte position (after the changes) is visible.
    ScrollIntoView {
        /// The position.
        pos: usize,
        /// Centre it rather than scrolling minimally.
        center: bool,
    },
    /// Scroll so a line is visible (VS Code `revealLine`).
    RevealLine(usize),
    /// Set or clear the view's find query.
    SetFind(Option<FindQuery>),
    /// Closing characters the editor inserted (auto-close), positions after the changes; the
    /// view tracks them for overtype and pair delete while a cursor stays inside the pair.
    AutoClosed(Vec<crate::view::AutoClosed>),
    /// Column-select or add-next-match state for the acting view, kept while its selection is
    /// unchanged.
    ViewSession(crate::view::ViewSession),
    /// Replace the view's folds (byte ranges hidden after the changes, each from a header
    /// line's end to its last line's end).
    SetFolds(Vec<std::ops::Range<usize>>),
    /// Set the document's `EditorConfig::insert_spaces` (VS Code's `model.updateOptions`, e.g.
    /// after converting indentation). Unlike the other effects it acts on the
    /// [`Document`](crate::Document), for every view; undo does not revert it, and
    /// [`Applied::config_changed`](crate::Applied::config_changed) reports it.
    SetInsertSpaces(bool),
    /// Set the document's `EditorConfig::word_wrap` (VS Code `editor.action.toggleWordWrap`).
    /// It acts on the [`Document`](crate::Document) as `SetInsertSpaces` does, except that a
    /// read-only document allows it (it changes how the text is shown, not the text).
    SetWordWrap(crate::config::WordWrap),
}

/// An edit (or a selection change) against one version of a document.
#[derive(Clone, PartialEq, Debug)]
pub struct Transaction {
    /// The version the changes and selection were computed against. A transaction whose base is
    /// older than the document is rebased over the newer changes if they are still in the
    /// recent-changes window, else refused.
    pub base: Version,
    /// The text changes (an identity change set for a selection-only transaction).
    pub changes: ChangeSet,
    /// The acting view's selection after the changes; `None` maps the current selection.
    pub selection: Option<Selection>,
    /// Effects for the acting view.
    pub effects: SmallVec<[Effect; 2]>,
    /// Where it came from.
    pub origin: Origin,
    /// Undo grouping.
    pub group: GroupHint,
    /// The host's monotonic clock in milliseconds when the input happened, for the pause rule.
    /// `None` skips the pause rule (core reads no clocks).
    pub time_ms: Option<u64>,
}

impl Transaction {
    /// A transaction with these changes, origin `Input(Other)`.
    pub fn new(base: Version, changes: ChangeSet) -> Self {
        Transaction {
            base,
            changes,
            selection: None,
            effects: SmallVec::new(),
            origin: Origin::Input(InputKind::Other),
            group: GroupHint::Continue,
            time_ms: None,
        }
    }

    /// A selection-only transaction for a text of `len` bytes.
    pub fn select(base: Version, len: usize, selection: Selection) -> Self {
        Transaction::new(base, ChangeSet::identity(len)).with_selection(selection)
    }

    /// Sets the selection after the changes.
    #[must_use]
    pub fn with_selection(mut self, s: Selection) -> Self {
        self.selection = Some(s);
        self
    }

    /// Sets the origin.
    #[must_use]
    pub fn with_origin(mut self, o: Origin) -> Self {
        self.origin = o;
        self
    }

    /// Sets the group hint.
    #[must_use]
    pub fn with_group(mut self, g: GroupHint) -> Self {
        self.group = g;
        self
    }

    /// Sets the input time.
    #[must_use]
    pub fn with_time(mut self, ms: u64) -> Self {
        self.time_ms = Some(ms);
        self
    }

    /// Adds an effect.
    #[must_use]
    pub fn with_effect(mut self, e: Effect) -> Self {
        self.effects.push(e);
        self
    }

    /// Whether the transaction changes text.
    pub fn changes_text(&self) -> bool {
        !self.changes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::InputKind::*;

    #[test]
    fn monaco_stop_rules() {
        assert!(!Typing.stop_between(Typing));
        assert!(Typing.stop_between(TypingFirstSpace));
        assert!(!TypingFirstSpace.stop_between(Typing));
        assert!(!TypingFirstSpace.stop_between(TypingConsecutiveSpace));
        assert!(TypingConsecutiveSpace.stop_between(Typing));
        assert!(Typing.stop_between(DeletingLeft));
        assert!(!DeletingLeft.stop_between(DeletingLeft));
        assert!(DeletingLeft.stop_between(DeletingRight));
        assert!(DeletingLeft.stop_between(Typing));
        assert!(Paste.stop_between(Typing));
    }
}
