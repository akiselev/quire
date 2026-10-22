# quire status

## Handoff (2026-10-22)

- QE0, QE1, QE2 gates passed (161ef59). core2 merged at b69a48c (full check.sh: 144 pass, 4 ignored).
- Approved (user, cadatomic register b948bcb): B1 (core2 API), F (language rules), B2 (tab stops,
  chords). Decided by coordinator: Q2 (queries under crates/quire-syntax/queries).
- With the user: B3 (SyntaxDriver, htmlx-svelte, tree-sitter 0.26; keep `unstable-driver` off by
  default), B4 (line announcement, branch widget2-d), B5 (comment / deleteAllRight commands, branch
  core3-b5), B1's flagged departures, Q1 (E5 target), Q3 (E2 line fetch).
- Running: widget (merge widget2 A+B, bind core2 commands), core3 (F, indentation and comment ports),
  qe4 (E1, E3, idle check, robustness proptest).

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | in progress | b69a48c (core2) | full check.sh pass | VS Code 354 pass / 41 ours / 0 bug / 380 N/A (89.6 %; gate 90 %) |
