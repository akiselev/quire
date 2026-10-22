# quire design (extract)

Extracted from svelters `a0e1e768` `plans/research/ARCH-CRATES.md` ("Decided", §1, §3, §4.2) and `plans/research/ARCH-EMBED.md` §7.2 on 2026-10-22. The source documents win; the seam changes in `docs/design/SEAM-CHANGES.md` win over both.

## Decided (user, 2026-10-22)

The user took every recommendation in §7 (O1–O14), to be revisited later: names `berth`
(`berth-core`, `egui_berth`) and `quire` (`quire-core`, `quire-syntax`, `egui_quire`); MIT OR Apache-2.0;
git tags only; host applies intents plus `auto_apply`; immediate viewports first; four pane events; left,
right and bottom regions; highlighter by E5; `<code-editor>` wrapper in cadatomic; host-owned undo for
documents; ropey 1.6 behind `Text`; VS Code keymap as data; cross-window drag without the winit fork
except Wayland; the crate owns the layout format.

Plus the seam decisions and corrections recorded in `ARCH-EMBED.md` "Decided" and cadatomic `ecd49c5`
(R1–R12). Repos: `github.com/akiselev/berth` and `github.com/akiselev/quire` (private). The ThinkPad builds
berth, with behaviour tests derived from KDDockWidgets' test suite (reimplemented, not copied: KDDW is
GPL/commercial) and egui_tiles/egui_dock as further references. The MacBook builds quire on ropey, with VS
Code's editor tests ported (MIT; keep its notice).


## 1. Ground rules shared by both crates

### 1.1 Dependencies and versions

