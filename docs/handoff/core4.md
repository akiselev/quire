# Handoff: quire-core (branch `core4`)

Written 2026-10-22 at the end of the core4 task. Everything is on master: `b5a3c97` (task 1),
then `d5b62e2`..`05d988b` (P1 to P5). The full `scripts/check.sh` passed before each push.

## Done

- **Rule regexes bounded (D1, user condition for fancy-regex), `src/lang.rs`.**
  - Backtrack limit 20,000 per match (fancy-regex's default is 1,000,000). The module doc has
    the measurement:
    - 99 rule regexes (fixtures, every VS Code built-in language-configuration.json,
      svelte-vscode, rust-analyzer) against 26k corpus lines, their halves and long joined lines,
      2.59M cases in all.
    - The common languages need at most 6.1k steps on lines up to 400 bytes. Julia (40k) and
      Ruby (12k/43k) are the outliers.
  - A limit hit is no match. The same text with the same pattern is not retried, and after 8
    hits the pattern is disabled.
  - Delegated programs and their DFA caches are capped at 1 MiB.
  - Test: `tests/document.rs` `catastrophic_rule_regex_is_no_match`. Enter and the electric `}`
    take at most 16 ms in debug.
  - The survey is the ignored test `lang::tests::backtrack_survey`; its input comes from
    `tests/port/bt-survey.mjs`.
  - JS translation fixes found by the survey: `-` next to a class escape (`[\w-.]`, HTML's
    onEnter rules), `[]` and `[^]`, and a backreference to an unset group (JS matches empty;
    `\N` becomes `(?(N)\N|)`). All 2.59M cases now agree with JavaScript.
- **P1 (approved):** `IndentationToSpaces` / `IndentationToTabs`, a literal port in
  `src/commands/indentation.rs`.
  - Tracked selections follow VS Code's interval-tree marker rule (`nodeAcceptEdit`).
  - `Effect::SetInsertSpaces(bool)` sets the document config. It is not undone, it is refused on
    read-only documents, and `Applied::config_changed` reports it.
- **P2 (approved):** `ReindentLines` (`getReindentEditOperations`, `replaceMove` cursor rule).
  No `ReindentSelectedLines`: VS Code's suite has no reindent tests.
- **P3 (approved):** `EditorConfig.comments: CommentOptions { insert_space, ignore_empty_lines }`
  (serde `comments.insertSpace`, `comments.ignoreEmptyLines`, both true by default) and
  `EditorConfig.line_comment_no_indent` (`lineCommentNoIndent`). The port maps them, and the
  comment `indentSize` argument maps to the model's indentSize.
- **P4:** `quire_core::columns::{visual_col, byte_at_visual_col}`. egui_quire uses them; its
  copy and its unicode-segmentation dependency are gone.
- **P5:** `SyntaxDriver::update(doc, visible: &[Range<usize>], now_ms)`. Each uncovered range
  gets its own job, one at a time; only the first job after an edit reparses. The playground
  passes both views. Test: `two_views_disjoint_ranges_are_both_highlighted_from_one_parse`.

## Numbers

- VSCODE-MAP: 940 tests, 526 translated, **501 pass / 25 ours / 0 bug**, 414 N/A.
- indentation 46/0 (41 N/A), lineComment 45/0 (5 N/A), blockComment 28/0.
- Workspace tests: 190 pass, 5 ignored.

## Next / open

- `lang::good_indent_for_line` is still `allow(dead_code)`; auto-indent on paste would use it.
- Rule regexes see raw lines (no tokens), as before.
- moveLines with autoIndent full (indentation rules) is still N/A.
