//! quire-core: the headless model of the quire code editor (no egui, no threads, wasm32-clean).
//!
//! - [`Text`]: the rope behind a small API (LF in memory; line ending and BOM recorded).
//! - [`ChangeSet`]: composable, invertible edits with position mapping ([`Bias`]).
//! - [`Selection`] / [`SelRange`]: multi-range selections with a primary range.
//! - [`Transaction`]: changes + selection + effects + origin + grouping, against a [`Version`].
//! - [`Document`] (shared per file) and [`EditorView`] (one per view): see [`document`] for the
//!   apply contract and how views stay in step.
//! - [`EditorCommand`]: VS Code-id commands, resolved purely into transactions.
//! - [`syntax`]: the highlighter seam and Monaco's compact token store.
//! - [`Diagnostic`]s per source and version, mapped forward through edits.
//!
//! Positions are UTF-8 byte offsets, valid only with their [`Version`].

pub use quire_text::{change, columns, history, recent, selection, text, version, words};

pub mod command;
mod commands;
pub mod config;
pub mod diagnostics;
pub mod document;
pub mod find;
pub mod folding;
mod lang;
pub mod large;
pub mod syntax;
pub mod transaction;
pub mod view;

pub use change::{Bias, ChangeError, ChangeSet, Edit, Operation};
pub use command::{CommandOutput, EditorCommand, HostRequest, Motion};
pub use config::{
    AutoClosingStrategy, CommentOptions, EditorConfig, EnterAction, IndentAction, IndentationRules,
    OnEnterRule, OvertypeStrategy, WordWrap, WrappingIndent,
};
pub use diagnostics::{DiagAction, DiagTags, Diagnostic, Diagnostics, Severity, SourceId};
pub use document::{Applied, Document, Executed, Refusal, SettledProbe};
pub use find::{FindError, FindQuery, FindState};
pub use history::{History, UndoGroup, UndoGroupClosed, UndoMode};
pub use large::{Degraded, DegradedReason, LargeFilePolicy};
pub use quire_text::{Anchor, Snapshot};
pub use recent::{LineEdit, RecentChanges, Stale};
pub use selection::{SelRange, Selection};
pub use text::{LineEnding, Text, TextSlice};
pub use transaction::{AuthorId, Effect, GroupHint, InputKind, Origin, Transaction};
pub use version::Version;
pub use view::{AutoClosed, EditorView, Preedit, Reveal, Scroll, SyncOutcome};

#[cfg(test)]
mod assert_traits {
    fn send_sync<T: Send + Sync>() {}
    #[test]
    fn snapshot_and_document_are_send_sync() {
        send_sync::<super::Snapshot>();
        send_sync::<super::Text>();
        send_sync::<super::Document>();
        send_sync::<super::EditorView>();
        send_sync::<super::SettledProbe>();
    }
}
