# presto_quire: lane record (presto batch 3, lane QUIRE)

Date: 2026-10-25. Branch `claude/presto-quire` (from `master` 3e78dcd). Brief: svelters
`plans/cloud/presto-b3/LANE-quire.md` and `plans/cloud/presto-b1/BRIEF.md` (minimal building:
`cargo check` only; tests written, not run).

## What landed

A new crate `crates/presto_quire` (about 8,000 lines with tests): the quire editor view on presto. It
depends on `quire-text`, `quire-core` (path) and on svelters' `presto-base`, `presto-core` and
`presto-text` by git (`ci/presto-base`). No egui anywhere in its graph.

| Module | What |
|---|---|
| `shape.rs` | One logical line shaped per call: presto-text's `TextSystem` (whole line unwrapped, one colour-only span per token, paint id = token type + 1) or the **ASCII monospace fast path** (`MonoTable`: the 95 printable glyphs probed once per font and size; x = column × advance, tabs to stops, no glyph for space and tab, one run per paint id). Char-boundary x for hit testing and wrapping. |
| `cache.rs` | **Two-generation caches** (GPUI scheme plus a byte budget, entries of the last two frames survive): shaped lines keyed by (line-content hash, token-run hash, font-set generation, ppem bits, features/variations/tab size), the text-stack note's key; wrapped rows separately, keyed by (shape key, wrap width bits, indent). 16 MiB and 8 MiB budgets. |
| `wrap.rs` | Soft wrap: VS Code `monospaceLineBreaksComputer` (copied from egui_quire's translation, Microsoft notice kept) over the shaped line's advances in points; a row is a slice of the shaped line (rewrap never reshapes), glyphs split to rows by x; wrapping indent per `WrappingIndent`. |
| `lines.rs` | Display lines: the document line with **injected text** (the IME preedit at the primary caret, inlay hints) spliced in, model ↔ display byte mapping, token paints (semantic tokens over the highlighter's), `stopRenderingLineAfter` prefixes copied from the rope's chunks. |
| `height.rs`, `widths.rs` | Copies of egui_quire's egui-free height index (Fenwick) and line-width table (incremental via `LineEdit`s, splices for soft wrap). |
| `visual.rs` | **quire-text's `VisualLines` over the wrapped rows**: Up/Down/PageUp/PageDown under soft wrap go through `quire_text::ops::motion::move_selection` (goal x in 1/64 pt; folded lines skipped). Home/End go to the row's edges. |
| `editor.rs` | `PrestoEditor`: layout of **only the viewport plus overscan** (20 rows by default), scroll (stored in the view as quire's anchor scroll; scrollBeyondLastLine), reveal, and input: keys through the bundled VS Code keymap with `when` contexts and two-chord sequences, typed text, clipboard keys, IME events, pointer (multi-click 1-2-3, drag selection with autoscroll, Shift+click, Alt+click cursors, Alt+Shift box selection, gutter line selection, fold toggles, Ctrl/Cmd+click links and inlay-hint commands/locations, double-click hint edits, secondary click), wheel, focus, timers (hover, parameter hints, semantic tokens), caret blink phases. Commands go through `Document::execute_as` (one mutation path). |
| `scene.rs` | Recording into four display lists, one per compositor space: background (local), **content** (content space: current line, find matches, selections, text runs, inlay badges, preedit underline, **diagnostics squiggles as wavy `Line`s**, secondary carets), **gutter** (y-scrolled: diagnostic marks, line numbers, fold chevrons), **overlay** (scrollbars; **suggest list, hover, parameter hints** as popups with clip). The primary caret is returned apart, for the compositor. |
| `layers.rs` | `EditorLayers`: the editor as four compositor layers (background `Content`, content `Scroll`, gutter `Scroll`, `Overlay`) with box/clip/scroll property nodes. **Scrolling is a compositor offset**: a scroll inside the recorded overscan re-records only the overlay and sends `PropOp::Scroll` / `FastCommit::ScrollOffset`; leaving it re-records. **The caret is a `FastCommit::Caret`** in window space. Run ids per editor (two tables), dropped runs listed per commit. |
| `ime.rs` | **`TextInputClient`** (`ImeTarget`, `ImeClient`) over a virtual text (the document with the preedit spliced in at the caret), **document-wide UTF-16 offsets**; commit = core `compositionType`, reconversion ranges replaced as one IME input transaction; candidate bounds per range in window space. |
| `kind.rs` | `code-editor`, a presto-core `Kind` (props `buffer`, `readonly`, `gutter`; signals `change`, `selectionchange`; role `MultilineTextInput`; focusable). Documents and fonts come from a `QuireHost` service, which also collects the `EditorEvent` outbox. Implements `TextInputClient` itself (presto-core's `Kind::text_input`). AccessKit: name, value (documents up to 64 KiB), read-only. |
| `keymap.rs`, `chord.rs`, `command.rs` | egui_quire's keymap ported to presto's `Key` (egui-named, so `keybindings.json` notation and the bundled `crates/egui_quire/keymaps/vscode-default.json` are reused unchanged) and `Modifiers` (AltGr kept apart from Ctrl+Alt); an own `OperatingSystem` enum. |
| `theme.rs` | `EditorTheme` in presto colours (egui_quire's Dark+/Light+ token palette), font family/size/line height, caret blink timing. |

quire-text: `Text::len_utf16`, `Text::byte_to_utf16_offset`, `Text::utf16_offset_to_byte`
(document-wide UTF-16, both backends: O(log n) on the rope through ropey's `char_to_utf16_cu` /
`utf16_cu_to_char`, a scan on the small backend, which holds at most 64 KiB) with a unit test
(TEXT-INPUT's R1). And TEXT-INPUT's R2: `PreeditUpdate::Update` that starts a composition over a
non-empty selection first deletes the selection in one IME transaction (`Executed::applied` carries
it), as AppKit and TSF do; later updates change only the preedit (test
`a_preedit_over_a_selection_replaces_it` in `tests/rules.rs`, not run). This changes what a field
sees on a composition start, so it is listed for approval with question 3. No other change outside the new crate except
the workspace `Cargo.toml` (member, presto git dependencies) and `THIRD_PARTY_NOTICES.md` (the wrap
translation in presto_quire).

**Host events**: every `egui_quire::EditorEvent` variant is in `presto_quire::EditorEvent` with the
same fields, plus four egui handled inside egui: `CopyText { text }`, `PasteRequested`,
`FindWidgetRequested { replace, seed }`, `FindWidgetClosed` (see question 3). `HoverLinkActivated` and
`DiagnosticAction` are defined but not emitted yet (the hover draws no clickable content).

## What I ran

- `cargo check -p presto_quire` and `cargo check -p presto_quire -p quire-text --tests`: clean, no
  warnings; `cargo clippy -p presto_quire`: no warnings; `cargo fmt`.
- `scripts/cargo.sh` was not used: its slot script (`../svelters/tools/dev/cargo-slot`) is not in
  this cloud container; cargo ran directly, niced, `-j 3`, `CARGO_PROFILE_DEV_DEBUG=0`.
- **No test was run** (batch rule). Written: 26 unit tests (cache 3, shape 6, wrap 5, lines 3,
  chord 2, height 4, widths 3) + 1 quire-text unit test (`document_utf16_offsets`) + 16 integration
  tests (`tests/editor.rs` 15: viewport-only layout, typing, Ctrl/Cmd chords, Save on macOS, clipboard
  events, click and double-click, wheel clamping, record contents, wavy diagnostics, wrapped Down,
  caret blink, IME preedit and commit, the UTF-16 client over the virtual text, layer scroll by
  offset vs re-record; `tests/kind.rs` 1: the kind in a presto-core document records, focuses on a
  click, types, reports). quire has no hosted CI; nothing has executed them yet.
- `scripts/check.sh` was not run (it builds and tests the workspace; the batch says check only).

## Not done (honest gaps)

- **Find bar**: egui_quire draws its own (two `TextEdit`s). presto has no text field kind in this
  batch, so the editor emits `FindWidgetRequested` and the host drives `PrestoEditor::set_find`.
- **AccessKit `TextRun` children** (egui_quire's page of lines around the caret): a kind projects one
  AccessKit node; the editor exposes name, value (≤ 64 KiB) and read-only only.
- **Hover**: plain text (part contents line by line, no Markdown rendering, no links, no focus/scroll
  commands drawn); the mouse side is simplified (a delayed hover at the position under the pointer,
  no "getting closer" or sticky logic). Inlay-hint tooltips are not drawn.
- **Suggest details pane** and the suggest list's own scrollbar; parameter-hints arrows.
- **Under soft wrap**, `InsertCursorAbove/Below`, keyboard column selection and Alt+Shift box
  selection go by logical lines (core's commands), not visual rows as egui_quire does; the upstream
  caret after a wrapped vertical move is not recorded (it can draw at the next row's start).
- **Middle-click PRIMARY paste**: presto's input has no PRIMARY payload; a middle press is ignored.
- **Bidi**: rows split glyphs by x, which is right for LTR; an RTL run inside a wrapped line keeps
  its glyphs on the row their origin falls in.
- **Char-boundary x on the general path** come from `TextLayout::caret_rect` per char (linear per
  query, so quadratic in the line); lines over 4096 bytes spread each run's advance over its chars
  instead (exact for monospace, an estimate otherwise). See question 2.
- The `code-editor` kind **re-records on scroll** (a kind records one list per node): see question 1.

## Questions for the coordinator (each with a recommendation)

1. **A kind that owns compositor layers.** presto-core gives a kind one display list per node and
   makes only CSS `overflow` containers scroll layers, whose *children* scroll. The editor needs
   content and gutter on separate scroll nodes (x+y and y only) and the caret as a fast commit, from
   one node. *Recommendation:* add to presto-core a `Kind::layers(&self, cx: &mut LayersCx)` hook (or
   let `RecordCx` record into named sub-lists with their own transform/scroll/clip nodes, plus
   `EventCx::fast(FastCommit)`), then make `code-editor` use `presto_quire::layers` (its structure is
   already that). Until then the kind is correct but scroll costs a re-record of one node.
2. **presto-text cluster geometry.** `TextLayout`'s clusters are private; the editor needs every
   cluster's x once per shaped line. *Recommendation:* add `TextLayout::clusters() -> impl Iterator<Item
   = (Range<usize>, f32, f32, bool)>` (range, x0, x1, rtl) in the TEXT lane; `shape.rs` then drops the
   per-char caret queries and the long-line estimate.
3. **New host events** (`CopyText`, `PasteRequested`, `FindWidgetRequested`, `FindWidgetClosed`): egui
   gave the widget a clipboard and a text field; presto's kinds have neither. *Recommendation:* keep
   them as host events (the host owns `PlatformCx::clipboard()`); this is behaviour that differs from
   egui_quire's surface, so it needs main-cadatomic's approval before quire `master`.
4. **Shared egui-free modules.** `height.rs`, `widths.rs`, the wrap break algorithm, the keymap and
   chord parser now exist in egui_quire and presto_quire. *Recommendation:* after consolidation move
   them into one egui-free crate (`quire-view`?) both renderers use, so the keymap audit covers both.
5. **Git dependency on svelters.** quire's CLAUDE.md says "no dependency on svelters"; the lane brief
   asks for the presto crates by git. *Recommendation:* accept it for `presto_quire` only (the other
   crates stay svelters-free), and switch to crates.io or a path dependency when presto is published
   or vendored. The dependency pulls `svelters-style` through `presto-core`.

## Next

1. Run the tests (`scripts/check.sh` or `cargo test -p presto_quire -p quire-text`) and fix what fails.
2. Questions 1 and 2, then move `code-editor` onto `layers.rs`.
3. The gaps above in order of use: wrapped InsertCursor/column selection, hover Markdown and focus,
   suggest details, a11y text runs, find bar once presto has a text field.
