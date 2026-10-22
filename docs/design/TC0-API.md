# TC0: `quire-text` public API, module map and split plan

Status: **approved** (main-cadatomic, cadatomic register `ae16d45`: all 13 open questions as
recommended, with the review notes folded in below). TC-EVAL results folded in (register
`20aa92e`; svelters `plans/research/TC-EVAL.md`, branch `research/tc-eval`, `d17b3da9`). Written
against quire master `056133c` and svelters ARCH-WIDGETS §4.1–§4.7 (`c5c34599`). Scope: `TC0.md`.
Approved amendments 1–7 of main-cadatomic's §4 sign-off are marked (A1)–(A7); TC-EVAL items (T1)–(T7).

**Sources from svelters** (svelters is MIT, same author: relicensing to MIT OR Apache-2.0 is fine,
svelters-agent). Each ported file carries one provenance line, `// ported from svelters <path> @
<sha>`:
- FX1 words: `crates/svelters-egui/src/native/words.rs` on svelters master (last changed in
  `4365d5a9`), its in-file tests, and the model-level FX1 pin-1 cases of
  `crates/svelters-egui/tests/native_focus.rs` (`fx1_gtk_focus_policy_and_words`: Pango's
  Ctrl+BackSpace on `snake_case` → `snake_`, Ctrl+Left → 0).
- FX3 undo: `crates/svelters-egui/src/native/undo.rs` on branch `ci/fx3` at `f120f7f3` (or svelters
  master if a salvage lane lands it there first; identical), its in-file tests, and the model-level
  parts of the `fx3_*` golden replays in `crates/svelters-egui/tests/native_edit.rs`
  (`fx3_undo_grouping`, including "a text set from outside starts a new history").

## 1. Names

| Placeholder (ARCH-WIDGETS) | Final | Why |
|---|---|---|
| `TextDoc` | `TextDocument` | parallels `quire_core::Document`, which wraps it; no clash in readers' heads |
| `TextView` | `TextView` | parallels `EditorView`, which wraps it |
| `FieldRules` | `EditRules` | per call, not per field (§2.5); quire-core uses it too |
| `DocRules` | `DocRules` | kept |

quire-core keeps `Document`, `EditorView`, `EditorCommand`, `Applied`, `Refusal`, `Executed`,
`HostRequest` as its own types (wrappers or mirrors). Every type that moves is re-exported by
quire-core under its current path (`quire_core::Text`, `quire_core::text::Text`,
`quire_core::words::WordClassifier`, …), so egui_quire and quire-syntax compile unchanged.

## 2. Public API of `quire-text`

Positions are UTF-8 byte offsets valid at a `Version`. No ropey type in the public API (A7). No
clock, threads, I/O, egui, regex, find or diff library in quire-text (T5).

### 2.1 Text and changes (moved from quire-core unchanged, plus §4.3's gaps)

- `Text`: backend-neutral text. **Backend (T1):** a copy-on-write `Arc<str>`/`String` below
  64 KiB, ropey 1.6 above, behind the unchanged `Text` API. A text is promoted to ropey once it
  passes 64 KiB and demoted only by `replace_all` to under 32 KiB (no flapping). Existing API kept (`len_bytes`, `len_chars`,
  `len_lines`, `line`, `line_range`, `slice`, `byte_to_line`, `line_to_byte`, `byte_to_char`,
  `char_to_byte`, `byte_to_utf16`, `utf16_to_byte`, `char_at`, `chars_at`, `chunks`, `from_file`,
  `line_ending`, BOM, …). Added: `as_str() -> Option<&str>` (Some when contiguous),
  `grapheme_next(b)`, `grapheme_prev(b)`.
