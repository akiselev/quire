# quire status

## Handoff (2026-10-22)

- QE0 and QE1 landed (6b4edbb). Handoffs: docs/handoff/core.md, docs/handoff/widget.md.
- Running: core2 (multi-cursor, line moves, more VS Code suites, folding, large files, IME core, E2),
  widget2 (QE1 gaps: tab stops, chords, PRIMARY, a11y announce, snapshots), syntax (QE2).
- check.sh on 6b4edbb: fmt/clippy fail only in quire-syntax WIP; core + widget scoped checks pass.

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | landed | 6b4edbb | core-scoped pass; workspace red on quire-syntax WIP | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | landed | 52c413f (in 6b4edbb) | widget-scoped pass | 33 kittests + 4 unit |
| QE2 | in progress | | | |
| QE4 | in progress (core2, widget2) | | | |
