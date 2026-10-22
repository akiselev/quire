//! Fold ranges: from indentation (core's fallback provider) or from a syntax provider
//! ([`Document::set_fold_ranges`](crate::Document::set_fold_ranges)), and the line mapping that
//! keeps them valid across edits until fresh ones arrive.
//!
//! [`indent_fold_ranges`] is translated from Visual Studio Code
//! (`src/vs/editor/contrib/folding/browser/indentRangeProvider.ts` `computeRanges` without
//! markers, offSide = false; `src/vs/editor/common/model/utils.ts` `computeIndentLevel`;
//! microsoft/vscode @ `bdce63db`).
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).

use crate::recent::LineEdit;
use crate::syntax::{FoldKind, FoldRange};
use crate::text::Text;

/// VS Code `computeIndentLevel`: the visual indentation, or `None` for a blank line.
fn indent_level(text: &Text, line: usize, tab_size: u32) -> Option<u32> {
    let tab = tab_size.max(1);
    let mut indent = 0u32;
    for c in text.line(line).chars() {
        match c {
            ' ' => indent += 1,
            '\t' => indent = indent - indent % tab + tab,
            _ => return Some(indent),
        }
    }
    None
}

/// Fold ranges from indentation: a line followed by more-indented lines folds them, through the
/// blank lines before the next line at its indentation or less (VS Code's rule). Sorted by start
/// line, outer before inner.
pub fn indent_fold_ranges(text: &Text, tab_size: u32) -> Vec<FoldRange> {
    struct Prev {
        indent: i64,
        end_above: usize,
    }
    let count = text.len_lines();
    let mut prev = vec![Prev {
        indent: -1,
        end_above: count,
    }];
    let mut out: Vec<FoldRange> = Vec::new();
    for line in (0..count).rev() {
        let Some(indent) = indent_level(text, line, tab_size) else {
            continue;
        };
        let indent = i64::from(indent);
        let mut top = prev.len() - 1;
        if prev[top].indent > indent {
            while prev[top].indent > indent {
                prev.pop();
                top -= 1;
            }
            let end = prev[top].end_above - 1;
            if end > line {
                out.push(FoldRange {
                    start_line: line,
                    end_line: end,
                    kind: FoldKind::Other,
                });
            }
        }
        if prev[top].indent == indent {
            prev[top].end_above = line;
        } else {
            prev.push(Prev {
                indent,
                end_above: line,
            });
        }
    }
    out.reverse();
    out
}

/// Maps fold ranges through line edits (old-text lines to new-text lines). A range whose header
/// line was replaced is dropped; an end inside the replaced lines moves to the last new line of
/// the edit.
pub fn map_fold_ranges(ranges: &mut Vec<FoldRange>, edits: &[LineEdit]) {
    for e in edits.iter().rev() {
        let removed = e.end_line - e.start_line;
        let delta = e.inserted_lines as isize - removed as isize;
        let shift = |l: usize| (l as isize + delta).max(0) as usize;
        ranges.retain_mut(|r| {
            // Lines inserted at the very start of the header line push it down whole.
            let pushed = removed == 0 && e.start_col == 0 && r.start_line == e.start_line;
            if r.start_line > e.end_line || pushed {
                r.start_line = shift(r.start_line);
            } else if r.start_line >= e.start_line && (removed > 0 || e.inserted_lines > 0) {
                // The header line itself was split or joined.
                if r.start_line > e.start_line || removed > 0 {
                    return false;
                }
            }
            if r.end_line > e.end_line {
                r.end_line = shift(r.end_line);
            } else if r.end_line >= e.start_line {
                r.end_line = e.start_line + e.inserted_lines;
            }
            r.end_line > r.start_line
        });
    }
}

/// The byte range a fold hides: from the end of its header line to the end of its last line.
pub fn fold_bytes(text: &Text, r: &FoldRange) -> std::ops::Range<usize> {
    let last = text.len_lines() - 1;
    text.line_range(r.start_line.min(last)).end..text.line_range(r.end_line.min(last)).end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(r: &[FoldRange]) -> Vec<(usize, usize)> {
        r.iter().map(|r| (r.start_line, r.end_line)).collect()
    }

    #[test]
    fn indentation_ranges() {
        let t = Text::from_file("a\n  b\n    c\n\n  d\ne\n  f\n\n");
        assert_eq!(
            lines(&indent_fold_ranges(&t, 4)),
            vec![(0, 4), (1, 3), (5, 8)]
        );
        let t = Text::from_file("x\n\ty\n\t\tz\nw");
        assert_eq!(lines(&indent_fold_ranges(&t, 4)), vec![(0, 2), (1, 2)]);
    }

    #[test]
    fn map_through_line_edits() {
        let t = Text::from_file("a\n  b\n  c\nd\n  e\n");
        let mut r = indent_fold_ranges(&t, 4);
        assert_eq!(lines(&r), vec![(0, 2), (3, 5)]);
        // Insert a line at the start: everything shifts by one.
        let c = crate::ChangeSet::insert_at(t.len_bytes(), 0, "new\n").unwrap();
        map_fold_ranges(&mut r, &LineEdit::compute(&c, &t));
        assert_eq!(lines(&r), vec![(1, 3), (4, 6)]);
    }
}
