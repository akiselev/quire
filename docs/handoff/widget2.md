# Handoff: widget2 (QE1 gaps, `egui_quire`)

## Branches
- On master: `ffcf3b5` (horizontal extent), `d3407bf` (PRIMARY), `78cc4b5` and `c07ef2f` (snapshots),
  and `86d5aed` (multi-cursor kittest). Each passed the full `scripts/check.sh`.
- On branch `widget2` (pushed to origin) and awaiting coordinator approval, because they add
  public API:
  - A, tab stops (`18c76a0`, `5cada0c`): new fields `Mono.tab_px` and `LineShaper.tab_size`,
    and `LineLayout::without_fast_path()`. `LineLayout` now has a private field.
  - B, two-chord bindings (`b628a08`, `3f65106`): `KeySeq`, `Binding.{linux,win,mac}:
    Option<KeySeq>`, `Binding::keys`, `Keymap::keys_for`, `Keymap::lookup_seq`, `KeyLookup`.
  - D, page-shift announcement (`fcac227`): `EditorText.line_announcement`.
- `widget2` also carries a copy of the multi-cursor test. When rebasing onto master, drop the
  duplicate. The snapshots on `widget2` are recorded with tab stops; master's are recorded
  without them.

## Done (all items in the widget2 brief)
1. Tabs go to tab stops (`EditorConfig::tab_size`). Each tab is its own `LayoutJob` section,
   and the text after it gets `leading_space` (the change from egui's fixed 4-space tab to the
   stop).
   - ASCII lines compute the stops from columns.
   - Other lines are laid out twice: once to find where each tab starts, then with the
     corrections.
   - Caret, hit-testing (`LineLayout::char_xs`/`col_at`), a11y run widths, preedit lines and
     trailing-tab width all follow.
2. Horizontal extent comes from `widths::LineWidths`: visible columns per line, kept up to date
   from `doc.recent().since(v)` `LineEdit`s.
   - Only edited lines are re-measured.
   - The maximum is rescanned from the u32 table only when the longest line shrinks.
   - A full measure happens on first use, on a tab-size change, or when the table is stale.
   - The galley widths seen since the last edit cover wide glyphs.
3. Two-chord bindings.
   - The pending chord is private (`Mem::pending_chord`) and waits indefinitely, as in VS Code
     (the coordinator withdrew the 3 s timeout).
   - Escape or a non-matching key cancels the chord and is consumed. Focus loss cancels it.
   - A prefix wins over a one-chord binding on the same keys.
   - The default keymap gains `ctrl+k ctrl+c` addCommentLine, `ctrl+k ctrl+u`
     removeCommentLine and mac `ctrl+k` deleteAllRight. They stay unresolved until core adds
     them.
4. The height index stays in `egui_quire` (coordinator agreed): it has no second consumer.
5. Linux PRIMARY is supported by egui 0.37 (`OutputCommand::TextSelectionSettled`,
   `Event::MiddleClickPaste`, egui-winit clipboard.rs).
   - The focused view reports its selection once it settles and has changed.
   - A middle click pastes at the pointer.
6. Page shifts are announced by a `Role::Status` child with `Live::Polite`, whose label is
   `EditorText.line_announcement`.
7. Snapshots: `tests/snapshots.rs` has light and dark images.
   - The `wgpu` and `snapshot` features of egui_kittest are enabled through a macOS-only
     dev-dependency table, because other backends rasterise differently.
   - Update with `UPDATE_SNAPSHOTS=1`.
8. Playground: it runs (24 s, no log output). `screencapture` from this shell records only the
   wallpaper (no Screen Recording permission), so there is no image.
9. Multiple cursors and selections already draw from `Selection`. A kittest counts the caret
   and selection rects and types into all cursors.
   - Core has not landed insertCursor*, addSelectionToNextFindMatch, moveLines*, copyLines* or
     deleteAllLeft. The pinned-missing list is unchanged except for the three new chord
     commands.
10. Core's find state has no current-match index yet; it is still derived (`glue::find_matches`).

## Tests
`scripts/cargo.sh test -p egui_quire`:
- on `widget2`: 6 unit tests, 46 kittests (`tests/editor.rs`), 2 snapshot tests (macOS);
- on master: 6 unit tests, 37 kittests, 2 snapshot tests.

## Next
- When A, B and D are approved: rebase `widget2` onto master, drop the duplicate multi-cursor
  commit, re-record snapshots if they differ, run `check.sh`, then push.
- Bind the core commands when they land (resolution is automatic through
  `EditorCommand::from_vscode_id` or serde), then shrink the list in
  `keymap_resolves_against_core_commands`.
- Use core's current-match index if it adds one.
