//! [`VisualLines`]: how the caller lays text out in rows, for Up/Down, PageUp/PageDown and the
//! Home/End of a wrapped row. quire-text knows logical lines only; [`NoWrap`] is the built-in
//! layout for text that does not wrap (single-line fields need no implementation of their own).

use crate::ops::model::{Model, Pos, col_from_visible, visible_col};
use crate::text::Text;
use std::ops::Range;

/// A visual row: a logical line and a row within it (0 when the line does not wrap).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct VisualLine {
    /// The logical line.
    pub line: usize,
    /// The row within the line.
    pub row: usize,
}

/// The goal x of vertical motion (the sticky column), in units the [`VisualLines`] implementor
/// chooses: quire uses visible columns (tabs expanded); svelters may use fixed-point pixels, so
/// the goal stays stable under zoom. Stored per range in [`SelRange::goal_column`](crate::SelRange).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Goal(pub u32);

impl Goal {
    /// The end of the row (End was pressed: later vertical moves keep to row ends).
    pub const END: Goal = Goal(u32::MAX);
}

/// Past which end of the document a vertical move went.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Edge {
    /// Before the first row.
    Start,
    /// After the last row.
    End,
}

/// The caller's layout, as vertical motion needs it.
pub trait VisualLines {
    /// The row holding `byte` (with `upstream`: the row it ends, at a wrap point) and the goal x
    /// of `byte` on it.
    fn locate(&self, text: &Text, byte: usize, upstream: bool) -> (VisualLine, Goal);
    /// The row `delta` rows away from `line` (rows the caller hides, such as folds, are skipped),
    /// or the edge the move went past.
    fn offset(&self, text: &Text, line: VisualLine, delta: isize) -> Result<VisualLine, Edge>;
    /// The byte on `line` nearest `goal` ([`Goal::END`]: the row's end), and whether it is drawn
    /// upstream (at the end of a wrapped row).
    fn byte_at(&self, text: &Text, line: VisualLine, goal: Goal) -> (usize, bool);
    /// The byte range of a row (without the line feed).
    fn range(&self, text: &Text, line: VisualLine) -> Range<usize>;
    /// Rows per page, for PageUp and PageDown.
    fn lines_per_page(&self) -> usize;
}

/// One row per logical line; the goal is the visible column (tabs expanded to `tab_size`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct NoWrap {
    /// Tab width in columns.
    pub tab_size: u32,
    /// Rows per page.
    pub page_lines: u32,
}

impl Default for NoWrap {
    fn default() -> Self {
        NoWrap {
            tab_size: 4,
            page_lines: 20,
        }
    }
}

impl VisualLines for NoWrap {
    fn locate(&self, text: &Text, byte: usize, _upstream: bool) -> (VisualLine, Goal) {
        let m = Model::new(text);
        let p = m.pos(byte);
        let chars = m.line_chars(p.line);
        let vis = visible_col(&chars, p.col, self.tab_size);
        (
            VisualLine {
                line: p.line,
                row: 0,
            },
            Goal(vis),
        )
    }

    fn offset(&self, text: &Text, line: VisualLine, delta: isize) -> Result<VisualLine, Edge> {
        let l = line.line as isize + delta;
        if l < 0 {
            Err(Edge::Start)
        } else if l >= text.len_lines() as isize {
            Err(Edge::End)
        } else {
            Ok(VisualLine {
                line: l as usize,
                row: 0,
            })
        }
    }

    fn byte_at(&self, text: &Text, line: VisualLine, goal: Goal) -> (usize, bool) {
        let m = Model::new(text);
        let chars = m.line_chars(line.line);
        let col = if goal == Goal::END {
            chars.len()
        } else {
            col_from_visible(&chars, goal.0, self.tab_size)
        };
        (m.off(Pos::new(line.line, col)), false)
    }

    fn range(&self, text: &Text, line: VisualLine) -> Range<usize> {
        text.line_range(line.line)
    }

    fn lines_per_page(&self) -> usize {
        self.page_lines as usize
    }
}
