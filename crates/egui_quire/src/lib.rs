//! egui_quire: the quire code editor widget for egui 0.37.
//!
//! [`CodeEditor`] draws one [`quire_core::EditorView`] of a shared [`quire_core::Document`],
//! applies input transactions in the frame they arrive, and returns what it applied and what the
//! host may act on ([`EditorOutput`]).

pub use egui;
pub use quire_core;

mod a11y;
pub mod chord;
pub mod command;
mod editor;
mod find_bar;
#[doc(hidden)]
pub mod galley_cache;
mod glue;
#[doc(hidden)]
pub mod height;
pub mod keymap;
#[doc(hidden)]
pub mod layout;
mod paint;
pub mod theme;
mod widths;
mod wrap;

pub use a11y::{page_size as a11y_page_size, run_id as a11y_run_id};
pub use chord::{Chord, KeySeq};
pub use command::{KeyAction, WidgetAction};
pub use editor::{CodeEditor, EditorEvent, EditorOutput, KeyHandling, default_keymap};
pub use find_bar::{query_id as find_query_id, replace_id as find_replace_id};
pub use glue::FindOptions;
pub use height::HeightIndex;
pub use keymap::{KeyLookup, Keymap};
pub use theme::{EditorText, EditorTheme};
pub use wrap::VisualLine;
