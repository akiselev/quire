//! Line operations: move lines, copy lines, delete all left.
//!
//! Translated from Visual Studio Code (`src/vs/editor/contrib/linesOperations/browser/
//! copyLinesCommand.ts`, `moveLinesCommand.ts` and `linesOperations.ts`, microsoft/vscode @
//! `bdce63db`).
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).
//!
//! Differences: move lines has no auto-indent (quire has no indentation rules, which is VS Code's
//! path when `indentRulesSupport` is null), and selections whose line spans touch move as one
//! block (VS Code would produce overlapping edits there).

use super::{Out, RangeEdit, St, build};
use crate::change::{Bias, ChangeSet};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::InputKind;

fn out(changes: ChangeSet, sel: Selection) -> Option<Out> {
    if changes.is_empty() {
        return None;
    }
    Some(Out {
        changes,
        sel,
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: true,
    })
}

/// VS Code `trackSelection` without `trackPreviousOnEmpty`: a cursor at its line's end stays
/// before text inserted there, any other cursor moves after; a range never grows at its edges.
pub(crate) fn track_range(text: &Text, changes: &ChangeSet, r: SelRange) -> SelRange {
    if r.is_empty() {
        let at_end = text.line_range(text.byte_to_line(r.head)).end == r.head;
        r.map(changes, if at_end { Bias::Before } else { Bias::After })
    } else {
        r.map(changes, Bias::Before)
    }
}

/// The line span of a range: the end line is dropped when a multi-line range ends at column 0.
fn line_span(text: &Text, r: &SelRange) -> (usize, usize, bool) {
    let s = text.byte_to_line(r.from());
    let e = text.byte_to_line(r.to());
    if e > s && r.to() == text.line_to_byte(e) {
        (s, e - 1, true)
    } else {
        (s, e, false)
    }
}

fn with_dir(from: usize, to: usize, backward: bool) -> SelRange {
    if backward {
        SelRange::new(to, from)
    } else {
        SelRange::new(from, to)
    }
}

/// `CopyLinesCommand` for every selection (`AbstractCopyLinesAction`).
pub(crate) fn copy_lines(st: &St, down: bool) -> Option<Out> {
    let text = &st.text;
    let ranges = st.sel.ranges();
    // Selections that would copy the same line: the earlier one wins (ranges are sorted).
    let mut ignore = vec![false; ranges.len()];
    let mut prev = 0;
    for i in 1..ranges.len() {
        if text.byte_to_line(ranges[prev].to()) == text.byte_to_line(ranges[i].from()) {
            ignore[i] = true;
        } else {
            prev = i;
        }
    }
    struct Cmd {
        tracked: SelRange,
        backward: bool,
        start_delta: usize,
        end_delta: usize,
    }
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut cmds = Vec::new();
    for (i, r) in ranges.iter().enumerate() {
        let (sl, el, dropped) = line_span(text, r);
        let mut end_delta = usize::from(dropped);
        let mut start_delta = 0;
        let (from, to) = if dropped {
            (r.from(), text.line_range(el).end)
        } else {
            (r.from(), r.to())
        };
        let src = text
            .slice(text.line_to_byte(sl)..text.line_range(el).end)
            .to_string();
        if src.is_empty() && down {
            start_delta += 1;
            end_delta += 1;
        }
        if !ignore[i] {
            if down {
                let at = text.line_to_byte(sl);
                edits.push((at..at, format!("{src}\n")));
            } else {
                let at = text.line_range(el).end;
                edits.push((at..at, format!("\n{src}")));
            }
        }
        cmds.push(Cmd {
            tracked: SelRange::new(from, to),
            backward: r.is_backward(),
            start_delta,
            end_delta,
        });
    }
    let changes = ChangeSet::replace(text.len_bytes(), edits).ok()?;
    let new_text = changes.apply(text).ok()?;
    let out_ranges: Vec<SelRange> = cmds
        .iter()
        .map(|c| {
            let t = track_range(text, &changes, c.tracked);
            let (mut from, mut to) = (t.from(), t.to());
            if c.start_delta != 0 {
                from = new_text.line_to_byte(
                    (new_text.byte_to_line(from) + c.start_delta).min(new_text.len_lines() - 1),
                );
            }
            if c.end_delta != 0 {
                to = new_text.line_to_byte(
                    (new_text.byte_to_line(to) + c.end_delta).min(new_text.len_lines() - 1),
                );
            }
            with_dir(from, to.max(from), c.backward)
        })
        .collect();
    out(changes, Selection::new(out_ranges, st.sel.primary_index()))
}

