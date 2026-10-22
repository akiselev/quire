//! [`Transaction`]: what [`Document::apply`](crate::Document::apply) takes: quire-text's
//! transaction carrying quire's [`Effect`]s.

use crate::find::FindQuery;

pub use quire_text::transaction::{AuthorId, DocEffect, GroupHint, InputKind, NoEffect, Origin};

/// An edit (or a selection change) against one version of a document, with quire's effects.
pub type Transaction = quire_text::Transaction<Effect>;

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
    /// Act on the view's suggest session.
    Suggest(SuggestEffect),
    /// Start, move or end the view's snippet session.
    Snippet(Option<crate::completion::SnippetSession>),
}

/// What a suggest command does to the acting view's session.
#[derive(Clone, PartialEq, Debug)]
pub enum SuggestEffect {
    /// Start a manual request (Ctrl+Space).
    Trigger,
    /// Close the list.
    Hide,
    /// Move the focus by one row (wrapping) or by a page.
    Select {
        /// Direction and amount.
        delta: isize,
        /// By a page (clamped).
        page: bool,
    },
    /// Toggle the details panel.
    ToggleDetails,
    /// An item was accepted: end the session, then retrigger (`retrigger`, the item's command
    /// was `editor.action.triggerSuggest`) or hand its command to the host.
    Accepted {
        /// Trigger again (automatic).
        retrigger: bool,
        /// A host command to run.
        command: Option<crate::completion::CompletionCommand>,
    },
}

impl From<NoEffect> for Effect {
    fn from(n: NoEffect) -> Self {
        match n {}
    }
}

impl DocEffect for Effect {
    fn writes_document(&self) -> bool {
        matches!(self, Effect::SetInsertSpaces(_))
    }
}
