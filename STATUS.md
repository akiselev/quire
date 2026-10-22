# quire status

## Handoff (2026-10-22)

- QE0, QE1, QE2 gates passed (161ef59). QE4 complete on 4452869 (full check.sh from a clean build).
- TC0 (`quire-text`) landed: full check.sh pass; allocation and timing gates pass (docs/handoff/tc0.md).
- Handoffs: docs/handoff/{core,core2,core3,core4,widget,widget2,widget3,widget4,syntax,qe4,wrapfu,tc0}.md.
- Open proposals: `editor.action.toggleWordWrap` (needs user approval); F1 first find off the UI
  thread (deferred); height-index splice on line-count change (internal, wrap-on Enter p99).
- Known limits: `CodeEditor::measure` ignores wrap; insertCursorAbove/Below and column select use
  document lines under wrap; wrapped moves are recorded as setSelection; IME composition line laid
  out uncached.
- Next for this machine: berth BD6 macOS half (brief from main-cadatomic).

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | complete (pending main-cadatomic) | 4452869 | full check.sh pass, clean quire build (206 pass, 5 ignored) | VS Code 501/526 = 95.2 %; E1 typing p99 0.27 ms, fling p99 1.23 ms, scroll p99 0.65 ms, open 40 ms; E3 1M-char line 0.48 ms |
| TC0 | gate passed | 1eb8ead | full check.sh pass (241 pass, 6 ignored) | master's list + 20 quire-text tests; VSCODE-MAP byte-identical; idle field 280 B, 2 allocations per inserted character; field create 250 ns, insert 1.2–1.55 µs (clock capped at 46); E2 unchanged |
| TC0b | gate passed | (this commit) | quire-text tests + release bench (no quire-core change) | 28 quire-text unit tests (+1 proptest `incremental_line_index`); `benches/lines.rs` edit + first line query, string backend, before → after: 8 KiB 11.3 → 2.31 µs, 16 KiB 21.4 → 2.44 µs, 32 KiB 37.6 → 2.85 µs, 64 KiB 71.4 → 3.64 µs (CPU_Scheduler_Limit 100, CPU_Speed_Limit 46; load 7.9 / 7.3 / 9.3) |