- `TextSlice<'a>`, `LineEnding`, `normalize_line_endings`.
- `Version`, `ChangeSet`, `Edit`, `Operation`, `Bias`, `ChangeError`: as today (compose, invert,
  map, map_pos). `ChangeSet::diff` (imara-diff) is behind quire-text's off-by-default `diff`
  feature, which only quire-core enables (T5: field builds contain no diff; quire-core's tests
  call `ChangeSet::diff` as an inherent method, so a quire-core extension trait would change test
  files). `replace_all` without the feature uses a common-prefix/suffix trim, not a diff.
- `Selection`, `SelRange`: as today; multi-range with primary stays in the type. The goal column
  (sticky x) stays per range in `SelRange::goal_column`, in units the `VisualLines` implementor
  defines (§2.6).
- `RecentChanges`, `RecentChange`, `LineEdit`, `Stale`: as today.
- `Anchor`, `Snapshot`: moved from `document.rs`.

### 2.2 Transactions

- `Transaction<E = NoEffect>`: `base`, `changes`, `selection: Option<Selection>`,
  `effects: SmallVec<[E; 2]>`, `origin`, `author: AuthorId`, `group`, `time_ms`. Builders as today
  (`new`, `select`, `with_selection`, `with_origin`, `with_author`, `with_group`, `with_time`,
  `with_effect`). quire-core: `pub type Transaction = quire_text::Transaction<Effect>;` so every
  existing construction and match compiles as is.
- `NoEffect`: uninhabited; fields never carry effects.
- `AuthorId(pub u64)`, `AuthorId::USER = AuthorId(0)` (A1; moved as quire-core lands it).
- `Origin`: as today (`Input(InputKind)`, `Command(Cow<'static, str>)`, `Undo`, `Redo`, `Remote`,
  `Reload`).
