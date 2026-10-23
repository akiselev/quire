//! Command → transaction (pure). See [`crate::command`].
//!
//! Behaviour follows VS Code's cursor and typing operations (`cursorMoveOperations.ts`,
//! `cursorWordOperations.ts`, `cursorTypeEditOperations.ts`, `cursorDeleteOperations.ts`,
//! `shiftCommand.ts`, `lineCommentCommand.ts`); the word operations are a literal translation in
//! [`words`] (MIT notice there). Differences, chosen to fit quire's design:
//! - the language service is `EditorConfig` itself: `brackets`, `on_enter_rules` and
//!   `indentation_rules` drive Enter, Tab on a blank line, re-indentation while typing and the
//!   bracket electric characters ([`crate::lang`], VS Code's `onEnter.ts` / `autoIndent.ts`);
//!   with no tokens, brackets inside strings and comments count;
//! - multi-char `type` text runs through the typing interceptors char by char (VS Code's
//!   keyboard path), composed into one transaction;
//! - auto-closing has no tokenizer: quotes never auto-close after a word character, brackets
//!   only before whitespace, a closing character or `auto_close_before`; no bracket-balance check.

mod comment;
mod edit;
mod find;
mod fold;
mod indentation;
mod lines;
pub(crate) use quire_text::ops::model;
pub(crate) mod hints;
mod motion;
mod multi;
pub(crate) mod suggest;
pub(crate) use quire_text::words::vscode as words;

pub(crate) use quire_text::ops::{RangeEdit, build, track};

use crate::change::ChangeSet;
use crate::command::{CommandOutput, EditorCommand, HostRequest};
use crate::config::EditorConfig;
use crate::document::{Document, Refusal};
use crate::selection::Selection;
use crate::text::Text;
use crate::transaction::{Effect, GroupHint, InputKind, Origin, Transaction};
use crate::view::{AutoClosed, EditorView};
use crate::words::WordClassifier;
use quire_text::command::{Dir, EditCommand, Unit};
use quire_text::words::WordStop;
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

impl From<quire_text::ops::edit::OpEdit> for Out {
    fn from(o: quire_text::ops::edit::OpEdit) -> Self {
        Out {
            changes: o.changes,
            sel: o.sel,
            kind: o.kind,
            auto_closed: Vec::new(),
            close_before: o.close_before,
        }
    }
}

pub(crate) struct Ctx<'a> {
    pub cfg: &'a EditorConfig,
    /// The document's cached classifier (`Document::edit_rules`).
    pub cls: &'a WordClassifier,
}

fn tr_from(doc: &Document, changes: ChangeSet, sel: Selection, origin: Origin) -> Transaction {
    Transaction::new(doc.version(), changes)
        .with_selection(sel)
        .with_origin(origin)
}

fn cmd_origin(cmd: &EditorCommand) -> Origin {
    Origin::Command(std::borrow::Cow::Borrowed(cmd.id()))
}

/// The quire-text command an editor command lowers to, where they overlap (one implementation
/// of motion, deletion and undo).
fn lower(cmd: &EditorCommand) -> Option<EditCommand> {
    use crate::command::Motion as M;
    use EditorCommand as C;
    let mv = |unit, dir, extend| EditCommand::Move { unit, dir, extend };
    let del = |unit, dir| EditCommand::Delete { unit, dir };
    Some(match cmd {
        C::CursorMove { to, select } => {
            let (unit, dir) = match to {
                M::Left => (Unit::Grapheme, Dir::Back),
                M::Right => (Unit::Grapheme, Dir::Forward),
                M::Up => (Unit::VisualLine, Dir::Back),
                M::Down => (Unit::VisualLine, Dir::Forward),
                M::PageUp { .. } => (Unit::Page, Dir::Back),
                M::PageDown { .. } => (Unit::Page, Dir::Forward),
                M::WordLeft => (Unit::Word(WordStop::StartFast), Dir::Back),
                M::WordStartLeft => (Unit::Word(WordStop::Start), Dir::Back),
                M::WordEndLeft => (Unit::Word(WordStop::End), Dir::Back),
                M::WordStartRight => (Unit::Word(WordStop::Start), Dir::Forward),
                M::WordEndRight => (Unit::Word(WordStop::End), Dir::Forward),
                M::LineStart => (Unit::LineEdge, Dir::Back),
                M::LineEnd => (Unit::LineEdge, Dir::Forward),
                M::DocumentStart => (Unit::DocEdge, Dir::Back),
                M::DocumentEnd => (Unit::DocEdge, Dir::Forward),
            };
            mv(unit, dir, *select)
        }
        C::SetSelection { selection } => EditCommand::SetSelection(selection.clone()),
        C::SelectAll => EditCommand::SelectAll,
        C::Paste { text, per_cursor } => EditCommand::Insert {
            text: text.clone(),
            kind: InputKind::Paste,
            distribute: *per_cursor,
        },
        C::Cut => EditCommand::Cut,
        C::DeleteLeft => del(Unit::Grapheme, Dir::Back),
        C::DeleteRight => del(Unit::Grapheme, Dir::Forward),
        C::DeleteWordLeft => del(Unit::Word(WordStop::StartOrSpace), Dir::Back),
        C::DeleteWordStartLeft => del(Unit::Word(WordStop::Start), Dir::Back),
        C::DeleteWordEndLeft => del(Unit::Word(WordStop::End), Dir::Back),
        C::DeleteWordRight => del(Unit::Word(WordStop::EndOrSpace), Dir::Forward),
        C::DeleteWordStartRight => del(Unit::Word(WordStop::Start), Dir::Forward),
        C::DeleteWordEndRight => del(Unit::Word(WordStop::End), Dir::Forward),
        C::DeleteAllLeft => del(Unit::LineEdge, Dir::Back),
        C::DeleteAllRight => del(Unit::LineEdge, Dir::Forward),
        C::Undo => EditCommand::Undo,
        C::Redo => EditCommand::Redo,
        _ => return None,
    })
}

