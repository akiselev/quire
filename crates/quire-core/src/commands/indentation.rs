//! The indentation contrib: convert indentation to spaces or tabs, re-indent lines.
//!
//! Translated from Visual Studio Code (microsoft/vscode @ `bdce63db`), files
//! `src/vs/editor/contrib/indentation/browser/indentation.ts` (`IndentationToSpacesCommand`,
//! `IndentationToTabsCommand`, `getIndentationEditOperations`),
//! `src/vs/editor/contrib/indentation/common/indentation.ts` (`getReindentEditOperations`) and the
//! marker rule of `src/vs/editor/common/model/intervalTree.ts` (`nodeAcceptEdit`), which decides
//! where the tracked selections land.
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).
//!
//! Differences: quire has no tokens, so re-indent does not skip lines that start inside a string
//! (`doesLineStartWithString`) and the indentation rules see raw lines.

use super::edit::normalize_indentation;
use super::{Out, St};
use crate::change::ChangeSet;
use crate::config::EditorConfig;
use crate::lang;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::InputKind;
use std::ops::Range;

/// One replacement in old-text offsets.
type Edit = (Range<usize>, String);

/// `adjustMarkerBeforeColumn`.
#[derive(Copy, Clone, PartialEq)]
enum Move {
    MarkerDefined,
    ForceMove,
    ForceStay,
}

fn adjust_before(marker: usize, stick_prev: bool, check: usize, m: Move) -> bool {
    if marker < check {
        return true;
    }
    if marker > check {
        return false;
    }
    match m {
        Move::ForceMove => false,
        Move::ForceStay => true,
        Move::MarkerDefined => stick_prev,
    }
}

/// `nodeAcceptEdit` for one tracked range `(start, end)` and one edit replacing `start..end` of
/// the old text with `ins` bytes.
fn accept_edit(
    node: (usize, usize),
    stick: (bool, bool),
    edit: (usize, usize, usize),
    force_move: bool,
) -> (usize, usize) {
    let (start, end, ins) = edit;
    let deleting = end - start;
    let common = deleting.min(ins);
    let (mut ns, mut ne) = node;
    let (mut s_done, mut e_done) = (false, false);
    let m = if force_move {
        Move::ForceMove
    } else if deleting > 0 {
        Move::ForceStay
    } else {
        Move::MarkerDefined
    };
    if adjust_before(ns, stick.0, start, m) {
        s_done = true;
    }
    if adjust_before(ne, stick.1, start, m) {
        e_done = true;
    }
    if common > 0 && !force_move {
        let m = if deleting > ins {
            Move::ForceStay
        } else {
            Move::MarkerDefined
        };
        if !s_done && adjust_before(ns, stick.0, start + common, m) {
            s_done = true;
        }
        if !e_done && adjust_before(ne, stick.1, start + common, m) {
            e_done = true;
        }
    }
    let m = if force_move {
        Move::ForceMove
    } else {
        Move::MarkerDefined
    };
    if !s_done && adjust_before(ns, stick.0, end, m) {
        ns = start + ins;
        s_done = true;
    }
    if !e_done && adjust_before(ne, stick.1, end, m) {
        ne = start + ins;
        e_done = true;
    }
    if !s_done {
        ns = (ns + ins).saturating_sub(deleting);
    }
    if !e_done {
        ne = (ne + ins).saturating_sub(deleting);
    }
    (ns, ne.max(ns))
}

/// Maps a selection range through sorted, disjoint edits as VS Code's tracked selections move:
/// `trackSelection` stickiness for commands, `forceMoveMarkers` for `replaceMove` edits.
fn track(text: &Text, edits: &[Edit], r: SelRange, force_move: bool) -> SelRange {
    let stick = if r.is_empty() {
        // `trackSelection` without `trackPreviousOnEmpty`: a cursor at its line's end grows only
        // when typing before it.
        let at_end = text.line_range(text.byte_to_line(r.head)).end == r.head;
        (at_end, at_end)
    } else {
        (false, true)
    };
    let mut node = (r.from(), r.to());
    // VS Code applies the content changes from the last to the first.
    for (range, s) in edits.iter().rev() {
        node = accept_edit(node, stick, (range.start, range.end, s.len()), force_move);
    }
    if r.anchor <= r.head {
        SelRange::new(node.0, node.1)
    } else {
        SelRange::new(node.1, node.0)
    }
}

fn out(text: &Text, sel: &Selection, edits: Vec<Edit>, force_move: bool) -> Option<Out> {
    if edits.is_empty() {
        return None;
    }
    let changes = ChangeSet::replace(
        text.len_bytes(),
        edits.iter().map(|(r, s)| (r.clone(), s.as_str())),
    )
    .expect("indentation edits are sorted and in bounds");
    let ranges: Vec<SelRange> = sel
        .ranges()
        .iter()
        .map(|r| track(text, &edits, *r, force_move))
        .collect();
    Some(Out {
        changes,
        sel: Selection::new(ranges, sel.primary_index()),
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: true,
    })
}

