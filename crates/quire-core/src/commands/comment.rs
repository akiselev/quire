//! Line and block comments.
//!
//! Translated from Visual Studio Code (microsoft/vscode @ bdce63db), files
//! `src/vs/editor/contrib/comment/browser/lineCommentCommand.ts`, `blockCommentCommand.ts` and
//! `comment.ts`.
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).
//!
//! Differences: columns count chars (VS Code counts UTF-16 units); one language per document.
//! The comment options come from `EditorConfig::comments` (`editor.comments.insertSpace`,
//! `editor.comments.ignoreEmptyLines`) and `EditorConfig::line_comment_no_indent` (the language's
//! `comments.lineComment.noIndent`).

use super::model::{Model, Pos};
use super::{Ctx, Out, St, track};
use crate::change::ChangeSet;
use crate::config::EditorConfig;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::InputKind;
use std::ops::Range;

/// `Type` of `LineCommentCommand`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum LineCommentType {
    Toggle,
    ForceAdd,
    ForceRemove,
}

/// `BlockCommentCommand._haystackHasNeedleAtOffset` (ASCII case-insensitive), on chars.
fn has_needle_at(hay: &[char], needle: &[char], offset: usize) -> bool {
    if offset + needle.len() > hay.len() {
        return false;
    }
    hay[offset..offset + needle.len()]
        .iter()
        .zip(needle)
        .all(|(a, b)| a == b || a.eq_ignore_ascii_case(b))
}

