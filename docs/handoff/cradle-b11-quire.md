# Cradle batch 11, lane `quire`: handoff

Session 2026-11-06. Branch `cradle-b10-quire` (continued, not merged; the merge waits for the
user's approval of the four ARCH-QUIRE §3.8 differences, review Q1). Coordinator: Cradle batch
coordinator `session_01EQ79tfL3cS9uzD95nyo993`. Input: `docs/handoff/cradle-b10-quire-review.md`.

## Shas

- Start: `8ed0913` (batch 10 review, on `origin/cradle-b10-quire`; the local checkout was at
  `313555d` and fast-forwarded).
- This lane: one commit on top of `8ed0913`, "QE4: Cradle batch 11 quire: D1, difference (d),
  handoff (code items blocked)" (its sha is the branch head after this push).

## Blocker (verbatim)

svelters is needed beside the checkout (`../svelters`, for `scripts/cargo.sh`'s slot script and
for the `skelter-*` git dependencies; `~/.cargo/git` is empty, so nothing is cached). Both
attempts were refused:

1. `add_repo { owner: "akiselev", repo: "svelters", access: "read" }`:
   "Permission for this action was denied by the Claude Code auto mode classifier. Reason:
   [Access Added As A Means]."
2. Retry `add_repo { owner: "akiselev", repo: "svelters" }`: the same message,
   "Permission for this action was denied by the Claude Code auto mode classifier. Reason:
   [Access Added As A Means]."

So `skelter_quire` (and the workspace, which resolves the svelters git dependencies) does not
build here: no test, no mutant run, no `scripts/check.sh`. Per the brief the lane stopped at what
can be done without building. Nothing in code changed except one doc comment.

## Items

| # | Item | Result |
|---|---|---|
| 1 | R1, TextRuns stale after a change that does not arrive as an event on this node | **Blocked** (no build) |
| 2 | R2, a selection end inside a folded region | **Blocked** (no build) |
| 3 | R3, virtual children's bounds coordinate space | **Blocked** (needs svelters' source to verify and a build to test) |
| 4 | Tests T1–T6 | **Blocked** (no build) |
| 5 | D1 | **Fixed** (doc only, no test applies): `a11y.rs` module doc and `cradle-b10-quire.md` now say "an empty last line has one empty run; any other empty line has a `"\n"` run" |
| 6 | Host-callable selection read/write on a `code-editor` node | **Blocked** (no build; also needs a coordinator decision on the signature, below) |
| 7 | (d) in the differences list of `docs/SKELTER-QUIRE.md` | **Fixed** (doc): the "AccessKit `TextRun` children" bullet now lists the clamp of a selection end off the page and notes the four differences await approval |

## Plans for the blocked items (for the next lane with svelters access)

Each: write the test, see it fail, then fix. All tests go in
`crates/skelter_quire/tests/a11y_runs.rs` unless stated (it already has `editor(text)`,
`pointer`, `typed` helpers and reads `TreeUpdate`s from the frame output).

**R1.** The key check (`Inner::a11y_key`, `kind.rs:153`) runs only at the end of `Kind::event`
(`kind.rs:486–496`). Tests: (a) two `code-editor` nodes on buffer `main`, type into the first,
frame, assert the second node's first run in the update reads the new text; (b) call the
document's text-input path (`TextInputClient::replace_text_in_range`, `kind.rs:629`) then frame,
assert the update holds the new run. Fix: move the key compare into a helper and call it from
`prepare` (which every editor of a repainted buffer runs) and from the three `TextInputClient`
mutators; request accessibility from `prepare` if `PrepareCx` can (check skelter-core for a
`request_a11y` on `PrepareCx`; if it has none, ask the coordinator for one, since skelter-core is
svelters' and must not be edited here). Note (a) also needs the host to repaint editor B after an
edit in A; check whether `Document::repaint_all` or the buffer's change already does.

**R2.** `pos_of` (`a11y.rs`, the `let Some(&k0) = ks.peek() else { return Some((last, …)) }`
branch) sends a hidden line between page lines to the page end. Test (unit, or through the kind):
10 lines, selection anchor on line 4, head on line 8, fold lines 3–5 (header line 3), assert the
anchor maps to the first run of line 6 at char 0 (or the end of line 3's run; pick one and
document it; the review suggests the next visible position). Fix: in that branch, when
`line <= last run's line`, return `(k, 0)` for the first run with `runs[k].line > line`; only past
the last line clamp to the end.

**R3.** Read in svelters how `A11yCx::default_a11y` sets the owner's bounds/transform and what
skelter-widgets' text field passes for its virtual children's bounds (owner-local or window).
The runs are owner-local today (`a11y.rs` `left`/`top`, no `to_window`). Test: place the editor at
a non-zero offset (e.g. a 50 px margin on the root box), assert a run's bounds lie inside the
editor node's bounds as the update states them (in the same space). Fix if wrong: offset each
`rect` by `Inner::to_window` (already kept, `kind.rs:135`) in `Kind::a11y`.

**T1–T6** (as the review lists): T1 move the caret to line 120 of a 200-line document, then the first run reads `line 100\n`
(page start `(120/50)*50`);
T2 an update after a click alone and after a wheel alone; T3 a 600-char line followed by another gives 601 chars with the `\n`, so runs of
255/255/91 linked previous/next, caret at
char 300 → (run 1, 45); T4 soft wrap: one run per visual row, no next/previous across rows;
T5 an inlay hint's text absent from the run; T6 `SetValue`/`ReplaceSelectedText` only when not
read-only (`kind.rs:520–525`).

**Item 6, selection read/write for the host.** Proposed (coordinator decides; small public API
inside the design): on `QuireHost`,
`pub fn selection(&self, node: NodeId) -> Option<quire_core::Selection>` and
`pub fn set_selection(&mut self, node: NodeId, sel: quire_core::Selection)`. Read: `flush`
(`kind.rs:253`) already runs after every event that can change the selection; it records
`(node, view.selection().clone())` into a `QuireHost` map (and `prepare` records it too, for
changes outside events). Write: `set_selection` queues it in a `QuireHost` map and calls for a
repaint; `prepare` takes the node's pending selection, clamps it to the document's current
version (`Selection` positions are valid only with their `Version`; clamp each end to
`len_bytes` and a char boundary), sets it on `Inner::view`, scrolls the caret into view, and the
R1 key check then re-sends the runs. Recommendation: this queue-through-the-service shape, since a
host has no handle to a kind instance; if skelter-core offers typed access to a node's kind
(`Document::kind_mut::<CodeEditorKind>(node)` or similar, unverified), a direct
`CodeEditorKind::{selection, set_selection}` is simpler. Test: set a selection from the host,
frame, assert `selection(node)` and the a11y text selection reflect it; then an edit, undo via the
host restoring the saved selection, assert the caret is back.

## Tests run

None (no build possible; see Blocker). Counts unchanged from batch 10: skelter_quire 54,
workspace 613 pass / 6 ignored (check.sh on `f14cf3c`, batch 10).

## check.sh

Not run: the code cannot build here, and this lane's change is docs only (one `//!` doc comment,
two docs), for which quire's CLAUDE.md says not to run it.

## Next

1. Get svelters beside the checkout (`../svelters` at `c033bf73`, svelters master), then the
   plans above in order R1, R2, R3, item 6, T1–T6; `scripts/check.sh` once at the end.
2. Q1 (user approval of differences (a)–(d)) is still open; do not merge before it.
