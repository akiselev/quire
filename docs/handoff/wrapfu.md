# Handoff: wrapfu (wrap follow-ups; `egui_quire`, `quire-core`)

Written 2026-10-22 at the end of the wrapfu lane. Five items, all on master (see the shas in
the final commit log). Measured E1 numbers are in `docs/design/E1.md` ("After wrapfu").

## Done

1. **`CodeEditor::measure` with wrap.**
   - With wrap on, measure counts the rows of each shown line at the wrap column the width
     gives, the way `layout_line` lays them out (`wrap::count_rows`).
   - Folded lines are left out, with wrap on or off.
   - The result is cached in `Mem::measured`.
2. **insertCursorAbove/Below and column selection by visual row (wrap on).**
   - The widget resolves these and records `setSelection`.
   - Covers the keyboard column-select steps and Alt+Shift+drag.
   - The column-select data is kept in `Mem::column`.
3. **W1: height-index splice.**
   - `HeightIndex` is chunked: 512 lines per chunk, with a Fenwick tree inside each chunk and
     one over the chunks.
   - `LineWidths` records its splices, and with wrap on a line-count change splices the index.
   - One Enter at 100k lines: 1.56 ms before, 0.035 ms after (release, minimum of 200 runs).
   - The E1 wrap-on runs were paired under load averages 200–360, so they are not comparable
     with the quiet numbers. **A quiet re-run is still owed.**
4. **The IME preedit line goes through the galley and wrap caches** (`layout_preedit_line`,
   `wrap::shape_formatted`). The cache key includes the preedit. It respects the long-line
   prefix.
5. **`AuthorId`:**
   - `Transaction::author` and `with_author`.
   - The fields `Applied::author`, `RecentChange::author`, `UndoGroup::author` and
     `UndoGroupClosed::author`.
   - `Document::execute_as` and `apply_remote_by`.
   - `History::undo_author` / `redo_author`.
   - The `CodeEditor::author` builder.
   - An author change closes the undo group; undo and redo keep the group's author.

## Open

- The quiet E1 wrap-on bench (the coordinator will run it).
- `catastrophic_rule_regex_is_no_match` (quire-core `tests/document.rs`) asserts < 50 ms in a
  debug build. It fails under heavy parallel load (236 ms at load averages 300+) and passes
  alone in 0.14 s. A candidate: mark it timing-sensitive, or measure it in release only.
- With folds present, any edit before a fold changes the fold byte ranges in `heights_sig`, so
  the height index is rebuilt anyway (this predates wrapfu).

## Detail decisions

- measure: folds are now excluded with wrap off too, so measure equals render (before, it
  counted every line). Hidden spans are merged because folds nest. quire has no maxLines
  option, so nothing caps the count.
- measure is cached in the editor's memory per (version, wrap, prefix, row height, advance,
  ppp, font, style, folds). With wrap off it does no layout.
- Wrapped column select:
  - Columns are x in editor-font columns, indent included, as the wrapped Up/Down goal already
    is.
  - Mouse columns come from the galley hit test inside a row's text. Past a row's end, or in a
    continuation row's indent, they come from whole advances (VS Code `mouseColumn`).
  - The explicit `columnSelect` command (document line and visual column, from a host or RPC)
    still goes to core on document lines; only widget input uses view rows.
  - The primary is the from row, as in VS Code.
- insertCursor under wrap:
  - The anchor and head each move one view row; at the first or last row the copy stays put and
    merges, as in VS Code's `translateUp/Down` with allowMoveOnEdgeLine false.
  - The head keeps its goal column, and the new range stores it.
  - The view reveals the furthest new head, as the core does.
- The height index keeps `line_at(y <= 0) == 0` (line 0 is never folded away).
- The preedit layout stays plain-coloured, as before; only the caching and the prefix changed.
- Item 5 naming: `AuthorId`, as proposed; `apply_remote_by` and `execute_as` are additive, so
  existing callers keep compiling. Undo and redo through `execute_as` keep the group's author
  rather than taking the caller's.

## Test counts (full final tree, egui_quire)

- 13 unit tests (new: the splice-versus-vector test), plus 1 ignored release timing test.
- 61 tests in `editor.rs` (new: widget input carries the editor's author).
- 20 tests in `wrap.rs` (new: 2 measure, 3 insert-cursor and column-select, 1 preedit).
- 3 snapshots.
- 2 keymap audit tests.
- 1 syntax test.

quire-core: new `tests/author.rs` (3 tests).
