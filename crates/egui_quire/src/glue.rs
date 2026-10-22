//! Every call from the widget into `quire-core` goes through here, so the widget's view of the
//! core API is in one place (beside `widths.rs`, which follows the document's recent changes).

use std::borrow::Cow;
use std::ops::Range;

use egui::Color32;
use quire_core::{
    Applied, ChangeSet, Document, EditorCommand, EditorView, FindQuery, GroupHint, HostRequest,
    InputKind, Origin, Preedit, SelRange, Selection, Text, Transaction, UndoMode, Version,
};

pub use quire_core::Severity;

use crate::galley_cache::hash_one;
use crate::theme::EditorTheme;

/// Bring the view up to date with the document (SEAM-CHANGES item 6).
pub fn sync(doc: &Document, view: &mut EditorView) {
    view.sync(doc);
}

pub fn text(doc: &Document) -> &Text {
    doc.text()
}

pub fn version(doc: &Document) -> Version {
    doc.version()
}

/// Line `i` without its line break.
pub fn line_str(text: &Text, i: usize) -> Cow<'_, str> {
    let s = text.line(i).to_cow();
    match s {
        Cow::Borrowed(b) => Cow::Borrowed(b.strip_suffix('\n').unwrap_or(b)),
        Cow::Owned(mut o) => {
            if o.ends_with('\n') {
                o.pop();
            }
            Cow::Owned(o)
        }
    }
}

pub fn selection(view: &EditorView) -> &Selection {
    view.selection()
}

/// What running a command produced.
#[derive(Debug, Default)]
pub struct Ran {
    pub applied: Option<Applied>,
    pub request: Option<HostRequest>,
}

/// Run a command through the core (pure command → transaction, then apply). `time_ms` is the
/// clock for undo grouping.
pub fn run(
    doc: &mut Document,
    view: &mut EditorView,
    cmd: &EditorCommand,
    time_ms: u64,
) -> Result<Ran, String> {
    let ex = doc
        .execute(view, cmd, Some(time_ms))
        .map_err(|e| e.to_string())?;
    Ok(Ran {
        applied: ex.applied,
        request: ex.request,
    })
}

/// Replace `range` with `text` as one input transaction (IME surrounding delete, a11y SetValue).
pub fn replace(
    doc: &mut Document,
    view: &mut EditorView,
    range: Range<usize>,
    text: &str,
    ime: bool,
) -> Option<Applied> {
    let len = doc.text().len_bytes();
    let changes = ChangeSet::replace(len, [(range.clone(), text)]).ok()?;
    let caret = range.start + text.len();
    let tr = Transaction::new(doc.version(), changes)
        .with_selection(Selection::point(caret))
        .with_origin(Origin::Input(if ime {
            InputKind::Ime
        } else {
            InputKind::Other
        }))
        .with_group(GroupHint::Close);
    doc.apply(tr, view).ok()
}

/// Apply a selection-only transaction.
pub fn select(doc: &mut Document, view: &mut EditorView, sel: Selection) -> Option<Applied> {
    let tr = Transaction::select(doc.version(), doc.text().len_bytes(), sel)
        .with_origin(Origin::Command("setSelection".into()));
    doc.apply(tr, view).ok()
}

pub fn single(anchor: usize, head: usize) -> Selection {
    Selection::single(anchor, head)
}

pub fn set_selection_cmd(sel: Selection) -> EditorCommand {
    EditorCommand::SetSelection { selection: sel }
}

pub fn ranges(sel: &Selection) -> &[SelRange] {
    sel.ranges()
}

pub fn is_settled(doc: &Document) -> bool {
    doc.is_settled()
}

pub fn host_undo(doc: &Document) -> bool {
    matches!(doc.undo_mode(), UndoMode::Host)
}

/// The view's IME preedit as (text, active range in bytes).
pub fn preedit(view: &EditorView) -> Option<&Preedit> {
    view.preedit()
}

/// Set or clear the preedit; `active_chars` is egui's char range.
pub fn set_preedit(view: &mut EditorView, p: Option<(&str, Option<Range<usize>>)>) {
    view.set_preedit(p.map(|(text, active_chars)| {
        let to_byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(b, _)| b);
        Preedit {
            text: text.to_owned(),
            active: active_chars.map(|r| to_byte(r.start)..to_byte(r.end)),
        }
    }));
}

pub fn set_focused(view: &mut EditorView, f: bool) {
    view.set_focused(f);
}

/// Colour runs `(start_byte, colour)` of `line` (resolved from token styles at paint time), and
/// a hash of the runs for the galley key.
pub fn line_runs(
    doc: &Document,
    line: usize,
    theme: &EditorTheme,
    out: &mut Vec<(usize, Color32)>,
) -> u64 {
    out.clear();
    let tokens = doc.tokens();
    if line >= tokens.len_lines() {
        return 0;
    }
    let toks = tokens.line(line);
    if toks.is_empty() {
        return 0;
    }
    for t in toks {
        out.push((t.start as usize, theme.token_color(t.style.ty() as usize)));
    }
    hash_one(toks)
}

/// Diagnostics, all sources.
pub fn diagnostics(doc: &Document) -> impl Iterator<Item = &quire_core::Diagnostic> {
    doc.diagnostics().all().map(|(_, d)| d)
}

/// Diagnostics overlapping `range` (touching counts, so empty ranges are found).
pub fn diagnostics_at(doc: &Document, pos: usize) -> Vec<&quire_core::Diagnostic> {
    doc.diagnostics()
        .all()
        .map(|(_, d)| d)
        .filter(|d| d.range.start <= pos && pos <= d.range.end.max(d.range.start + 1))
        .collect()
}