| Rule | Why |
|---|---|
| Renderer crates depend on `egui = "=0.37.0"` with `default-features = false` plus what they use. They depend on `accesskit` only through `egui::accesskit`. They never depend on eframe (examples use it as a dev-dependency) | User decision: egui only. One egui copy per graph (`EGUI.md` §5 item 3: two eguis fail as `expected egui::Ui, found egui::Ui`). `native-ext` §6.6: egui types carry no ABI promise, so pin exactly |
| The renderers re-export `egui` | The same pattern as svelters-egui (`crates/svelters-egui/src/lib.rs`) |
| Core crates do not depend on egui, winit, wgpu or tokio, and build for `wasm32-unknown-unknown`. CI checks both (as cadatomic's `check.sh` does, `01-overview` §3.1) | The core is testable without a display. The hub's wasm viewer (`code-editor` §4.4) and remote layouts can reuse it |
| Optional `serde` (on by default) and `schemars` features in the cores | cadatomic registers each dock command and editor command as a registry command whose `Args: Serialize + DeserializeOwned + JsonSchema` has an object root (`command-arch` §2.3, §4.1) |
| No thread creation. Off-thread work goes through a host-supplied `Spawner` | `native-ext` §6.1: "Jobs and threads are host-owned". cadatomic's job pool and svelters' workers stay in charge |
| `unsafe_code = "forbid"` | cadatomic's workspace denies unsafe code (`native-ext` §1). The thread-local viewport trick needs no `unsafe` (`docking` §4.3) |
| MSRV 1.95, the same as egui 0.37 and cadatomic (`01-overview` §4) | |
| egui upgrades: one crate minor per egui minor, upgraded in the same joint lane as svelters-egui and cadatomic | svelters `CLAUDE.md`; `EGUI.md` §5 item 6; `CADATOMIC.md` §6 D5 |

### 1.2 Principles carried over from cadatomic

Each principle is something a host needs from a library crate, so it holds in the crate and not only in cadatomic:

- **Ids, never labels or positions** (`01-overview` §3.4):
  - Panes are host strings.
  - Groups and splits get persisted counters that no operation rewrites.
  - Text positions are only valid with a `Version`, the same rule as `models` §4.2: "a position is valid only
    together with its `ModelVersion`".
- **One mutation path** (`command-arch` §1, §8.4; `docking` §5.2):
  - Model mutators are private.
  - `apply(cmd)` is the only entry point, and availability is a pure function of the state and the policy.
  - This avoids failure class F7 (`docking` §3: "many mutation entry points, each re-implementing policy").
- **Nothing moves during a pass** (`docking` §5.2, F5; `01-overview` §3.3):
  - The widgets queue intents.
  - The host or `auto_apply` applies them before the next pass.
- **Wait-idle is a real barrier** (`01-overview` §3.10). Both crates expose `is_settled()`, which is false while:
  - a drag is in progress;
  - intents are queued;
  - a highlight job is pending;
  - an animation is running.

  cadatomic registers it with `IdleBarrier`, as it does svelters' `settled()` (`CADATOMIC.md` SVP4).
- **Render only what changed** (`01-overview` §3.8; `framework` §8):
  - There is no repaint request when idle.
  - The caret blink stops after a few seconds of idle and under reduced motion (`code-editor` §4.6).
  - Repaints are requested per viewport (`request_repaint_of`), so one spinner does not wake every window
    (`slint` §5).
- **Text comes from the host**:
  - Every user-visible string goes through a text struct the host fills from its catalog. cadatomic's existing
    `DockText` is the model (`dock/mod.rs`).
  - Refusal reasons are ids with arguments, such as `dock.split.too_small{min}`, as in `command-arch` §4.1
    `Availability::Unavailable { reason: ReasonId, args }`.
- **Theming comes from the host**:
  - `DockStyle` and `EditorTheme` are plain structs, with defaults derived from `egui::Visuals`.
  - cadatomic fills them from its role and component tokens (`framework` §2.3). The crates never hard-code
    colours.
- **Accessibility from the first commit** (`docking` F9, §5.8; `framework` §6):
  - Roles and names are built through `Context::accesskit_node_builder` (egui 0.37, `context.rs:4203`).
  - kittest finds things by role and label.
- **Anchors for scenarios, tutorials and MCP** (`framework` §2.1 `AnchorId`; `docking` §5.2):
  - Both widgets answer `anchor_rect(&str)` queries for their own sub-anchors, such as
    `window[main]/editor/group[g3]/tabstrip`, or `line[42]` and `caret` in the editor.
  - cadatomic prefixes them with the anchor of the hosting node.

### 1.3 The common shape: state, command, reducer, widget

```rust
// In each core crate (names per crate below):
pub struct State { /* plain data; Clone is cheap (Arc / rope sharing) */ }
pub enum Command { /* serde, tagged; one variant per operation */ }
impl State {
    pub fn availability(&self, cmd: &Command, policy: &dyn Policy) -> Availability;   // pure
    pub fn apply(&mut self, cmd: Command, policy: &dyn Policy) -> Result<Applied, Refusal>;
    pub fn is_settled(&self) -> bool;
}
// In each renderer crate:
pub struct Widget<'a> { state: &'a State, /* config */ }
impl Widget<'_> { pub fn show(self, ui: &mut egui::Ui, /* host */) -> Output /* intents + events */; }
```

- **Prior art:**
  - CodeMirror 6: `EditorState` is immutable, `view.dispatch(tr)` applies a transaction, and the host can
    intercept with `dispatchTransactions`.
  - Elm and Redux reducers.
  - KDDW's core/view split (`docking` §11.3), in which views never do geometry.
- **The difference from egui_tiles and egui_dock:** they mutate inside `ui` through a `Behavior`/`TabViewer` with
  20–39 hooks (`docking` §2.1), which is the root cause of F5 and F7.
- **Hosts without a registry** call `Widget::auto_apply(true)`, which applies the output's intents itself at the
  start of the next `show`.

## 3. The code-editor crate (`quire`)

### 3.1 Goals and non-goals

**Goals (P0 of `code-editor` §8, split as TRACKS ED1/ED2):**

- **A buffer and edit model in an egui-free core:**
  - a rope (`ropey` 1.6) behind `Text`;
  - `ChangeSet`, composable and invertible, Helix's model (`code-editor` §4.2);
  - `Transaction` (changes, selection, effects, annotations, base version);
  - anchors with bias;
  - `Snapshot`s that clone in O(1) for background work;
  - undo groups with Monaco's closing rules;
  - CRLF and BOM recorded.
- **Editing behaviour as data-driven commands:**
  - cursor and word movement with per-platform word rules (widget-survey pins 1–2);
  - multi-click selection;
  - typing operations: auto-close, auto-surround, overtype, indent on enter, comment toggle;
  - find and replace (`regex-automata` with a size limit, `models` §3);
  - multi-cursor and column selection (ED2);
  - folding ranges (ED2).
- **Syntax:**
  - a `Highlighter` trait;
  - Monaco's compact per-line token store;
  - a tree-sitter implementation for Svelte, TypeScript, JavaScript and CSS with injections, off the UI thread;
  - native CST highlighters from hosts plug into the same trait (`code-editor` §4.3).
- **Diagnostics in, per source and version**, mapped forward through later edits. Squiggles, gutter marks and
  hover text.
- **The egui widget:**
  - virtualised lines, a per-line galley cache and a monospace fast path;
  - gutter, caret and selections;
  - IME preedit inline;
  - clipboard, including Linux PRIMARY;
  - AccessKit paging;
  - a find bar.
- **Performance targets** E1, E2, E3, E5 and E6 (§3.9).
- **A clean wrap as svelters `<code-editor>`** (§4.2).

**Non-goals (owned by the host or later work):**

- An LSP client, language services, file I/O, document revisions in an object store, Problems, and a command
  registry.
  - These are cadatomic's (`code-editor` §4.4, `cadatomic-lsp`) and LIVEIDE's.
  - The editor takes their results through a small surface: diagnostics now, then completions, hover and inlay
    hints as injected text in P1.
- CRDT collaboration (CE9). The editor stays CRDT-agnostic but ready for it (§3.4 "Remote").
- Proportional fonts (CE11: monospace only).
- Bidi (CE10: later).
- The minimap (later).
- The diff and merge editors (P2, `code-editor` §4.11). These are a second widget in the same repo later, on the
  same core.

### 3.2 Prior art and what to take

| Source (licence) | What it is | Take | Do not take |
|---|---|---|---|
| **CodeMirror 6** (MIT) | Immutable `EditorState`, `Transaction` (changes, selection, effects, annotations), `ChangeSet` with `mapPos`, `StateField`s, facets, `dispatch`, `history()` grouping by time and `userEvent`, a viewport with only visible lines rendered, a height map | **The state/transaction model and the host-dispatch hook** (`dispatchTransactions`): how a host intercepts, refuses or rebases every edit. Annotations (`userEvent: "input.type"`, `"undo"`, remote origin) drive undo grouping. Its height map is a source for our height index | The extension/facet system as a public API surface (too open for a typed Rust API; we take fixed fields plus one `Effect` enum) |
| **Monaco / VS Code** (MIT) | `TextModel` piece tree, edit-stack grouping, an interval tree for decorations, view-model line projections, cursor and typing operations, ~3,147 unit tests (`code-editor` §1) | **Algorithms and tests**, literal translation allowed (CE2): grouping rules, the decoration interval tree, `ViewModelLines` projections, cursor and typing ops, the snippet parser, fixtures from `cursor.test.ts` (E11) | The piece tree (a rope gives the same bounds, `code-editor` §1.1); DOM, textarea and EditContext input |
| **Zed** (`text`, `rope`, `editor`: GPL-3.0; `sum_tree`: Apache-2.0) | A CRDT buffer on `SumTree`, anchors as (insertion id, offset), a syntax map of injected layers reparsed on a background thread, multibuffers (`code-editor` §2.1) | **Designs only** for the GPL parts: anchors, snapshots for background threads, syntax layers in a tree. `zed-sum-tree` 0.2.0 is Apache and usable if a summary tree beats our Fenwick index (unlikely in P0) | Any GPL code |
| **Helix** (MPL-2.0); `tree-house` 0.4.0 (MPL-2.0) | `Transaction = ChangeSet + Selection`, composable and invertible changes, a history tree, ropey, tree-house incremental highlighting with injections | The `ChangeSet` model and its laws; tree-house as the candidate highlighter (E5). MPL is file-level copyleft: depending on it is fine; copying its files means keeping them MPL (§3.13) | A terminal-shaped rendering model |
| **xi-editor** (Apache-2.0) | A persistent rope; the 2020 retrospective (`code-editor` §2.1) | Keep editing and the buffer in the UI process with snapshots for workers; async front/back separation was "not a good idea"; CRDTs did not pay for a single user | Process separation; a highlighting-specialised plug-in API |
| **Lapce / floem** (Apache-2.0 / MIT; `floem` 0.2.0, 2024-11) | `lapce-xi-rope` 0.4.0 (2025-12), an editor core split from the view, an LSP client | Confirms the core/view split. `lapce-xi-rope` is a third rope candidate | Floem's reactive runtime (svelters has its own) |
| **cosmic-text** 0.19.0 (MIT OR Apache-2.0); glyphon | `Buffer` of `BufferLine`s, each with its own shape and layout cache, invalidated per line; an `Editor` on top | **Per-line shaping caches** invalidated by line edits: the same idea as our galley cache. A text stack is not needed: egui 0.37 shapes with harfrust and skrifa (`code-editor` §2.2) | Replacing egui's text stack (parley stays the E8 fallback for bidi) |
| **egui `TextEdit`** 0.37 (MIT OR Apache-2.0) | One galley for the whole text, a `String`, one cursor range, time-based undo (`code-editor` §2.3) | Read its IME handling (`IMEOutput` with `purpose`, `rect`, `cursor_rect`, `should_interrupt_composition`; `Memory::owns_ime_events` = focus) and its AccessKit `TextRun` code (`text_selection/accesskit_text.rs`) | It as a base (whole-text galley, no virtualisation) |
| **egui_code_editor** 0.4.2, 2026-10-07, still egui ^0.36 (MIT) | A `TextEdit` plus a layouter with token rules | Nothing structural | It as a base (`code-editor` §2.1; `widget-survey` row 242) |
| **ropey** 1.6.1 (MIT) vs **crop** 0.4.3 (MIT) | A B-tree rope with char, line and UTF-16 indexing vs byte-indexed with cheap clones | ropey first, behind `Text`; E2 decides. Note ropey 2.0 beta (byte-indexed) | Leaking the rope type in the public API |

### 3.3 Where this design follows or departs from cadatomic's research

| Source | Their position | Here | Why |
|---|---|---|---|
| `code-editor` §4 diagram, §4.14 | `TextBuffer`, `ChangeSet`, anchors, `SyntaxProvider` in `cadatomic-core`; `CodeEditor` in the framework (L3) | **Superseded.** Buffer, change sets, anchors, history, the highlighter trait and the tree-sitter provider live in the editor repo (`quire-core`, `quire-syntax`). cadatomic keeps `TextDocumentModel`, the `text.edit` command, `LanguageService` and the LSP client | User decision. `quire-core` keeps cadatomic-core's constraints (no egui or tokio, wasm32), so cadatomic-core can depend on it |
| `code-editor` §4.6, `models` §5.5 | The editor is one scene node, with paint in sub-caches on the retained scene | **Superseded.** An immediate egui widget with its own caches. It is still wrappable as a scene kind (§4.2) | User decision; svelters' scene draws provider kinds by calling their `ui` (`EGUI.md` §3.3) |
| `live-ide` §6.5, §7, J9 | "A code editor component in the framework", with LSP client and Problems inside | **Superseded** for the widget (own repo). The LSP client and Problems stay with the host | User decision |
| `code-editor` §4.1, CE3 | `text.edit` is the one mutating command; one revision per closed undo group | **Agree.** `Transaction`s are what `text.edit` carries. In `UndoMode::Host`, the editor emits `UndoGroupClosed` and leaves revisions and undo (head moves) to the host | CE3 |
| `command-arch` §1 item 8, §3.6 | Text undo is `TextEdit`'s own and "recorded nowhere" | **Superseded by CE3** for code documents. Scratch editors (query boxes, REPL cells) keep a local history (`UndoMode::Local`) | CE3 |
| `code-editor` §4.2 | ropey 1.6 first, crop in E2 | **Agree**, with ropey 2.0 beta and `lapce-xi-rope` added to E2 | — |
| `code-editor` §4.3 | `SyntaxProvider: Send + Sync` with `parse`, `highlights`, `brackets`, `folds`, `outline`, `indent`, `injections` | **Agree in shape.** `quire_core::syntax::Highlighter` keeps those methods, with tree types made associated, so tree-sitter and native CSTs both fit | — |
| `code-editor` §4.7, CE4 | Keys → registry commands via VS Code's keymap and context keys | **Agree, split:** the editor ships `EditorCommand` (VS Code ids) and a default VS Code keymap as data. With `KeyHandling::Host` it does no key lookup itself and the host's layered keymap (`command-arch` §3.5) dispatches `EditorCommand`s | CE4; cadatomic's `TextFocus` layer |
| `code-editor` §4.8 | AccessKit `MultilineTextInput` with `TextRun`s, a 500-line page | **Agree** (§3.8) | — |
| `models` §4.2, §5.4, §5.5 | Versioned positions; stale-while-revalidate jobs tethered to versions; a Fenwick height index; ~2 ms UI budget | **Agree, reimplemented in the crate** (it cannot depend on cadatomic's `RowHeights`) | Independence |
| `models` §3 | `regex-automata` with a compile size limit; "highlight only what is visible" | **Agree** for find | — |
| `live-ide` §6.4 | Unsaved buffer → compile on a 150 ms debounce → diagnostics; save → HMR | **Agree.** The editor supplies versioned snapshots and `Changed` and `SaveRequested` events; the debounce is the host's (§4.2) | — |
| `widget-survey` W4, rows 208/242 | Text fields on `TextEdit`; the code editor "on TextField core" | **Inverted:** the editor core comes first. A one-line mode (`code-editor` §4.12, formulas) and later multi-line fields can reuse it if pins 1–6 show `TextEdit`'s gaps | `code-editor` §2.3 |
| `native-ext` §6.1 | Hosts own threads | **Agree:** a `Spawner` trait; the editor never spawns | — |

### 3.4 Core model (`quire-core`)

```rust
/// Monotonic per EditorState; bumps on every applied change (not on selection-only transactions).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Version(pub u64);

pub struct Text { rope: ropey::Rope, line_ending: LineEnding, bom: bool }    // LF in memory (code-editor §4.1)
impl Text {
    pub fn len_bytes(&self) -> usize; pub fn len_lines(&self) -> usize;
    pub fn line(&self, i: usize) -> TextSlice<'_>; pub fn slice(&self, r: Range<usize>) -> TextSlice<'_>;
    pub fn byte_to_line_col(&self, b: usize) -> (usize, usize); pub fn utf16_to_byte(&self, line: usize, col16: usize) -> usize;
    pub fn chunks(&self) -> impl Iterator<Item = &str>;                       // for regex and the compiler
}

/// Sorted, non-overlapping edits against one base length; compose, invert, map positions (Helix / CM6 laws).
pub struct ChangeSet { /* retain / delete / insert ops */ }
impl ChangeSet {
    pub fn replace(base_len: usize, edits: impl IntoIterator<Item = (Range<usize>, impl Into<Arc<str>>)>) -> Result<Self, ChangeError>;
    pub fn compose(&self, next: &ChangeSet) -> Result<ChangeSet, ChangeError>;
    pub fn invert(&self, before: &Text) -> ChangeSet;
    pub fn map_pos(&self, pos: usize, bias: Bias) -> usize;
    pub fn diff(old: &Text, new: &Text) -> ChangeSet;                         // imara-diff; for reloads (keeps carets)
}

pub struct Selection { ranges: SmallVec<[SelRange; 1]>, primary: usize }     // multi-cursor from day one in the type
pub struct SelRange { pub anchor: usize, pub head: usize, pub goal_column: Option<u32> }

pub struct Transaction {
    pub base: Version,                       // refused if != state.version() unless rebasable (see Remote)
    pub changes: ChangeSet,
    pub selection: Option<Selection>,        // after the changes
    pub effects: SmallVec<[Effect; 2]>,      // fold/unfold, scroll-into-view, set-diagnostics, ...
    pub origin: Origin,                      // Input(InputKind) | Command(&'static str) | Undo | Redo | Remote | Reload
    pub group: GroupHint,                    // Continue | Close | Isolated (Monaco grouping input)
}

pub struct Snapshot { pub text: Text /* Arc-shared */, pub version: Version }   // O(1); Send + Sync

pub struct EditorState {
    text: Text, version: Version, selection: Selection, selection_version: u64,
    history: History, mode: UndoMode,                                 // Local | Host
    recent: RecentChanges,                                            // last N change sets, for mapping stale data
    tokens: TokenStore, diagnostics: Diagnostics, folds: Folds, find: Option<FindState>,
    config: EditorConfig, saved_at: Option<Version>, focused: bool,
}
impl EditorState {
    pub fn new(text: &str, config: EditorConfig) -> Self;
    pub fn version(&self) -> Version; pub fn snapshot(&self) -> Snapshot; pub fn text(&self) -> &Text;
    pub fn selection(&self) -> &Selection;
    pub fn apply(&mut self, tr: Transaction) -> Result<Applied, Refusal>;                 // the only mutation path
    pub fn apply_remote(&mut self, base: Version, changes: ChangeSet, origin: Origin) -> Result<Applied, Refusal>;
    pub fn command(&self, cmd: &EditorCommand) -> Result<Option<Transaction>, Refusal>;   // pure: command → transaction
    pub fn set_diagnostics(&mut self, source: SourceId, at: Version, diags: Vec<Diagnostic>) -> Result<(), Stale>;
    pub fn set_tokens(&mut self, at: Version, lines: Range<usize>, runs: TokenRuns) -> Result<(), Stale>;
    pub fn mark_saved(&mut self, at: Version); pub fn is_dirty(&self) -> bool;
    pub fn is_settled(&self) -> bool;
}
```

- **Positions** are UTF-8 byte offsets (`code-editor` §1.2). LSP's UTF-16 columns are converted at the host's LSP
  boundary with `Text::utf16_to_byte` and its inverse.
- **Stale data:**
  - `RecentChanges` keeps the last 64 change sets (configurable).
  - Diagnostics and tokens computed at an older version are mapped forward through them.
  - Data older than the window is refused as `Stale` and the host recomputes.
  - Until fresh tokens arrive, the previous tokens are shifted by the change set, so nothing flickers
    (`code-editor` §4.3, `models` §5.4).
- **Remote and CRDT readiness** (CE9):
  - The editor holds no CRDT. A collaborative host merges with Eg-walker or yrs (`code-editor` §6) and feeds the
    result in as `apply_remote(base, changes, Origin::Remote)`.
  - Selections, anchors, diagnostics and folds map through.
  - Local undo skips remote changes (per-user undo by origin, as in CodeMirror's history).
  - Anchors are the only persistent positions the crate hands out (`code-editor` §4.1 "anchors").
- **Undo groups** (`code-editor` §4.1; Monaco `editStack.ts`): typing coalesces until one of these closes the
  group:
  - a word boundary, per the platform policy;
  - a cursor jump;
  - a pause of more than 1 s;
  - a non-typing command;
  - a focus change;
  - `GroupHint::Close`.

  When a group closes, the output carries `UndoGroupClosed { from: Version, to: Version, changes: ChangeSet,
  selection_before, selection_after }`:
  - In `UndoMode::Host` this is the CE3 revision boundary. `Undo` and `Redo` are emitted as intents, and the host
    answers with transactions whose `origin` is `Undo` or `Redo`.
  - In `UndoMode::Local` the editor applies its own history.
- **Large-file policy:** above thresholds measured by E3 (Monaco's are 20 MB or 300k lines, and lines over 10k
  characters), tokenisation, folding and brackets turn off, and long lines render a prefix. The state reports
  `Degraded { reasons }`, so the host can say so.

### 3.5 Commands

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(tag = "id")]
pub enum EditorCommand {
    #[serde(rename = "cursorMove")]      CursorMove { to: Motion, select: bool },     // Motion: Left, WordRight, LineEnd, PageDown, …
    #[serde(rename = "type")]            Type { text: String },
    #[serde(rename = "deleteLeft")]      DeleteLeft, /* deleteWordLeft, deleteRight, … */
    #[serde(rename = "editor.action.clipboardPasteAction")] Paste { text: String, per_cursor: bool },
    #[serde(rename = "editor.action.commentLine")]          CommentLine,
    #[serde(rename = "editor.action.insertCursorBelow")]    InsertCursorBelow,  // ED2
    #[serde(rename = "editor.action.addSelectionToNextFindMatch")] AddNextMatch, // ED2
    #[serde(rename = "actions.find")]    Find { query: FindQuery },
    #[serde(rename = "undo")] Undo, #[serde(rename = "redo")] Redo,
    #[serde(rename = "workbench.action.files.save")] Save,                        // emits SaveRequested only
    /* fold / unfold (ED2), indent / outdent, selectAll, … */
}
```

- Ids follow VS Code wherever one exists, so imported keymaps and agents recognise them (`code-editor` §4.9, CE4).
- `EditorState::command` turns a command into a transaction without applying it. That is the pure
  preview/validate step `command-arch` §3.3 needs.
- **cadatomic's mapping:**
  - cursor and selection commands are `View` commands, recorded by scenarios only;
  - text-changing commands resolve to one `text.edit` with the resulting `ChangeSet`, an `Edit` command that is
    logged as resolved, not as keystrokes (`code-editor` §4.1).

### 3.6 Syntax (`quire-core::syntax` + `quire-syntax`)

```rust
pub trait Highlighter: Send + Sync + 'static {
    type Tree: Send + Sync + 'static;
    fn language(&self) -> &str;
    fn parse(&self, snap: &Snapshot, prev: Option<(&Self::Tree, &ChangeSet)>, cancel: &CancelFlag) -> Option<Self::Tree>;
    fn highlights(&self, tree: &Self::Tree, snap: &Snapshot, lines: Range<usize>) -> TokenRuns;   // compact, per line
    fn brackets(&self, _t: &Self::Tree, _lines: Range<usize>) -> Vec<BracketPair> { vec![] }
    fn folds(&self, _t: &Self::Tree) -> Vec<FoldRange> { vec![] }
    fn indent(&self, _t: &Self::Tree, _snap: &Snapshot, _line: usize) -> Option<IndentHint> { None }
}
pub trait Spawner { fn spawn(&self, job: Box<dyn FnOnce() + Send>); }   // host-owned threads (native-ext §6.1)
pub struct SyntaxDriver<H: Highlighter> { /* debounces, spawns parse jobs on snapshots, applies results by version */ }
```

- **Token store** (Monaco's design, `code-editor` §1.1, §4.3):
  - per line, `(start_byte: u32, style: u32)`;
  - `style` packs the token type (from a fixed list aligned with LSP semantic token types), modifiers and the
    language;
  - theme resolution happens at paint time, so a theme switch repaints without re-tokenising.
- **`quire-syntax`:**
  - `tree-sitter` 0.27 with grammars `tree-sitter-svelte-ng` 1.0.2, `-typescript` 0.23.2 and `-css` 0.25.0 (all
    MIT);
  - Svelte's `<script lang="ts">` and `<style>` injections;
  - highlight queries in Helix's directory layout;
  - one Cargo feature per language, because grammars are C builds.
- **Highlighter engine choice** (O8): E5 picks between `tree-house` 0.4 and our own query runner over
  `tree-sitter`'s `QueryCursor`.
- **Grammar ABI:** the grammars use `tree-sitter-language` 0.1, which works with tree-sitter 0.27 through
  `LanguageFn` [unverified: E5 checks the ABI].
- **UI-thread budget:**
  - the UI thread never parses;
  - parse and highlight run through `Spawner`, and results apply only if their version still matches or can be
    shifted;
  - E4's inline exception (a native reparse under 0.5 ms) is the host's choice through a synchronous `Spawner`.

### 3.7 The widget (`egui_quire`)

```rust
pub struct CodeEditor<'a> { state: &'a EditorState, id_salt: egui::Id, theme: &'a EditorTheme, keys: KeyHandling, a11y_name: Option<&'a str> }
pub enum KeyHandling { Builtin(Keymap /* VS Code default */), Host }

