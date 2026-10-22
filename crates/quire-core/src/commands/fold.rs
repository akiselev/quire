//! Fold commands (VS Code `editor.fold`, `editor.unfold`, `editor.toggleFold`, `editor.foldAll`,
//! `editor.unfoldAll` with their default of one level). Cursors inside a range that gets folded
//! move to its header line's end, as VS Code does.

use crate::document::Document;
use crate::folding::fold_bytes;
use crate::selection::SelRange;
use crate::transaction::{Effect, Transaction};
use crate::view::EditorView;
use std::ops::Range;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Op {
    Fold,
    Unfold,
    Toggle,
    FoldAll,
    UnfoldAll,
}

pub(crate) fn run(doc: &Document, view: &EditorView, op: Op) -> Option<Transaction> {
    let text = doc.text();
    let ranges = doc.fold_ranges();
    let mut folds: Vec<Range<usize>> = view.folds().to_vec();
    match op {
        Op::FoldAll => {
            folds = ranges.iter().map(|r| fold_bytes(text, r)).collect();
        }
        Op::UnfoldAll => folds.clear(),
        _ => {
            let mut lines: Vec<usize> = view
                .selection()
                .ranges()
                .iter()
                .map(|r| text.byte_to_line(r.head))
                .collect();
            lines.dedup();
            for line in lines {
                // Ranges containing the line, innermost (latest start, then smallest) first.
                let mut at: Vec<Range<usize>> = ranges
                    .iter()
                    .filter(|r| r.start_line <= line && line <= r.end_line)
                    .map(|r| fold_bytes(text, r))
                    .collect();
                at.sort_by_key(|r| (std::cmp::Reverse(r.start), r.end));
                let folded = |f: &Vec<Range<usize>>, r: &Range<usize>| f.contains(r);
                let fold_one = at.iter().find(|r| !folded(&folds, r)).cloned();
                let unfold_one = at.iter().find(|r| folded(&folds, r)).cloned();
                let do_fold = match op {
                    Op::Fold => true,
                    Op::Unfold => false,
                    _ => at.first().is_some_and(|r| !folded(&folds, r)),
                };
                if do_fold {
                    if let Some(r) = fold_one {
                        folds.push(r);
                    }
                } else if let Some(r) = unfold_one {
                    folds.retain(|f| *f != r);
                }
            }
        }
    }
    folds.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    folds.dedup();
    if folds == view.folds() {
        return None;
    }
    // Move positions out of hidden text to the fold start (the header line's end).
    let hide = |p: usize| {
        folds
            .iter()
            .filter(|f| p > f.start && p <= f.end)
            .map(|f| f.start)
            .min()
            .unwrap_or(p)
    };
    let sel = view
        .selection()
        .transform(|r| SelRange::new(hide(r.anchor), hide(r.head)));
    Some(
        Transaction::select(doc.version(), text.len_bytes(), sel)
            .with_effect(Effect::SetFolds(folds)),
    )
}