/// Rows for vertical motion: logical lines, folds skipped, `page` lines per page.
pub(crate) struct FoldedLines<'a> {
    pub base: quire_text::NoWrap,
    pub folds: &'a [Range<usize>],
}

impl quire_text::VisualLines for FoldedLines<'_> {
    fn locate(
        &self,
        text: &Text,
        byte: usize,
        up: bool,
    ) -> (quire_text::VisualLine, quire_text::Goal) {
        self.base.locate(text, byte, up)
    }
    fn offset(
        &self,
        text: &Text,
        line: quire_text::VisualLine,
        delta: isize,
    ) -> Result<quire_text::VisualLine, quire_text::Edge> {
        let l = motion::step_lines(text, self.folds, line.line, delta);
        if l < 0 {
            Err(quire_text::Edge::Start)
        } else if l >= text.len_lines() as isize {
            Err(quire_text::Edge::End)
        } else {
            Ok(quire_text::VisualLine {
                line: l as usize,
                row: 0,
            })
        }
    }
    fn byte_at(
        &self,
        text: &Text,
        line: quire_text::VisualLine,
        goal: quire_text::Goal,
    ) -> (usize, bool) {
        self.base.byte_at(text, line, goal)
    }
    fn range(&self, text: &Text, line: quire_text::VisualLine) -> Range<usize> {
        self.base.range(text, line)
    }
    fn lines_per_page(&self) -> usize {
        self.base.lines_per_page()
    }
}