- `InputKind`: as today, plus `DeletingWordLeft`, `DeletingWordRight`, `Cut` (non-coalescing, so
  Monaco grouping is unchanged; the GTK and Qt rule sets need them, FX3's `EditKind`).
- `GroupHint`: as today.

### 2.3 Document and view

```rust
pub struct TextDocument { /* text, version, history: Option<Box<History>>,
                              recent: Option<Box<RecentChanges>>, rules: Arc<DocRules> */ }
impl TextDocument {
    pub fn new(text: &str, rules: Arc<DocRules>) -> Self; // no history/ring allocation (§4.7.4, T2)
    pub fn from_text(text: Text, rules: Arc<DocRules>) -> Self;
    pub fn text(&self) -> &Text;  pub fn version(&self) -> Version;  pub fn snapshot(&self) -> Snapshot;
    pub fn rules(&self) -> &Arc<DocRules>;  pub fn set_rules(&mut self, r: Arc<DocRules>);
    pub fn history(&self) -> &History;  pub fn recent(&self) -> Option<&RecentChanges>;
    pub fn apply(&mut self, tr: Transaction, view: &mut TextView) -> Result<Applied, Refusal>;
    /// For wrappers with their own effects: applies text, selection and history, hands effects back.
    pub fn apply_with<E>(&mut self, tr: Transaction<E>, view: &mut TextView)
        -> Result<(Applied, SmallVec<[E; 2]>), Refusal>;
    pub fn apply_remote(&mut self, base: Version, changes: ChangeSet, origin: Origin,
                        author: AuthorId) -> Result<Applied, Refusal>;           // (A1)
    pub fn replace_all(&mut self, text: &str, mode: ExternalSet, view: Option<&mut TextView>) -> Applied;
    pub fn resolve(&self, view: &TextView, cmd: &EditCommand, rules: &EditRules,
                   layout: &dyn VisualLines) -> Resolved;                        // pure
    pub fn execute(&mut self, view: &mut TextView, cmd: EditCommand, rules: &EditRules,
                   layout: &dyn VisualLines, time_ms: Option<u64>) -> Executed;  // resolve + apply
    pub fn close_undo_group(&mut self) -> Option<UndoGroupClosed>;
    pub fn tick(&mut self, now_ms: u64) -> Option<UndoGroupClosed>;
    pub fn undo_mode(&self) -> UndoMode;  pub fn set_undo_mode(&mut self, m: UndoMode) -> Option<UndoGroupClosed>;
    pub fn can_undo(&self) -> bool;  pub fn can_redo(&self) -> bool;  pub fn has_open_group(&self) -> bool;
    pub fn anchor(&self, pos: usize, bias: Bias) -> Anchor;
    pub fn resolve_anchor(&self, a: &Anchor) -> Result<usize, Stale>;
}

pub struct TextView { /* selection, selection_version, synced_at, focused, preedit, upstream */ }
impl TextView {
    pub fn new(doc: &TextDocument) -> Self;               // allocation-free
    pub fn selection(&self) -> &Selection;  pub fn selection_version(&self) -> u64;
    pub fn synced_at(&self) -> Version;  pub fn is_behind(&self, doc: &TextDocument) -> bool;
    pub fn set_selection(&mut self, doc: &TextDocument, sel: Selection);   // clamps, syncs first
    pub fn sync(&mut self, doc: &TextDocument) -> SyncOutcome;
    /// Sync, calling `step` for each change mapped (or once with `Clamped`) so a wrapper maps its own state.
    pub fn sync_with(&mut self, doc: &TextDocument, step: impl FnMut(SyncStep<'_>)) -> SyncOutcome;
    pub fn map_through(&mut self, applied: &Applied) -> bool;
    pub fn focused(&self) -> bool;  pub fn set_focused(&mut self, f: bool) -> bool; // blur clears preedit
    pub fn preedit(&self) -> Option<&Preedit>;  pub fn set_preedit(&mut self, p: Option<Preedit>);
    pub fn composing(&self) -> bool;
    pub fn upstream(&self) -> &[usize];   // heads drawn at the end of a wrapped row (affinity)
}
```

- `Applied`: `prev_version`, `version`, `changes`, `origin`, `author` (A1), `selection_changed`,
  `closed: SmallVec<[UndoGroupClosed; 1]>`; `changed_text()`.
- `Refusal`: `Stale`, `LengthMismatch`, `NotCharBoundary`, `CarriageReturn`, `ReadOnly`, `Rebase`,
  `LineBreak` (single-line, §2.5), `MaxChars`.
- `SyncOutcome` (`UpToDate | Mapped | Clamped`), `SyncStep<'a>` (`Mapped(&ChangeSet) | Clamped(&Text)`).
- `Preedit`: `text`, `active: Option<Range<usize>>`, `kind: PreeditKind` (`Ime | Hex`).
- `ExternalSet`: `ResetHistory` (GTK `set_text`, Qt `setText`: FX3), `Foreign` (quire's reload:
  diff applied as `Origin::Reload`, stacks rebased), `Undoable` (an app-level revert: its own undo
  group).

### 2.4 History

- `History` (allocated on the first edit, T2): facade over the rule set: `rules()`, `mode()`, `can_undo`, `can_redo`,
  `has_open_group`, `last_input_kind`, `open_group_since`, `peek_undo`, `peek_redo`; Monaco-only
  `undo_stack()`/`redo_stack()` (empty slices for GTK/Qt).
- `UndoRules` (`#[non_exhaustive]`): `Monaco` (quire's `history.rs`), `Gtk`, `Qt` (FX3's models,
  ported to byte offsets and change sets).
- `UndoMode` (`Local | Host`), `UndoGroup`, `UndoGroupClosed`: as today.

### 2.5 Rules

```rust
pub struct DocRules {          // what text is valid; enforced by apply(); shared as Arc<DocRules> (T3)
    pub undo: UndoRules, pub undo_mode: UndoMode, pub undo_pause_ms: Option<u64>,
    pub undo_limit: Option<usize>,   // oldest undo steps dropped past it (T4); None: unbounded
    pub lines: LineMode,        // Multi | Single { breaks: LineBreaks }
    pub max_chars: Option<u32>, // code points (GTK max-length); HTML maxlength counts UTF-16 units (A4)
    pub read_only: bool,        // foreign changes still apply
    pub recent_window: usize,   // ring capacity, allocated on the first edit (T2);
}                               // DocRules::field(): 4, quire's editor: 64 (EditorConfig)
pub enum LineBreaks { Strip, Spaces, Keep }   // how inserted '\n' is treated in Single; Strip default
pub struct EditRules {         // how commands behave; passed by reference per call, built once and
                               // cached by the caller (svelters: from props and Policy; T3)
    pub words: WordRules,
    pub word_move_collapses: bool,     // GTK: Ctrl+Arrow on a selection collapses to its edge
    pub backspace: DeleteUnit,         // VsCode (code point; emoji cluster whole) | Grapheme | CodePoint
    pub smart_home: bool,
    pub empty_selection_clipboard: bool, // VS Code: cut/copy of an empty selection takes the line
    pub secret: bool,                  // word motions jump to the ends; no SetClipboard, no SetPrimary (A3)
    pub primary: PrimaryRule,          // Off | Set | SetAndClear (GTK)
    pub filter: Option<InputFilter>,   // fn(&str) -> Cow<'_, str>, applied to inserted text
}
```

`WordRules::VsCode` holds a prebuilt `WordClassifier`; callers cache the `EditRules` (and so the
classifier) instead of rebuilding it per command (T3). quire-core today rebuilds one per command
(`commands::run`: `WordClassifier::new(&cfg.word_separators)`) and per double-click (egui_quire
`glue::word_range_at`); step 2 fixes both: `Document` caches its `EditRules`, rebuilt only in
`set_config`, with a new accessor `Document::edit_rules()` (a small public helper: listed for the
coordinator before it lands).

`max_chars`: an insertion is truncated at a code point boundary to fit (GTK); nothing left to
insert is `Rejected::MaxChars`. `NoWrap` and `EditRules::default()` make a single-line field need
no trait impl (A2).

### 2.6 Words and visual lines

- `WordRules` (`#[non_exhaustive]`): `VsCode(WordClassifier)` (quire: the literal VS Code
  translation), `Pango`, `Qt` (FX1, A6), `Custom(&'static dyn WordBoundaries)`.
- `WordBoundaries` (trait, `Sync`): `next(&Text, at, WordStop) -> usize`,
  `prev(&Text, at, WordStop) -> usize`, `word_at(&Text, at) -> Range<usize>`.
- `WordStop`: `Native`, `Start`, `StartFast`, `End`, `StartOrSpace`, `EndOrSpace` (VS Code's
  `WordNavigationType` plus deleteWord's whitespace-first rule; Pango/Qt treat all as `Native`).
- `WordClassifier`, `CharClass`, `word_range_at`, `line_range_at`: moved unchanged.
- `VisualLines` (trait, implemented by the caller on its layout):
  `locate(&Text, byte, upstream) -> (VisualLine, Goal)`,
  `offset(&Text, VisualLine, delta: isize) -> Result<VisualLine, Edge>`,
  `byte_at(&Text, VisualLine, Goal) -> (usize, bool /*upstream*/)`,
  `range(&Text, VisualLine) -> Range<usize>`, `lines_per_page() -> usize`.
- `VisualLine { line: usize, row: usize }`, `Goal(pub u32)` (`Goal::END = u32::MAX`), `Edge`
  (`Start | End`).
- `NoWrap { tab_size: u32, page_lines: u32 }` (A2): one row per logical line, goal = visible
  column (tabs expanded). quire-core's fold-skipping impl wraps it.

### 2.7 Commands

```rust
#[non_exhaustive]
pub enum EditCommand {
    Insert { text: String, kind: InputKind, distribute: bool }, // distribute: one line per cursor (paste)
    Move { unit: Unit, dir: Dir, extend: bool },
    Delete { unit: Unit, dir: Dir },
    SelectAll, SelectWordAt(usize), SelectLineAt(usize),
    SetSelection(Selection), ReplaceSelection(String),
    Undo, Redo, Cut, Copy, Paste,
    Preedit(PreeditUpdate),   // Update { text, active } | Commit { text, replace_prev_chars,
                              //   replace_next_chars } (code points, SEAM-CHANGES 7) | Cancel
    HexEntry(HexKey),         // Start | Digit(u8) | Backspace | Commit | Cancel  (GTK Ctrl+Shift+U)
}
pub enum Unit { Grapheme, Word(WordStop), LineEdge, VisualLine, Page, DocEdge }
pub enum Dir { Back, Forward }
pub struct Resolved { pub tr: Option<Transaction>, pub requests: SmallVec<[HostRequest; 1]>,
                      pub rejected: Option<Rejected> }
pub struct Executed { pub applied: Option<Applied>, pub requests: SmallVec<[HostRequest; 1]>,
                      pub reveal: Option<usize>, pub rejected: Option<Rejected> }
pub enum HostRequest { SetClipboard(String), ReadClipboard, SetPrimary(String), ClearPrimary,
                       Beep, Undo, Redo /* Host mode */ }
pub enum Rejected { ReadOnly, MaxChars, Filtered, LineBreak, Secret }
```

`pub mod ops`: the pure building blocks quire-core's richer commands call (`RangeEdit`,
`build`, `track`, `Model`/`Pos`, grapheme and left-delete offsets, visible columns, the word
operations). Documented as "for editors built on quire-text".

## 3. Module map

| quire-text module | Source |
|---|---|
| `text.rs` (+ `text/{small,rope}.rs` in step 6) | quire-core `text.rs` (+ `as_str`, `grapheme_next/prev`); the two backends (T1) |
| `change.rs`, `version.rs`, `selection.rs`, `recent.rs`, `columns.rs` | quire-core, unchanged |
| `transaction.rs` | quire-core `transaction.rs` minus `Effect` (stays in quire-core); `Transaction<E>`, `AuthorId` |
| `document.rs` | from quire-core `document.rs`: `Snapshot`, `Anchor`, `Refusal` (minus `InvalidQuery`), `Applied`, rebase, validate, history and recent bookkeeping of `apply_inner` |
| `view.rs` | from quire-core `view.rs`: selection, `selection_version`, `synced_at`, focus, `Preedit`, `sync`/`map_through` core |
| `history/{mod,monaco}.rs` | quire-core `history.rs` |
| `history/{gtk,qt}.rs` | svelters FX3 `native/undo.rs` @ `f120f7f3` (+ its tests; provenance line) |
| `words/{mod,vscode}.rs` | quire-core `words.rs` + `commands/words.rs` (Microsoft notice kept) |
| `words/{pango,qt}.rs` | svelters FX1 `native/words.rs` @ `4365d5a9` (+ its 2 tests; provenance line) |
| `ops/model.rs` | quire-core `commands/model.rs` |
| `ops/{mod,motion,edit}.rs` | quire-core `commands/mod.rs` (`build`, `track`, `RangeEdit`), `motion.rs` (horizontal, word, line-edge, doc-edge; `vertical` generalised over `VisualLines`), `edit.rs` (`delete_right`, plain `delete_left`, `delete_word`, `paste`, `cut`, the plain-insert step of `simple_type`), `lines.rs` (`delete_all_left/right`) |
| `command.rs`, `rules.rs`, `visual.rs` | new (§2.5–§2.7) |

**Stays in quire-core:** `config` (`EditorConfig` is mapped to `DocRules`/`EditRules`), `lang`,
`syntax`, `diagnostics`, `folding`, `find`, `large`, `Effect`, `EditorCommand`, the typing
interceptors (auto-close, overtype, surround, Enter/indent, electric), auto-closed pair delete,
tab-stop backspace, Tab/shift, comments, indentation conversion, line move/copy/insert/delete,
multi-cursor and column select, fold and find commands, `Document` (tokens, diagnostics,
`saved_at`/dirty, folds, degraded, config, `SettledProbe`) and `EditorView` (folds, find, scroll,
reveal, auto-closed, view session) as wrappers around `TextDocument`/`TextView`.

New dependencies: `unicode-general-category` 1.x (FX1; Apache-2.0); dev: `stats_alloc` 0.1 (MIT; counting
allocator without our own `unsafe`, which the workspace forbids).

## 4. How `EditorCommand` lowers to `EditCommand` (A5)

quire-core's `commands::run` resolves the overlapping commands through `TextDocument::resolve`
with `EditRules` built from `EditorConfig` (`VsCode(word_separators)`, `DeleteUnit::VsCode`,
`smart_home`, `empty_selection_clipboard`) and a `FoldedLines { NoWrap, folds, page_lines }`
layout, then restores quire's observable transaction shape (origin, group hint, ScrollIntoView and
reveal effects) exactly as today.

