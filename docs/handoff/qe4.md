# Handoff: qe4 (E1 headless, E3, idle, robustness)

## Done (branch `qe4`; landed on master when check.sh passes)

- **E1, headless part:** `crates/egui_quire/benches/e1.rs` (criterion plus a report). The shared
  rig is in `benches/support/{rig,gen}.rs`; allocations are counted with the dev-only `stats_alloc`
  global allocator. Numbers and fixes are in `docs/design/E1.md`. Scrolling and open meet their
  targets. Typing has a mean of 0.5 ms; its p99 is about 3 ms on the Enter frames (fold range
  rebuild). Clean idle frames make 7 allocations, none of them per line.
- **E3:** `crates/egui_quire/benches/e3.rs`. Numbers and recommended thresholds are in
  `docs/design/E3.md`: keep 20 MB, 300k lines and 10k characters, and render a prefix of long
  lines (the long-line cliff is the real problem).
- **Idle:** the kittest `idle_requests_no_frames_after_the_blink_stops` (`tests/editor.rs`) covers
  typing, pointer hover and diagnostics, then checks 5 s of idle with no repaint request.
- **Robustness:** `crates/quire-core/tests/robust.rs` drives random `EditorCommand` streams through
  one Document and two views, with undo, redo, every command on master, folds and find. Set
  `QUIRE_ROBUST_CASES=n` for longer runs; 30k+ cases passed. Three pinned regression tests sit in
  the same file.
- **Internal fixes:** no public API changed. The list is in `E1.md` ("Fixes made") and covers:
  - incremental indentation levels (`folding::IndentLevels`);
  - incremental find refresh for plain queries, with the compiled regex cached in the view;
  - one-pass line widths and long-line check;
  - a uniform `HeightIndex`;
  - no per-frame allocations for the keymap, the default strings, gutter numbers or mono metrics;
  - buffers reused across frames.
- **Bugs the proptest found, all fixed:**
  1. Typing a closing bracket or quote right before a multi-byte char panicked (overtype sliced
     inside the char). Auto-close's prefix look-back had the same bug.
  2. IndentLines with two selections kept the second range's start unmapped, so the selection
     landed inside a char or at the wrong place.
  3. MoveLinesDown over the last line put the selection one byte past the end of the text.

## Next

- F2 (E3.md): render a prefix of lines longer than `max_line_len` (widget).
- F1 (E3.md, needs API): run the first find off the UI thread for files over about 4 MB.
- P3 (E1.md): an incremental fold range update, so Enter frames are O(edit).
- P1 (E1.md, needs API): `Text::line_into`, to remove the last two per-frame allocations.
- P2 (E1.md): an ASCII shaping fast path, so a 40-lines-per-frame fling gets under 2 ms.

## Open problems

- The E1 numbers are from a shared machine (load 7–40), so treat p99 as an upper bound.
- `dirty` stays true after editing back to the saved text by hand. That matches VS Code; the
  robust test asserts only "changed ⇒ dirty".
