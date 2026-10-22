//! quire-syntax: tree-sitter highlighters for quire.
//!
//! Grammars: Svelte (`tree-sitter-htmlx-svelte`), TypeScript and TSX (`tree-sitter-typescript`),
//! JavaScript (`tree-sitter-javascript`) and CSS (`tree-sitter-css`), one Cargo feature each.
//! Queries live in `queries/<lang>/*.scm` (Helix's layout); see `THIRD_PARTY_NOTICES.md`.
//!
//! The engine (E5, `docs/design/E5.md`) is our own layered query runner over tree-sitter's
//! `QueryCursor`: one tree per language layer, injections found by `injections.scm`, layers
//! reused and reparsed incrementally across edits.

mod config;
// SyntaxDriver: public API pending the coordinator's approval; behind a non-default feature until then.
#[cfg(feature = "unstable-driver")]
mod driver;
mod highlight;
mod highlighter;
mod lang;
mod queries;
mod source;
mod style;
mod tree;

pub use config::{QueryError, check_queries};
#[cfg(feature = "unstable-driver")]
pub use driver::{DEFAULT_DEBOUNCE_MS, DEFAULT_MARGIN_LINES, SyntaxDriver};
pub use highlighter::{SyntaxTree, TreeSitterHighlighter};
pub use lang::Lang;
pub use style::name as style_name;
pub use tree::ParseStats;

#[cfg(test)]
mod dev_tests;
