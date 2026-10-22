# quire status

## Handoff (2026-10-22)

- QE0, QE1, QE2 gates passed (full check.sh on 161ef59). Handoffs: docs/handoff/{core,widget,widget2,syntax}.md.
- Running: core2 (QE4 core: multi-cursor, line moves, folding, large files, IME core, more VS Code suites, E2).
- Waiting on approval (main-cadatomic): batch 1 core2 API; batch 2 widget2 tabs + chords (branch widget2);
  batch 3 SyntaxDriver (behind `unstable-driver`) + htmlx-svelte / tree-sitter 0.26; batch 4 line
  announcement (widget2); Q1 E5 target revision; Q2 queries under crates/quire-syntax/queries.
- After approvals: rebase widget2, wire SyntaxDriver into the widget, bind core2 commands.

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | in progress (core2, widget2) | | | |
