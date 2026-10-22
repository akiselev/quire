# Handoff: widget4 (soft wrap, long-line prefix, P3, P2; `egui_quire`, `quire-core`)

Written 2026-10-22 at the end of the widget4 task. The work is on branch `widget4` (rebased on
master `3eb3f4e`). The full `scripts/check.sh` passed: 206 tests, 5 ignored. It is not on master
yet: it adds three `EditorConfig` fields, which the coordinator decides (listed under "Needs a
decision").

## Done

- **Soft wrap with measured heights (QE4 / ED2), `src/wrap.rs`.**
  - Config: `EditorConfig::word_wrap: WordWrap { Off, On }` (VS Code `editor.wordWrap`; `on` wraps
    at the viewport width) and `wrapping_indent: WrappingIndent { None, Same, Indent, DeepIndent }`
    (VS Code `editor.wrappingIndent`, default `same`).
  - Wrapping column as VS Code: the columns that fit beside the vertical scrollbar.
  - Break positions are a translation of VS Code's `monospaceLineBreaksComputer.ts`: the
    break-before and break-after character sets, Kinsoku rules, ideographs and the wrapped
    indent. Advances are in columns of the editor font, measured from the galley for non-ASCII
    lines.
  - Each row is its own galley, cached per line in `Mem::wraps`. Rows after the first start at
    the indent, and their tabs stop at the line's stops (`TabStops::origin`).
  - Height index: with wrap on, each line's height is estimated from its column count. Laying a
    line out measures it and `set`s the measured height (`FrameCx::note_rows`). Paint lays out
    the visible lines and repeats once or twice if heights changed. Edited lines get a fresh
    estimate; a changed line count rebuilds the index from estimates. Scroll stays anchored to a
    line, so re-measuring off-screen lines does not move the view.
  - Carets, selections, find matches, squiggles and the diagnostic hover go across rows
    (`VisualLine::locate`, `hit`, `x_in_row`; `paint::SpanRects`).
  - A caret at a wrap point belongs to the next row's start unless it came from End, a click
    past a row's end or a vertical move to a row end. Those are "upstream" heads, kept in
    `Mem::upstream` keyed by the selection and version.
  - Up/Down/PageUp/PageDown and Home/End move by visual row, as VS Code's view-model cursor moves
    do (`cursorMoveCommands.ts` `_moveToLineStart` / `_moveToLineEnd`, `moveUp` / `moveDown`):
    - Home on a continuation row goes to that row's start, then to the line start (smart home).
    - End goes to the row end, then the line end.
    - The goal column is an x in columns including the indent.
    - The core knows only document lines, so `FrameCx::wrapped_motion` resolves these to a
      selection and records `setSelection`, as for the mouse.
  - Gutter numbers, fold markers and diagnostic dots go on the first row. The current-line
    highlight covers every row. A folded header's "⋯" goes after its last row. Hidden lines
    stay zero-height, and vertical moves skip them through the height index.
  - With wrap on there is no horizontal scroll.
  - The a11y page counts visual rows. Each row is one or more `TextRun`s (at most 255 chars
    each), and run ids stay `(line, k)`.
- **F2, long-line prefix (E3).** `EditorConfig::stop_rendering_line_after` (VS Code
  `editor.stopRenderingLineAfter`, default 10,000 chars; `usize::MAX` renders every line in full).
  - Longer lines lay out only that prefix, with a "…" marker (`VisualLine::marker`).
  - Positions past the prefix map to the marker's end.
  - A click on the marker's right half goes to the line end.
  - The horizontal extent is clamped to the prefix.
  - Works together with wrap.
- **P3, incremental fold ranges (E1),** in `quire-core` `folding.rs`.
  - `IndentLevels::update` returns a `LevelChange`, and `update_ranges` recomputes only the
    ranges around the edited lines (the module doc gives the rule).
  - `Document::apply` updates the cached ranges in place.
- **P2, ASCII shaping fast path (E1), `src/ascii.rs`.**
  - Printable-ASCII and tab lines in the plain font are built from glyph templates: each char at
    each of egui's four subpixel bins, taken from one layout.
  - The galleys equal egui's (unit test at 4 scales; snapshots unchanged).
  - Gutter numbers use it too.
  - The galley cache evicts a sixteenth at a time.
- **Font atlas rebuilds:** `ascii::AtlasState` (text options, fill ratio). When egui rebuilt
  the atlas, the editor drops its galley caches and the templates are rebuilt. This was a latent
  bug for any cached galley after an atlas overflow.
- **Benches:** `E1_WRAP=1` runs E1 with wrap on in a 480-point editor. `Rig::width` was added, and
  the bench pointer moved to x = 240.
- **Docs:** `E1.md` and `E3.md` have "After widget4" sections with before/after numbers.

## Numbers (shared machine, load 4–5, p99 is an upper bound)

| | before (master) | after |
|---|---|---|
| E1 typing mean / p99 | 0.40 / 2.08 ms | 0.16 / 0.27 ms |
| E1 page fling (40 lines/frame) mean / p99 | 1.69 / 3.79 ms | 0.74 / 1.23 ms |
| E1 scroll 3 lines mean / p99 | 0.41 / 0.75 ms | 0.25 / 0.65 ms |
| E1 idle | 0.12 ms, 7 allocs | 0.12 ms, 7 allocs |
| E3 typing on a 10k / 100k / 1M-char line | 3.5 / 38 / 421 ms | 1.5 / 0.24 / 0.48 ms |
| E1 with wrap on: typing p99, fling p99 | — | 1.53 ms, 1.80 ms |

## Tests

`scripts/cargo.sh test -p egui_quire`:
- 12 unit tests (new: 3 for wrap breaks, 1 comparing the ASCII fast path with egui);
- 60 kittests in `tests/editor.rs`;
- 11 kittests in `tests/wrap.rs` (wrap rows and heights, Up/Down by row, Home/End, a click past a
  row end, indent, folds with wrap, no horizontal scroll, reflow while typing, prefix carets and
  clicks, the prefix a11y run, prefix with wrap);
- 3 snapshots (new: `editor_wrap_prefix.png`);
- 2 keymap audit tests and 1 syntax test.

quire-core's folding unit test now checks the incremental ranges against a full recompute after
each of 400 random edits. The robust proptest checks them too; 3,000 cases passed.

## Needs a decision (coordinator)

- The three `EditorConfig` fields above, with the `WordWrap` and `WrappingIndent` enums
  re-exported from `quire_core`. VS Code names, at the end of the struct.
- Wrapped motions are recorded as `setSelection`, not `cursorMove`. Replaying a recorded
  `cursorMove` without the widget moves by document line. The alternative is a view-line
  provider passed into core's motions, which would be new API.

## Proposals (not implemented)

- `editor.action.toggleWordWrap` (VS Code Alt+Z, all OSes): a widget command that flips
  `word_wrap` on the document config. It is a new user-facing command, so it needs the user's
  approval.
- W1: splice the height index on line-count changes under wrap (Enter frames, 1.5 ms p99 at 100k
  lines with wrap on).

## Open problems

- `CodeEditor::measure` still returns lines × row height (it ignores wrap).
- `InsertCursorAbove`/`Below` and column selection still work on document lines under wrap; VS
  Code uses view lines.
- With wrap on, the IME preedit line is laid out uncached, and wrapped (`wrap::shape_uncached`).
  On a cut long line it lays out the whole line.
- The line-break advances for non-ASCII lines come from the galley, so a font whose wide glyphs
  are not two columns wraps by their real width (VS Code's DOM computer does the same; its
  monospace computer assumes the font's full-width ratio).