impl<'a> CodeEditor<'a> {
    pub fn new(state: &'a EditorState, id_salt: impl std::hash::Hash) -> Self;
    /// Size for layout without drawing (lines × line height, gutter width), for svelters' measure (§4.2).
    pub fn measure(&self, ctx: &egui::Context, available: egui::Vec2) -> egui::Vec2;
    /// Draw and interact inside `rect` (the scene gives the rect); never mutates `state`.
    pub fn show_in(self, ui: &mut egui::Ui, rect: egui::Rect) -> EditorOutput;
    /// Convenience: allocate available space and `show_in`.
    pub fn show(self, ui: &mut egui::Ui) -> EditorOutput;
}

pub struct EditorOutput {
    pub response: egui::Response,
    pub transactions: Vec<Transaction>,          // intents; host dispatches (or EditorState::apply directly)
    pub commands: Vec<EditorCommand>,            // unresolved commands when the host wants them (palette, macros)
    pub events: Vec<EditorEvent>,
    pub caret_rect: Option<egui::Rect>,          // anchors, tutorials, IME placement
}
pub enum EditorEvent {
    SaveRequested,
    UndoRequested, RedoRequested,                // UndoMode::Host
    FocusChanged(bool),
    HoverAt { pos: usize, version: Version },    // host may answer with hover content (P1)
    LinkActivated { range: Range<usize>, version: Version }, // Ctrl+click (go to definition, P1)
    ContextMenu { pos: usize },                  // host draws the menu from its registry
    DiagnosticAction { command_id: String, args: serde_json::Value }, // a diagnostic's action chosen (§4.2)
}
// Applying transactions yields (from EditorState::apply → Applied):
pub struct Applied { pub version: Version, pub changes: ChangeSet, pub selection_changed: bool, pub closed_group: Option<UndoGroup> }
```

**Rendering** (`code-editor` §4.6):

- **Visible lines** come from the scroll offset through a Fenwick height index (`models` §5.5):
  - uniform heights without wrap;
  - measured heights by line with wrap, which is ED2.
- **Per-line galley cache:**
  - the key is (line content hash, token style hash, font id, pixels per point, wrap width);
  - built from an `egui::text::LayoutJob` with one section per token run, through `ctx.fonts_mut`;
  - LRU-bounded;
  - only lines entering the viewport are shaped.
- **Monospace fast path** (CE11): column→x is arithmetic for ASCII. Tabs, wide CJK, emoji and combining marks use
  galley positions.
- **Scrolling** translates existing shapes. `Context::transform_layer_shapes` (0.37, `context.rs:3596`) is the
  option for a dedicated layer. E1 measures whether it pays.
- **Overlays:** caret, selections, squiggles, find matches and bracket highlights are cheap rect and path shapes
  each frame they change.
- **The caret blink** uses `request_repaint_after` on the editor's viewport only, stops after 5 s idle, and stays
  still under reduced motion.

### 3.8 Input, IME, clipboard and accessibility

- **Keys:**
  - With `KeyHandling::Builtin`, the keymap maps chords to `EditorCommand`s, and `Event::Text` maps to
    `Type { text }`.
  - With `KeyHandling::Host`, keys are not consumed, except `Event::Text` and IME events while focused. The host's
    `TextFocus` layer (`command-arch` §3.5) dispatches commands.
  - `KeyboardShortcut::conflicts_with_text_editing` (0.37) lets the host drop conflicting chords from lower layers.
- **IME** (egui 0.37 `ImeEvent`, `data/input/ime_event.rs`):
  - `Preedit { text, active_range_chars }` renders as injected text at each cursor with the active range
    underlined, and changes no buffer.
  - `Commit(text)` becomes one transaction and one undo group.
  - `DeleteSurrounding { before_chars, after_chars }` becomes a delete transaction.
  - Escape cancels the preedit only (widget-survey pin 6).
  - While focused (`Memory::owns_ime_events` = has focus), the widget writes
    `PlatformOutput::ime = IMEOutput { purpose, rect, cursor_rect, should_interrupt_composition }` with the caret
    rect transformed to screen space, exactly as `TextEdit` does (`widgets/text_edit/builder.rs` ~1050).
  - Reconversion and surrounding text are not exposed by winit 0.30 [unverified]. They are winit fork candidates
    (D7 set).
- **Clipboard:**
  - `Event::Copy/Cut/Paste` and `ctx.copy_text`;
  - multi-cursor paste distributes lines when the counts match;
  - Linux PRIMARY selection follows egui 0.37's support (W5 decides the policy).
- **Mouse:**
  - click, plus double- and triple-click by word and line, with drag extending by unit;
  - Alt+click adds a cursor, and Alt+Shift+drag makes a column selection (ED2);
  - Ctrl+click emits `LinkActivated`.
- **Accessibility** (`code-editor` §4.8, E9):
  - The widget's node is `Role::MultilineTextInput` with the host-supplied name.
  - Its children are `Role::TextRun` nodes for a **page** of lines around the caret: 500 when a screen reader is
    active, else 50. The pattern is egui's own `accesskit_text.rs`, with node ids `Id::new((editor_id, "run",
    line))`.
  - `text_selection` maps the primary selection into the page.
  - When the caret leaves the page, the page shifts and the line number is announced.
  - `SetValue` and `ReplaceSelectedText` actions become transactions.
  - kittest 0.37 fails an unnamed input, so the name is required.

### 3.9 Performance targets (from `code-editor` §9 and `live-ide` §9)

| Id | Target | Where measured |
|---|---|---|
| E1 | 100k-line file: p99 frame ≤ 8.3 ms while scrolling and typing; editor CPU ≤ 2 ms per frame; open to first paint ≤ 300 ms | `benches/` headless with kittest plus a wgpu run on a GPU machine |
| E2 | Rope: memory ≤ 1.5× the file size; edit p99 ≤ 50 µs at 100 MB; line fetch ≤ 1 µs (measured 1.7 µs at 100 MB, accepted 2026-10-22, register be6b40e) | criterion in `quire-core` |
| E3 | Large-file thresholds set from measurements; 60 fps degraded, never frozen | benches |
| E5 | Re-highlight after an edit ≤ 10 ms off-thread on a 20k-line Svelte file, ≤ 2 ms for ≤ 5k lines (revised 2026-10-22 from 2 ms at 20k; register be6b40e); injections correct | `quire-syntax` benches and fixtures |
| E6 | Keypress to presented frame ≤ 1 frame plus the compositor | cadatomic hardware run (outside crate CI) |
| AP6 | In-process svelters diagnostics per keystroke ≤ 20 ms | host side (svelters CX1/SV27); the editor's part is `snapshot()` in O(1) |
| — | Idle: 0 frames after the blink stops; no allocation per frame for clean lines | kittest frame counter |

### 3.10 Testing

| Layer | What |
|---|---|
| Core laws | proptest: `compose` is associative; `apply(invert(c))` restores the text; `map_pos` is monotone; anchors survive random edit streams; undo then redo is the identity; remote changes interleaved with local edits keep selections valid |
| Monaco fixtures (E11) | `cursor.test.ts` (263 tests), typing, word and find-replace tests translated to data (text, cursors, keystrokes, expected text and cursors) and run against `EditorState::command`/`apply` with no egui. P0 gate ≥ 90 %; each failure marked "ours" or a bug. MIT notice kept |
| Native pins | widget-survey pins 1–6 run through cadatomic's NA2 harness; the crate exposes the observables (text, selection with direction, focus, preedit) through `EditorState` getters |
| Widget | `egui_kittest`: typing, clipboard events, IME event injection (`Harness::event(Event::Ime(...))`), multi-click, a11y tree page shifts, find bar; snapshot images per theme |
| Syntax | grammar fixtures with expected token runs; injection fixtures for Svelte |
| Perf | criterion benches E1 (headless part), E2, E5; CI fails on > 20 % regression against the stored baseline |

### 3.11 Milestones

| Id | Content | Size (lane-days) | Maps to |
|---|---|---|---|
| **QE0** | Repo and CI (as BD0); `quire-core` text, `ChangeSet`, `Transaction`, selection, history (local and host modes), versions, snapshots, recent-changes mapping; proptests; E2 | 4–5 | ED1 |
| **QE1** | `egui_quire`: virtualised lines, galley cache, one cursor and selection, VS Code keymap (`Builtin`) and `Host` mode, clipboard, find (`regex-automata`), gutter, diagnostics (squiggles, gutter, hover text), save and dirty, a11y node with a first page, kittest suite | 5–7 | ED1 |
| **QE2** | `quire-syntax`: tree-sitter Svelte, TS, JS and CSS with injections, token store, `SyntaxDriver` on `Spawner`, E5 | 3–4 | ED1 |
| **QE3** | cadatomic wiring (in cadatomic): a Rust dock pane hosting the editor on files under `ui/`, svelters `compile()` diagnostics (CX1 later), `UndoMode::Host` with `text.edit` — the TRACKS ED1 demo editor | 2–3 | ED1, LI1 prerequisite |
| **QE4** | Rest of P0: multi-cursor and column selection, folding, IME complete (E7), AccessKit paging (E9), Monaco fixtures ≥ 90 % (E11), large-file policy (E3), soft wrap with measured heights | 10–14 | ED2 |
| **QE5** | `<code-editor>` kind in cadatomic's framework crate over SV1 (§4.2) | 1–2 | ED2 (needs RA1) |

The total is 25–35 lane-days. QE0–QE2 need nothing from svelters. QE5 needs RUSTAPI RA1. P1 (completion, hover,
inlay hints as injected text, semantic tokens) and P2 (diff and merge) are later milestones on the same core.

### 3.12 Repository layout

```text
quire/                          (own repo; cadatomic sessions own it)
  crates/quire-core/            text, changes, selection, history, commands, find, folds, tokens, diagnostics, height index
                                (ropey, smallvec, regex-automata, imara-diff; serde/schemars opt)   no egui, wasm32-clean
  crates/quire-syntax/          Highlighter impls on tree-sitter 0.27; grammars and queries behind features   (C build)
  crates/egui_quire/            widget, galley cache, input, IME, clipboard, a11y, find bar                   (egui)
  fixtures/monaco/              translated test fixtures (MIT notice)
  crates/quire-syntax/queries/<lang>/*.scm   highlight/injection/fold queries, compiled in (licence per file, see §3.13; Q2, 2026-10-22)
  benches/  examples/playground (eframe dev-dep)
  LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md
```

### 3.13 Licensing of borrowed designs and code

- **VS Code and Monaco (MIT):**
  - literal translation is allowed (CE2), keeping Microsoft's copyright and the MIT text in the translated files
    and in `THIRD_PARTY_NOTICES.md` (`code-editor` §1.2);
  - the fixtures carry the same notice.
- **CodeMirror 6 (MIT):** design, with a notice if any code is translated.
- **Zed's `text`, `rope`, `editor` and `multi_buffer` (GPL-3.0-or-later):**
  - designs only, nothing copied;
  - `zed-sum-tree` (Apache-2.0) may be used as a dependency.
- **Helix and `tree-house` (MPL-2.0):**
  - depending on `tree-house` is fine;
  - copied query files stay MPL-2.0 as separate files with their header, since MPL is file-level copyleft;
  - nvim-treesitter queries (Apache-2.0) are the alternative source;
  - check each file.
- **Grammars:** `tree-sitter-svelte-ng`, `-typescript` and `-css` are MIT.
- **ropey and crop** are MIT. **xi and Lapce** are Apache-2.0.


### 4.2 Editor → host, and `<code-editor>`

**Edits out:**

- Every applied transaction yields `Applied { version, changes, selection_changed, closed_group }`.
- `EditorState::snapshot()` is O(1) and `Send`.

**The live module loader** (`live-ide` §6.4; svelters LOADER Q1 "source overlay: in-memory file contents by path,
read before disk"):

- On `Applied` with a new version, the host debounces (150 ms) and calls the loader's overlay with
  `(path, snapshot.text, snapshot.version)`.
- The compile's diagnostics come back tagged with that version, through
  `state.set_diagnostics("svelters", version, diags)`. Diagnostics are mapped forward if the buffer moved on, and
  refused as `Stale` if they fall outside the window.
- **Save:** `EditorEvent::SaveRequested`. The host writes the file or the layer revision (`live-ide` §6.3), calls
  `state.mark_saved(version)`, and triggers HMR. Styles and tokens may apply on type (`live-ide` §6.4).
- **External change** (an agent's `ide.sources.write`, `live-ide` §6.8; a file watcher):
  - the host calls `apply_remote(base, ChangeSet::diff(old, new), Origin::Reload)`, which keeps carets;
  - when both sides changed, the host's three-way merge decides first.

**Diagnostics in:**

```rust
pub struct Diagnostic {
    pub range: Range<usize>,            // bytes, at the version passed to set_diagnostics
    pub severity: Severity,             // Error | Warning | Info | Hint
    pub message: String,                // host-localised or the server's text, labelled by source
    pub code: Option<String>,           // e.g. "unknown_element", "cad.slot_unknown"
    pub id: Option<String>,             // host's stable id (code-editor §4.10) for Problems ↔ editor sync
    pub tags: DiagTags,                 // unnecessary, deprecated
    pub related: Vec<(Range<usize>, String)>,
    pub actions: Vec<DiagAction>,       // { title, command_id: String, args: serde_json::Value }, e.g. "Grant…" (live-ide §5.2)
}
pub struct SourceId(Arc<str>);          // "svelters", "tsgo", "oxc", "runtime" (live-ide §6.4: merged, tagged by source)
```

Sources replace their own set atomically. The editor never interprets `actions`. Choosing one emits
`EditorEvent::DiagnosticAction { command_id, args }` (§3.7), and the host runs it through its registry.

**What the state and widget expose for a svelters `<code-editor>` kind** (SV1 `#[svelters::widget]` struct form,
RUSTAPI RA1; `EGUI.md` §3.3):

| Kind facet | Editor side |
|---|---|
| Node-attached Rust state | `EditorState`: `Send + 'static`, no egui types. It lives in the provider block on the node (RA1 "typed per-provider data on the node") or behind a host handle (`doc` prop). It survives prop updates and moves between slots and viewports |
| Props | `language: string`; `readonly: bool`; `options: EditorConfig` (serde: tab size, insert spaces, line numbers, word wrap, font size token, undo mode); `aria-label`; **`value: string`** (scratch mode, small buffers only); **`doc: DocRef`** (a host handle to a shared `EditorState` for files, so typing never round-trips text through JS, the same precedent as `models` §6.3 `SearchField` native binding); `selection` (bindable) |
| Exact observation (SV10) | Every value the widget changes is reported by `Applied`/`EditorOutput` the frame it changes: `version`, `selection_version`, `is_dirty`, focus. The kind wrapper calls `cx.set` for `value` and `selection` and `cx.emit` for events, so `bind:` is `exact` |
| Events | `onchange { version, changes }` (coalesced per checkpoint), `onselectionchange`, `onsave`, `onfocus` and `onblur`, `ondiagnosticaction`, `oncontextmenu` |
| Methods | `focus()`, `revealLine(n)`, `setSelection(sel)`, `executeCommand(EditorCommand)`, `setDiagnostics(source, version, diags)` |
| Measure | `CodeEditor::measure(ctx, available)`: intrinsic size from line count and line height (capped by an `options.maxLines`), or fill. No `request_discard` in steady state ("measured equals rendered", `framework` §2.2) |
| Paint and interact | `CodeEditor::show_in(ui, rect)`: the scene passes the taffy rect. All mutation leaves as `transactions`, which the wrapper applies after the pass (svelters' rule: JS never runs during the pass, `EGUI.md` §1.4) |
| Identity | The `id_salt` is the node's salted egui id (SV16 anchor or identity), so caret, scroll and IME ownership survive reparenting (SV18) and, with ID1, hot reload |
| A11y | Its own `MultilineTextInput` subtree under the node's id. The name comes from `aria-label` or the label prop |
| Settle | `EditorState::is_settled()` feeds the node's contribution to svelters' `settled()` (SVP4) |

**Where the kind wrapper lives** is O9. The recommendation is cadatomic's framework crate first, since it is the
consumer (`01-overview` §3.12 "traits and crates only with a consumer").


### 7.2 The editor seam (`<code-editor>` through EXTEND)

C-7 says the editor is cadatomic's own egui-only crate. editor §4.6/§4.14 place it as a framework `Component` in
`cadatomic-ui`; the user's decision wins, and nothing below depends on where the crate lives. To be wrappable as a
kind with `#[svelters::widget]` (§8.2), with no JS on the editing path (editor §4.12; js-api §6.6 safe mode), the
editor crate must offer:

| # | Requirement | Why |
|---|---|---|
| E-1 | State as a plain `'static` struct (`EditorState`), created from options, not requiring `Send` | stored in the node's provider block; survives moves |
| E-2 | `fn show(&mut self, ui: &mut egui::Ui, id: egui::Id, size: egui::Vec2) -> EditorOutput`: draws in the rect it is given and uses **only the caller's `Id`** (and ids derived from it) for egui memory | node ids are salted and stable (§4.4); ids from `ui.id()` would break moves and two embeds |
| E-3 | `EditorOutput { response, edits: SmallVec<ChangeSet>, selection_changed: bool, focus_changed: Option<bool>, consumed_keys: …, commands: Vec<EditorCommandRequest> }`: results returned, never callbacks | events go to the outbox after the pass; no JS or host call inside `ui` |
| E-4 | A document input that is a handle, not a string: `set_document(Arc<dyn TextDoc>)` or a `Snapshot` + version, plus a small-text mode `set_text(&str)` / `text()` for `bind:value` scratch buffers | big documents never cross into JS (models §6.3 rule "no bulk copy into JS, ever") |
| E-5 | `fn desired_size(&self, width: f32, fonts: &egui::text::Fonts) -> egui::Vec2` for auto-height (one-line formula mode, editor §4.12) | `Widget::measure` width-down/height-up (slint §4) |
| E-6 | Its AccessKit nodes (`MultilineTextInput`, text runs) under the given id, with a name the caller supplies | K-6, widgets §5.3 NA3 |
| E-7 | IME through egui's `Event::Ime` / `IMEOutput`, and a `composing()` flag | the embed must not route Enter or Escape to default buttons while composing (widgets §5.3 pin 6) |
| E-8 | `fn apply(&mut self, cmd: EditorCommand)` for methods JS may call (`focus`, `select`, `revealLine`) | kind methods (`call`) |
| E-9 | No eframe dependency, no global state, no threads it spawns (native-ext §6.1) | one egui, cadatomic owns jobs |

The wrapper in cadatomic is then about 80 lines of `#[svelters::widget(tag = "code-editor")]`:

- props: `doc: Obj<Document>`, `language`, `readonly`, a bindable `value` for scratch mode;
- events: `change: Event<Edits>`, `cursor: Event<Selection>`;
- `#[state] editor: EditorState`.

`bind:value` is exact by construction because writes go through `cx.set` (SV10).

## 8. EXTEND: the proposed API

### 8.1 Registry and ids

- **The registry.** `kinds::KindRegistry` replaces the static `KINDS` slice. It is built once per process from:
  - **built-in kinds**, which keep their generated tables and the `enum W` fast path. The SV1 pushback
    (CADATOMIC.md) and slint §10.1 both say to keep generated enums for built-ins;
