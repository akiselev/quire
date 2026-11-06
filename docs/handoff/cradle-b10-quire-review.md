# Cradle batch 10, lane `quire-berth` (quire half): review

Reviewer session, 2026-11-06. Branch `cradle-b10-quire`, reviewed range `7c0a3aa...313555d`
(6 commits: 1ab681c, 1587ebe, e34359a, f14cf3c, 52e4120, 313555d). Coordinator: Cradle batch
coordinator `session_01EQ79tfL3cS9uzD95nyo993`. Lane report: `docs/handoff/cradle-b10-quire.md`.

## Verdict

**Mergeable once the coordinator answers Q1 below.** No defect found that blocks merging; two
plausible gaps (R1, R2) and test gaps (T1–T6) are follow-ups. Nothing was changed in code by this
review.

## How the review was done, and what it could not do

- **Static only. Nothing was built or run.** `cargo fetch` fails here: the svelters git dependency
  needs credentials (`could not read Username for 'https://github.com'`), and
  `add_repo akiselev/svelters` (read) was **refused** by the auto-mode classifier ("[Access Added As
  A Means]"). The lane had that access; this session does not. So: no `scripts/cargo.sh test`, no
  mutant runs, no `scripts/check.sh`, and **no regression tests for the findings** (a test that
  cannot be compiled here would only risk breaking the branch). The findings below carry the test
  to write first.
- **`add_repo akiselev/cradle` (read) was refused** the same way, so the lane brief
  (`docs/notes/batch10/LANE-quire-berth.md`) and `REVIEW.md` were not read. The review is against
  the lane report, quire's `CLAUDE.md`, `docs/design/ARCH-QUIRE.md` §3.8 and egui_quire's
  `src/a11y.rs`.
- **svelters' API was not read** (`NodeView::scale`, `A11yCx::add_virtual_child`,
  `skelter_core::a11y::{virtual_id, MAX_VIRTUAL_CHILDREN}`, `EventCx::request_a11y`,
  `Document::resize_window` marking `PAINT`, "layout marks `A11Y`"). Claims resting on it are
  marked *unverified*.
- Every changed line of `git diff 7c0a3aa...HEAD` was read: `Cargo.lock`, `STATUS.md`,
  `docs/SKELTER-QUIRE.md`, `docs/handoff/cradle-b10-quire.md`, `skelter_quire/src/{a11y,editor,kind,lib}.rs`,
  `skelter_quire/tests/{a11y_runs,theme,window_scale}.rs`.

## Hard rules

