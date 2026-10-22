# Handoff: widget (QE1, `egui_quire`)

Branch `widget`, landed on master. Tests: `scripts/cargo.sh test -p egui_quire` — 4 unit tests,
33 kittests in `crates/egui_quire/tests/editor.rs`, all passing. Clippy `-D warnings` clean for
the crate.

## Done

- `CodeEditor::new(&mut Document, &mut EditorView, id_salt, a11y_name)` with `.id(Id)`,
  `.theme`, `.text` (host strings), `.key_handling(KeyHandling::{Builtin(Keymap), Host})`,
  `.read_only`; `measure`, `show_in(ui, rect)`, `show(ui)`. Egui memory only under the editor id
  and ids derived from it (E-2). Static helpers: `id_for`, `anchor_rect(ctx, id, "caret" |
  "line[N]")`, `galley_stats` (hidden, tests/perf).
- `EditorOutput { response, applied, commands, events, caret_rect, settled, dirty,
  visible_lines }`; events: SaveRequested, Undo/RedoRequested (UndoMode::Host, or the core's
  `HostRequest`), FocusChanged, HoverAt (0.5 s rest), LinkActivated (Ctrl/Cmd+click word),
  ContextMenu, DiagnosticAction (args parsed from `args_json`).
- Input is applied in-frame through `Document::execute` (time in ms from egui for grouping);
  mouse selection is a selection-only transaction recorded as `setSelection`.
- Keys: `keymaps/vscode-default.json` (VS Code `keybindings.json` shape: key/mac/linux/win,
  command, args, when ∈ {findWidgetVisible, editorHasMultipleSelections}); resolved via
  `EditorCommand::from_vscode_id`, then serde `{id, ...args}`, then widget actions (open find /
  replace). Unknown ids are not bound and listed in `Keymap::unresolved()`. Exact physical
  modifier match; synthetic `command`-only modifiers are mapped per platform. Handled events are
  removed from egui input (R11). Host mode handles only Text, IME and clipboard events.
- Rendering: Fenwick `HeightIndex` (uniform), LRU galley cache keyed (content, token-style ⊕ theme
  hash, font, ppp, wrap), one `LayoutJob` section per token run with colours resolved at paint
  time (default Dark+/Light+ palette), monospace fast path (`layout::Mono`, measured advance;
  within 1 physical px of egui's snapped glyph origins), gutter with numbers and worst-severity
  marks, current line, selections (all ranges), find matches, squiggles (hint = short underline),
  carets (blink stops after 5 s idle, off when `visuals.text_cursor.blink` is false), scrollbars,
  drag autoscroll. Scroll is stored in the `EditorView` (anchor byte + offset); `Reveal` effects
  honoured.
- IME: preedit spliced into the caret line (underlined, no buffer change, `view.set_preedit`),
  commit = one Type, DeleteSurrounding = one transaction, Escape cancels preedit only (focus
  filter keeps Escape while composing), `IMEOutput` written while focused.
- Clipboard: Copy/Cut/Paste events; empty selection copies whole lines (matches core's Cut).
- Find bar: query/replace inputs (a11y labels from `EditorText`), case/word/regex toggles, count
  or regex error, prev/next, replace one/all, Escape closes and refocuses the editor.
- Diagnostics: hover tooltip with messages and action buttons, kept open while hovered.
- A11y: `MultilineTextInput` node with name and Focus/SetValue/ReplaceSelectedText; TextRun pages
  (50, 500 with screen reader) that follow the caret; ids `Id::unique((editor_id, "run", line))`
  (+ chunk for >255 chars); primary selection mapped. Runs are parented through child Uis with
  `accessibility_parent` (egui's `register_accesskit_parent` is crate-private).
- `examples/playground.rs` (eframe dev-dependency; two views of one document). Builds; not run
  here.

## Next / open

- Reduced motion: egui 0.37 has no option; hosts map it to `visuals.text_cursor.blink = false`.
- Snapshot images not attempted (no wgpu feature).
- Tabs render at egui's fixed tab width (not tab stops).
- Content width for horizontal scroll is the widest line laid out so far.
- Two-step chords unsupported. Keymap ids core lacks: deleteAllLeft,
  addSelectionToNextFindMatch, copyLines{Up,Down}Action, insertCursor{Above,Below},
  moveLines{Up,Down}Action.
- Not sent: `TextSelectionSettled` (Linux PRIMARY); page-shift announcement.
- Per-frame allocations remain for token runs and owned line strings across rope chunks.
- QE4: soft wrap with measured heights, Alt+click / column selection, IME polish, AccessKit
  paging beyond a page.