| EditorCommand | EditCommand | quire-core keeps |
|---|---|---|
| `CursorMove Left/Right` | `Move { Grapheme, Back/Forward }` | — |
| `CursorMove Up/Down`, `PageUp/PageDown {lines}` | `Move { VisualLine \| Page }` | the fold-skipping layout |
| `WordLeft / WordStartLeft / WordEndLeft` | `Move { Word(StartFast / Start / End), Back }` | — |
| `WordStartRight / WordEndRight` | `Move { Word(Start / End), Forward }` | — |
| `LineStart / LineEnd`, `DocumentStart / End` | `Move { LineEdge \| DocEdge }` | — |
| `SetSelection`, `SelectAll` | same | — |
| `Paste { text, per_cursor }` | `Insert { kind: Paste, distribute: per_cursor }` | — |
| `Cut` | `Cut` | drops the `SetClipboard` request (the widget copies); origin `Input(Other)` |
| `DeleteLeft` | `Delete { Grapheme, Back }` | auto-closed pair delete and tab-stop delete checked first |
| `DeleteRight` | `Delete { Grapheme, Forward }` | — |
| `DeleteWord{,Start,End}{Left,Right}` | `Delete { Word(StartOrSpace / Start / End …) }` | pair delete (left); origin `Command(id)` |
| `DeleteAllLeft / DeleteAllRight` | `Delete { LineEdge, Back/Forward }` | `Isolated`, origin `Command(id)` |
| `Undo / Redo` | same | `HostRequest::Undo/Redo` mapped to its own enum |
| `CompositionType` | function-level: `ops` delete-surrounding, then quire's typing interceptors | interceptors |
| `Type` | function-level: interceptors, plain step via `ops` insert (space-kind classification shared) | interceptors |
| everything else | not lowered | all of it |