| Rule | State |
|---|---|
| quire-core headless (no egui/winit/wgpu/tokio), wasm32 | Unchanged: the diff touches only `skelter_quire`, docs and `Cargo.lock`. quire-core's deps are quire-text, smallvec, regex-automata, fancy-regex, serde/schemars. |
| No threads | None spawned. |
| egui `=0.37.0` | Not touched. |
| `unsafe_code = "forbid"` | No `unsafe`. |
| No svelters dependency in quire-core | Holds. The new `svelters-abi` lock entry is a dependency of `skelter-base` and `svelters-style` only. (`skelter_quire`'s svelters dependency predates the lane.) |
| Ropey never public | New public API (`a11y::TextRun`, `TextRuns`, `SkelterEditor::a11y_runs`) takes `quire_core::Document`/`EditorView`, no rope. |
| One mutation path | The a11y code only reads. `a11y_action` (pre-existing) goes through `replace_range`. |
| No credentials in committed files | None. |

## Lane report claims

| Claim | Checked |
|---|---|
| Commits 1ab681c (CQ-1), 1587ebe (lock), e34359a (CQ-2), f14cf3c (theme) | Match `git log`. |
| `QuireHost.scale` removed; `prepare` uses `cx.scale()` | Yes (`kind.rs:356`). No other reader in quire (grep). Cradle/svelters-gallery/cadatomic usage: *unverified* (no access); the report itself says cadatomic was not checked. |
| A window scale change re-lays out without a hook (skelter-core marks the subtree `PAINT`) | *Unverified* in skelter-core; the test `window_scale.rs` covers it end to end (`resize_window` then `moved == at_2`), so if check.sh passed, it holds. |
| `SkelterEditor::layout` honours a new theme | Yes: the shaper font, features and `row_h` are re-read from `self.theme` every layout (`editor.rs:435–457`), `content_key` hashes the theme, `laid` is rebuilt each layout, `sync_heights` keys on `row_h`/column. A live theme change (even font family/size) is therefore safe. |
| Only svelters entries changed in `Cargo.lock`, `svelters-abi` added | Yes. |
| Runs as egui_quire: one per visual row, ≤ 255 chars, trailing `\n` width 0 except on the last line, injected pieces skipped, char lengths/positions/widths/word starts | Yes, line by line against `egui_quire/src/a11y.rs`. Word-start logic identical. `word_starts` as `u8` is safe (≤ 254). |
| "an empty run for an empty line" (`a11y.rs` module doc, report) | **Imprecise**: an empty line that is not the last gets a `"\n"` run (the test expects exactly that); only an empty *last* line gets an empty run. Doc nit, see D1. |
| Selection: ends off the page clamp to the page's first/last position | Yes (`pos_of`). This **differs from egui_quire**, which sets no `text_selection` when an end is off the page. Not listed among the differences in `docs/SKELTER-QUIRE.md`; see Q1. |
| Selection column counted in shown chars via `to_display` | Yes; more correct than egui_quire under a projection (egui_quire counts document chars against runs that hold display chars). |
| Previous/next linked only within a visual row | Yes (`kind.rs`, `r.chunk > 0` / `r.chunk + 1 < r.chunks`). Documented. |
| `SetTextSelection` not handled by egui_quire either | Yes (no `SetTextSelection` handler anywhere in `crates/`). |
| a11y re-send key (version, selection, scroll) checked after each event | Yes, but only in `Kind::event`; see R1. |
| Test counts 51 → 54 (skelter_quire), 609 → 613 (workspace), check.sh passed on f14cf3c | Arithmetic consistent (4 new tests: 1 + 2 + 1). Not re-run (see above). |
| Bounds "in the node's local space (points), as `code-editor` draws it" | The x/y match the recorded content (`text_left() - scroll.x`, `PAD_Y - scroll.y + y_of(line)`, as `scene.rs:203` and `kind.rs:385`). Whether skelter-core wants virtual children's bounds in the owner's local space or in window space is *unverified*; see R3. |

## Findings

Ranked most severe first. None was confirmed by running code (see above); each says how to confirm.

### R1 (plausible, medium): TextRuns go stale after a change that does not arrive as an event on this node

`request_a11y` is called only at the end of `Kind::event`. These change the text or selection
without an `event` on the node:

1. **Platform IME through `TextInputClient`** (`replace_text_in_range`,
   `replace_and_mark_text_in_range`, `unmark_text`, `kind.rs:629–642`): a committed composition edits
   the document directly.
2. **A second `code-editor` on the same buffer** (split view): an edit in editor A changes the shared
   document's version; editor B only re-lays out when the host repaints, and nothing re-sends B's
   accessibility.
3. **Host-side edits**: `apply_remote` (collaboration), `SkelterEditor::set_find` driven from the
   host's find bar, a host command.

A screen reader then reads the old runs and value until the user next presses a key or clicks in
that editor. (Before the lane the same was true of the value, so this is a gap the lane narrowed,
not a regression.) Whether skelter-core already re-sends after a `TextInputClient` call is
*unverified*.

- **Test first** (`tests/a11y_runs.rs`): two `code-editor` nodes on buffer `main`; type into the first;
  `d.frame(...)`; assert the second node's first run in `c.a11y` reads the new text. And: call
  `replace_text_in_range` through `Document`'s text-input path; assert the update holds the new run.
- **Fix suggestion:** keep the key check, but run it where every path passes: in `prepare` (it runs
  on every record), requesting accessibility from there if `PrepareCx` can (or from skelter-core's
  equivalent), and in the `TextInputClient` methods. Recommendation: follow-up item for the next
  lane, not a merge blocker.

### R2 (plausible, low): a selection end inside a folded region on the page maps to the page's end

`pos_of` finds no runs for a hidden line between page lines and returns `(last, len)`, the end of the
page, instead of the next visible position (egui_quire returns no selection there). Reachable when a
selection spans a fold that was collapsed after selecting. Test first: fold lines 3–5 of a 10-line
document with a selection anchored on line 4 and the head on line 8; assert the anchor maps to the
run of line 3 (the fold header) or line 6, not the last run. Fix: when the line is within
`first.line..=last.line` but has no runs, map to the first run whose line is greater (char 0).

### R3 (unverified): coordinate space of the virtual children's bounds

The runs' bounds are in the editor node's local space. egui_quire converts to global. AccessKit
reads a node's `bounds` in its parent's coordinate space unless the node has a `transform`; if
skelter-core gives the editor node a local→window `transform` this is right, otherwise the runs are
offset by the editor's position in the window. The test only checks `b2.y0 > b0.y0`, which holds
either way. Check: in svelters, how `default_a11y` sets the node's bounds/transform and what
skelter-widgets' text field passes for its virtual children; add an assertion that a run's bounds
fall inside the editor node's bounds (as the update gives them) with the editor placed at a non-zero
offset in the window.

### D1 (doc nit)

`a11y.rs` module doc and the report: "an empty line has one empty run" → "an empty last line has one
empty run; any other empty line has a `"\n"` run".

### P1 (performance note, no action)

Every re-send (each keystroke, each wheel tick: scroll is in the key) calls `a11y_runs`, which
clones each page line's `LaidLine` and lays out page lines outside the viewport (when the user has
scrolled away from the caret), plus two more `laid_or_layout` calls for the selection ends. The
shaping caches should absorb it; measure if E9 timings matter.

