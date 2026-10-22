# Handoff: quire-core (branch `core2`)

Written 2026-10-22 by the core2 builder near its context budget. The branch is `core2`, rebased on
master `b512361` and pushed to origin/core2. **It has not landed on master:** the public API
(batch 1 = A–E, batch 2) is waiting for the coordinator's approval (rule 2). Full
`scripts/check.sh` passes on the branch.

## Done on core2

- **Commands** (`src/commands/lines.rs`, `multi.rs`, `fold.rs`; ids in `command.rs` and
  `from_vscode_id`):
  - insertCursorAbove/Below;
  - addSelectionToNextFindMatch (VS Code session semantics; the session lives in the view as
    `ViewSession` while the selection is unchanged);
  - moveLinesUp/Down (touching selections move as one block; no indentation rules);
  - copyLinesUp/Down and deleteAllLeft (literal ports);
  - column select: `cursorColumnSelect{Left,Right,Up,Down,PageUp,PageDown}` and `columnSelect`
    with anchor/head line and visual column for Alt+Shift+drag;
  - fold, unfold, toggleFold, foldAll, unfoldAll;
  - compositionType (IME commit and DeleteSurrounding: one transaction, Isolated group, through
    the typing rules).
- **Effects:** `Effect::ViewSession` (opaque) and `Effect::SetFolds`.
- **Find:** `FindState::current_match` and `EditorView::find_current_match`. Zero-length matches
  are kept (VS Code); find next/previous steps past one at the cursor.
- **Auto-close:** contained pairs (`(` inside `(*`,`*)`; fixes issue #72177).
- **Folding** (`src/folding.rs`):
  - `indent_fold_ranges` is VS Code's indentRangeProvider, ported.
  - `Document::{fold_ranges, set_fold_ranges, clear_fold_ranges}`: syntax ranges are mapped by
    `LineEdit`s; indentation ranges are cached per version in a `OnceLock`.
  - View helpers `is_line_hidden` and `fold_at_line`; vertical motion skips folded lines.
  - **For the syntax agent:** the SyntaxDriver should call `doc.set_fold_ranges(at,
    highlighter.folds(tree))`.
- **Large files** (`src/large.rs`): `LargeFilePolicy` lives in `EditorConfig.large_file`.
  `Degraded { reasons }` and `Document::degraded()` are decided at load, on `set_config` and on a
  Reload (as Monaco does). `syntax_off()` disables `fold_ranges`.
- **E2:** `benches/e2.rs` (criterion) with numbers in `docs/design/E2.md`. At 100 MB: RSS is
  1.38× the text, edit p99 is 20 µs, a 50-line viewport takes 46 µs.
- **VS Code port** (`scripts/port-vscode-tests.sh`; `PORT_ONLY=<substr>` regenerates one suite):
  - Prelude stubs are resolved by name (`__resolve`), proxies are permissive outside tests, and
    tests with several models or editors split into `#n` cases.
  - New harness steps (documented at the top of `tests/monaco.rs`): `repeat`/`expectStops`,
    `columnSelect`, `setValue`, `expectMaxColumn`, `expectSelCount`, `expectRanges`,
    `expectFind{All,Next,Prev}`, `applyEdits`/`applyInverse`/`expectRefused`,
    `expectValueInRange`, `ime*`, `setEOL`.
  - Suites that are N/A as a whole are listed in `SUITE_NA` in `port.mjs`. Triage reasons are in
    `tests/port/triage.json`.

## Test counts (last run)

- quire-core: lib 28, document 13, laws 10, folding 5, monaco 1 (354 fixture cases pass, 41
  expected "ours", 0 unexpected).
- Workspace `scripts/check.sh`: all passed. egui_quire's keymap test was updated: every keymap id
  now resolves.

## VSCODE-MAP (`crates/quire-core/tests/VSCODE-MAP.md`, per-suite table generated)

| suite | tests | pass | ours | bug | N/A |
|---|---|---|---|---|---|
| cursor.test.ts | 267 | 163 | 26 | 0 | 78 |
| wordOperations | 58 | 44 | 3 | 0 | 11 |
| textModelSearch | 77 | 46 | 12 | 0 | 19 |
| editableTextModel | 62 | 43 | 0 | 0 | 19 |
| linesOperations | 59 | 23 | 0 | 0 | 36 |
| moveLinesCommand | 15 | 11 | 0 | 0 | 4 |
| copyLinesCommand | 10 | 8 | 0 | 0 | 2 |
| multicursor | 76 | 10 | 0 | 0 | 66 |
| model | 33 | 6 | 0 | 0 | 27 |
| textModel, modelEditOperation, editStack, range, cursorColumns, model.line | 118 | 0 | 0 | 0 | 118 |
| **total** | 775 | 354 | 41 | 0 | 380 |

That is 89.6 % pass over the non-N/A rows. The E11 gate is ≥ 90 %. Proposal F (onEnter and
indentation rules) would turn 17 of the cursor "ours" rows into passes.

## Next

1. On approval: run `git fetch && git rebase origin/master`, then `scripts/check.sh`, then
   `git push origin HEAD:master`. Tell the widget agent the commands exist; its keymap test is
   already updated here.
2. Batch 2, once approved: AddCommentLine, RemoveCommentLine, DeleteAllRight, BlockComment. Then
   port the comment contrib (lineCommentCommand.test, blockCommentCommand.test) and indentation
   (indentation.test, mostly through `testCommand`-style stubs).
3. F (after approval): an onEnter rules and indentationRules config, and electric characters from
   the decrease pattern. Groundwork can stay private until then.
4. Cheap N/A reductions:
   - textModel `validatePosition` / `getOptions` (N/A by design today);
   - multicursor tests that depend on find-widget state (they need `FindState` toggles without
     selecting, as `state.change` does);
   - `model.applyEdits(…, true)` undo edits in modelEditOperation.

## Open problems / decisions taken

- multiCursorMergeOverlapping=false is not supported: Selection always merges. 1 row is "ours".
- autoClosingPairs.beforeText, autoSurround quotes/brackets and autoClosingComments are not
  supported (5 rows "ours"). Supporting them needs config changes, which are public.
- CompositionType counts code points, where VS Code counts UTF-16 units. A long line alone does
  not turn syntax off. Both choices are flagged to the coordinator.
- The fold header line is dropped when an edit splits or joins it, unless lines are inserted at
  its column 0 (then the fold moves down).
- The E2 numbers come from a shared machine, so treat them as an upper bound.
