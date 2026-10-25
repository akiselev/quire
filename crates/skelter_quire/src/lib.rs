//! skelter_quire: the quire code editor on skelter, svelters' retained toolkit (svelters
//! `plans/research/skelter/architecture.md` §9, §11.3; `notes/round2/text-stack.md` §5).
//!
//! [`SkelterEditor`] draws one [`quire_core::EditorView`] of a shared [`quire_core::Document`]: it
//! shapes the lines of the viewport plus overscan through skelter-text (with quire's ASCII
//! monospace fast path), caches shaped lines and wrapped rows for two frames, records display
//! lists per compositor space, and turns skelter input into quire commands and the host events
//! egui_quire emits. Three ways to host it:
//!
//! - [`layers::EditorLayers`]: the editor's own compositor layers; scrolling and the caret blink
//!   are fast commits.
//! - [`kind::CodeEditorKind`]: the `code-editor` skelter-core kind (one display list per node).
//! - [`ime::ImeClient`]: the editor as skelter's `TextInputClient`, for the platform IME bridges.
//!
//! | Module | What |
//! |---|---|
//! | [`theme`] | [`EditorTheme`]: colours, font, caret timing |
//! | [`cache`] | the two-generation caches and their keys |
//! | [`shape`] | line shaping: skelter-text per logical line, the ASCII monospace fast path |
//! | [`wrap`] | soft wrap (VS Code's line breaks) over shaped lines |
//! | [`lines`] | display lines: injected text (preedit, inlay hints), token paints |
//! | [`height`] | per-line heights (virtual scroll) |
//! | [`visual`] | quire-text's `VisualLines` over the wrapped rows |
//! | [`editor`] | [`SkelterEditor`]: layout, scroll, input, commands, host events |
//! | [`scene`] | recording: background, content, gutter and overlay lists |
//! | [`layers`] | the compositor layers and fast commits |
//! | [`ime`] | the text-input client |
//! | [`kind`] | the `code-editor` kind |
//! | [`keymap`], [`chord`], [`command`] | VS Code keybindings over skelter keys |

pub use quire_core;
pub use skelter_base;

pub mod cache;
pub mod chord;
pub mod command;
pub mod editor;
pub mod height;
pub mod ime;
pub mod keymap;
pub mod kind;
pub mod layers;
pub mod lines;
pub mod scene;
pub mod shape;
pub mod theme;
pub mod visual;
mod widths;
pub mod wrap;

pub use editor::{
    EditorEvent, EditorOptions, EditorOutput, KeyHandling, SkelterEditor, default_keymap,
};
pub use keymap::{Keymap, OperatingSystem};
pub use scene::{EditorScene, RunSink};
pub use theme::EditorTheme;
