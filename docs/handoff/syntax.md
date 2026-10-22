# Handoff: syntax (QE2, quire-syntax)

## Done (on master at a87b99e and its successor)
- `crates/quire-syntax`: tree-sitter 0.26.13 with these grammars, one Cargo feature each (all on
  by default): tree-sitter-htmlx-svelte 0.1.16, tree-sitter-typescript 0.23.2 (TS and TSX),
  tree-sitter-javascript 0.25.0 and tree-sitter-css 0.25.0. Why 0.26 and htmlx rather than 0.27 and
  svelte-ng: `docs/design/E5.md`.
- Queries in `crates/quire-syntax/queries/<lang>/{highlights,injections,folds,indents,brackets}.scm`
  (Helix layout, `; inherits:` supported). They live in the crate, not at the repo root, so
  `include_str!` works. Sources are recorded in `THIRD_PARTY_NOTICES.md` and in each file's header.
- Engine: our own layered `QueryCursor` runner (E5 decision).
  - `tree.rs`: layers, injections, incremental reuse, lazy shifts.
  - `highlight.rs`: highlights, folds, brackets, indents.
- `TreeSitterHighlighter` implements `quire_core::syntax::Highlighter`; its tree type is
  `SyntaxTree`. Capture names map to core `TokenType`s, and `*.builtin` captures get the
  defaultLibrary modifier.
- `SyntaxDriver<H>` (`src/driver.rs`) sits behind the non-default feature `unstable-driver`.
  Its public API is waiting for the coordinator's approval (proposal sent).
- Criterion bench `benches/e5.rs`; numbers in E5.md: about 7.8 ms per edit on a 20k-line Svelte
  file, against a 3.7 ms tree-sitter floor.

## Tests (`scripts/cargo.sh test -p quire-syntax --all-features`)
| Suite | Passed | Ignored |
|---|---|---|
| unit | 4 | 4 |
| abi | 1 | |
| driver | 4 | |
| highlight | 10 | |
| incremental | 6 | |
| injections | 9 | |
| structure | 8 | |

The four ignored unit tests in `src/dev_tests.rs` are manual measurement and debug tools, run
with `--ignored --nocapture`:
- `e5_timing`: quick release-mode timing;
- `ranged_query_cost`: cost of a range-limited query;
- `root_reparse_cost`: tree-sitter floor, Svelte against TS;
- `dump_sexp`: prints trees; set `QS_SRC` and `QS_LANG`.

Fixture snapshots: set `QUIRE_BLESS=1` to rewrite `tests/fixtures/*.tokens`. For a longer fuzz
run, set `QUIRE_FUZZ_SEEDS=n`.

## Next
- Once the coordinator approves the SyntaxDriver API, drop the `unstable-driver` gate. After that,
  wire it into egui_quire (widget agent): call `update(doc, visible_lines, now_ms)` each frame, set
  the waker to `ctx.request_repaint`, and use `next_deadline()` to schedule frames.
- If E5's 2 ms stays the target, the remaining non-tree-sitter cost is `changed_ranges` on the
  root (about 1.6 ms) plus layer bookkeeping (about 1 ms):
  - skip `changed_ranges` when the edit stays inside one leaf (no structural change possible);
  - store children as CSR instead of `Vec<Vec<u32>>`;
  - avoid rebuilding `by_start` per parse.

## Open problems
- E5's ≤ 2 ms target is not reachable for 20k-line files on tree-sitter (see E5.md); the target
  needs to be revised.
- tree-sitter-htmlx-svelte keeps HTML entities inside text, so `&amp;` gets no token.
- Query quality follows the upstream queries: a capitalised function name gets the type colour
  in TS, and destructured parameters are not marked as parameters in TS.
- Move to tree-sitter 0.27 when htmlx-svelte does.
