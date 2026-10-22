# quire

A code editor for egui 0.37, in three crates:

- `quire-core`: rope-backed text, composable change sets, transactions, selections, undo
  history (local or host-owned), diagnostics and tokens. No egui; builds for wasm32.
- `quire-syntax`: tree-sitter highlighting for Svelte, TypeScript, JavaScript and CSS, run off
  the UI thread through a host-supplied spawner.
- `egui_quire`: the widget (virtualised lines, VS Code keymap, find, gutter, IME, AccessKit).

The licence is not decided yet (no licence is granted for now). See `THIRD_PARTY_NOTICES.md` for
translated VS Code material.