/// Find matches of the view (sorted) and the index of the one at the primary selection.
pub fn find_matches<'v>(
    view: &'v EditorView,
    _doc: &Document,
) -> (&'v [Range<usize>], Option<usize>) {
    let Some(f) = view.find() else {
        return (&[], None);
    };
    let sel = view.selection().primary().range();
    let cur = f
        .matches
        .binary_search_by(|m| m.start.cmp(&sel.start))
        .ok()
        .filter(|&i| f.matches[i] == sel);
    (&f.matches, cur)
}

/// Find options as the bar shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

fn query(q: &str, o: FindOptions) -> FindQuery {
    FindQuery {
        search_string: q.to_owned(),
        is_regex: o.regex,
        match_case: o.case_sensitive,
        whole_word: o.whole_word,
    }
}

/// Is the view's find state out of date for this query (or absent)?
pub fn find_query_stale(view: &EditorView, q: &str, o: FindOptions) -> bool {
    match view.find() {
        None => true,
        Some(f) => f.stale || f.query != query(q, o),
    }
}

pub fn set_find(doc: &Document, view: &mut EditorView, q: &str, o: FindOptions) {
    if q.is_empty() {
        view.set_find(doc, None);
    } else {
        view.set_find(doc, Some(query(q, o)));
    }
}

pub fn refresh_find(doc: &Document, view: &mut EditorView) {
    view.refresh_find(doc);
}

pub fn clear_find(doc: &Document, view: &mut EditorView) {
    view.set_find(doc, None);
}

pub fn find_error(view: &EditorView) -> Option<String> {
    view.find()
        .and_then(|f| f.error.as_ref())
        .map(|e| e.0.clone())
}

pub fn find_next_cmd() -> EditorCommand {
    EditorCommand::FindNext
}

pub fn find_prev_cmd() -> EditorCommand {
    EditorCommand::FindPrevious
}

pub fn replace_one_cmd(r: &str) -> EditorCommand {
    EditorCommand::ReplaceOne {
        replacement: r.to_owned(),
    }
}

pub fn replace_all_cmd(r: &str) -> EditorCommand {
    EditorCommand::ReplaceAll {
        replacement: r.to_owned(),
    }
}

pub fn type_cmd(text: &str) -> EditorCommand {
    EditorCommand::Type {
        text: text.to_owned(),
    }
}

pub fn paste_cmd(text: &str) -> EditorCommand {
    EditorCommand::Paste {
        text: text.to_owned(),
        per_cursor: true,
    }
}

pub fn cut_cmd() -> EditorCommand {
    EditorCommand::Cut
}

/// Give page motions the viewport's line count.
pub fn with_page_lines(cmd: EditorCommand, page: u32) -> EditorCommand {
    use quire_core::Motion;
    match cmd {
        EditorCommand::CursorMove {
            to: Motion::PageUp { .. },
            select,
        } => EditorCommand::CursorMove {
            to: Motion::PageUp { lines: page },
            select,
        },
        EditorCommand::CursorMove {
            to: Motion::PageDown { .. },
            select,
        } => EditorCommand::CursorMove {
            to: Motion::PageDown { lines: page },
            select,
        },
        c => c,
    }
}

/// Does this command change text (refused in read-only mode)?
pub fn changes_text(cmd: &EditorCommand) -> bool {
    use EditorCommand::*;
    !matches!(
        cmd,
        CursorMove { .. }
            | SetSelection { .. }
            | SelectAll
            | ExpandLineSelection
            | RemoveSecondaryCursors
            | Save
            | Find { .. }
            | FindNext
            | FindPrevious
            | CloseFind
            | RevealLine { .. }
    )
}

/// The view's scroll in pixels, given the line height index.
pub fn scroll_px(view: &EditorView, doc: &Document, heights: &crate::HeightIndex) -> egui::Vec2 {
    let s = view.scroll();
    let t = doc.text();
    let line = t.byte_to_line(s.anchor.min(t.len_bytes()));
    egui::vec2(s.horizontal, heights.y_of(line) + s.offset)
}

pub fn set_scroll_px(
    view: &mut EditorView,
    doc: &Document,
    heights: &crate::HeightIndex,
    px: egui::Vec2,
) {
    let line = heights
        .line_at(px.y)
        .min(doc.text().len_lines().saturating_sub(1));
    let s = quire_core::Scroll {
        anchor: doc.text().line_to_byte(line),
        offset: px.y - heights.y_of(line),
        horizontal: px.x,
    };
    if view.scroll() != s {
        view.set_scroll(s);
    }
}

/// A reveal request left by a transaction effect: (byte pos, center).
pub fn take_reveal(view: &mut EditorView, doc: &Document) -> Option<(usize, bool)> {
    view.take_reveal().map(|r| match r {
        quire_core::Reveal::Pos { pos, center } => (pos, center),
        quire_core::Reveal::Line(l) => (
            doc.text()
                .line_to_byte(l.min(doc.text().len_lines().saturating_sub(1))),
            true,
        ),
    })
}

/// The word at `pos` as double-click selects it.
pub fn word_range_at(doc: &Document, pos: usize) -> Range<usize> {
    let cls = quire_core::words::WordClassifier::new(&doc.config().word_separators);
    quire_core::words::word_range_at(doc.text(), pos, &cls)
}

/// The line at `pos` with its line feed, as triple-click selects it.
pub fn line_range_at(doc: &Document, pos: usize) -> Range<usize> {
    quire_core::words::line_range_at(doc.text(), pos.min(doc.text().len_bytes()))
}
