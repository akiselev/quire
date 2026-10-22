//! Line stepping over folds and `expandLineSelection`; the cursor motions themselves are
//! quire-text's (`quire_text::ops::motion`), which `cursorMove` lowers to.

use crate::selection::{SelRange, Selection};
use crate::text::Text;

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
