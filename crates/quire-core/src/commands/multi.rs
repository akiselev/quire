//! Multi-cursor commands: add cursor above/below, add selection to next find match, column
//! selection.
//!
//! Translated from Visual Studio Code (`src/vs/editor/contrib/multicursor/browser/multicursor.ts`,
//! `src/vs/editor/common/cursor/cursorMoveCommands.ts` (`addCursorUp/Down`),
//! `cursorMoveOperations.ts` (`translateUp/Down`) and `cursorColumnSelection.ts`,
//! microsoft/vscode @ `bdce63db`).
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).

use super::Ctx;
use super::model::{Model, Pos, col_from_visible, visible_col};
use crate::find::{FindQuery, find_all};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::view::{ColumnSelectData, EditorView, NextMatchSession, SessionKind, ViewSession};

/// The result of a selection command: the selection and the session to record with it.
pub(crate) struct SelOut {
    pub sel: Selection,
    pub session: Option<ViewSession>,
    /// Where to reveal (defaults to the primary head).
    pub reveal: Option<usize>,
}

fn vis_of(ctx: &Ctx, m: &Model, b: usize) -> (Pos, u32) {
    let p = m.pos(b);
    let chars = m.line_chars(p.line);
    (p, visible_col(&chars, p.col, ctx.cfg.tab_size))
}

/// `MoveOperations.up/down` with `allowMoveOnFirstLine = false`: stays put past the edge.
fn translate(ctx: &Ctx, m: &Model, b: usize, goal: Option<u32>, delta: isize) -> (usize, u32) {
    let (p, vis) = vis_of(ctx, m, b);
    let goal = goal.unwrap_or(vis);
    let target = p.line as isize + delta;
    if target < 0 || target >= m.line_count() as isize {
        return (b, goal);
    }
    let line = target as usize;
    let chars = m.line_chars(line);
    let col = if goal == u32::MAX {
        chars.len()
    } else {
        col_from_visible(&chars, goal, ctx.cfg.tab_size)
    };
    (m.off(Pos::new(line, col)), goal)
}

/// `CursorMoveCommands.addCursorUp/Down` on logical lines; the primary stays.
pub(crate) fn insert_cursor(ctx: &Ctx, text: &Text, sel: &Selection, down: bool) -> SelOut {
    let m = Model::new(text);
    let delta = if down { 1 } else { -1 };
    let mut ranges: Vec<SelRange> = sel.ranges().to_vec();
    let mut reveal = None;
    for r in sel.ranges() {
        let (anchor, _) = translate(ctx, &m, r.anchor, None, delta);
        let (head, goal) = translate(ctx, &m, r.head, r.goal_column, delta);
        let nr = SelRange::new(anchor, head).with_goal(Some(goal));
        reveal = Some(match reveal {
            None => head,
            Some(p) if down => head.max(p),
            Some(p) => head.min(p),
        });
        ranges.push(nr);
    }
    SelOut {
        sel: Selection::new(ranges, sel.primary_index()),
        session: None,
        reveal,
    }
}

fn configured_word(ctx: &Ctx, text: &Text, pos: usize) -> Option<std::ops::Range<usize>> {
    let m = Model::new(text);
    let (a, b) = super::words::word_at(&m, ctx.cls, m.pos(pos));
    (a != b).then(|| m.off(a)..m.off(b))
}

fn same_text(text: &Text, ranges: &[SelRange], match_case: bool) -> bool {
    let norm = |r: &SelRange| {
        let s = text.slice(r.range()).to_string();
        if match_case { s } else { s.to_lowercase() }
    };
    let first = norm(&ranges[0]);
    ranges[1..]
        .iter()
        .all(|r| !r.is_empty() && norm(r) == first)
}

/// `MultiCursorSelectionController.addSelectionToNextFindMatch`.
pub(crate) fn add_next_match(ctx: &Ctx, text: &Text, view: &EditorView) -> Option<SelOut> {
    let sel = view.selection();
    // findOptions mode: the view's find toggles for a non-empty selection.
    let (find_case, find_word) = view
        .find()
        .map(|f| (f.query.match_case, f.query.whole_word))
        .unwrap_or((false, false));
    let mut session = match view.session() {
        Some(SessionKind::NextMatch(s)) => Some(s.clone()),
        _ => None,
    };
    let mut current: Option<SelRange> = None;
    if session.is_none() {
        if sel.len() > 1 && !same_text(text, sel.ranges(), find_case) {
            let s = sel.transform(|r| {
                if !r.is_empty() {
                    return r;
                }
                match configured_word(ctx, text, r.head) {
                    Some(w) => SelRange::new(w.start, w.end),
                    None => r,
                }
            });
            return Some(SelOut {
                sel: s,
                session: None,
                reveal: None,
            });
        }
        let (whole_word, match_case) = if sel.len() == 1 && sel.primary().is_empty() {
            (true, true)
        } else {
            (find_word, find_case)
        };
        let p = sel.primary();
        let search = if p.is_empty() {
            let w = configured_word(ctx, text, p.head)?;
            current = Some(SelRange::new(w.start, w.end));
            text.slice(w).to_string()
        } else {
            text.slice(p.range()).to_string()
        };
        session = Some(NextMatchSession {
            search,
            whole_word,
            match_case,
            // VS Code's last selection in insertion order; ours are sorted, so the last one.
            last_added: *sel.ranges().last().expect("a selection has a range"),
        });
    }
    let mut session = session?;
    let next = match current {
        Some(c) => c,
        None => {
            let q = FindQuery {
                search_string: session.search.clone(),
                is_regex: false,
                match_case: session.match_case,
                whole_word: session.whole_word,
            };
            let ms = find_all(&q, text, &ctx.cfg.word_separators).ok()?;
            let from = session.last_added.to();
            let m = ms.iter().find(|m| m.start >= from).or(ms.first())?.clone();
            SelRange::new(m.start, m.end)
        }
    };
    // An empty single cursor is replaced by its word; otherwise the match is added.
    let new_sel = if sel.len() == 1 && sel.primary().is_empty() {
        Selection::from_range(next)
    } else {
        let mut v = sel.ranges().to_vec();
        v.push(next);
        Selection::new(v, sel.primary_index())
    };
    session.last_added = next;
    Some(SelOut {
        session: Some(ViewSession {
            kind: SessionKind::NextMatch(session),
            at: new_sel.clone(),
        }),
        sel: new_sel,
        reveal: Some(next.head),
    })
}