fn find_from(hay: &[char], needle: &[char], from: isize) -> Option<usize> {
    // JavaScript `indexOf(needle, from)`.
    let from = from.max(0) as usize;
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn rfind_from(hay: &[char], needle: &[char], from: isize) -> Option<usize> {
    // JavaScript `lastIndexOf(needle, from)`.
    if from < 0 || needle.len() > hay.len() {
        return None;
    }
    let start = (from as usize).min(hay.len() - needle.len());
    (0..=start)
        .rev()
        .find(|&i| hay[i..i + needle.len()] == *needle)
}

/// An edit in (line, char column) coordinates.
#[derive(Clone, Debug)]
struct Op {
    from: Pos,
    to: Pos,
    text: String,
}

impl Op {
    fn insert(at: Pos, text: String) -> Self {
        Op {
            from: at,
            to: at,
            text,
        }
    }

    fn delete(from: Pos, to: Pos) -> Self {
        Op {
            from,
            to,
            text: String::new(),
        }
    }
}

/// How a command computes its selection after the edits.
enum CursorRule {
    /// Line comments: the tracked selection, shifted by `delta` columns, the end moved down a
    /// line when the selection ended at a line start.
    Tracked { delta: usize, move_end_down: bool },
    /// Block comments: from the end of the first edit to the start of the second, or the end of
    /// the single edit shifted left by `back` columns.
    Block { back: usize },
}

struct Cmd {
    sel: SelRange,
    ops: Vec<Op>,
    rule: CursorRule,
}

// ---------------------------------------------------------------------------------------------
// line comments

struct LineData {
    ignore: bool,
    offset: usize,
    len: usize,
}

/// `LineCommentCommand._analyzeLines`.
fn analyze_lines(
    cfg: &EditorConfig,
    ty: LineCommentType,
    m: &Model,
    token: &[char],
    start: usize,
    end: usize,
    ignore_first: bool,
) -> (bool, Vec<LineData>) {
    let mut only_ws = true;
    let mut remove = ty != LineCommentType::ForceAdd;
    let mut lines: Vec<LineData> = Vec::new();
    for line in start..=end {
        let mut d = LineData {
            ignore: false,
            offset: 0,
            len: token.len(),
        };
        if line == start && ignore_first {
            d.ignore = true;
            lines.push(d);
            continue;
        }
        let chars = m.line_chars(line);
        let Some(first) = m.first_non_ws(line) else {
            d.ignore = cfg.comments.ignore_empty_lines;
            d.offset = if cfg.line_comment_no_indent {
                0
            } else {
                chars.len()
            };
            lines.push(d);
            continue;
        };
        only_ws = false;
        d.offset = if cfg.line_comment_no_indent { 0 } else { first };
        if remove && !has_needle_at(&chars, token, d.offset) {
            match ty {
                LineCommentType::Toggle => remove = false,
                LineCommentType::ForceAdd => {}
                LineCommentType::ForceRemove => d.ignore = true,
            }
        }
        if remove && cfg.comments.insert_space {
            // VS Code measures from the first non-whitespace column even with `noIndent`.
            let e = first + d.len;
            if e < chars.len() && chars[e] == ' ' {
                d.len += 1;
            }
        }
        lines.push(d);
    }
    if ty == LineCommentType::Toggle && only_ws {
        remove = false;
        for d in &mut lines {
            d.ignore = false;
        }
    }
    (remove, lines)
}

/// `LineCommentCommand._normalizeInsertionPoint`.
fn normalize_insertion_point(m: &Model, lines: &mut [LineData], start: usize, indent: u32) {
    let next = |v: u32, tab: bool| {
        if tab {
            v + (indent - v % indent)
        } else {
            v + 1
        }
    };
    let mut min_vis = u32::MAX;
    for (i, d) in lines.iter().enumerate() {
        if d.ignore {
            continue;
        }
        let chars = m.line_chars(start + i);
        let mut v = 0;
        let mut j = 0;
        while v < min_vis && j < d.offset {
            v = next(v, chars[j] == '\t');
            j += 1;
        }
        min_vis = min_vis.min(v);
    }
    if min_vis == u32::MAX {
        min_vis = 0;
    }
    min_vis = min_vis / indent * indent;
    for (i, d) in lines.iter_mut().enumerate() {
        if d.ignore {
            continue;
        }
        let chars = m.line_chars(start + i);
        let mut v = 0;
        let mut j = 0;
        while v < min_vis && j < d.offset {
            v = next(v, chars[j] == '\t');
            j += 1;
        }
        d.offset = if v > min_vis { j - 1 } else { j };
    }
}

/// `LineCommentCommand._attemptRemoveBlockComment`.
fn attempt_remove_block(
    m: &Model,
    from: Pos,
    to: Pos,
    empty: bool,
    start_tok: &str,
    end_tok: &str,
) -> Option<Vec<Op>> {
    let mut st: Vec<char> = start_tok.chars().collect();
    let mut et: Vec<char> = end_tok.chars().collect();
    let mut start_line = from.line;
    let mut end_line = to.line;
    let first_col = m.first_non_ws(from.line).map_or(0, |f| f + 1);
    let allowed_before = et.len() + first_col.max(from.col + 1);
    let sl = m.line_chars(start_line);
    let el = m.line_chars(end_line);
    let mut si = rfind_from(&sl, &st, allowed_before as isize - 1);
    let mut ei = find_from(&el, &et, to.col as isize - st.len() as isize);
    if si.is_some() && ei.is_none() {
        ei = find_from(&sl, &et, (si? + st.len()) as isize);
        end_line = start_line;
    }
    if si.is_none()
        && let Some(e) = ei
    {
        si = rfind_from(&m.line_chars(end_line), &st, e as isize);
        start_line = end_line;
    }
    if empty && (si.is_none() || ei.is_none()) {
        let l = m.line_chars(start_line);
        si = find_from(&l, &st, 0);
        if let Some(s) = si {
            ei = find_from(&l, &et, (s + st.len()) as isize);
        }
    }
    let sl = m.line_chars(start_line);
    let el = m.line_chars(end_line);
    if let Some(s) = si
        && sl.get(s + st.len()) == Some(&' ')
    {
        st.push(' ');
    }
    if let Some(e) = ei
        && e > 0
        && el.get(e - 1) == Some(&' ')
    {
        et.insert(0, ' ');
        ei = Some(e - 1);
    }
    let (s, e) = (si?, ei?);
    Some(remove_block_ops(
        Pos::new(start_line, s + st.len()),
        Pos::new(end_line, e),
        st.len(),
        et.len(),
    ))
}

fn line_comment_cmd(
    m: &Model,
    ctx: &Ctx,
    ty: LineCommentType,
    sel: SelRange,
    ignore_first: bool,
) -> Cmd {
    let mut from = m.pos(sel.from());
    let mut to = m.pos(sel.to());
    let mut cmd = Cmd {
        sel,
        ops: Vec::new(),
        rule: CursorRule::Tracked {
            delta: 0,
            move_end_down: false,
        },
    };
    if from.line == to.line && ignore_first {
        // VS Code replaces the line break with itself here: no edit.
        return cmd;
    }
    let mut move_end_down = false;
    if from.line < to.line && to.col == 0 {
        move_end_down = true;
        to = Pos::new(to.line - 1, m.max_col(to.line - 1));
        // The command tracks the shortened selection (same direction).
        let end = m.off(to);
        cmd.sel = if sel.is_backward() {
            SelRange::new(end, sel.head)
        } else {
            SelRange::new(sel.anchor, end)
        };
    }
    let Some(token) = ctx.cfg.line_comment.as_deref().filter(|t| !t.is_empty()) else {
        // `_executeBlockComment`.
        let Some((st, et)) = ctx
            .cfg
            .block_comment
            .as_ref()
            .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        else {
            return cmd;
        };
        let empty = from == to;
        let mut delta = 0;
        let ops = match attempt_remove_block(m, from, to, empty, st, et) {
            Some(ops) => ops,
            None => {
                let ops = if empty {
                    let chars = m.line_chars(from.line);
                    let f = m.first_non_ws(from.line).unwrap_or(chars.len());
                    add_block_ops(
                        Pos::new(from.line, f),
                        Pos::new(from.line, chars.len()),
                        st,
                        et,
                        ctx.cfg.comments.insert_space,
                    )
                } else {
                    from = Pos::new(from.line, m.first_non_ws(from.line).unwrap_or(0));
                    add_block_ops(
                        from,
                        Pos::new(to.line, m.max_col(to.line)),
                        st,
                        et,
                        ctx.cfg.comments.insert_space,
                    )
                };
                if ops.len() == 1 {
                    delta = st.chars().count() + 1;
                }
                ops
            }
        };
        cmd.ops = ops;
        cmd.rule = CursorRule::Tracked {
            delta,
            move_end_down,
        };
        return cmd;
    };
    let tok: Vec<char> = token.chars().collect();
    let (remove, mut lines) = analyze_lines(ctx.cfg, ty, m, &tok, from.line, to.line, ignore_first);
    let ops: Vec<Op> = if remove {
        lines
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.ignore)
            .map(|(i, d)| {
                let l = from.line + i;
                Op::delete(Pos::new(l, d.offset), Pos::new(l, d.offset + d.len))
            })
            .collect()
    } else {
        normalize_insertion_point(m, &mut lines, from.line, ctx.cfg.indent_size.max(1));
        let after = if ctx.cfg.comments.insert_space {
            " "
        } else {
            ""
        };
        lines
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.ignore)
            .map(|(i, d)| Op::insert(Pos::new(from.line + i, d.offset), format!("{token}{after}")))
            .collect()
    };
    // A comment inserted at a cursor at its line's end moves the cursor after it.
    let cursor = m.pos(sel.head);
    let mut delta = 0;
    for op in &ops {
        if op.from == op.to && op.from == cursor && m.max_col(cursor.line) == cursor.col {
            delta = op.text.chars().count();
        }
    }
    cmd.ops = ops;
    cmd.rule = CursorRule::Tracked {
        delta,
        move_end_down,
    };
    cmd
}

