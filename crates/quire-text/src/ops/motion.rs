//! Cursor motions (VS Code `cursorMoveOperations.ts`; word motions per [`WordRules`]): the one
//! implementation behind quire's `cursorMove` and the field commands' `Move`.

use super::model::{Model, Pos};
use crate::command::{Dir, EditRules, Unit};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::visual::{Edge, Goal, VisualLines};
use crate::words::vscode::{self, WordNav};
use crate::words::{WordRules, WordStop};

/// Moves every range of `sel` by `unit` in `dir`; `extend` keeps the anchors.
pub fn move_selection(
    text: &Text,
    sel: &Selection,
    unit: Unit,
    dir: Dir,
    extend: bool,
    rules: &EditRules,
    layout: &dyn VisualLines,
) -> Selection {
    let m = Model::new(text);
    let multi = sel.len() > 1;
    sel.transform(|r| move_range(&m, r, unit, dir, extend, multi, rules, layout))
}

fn finish(r: SelRange, head: usize, extend: bool, goal: Option<u32>) -> SelRange {
    let anchor = if extend { r.anchor } else { head };
    SelRange::new(anchor, head).with_goal(goal)
}

#[allow(clippy::too_many_arguments)]
fn move_range(
    m: &Model,
    r: SelRange,
    unit: Unit,
    dir: Dir,
    extend: bool,
    multi: bool,
    rules: &EditRules,
    layout: &dyn VisualLines,
) -> SelRange {
    let text = m.text;
    let back = dir == Dir::Back;
    match unit {
        Unit::Grapheme => {
            if !r.is_empty() && !extend {
                return SelRange::point(if back { r.from() } else { r.to() });
            }
            let head = if back {
                m.prev_grapheme(r.head)
            } else {
                m.next_grapheme(r.head)
            };
            finish(r, head, extend, None)
        }
        Unit::VisualLine => vertical(m, r, if back { -1 } else { 1 }, extend, layout),
        Unit::Page => {
            let n = layout.lines_per_page().max(1) as isize;
            vertical(m, r, if back { -n } else { n }, extend, layout)
        }
        Unit::Word(stop) => {
            if rules.secret {
                return finish(r, if back { 0 } else { text.len_bytes() }, extend, None);
            }
            if rules.word_move_collapses && !r.is_empty() && !extend {
                return SelRange::point(if back { r.from() } else { r.to() });
            }
            let head = match &rules.words {
                WordRules::VsCode(cls) => {
                    let p = m.pos(r.head);
                    let p = if back {
                        let nav = match stop {
                            WordStop::Native | WordStop::StartFast => WordNav::WordStartFast,
                            WordStop::Start | WordStop::StartOrSpace => WordNav::WordStart,
                            WordStop::End | WordStop::EndOrSpace => WordNav::WordEnd,
                        };
                        vscode::move_word_left(m, cls, p, nav, multi)
                    } else {
                        let nav = match stop {
                            WordStop::Start | WordStop::StartOrSpace => WordNav::WordStart,
                            _ => WordNav::WordEnd,
                        };
                        vscode::move_word_right(m, cls, p, nav)
                    };
                    m.off(p)
                }
                w => {
                    if back {
                        w.prev_native(text, r.head, stop)
                    } else {
                        w.next_native(text, r.head, stop)
                    }
                }
            };
            finish(r, head, extend, None)
        }
        Unit::LineEdge => {
            if back {
                line_start(m, r, extend, rules, layout)
            } else {
                let (vl, _) = layout.locate(text, r.head, false);
                finish(r, layout.range(text, vl).end, extend, Some(u32::MAX))
            }
        }
        Unit::DocEdge => finish(r, if back { 0 } else { text.len_bytes() }, extend, None),
    }
}

/// Home: on a wrapped row past the first, the row's first non-whitespace; else the line's first
/// non-whitespace column, or column 0 if already there (smart home; plain column 0 without it).
fn line_start(
    m: &Model,
    r: SelRange,
    extend: bool,
    rules: &EditRules,
    layout: &dyn VisualLines,
) -> SelRange {
    let text = m.text;
    let (vl, _) = layout.locate(text, r.head, false);
    if vl.row > 0 {
        let rr = layout.range(text, vl);
        let row = text.slice(rr.clone()).to_cow();
        let first = rr.start + row.find(|c| c != ' ' && c != '\t').unwrap_or(0);
        if r.head != first {
            return finish(r, first, extend, None);
        }
    }
    let p = m.pos(r.head);
    let target = if rules.smart_home {
        let first = m.first_non_ws(p.line).unwrap_or(0);
        if p.col == first { 0 } else { first }
    } else {
        0
    };
    finish(r, m.off(Pos::new(p.line, target)), extend, None)
}

/// `MoveOperations.moveUp` / `moveDown` over the caller's rows, with a goal column in place of
/// leftover columns.
fn vertical(
    m: &Model,
    r: SelRange,
    delta: isize,
    extend: bool,
    layout: &dyn VisualLines,
) -> SelRange {
    let text = m.text;
    let len = text.len_bytes();
    let start = if !r.is_empty() && !extend {
        if delta < 0 { r.from() } else { r.to() }
    } else {
        r.head
    };
    let (vl, cur) = layout.locate(text, start, false);
    let goal = if r.is_empty() || extend {
        r.goal_column
    } else {
        None
    }
    .unwrap_or(cur.0);
    let at_edge = if delta < 0 { start == 0 } else { start == len };
    let head = match layout.offset(text, vl, delta) {
        Err(Edge::Start) => 0,
        Err(Edge::End) => len,
        Ok(t) => layout.byte_at(text, t, Goal(goal)).0,
    };
    let goal = if at_edge { None } else { Some(goal) };
    let anchor = if extend { r.anchor } else { head };
    SelRange::new(anchor, head).with_goal(goal)
}
