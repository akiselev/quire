# Handoff: tc0 (TC0, the `quire-text` extraction)

Written 2026-10-22 at the end of the TC0 lane. Two agents worked on it; the second took over
when the first ran out of context. Design: `docs/design/TC0.md`; the API as built:
`docs/design/TC0-API.md` (entries marked *(built)* differ from the approved draft).

## Done

- **Steps 1 and 2 (17c0f3c).** `crates/quire-text` holds Text, ChangeSet, Selection,
  `Transaction<E>`, history, recent changes, words, `TextDocument` and `TextView`. quire-core's
  `Document` and `EditorView` wrap the last two, and every moved type is re-exported under its old
  path.
- **Step 3 (28df411).** `EditCommand`, `EditRules`, `VisualLines` and `NoWrap`, plus
  `resolve`/`execute` and the FX1 word rules (`words/native.rs`). quire-core's overlapping
  commands are lowered and the duplicate code is deleted. `Document::edit_rules()` caches the
  word classifier.
- **Step 4 (51de89a).** The FX3 undo models (`history/native.rs`), `replace_all` with the
  `ExternalSet` modes, and `undo_limit`. Tests: `tests/{alloc,native,rules}.rs`.
- **Step 6 and the allocation budget (1eb8ead).**
  - `Text` has two backends: a copy-on-write `Arc<String>` up to 64 KiB, ropey above that.
    Hysteresis: a text is promoted past 64 KiB and demoted only by `replace_all` under 32 KiB.
  - `apply` edits the text in place.
  - `ChangeSet` keeps three operations inline.
  - `ops` has one-range fast paths.
  - `benches/field.rs`; check.sh gains the quire-text checks.
  - THIRD_PARTY_NOTICES carries svelters' MIT notice.
- **Step 5 (egui_quire's wrapped motions on `VisualLines`) was skipped.** It was optional (Q3);
  a follow-up lane can do it, gated by unchanged `tests/wrap.rs` and `tests/editor.rs`.

## Proof of neutrality

- Workspace tests: 241 pass, 6 ignored (247). The test list is master's 227 plus 20 new
  quire-text tests. Nothing under `crates/{quire-core,egui_quire,quire-syntax}/tests` or
  `fixtures/` changed.
- VSCODE-MAP: regenerated from the monaco harness with `tests/port/map.mjs`, byte-identical to
  master's (501 pass, 25 ours, 0 bug, 414 N/A).
- **Departure:** the step-3 differential proptest (old against lowered) was not written, because
  the old code was deleted in the same change. The unchanged suite and VSCODE-MAP are the check
  instead.
- **Gate history:** the previous lane's gate had one failure, `catastrophic_rule_regex_is_no_match`
  (a timing bound), fixed on master in `14ff0e9`. This lane's `scripts/check.sh` was the full run
  covering it, and it passed.

## Gates (TC-EVAL T6, T7)

- Idle field: 280 B (`TextDocument` 96 B + `TextView` 120 B + 64 B heap); the gate is 512 B.
- One inserted character: 2 allocations each for GTK, Qt and Monaco (gate 8).
- `TextDocument::new("short")` allocates exactly what `Text::from("short")` does (2). `TextView::new`
  allocates nothing.
- Timing (`benches/field.rs`): run in a quiet window granted by the coordinator, with the clock
  capped (CPU_Scheduler_Limit 100, CPU_Speed_Limit 46; load averages 5.1 / 6.8 / 11.4):
  - create: 250 ns (gate 1 µs);
  - insert one character: GTK 1.21 µs, Qt 1.20 µs, Monaco 1.55 µs (gate 2 µs).
  - **Pass.**
- E2 rerun (T7): large files (ropey) are unchanged or faster. See `docs/design/E2.md`, "After TC0".

## Open

- Step 5 (above).
- The string backend's line queries cost O(n) once per version, to build the line-feed index.
  Editors on 10–64 KiB files pay this once after each edit. No benchmark covers it yet; E1 on a
  mid-size file would show it.
- `Text::grapheme_next`/`grapheme_prev` are new and used only by the `backends_agree` test so far.
