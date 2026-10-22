# TC0: `quire-text`, the shared text-editing core

Decided 2026-10-22 (user: svelters draws every widget; text fields share one editing core from
quire. Scope details: main-cadatomic). Status: waiting for svelters ARCH-WIDGETS' interface section;
the public API goes to main-cadatomic for review before the crate is split.

## Rules

- New crate `crates/quire-text`. No egui, no svelters, wasm32-clean, no threads. `quire-core`
  depends on it; svelters pins it by git rev.
- The split changes no quire behaviour: VSCODE-MAP and every test stay green.
- Keeps the `Document` / `EditorView` split (shared buffer, per-view selection) so a single-line
  field uses the same model.

## In `quire-text`

- `Text` (ropey 1.6 private), `Version`, `ChangeSet`, `Transaction`, `Selection`/`SelRange`
  (multi-range stays in the type; fields use one), `Origin`, `AuthorId`, `Applied`, `Snapshot`,
  `RecentChanges`, history and undo grouping (Local and Host modes).
- IME composition model (preedit on the view, `compositionType` semantics).
- Basic motion and edit commands shared by fields and the editor: character, word, line,
  Home/End moves with and without select; select all; delete character and word both ways;
  insert or replace selection; paste normalisation.
- **Word rules as a host-chosen policy** (`WordRules`, trait or data table). quire uses VS Code's
  rules; svelters fields use each platform's native rules (W1; svelters FX1 matches GTK on 141/144).
- **Single-line mode** at the Document level: newlines refused or stripped (paste strips them),
  Enter not consumed.

## Stays in `quire-core`

Auto-close and overtype, find, indentation and language rules, comments, folding, tokens,
diagnostics, large-file policy, code-editor commands (multi-cursor commands, line moves, column
select, …).
