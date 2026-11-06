# Cradle batch 10, lane `quire-berth`: report

Session `session_01EQ79tgiJ75udjsdXHUtMSq`, 2026-11-06. Brief and lane file: Cradle
`docs/notes/batch10/BRIEF.md`, `docs/notes/batch10/LANE-quire-berth.md` (Cradle master 94b89e5).
Coordinator: Cradle batch coordinator `session_01EQ79tfL3cS9uzD95nyo993`.

- **quire:** branch `cradle-b10-quire` from master 7c0a3aa. Code sha f14cf3c; the final sha is the
  commit that records the check.sh result in this file. Not merged to master (the Cradle coordinator merges).
- **berth:** not started: `add_repo akiselev/berth` was refused twice (below). No berth branch exists.

## Items

| # | Item | State | Commit | Regression test (failed first) |
|---|---|---|---|---|
| 1 | quire CQ-1: `skelter_quire` lays out at the node's window scale | **Fixed** | 1ab681c | `skelter_quire/tests/window_scale.rs::an_editor_lays_out_at_its_windows_scale`. Before: glyph origins at scale 1 and scale 2 equal (`[(50.026, 16.0), (22.872, 16.0)]` both). Also checks that a window's scale change (`resize_window`) re-lays out the editor as one created at the new scale. |
| 2 | quire CQ-2: text-run accessibility children | **Fixed** | 1587ebe (lock), e34359a | `skelter_quire/tests/a11y_runs.rs` (2 tests). Before: no `TextRun` children (`left: []`, `left: 0` vs 50). Mutant without `cx.request_a11y()` fails the post-edit half ("the editor's accessibility is sent again"). |
| 3 | quire: set-theme on an existing editor (Cradle adoption Q3) | **Fixed** | f14cf3c | `skelter_quire/tests/theme.rs::a_theme_set_on_the_host_reaches_an_existing_editor`. Before: background stayed `0.1176…` after the host set `(0.1, 0.2, 0.3)` and called `repaint_all`. |
| 4 | berth CB-1: build `skelter_berth` against svelters by path without a consumer-side `[patch]` | **Blocked** | none | `add_repo` refused twice (verbatim below); no berth checkout, so nothing was read or changed. |
| 5 | berth CB-2: a pane rename on `Layout` | **Blocked** | none | same blocker. |

### What changed

