//! Cursor motions (VS Code `cursorMoveOperations.ts`; word motions in [`super::words`]).

use super::Ctx;
use super::model::{Model, Pos, col_from_visible, visible_col};
use super::words::{self, WordNav};
use crate::command::Motion;
use crate::selection::{SelRange, Selection};
use crate::text::Text;

pub(crate) fn cursor_move(
    ctx: &Ctx,
    text: &Text,
    sel: &Selection,
    to: Motion,
    select: bool,
) -> Selection {
    let m = Model::new(text);
    let multi = sel.len() > 1;
    sel.transform(|r| move_range(ctx, &m, r, to, select, multi))
}

fn finish(r: SelRange, head: usize, select: bool, goal: Option<u32>) -> SelRange {
    let anchor = if select { r.anchor } else { head };
    SelRange::new(anchor, head).with_goal(goal)
}

fn move_range(
    ctx: &Ctx,
    m: &Model,
    r: SelRange,
    to: Motion,
    select: bool,
    multi: bool,
) -> SelRange {
    let text = m.text;
    match to {
        Motion::Left => {
            if !r.is_empty() && !select {
                return SelRange::point(r.from());
            }
            finish(r, m.prev_grapheme(r.head), select, None)
        }
        Motion::Right => {
            if !r.is_empty() && !select {
                return SelRange::point(r.to());
            }
            finish(r, m.next_grapheme(r.head), select, None)
        }
        Motion::Up => vertical(ctx, m, r, -1, select),
        Motion::Down => vertical(ctx, m, r, 1, select),
        Motion::PageUp { lines } => vertical(ctx, m, r, -(lines.max(1) as isize), select),
        Motion::PageDown { lines } => vertical(ctx, m, r, lines.max(1) as isize, select),
        Motion::WordLeft | Motion::WordStartLeft | Motion::WordEndLeft => {
            let nav = match to {
                Motion::WordLeft => WordNav::WordStartFast,
                Motion::WordStartLeft => WordNav::WordStart,
                _ => WordNav::WordEnd,
            };
            let p = words::move_word_left(m, &ctx.cls, m.pos(r.head), nav, multi);
            finish(r, m.off(p), select, None)
        }
        Motion::WordStartRight | Motion::WordEndRight => {
            let nav = if to == Motion::WordStartRight {
                WordNav::WordStart
            } else {
                WordNav::WordEnd
            };
            let p = words::move_word_right(m, &ctx.cls, m.pos(r.head), nav);
            finish(r, m.off(p), select, None)
        }
        Motion::LineStart => {
            let p = m.pos(r.head);
            let target = if ctx.cfg.smart_home {
                let first = m.first_non_ws(p.line).unwrap_or(0);
                if p.col == first { 0 } else { first }
            } else {
                0
            };
            finish(r, m.off(Pos::new(p.line, target)), select, None)
        }
        Motion::LineEnd => {
            let line = text.byte_to_line(r.head);
            finish(r, text.line_range(line).end, select, Some(u32::MAX))
        }
        Motion::DocumentStart => finish(r, 0, select, None),
        Motion::DocumentEnd => finish(r, text.len_bytes(), select, None),
    }
}

/// `MoveOperations.moveUp` / `moveDown` with a goal column in place of leftover columns.
fn vertical(ctx: &Ctx, m: &Model, r: SelRange, delta: isize, select: bool) -> SelRange {
    let tab = ctx.cfg.tab_size;
    let start = if !r.is_empty() && !select {
        if delta < 0 { r.from() } else { r.to() }
    } else {
        r.head
    };
    let p = m.pos(start);
    let chars = m.line_chars(p.line);
    let cur_vis = visible_col(&chars, p.col, tab);
    let goal = if r.is_empty() || select {
        r.goal_column
    } else {
        None
    }
    .unwrap_or(cur_vis);
    let line_count = m.line_count() as isize;
    let target = step_lines(m.text, ctx.folds, p.line, delta);
    let was_first = p.line == 0 && p.col == 0;
    let was_last = p.line as isize == line_count - 1 && p.col == chars.len();
    let at_edge = if delta < 0 { was_first } else { was_last };
    let (line, col) = if target < 0 {
        (0, 0)
    } else if target >= line_count {
        let l = (line_count - 1) as usize;
        (l, m.max_col(l))
    } else {
        let l = target as usize;
        let lc = m.line_chars(l);
        let col = if goal == u32::MAX {
            lc.len()
        } else {
            col_from_visible(&lc, goal, tab)
        };
        (l, col)
    };
    let head = m.off(Pos::new(line, col));
    let goal = if at_edge { None } else { Some(goal) };
    let anchor = if select { r.anchor } else { head };
    SelRange::new(anchor, head).with_goal(goal)
}

/// The line `delta` visible lines away (lines hidden in folds are skipped); may lie outside the
/// document, which callers treat as the edge.
pub(crate) fn step_lines(
    text: &Text,
    folds: &[std::ops::Range<usize>],
    line: usize,
    delta: isize,
) -> isize {
    if folds.is_empty() {
        return line as isize + delta;
    }
    let count = text.len_lines() as isize;
    let hidden_by = |l: usize| {
        let (s, e) = (text.line_to_byte(l), text.line_range(l).end);
        folds.iter().find(|f| f.start < s && e <= f.end).cloned()
    };
    let mut l = line as isize;
    for _ in 0..delta.unsigned_abs() {
        l += delta.signum();
        if l < 0 || l >= count {
            return l;
        }
        if let Some(f) = hidden_by(l as usize) {
            l = if delta > 0 {
                text.byte_to_line(f.end) as isize + 1
            } else {
                text.byte_to_line(f.start) as isize
            };
            if l >= count {
                return l;
            }
        }
    }
    l
}

/// `expandLineSelection`: select the current line(s) including the line break; again: extend by
/// one line.
pub(crate) fn expand_line_selection(text: &Text, sel: &Selection) -> Selection {
    sel.transform(|r| {
        let start = text.line_to_byte(text.byte_to_line(r.from()));
        let end_line = text.byte_to_line(r.to());
        let end = if end_line + 1 < text.len_lines() {
            text.line_to_byte(end_line + 1)
        } else {
            text.len_bytes()
        };
        SelRange::new(start, end)
    })
}
