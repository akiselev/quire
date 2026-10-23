# Handoff: p1-hover (P1 part 2: hover and signature help)

Written 2026-10-22. VS Code reference: microsoft/vscode `bdce63db5876`.

## Done

- **quire-core `hover`** (translated from hoverOperation, contentHoverWidgetWrapper,
  computeHoverPositions, hoverUtils and the marker and markdown participants):
  - `HoverState`, one per view. The delayed operation runs on the host clock: the providers are
    asked at delay/2, diagnostics are added and the hover shown at the full delay, and
    "Loading..." is added at 3×delay.
  - Anchor reuse and filtering, sticky "mouse getting closer", `insistOnKeepingHoverVisible`.
  - Parts are shown in order: diagnostics, then Markdown (by provider order), then Loading.
  - `HoverConfig` (`editor.hover.*`), `should_show_hover`, `is_trigger_modifier_pressed`,
    `is_position_within`, `compute_hover_positions`.
- **quire-core `parameter_hints`** (translated from ParameterHintsModel and
  getParameterLabelOffsets):
  - `ParameterHintsState`, with a throttled 120 ms delayer on the host clock.
  - Trigger contexts are merged as VS Code does. A retrigger carries the previously active help.
  - next/previous honour `cycle`. Content and cursor changes retrigger, the mouse (`setSelection`)
    and blur cancel.
  - Data types follow Monaco: `SignatureHelp`, `SignatureInformation`, `ParameterInformation`
    (label text or byte offsets), `Documentation` (plain or Markdown), `SignatureHelpSource`
    (trigger and retrigger characters).
- **Host seam**:
  - The host declares providers with `Document::set_hover_sources` and
    `Document::set_signature_help_sources`.
  - Requests come out as `EditorEvent::HoverAt { id, pos, version }` and
    `EditorEvent::SignatureHelpRequested { id, pos, version, context }`.
  - The host answers with `EditorView::set_hover` and `set_signature_help`. An answer is mapped
    forward, or refused as `AnswerRefusal::{Superseded, Stale}`.
  - Time comes in through `EditorView::tick(doc, now_ms)` and `next_deadline()`; commands also
    carry `time_ms`.
  - Markdown links emit `EditorEvent::HoverLinkActivated { href }` (coordinator-approved).
- **Commands** (VS Code ids, pre-approved):
  - hover: editor.action.showHover (Ctrl+K Ctrl+I), editor.action.hideHover, and
    scrollUp/Down/Left/Right, pageUp/Down and goToTop/Bottom Hover (when `editorHoverFocused`);
  - parameter hints: editor.action.triggerParameterHints (Ctrl+Shift+Space), closeParameterHints
    (Escape / Shift+Escape), showPrev/NextParameterHint (Up/Down, Alt+Up/Down, mac Ctrl+P/N).
- **egui_quire**:
  - `markdown` (a minimal renderer): paragraphs, headings, lists, quotes, rules, fenced and
    indented code (monospace, no colour), code spans, strong, emphasis, links and escapes.
  - `hover_widget`: placed above (or below) the anchor at showAtPosition, falling back to the
    secondary position. It has a max width of 500 and a scroll area, diagnostics with their
    source, code and action buttons, a copy button per part, and AccessKit `Tooltip`.
  - `parameter_hints_widget`: placed above (or below) the caret. The active parameter is
    highlighted and underlined. It shows the overload counter with ▲/▼, the parameter and
    signature docs, and AccessKit `Tooltip` with an assertive description "{param}, {docs}, hint".
  - The controller's mouse policy lives in editor.rs: the hiding-delay runner, the sticky hover,
    and hiding on mouse down, drag, scroll, mouse leave or any key that isn't a chord prefix or a
    hover command. A focused hover takes the keys: hover commands work, Escape hides it, and
    other keys are ignored.
  - The old diagnostics tooltip (`diagnostic_hover`) is gone. Diagnostics are now in the hover.
