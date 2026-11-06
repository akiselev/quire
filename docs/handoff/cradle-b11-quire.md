# Cradle batch 11, lane `quire`: handoff

Branch `cradle-b10-quire` (continued, not merged to quire master: the coordinator lands it; see
"check.sh"). Coordinator: Cradle batch coordinator `session_01EQ79tfL3cS9uzD95nyo993`. Input:
`docs/handoff/cradle-b10-quire-review.md`.

## Sessions and shas

- Session 1 (2026-11-06, `a4b5ec6`): no svelters access (`add_repo` refused twice); did D1 and
  difference (d) as docs only, left the code items blocked.
- Session 2 (2026-11-06, this record): svelters at `/home/user/svelters` (master `e4c50658`).
  Commits on top of `a4b5ec6`:
  - `e675f78` Q1 answered: the user approved differences (a)–(d) (user, 2026-11-06, "Quire
    accessibility approved in the interim"), recorded in `docs/SKELTER-QUIRE.md` and the review.
  - `7008e83` R2, tests T1–T6, R3.
  - `27ef782` R1 and item 5.
  - this handoff (the branch head after the push).

## How it was built

quire's `Cargo.lock` pins svelters `7e3c06d2` by git. Nothing was fetched: every cargo command ran
through `scripts/cargo.sh` with `--config <scratch>/local-svelters.toml`, a `[patch]` of
`skelter-base`, `skelter-core` and `skelter-text` to `/home/user/svelters/crates/*` (as Cradle's
root manifest does), plus `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` for disk. `git diff
7e3c06d2 e4c50658 -- crates/skelter-core crates/skelter-base crates/skelter-text
crates/svelters-style crates/skelter-anim crates/svelters-abi` is empty, so this is the code the
lock pins. The patch rewrote `Cargo.lock` locally; it was restored byte for byte and is not in any
commit. Nothing in svelters was changed.

## Items

| # | Item | Outcome | Test (failed first unless said) |
|---|---|---|---|
| 1 | R1, TextRuns stale after a change that is not an event on the node | **Fixed** | `a_second_editor_on_the_buffer_follows_an_edit_in_the_first` (failed: "B is sent again"); `a_host_side_edit_sends_the_runs_again` (its first assert shows the gap); the IME path needed no fix: `a_text_input_client_edit_sends_the_runs_again` passed at once, because skelter-core's `Document::with_text_input` marks the node `PAINT \| A11Y` (svelters `crates/skelter-core/src/input/dispatch.rs:780–789`) |
| 2 | R2, a selection end inside a fold maps to the page's end | **Fixed** | `a_selection_end_inside_a_fold_maps_to_the_next_visible_line` (failed with `(7, 2)`, the page's end; now `(4, 0)`, the start of line 6) |
| 3 | R3, coordinate space of the virtual children's bounds | **Declined (correct as is)** | `run_bounds_are_in_the_editors_own_space` (passes; guard). skelter-core gives a virtual child no transform and documents its bounds "in the node's own space" (`cx.rs` `A11yCx::add_virtual_child`, ~line 1069; "virtual children have no transform", `add_virtual_child_of`); the owner gets bounds `(0,0,w,h)` and a parent-relative transform (`a11y.rs:169–188`). The test puts the editor at a 50/60 px padding: the node's transform changes, the runs' bounds do not and lie inside the node's bounds |
| 4 | T1–T6 | **Added** (all pass; they guard behaviour that was right) | T1 `the_page_moves_by_whole_pages_with_the_caret`; T2 `a_click_alone_sends_the_runs_again`, `a_wheel_alone_sends_the_runs_again`; T3 `a_long_line_is_cut_into_runs_of_255_chars`, `the_chunks_of_a_row_are_linked_on_the_line`; T4 `a_wrapped_line_has_one_run_per_row`, `the_rows_of_a_wrapped_line_are_not_linked`; T5 `an_inlay_hint_is_not_in_the_runs`; T6 `a_read_only_editor_offers_no_edit_actions` |
| 5 | Host selection read/write for Cradle | **Fixed** (new API) | `the_host_reads_and_sets_an_editors_selection` (set, clamp, then an editor edit, the host's own undo and the saved selection restored; a mutant that skips applying the queued selection fails it) |
| – | Q1 (coordinator message) | **Recorded** | docs only (`e675f78`) |

All tests are in `crates/skelter_quire/tests/a11y_runs.rs`.

### R1: what changed

`QuireHost` keeps, per editor node, what it last showed (its document weakly, the version, the
selection), written in `prepare` and after each event (`flush`). New
`skelter_quire::kind::refresh_editors(&mut skelter_core::Document)`: the host calls it after a turn
(after a host edit, an undo, or edits in another editor); every editor whose document moved past
the version it showed, or whose selection the host set, re-records and re-sends its accessibility
next frame. skelter-core offers no per-node repaint to a kind or a host (only `repaint_all`,
`rerecord_all`, `request_a11y`, `document.rs:1509–1523, 1682`; `PrepareCx` has no
`request_a11y`, and `EventCx::set_native_on`/`emit_on` reach only the node's own subtree,
`cx.rs:823–838`), so it bumps a new hidden prop `revision` (`I64`, `ChangeClass::Paint`, which
marks `PAINT | A11Y`, `document.rs:1111`). The editor also stores its accessibility key when it
builds its node, so a later event that changes nothing does not send it again.

**Request to svelters (for the coordinator):** a public `Document::request_paint(node)` (marks
`PAINT | A11Y`) would replace the hidden prop; `refresh_editors` keeps its signature.

**For Cradle:** `cradle-shell`'s after-turn hook (`native/quire.rs` `deliver`) should call
`skelter_quire::kind::refresh_editors(d)`; until it does, Cradle commands that edit a shown buffer
(`QuireText::replace`, undo/redo) leave the editors' pixels and runs stale until their next event.

### Item 5: why the service shape

`QuireHost::selection(node) -> Option<Selection>` and `QuireHost::set_selection(node, Selection)`
(the coordinator-approved shape). skelter-core's typed access to a kind is shared only
(`Document::with_kind(node, |&dyn Kind|)`, `document.rs:491`, with `Kind::as_any`), no `&mut` and
no dirty mark, so a direct `CodeEditorKind` setter would need interior mutability plus a separate
repaint anyway. The queued selection is applied in `prepare` before layout (`EditorView::
set_selection` clamps to the text and rounds down to char boundaries; the caret is revealed); it
is a view change, not a transaction, so it is not reported back through `committed` as the
editor's own. `selection` returns a queued selection until the editor takes it. Host flow:
`host.set_selection(n, sel); refresh_editors(&mut d);`.

New public items (coordinator decides, all inside the approved design): `QuireHost::selection`,
`QuireHost::set_selection`, `kind::refresh_editors`, the hidden `revision` prop. `QuireHost` gained
private fields (it is built with `QuireHost::new` everywhere: svelters `skelter-gallery-app`,
Cradle `cradle-shell`; cadatomic's `cadatomic-ui/src/skelter/code_editor.rs` was grepped and
builds none by literal, but it still reads `QuireHost.scale`, removed in batch 10: it breaks when
cadatomic moves its quire pin, not now).

### Observation, not fixed

An IME commit through `TextInputClient` is noted in the editor's output but reaches
`QuireHost.committed`/`events` only at that node's next event (`flush` runs in `Kind::event`). Not
lost, delayed; `refresh_editors` could flush it if Cradle needs the receipt in the same turn.

## Tests run (exact commands, all from `/home/user/quire`, env as above)

- `scripts/cargo.sh test --config <scratch>/local-svelters.toml -p skelter_quire` on `27ef782`'s
  code: 27 + 17 + 17 + 4 + 2 + 1 + 1 = **69 passed**, 0 failed (54 before; 15 new in `a11y_runs`).
- Before the fixes: the same with `--test a11y_runs`: R1's and R2's tests failed as listed; mutants
  (no queued selection applied; `refresh_editors` marking nothing) failed the item-5 test and the
  three R1 tests.
- `scripts/cargo.sh clippy --config … -p skelter_quire --all-targets`: no warnings.
  `scripts/cargo.sh fmt -p skelter_quire`.

## check.sh

**Not run.** `df -h /home/user`: 1.4 GB free (quire's `CLAUDE.md` wants 25 GB; the container's
other checkouts' targets use the rest). Only skelter_quire changed; its full test suite and clippy
ran (above). quire-core, quire-text, quire-syntax and egui_quire were not built or tested (no
change there). The coordinator should run `scripts/check.sh` on a machine with room before
landing.

## Not done

- `scripts/check.sh` (disk, above).
- No build against svelters by git (identical code, see "How it was built").
- The svelters request (`Document::request_paint`) and the Cradle `deliver` call are for other
  lanes.
