# Handoff: quire-core (branch `core3`)

Written 2026-10-22 at the end of the core3 task. Everything is on master (`f38646e` F,
`c2defb0` batch 5 + ports + fold fix); full `scripts/check.sh` passed before each push.

## Done

- **Proposal F (approved): language rules in `EditorConfig`.**
  - Types: `OnEnterRule`, `EnterAction`, `IndentAction`, `IndentationRules`; fields
    `on_enter_rules`, `indentation_rules`. Serde uses VS Code's language-configuration names
    (`indentAction`, `increaseIndentPattern`, …), so a `language-configuration.json` loads directly.
  - `src/lang.rs` translates onEnter.ts, enterAction.ts, autoIndent.ts and electricCharacter.ts.
    Enter, Tab on a blank line, re-indent on type (decrease pattern) and shift's "miss-aligned"
    line handling use it. Electric characters are the closing brackets of `brackets` (VS Code).
  - Regexes are JavaScript syntax, compiled with **fancy-regex 0.19** (built on regex-automata;
    needed for lookaround and backreferences, which VS Code's own rule sets use). A small
    translator handles JS-only syntax (lone `{`/`}`, `[` inside classes, `\/`); flags go inline.
- **Batch 5 (approved):** `AddCommentLine`, `RemoveCommentLine`, `BlockComment`, `DeleteAllRight`.
  `src/commands/comment.rs` is a literal port of lineCommentCommand.ts, blockCommentCommand.ts and
  the comment.ts actions (shared-line rule, block fallback when no line token). Comment commands
  are now undo-isolated (VS Code pushes stops around them).
- **Folds:** `map_fold_ranges` keeps a fold when its header line is split or joined; the header is
  the line holding the old header end (VS Code's decoration tracking).
- **Port:** suites lineCommentCommand, blockCommentCommand, indentation added; `testCommand` with a
  language; VS Code's test language definitions (`test/common/modes/supports/*.ts`) are inlined;
  autoIndent levels map to dropping rules; CharacterPairSupport defaults (pairs from brackets, doc
  comment pair) and OnEnterSupport default brackets are applied in `port.mjs`.

## Numbers

VSCODE-MAP: 940 tests, 518 translated, **487 pass / 31 ours / 0 bug (94.0 %)**, 422 N/A.
cursor 185/10/0 (72 N/A); lineComment 41/4; blockComment 26/2; indentation 38/0 (49 N/A, 26 of them
skipped upstream). Workspace tests: 168 pass, 4 ignored; monaco harness 491 fixture cases pass,
31 expected failures.

## Next / open

- Proposals not yet sent as API: `editor.action.indentationToSpaces` / `indentationToTabs`
  (8 indentation rows), `editor.action.reindentlines` (uses `lang::good_indent_for_line`, kept
  `allow(dead_code)`), comment options `insertSpace` / `ignoreEmptyLines` / `lineComment.noIndent`
  (7 rows "ours").
- cursor "issue #122714" is triaged "ours": the test's language has no pairs, yet VS Code expects
  `{` to auto-close; not reproduced (see triage.json).
- quire's `brackets` serves both onEnter and electric characters; VS Code builds onEnter brackets
  by default when a language has rules but no brackets, while electric characters need explicit
  brackets. No test shows the difference.
- moveLines with autoIndent full (indentation rules) is still N/A.