// ---------------------------------------------------------------------------------------------
// block comments

/// `BlockCommentCommand._createRemoveBlockCommentOperations`: `inner` is the commented text
/// between the (space-extended) tokens.
fn remove_block_ops(inner_from: Pos, inner_to: Pos, st_len: usize, et_len: usize) -> Vec<Op> {
    if inner_from != inner_to {
        vec![
            Op::delete(
                Pos::new(inner_from.line, inner_from.col - st_len),
                inner_from,
            ),
            Op::delete(inner_to, Pos::new(inner_to.line, inner_to.col + et_len)),
        ]
    } else {
        vec![Op::delete(
            Pos::new(inner_from.line, inner_from.col - st_len),
            Pos::new(inner_to.line, inner_to.col + et_len),
        )]
    }
}

/// `BlockCommentCommand._createAddBlockCommentOperations`.
fn add_block_ops(from: Pos, to: Pos, st: &str, et: &str, insert_space: bool) -> Vec<Op> {
    let sp = if insert_space { " " } else { "" };
    if from != to {
        vec![
            Op::insert(from, format!("{st}{sp}")),
            Op::insert(to, format!("{sp}{et}")),
        ]
    } else {
        vec![Op {
            from,
            to,
            text: format!("{st}  {et}"),
        }]
    }
}