- **Keymap audit**: the script also reads hoverActionIds.ts, hoverActions.ts,
  provideSignatureHelp.ts and parameterHints.ts.
- **VSCODE-MAP**:
  - new rows in hand.json for contentHover, hoverCopyButton, hoverUtils and parameterHintsModel;
  - the 14 nested-snippet rows were relabelled "ours (queued: SnippetSession.merge)"
    (coordinator).

## Decisions (within VS Code behaviour; coordinator-approved where noted)

- `HoverAt` gained an `id` and is a request to the host's hover providers. It is emitted only
  when hover sources are declared, at delay/2 (150 ms). Previously it was emitted after 500 ms
  with no provider check.
- "Provider should not be retriggered if previous help is canceled first" is ours (async host
  provider; approved).
- No cancellation event goes to the host; late answers are refused (approved).
- Fenced code is monospace with no syntax colouring (approved).
- A selection set through `setSelection` (the widget's mouse) counts as VS Code's mouse source
  and cancels the parameter hints. `EditorView::set_selection` (host API) counts as "api" and
  retriggers.
- The anchor is the empty range at the nearest position under the mouse. The empty area after a
  line's end counts within half a character, as VS Code does.
- Diagnostics match with one character of slack, as marker decorations are `showIfCollapsed`.
- The hover's accessibility-mode autofocus (`accessibilitySupport`) is not modelled: showHover
  on a visible hover focuses it (FocusIfVisible), otherwise it shows it without focus.
- The hover does not hide on editor blur (VS Code shows hovers over unfocused editors). Parameter
  hints cancel on blur.
- The arrows in the parameter hints are painted triangles, because egui's default fonts have no
  arrow glyphs. The copy button is the text "Copy" (from EditorText).
- Not registered (not pre-approved, no consumer): editor.action.showDefinitionPreviewHover,
  increase/decreaseHoverVerbosityLevel.

## Next

- hoverCopyButton Enter/Space rows ("todo"): Tab focus on the parts of a focused hover.
- The hover's verbosity actions (if wanted; needs provider support in the Hover type).

## State at handoff (not landed)

- The branch `p1-hover` is pushed, and all touched tests pass. **The end-of-lane `scripts/check.sh` has not run:**
  free disk fell to 6.2 GB (other lanes' targets), below the 25 GB check.sh rule and the 15 GB build floor.
  To land: free the disk, `git fetch && git rebase origin/master`, run `scripts/check.sh` once, then
  `git push origin HEAD:master`.
- clippy `-D warnings` and fmt were clean on quire-core and egui_quire before the last small test edits; check.sh
  reruns both.
- **Open problem:** in kittest, a *pointer* click on the hover's link and copy buttons registers no click,
  while the hover stays open (the debug log shows it visible, with the pointer on it). A pointer click
  on a diagnostic action button in the hover does work (editor.rs `diagnostic_hover_shows_message_and_action`).
  The hints.rs tests click those buttons through AccessKit (`click_accesskit`). The difference is
  that hints.rs focuses the editor first. Check this in the playground before relying on mouse clicks on hover links.

## Test counts (filtered runs)

- quire-core: hover 40 (contentHover 2, hoverUtils 29, own 9), parameter_hints 20 (parameterHintsModel 10, own 10).
- egui_quire: hints 7 kittests, editor 61 (3 tests adjusted: the harness declares a hover source;
  idle test points at text; the diagnostic hover query), lib 16 (incl. 3 markdown unit tests), keymap_audit
  and suggest pass.
- Keymap audit: 96 ids, 95 match, 0 mismatch, 1 quire-only.
- VSCODE-MAP: 1236 listed, translated 715 (pass 674, ours 41, bug 0), N/A 469, todo 52 (was 1189 /
  673 / 633 / 40 / 466 / 50). New suites: contentHover 2 pass; hoverUtils 29 pass; hoverCopyButton 1 pass,
  3 N/A, 2 todo; parameterHintsModel 9 pass, 1 ours. Existing rows unchanged apart from the 14
  relabelled nested-snippet rows.
