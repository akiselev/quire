//! egui_quire: the quire code editor widget for egui 0.37.

pub use egui;

pub mod chord;
pub mod galley_cache;
pub mod height;
pub mod theme;

pub use chord::Chord;
pub use height::HeightIndex;
pub use theme::{EditorText, EditorTheme};