fn block_comment_cmd(m: &Model, ctx: &Ctx, sel: SelRange) -> Option<Cmd> {
    let (st0, et0) = ctx
        .cfg
        .block_comment
        .as_ref()
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())?;
    let from = m.pos(sel.from());
    let to = m.pos(sel.to());
    let sl = m.line_chars(from.line);
    let el = m.line_chars(to.line);
    let mut st: Vec<char> = st0.chars().collect();
    let mut et: Vec<char> = et0.chars().collect();
    let mut si = rfind_from(&sl, &st, (from.col + st.len()) as isize);
    let mut ei = find_from(&el, &et, to.col as isize - et.len() as isize);
    if let (Some(s), Some(e)) = (si, ei) {
        let et_str: String = et.iter().collect();
        let forced = if from.line == to.line {
            s + st.len() <= e
                && sl[s + st.len()..e]
                    .iter()
                    .collect::<String>()
                    .contains(&et_str)
        } else {
            sl[(s + st.len()).min(sl.len())..]
                .iter()
                .collect::<String>()
                .contains(&et_str)
                || el[..e].iter().collect::<String>().contains(&et_str)
        };
        // VS Code's `substring` swaps reversed bounds; tokens overlapping on one line (e.g. the
        // selection inside `/*/`) fall through to adding.
        let overlapping = from.line == to.line && s + st.len() > e;
        if forced || overlapping {
            si = None;
            ei = None;
        }
    }
    let (ops, rule) = if let (Some(s), Some(mut e)) = (si, ei) {
        if ctx.cfg.comments.insert_space && s + st.len() < sl.len() && sl[s + st.len()] == ' ' {
            st.push(' ');
        }
        if ctx.cfg.comments.insert_space && e > 0 && el[e - 1] == ' ' {
            et.insert(0, ' ');
            e -= 1;
        }
        (
            remove_block_ops(
                Pos::new(from.line, s + st.len()),
                Pos::new(to.line, e),
                st.len(),
                et.len(),
            ),
            CursorRule::Block { back: 0 },
        )
    } else {
        let ops = add_block_ops(from, to, st0, et0, ctx.cfg.comments.insert_space);
        let back = if ops.len() == 1 {
            et0.chars().count() + 1
        } else {
            0
        };
        (ops, CursorRule::Block { back })
    };
    Some(Cmd { sel, ops, rule })
}

// ---------------------------------------------------------------------------------------------
// running commands together (`executeCommands`)

