//! Command → transaction (pure). See [`crate::command`].
//!
//! Behaviour follows VS Code's cursor and typing operations (`cursorMoveOperations.ts`,
//! `cursorWordOperations.ts`, `cursorTypeEditOperations.ts`, `cursorDeleteOperations.ts`,
//! `shiftCommand.ts`, `lineCommentCommand.ts`); the word operations are a literal translation in
//! [`words`] (MIT notice there). Differences, chosen to fit quire's design:
//! - no language service: Enter indents after an opening bracket from `EditorConfig::brackets`
//!   and splits `{|}` onto three lines (VS Code's default bracket `onEnter` behaviour); Tab on a
//!   blank line uses the previous non-blank line's indentation the same way;
//! - multi-char `type` text runs through the typing interceptors char by char (VS Code's
//!   keyboard path), composed into one transaction;
//! - auto-closing has no tokenizer: quotes never auto-close after a word character, brackets
//!   only before whitespace, a closing character or `auto_close_before`; no bracket-balance check.

mod edit;
mod find;
mod fold;
mod lines;
pub(crate) mod model;
mod motion;
mod multi;
pub(crate) mod words;

use crate::change::{Bias, ChangeSet};
use crate::command::{CommandOutput, EditorCommand, HostRequest};
use crate::config::EditorConfig;
use crate::document::{Document, Refusal};
use crate::history::UndoMode;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::{Effect, GroupHint, InputKind, Origin, Transaction};
use crate::view::{AutoClosed, EditorView};
use crate::words::WordClassifier;
use std::ops::Range;

/// What the pure command functions see: a text, a selection and the view's tracked auto-closed
/// characters. Multi-char typing steps through successive states.
#[derive(Clone)]
pub(crate) struct St {
    pub text: Text,
    pub sel: Selection,
    pub auto_closed: Vec<AutoClosed>,
    pub last_kind: Option<InputKind>,
}

/// One edit result.
pub(crate) struct Out {
    pub changes: ChangeSet,
    pub sel: Selection,
    pub kind: InputKind,
    pub auto_closed: Vec<AutoClosed>,
    /// Close the open undo group before this edit (VS Code `shouldPushStackElementBefore`).
    pub close_before: bool,
}