/// `getIndentationEditOperations`: each line's indentation (the whole line when it is blank)
/// with every tab replaced by `tab_size` spaces, or every run of `tab_size` spaces by a tab.
pub(crate) fn indentation_to(st: &St, tab_size: u32, to_spaces: bool) -> Option<Out> {
    let text = &st.text;
    if text.is_empty() {
        return None;
    }
    let spaces = " ".repeat(tab_size as usize);
    let mut edits = Vec::new();
    for l in 0..text.len_lines() {
        let line = text.line(l).to_string();
        let ws = line.find(|c| c != ' ' && c != '\t').unwrap_or(line.len());
        if ws == 0 {
            continue;
        }
        let old = &line[..ws];
        let new = if to_spaces {
            old.replace('\t', &spaces)
        } else if spaces.is_empty() {
            // `new RegExp('', 'gi')` matches between every character.
            let mut s = String::from("\t");
            for c in old.chars() {
                s.push(c);
                s.push('\t');
            }
            s
        } else {
            old.replace(&spaces, "\t")
        };
        if new != old {
            let start = text.line_to_byte(l);
            edits.push((start..start + ws, new));
        }
    }
    out(text, &st.sel, edits, false)
}

/// `getReindentEditOperations` over the whole document (`editor.action.reindentlines`).
pub(crate) fn reindent_lines(cfg: &EditorConfig, st: &St) -> Option<Out> {
    let text = &st.text;
    if text.is_empty() || cfg.indentation_rules.is_none() {
        return None;
    }
    let edits = reindent_edits(cfg, text, 0, text.len_lines() - 1);
    out(text, &st.sel, edits, true)
}

/// `getReindentEditOperations(model, start, end)`, 0-based inclusive lines.
fn reindent_edits(cfg: &EditorConfig, text: &Text, start: usize, end: usize) -> Vec<Edit> {
    let line = |l: usize| text.line(l).to_string();
    // `getProcessedLine(lineNumber, newIndentation)`: the line with its indentation replaced.
    let with_indent = |l: usize, indent: Option<&str>| {
        let t = line(l);
        match indent {
            None => t,
            Some(i) => format!("{i}{}", &t[lang::leading_ws(&t).len()..]),
        }
    };
    let ignore = |l: usize| lang::should_ignore(cfg, &line(l));
    let end = end.min(text.len_lines() - 1);
    let mut start = start;
    // Skip `unIndentedLinePattern` lines.
    while start <= end {
        if !ignore(start) {
            break;
        }
        start += 1;
    }
    if start + 1 > end {
        return Vec::new();
    }
    let shift = |i: &str| lang::shift_indent(cfg, i);
    let unshift = |i: &str| lang::unshift_indent(cfg, i);
    let mut edits = Vec::new();

    // The first line's indentation is the base.
    let current = line(start);
    let mut global_indent = lang::leading_ws(&current).to_string();
    // Differs from `global_indent` after a line matching `indentNextLinePattern`.
    let mut ideal_next = global_indent.clone();
    if lang::should_increase(cfg, &current) {
        ideal_next = shift(&ideal_next);
        global_indent = shift(&global_indent);
    } else if lang::should_indent_next_line(cfg, &current) {
        ideal_next = shift(&ideal_next);
    }
    for l in start + 1..=end {
        let t = line(l);
        let old_indent = lang::leading_ws(&t);
        let current_ideal = ideal_next.clone();
        if lang::should_decrease(cfg, &with_indent(l, Some(&current_ideal))) {
            ideal_next = unshift(&ideal_next);
            global_indent = unshift(&global_indent);
        }
        if old_indent != ideal_next {
            let s = text.line_to_byte(l);
            edits.push((
                s..s + old_indent.len(),
                normalize_indentation(cfg, &ideal_next),
            ));
        }
        // The next line's ideal indentation.
        if lang::should_ignore(cfg, &t) {
            // An `unIndentedLinePattern` line inherits from above and changes nothing.
            continue;
        } else if lang::should_increase(cfg, &with_indent(l, Some(&current_ideal))) {
            global_indent = shift(&global_indent);
            ideal_next = global_indent.clone();
        } else if lang::should_indent_next_line(cfg, &with_indent(l, Some(&current_ideal))) {
            ideal_next = shift(&ideal_next);
        } else {
            ideal_next = global_indent.clone();
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_edit_follows_interval_tree() {
        // A cursor at the end of a replaced range that grows stays inside the common part.
        assert_eq!(accept_edit((3, 3), (true, true), (0, 3, 6), false), (3, 3));
        // After the edit: shifted.
        assert_eq!(
            accept_edit((4, 4), (false, false), (0, 2, 6), false),
            (8, 8)
        );
        // At the edit start: stays (deletion), or moves with forceMoveMarkers.
        assert_eq!(
            accept_edit((0, 0), (false, false), (0, 2, 6), false),
            (0, 0)
        );
        assert_eq!(accept_edit((0, 0), (false, false), (0, 2, 6), true), (6, 6));
        // Pure insertion before a cursor at the same offset.
        assert_eq!(
            accept_edit((0, 0), (false, false), (0, 0, 4), false),
            (4, 4)
        );
        assert_eq!(accept_edit((0, 0), (true, true), (0, 0, 4), false), (0, 0));
    }
}