fn run_cmds(text: &Text, sel: &Selection, cmds: Vec<Cmd>) -> Option<Out> {
    let m = Model::new(text);
    // (byte range, text, command, op index)
    let mut all: Vec<(Range<usize>, String, usize, usize)> = Vec::new();
    for (ci, c) in cmds.iter().enumerate() {
        for (oi, op) in c.ops.iter().enumerate() {
            all.push((m.off(op.from)..m.off(op.to), op.text.clone(), ci, oi));
        }
    }
    if all.iter().all(|(r, t, _, _)| r.is_empty() && t.is_empty()) {
        return None;
    }
    all.sort_by_key(|(r, _, ci, oi)| (r.start, r.end, *ci, *oi));
    let changes = ChangeSet::replace(
        text.len_bytes(),
        all.iter().map(|(r, t, _, _)| (r.clone(), t.as_str())),
    )
    .ok()?;
    let new_text = changes.apply(text).ok()?;
    let nm = Model::new(&new_text);
    // The ranges of the edits in the new text (VS Code's inverse edit operations).
    let mut new_ranges: Vec<Vec<Range<usize>>> =
        cmds.iter().map(|c| vec![0..0; c.ops.len()]).collect();
    let mut delta: isize = 0;
    for (r, t, ci, oi) in &all {
        let s = (r.start as isize + delta) as usize;
        new_ranges[*ci][*oi] = s..s + t.len();
        delta += t.len() as isize - (r.end - r.start) as isize;
    }
    let ranges: Vec<SelRange> = cmds
        .iter()
        .enumerate()
        .map(|(ci, c)| match c.rule {
            CursorRule::Tracked {
                delta,
                move_end_down,
            } => {
                let t = track(text, &changes, &Selection::single(c.sel.anchor, c.sel.head));
                let r = t.primary();
                let (mut a, mut h) = (nm.pos(r.anchor), nm.pos(r.head));
                if move_end_down {
                    if r.is_backward() {
                        a = Pos::new(a.line + 1, 0);
                    } else {
                        h = Pos::new(h.line + 1, 0);
                    }
                }
                a.col += delta;
                h.col += delta;
                SelRange::new(nm.off(a), nm.off(h))
            }
            CursorRule::Block { back } => {
                let nr = &new_ranges[ci];
                if nr.len() == 2 {
                    SelRange::new(nr[0].end, nr[1].start)
                } else if let Some(r) = nr.first() {
                    let p = nm.pos(r.end);
                    let p = nm.off(Pos::new(p.line, p.col.saturating_sub(back)));
                    SelRange::point(p)
                } else {
                    c.sel
                }
            }
        })
        .collect();
    let primary = sel.primary_index();
    Some(Out {
        changes,
        sel: Selection::new(ranges, primary),
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: true,
    })
}

/// `CommentLineAction.run`: one `LineCommentCommand` per selection; of two selections that share
/// a line, the one made later skips its first line.
pub(crate) fn line_comment(ctx: &Ctx, st: &St, ty: LineCommentType) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    let mut order: Vec<(usize, SelRange, bool)> = st
        .sel
        .ranges()
        .iter()
        .enumerate()
        .map(|(i, r)| (i, *r, false))
        .collect();
    order.sort_by_key(|(_, r, _)| (r.from(), r.to()));
    let mut prev = 0;
    for i in 1..order.len() {
        let pl = text.byte_to_line(order[prev].1.to());
        let cl = text.byte_to_line(order[i].1.from());
        if pl == cl {
            if order[prev].0 < order[i].0 {
                order[i].2 = true;
            } else {
                order[prev].2 = true;
                prev = i;
            }
        }
    }
    // Commands run in selection order so the output selection keeps the input order.
    order.sort_by_key(|(i, _, _)| *i);
    let cmds = order
        .into_iter()
        .map(|(_, r, ignore)| line_comment_cmd(&m, ctx, ty, r, ignore))
        .collect();
    run_cmds(text, &st.sel, cmds)
}

/// `BlockCommentAction.run`: one `BlockCommentCommand` per selection.
pub(crate) fn block_comment(ctx: &Ctx, st: &St) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    let cmds: Option<Vec<Cmd>> = st
        .sel
        .ranges()
        .iter()
        .map(|r| block_comment_cmd(&m, ctx, *r))
        .collect();
    run_cmds(text, &st.sel, cmds?)
}
