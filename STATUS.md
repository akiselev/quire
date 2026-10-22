# quire status

## Handoff (2026-10-22)

- QE0, QE1, QE2 gates passed (161ef59). QE4 gate run passed on 4364817 (E11 94.0 %).
- Landed since: core2 (B1), widget2 (A+B+D), SyntaxDriver (no feature gate), F and B5, D1 regex
  limit (b5a3c97), keymap audit (W1), qe4 benches and fixes. Handoffs in docs/handoff/.
- Running: core4 (P1 indentation to spaces/tabs, P2 reindent, P3 comment options, P4 public
  visual_col, P5 multi-range SyntaxDriver::update), widget4 (soft wrap, long-line prefix,
  incremental folds, ASCII shaping fast path).
- Open proposals: F1 first find off the UI thread for large files (deferred); `Text::line_into`
  (not needed unless zero allocations per frame becomes a rule).

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | gate run passed; remainder in progress | 4364817 | full check.sh pass, clean quire build (185 pass, 5 ignored) | VS Code 487/518 = 94.0 % (E11 met); E1 open 40 ms, scroll p99 0.63 ms, typing p99 3.3 ms (miss), fling p99 5 ms (miss); remaining: soft wrap, long-line prefix, incremental folds, ASCII fast path |
