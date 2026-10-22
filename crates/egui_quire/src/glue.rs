//! Every call from the widget into `quire-core` goes through here, so the widget's view of the
//! core API is in one place.

use std::borrow::Cow;
use std::ops::Range;

use egui::Color32;
use quire_core::{Applied, Document, EditorCommand, EditorView, SelRange, Selection, Text, Transaction, Version};

use crate::theme::EditorTheme;

/// Bring the view up to date with the document (SEAM-CHANGES item 6).
pub fn sync(doc: &Document, view: &mut EditorView) {
    view.sync(doc);
}

pub fn text(doc: &Document) -> &Text {
    doc.text()
}

pub fn version(doc: &Document) -> Version {
    doc.version()
}

/// Line `i` without its line break.
pub fn line_str(text: &Text, i: usize) -> Cow<'_, str> {
    let s: Cow<'_, str> = text.line(i).into();
    match s {
        Cow::Borrowed(b) => Cow::Borrowed(b.strip_suffix('\n').unwrap_or(b)),
        Cow::Owned(mut o) => {
            if o.ends_with('\n') {
                o.pop();
            }
            Cow::Owned(o)
        }
    }
}

pub fn selection(view: &EditorView) -> &Selection {
    view.selection()
}

/// Run a command through the core (pure command → transaction) and apply it.
pub fn run(doc: &mut Document, view: &mut EditorView, cmd: &EditorCommand) -> Result<Option<Applied>, String> {
    let tr = doc.command(view, cmd).map_err(|e| format!("{e:?}"))?;
    match tr {
        Some(tr) => doc.apply(tr, view).map(Some).map_err(|e| format!("{e:?}")),
        None => Ok(None),
    }
}

/// Apply a selection-only transaction.
pub fn set_selection(doc: &mut Document, view: &mut EditorView, sel: Selection) -> Option<Applied> {
    let tr = Transaction::select(doc.version(), sel);
    doc.apply(tr, view).ok()
}

/// Replace `range` with `text` as one input transaction (IME surrounding delete, find replace).
pub fn replace(doc: &mut Document, view: &mut EditorView, range: Range<usize>, text: &str) -> Option<Applied> {
    let tr = Transaction::replace(doc, range, text).ok()?;
    doc.apply(tr, view).ok()
}

pub fn single(anchor: usize, head: usize) -> Selection {
    Selection::single(anchor, head)
}

pub fn ranges(sel: &Selection) -> &[SelRange] {
    sel.ranges()
}

pub fn is_dirty(doc: &Document) -> bool {
    doc.is_dirty()
}

pub fn is_settled(doc: &Document) -> bool {
    doc.is_settled()
}

pub fn host_undo(doc: &Document) -> bool {
    matches!(doc.undo_mode(), quire_core::UndoMode::Host)
}

pub fn set_composing(view: &mut EditorView, composing: bool) {
    view.set_composing(composing);
}

/// Colour runs `(start_byte, colour)` of `line`, and a hash of them for the galley key.
pub fn line_runs(_doc: &Document, _line: usize, _theme: &EditorTheme, _out: &mut Vec<(usize, Color32)>) -> u64 {
    0
}

pub fn type_cmd(text: &str) -> EditorCommand {
    EditorCommand::Type { text: text.to_owned() }
}

pub fn paste_cmd(text: &str) -> EditorCommand {
    EditorCommand::Paste { text: text.to_owned(), per_cursor: true }
}