Lowering is behaviour-preserving by construction (the moved code is the code that ran before), and
the duplicates are deleted in the same change: one motion, delete and undo-grouping
implementation.

egui_quire's wrap-aware `wrapped_motion` (Up/Down/Home/End and, since `056133c`, column select by
visual row) duplicates `vertical` over its wrap map. Moving it onto `VisualLines` is open question 3.

## 5. Behaviour-neutrality plan

Each step is a commit on `tc0`; after each, quire-core, egui_quire and quire-syntax tests run
(touched crates only, through `scripts/cargo.sh`) and must match the baseline counts recorded on
master before step 1.

1. **Move.** Create `crates/quire-text` with the moved modules verbatim; quire-core re-exports
   them under the same paths. No logic change. Unit tests move with their modules.
2. **Split the model.** `Transaction<E>` with `type Transaction = quire_text::Transaction<Effect>`;
   `Document` wraps `TextDocument` (via `apply_with`, then tokens, folds, diagnostics, dirty,
   degraded and effects as today); `EditorView` wraps `TextView` (its folds, find, auto-closed
   and scroll anchor map through `sync_with`, in today's order: secondary state first, then the
   selection); AuthorId plumbing (A1).
3. **Lower.** `EditCommand`, `resolve`/`execute`, the §4 table. Before deleting the old paths, a
   temporary differential proptest runs old `commands::run` against the lowered path for every
   overlapping command on random texts, selections and configs (byte-identical transactions);
   it is deleted with the old code in the same commit.
