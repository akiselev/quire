# Handoff: widget2 (QE1 gaps and ED2 bindings, `egui_quire`)

Everything has landed on master. The last widget commit is `aa79b12`, and each push passed the
full `scripts/check.sh`.

## Landed
- **Tab stops** (`EditorConfig::tab_size`), in the monospace and galley paths.
  - Each tab is its own `LayoutJob` section, and the text after it gets a `leading_space`
    correction.
  - Caret, hit-testing, a11y run widths, preedit and trailing-tab width all follow.
  - `layout`, `galley_cache` and `height` are `#[doc(hidden)]`.
- **Horizontal extent** from `widths::LineWidths`: visible columns per line, maintained from the
  recent `LineEdit`s.
- **Two-chord bindings:** `KeySeq`, `Keymap::lookup_seq`, `KeyLookup`.
  - The pending chord waits indefinitely, as in VS Code.
  - Escape or a non-matching key cancels it and is swallowed; focus loss also cancels it.
- **Linux PRIMARY:** `TextSelectionSettled` is sent once the selection settles, and
  `MiddleClickPaste` pastes at the pointer.
- **Page-shift announcement:** a `Role::Status` child with `Live::Polite`, labelled with
  `EditorText.line_announcement`.
- **Snapshots** (`tests/snapshots.rs`): light and dark, macOS (Metal) only. Update with
  `UPDATE_SNAPSHOTS=1`.
- **Core ED2 commands bound and tested through the keymap:**
  - insertCursorAbove/Below, addSelectionToNextFindMatch, move/copy lines, deleteAllLeft
    (macOS only, as in VS Code);
  - cursorColumnSelect* (Up/Down are not bound on Linux, where VS Code's keys copy lines);
  - editor.fold/unfold (ctrl+shift+[ and ]), and toggleFold/foldAll/unfoldAll on the ctrl+k
    chords.
- **Mouse:** Alt+Shift+drag makes a box selection (`columnSelect`); Alt+click adds a cursor.
- **Folds:**
  - Hidden lines have zero height in the height index, so `line_at` never lands on them and
    painting skips them.
  - Folded headers get a "⋯" placeholder.
  - The gutter fold column shows chevrons (always for folded headers, on gutter hover otherwise).
    A click toggles via a selection transaction with `Effect::SetFolds`.
- **Find bar:** uses `EditorView::find_current_match`.
- **IME:** commit and DeleteSurrounding go through `compositionType`.
- **SyntaxDriver** (quire-syntax, always built now):
  - Fresh trees feed `Document::set_fold_ranges`.
  - Host integration is in `examples/playground.rs` (Svelte sample, thread spawner) and
    `tests/syntax.rs` (quire-syntax is a dev-dependency only). The host loop: `update` before
    `show` with the last `visible_lines`, waker = `ctx.request_repaint`, `next_deadline` sets
    `request_repaint_after`, and `settled` follows the driver's pending work. No new egui_quire
    API.

## Tests
`scripts/cargo.sh test -p egui_quire`:
- 6 unit tests;
- 54 kittests (`tests/editor.rs`);
- 1 syntax integration test (`tests/syntax.rs`);
- 2 snapshot tests (macOS).

## Next
- **B5:** when core3 lands addCommentLine, removeCommentLine, deleteAllRight and blockComment,
  they resolve automatically: the keymap entries already exist, including blockComment as
  shift+alt+a, and ctrl+shift+a on Linux. Then empty the pinned list in
  `keymap_resolves_against_core_commands` and add kittests that press them.
- The playground runs, but no screenshot was possible: `screencapture` from the shell lacks
  Screen Recording permission.
- **Open:**
  - Box-selection columns come from x / advance; wide CJK glyphs are approximate.
  - The a11y page still includes hidden (folded) lines.
