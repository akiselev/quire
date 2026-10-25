//! quire-text's [`VisualLines`] over the editor's wrapped rows: vertical motion, PageUp/PageDown
//! and row Home/End under soft wrap go through quire-text's one motion implementation.
//!
//! The goal x of vertical motion is in 1/64 points (fixed point, so the sticky column is stable).
//! Rows of lines hidden by folds are skipped. Lines outside the laid-out viewport are laid out on
//! demand through the editor's caches (the trait takes `&self`, hence the `RefCell`).

use std::cell::RefCell;
use std::ops::Range;

use quire_core::{Document, EditorView, Text};
use quire_text::{Edge, Goal, VisualLine, VisualLines};

use crate::editor::{LaidLine, SkelterEditor};

/// Fixed-point units per point of the goal x.
pub const GOAL_UNITS: f32 = 64.0;

/// The editor's rows as [`VisualLines`].
pub struct WrappedLines<'a> {
    inner: RefCell<Inner<'a>>,
    page: usize,
}

struct Inner<'a> {
    editor: &'a mut SkelterEditor,
    doc: &'a Document,
    view: &'a mut EditorView,
}

impl std::fmt::Debug for WrappedLines<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrappedLines")
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

impl<'a> WrappedLines<'a> {
    pub fn new(
        editor: &'a mut SkelterEditor,
        doc: &'a Document,
        view: &'a mut EditorView,
        page: usize,
    ) -> Self {
        WrappedLines {
            inner: RefCell::new(Inner { editor, doc, view }),
            page,
        }
    }

    fn line(&self, line: usize) -> LaidLine {
        let mut i = self.inner.borrow_mut();
        let Inner { editor, doc, view } = &mut *i;
        match editor.laid(line) {
            Some(l) => l.clone(),
            None => editor.layout_line(doc, view, line),
        }
    }

    fn hidden(&self, text: &Text, line: usize) -> bool {
        self.inner.borrow().view.is_line_hidden(text, line)
    }
}

fn to_goal(x: f32) -> Goal {
    Goal((x.max(0.0) * GOAL_UNITS) as u32)
}

impl VisualLines for WrappedLines<'_> {
    fn locate(&self, text: &Text, byte: usize, upstream: bool) -> (VisualLine, Goal) {
        let byte = byte.min(text.len_bytes());
        let line = text.byte_to_line(byte);
        let l = self.line(line);
        let d = l.display.to_display(byte - text.line_to_byte(line));
        let (row, x) = l.rows.locate(d, upstream);
        (VisualLine { line, row }, to_goal(x))
    }

    fn offset(&self, text: &Text, at: VisualLine, delta: isize) -> Result<VisualLine, Edge> {
        let (mut line, mut row) = (at.line, at.row as isize);
        let mut left = delta;
        let n = text.len_lines();
        while left != 0 {
            let step = left.signum();
            row += step;
            let rows = self.line(line).rows.n_rows() as isize;
            if row < 0 || row >= rows {
                // the next visible line in this direction
                let mut l = line as isize;
                loop {
                    l += step;
                    if l < 0 {
                        return Err(Edge::Start);
                    }
                    if l >= n as isize {
                        return Err(Edge::End);
                    }
                    if !self.hidden(text, l as usize) {
                        break;
                    }
                }
                line = l as usize;
                row = if step > 0 {
                    0
                } else {
                    self.line(line).rows.n_rows() as isize - 1
                };
            }
            left -= step;
        }
        Ok(VisualLine {
            line,
            row: row as usize,
        })
    }

    fn byte_at(&self, text: &Text, at: VisualLine, goal: Goal) -> (usize, bool) {
        let l = self.line(at.line);
        let start = text.line_to_byte(at.line);
        let row = at.row.min(l.rows.n_rows() - 1);
        let (d, up) = if goal == Goal::END {
            let r = &l.rows.rows[row];
            (r.range.end, row + 1 < l.rows.n_rows())
        } else {
            l.rows.hit(row, goal.0 as f32 / GOAL_UNITS)
        };
        (start + l.display.to_model(d), up)
    }

    fn range(&self, text: &Text, at: VisualLine) -> Range<usize> {
        let l = self.line(at.line);
        let start = text.line_to_byte(at.line);
        let r = &l.rows.rows[at.row.min(l.rows.n_rows() - 1)];
        start + l.display.to_model(r.range.start)..start + l.display.to_model(r.range.end)
    }

    fn lines_per_page(&self) -> usize {
        self.page
    }
}