/// An edit for one selection range; `anchor`/`head` are relative to the edit's start in the new
/// text.
pub(crate) struct RangeEdit {
    pub range: Range<usize>,
    pub insert: String,
    pub anchor: usize,
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
pub(crate) fn build(
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
pub(crate) fn track(text: &Text, changes: &ChangeSet, sel: &Selection) -> Selection {
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

pub(crate) struct Ctx<'a> {
    pub cfg: &'a EditorConfig,
    pub cls: WordClassifier,
    /// The view's folds: vertical motion skips hidden lines.
    pub folds: &'a [Range<usize>],
}

fn tr_from(doc: &Document, changes: ChangeSet, sel: Selection, origin: Origin) -> Transaction {
    Transaction::new(doc.version(), changes)
        .with_selection(sel)
        .with_origin(origin)
}

fn cmd_origin(cmd: &EditorCommand) -> Origin {
    Origin::Command(std::borrow::Cow::Borrowed(cmd.id()))
}

pub(crate) fn run(
    doc: &Document,
    view: &EditorView,
    cmd: &EditorCommand,
) -> Result<CommandOutput, Refusal> {
    use EditorCommand as C;
    let cfg = doc.config();
    let ctx = Ctx {
        cfg,
        cls: WordClassifier::new(&cfg.word_separators),
        folds: view.folds(),
    };
    let text = doc.text();
    let sel = view.selection();
    let st = || St {
        text: text.clone(),
        sel: sel.clone(),
        auto_closed: view.auto_closed().to_vec(),
        last_kind: doc.history().last_input_kind(),
    };
    let select = |s: Selection| -> Result<CommandOutput, Refusal> {
        Ok(CommandOutput::Transaction(
            Transaction::select(doc.version(), text.len_bytes(), s).with_effect(
                Effect::ScrollIntoView {
                    pos: 0,
                    center: false,
                },
            ),
        )
        .with_reveal_primary())
    };
    let edit_out = |o: Option<Out>, origin: Origin| -> Result<CommandOutput, Refusal> {
        Ok(match o {
            None => CommandOutput::Nothing,
            Some(o) => {
                let mut tr = tr_from(doc, o.changes, o.sel, origin);
                if o.close_before {
                    tr = tr.with_group(GroupHint::Close);
                }
                if !o.auto_closed.is_empty() {
                    tr = tr.with_effect(Effect::AutoClosed(o.auto_closed));
                }
                CommandOutput::Transaction(tr.with_effect(Effect::ScrollIntoView {
                    pos: 0,
                    center: false,
                }))
                .with_reveal_primary()
            }
        })
    };
    match cmd {
        C::CursorMove { to, select: ext } => {
            select(motion::cursor_move(&ctx, text, sel, *to, *ext))
        }
        C::SetSelection { selection } => {
            let s = selection.clamped(text);
            select(s)
        }
        C::SelectAll => select(Selection::single(0, text.len_bytes())),
        C::ExpandLineSelection => select(motion::expand_line_selection(text, sel)),
        C::RemoveSecondaryCursors => select(sel.clone().into_primary()),
        C::Type { text: typed } => {
            if typed.is_empty() {
                return Ok(CommandOutput::Nothing);
            }
            let typed = crate::text::normalize_line_endings(typed);
            let out = edit::type_text(&ctx, st(), &typed);
            let kind = out.as_ref().map(|o| o.kind);
            let r = edit_out(out, Origin::Input(kind.unwrap_or(InputKind::Typing)))?;
            Ok(r)
        }
        C::Paste {
            text: t,
            per_cursor,
        } => {
            let t = crate::text::normalize_line_endings(t);
            edit_out(
                edit::paste(&st(), &t, *per_cursor),
                Origin::Input(InputKind::Paste),
            )
        }
        C::Cut => edit_out(edit::cut(&ctx, &st()), Origin::Input(InputKind::Other)),
        C::DeleteLeft => edit_out(
            edit::delete_left(&ctx, &st()),
            Origin::Input(InputKind::DeletingLeft),
        ),
        C::DeleteRight => edit_out(
            edit::delete_right(&st()),
            Origin::Input(InputKind::DeletingRight),
        ),
        C::DeleteWordLeft | C::DeleteWordStartLeft | C::DeleteWordEndLeft => {
            let (nav, ws) = match cmd {
                C::DeleteWordLeft => (words::WordNav::WordStart, true),
                C::DeleteWordStartLeft => (words::WordNav::WordStart, false),
                _ => (words::WordNav::WordEnd, false),
            };
            edit_out(
                edit::delete_word(&ctx, &st(), true, nav, ws),
                cmd_origin(cmd),
            )
        }
        C::DeleteWordRight | C::DeleteWordStartRight | C::DeleteWordEndRight => {
            let (nav, ws) = match cmd {
                C::DeleteWordRight => (words::WordNav::WordEnd, true),
                C::DeleteWordStartRight => (words::WordNav::WordStart, false),
                _ => (words::WordNav::WordEnd, false),
            };
            edit_out(
                edit::delete_word(&ctx, &st(), false, nav, ws),
                cmd_origin(cmd),
            )
        }
        C::Tab => edit_out(edit::tab(&ctx, &st()), cmd_origin(cmd)),
        C::Outdent | C::OutdentLines => edit_out(edit::shift(&ctx, &st(), true), cmd_origin(cmd)),
        C::IndentLines => edit_out(edit::shift(&ctx, &st(), false), cmd_origin(cmd)),
        C::CommentLine => edit_out(edit::comment_line(&ctx, &st()), cmd_origin(cmd)),
        C::InsertLineAfter => edit_out(edit::insert_line(&ctx, &st(), true), cmd_origin(cmd)),
        C::InsertLineBefore => edit_out(edit::insert_line(&ctx, &st(), false), cmd_origin(cmd)),
        C::DeleteLines => edit_out(edit::delete_lines(&st()), cmd_origin(cmd)),
        C::InsertCursorAbove | C::InsertCursorBelow => sel_out(
            doc,
            multi::insert_cursor(&ctx, text, sel, matches!(cmd, C::InsertCursorBelow)),
        ),
        C::AddNextMatch => match multi::add_next_match(&ctx, text, view) {
            Some(o) => sel_out(doc, o),
            None => Ok(CommandOutput::Nothing),
        },
        C::ColumnSelectLeft
        | C::ColumnSelectRight
        | C::ColumnSelectUp
        | C::ColumnSelectDown
        | C::ColumnSelectPageUp { .. }
        | C::ColumnSelectPageDown { .. } => {
            let dir = match cmd {
                C::ColumnSelectLeft => multi::ColumnDir::Left,
                C::ColumnSelectRight => multi::ColumnDir::Right,
                C::ColumnSelectUp => multi::ColumnDir::Up(1),
                C::ColumnSelectDown => multi::ColumnDir::Down(1),
                C::ColumnSelectPageUp { lines } => multi::ColumnDir::Up(*lines),
                C::ColumnSelectPageDown { lines } => multi::ColumnDir::Down(*lines),
                _ => unreachable!("column select command"),
            };
            sel_out(doc, multi::column_select_step(&ctx, text, view, dir))
        }
        C::ColumnSelect {
            anchor_line,
            anchor_column,
            head_line,
            head_column,
        } => sel_out(
            doc,
            multi::column_select_mouse(
                &ctx,
                text,
                *anchor_line,
                *anchor_column,
                *head_line,
                *head_column,
            ),
        ),
        C::MoveLinesUp | C::MoveLinesDown => isolated(edit_out(
            lines::move_lines(&st(), matches!(cmd, C::MoveLinesDown)),
            cmd_origin(cmd),
        )),
        C::CopyLinesUp | C::CopyLinesDown => isolated(edit_out(
            lines::copy_lines(&st(), matches!(cmd, C::CopyLinesDown)),
            cmd_origin(cmd),
        )),
        C::DeleteAllLeft => isolated(edit_out(lines::delete_all_left(&st()), cmd_origin(cmd))),
        C::Fold | C::Unfold | C::ToggleFold | C::FoldAll | C::UnfoldAll => {
            let op = match cmd {
                C::Fold => fold::Op::Fold,
                C::Unfold => fold::Op::Unfold,
                C::ToggleFold => fold::Op::Toggle,
                C::FoldAll => fold::Op::FoldAll,
                _ => fold::Op::UnfoldAll,
            };
            Ok(match fold::run(doc, view, op) {
                Some(tr) => CommandOutput::Transaction(tr),
                None => CommandOutput::Nothing,
            })
        }
        C::Undo | C::Redo => {
            let undo = matches!(cmd, C::Undo);
            if doc.undo_mode() == UndoMode::Host {
                return Ok(CommandOutput::Request(if undo {
                    HostRequest::Undo
                } else {
                    HostRequest::Redo
                }));
            }
            let top = if undo {
                doc.history().peek_undo()
            } else {
                doc.history().peek_redo()
            };
            Ok(match top {
                None => CommandOutput::Nothing,
                Some((changes, s)) => CommandOutput::Transaction(
                    Transaction::new(doc.version(), changes.clone())
                        .with_selection(
                            s.clamped(&changes.apply(text).unwrap_or_else(|_| text.clone())),
                        )
                        .with_origin(if undo { Origin::Undo } else { Origin::Redo })
                        .with_group(GroupHint::Isolated),
                )
                .with_reveal_primary(),
            })
        }
        C::Save => Ok(CommandOutput::Request(HostRequest::Save)),
        C::Find { query } => find::find(doc, view, query.clone()),
        C::FindNext => find::find_step(doc, view, true),
        C::FindPrevious => find::find_step(doc, view, false),
        C::ReplaceOne { replacement } => find::replace_one(doc, view, replacement),
        C::ReplaceAll { replacement } => find::replace_all(doc, view, replacement),
        C::CloseFind => Ok(CommandOutput::Transaction(
            Transaction::new(doc.version(), ChangeSet::identity(text.len_bytes()))
                .with_effect(Effect::SetFind(None)),
        )),
        C::RevealLine { line } => Ok(CommandOutput::Transaction(
            Transaction::new(doc.version(), ChangeSet::identity(text.len_bytes()))
                .with_effect(Effect::RevealLine(*line)),
        )),
    }
}

/// A selection command's output: selection-only transaction, the session effect and a reveal.
fn sel_out(doc: &Document, o: multi::SelOut) -> Result<CommandOutput, Refusal> {
    let reveal = o.reveal.unwrap_or(o.sel.primary().head);
    let mut tr = Transaction::select(doc.version(), doc.text().len_bytes(), o.sel).with_effect(
        Effect::ScrollIntoView {
            pos: reveal,
            center: false,
        },
    );
    if let Some(s) = o.session {
        tr = tr.with_effect(Effect::ViewSession(s));
    }
    Ok(CommandOutput::Transaction(tr))
}

/// Pushes undo stops before and after (VS Code `pushUndoStop` around `executeCommands`).
fn isolated(r: Result<CommandOutput, Refusal>) -> Result<CommandOutput, Refusal> {
    r.map(|o| match o {
        CommandOutput::Transaction(tr) => {
            CommandOutput::Transaction(tr.with_group(GroupHint::Isolated))
        }
        o => o,
    })
}

impl CommandOutput {
    /// Points a placeholder `ScrollIntoView` effect at the primary head after the transaction.
    fn with_reveal_primary(self) -> Self {
        match self {
            CommandOutput::Transaction(mut tr) => {
                let head = tr.selection.as_ref().map(|s| s.primary().head).unwrap_or(0);
                let mut found = false;
                for e in tr.effects.iter_mut() {
                    if let Effect::ScrollIntoView { pos, .. } = e {
                        *pos = head;
                        found = true;
                    }
                }
                if !found {
                    tr.effects.push(Effect::ScrollIntoView {
                        pos: head,
                        center: false,
                    });
                }
                CommandOutput::Transaction(tr)
            }
            o => o,
        }
    }
}
