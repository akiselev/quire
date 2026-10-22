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
mod highlight;
mod lang;
mod queries;
mod source;
mod style;
mod tree;

pub use config::{QueryError, check_queries};
pub use lang::Lang;
pub use tree::ParseStats;

#[cfg(test)]
mod dev_tests;