## Tests: would each fail if the behaviour broke?

| Test | Assessment |
|---|---|
| `window_scale::an_editor_lays_out_at_its_windows_scale` | Yes: scale-1 and scale-2 origins differ only if the window's scale reaches shaping; the `resize_window` half fails if a scale change does not re-lay out. Good. |
| `theme::a_theme_set_on_the_host_reaches_an_existing_editor` | Yes: fails without the `prepare` copy. Relies on "the first solid quad is the background", which is true of `scene.rs` today. |
| `a11y_runs::the_text_is_exposed_…` | Yes for: run values, `\n` handling, char arrays, word starts, selection node/index, re-send after an edit (lane's mutant). |
| `a11y_runs::a_long_document_exposes_a_page_…` | Only that the first page is 50 runs. |

Gaps (a mutant that breaks each still passes every test):

- **T1** page shift: a page that never moves passes. Move the caret to line 120 (Ctrl+End or a click
  after scrolling) and assert the first run reads `line 100\n`.
- **T2** selection-only and scroll-only re-send: a key holding only the version passes (the test's
  click and typing land in one update). Assert an update after a click alone, and after a wheel alone.
- **T3** chunking: no line over 255 chars; assert a 600-char line gives runs of 255/255/91 (+`\n`)
  linked previous/next, and a caret at char 300 maps to (run 1, 45).
- **T4** soft wrap: no wrapped line; assert one run per visual row and the unlinked row boundary.
- **T5** injected text: no inlay hint/preedit; assert a hint's text is absent from the run.
- **T6** read-only: `SetValue`/`ReplaceSelectedText` advertised only when editable.
- Plus R1–R3's tests above.

## Approvals (quire `CLAUDE.md` "Questions and approvals")

The lane report says "nothing differs from VS Code", which is true, but quire's rule also covers
behaviour that differs from **`ARCH-QUIRE.md` §3**, and CQ-2 does. That was not raised.

**Q1 (needs the user's approval, via main-cadatomic, before master):** `skelter_quire`'s accessibility
differs from ARCH-QUIRE §3.8 in four points: (a) no announcement of the line when the page shifts
(§3.8: "the line number is announced"); (b) the page is always 50 lines (§3.8: 500 with a screen
reader); (c) previous/next links only within a visual row (egui_quire links a whole line's runs);
(d) a selection end off the page is clamped to the page's first/last position (egui_quire sets no
selection). Before the lane `skelter_quire` had no runs at all (already a documented difference in
`docs/SKELTER-QUIRE.md`), so this narrows the gap.
**Recommendation: approve (a)–(d) as documented interim differences for the skelter view**, since
skelter has no screen-reader flag and `skelter_quire` no localised strings; add (d) to the
differences list in `docs/SKELTER-QUIRE.md`; track (a) and (b) as follow-ups for when skelter
reports a screen reader and quire's skelter view gets strings. (c) is arguably the more correct
reading of AccessKit's `next_on_line`; keep it.

**Q1 answered (user, 2026-11-06, "Quire accessibility approved in the interim", relayed by the
Cradle batch coordinator):** (a)–(d) approved as documented interim differences; listed in
`docs/SKELTER-QUIRE.md` with (a) and (b) as follow-ups.

**Coordinator decides (no user approval):**

- `QuireHost.scale` removed (public field). Recommendation: accept, as the lane says; the
  coordinator should confirm cadatomic does not set it (not checked by the lane or this review).
- New public items `skelter_quire::a11y::{TextRun, TextRuns, RunPosition, PAGE_LINES,
  MAX_RUN_CHARS}`, `SkelterEditor::a11y_runs`. Recommendation: accept.
- `QuireHost.theme` is now live (with `Document::repaint_all`). Matches VS Code (themes apply live);
  not a command. Recommendation: accept.

## Merge note for the coordinator

`Cargo.lock` pins svelters `7e3c06d2` under `branch=master`. Cargo fetches the sha directly, so it
builds as long as that commit exists on GitHub, but if `7e3c06d2` is a batch-10 svelters branch
commit not yet on svelters master, merge svelters first (or a later `cargo update -p skelter-core`
on quire master would move the pin to a master that lacks `EventCx::request_a11y`). Not checked
here (no svelters access).

## Blockers for this review

- `add_repo akiselev/cradle` (read): refused ("[Access Added As A Means]").
- `add_repo akiselev/svelters` (read): refused the same way; without it nothing in `skelter_quire`
  builds, so no test, mutant or `scripts/check.sh` was run, and no regression test was written.
