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
