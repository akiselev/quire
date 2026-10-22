# Handoff: widget3 (keymap audit, a11y folds, box-selection columns, B5 bindings; `egui_quire`)

Everything below is on master; each push passed the full `scripts/check.sh`.

## Landed
- **Keymap audit against VS Code `bdce63db`** (decision W1: per-OS keymaps match VS Code exactly).
  - `scripts/vscode-keybindings.py` (run with `python3 -I`; `--src DIR` reuses a checkout,
    otherwise it curls the sources into a new temp dir). It reads every `kbOpts` /
    `KeybindingsRegistry` rule for the ids in `keymaps/vscode-default.json`, resolves them per OS
    with VS Code's `bindToCurrentPlatform` (a platform object replaces primary and secondary;
    `CtrlCmd` = Ctrl/Cmd, `WinCtrl` = Ctrl on macOS, Meta elsewhere), merges `kbExpr` with the
    command's `precondition`, and writes `keymaps/vscode-extracted.json` and
    `docs/design/KEYMAP-AUDIT.md`.
  - `when`: focus, writable, tabDoesNotMoveFocus, folding enabled, !isComposing and the Windows
    screen-reader word-nav guard are implicit; `editorHasMultipleSelections` and
    `findWidgetVisible` are modeled; rules with other contexts (column-selection mode, find input
    focused) are listed as N/A.
  - 71 ids: 70 match after fixes, 1 N/A (`type`, which VS Code does not bind). 20 commands fixed:
    Windows 4, Linux 8, macOS 12 command/OS pairs (mac emacs secondaries ctrl+b/f/p/n, ctrl+h,
    ctrl+backspace, ctrl+d, ctrl+delete, cmd+delete, f3/shift+f3; shift+escape for
    closeFindWidget and removeSecondaryCursors; Linux column-select keys removed; ctrl+shift+z no
    longer redo on mac; Windows ctrl+shift+up/down select; Linux insertCursor ctrl+shift+up/down).
  - `tests/keymap_audit.rs`: the keymap equals the extracted table per OS (primary first), and
    every expected key, pressed on its OS with its context, reaches its command (nothing shadows
    it).
  - When adding a binding: add it to `vscode-default.json`, rerun the script, and commit both.
    A new id needs a VS Code source file in `SOURCES` if it is declared elsewhere.
- **A11y page skips folded lines:** the page counts visible rows (row = y / row_h in the height
  index, line at row r = `line_at(mid-row r)`), so hidden lines get no TextRun. Run ids stay
  keyed by document line.
- **Box-selection columns** come from the galley hit test plus quire-core's visual-column rule
  (`glue::visible_col`, a copy of core's crate-private `visible_col`/`is_wide`; egui_quire now
  depends on `unicode-segmentation`). Past the line end, whole advances are added, as in VS Code.
- **B5:** core3 landed addCommentLine, removeCommentLine, blockComment and deleteAllRight and
  emptied the pinned-unresolved list; kittests press each binding (deleteAllRight's both mac
  keys).

## Tests
`scripts/cargo.sh test -p egui_quire` (macOS): 6 unit, 59 kittests (`tests/editor.rs`),
2 keymap audit, 1 syntax, 2 snapshots.

## Open
- **SyntaxDriver takes one visible range per document.** With two views of one document, the host
  should pass the hull of both views' visible lines (`min(start)..max(end)`); a per-view list of
  ranges would be a new public signature on `SyntaxDriver::update` (needs approval).
- `glue::visible_col` duplicates core's rule. If core exposes it publicly (new API, approval),
  switch to that.
- Column-selection mode (`editor.columnSelection`, shift+arrows while it is on) is not modeled.
