//! Command → transaction (pure). See [`crate::command`].

use crate::command::{CommandOutput, EditorCommand};
use crate::document::{Document, Refusal};
use crate::view::EditorView;

pub(crate) fn run(
    doc: &Document,
    view: &EditorView,
    cmd: &EditorCommand,
) -> Result<CommandOutput, Refusal> {
    let _ = (doc, view, cmd);
    todo!("commands land in phase 3")
}
