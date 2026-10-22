//! Building blocks for editors built on quire-text: line/column helpers ([`model`]), the per-range
//! edit builder ([`build`]), motions ([`motion`]) and the edits field commands are made of
//! ([`edit`]). quire-core's richer commands use them; fields use
//! [`TextDocument::execute`](crate::TextDocument::execute).

pub mod edit;
pub mod model;
pub mod motion;

use crate::change::{Bias, ChangeSet};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use std::ops::Range;

/// An edit for one selection range; `anchor`/`head` are relative to the edit's start in the new
/// text.
#[derive(Clone, Debug)]
pub struct RangeEdit {
    /// The replaced range (old text).
    pub range: Range<usize>,
    /// The inserted text.
    pub insert: String,
    /// The anchor after the edit, relative to the edit's start.
    pub anchor: usize,
    /// The head after the edit, relative to the edit's start.
    pub head: usize,
}

impl RangeEdit {
    /// Replace `range` with `insert`, cursor after the insert.
    pub fn replace(range: Range<usize>, insert: impl Into<String>) -> Self {
        let insert = insert.into();
        let n = insert.len();
        RangeEdit {
            range,
            insert,
            anchor: n,
            head: n,
        }
    }
}

/// Builds a change set from per-range edits (`None`: the range has no edit and is mapped).
/// Overlapping deletions are merged.
pub fn build(
    text: &Text,
    sel: &Selection,
    edits: Vec<Option<RangeEdit>>,
) -> (ChangeSet, Selection) {
    let mut idx: Vec<usize> = (0..edits.len()).filter(|&i| edits[i].is_some()).collect();
    idx.sort_by_key(|&i| edits[i].as_ref().map(|e| (e.range.start, e.range.end)));
    // Merge overlapping edits (keep the first's insert, union the ranges).
    let mut merged: Vec<(Range<usize>, String, Vec<usize>)> = Vec::new();
    for &i in &idx {
        let e = edits[i].as_ref().expect("edit");
        if let Some(last) = merged.last_mut()
            && (e.range.start < last.0.end
                || (e.range.start == last.0.start
                    && e.range.start == e.range.end
                    && last.0.start == last.0.end))
        {
            last.0.end = last.0.end.max(e.range.end);
            last.2.push(i);
            continue;
        }
        merged.push((e.range.clone(), e.insert.clone(), vec![i]));
    }
    let changes = ChangeSet::replace(
        text.len_bytes(),
        merged.iter().map(|(r, s, _)| (r.clone(), s.as_str())),
    )
    .expect("command edits are sorted and in bounds");
    let mut new_ranges: Vec<Option<SelRange>> = vec![None; edits.len()];
    let mut delta: isize = 0;
    for (r, s, members) in &merged {
        let new_start = (r.start as isize + delta) as usize;
        for &i in members {
            let e = edits[i].as_ref().expect("edit");
            let (a, h) = if members.len() == 1 {
                (e.anchor, e.head)
            } else {
                (0, 0)
            };
            new_ranges[i] = Some(SelRange::new(
                new_start + a.min(s.len()),
                new_start + h.min(s.len()),
            ));
        }
        delta += s.len() as isize - (r.end - r.start) as isize;
    }
    let ranges: Vec<SelRange> = sel
        .ranges()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            new_ranges
                .get(i)
                .copied()
                .flatten()
                .unwrap_or_else(|| r.map(&changes, Bias::Before))
        })
        .collect();
    (changes, Selection::new(ranges, sel.primary_index()))
}

/// Maps a selection the way VS Code's `trackSelection` does: a cursor at a line end stays before
/// text inserted at it, any other cursor moves after; a non-empty range never grows at its edges.
pub fn track(text: &Text, changes: &ChangeSet, sel: &Selection) -> Selection {
    sel.transform(|r| {
        if r.is_empty() {
            let line = text.byte_to_line(r.head);
            let at_end = text.line_range(line).end == r.head;
            r.map(changes, if at_end { Bias::Before } else { Bias::After })
        } else {
            r.map(changes, Bias::Before)
        }
    })
}
