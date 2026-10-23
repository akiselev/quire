# quire status

## Handoff (2026-10-22, wind-down)

- master: TC0 (quire-text, 6f35566), TC0b (fe4919f), P1 part 1 completion and snippets (3b4b15a),
  licence MIT OR Apache-2.0 restored (2b53b5c). Last full check.sh: 389 pass / 6 ignored (3b4b15a).
- **Not landed:** P1 part 2, hover and signature help, on branch `p1-hover` at 7fb2955 (base
  3b4b15a). Filtered tests pass; handoff in that branch's docs/handoff/p1-hover.md. Stopped because
  free disk fell to 6 GB from other lanes' builds. To land: >= 25 GB free, rebase on master, one
  check.sh, fast-forward.
- Decided for P1 part 2 (main-cadatomic): EditorEvent::HoverLinkActivated { href }; parameter-hints
  retrigger after cancel is "ours (async host provider)"; hover code fences monospace, no colour;
  no cancellation event.
- Queued: SnippetSession.merge (14 VSCODE-MAP rows "ours (queued)"), then P1 part 3 inlay hints,
  part 4 semantic tokens; choice placeholders and snippet variables (todo); TC0 step 5 (widget
  wrapped motions on VisualLines); F1 off-thread first find.
- Rules for agents: CLAUDE.md (disk 15 GB, check.sh 25 GB and once per lane, never pkill by
  pattern, no credentials in commits).

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | complete (pending main-cadatomic) | 4452869 | full check.sh pass, clean quire build (206 pass, 5 ignored) | VS Code 501/526 = 95.2 %; E1 typing p99 0.27 ms, fling p99 1.23 ms, scroll p99 0.65 ms, open 40 ms; E3 1M-char line 0.48 ms |
| TC0 | gate passed | 1eb8ead | full check.sh pass (241 pass, 6 ignored) | master's list + 20 quire-text tests; VSCODE-MAP byte-identical; idle field 280 B, 2 allocations per inserted character; field create 250 ns, insert 1.2–1.55 µs (clock capped at 46); E2 unchanged |
| TC0b | gate passed | (this commit) | quire-text tests + release bench (no quire-core change) | 28 quire-text unit tests (+1 proptest `incremental_line_index`); `benches/lines.rs` edit + first line query, string backend, before → after: 8 KiB 11.3 → 2.31 µs, 16 KiB 21.4 → 2.44 µs, 32 KiB 37.6 → 2.85 µs, 64 KiB 71.4 → 3.64 µs (CPU_Scheduler_Limit 100, CPU_Speed_Limit 46; load 7.9 / 7.3 / 9.3) |
| P1c | landed (merge-vs-replace pending approval) | (this commit) | full check.sh pass (389 pass, 6 ignored) | quire-core: suggest_filters 29, suggest_model 13, suggest_session 28, snippet_parser 55, snippet_session 16, vscode_hand_map 1; egui_quire: suggest 5 kittests, keymap audit 83 ids / 82 match. VSCODE-MAP new suites: 132 pass / 15 ours / 50 todo / 51 N/A; overall 633/673 = 94.1 % (633/723 = 87.6 % counting todo) |