/// Which way a column-select command extends.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum ColumnDir {
    Left,
    Right,
    Up(u32),
    Down(u32),
}

/// `ColumnSelection.columnSelect`.
pub(crate) fn column_select(ctx: &Ctx, text: &Text, data: ColumnSelectData) -> SelOut {
    let m = Model::new(text);
    let tab = ctx.cfg.tab_size;
    let ColumnSelectData {
        from_line,
        from_vis,
        to_line,
        to_vis,
    } = data;
    let reversed = from_line > to_line;
    let is_rtl = from_vis > to_vis;
    let is_ltr = from_vis < to_vis;
    let count = from_line.abs_diff(to_line) + 1;
    let line_at = |i: usize| {
        if reversed {
            from_line - i
        } else {
            from_line + i
        }
    };
    let mut ranges = Vec::new();
    for i in 0..count {
        let line = line_at(i);
        let chars = m.line_chars(line);
        let sc = col_from_visible(&chars, from_vis, tab);
        let ec = col_from_visible(&chars, to_vis, tab);
        let vs = visible_col(&chars, sc, tab);
        let ve = visible_col(&chars, ec, tab);
        if is_ltr && (vs > to_vis || ve < from_vis) {
            continue;
        }
        if is_rtl && (ve > from_vis || vs < to_vis) {
            continue;
        }
        ranges.push(SelRange::new(
            m.off(Pos::new(line, sc)),
            m.off(Pos::new(line, ec)),
        ));
    }
    if ranges.is_empty() {
        for i in 0..count {
            let end = text.line_range(line_at(i)).end;
            ranges.push(SelRange::point(end));
        }
    }
    let reveal = if reversed {
        ranges.iter().map(|r| r.head).min()
    } else {
        ranges.iter().map(|r| r.head).max()
    };
    // The primary is the first state (the `from` line).
    let sel = Selection::new(ranges, 0);
    SelOut {
        session: Some(ViewSession {
            kind: SessionKind::Column(data),
            at: sel.clone(),
        }),
        sel,
        reveal,
    }
}

/// The column-select data in effect: the recorded one, else from the primary range
/// (`Cursor.getCursorColumnSelectData`).
fn current_data(ctx: &Ctx, text: &Text, view: &EditorView) -> ColumnSelectData {
    if let Some(SessionKind::Column(d)) = view.session() {
        return *d;
    }
    let m = Model::new(text);
    let p = view.selection().primary();
    let (a, av) = vis_of(ctx, &m, p.anchor);
    let (h, hv) = vis_of(ctx, &m, p.head);
    ColumnSelectData {
        from_line: a.line,
        from_vis: av,
        to_line: h.line,
        to_vis: hv,
    }
}

/// `columnSelectLeft/Right/Up/Down` (and the paged variants).
pub(crate) fn column_select_step(
    ctx: &Ctx,
    text: &Text,
    view: &EditorView,
    dir: ColumnDir,
) -> SelOut {
    let m = Model::new(text);
    let mut d = current_data(ctx, text, view);
    match dir {
        ColumnDir::Left => d.to_vis = d.to_vis.saturating_sub(1),
        ColumnDir::Right => {
            let (lo, hi) = (d.from_line.min(d.to_line), d.from_line.max(d.to_line));
            let max = (lo..=hi)
                .map(|l| {
                    let chars = m.line_chars(l);
                    visible_col(&chars, chars.len(), ctx.cfg.tab_size)
                })
                .max()
                .unwrap_or(0);
            if d.to_vis < max {
                d.to_vis += 1;
            }
        }
        ColumnDir::Up(n) => d.to_line = d.to_line.saturating_sub(n.max(1) as usize),
        ColumnDir::Down(n) => d.to_line = (d.to_line + n.max(1) as usize).min(m.line_count() - 1),
    }
    column_select(ctx, text, d)
}

/// The mouse `columnSelect`: from an anchor (line, visual column) to a head.
pub(crate) fn column_select_mouse(
    ctx: &Ctx,
    text: &Text,
    anchor_line: usize,
    anchor_column: u32,
    head_line: usize,
    head_column: u32,
) -> SelOut {
    let last = text.len_lines() - 1;
    column_select(
        ctx,
        text,
        ColumnSelectData {
            from_line: anchor_line.min(last),
            from_vis: anchor_column,
            to_line: head_line.min(last),
            to_vis: head_column,
        },
    )
}
