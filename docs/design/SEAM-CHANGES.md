# Seam changes that override ARCH-QUIRE.md

Decided 2026-10-22 (user, relayed through main-cadatomic; cadatomic register `82486b6`,
"Seam corrections R1–R12" and "Architecture recommendations taken"). Where `ARCH-QUIRE.md`
disagrees, this file wins.

1. **Seam 10: Document + EditorView.** `EditorState` is split in two:
   - `Document` (shared, one per file): text, version, history, undo mode, recent changes, tokens,
     diagnostics, `saved_at`, config.
   - `EditorView` (one per view of a document): selection, `selection_version`, folds, find state,
     scroll, focus, composing (IME preedit).
   - Two views of one file keep separate carets. Edits made through one view map the other view's
     selection, folds and find state forward (through `Applied.changes`).
2. **Seam 9: the widget applies input.** The widget takes `&mut Document` and `&mut EditorView` and
   applies input transactions itself during the frame, so typed text appears in the same frame.
   It still returns the `Applied` records in its output. `UndoGroupClosed` is the host's `text.edit`
   boundary (CE3, `UndoMode::Host`): in Host mode undo/redo are emitted as intents and the host
   answers with transactions whose origin is `Undo`/`Redo`.
3. **R11: keys.** The widget consumes the keys it handles from egui's input
   (`InputState::consume_key` / `consume_shortcut`), so a host can tell what is left (e.g. before
   routing Enter or Escape to default buttons). `EditorView::composing()` reports an active IME
   preedit.
4. **R8: settled.** `Document::is_settled()` / `EditorView` pending work must be readable cheaply
   (`Send + Sync` probe is fine) for svelters' `Widget::busy()` (pending parse jobs, pending
   debounce, caret blink does not count).
5. **R12: process.** Local `scripts/check.sh` only (fmt, clippy -D warnings, tests, wasm32 check of
   quire-core); no hosted CI.
6. **Views sync themselves** (main-cadatomic, 2026-10-22). `Document::apply(&mut self, tr,
   acting: &mut EditorView)` applies through the acting view. Other views do not depend on the host
   handing them every `Applied`. Each `EditorView` records the `Document` version it last synced at.
   Before use (`show_in`, commands) it maps itself forward through the Document's `RecentChanges`.
   If it is older than that window, it clamps its selection and folds to valid positions and marks
   find stale. A proptest covers two views with interleaved edits, with both selections valid after
   every step.
7. **Approved departures and targets** (user, 2026-10-22, cadatomic register `be6b40e`):
   - `CompositionType` counts `replace_prev_chars`/`replace_next_chars` in code points, not UTF-16
     units; a host bridging an IME converts at the boundary.
   - One long line does not turn off syntax for the whole file (`Degraded::syntax_off()` is size or
     line count only).
   - `MoveLines*` merges selections whose line spans touch into one block.
   - Svelte grammar `tree-sitter-htmlx-svelte` 0.1.16 and `tree-sitter` 0.26.13 (§3.6 named
     svelte-ng 1.0.2 and 0.27).
   - E5 and E2 targets revised in §3.9.
   - fancy-regex accepted with a low backtrack limit; a limit hit is no match (user, 2026-10-22).
     The limit is 20,000 (fancy-regex's default is 1,000,000), measured against VS Code's rule sets.
     After 8 hits a rule pattern is not run again. Delegated programs are capped at 1 MiB. The
     reasons are in the `quire-core` `lang` module doc.
