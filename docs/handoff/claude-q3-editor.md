# Handoff: claude/q3-editor

Lane q3-editor (round 3 of the component polish; brief `docs/handoff/q3-editor-BRIEF.md`), three
items for cadatomic's gallery. Branch `claude/q3-editor` from master `ed0665b`. All three are done
and on the branch; the coordinator lands it (fast-forward) and moves the consumers' pins.

The evidence file (cadatomic `claude/gal2-components`, `docs/notes/gal2-requests/components.md`)
was not read: attaching `akiselev/cadatomic` was refused in this container. The work follows the
brief's own description of each item.

## What landed

| Commit | Item | What |
|---|---|---|
| `40f098d` | Q-G2 | `CodeEditorKind::register_as` (skelter_quire) |
| `f6f1b32` | Q-P10 | Reveal keeps the last line clear of the horizontal scrollbar (skelter_quire and egui_quire) |
| `cb551df` | Q-P9 | quire-syntax `toml` feature, `Lang::Toml` |

### 1. `CodeEditorKind::register_as(reg, name)` (Q-G2)

- skelter-core's `KindFactory` is a plain `fn() -> Box<dyn Kind>`, so a kind can't capture the spec
  it was registered with. The kind now resolves its props and signals **by index** through the
  registry entry of its node (`cx.kind().prop_ids()[i]`, `signal_ids()[i]`). Index constants sit
  beside `PROPS`/`SIGNALS` in `kind.rs`. `SPEC.prop_named(..)` is no longer used at runtime.
- `register_as` registers a copy of `SPEC` with `name` (`KindSpec { name, ..SPEC }`, leaked once per
  call because the registry holds `&'static KindSpec`). `register_as(reg, "code-editor")` is
  `register`. `register(reg)` is `reg.register::<CodeEditorKind>()`, so existing callers are
  unchanged. A duplicate tag is skelter-core's `DuplicateKind` error.

### 2. Horizontal scrollbar over the last line (Q-P10)

