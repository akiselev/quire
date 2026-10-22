# Handoff: quire-core (branch `core`)

Written 2026-10-22 by the quire-core builder at the end of its session (context budget).

## Done (all on master)

- **Phase 1, public API:** `Version`, `Text`/`TextSlice`/`LineEnding` (ropey 1.6 private, LF in memory,
  CRLF and BOM recorded, VS Code's majority rule), `ChangeSet` (retain/delete/insert, `replace`, `compose`,
  `invert`, `map` (OT transform), `map_pos(pos, Bias)`, `apply`, `diff` via imara-diff 0.2), `Selection`/
  `SelRange` (multi-range, primary, `goal_column`), `Transaction`, `Origin`, `InputKind`, `GroupHint`,
  `Effect`, `Snapshot`, `Applied`, `UndoGroup`/`UndoGroupClosed`, `UndoMode`, `Refusal`, `Stale`, `Anchor`,
  `Document` + `EditorView` (SEAM items 1 and 6), `Diagnostic`/`Severity`/`SourceId`/`DiagAction`/`DiagTags`,
  `EditorConfig` (VS Code names under serde), `EditorCommand` (serde tag = VS Code id) + `Motion`,
  `syntax.rs` (`Highlighter`, `Spawner`, `InlineSpawner`, `CancelFlag`, `TokenRuns`, `TokenStyle` packing,
  `TokenType` (LSP order + tree-sitter kinds), `TokenModifiers`, `TokenStore`, `BracketPair`, `FoldRange`,
  `IndentHint`, `PendingWork`), `SettledProbe` (SEAM item 4). `SyntaxDriver` left to the syntax agent.
- **Apply contract** (module docs of `document.rs`): `Document::apply(tr, &mut acting_view)` syncs the acting
  view first (does not refuse it); stale-base transactions are rebased through `RecentChanges` or refused
  as `Refusal::Stale`; validation is atomic. Other views call `EditorView::sync(&doc)` (maps through recent
  changes, clamps and marks find stale past the window). `map_through(&Applied)` exists for fan-out hosts.
- **Phase 2:** history with Monaco's grouping (word boundary via `InputKind::stop_between`, cursor jump,
  pause via `Transaction::time_ms` / `Document::tick(now)`, non-typing commands, `close_undo_group` for focus,
  `GroupHint`), host mode emitting `UndoGroupClosed` (with `inverse`), local stacks rebased over remote
  changes, dirty tracking (undo back to the saved text is clean), diagnostics per source mapped/refused,
  token shifting by line edits, stale token runs mapped by whole lines. Tests: `tests/laws.rs` (10, incl. the
  two-views-past-the-window proptest), `tests/document.rs` (13), unit tests (25). wasm32 check passes.
- **Phase 3:** commands in `src/commands/` (cursor moves with grapheme-aware visible columns, word motions
  and deletes translated from VS Code `cursorWordOperations.ts`, smart home, page moves, type with
  auto-close/auto-surround/overtype (tracked auto-closed characters in the view), Enter with bracket indent,
  delete left (tab stops, auto pair delete, code point vs emoji) / right / word, Tab/outdent/indent lines
  (`ShiftCommand`), comment line, insert line before/after, delete lines, select all, expand line
  selection, paste per cursor, cut, find/next/previous/replace one/replace all on regex-automata with a size
  limit, undo/redo, save request, reveal line). `EditorCommand::from_vscode_id` maps argument-free ids.
- **VS Code port:** `scripts/port-vscode-tests.sh` runs `cursor.test.ts` (vscode @ `bdce63db`) under
  recording stubs (`crates/quire-core/tests/port/prelude.mjs`, `port.mjs`) and writes
  `fixtures/monaco/cursor.test.fixture`; `tests/monaco.rs` runs the fixtures; `tests/port/map.mjs` writes
  `crates/quire-core/tests/VSCODE-MAP.md`. **263 tests: 161 translated, 141 pass, 20 fail (ours: language
  indentation rules, onEnter rules, electric characters, beforeText, multiCursorMergeOverlapping, mouse
  drag modes), 0 fail (bug); 102 N/A** (IME composition, tokenization, column selection, model APIs such
  as setValue/applyEdits, overtype input mode, paste-on-new-line).

## Test counts (last run)

`scripts/cargo.sh test -p quire-core`: lib 25 pass, document 13 pass, laws 10 pass, monaco 1 pass
(145 fixture cases pass, 20 expected failures, 0 unexpected).

## Next

1. Port `wordOperations.test.ts`: its tests use `testRepeatedActionAndExtractPositions` and pipe-position
   serialization; add stubs for those in `prelude.mjs` that record one step `expectStops <cmd> <start> <stop>
   <expected>` and a harness step that repeats the command collecting positions. Commands already exist
   (`cursorWord*`, `deleteWord*`; `cursorWordAccessibility*` and `deleteInsideWord` are not implemented).
2. Then `src/vs/editor/test/common/model/*` (text model, edits, EOL) and `common/core/*` (range, position,
   selection) — mostly API tests; translate what applies to `Text`/`ChangeSet`/`Selection` by hand or mark
   N/A (piece tree internals).
3. contrib: linesOperations (move/copy lines not yet implemented), find, comment (block comments not
   implemented), indentation.
4. N/A reductions worth doing: `setValue` (as a reload), `getLineMaxColumn`, IME composition steps
   (`compositionType` maps to a replace-previous-chars transaction), multicursor paste text.
5. Language features behind the 20 "ours" failures (indentation rules, onEnter rules, electric characters)
   would need a language-configuration seam; not in scope yet.

## Open problems / decisions taken (no user asked)

- `DiagAction.args_json: String` instead of `serde_json::Value` so quire-core needs no JSON dependency.
- `Transaction::time_ms` carries the host clock for the pause rule; core reads no clocks.
- `UndoGroupClosed` is reported in both modes (hosts in local mode may ignore it).
- In host mode an open undo group makes `is_settled()` false until `tick`/`close_undo_group` closes it.
- Inserted text containing `\r` is refused (`Refusal::CarriageReturn`); `Type` and `Paste` normalise first.
- Multi-char `type` runs through the typing interceptors char by char into one transaction.
- No trimAutoWhitespace, no `cursorWordAccessibility*`, no block comments, no move/copy lines yet.
- `fixtures/monaco/*.summary.json` are generated inputs for `map.mjs`; keep them with the fixtures.
