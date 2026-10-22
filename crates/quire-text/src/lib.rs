//! quire-text: the shared text-editing core of quire's editor and svelters' text fields (no egui,
//! no threads, no clock, wasm32-clean).
//!
//! - [`Text`]: the text behind a small backend-neutral API (LF in memory).
//! - [`ChangeSet`]: composable, invertible edits with position mapping ([`Bias`]).
//! - [`Selection`] / [`SelRange`]: multi-range selections with a primary range.
//! - [`Transaction`]: changes + selection + origin + author + grouping, against a [`Version`].
//! - [`TextDocument`] (shared per buffer) and [`TextView`] (one per view): see [`document`].
//! - [`History`]: undo with pluggable grouping rules ([`UndoRules`]).
//! - [`DocRules`]: what text a document accepts.
//!
//! Positions are UTF-8 byte offsets, valid only with their [`Version`].

pub mod change;
pub mod columns;
pub mod document;
pub mod history;
pub mod ops;
pub mod recent;
pub mod rules;
pub mod selection;
pub mod text;
pub mod transaction;
pub mod version;
pub mod view;
pub mod words;

pub use change::{Bias, ChangeError, ChangeSet, Edit, Operation};
pub use document::{Anchor, Applied, Refusal, Snapshot, TextDocument};
pub use history::{History, UndoGroup, UndoGroupClosed, UndoMode, UndoRules};
pub use recent::{LineEdit, RecentChange, RecentChanges, Stale};
pub use rules::{DocRules, LineBreaks, LineMode};
pub use selection::{SelRange, Selection};
pub use text::{LineEnding, Text, TextSlice};
pub use transaction::{AuthorId, DocEffect, GroupHint, InputKind, NoEffect, Origin, Transaction};
pub use version::Version;
pub use view::{Preedit, SyncOutcome, SyncStep, TextView};

#[cfg(test)]
mod assert_traits {
    fn send_sync<T: Send + Sync>() {}
    #[test]
    fn document_and_view_are_send_sync() {
        send_sync::<super::Snapshot>();
        send_sync::<super::Text>();
        send_sync::<super::TextDocument>();
        send_sync::<super::TextView>();
    }
}