- Reproduced first. In skelter_quire, the wheel case was already fine: `scrollBeyondLastLine` (always
  on in quire, VS Code's default) lets the last line scroll to the top. The overlap came from
  **reveal**: Ctrl+End or arrow down to the last line put it at the bottom edge, under the bar.
  egui_quire had the same bug (caret row bottom at 392, bar top at 384).
- Fix: VS Code `ViewLines._computeScrollTopToRevealRange` adds the horizontal scrollbar's height to
  the bottom padding of a minimal (`Simple`) reveal ("Reveal one line more when the last line would
  be covered by the scrollbar"). Both widgets now do the same with `SCROLLBAR_W` (8 pt) while the
  bar is shown. Centred reveals are unchanged.
- VS Code also adds that padding when the bar is hidden. quire pads only while the bar is shown:
  otherwise a content-sized editor (egui_quire's `measure`, wrap on) scrolls by 8 pt on Ctrl+End.
  Two wrap tests caught this. See question 1.
- skelter_quire's horizontal bar condition moved into a public
  `SkelterEditor::horizontal_scrollbar_shown()`, shared by the overlay and `reveal`.

### 3. TOML (Q-P9)

- `tree-sitter-toml-ng` 0.7.0 (MIT, grammar ABI 14, `tree-sitter-language` 0.1) builds with the
  workspace's tree-sitter 0.26.13. It sits behind the optional `toml` feature, which is **not** a
  default.
- `Lang::Toml` (id and name `toml`, extension `toml`), in `Lang::ALL` (now 6; `CONFIGS` is sized
  by `Lang::ALL.len()`).
- Queries in `queries/toml/`:
  - `highlights.scm`: the grammar's query, header kept. Changed for quire: the pair patterns
    capture the key, not the whole pair (with the whole pair captured, the inner
    `(bare_key) @type` won and keys showed as types); quoted pair keys are properties; escapes are
    `string.escape`.
  - `folds.scm`, `brackets.scm` and `indents.scm`: quire's own (tables, arrays, inline tables).
- Token mapping: table headers `type`, keys `property`, strings `string`, escapes `string.escape`,
  numbers `number`, booleans `boolean`, dates `string.special`, `=` `operator`, brackets
  `punctuation.bracket`, `.`/`,` `punctuation.delimiter`, comments `comment`.
- `THIRD_PARTY_NOTICES.md` has the table row and an MIT block. The published crate ships no
  licence file, so the copyright lines use the authors the crate's metadata names.
- README, the crate description and the `lib.rs` doc name TOML.

## Public API added

- `skelter_quire::kind::CodeEditorKind::register(&mut KindRegistry) -> Result<KindId, RegistryError>`
- `skelter_quire::kind::CodeEditorKind::register_as(&mut KindRegistry, &'static str) -> Result<KindId, RegistryError>`
- `skelter_quire::editor::SkelterEditor::horizontal_scrollbar_shown(&self) -> bool`
- `quire_syntax::Lang::Toml` (the `toml` feature). `Lang::ALL` is `[Lang; 6]`.

## Tests

New tests:
- skelter_quire `tests/kind.rs`:
  - `register_as_two_tags_take_their_props`: code-editor and gallery-source in one registry. Each
    renders its own buffer; `readonly` reaches only the gallery node; a duplicate tag is refused.
  - `register_as_beside_a_host_code_editor`: a host's own `code-editor` kind next to
    `gallery-source`.
- skelter_quire `tests/editor.rs` `the_last_line_scrolls_clear_of_the_horizontal_scrollbar`: 400
  long lines; wheel to the end, Ctrl+End and arrow down. In each case the last line's glyph box
  ends above the bar thumb; this failed before the fix.
- egui_quire `tests/editor.rs` `the_last_line_is_revealed_above_the_horizontal_scrollbar` (kittest):
  Ctrl+End, then wheel to the end; failed before the fix.
- quire-syntax, all `#[cfg(feature = "toml")]`:
  - `abi::toml_grammar_loads`
  - `highlight::toml_fixture` (`fixtures/sample.toml.tokens`)
  - `highlight::toml_spots`
  - `incremental::toml_edits` (seeds 14, 15)
  - `structure::toml_folds_brackets_and_indent`
  - `queries_compile` accepts `toml` as optional.

Filtered runs (all through `scripts/cargo.sh`):
- `test -p skelter_quire`: 26 unit, 17 editor, 3 kind.
- `test -p egui_quire --test editor --test wrap`: 62 and 20.
- `test -p quire-syntax --features toml`: 4 unit (4 ignored), abi 2, driver 6, highlight 12,
  incremental 7, injections 9, structure 9.
- `test -p quire-syntax` (default features, no toml): highlight 10, structure 8, abi 1.

`scripts/check.sh` on `cb551df` (the code; later commits are docs only): **all passed**. fmt,
clippy `-D warnings`, `cargo test --workspace --all-features` **601 pass, 0 fail, 6 ignored**,
wasm32 checks of quire-core and quire-text, and the dependency and egui `=0.37.0` guards. The
first run stopped at the wasm32 step because this container lacked the `wasm32-unknown-unknown`
target (`rustup target add` fixed it). The second, complete run is the one recorded.

## Questions (for the coordinator)

1. **Reveal padding when no bar is shown.** VS Code pads a minimal reveal by the horizontal
   scrollbar height whether or not the bar is visible. quire pads only while the bar is drawn,
   because a content-sized editor would otherwise scroll by 8 pt. Recommendation: keep quire's
   narrower rule. It matches VS Code wherever the bar can cover a line.
2. **`scrollBeyondLastLine: false`.** quire has no such option (always on). If the gallery wants
   it off, VS Code then adds the horizontal bar's height to the scroll height (`ViewLayout`
   `_getContentHeight`), so the last line still clears the bar. Recommendation: add
   `EditorOptions::scroll_beyond_last_line` (default true) with that rule only when a consumer
   asks.
3. **The evidence file** wasn't readable here. If its Q-P10 case is not reveal (Ctrl+End, arrow down
   or typing at the end), send it, and the case gets a test. Recommendation: check it against
   the new tests before moving the pins.

## Next

Nothing open in this lane. After the coordinator lands it, cadatomic can call
`CodeEditorKind::register_as(reg, "gallery-source")` and turn on `quire-syntax/toml`.

## Coordinator answers (2026-10-28)

Landed at `388670c`. (1) and (2) accepted as recommended. (3): cadatomic's P10 came from a static
shot. VS Code's bars overlay the text too, so a partly covered bottom line at rest matches VS Code,
and the reveal case was the real bug.

## Follow-up: ligatures and the ASCII fast path (Q-E1)

Asked by the coordinator after landing (found by cadatomic gal3). The coordinator asked for a new
branch, `claude/q3-features`. This session may push only `claude/q3-editor`, so the work is on that
branch, on top of the landed `388670c`, which it fast-forwards from.

| Commit | What |
|---|---|
| `4431a18` | Ligature-safe `MonoTable::probe`; `EditorTheme::font_features`; JetBrains Mono test font |
| `76d7302` | fmt of the new test |

- **The bug.** `MonoTable::probe` shaped the 95 printable characters as one string. With
  JetBrains Mono's default features, `calt` rewrites `<=>`, `|}~` and similar sequences into
  ligature pieces of the same advance, so the probe accepted them. Every `<`, `=`, `>` then drew
  as a piece of `<=>`. The new test shows this: shaped together, the 95 glyphs differ from the
  glyphs shaped alone.
- **The fix.**
  - Table glyphs now come from each character **shaped alone**.
  - The probe refuses the table (`None`) when shaping the 95 together, or any of
    `shape::LIGATURE_PROBES`, changes a glyph or a column. `LIGATURE_PROBES` holds about 60 common
    code-font ligature sequences (`->`, `<=`, `!=`, `www`, `0x0`, ...).
  - A ligature the probes miss draws the plain glyph on the fast path, never a ligature piece.
- **`EditorTheme::font_features`** (CSS `font-feature-settings`) reaches the general path's style
  and the probe, through the new `LineShaper::set_features`. The default is
  `skelter_quire::LIGATURES_OFF` = `"liga" 0, "calt" 0`, which is what VS Code applies for
  `editor.fontLigatures: false`, its default. An empty string keeps the font's own features, so
  ligatures are on.
- **Departure from the ask:** with ligatures on, a font that ligates ASCII leaves the fast path for
  **every** line, not only for lines that contain a ligature. Finding out whether a line contains a
  ligature takes shaping it, which is the general path's work. Lines are cached either way. See
  question 4.
- **Test font:** `crates/skelter_quire/tests/fonts/JetBrainsMono-Regular.subset.ttf` (43 KB, ASCII
  with `calt`/`liga`, from JetBrains Mono 2.304, OFL-1.1) and its licence text beside it. The notice
  is in `THIRD_PARTY_NOTICES.md` under "Test fonts". It is used by tests only.

Public API added: `EditorTheme::font_features: String`, `skelter_quire::LIGATURES_OFF` (also
`theme::LIGATURES_OFF`), `shape::LIGATURE_PROBES`, `LineShaper::set_features(&str)`,
`LineShaper::features()`. Adding a field to `EditorTheme` breaks hosts that build it with a struct
literal and no `..Default::default()`.

Tests: `skelter_quire/tests/shape.rs` (new):
- `ligatures_off_shape_one_glyph_per_ascii_character_on_the_fast_path`: each table glyph equals the
  character shaped alone; a line full of would-be ligatures is fast, one glyph per character.
- `ligatures_on_refuse_the_table_and_draw_the_ligature`: the old probe's input already holds
  ligature pieces; the new probe refuses; `x <= y` takes the general path and draws the `<=`
  ligature with caret stops per character; switching to `LIGATURES_OFF` brings the fast path back.

`scripts/cargo.sh test -p skelter_quire`: 26 unit, 17 editor, 3 kind, 2 shape.

`scripts/check.sh` on `76d7302`: **all passed**, **603 pass, 0 fail, 6 ignored**.

4. **Fast path with ligatures on.** Recommendation: keep "a ligating font has no fast path while
   ligatures are on". The cheaper alternative is to guess ligature lines from character pairs, and
   a wrong guess brings back the ligature-piece bug. VS Code's default (and quire's) is ligatures
   off, which keeps the fast path.
