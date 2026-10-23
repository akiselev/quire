# Handoff: p1-completion (P1 part 1: completion and snippets)

Written 2026-10-22 at the end of the lane. VS Code reference: microsoft/vscode `bdce63db5876`.

## Done

- **quire-core `completion`**
  - `filters`: fuzzyScore, the graceful variants, anyScore and createMatches. Literal translation over UTF-16 units.
  - `model`: the CompletionModel (incremental refilter, filterText, snippet sort) and the provider comparator.
  - `session`: SuggestModel's trigger, refilter, retrigger, cancel, isIncomplete and trigger characters, plus the list state (focus, scroll, details, resolve).
  - `snippet`: the snippet parser and marker tree (arena), transforms, FormatString.
  - `insert`: per-cursor insertion (overwriteBefore/After, adjustWhitespace, variables) and the snippet session (tab stops, mirrors, transforms on leave).
- **Host seam**
  - The host declares `Document::set_completion_sources`.
  - Requests come out through `EditorView::take_suggest_outputs`; egui_quire turns them into `EditorEvent::CompletionRequested`, `CompletionResolveRequested` and `CompletionCommand`.
  - The host answers with `EditorView::set_completions(doc, id, version, lists)`. An answer is mapped forward, or refused as Stale or Superseded.
  - Resolve answers go to `resolve_completion_item`.
- **Commands** (EditorCommand, VS Code ids):
  - suggest: triggerSuggest, acceptSelectedSuggestion, hideSuggestWidget, selectNext/Prev/NextPage/PrevPageSuggestion, toggleSuggestionDetails;
  - snippets (approved): jumpToNext/PrevSnippetPlaceholder, leaveSnippet.
- **egui_quire**
  - `suggest_widget`: a foreground Area under the caret, flipped above when it doesn't fit. It shows kind glyphs, match highlights, the label detail and description, and the inline detail. It also has the details panel, "Loading..." after 50 ms, "No suggestions.", click to accept (on press), and the wheel.
  - AccessKit: a ListBox named "Suggest" with ListBoxOption rows that carry `selected`.
  - Keymap `When` is now a conjunction of disjunctions with the suggest and snippet context keys. Commit characters are handled, and the list closes on blur.
- **Keymap audit**: the script reads suggestController.ts and snippetController2.ts and models `||` terms. Result: 83 ids, 82 match, 0 mismatch.
- **VSCODE-MAP**: hand-ported suites are listed in `tests/port/hand.json`, which `map.mjs` reads. `tests/vscode_hand_map.rs` checks that every listed test exists. The new status "todo" is kept outside the translated count.

## Next

- Port the 50 "todo" rows: snippetSession (transforms, typing at the final tabstop, nested placeholders), snippetController2 and snippetController2.old.
- If main-cadatomic approves keeping "replace": nothing. Otherwise implement VS Code's merge of nested snippets (SnippetSession.merge). Estimate: 0.5–1 lane-day.
- Choice placeholders (`${1|a,b|}`) insert their first option. VS Code also opens the suggest list with the choices; not done.
- The CLIPBOARD, time, random and workspace snippet variables are unresolved and insert their default.
- acceptAlternativeSelectedSuggestion (Shift+Enter/Tab) and focusSuggestion are not bound; they were not pre-approved.

## Decisions (within VS Code behaviour)

- Columns inside the model are byte columns on the request line. Only differences are used, as in VS Code.
- The word definition is runs of characters that are neither whitespace nor `wordSeparators`, so VS Code's number-with-dot pattern doesn't apply.
- Quick suggestions read the token type before the cursor for `{other, comments, strings}`.
- A retrigger keeps showing the old list until the answer arrives. Reused providers are not asked again.
- Focus goes to the preselected item if it is among the top-score items, else to the first. Refiltering resets focus, as VS Code does.
- Accepting is its own undo group (GroupHint::Isolated). Additional edits are applied in the same transaction; if they overlap, they are dropped.
- Undo ends snippet mode. Inserting a snippet during snippet mode replaces the session (pending approval).

## Test counts

- check.sh: 389 pass, 6 ignored (full run).
- New tests: suggest_filters 29, suggest_model 13, suggest_session 28, snippet_parser 55, snippet_session 16, vscode_hand_map 1, egui_quire suggest 5 kittests.