1. **CQ-1** (`crates/skelter_quire/src/kind.rs`): `CodeEditorKind::prepare` takes the scale from
   `NodeView::scale` (the node's window; 1 when detached) instead of `QuireHost.scale`. `prepare`
   runs on every record, and skelter-core marks a window's subtree `PAINT` when its scale changes
   (`Document::resize_window`), so a scale change re-lays out the editor without a
   `Kind::window_changed` hook. `QuireHost.scale` is **removed** (public field; see "for the
   coordinator").
2. **CQ-2** (new `crates/skelter_quire/src/a11y.rs`, `kind.rs`): `SkelterEditor::a11y_runs` builds
   `TextRun`s for a page of 50 visible lines around the caret, as egui_quire's `a11y.rs`: one run per
   visual row, cut at 255 chars, a trailing `\n` of width 0 on every line but the last, an empty run
   for an empty last line (any other empty line gets a `"\n"` run; review D1), shown text without injected pieces (inlay hints, preedit; a projection's
   replacement text is what is read), char lengths, positions, widths, word starts; the selection
   (anchor, focus) mapped onto the runs, an end off the page clamped to the page's first/last
   position. The kind adds them as skelter-core virtual children (`A11yCx::add_virtual_child`,
   at most `MAX_VIRTUAL_CHILDREN`), sets the node's `text_selection` to their virtual ids, adds
   `SetValue`/`ReplaceSelectedText` actions when editable (it already handled them), and calls
   `EventCx::request_a11y` after an event that changed the text, selection or scroll. The value
   (≤ 64 KiB) stays. `docs/SKELTER-QUIRE.md` updated.
3. **Theme** (`kind.rs`): `prepare` replaces the editor's theme with `QuireHost.theme` when they
   differ. A host that changes the theme calls `Document::repaint_all` (as for the widgets' theme).
   No new API; `QuireHost.theme`'s doc says so.

### Decisions taken

- **svelters in `Cargo.lock`: cd2adc4 → 7e3c06d2** (1587ebe), the batch's sibling revision, for
  `EventCx::request_a11y` (svelters 5b1b4d469). Cradle already builds skelter_quire against
  7e3c06d2. Only svelters git entries changed (and `svelters-abi` added); skelter_quire's 51
  existing tests passed unchanged on it.
- **Runs are linked previous/next on their visual row**, not across a wrapped line's rows (egui_quire
  links all of a line's runs). AccessKit's `next_on_line` names the visual line; skelter-widgets'
  text field links the same way.
- **No live-region announcement on a page shift** (egui_quire announces "line N" through
  `EditorText`): skelter_quire has no localised strings. **The page is always 50 lines** (egui_quire
  uses 500 with a screen reader; skelter has no screen-reader flag).
- **`SetTextSelection` is not handled** (egui_quire does not handle it either; skelter-widgets' text
  field does). A follow-up if Cradle wants it: map `(run, char)` back through `a11y_runs`.
- **The a11y re-send key** is (document version, selection, scroll); layout changes already re-send
  (skelter-core marks `A11Y` on a layout change). A scale change does not re-send (bounds are in
  points; only sub-pixel positions move).

### For the coordinator (quire's approval rules)

- **Signature change: `QuireHost.scale` removed** (CQ-1). Nothing reads it any more; leaving an
  unread public field would mislead. Consumers that set it stop compiling: Cradle
  (`crates/cradle-shell/src/native/quire.rs`) and svelters' gallery (`skelter-gallery-app`) use
  `QuireHost::new` and never set it (grep at Cradle 94b89e5, svelters 7e3c06d2). cadatomic was not
  checked (not in this container). Recommendation: accept; the alternative is a
  `#[deprecated]` field.
- **New public items:** `skelter_quire::a11y` (`TextRun`, `TextRuns`, `RunPosition`, `PAGE_LINES`,
  `MAX_RUN_CHARS`) and `SkelterEditor::a11y_runs`. Internal to the editor's a11y projection, inside
  the design (egui_quire parity). Recommendation: accept.
- **Behaviour:** the theme on `QuireHost` is live (item 3). Not a new command; the coordinator's
  adoption Q3 answer asked for it.
- No new user-facing command; nothing differs from VS Code.

### Questions

- None blocking. Cradle CR-7 can now drop its "waits for quire CQ-1" note once Cradle's quire pin
  moves to this branch's merge.

## Tests run

All in `/home/user/quire` through `scripts/cargo.sh` (cargo-slot from `/home/user/svelters`),
rustc 1.97.0, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_NET_GIT_FETCH_WITH_CLI=true`.

| Command | Result |
|---|---|
| `scripts/cargo.sh test -p skelter_quire --test window_scale` (before the fix) | 0 passed, 1 failed (expected) |
| `scripts/cargo.sh test -p skelter_quire --test window_scale --test kind` (after) | 1 + 4 passed |
| `scripts/cargo.sh test -p skelter_quire` on svelters 7e3c06d2 (before CQ-2) | 27 + 17 + 4 + 2 + 1 + 0 = 51 passed |
| `scripts/cargo.sh test -p skelter_quire --test a11y_runs` (src stashed) | 0 passed, 2 failed (expected) |
| same, with CQ-2 | 2 passed; mutant without `request_a11y`: 1 passed, 1 failed (expected) |
| `scripts/cargo.sh test -p skelter_quire --test theme` (src stashed / with fix) | failed (expected) / 1 passed |
| `scripts/cargo.sh test -p skelter_quire` (final) | 27 + 2 + 17 + 4 + 2 + 1 + 1 + 0 = 54 passed |
| `scripts/cargo.sh clippy -p skelter_quire --all-targets -- -D warnings` | clean |
| `scripts/cargo.sh fmt -p skelter_quire -- --check` | clean |
| `scripts/check.sh` (once, at the end, on f14cf3c) | **all passed**, exit 0: fmt, workspace clippy `-D warnings`, `cargo test --workspace --all-features` **613 passed / 0 failed / 6 ignored** (master's 609 + 4 new), wasm32 quire-core and quire-text, dependency checks, egui =0.37.0 |

## Blockers

`add_repo` for `akiselev/berth` (access `push`), twice, both refused with:

> Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Access Added
> As A Means]. If you have other tasks that don't depend on this action, continue working on those.

So CB-1 and CB-2 were not started: no berth checkout, no branch `cradle-b10-berth`, no berth report.
They need a session whose `add_repo akiselev/berth` is allowed.

## Setup problems

- `add_repo akiselev/svelters` (read) was not in the lane file but was needed: quire's skelter crates
  are git dependencies on svelters, and cargo's fetch failed with 401 before it. It was allowed.
- cargo needs `CARGO_NET_GIT_FETCH_WITH_CLI=true` here to fetch svelters through the git proxy.

## Carry-overs

- CB-1, CB-2 (berth): blocked, above.
- CQ-2 follow-ups if wanted: `SetTextSelection`, a page-shift announcement, a larger page under a
  screen reader once skelter reports one.