/// `MoveLinesCommand` over blocks of touching selections.
pub(crate) fn move_lines(st: &St, down: bool) -> Option<Out> {
    let text = &st.text;
    let last_line = text.len_lines() - 1;
    // Blocks of line spans: (first line, last line).
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for r in st.sel.ranges() {
        let (s, e, _) = line_span(text, r);
        match blocks.last_mut() {
            Some(b) if s <= b.1 + 1 => b.1 = b.1.max(e),
            _ => blocks.push((s, e)),
        }
    }
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    // (byte range of positions that shift, signed shift)
    let mut shifts: Vec<(usize, usize, isize)> = Vec::new();
    for &(s, e) in &blocks {
        let block_start = text.line_to_byte(s);
        let block_end = text.line_range(e).end;
        if down {
            if e == last_line {
                continue;
            }
            let moving = text.line_range(e + 1);
            let moving_text = text.slice(moving.clone()).to_string();
            edits.push((block_start..block_start, format!("{moving_text}\n")));
            edits.push((block_end..moving.end, String::new()));
            shifts.push((block_start, block_end + 1, moving_text.len() as isize + 1));
        } else {
            if s == 0 {
                continue;
            }
            let moving = text.line_range(s - 1);
            let moving_text = text.slice(moving.clone()).to_string();
            edits.push((moving.start..block_start, String::new()));
            edits.push((block_end..block_end, format!("\n{moving_text}")));
            // A position just after the block's line break (a selection ending at column 0 of
            // the next line) moves with the block too.
            let upper = if e < last_line {
                block_end + 1
            } else {
                block_end
            };
            shifts.push((block_start, upper, -(moving_text.len() as isize + 1)));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let changes = ChangeSet::replace(text.len_bytes(), edits).ok()?;
    let map = |p: usize| -> usize {
        for &(a, b, d) in &shifts {
            if p >= a && p <= b {
                return (p as isize + d) as usize;
            }
        }
        changes.map_pos(p, Bias::Before)
    };
    let sel = st.sel.transform(|r| {
        let goal = r.goal_column;
        SelRange::new(map(r.anchor), map(r.head)).with_goal(goal)
    });
    out(changes, sel)
}

/// `DeleteAllLeftAction`: delete from each cursor to its line start (at column 0: join with the
/// previous line); a selection deletes from its first line's start to its end.
pub(crate) fn delete_all_left(st: &St) -> Option<Out> {
    let text = &st.text;
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let line = text.byte_to_line(r.from());
            let ls = text.line_to_byte(line);
            let range = if r.is_empty() {
                if r.head == ls {
                    if line == 0 {
                        0..0
                    } else {
                        text.line_range(line - 1).end..ls
                    }
                } else {
                    ls..r.head
                }
            } else {
                ls..r.to()
            };
            Some(RangeEdit::replace(range, ""))
        })
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    out(changes, sel)
}

/// `DeleteAllRightAction`: delete from each cursor to its line end (at the line end: join with
/// the next line) and each selection; overlapping ranges merge; the cursors end at the starts,
/// the primary one first.
pub(crate) fn delete_all_right(st: &St) -> Option<Out> {
    let text = &st.text;
    let primary = st.sel.primary();
    let mut ranges: Vec<std::ops::Range<usize>> = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            if r.is_empty() {
                let lr = text.line_range(text.byte_to_line(r.head));
                if r.head == lr.end {
                    r.head..(lr.end + 1).min(text.len_bytes())
                } else {
                    r.head..lr.end
                }
            } else {
                r.range()
            }
        })
        .collect();
    ranges.sort_by_key(|r| (r.start, r.end));
    // Merge ranges that intersect (touching counts, as in VS Code's `Range.intersectRanges`).
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
            continue;
        }
        merged.push(r);
    }
    let changes =
        ChangeSet::replace(text.len_bytes(), merged.iter().map(|r| (r.clone(), ""))).ok()?;
    if changes.is_empty() {
        return None;
    }
    let mut cursors: Vec<SelRange> = Vec::new();
    let mut primary_cursor: Option<SelRange> = None;
    let mut removed = 0;
    for r in &merged {
        let c = SelRange::point(r.start - removed);
        removed += r.end - r.start;
        let p = primary.range();
        let intersects = r.start <= p.end && p.start <= r.end;
        if intersects && primary_cursor.is_none() {
            primary_cursor = Some(c);
        } else {
            cursors.push(c);
        }
    }
    if let Some(p) = primary_cursor {
        cursors.insert(0, p);
    }
    out(changes, Selection::new(cursors, 0))
}