/// Resolves a lowered command through quire-text and restores quire's transaction shape: the
/// origin (word and line deletions are commands, a cut is `Input(Other)`), the reveal effect,
/// and host requests as quire's.
fn run_lowered(
    doc: &Document,
    view: &EditorView,
    cmd: &EditorCommand,
    ecmd: &EditCommand,
) -> Result<CommandOutput, Refusal> {
    use EditorCommand as C;
    let cfg = doc.config();
    let page = match cmd {
        C::CursorMove {
            to:
                crate::command::Motion::PageUp { lines } | crate::command::Motion::PageDown { lines },
            ..
        } => *lines,
        _ => 1,
    };
    let layout = FoldedLines {
        base: quire_text::NoWrap {
            tab_size: cfg.tab_size,
            page_lines: page,
        },
        folds: view.folds(),
    };
    let r = doc
        .text_document()
        .resolve(view.text_view(), ecmd, doc.edit_rules(), &layout);
    if let Some(req) = r.requests.first() {
        match req {
            quire_text::HostRequest::Undo => return Ok(CommandOutput::Request(HostRequest::Undo)),
            quire_text::HostRequest::Redo => return Ok(CommandOutput::Request(HostRequest::Redo)),
            _ => {}
        }
    }
    let Some(tr) = r.tr else {
        return Ok(CommandOutput::Nothing);
    };
    let mut tr: Transaction = tr.with_effect_type();
    match cmd {
        C::DeleteWordLeft
        | C::DeleteWordRight
        | C::DeleteWordStartLeft
        | C::DeleteWordEndLeft
        | C::DeleteWordStartRight
        | C::DeleteWordEndRight
        | C::DeleteAllLeft
        | C::DeleteAllRight => tr.origin = cmd_origin(cmd),
        C::Cut => tr.origin = Origin::Input(InputKind::Other),
        _ => {}
    }
    let undo = matches!(cmd, C::Undo | C::Redo);
    if !undo {
        tr = tr.with_effect(Effect::ScrollIntoView {
            pos: 0,
            center: false,
        });
    }
    Ok(CommandOutput::Transaction(tr).with_reveal_primary())
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
        cls: doc
            .edit_rules()
            .words
            .classifier()
            .expect("quire's documents use VS Code's word rules"),
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
    let editor_delete = matches!(
        cmd,
        C::DeleteLeft | C::DeleteWordLeft | C::DeleteWordStartLeft | C::DeleteWordEndLeft
    ) && edit::needs_editor_delete(&ctx, &st(), cmd);
    if !editor_delete && let Some(e) = lower(cmd) {
        return run_lowered(doc, view, cmd, &e);
    }
    match cmd {
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
        C::DeleteLeft => edit_out(
            edit::delete_left(&ctx, &st()),
            Origin::Input(InputKind::DeletingLeft),
        ),
        C::DeleteWordLeft | C::DeleteWordStartLeft | C::DeleteWordEndLeft => {
            // Only an auto-closed pair delete reaches here (`needs_editor_delete`).
            edit_out(edit::pair_delete(&st()), cmd_origin(cmd))
        }
        C::Tab => edit_out(edit::tab(&ctx, &st()), cmd_origin(cmd)),
        C::Outdent | C::OutdentLines => edit_out(edit::shift(&ctx, &st(), true), cmd_origin(cmd)),
        C::IndentLines => edit_out(edit::shift(&ctx, &st(), false), cmd_origin(cmd)),
        C::CommentLine | C::AddCommentLine | C::RemoveCommentLine => {
            let ty = match cmd {
                C::CommentLine => comment::LineCommentType::Toggle,
                C::AddCommentLine => comment::LineCommentType::ForceAdd,
                _ => comment::LineCommentType::ForceRemove,
            };
            isolated(edit_out(
                comment::line_comment(&ctx, &st(), ty),
                cmd_origin(cmd),
            ))
        }
        C::BlockComment => isolated(edit_out(
            comment::block_comment(&ctx, &st()),
            cmd_origin(cmd),
        )),
        C::IndentationToSpaces | C::IndentationToTabs => {
            // VS Code runs the command, then `model.updateOptions({ insertSpaces })` even when
            // nothing changed.
            let spaces = matches!(cmd, C::IndentationToSpaces);
            let o = indentation::indentation_to(&st(), cfg.tab_size, spaces);
            let r = match o {
                Some(o) => edit_out(Some(o), cmd_origin(cmd)),
                None => Ok(CommandOutput::Transaction(
                    Transaction::select(doc.version(), text.len_bytes(), sel.clone())
                        .with_origin(cmd_origin(cmd)),
                )),
            };
            isolated(r.map(|o| match o {
                CommandOutput::Transaction(tr) => {
                    CommandOutput::Transaction(tr.with_effect(Effect::SetInsertSpaces(spaces)))
                }
                o => o,
            }))
        }
        C::ToggleWordWrap => {
            // VS Code (`toggleWordWrap.ts`) overrides the configured wrap with the opposite of
            // the current one; quire has one setting, so the toggle flips it. No undo stop.
            let w = match cfg.word_wrap {
                crate::config::WordWrap::Off => crate::config::WordWrap::On,
                crate::config::WordWrap::On => crate::config::WordWrap::Off,
            };
            Ok(CommandOutput::Transaction(
                Transaction::select(doc.version(), text.len_bytes(), sel.clone())
                    .with_origin(cmd_origin(cmd))
                    .with_effect(Effect::SetWordWrap(w)),
            ))
        }
        C::ReindentLines => isolated(edit_out(
            indentation::reindent_lines(cfg, &st()),
            cmd_origin(cmd),
        )),
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
        C::CompositionType {
            text: typed,
            replace_prev_chars,
            replace_next_chars,
        } => {
            let typed = crate::text::normalize_line_endings(typed);
            let o = edit::composition_type(
                &ctx,
                st(),
                &typed,
                *replace_prev_chars,
                *replace_next_chars,
            );
            isolated(edit_out(o, Origin::Input(InputKind::Typing)))
        }
        C::Save => Ok(CommandOutput::Request(HostRequest::Save)),
        C::TriggerSuggest
        | C::AcceptSelectedSuggestion
        | C::HideSuggestWidget
        | C::SelectNextSuggestion
        | C::SelectPrevSuggestion
        | C::SelectNextPageSuggestion
        | C::SelectPrevPageSuggestion
        | C::ToggleSuggestionDetails
        | C::JumpToNextSnippetPlaceholder
        | C::JumpToPrevSnippetPlaceholder
        | C::LeaveSnippet => suggest::run(doc, view, cmd),
        C::ShowHover
        | C::HideHover
        | C::ScrollUpHover
        | C::ScrollDownHover
        | C::ScrollLeftHover
        | C::ScrollRightHover
        | C::PageUpHover
        | C::PageDownHover
        | C::GoToTopHover
        | C::GoToBottomHover
        | C::TriggerParameterHints
        | C::CloseParameterHints
        | C::ShowPrevParameterHint
        | C::ShowNextParameterHint => Ok(hints::run(doc, view, cmd)),
        C::CursorMove { .. }
        | C::SetSelection { .. }
        | C::SelectAll
        | C::Paste { .. }
        | C::Cut
        | C::DeleteRight
        | C::DeleteWordRight
        | C::DeleteWordStartRight
        | C::DeleteWordEndRight
        | C::DeleteAllLeft
        | C::DeleteAllRight
        | C::Undo
        | C::Redo => unreachable!("lowered to quire-text"),
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