4. **Ports and rules.** FX1/FX3 ports with their tests (A6), `DocRules`/`EditRules`
   (single-line, `max_chars`, `secret`, filters, PRIMARY, `undo_limit`), `NoWrap`, preedit/hex
   entry, lazy History/RecentChanges, `Arc<DocRules>`, the allocation and wasm checks. The commit
   that ports FX3 names the retired test `fx3_splice_places_repeats_at_the_caret` and why (Q6).
5. **Optional for TC0:** egui_quire's wrapped motions onto `VisualLines` (Q3); gate
   `tests/wrap.rs` and `tests/editor.rs` unchanged. If it slips, TC0 lands without it and a
   follow-up does it.
6. **Backend swap (T1, T7).** Its own step, after step 4: `Text` becomes the small CoW string
   below 64 KiB and ropey above, with promotion and demotion as in §2.1. Same proof as every step:
   empty test diff, byte-identical VSCODE-MAP. Then the TC-EVAL gates (§6) and, last, the E2 bench
   rerun to show large files (ropey) unchanged.

Proof of neutrality: no file under `crates/*/tests/` changes (`git diff --stat origin/master --
'crates/*/tests'` is empty, except the egui_quire `Preedit` literal in `glue.rs`, which is source);
`tests/VSCODE-MAP.md` regenerated by `tests/port/map.mjs` is byte-identical; the E2 bench is
rerun at the end (T7). `scripts/check.sh` runs once, at the end
(it gains the quire-text wasm32 check and a grep that `ropey` and `std::time` appear only privately
in `quire-text/src/text.rs` resp. nowhere).

