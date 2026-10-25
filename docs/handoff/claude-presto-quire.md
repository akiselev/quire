# Handoff: claude/presto-quire

The lane record is `docs/PRESTO-QUIRE.md` (what landed, what was run, gaps, questions, next).

- Done: `crates/presto_quire` (shaping with the ASCII monospace fast path, two-generation caches,
  soft wrap, VisualLines motion, recorder, compositor layers with scroll and caret fast commits,
  TextInputClient, the `code-editor` kind); quire-text document-wide UTF-16 offsets.
- Tests: 26 unit + 1 quire-text unit + 16 integration tests written, **none run** (batch rule);
  `cargo check --tests` and clippy are clean.
- Next: run the tests and fix; coordinator questions 1–5 in the record.
