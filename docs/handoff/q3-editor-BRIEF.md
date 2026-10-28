# Lane q3-editor: three editor items for cadatomic's gallery (brief)

The user approved this lane on 2026-10-28 (round 3 of the component polish: "Go ahead and start"). The coordinator is
cloud session `session_01HBbGRgGENFKUjsqyGryCSr` (it coordinates the polish batches in svelters and cadatomic, in
macbook-cadatomic's place for this lane). quire's `CLAUDE.md` binds you, with these specifics for a cloud container:

- **Branch** `claude/q3-editor` from master `ed0665b` (the rev cadatomic and svelters pin). Push only this branch;
  don't push master. The coordinator lands it (fast-forward) after your end-of-lane `scripts/check.sh` passes, and
  then moves the consumers' pins.
- **Setup:** a fresh cloud container, one lane in it: the shared-machine slot rules don't apply beyond using
  `scripts/cargo.sh`; keep the 15 GB / 25 GB disk floors. egui is exactly 0.37.0.
- **Approvals:** all three items stay inside VS Code's behaviour and the agreed design, or are small public helpers;
  the coordinator approves them here. A behaviour that would differ from VS Code: stop and record a proposal.
- **Record:** `docs/handoff/claude-q3-editor.md` (what landed, public API added, tests with counts and commands,
  `check.sh` result, questions with a recommendation each), committed as your last push; then message the coordinator
  the sha.

Evidence: cadatomic branch `claude/gal2-components`, `docs/notes/gal2-requests/components.md` "To quire" (read it; the
container may need `add_repo akiselev/cadatomic`, read only).

1. **`CodeEditorKind::register_as(reg, name)`** (skelter_quire; Q-G2). Today the kind keys its props through its
   static `code-editor` spec (`SPEC.prop_named(..)`), so it can't be registered under another tag. Make the kind
   resolve its props from the spec it was registered with (or by index), add `register_as` registering a copy of the
   spec under `name`, keep `register` as `register_as(reg, "code-editor")`. Test: two registrations under different
   tags in one registry both take their props and render; a host's other `code-editor` kind coexists with
   `gallery-source`.
2. **The horizontal scroll bar** (skelter_quire, and egui_quire if it shares the bug; Q-P10): with no wrap and content
   wider than the view, the horizontal bar is drawn over the last visible line. Read VS Code's editor layout
   (`EditorLayoutInfo`: `horizontalScrollbarHeight`, how the last line and `scrollBeyondLastLine` interact with it)
   and match it: the last line must be scrollable clear of the bar. Test: with 400 long lines scrolled to the end, the
   last line's glyphs and the bar don't overlap.
3. **TOML** (quire-syntax; Q-P9): an optional `toml` feature on a TOML tree-sitter grammar that builds with the
   workspace's tree-sitter (check `tree-sitter-toml-ng` first; record the licence in `THIRD_PARTY_NOTICES.md`), a
   highlights query mapped to quire's token types (keep any copied query file's header, as the Helix rule says),
   `Lang::Toml`, and its name `toml` wherever the other languages are named. Tests as for the other grammars.

End with one `scripts/check.sh`, the record, push, and the message.
