# quire status

## Handoff (2026-10-22)

- QE0 and QE1 landed (6b4edbb). Handoffs: docs/handoff/core.md, docs/handoff/widget.md.
- Running: core2 (multi-cursor, line moves, more VS Code suites, folding, large files, IME core, E2),
  widget2 (QE1 gaps: tab stops, chords, PRIMARY, a11y announce, snapshots), syntax (QE2).
- QE0/QE1 gate: full check.sh passed on 29c4a34. Pending approval (main-cadatomic): core2 batch 1, widget2 batch 2, SyntaxDriver + grammar versions batch 3.

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | in progress | | | |
| QE4 | in progress (core2, widget2) | | | |
