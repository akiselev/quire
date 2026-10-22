//! Find and replace commands.

use super::track;
use crate::change::ChangeSet;
use crate::command::CommandOutput;
use crate::document::{Document, Refusal};
use crate::find::{FindQuery, expand_replacement, find_all};
use crate::selection::Selection;
use crate::transaction::{Effect, Origin, Transaction};
use crate::view::EditorView;
use std::ops::Range;

fn matches(doc: &Document, view: &EditorView) -> Option<(FindQuery, Vec<Range<usize>>)> {
    let f = view.find()?;
    if !f.stale && f.version == doc.version() && f.error.is_none() {
        return Some((f.query.clone(), f.matches.clone()));
    }
    let m = find_all(&f.query, doc.text(), &doc.config().word_separators).ok()?;
    Some((f.query.clone(), m))
}

fn select_tr(doc: &Document, r: Range<usize>) -> Transaction {
    Transaction::select(
        doc.version(),
        doc.text().len_bytes(),
        Selection::single(r.start, r.end),
    )
    .with_effect(Effect::ScrollIntoView {
        pos: r.start,
        center: true,
    })
}

fn next_from(ms: &[Range<usize>], pos: usize) -> Option<Range<usize>> {
    ms.iter().find(|m| m.start >= pos).or(ms.first()).cloned()
}

fn prev_from(ms: &[Range<usize>], pos: usize) -> Option<Range<usize>> {
    ms.iter()
        .rev()
        .find(|m| m.end <= pos)
        .or(ms.last())
        .cloned()
}

pub(crate) fn find(
    doc: &Document,
    view: &EditorView,
    q: FindQuery,
) -> Result<CommandOutput, Refusal> {
    let ms = find_all(&q, doc.text(), &doc.config().word_separators)
        .map_err(|e| Refusal::InvalidQuery(e.0))?;
    let from = view.selection().primary().from();
    let tr = match next_from(&ms, from) {
        Some(m) => select_tr(doc, m),
        None => Transaction::new(doc.version(), ChangeSet::identity(doc.text().len_bytes())),
    };
    Ok(CommandOutput::Transaction(
        tr.with_effect(Effect::SetFind(Some(q))),
    ))
}

pub(crate) fn find_step(
    doc: &Document,
    view: &EditorView,
    forward: bool,
) -> Result<CommandOutput, Refusal> {
    let Some((_, ms)) = matches(doc, view) else {
        return Ok(CommandOutput::Nothing);
    };
    let p = view.selection().primary();
    let m = if forward {
        next_from(&ms, if p.is_empty() { p.head } else { p.to() })
    } else {
        prev_from(&ms, p.from())
    };
    Ok(match m {
        Some(m) => CommandOutput::Transaction(select_tr(doc, m)),
        None => CommandOutput::Nothing,
    })
}

pub(crate) fn replace_one(
    doc: &Document,
    view: &EditorView,
    replacement: &str,
) -> Result<CommandOutput, Refusal> {
    let Some((q, ms)) = matches(doc, view) else {
        return Ok(CommandOutput::Nothing);
    };
    let p = view.selection().primary();
    let Some(cur) = ms
        .iter()
        .find(|m| m.start == p.from() && m.end == p.to())
        .cloned()
    else {
        return find_step(doc, view, true);
    };
    let text = doc.text();
    let rep = expand_replacement(&q, text, cur.clone(), replacement)
        .map_err(|e| Refusal::InvalidQuery(e.0))?;
    let changes = ChangeSet::replace(text.len_bytes(), [(cur.clone(), rep.as_str())])
        .map_err(Refusal::Rebase)?;
    let new_text = changes.apply(text).map_err(Refusal::Rebase)?;
    let after = cur.start + rep.len();
    let new_ms = find_all(&q, &new_text, &doc.config().word_separators).unwrap_or_default();
    let sel = match next_from(&new_ms, after) {
        Some(m) => Selection::single(m.start, m.end),
        None => Selection::point(after),
    };
    Ok(CommandOutput::Transaction(
        Transaction::new(doc.version(), changes)
            .with_selection(sel)
            .with_origin(Origin::Command("editor.action.replaceOne".into())),
    ))
}

pub(crate) fn replace_all(
    doc: &Document,
    view: &EditorView,
    replacement: &str,
) -> Result<CommandOutput, Refusal> {
    let Some((q, ms)) = matches(doc, view) else {
        return Ok(CommandOutput::Nothing);
    };
    if ms.is_empty() {
        return Ok(CommandOutput::Nothing);
    }
    let text = doc.text();
    let mut edits = Vec::with_capacity(ms.len());
    for m in &ms {
        let rep = expand_replacement(&q, text, m.clone(), replacement)
            .map_err(|e| Refusal::InvalidQuery(e.0))?;
        edits.push((m.clone(), rep));
    }
    let changes = ChangeSet::replace(text.len_bytes(), edits).map_err(Refusal::Rebase)?;
    let sel = track(text, &changes, view.selection());
    Ok(CommandOutput::Transaction(
        Transaction::new(doc.version(), changes)
            .with_selection(sel)
            .with_origin(Origin::Command("editor.action.replaceAll".into())),
    ))
}
