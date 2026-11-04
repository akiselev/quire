# quire status

## Handoff (2026-11-04, scratchpad-v1 branch)

- Branch `scratchpad-v1`, based on `f2208478faa1466061af03374639ed40e9842eff`.
  Generic source-backed replacement projections and a committed-transaction outbox support Scratchpad.
  Source text, commands and undo remain canonical. No product calculation semantics enter Quire.
- `quire-core` and `skelter_quire` tests: **396 passed**, including Unicode/stale-range validation,
  live mapping across 99 edits, invalidation, independent view reveal and IME preedit fallback.
- Optimized inactive-provider timer scheduling is repaired; 75 release provider/deadline tests and
  named-package all-target Clippy pass. Scratchpad release input/undo now settles and passes.
- Full workspace `scripts/check.sh` has not run. Root filesystem is below its documented 25 GB
  start threshold. This branch has not been merged or pushed to master.
- Details and exact test invocation: `docs/handoff/scratchpad-v1.md`. Existing milestone history below
  is retained. Svelters source was not changed.

## Milestones

| Id | State | sha | check.sh | Tests |
|---|---|---|---|---|
| QE0 | gate passed | 29c4a34 | full check.sh pass (110 pass, 4 ignored) | core 25 unit + 13 document + 10 proptests; VS Code cursor.test.ts 141 pass / 20 ours / 0 bug / 102 N/A |
| QE1 | gate passed | 29c4a34 | full check.sh pass | 33 kittests + 4 unit |
| QE2 | gate passed | 161ef59 | full check.sh pass (136 pass, 4 ignored) | syntax 38 + driver 4; E5 missed: 7.8 ms vs 2 ms target (tree-sitter reparse 3.7 ms); revision asked |
| QE4 | complete (pending main-cadatomic) | 4452869 | full check.sh pass, clean quire build (206 pass, 5 ignored) | VS Code 501/526 = 95.2 %; E1 typing p99 0.27 ms, fling p99 1.23 ms, scroll p99 0.65 ms, open 40 ms; E3 1M-char line 0.48 ms |
| TC0 | gate passed | 1eb8ead | full check.sh pass (241 pass, 6 ignored) | master's list + 20 quire-text tests; VSCODE-MAP byte-identical; idle field 280 B, 2 allocations per inserted character; field create 250 ns, insert 1.2–1.55 µs (clock capped at 46); E2 unchanged |
| TC0b | gate passed | (this commit) | quire-text tests + release bench (no quire-core change) | 28 quire-text unit tests (+1 proptest `incremental_line_index`); `benches/lines.rs` edit + first line query, string backend, before → after: 8 KiB 11.3 → 2.31 µs, 16 KiB 21.4 → 2.44 µs, 32 KiB 37.6 → 2.85 µs, 64 KiB 71.4 → 3.64 µs (CPU_Scheduler_Limit 100, CPU_Speed_Limit 46; load 7.9 / 7.3 / 9.3) |
| P1c | landed (merge built in P1m, see below) | (this commit) | full check.sh pass (389 pass, 6 ignored) | quire-core: suggest_filters 29, suggest_model 13, suggest_session 28, snippet_parser 55, snippet_session 16, vscode_hand_map 1; egui_quire: suggest 5 kittests, keymap audit 83 ids / 82 match. VSCODE-MAP new suites: 132 pass / 15 ours / 50 todo / 51 N/A; overall 633/673 = 94.1 % (633/723 = 87.6 % counting todo) |
| P1h | on branch `claude/b2-quire-p1` (cloud lane; coordinator merges) | 2f49f16 | full check.sh pass (456 pass, 6 ignored) | P1 part 2 (hover, signature help) from `p1-hover` rebased on 819d32b; one clippy fix (egui::Role). Counts as p1-hover.md: hover 40, parameter_hints 20, hints 7 kittests; VSCODE-MAP 674 pass / 41 ours / 52 todo |
| P1m | on branch `claude/b2-quire-p1` | f21aa8f | full check.sh pass (485 pass, 6 ignored) | SnippetSession.merge (VS Code), stop model with enclosing placeholders; snippet_session 16 → 45; VSCODE-MAP 703 pass / 27 ours / 37 todo |
| P1i | on branch `claude/b2-quire-p1` | e167301 | full check.sh pass (515 pass, 6 ignored) | inlay hints as injected text: injected (ModelLineProjectionData, lineBreakData 9), inlay_hints 14 own, egui_quire inlay 7 kittests; VSCODE-MAP 712 pass |
| P1s | on branch `claude/b2-quire-p1` | 6dfc1cc | full check.sh pass (547 pass, 6 ignored) | semantic tokens: tokensStore 16 (+1 N/A), styling 3, ModelSemanticColoring 6 (+5 N/A), getSemanticTokens 1, Dto 4 N/A, 5 own, 1 kittest; VSCODE-MAP 738 / 765 translated = 96.5 % (1281 listed) |