## 6. §4.7 acceptance → tests

| §4.7 | Test |
|---|---|
| 1. quire-core builds on quire-text, tests unchanged | full quire-core, egui_quire, quire-syntax suites at baseline counts; empty test diff; VSCODE-MAP identical |
| 2. FX1 word tests | `quire-text/tests/words_native.rs`: `fx1_words_match_gtk_golden`, `fx1_words_match_qt_golden` (golden char offsets kept literal, converted at the boundary), and `fx1_pango_snake_case` (the model-level part of `native_focus.rs::fx1_gtk_focus_policy_and_words`) |
| 2. FX3 undo replays | `quire-text/src/history/{gtk,qt}.rs` unit ports of `fx3_undo_typing/edits/paste_against_both_goldens`, plus the same three in `tests/undo_native.rs` driven through `TextDocument::execute` (typing, BackSpace, Ctrl+BackSpace, paste, selection moves), plus `fx3_undo_grouping` (the model-level part of `native_edit.rs::fx3_undo_grouping`, with `replace_all(ResetHistory)` starting a new history). `fx3_splice_places_repeats_at_the_caret` is retired: change sets make the before/after diff unnecessary (open question 6) |
| 3. wasm32, dependency tree | `scripts/cargo.sh check -p quire-text --target wasm32-unknown-unknown` in check.sh; `cargo tree -p quire-text -e normal` has no egui/svelters; grep for public ropey and `std::time` |
| 4. `TextDocument::new("short")` allocates no history or ring | `tests/alloc.rs`: allocations of `TextDocument::new("short", rules.clone())` equal those of `Text::from("short")`; `TextView::new` allocates nothing (`stats_alloc`) |
| T6: idle field ≤ 512 B | `tests/alloc.rs::idle_field_fits_512_bytes`: `TextDocument` + `TextView`, single-line, empty, shared `Arc<DocRules>`: bytes live (stats_alloc) plus `size_of` of both ≤ 512 |
| T6: one inserted character ≤ 8 allocations | `tests/alloc.rs::insert_char_allocates_at_most_8`: `execute(Insert "a")` on a warm field (history and ring already allocated) |
| T6: creation ≤ 1 µs, insert of one character ≤ 2 µs | criterion bench `quire-text/benches/field.rs`, run once at the end with no other builder running (the report says so); not a unit test |
| T7: large files unchanged | E2 bench (`quire-core/benches/e2.rs`) rerun after step 6, compared with the QE2 numbers |
| extra: views sync themselves (SEAM-CHANGES 6) | `tests/views.rs` proptest: two `TextView`s, interleaved edits, both selections valid after every step |
| extra: rules | `tests/rules.rs`: single-line paste per `LineBreaks`; `max_chars` truncation in code points; `secret` emits no `SetClipboard`/`SetPrimary` and word motions go to the ends; read-only rejects edits, allows motion and copy |

## 7. Questions (all decided as recommended, register `ae16d45`)

1. **Effects in `Transaction`.** Generic `Transaction<E = NoEffect>` (quire-core aliases it with
   its `Effect`) or a quire-core wrapper struct around a plain transaction. *Decided: generic*:
   zero source churn in quire-core and egui_quire.
2. **Goal (sticky x) units.** *Decided: `u32`, units belong to the implementor*: quire uses
   visible columns; svelters may use fixed-point pixels (so the sticky x stays stable under zoom).
   Keeps `SelRange` and its serde form unchanged.
3. **egui_quire's wrapped motions onto `VisualLines`** (A5's "no duplicates" covers them; moves
   the `upstream` flags into `TextView`). *Decided: step 5, optional for TC0*, gated by the
   unchanged wrap and editor kittests; if it slips, TC0 lands without it.
4. **Host undo mode with GTK/Qt rules.** *Decided*: TC0 supports `Host` with `Monaco` grouping
   only; `Gtk`/`Qt` are `Local` (fields keep their own stacks; GTK and Qt keep them across focus).
5. **Foreign changes under GTK/Qt rules.** *Decided*: reset the history (what `set_text` does);
   Monaco keeps rebasing.
6. **FX3's `splice` test.** *Decided: retired* (no before/after reconstruction in TC0); the
   commit that ports FX3 names it. svelters keeps its copy until OW4 deletes it.
7. **Rule split.** svelters' per-view `FieldRules` vs `TC0.md`'s "single-line at the Document
   level". *Decided*: content rules (`lines`, `max_chars`, `read_only`) in `DocRules`, enforced
   by `apply` for every local transaction; behaviour rules in `EditRules`, per call.
8. **`LineBreaks::Keep`** (FX3: GTK keeps breaks pasted into a single-line field) goes beyond
   `TC0.md`'s "refused or stripped". *Decided*: offer it, default `Strip`.
9. **`ExternalSet`.** ARCH-WIDGETS equates `Undoable` with quire's reload, but quire's reload is
   foreign (not undoable). *Decided: three modes*: `ResetHistory`, `Foreign`, `Undoable`.
10. **New `InputKind` variants** (`DeletingWordLeft/Right`, `Cut`) for the GTK/Qt rule sets.
    *Decided: added*; quire-core keeps reporting `Command(id)` / `Input(Other)`, so
    observable behaviour is unchanged.
11. **Pango/Qt double-click words** (`word_at`) are not measured by WG3; FX1 is single-line.
    *Decided*: derive `word_at` from the same attributes, run Pango per paragraph (crossing
    `\n` as GtkTextView does), mark both unmeasured, and ask svelters for a pin.
12. **`pub mod ops`** exposes building blocks quire-core needs. *Decided: public and documented
    as for editors built on quire-text*, not `doc(hidden)`.
13. **quire-core `Refusal`.** *Decided*: keep its own enum (it adds `InvalidQuery`) with
    `From<quire_text::Refusal>`; the new `LineBreak`/`MaxChars` variants are mirrored but
    unreachable from `EditorConfig`.
