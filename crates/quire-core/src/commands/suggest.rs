//! The suggest and snippet commands (VS Code's suggest widget commands and SnippetController2's
//! placeholder navigation).

use crate::change::ChangeSet;
use crate::command::{CommandOutput, EditorCommand};
use crate::completion::snippet::Snippet;
use crate::completion::{InsertMode, WidgetState};
use crate::document::{Document, Refusal};
use crate::transaction::{Effect, GroupHint, Origin, SuggestEffect, Transaction};
use crate::view::EditorView;

fn effect(doc: &Document, e: Effect) -> CommandOutput {
    CommandOutput::Transaction(
        Transaction::new(doc.version(), ChangeSet::identity(doc.text().len_bytes())).with_effect(e),
    )
}

pub(crate) fn run(
    doc: &Document,
    view: &EditorView,
    cmd: &EditorCommand,
) -> Result<CommandOutput, Refusal> {
    use EditorCommand as C;
    let st = view.suggest();
    let visible = st.is_visible();
    Ok(match cmd {
        C::TriggerSuggest => {
            if doc.config().read_only || visible {
                CommandOutput::Nothing
            } else {
                effect(doc, Effect::Suggest(SuggestEffect::Trigger))
            }
        }
        C::HideSuggestWidget if visible => effect(doc, Effect::Suggest(SuggestEffect::Hide)),
        C::SelectNextSuggestion | C::SelectPrevSuggestion
        | C::SelectNextPageSuggestion | C::SelectPrevPageSuggestion
            if matches!(st.widget_state(), WidgetState::Open | WidgetState::Frozen) =>
        {
            let (delta, page) = match cmd {
                C::SelectNextSuggestion => (1, false),
                C::SelectPrevSuggestion => (-1, false),
                C::SelectNextPageSuggestion => (1, true),
                _ => (-1, true),
            };
            effect(doc, Effect::Suggest(SuggestEffect::Select { delta, page }))
        }
        C::ToggleSuggestionDetails if st.focused().is_some() => {
            effect(doc, Effect::Suggest(SuggestEffect::ToggleDetails))
        }
        C::AcceptSelectedSuggestion => match accept(doc, view, cmd) {
            Some(tr) => CommandOutput::Transaction(tr),
            None => CommandOutput::Nothing,
        },
        C::JumpToNextSnippetPlaceholder | C::JumpToPrevSnippetPlaceholder => {
            let Some(sn) = view.snippet() else {
                return Ok(CommandOutput::Nothing);
            };
            let next = matches!(cmd, C::JumpToNextSnippetPlaceholder);
            if (next && !sn.has_next()) || (!next && !sn.has_prev()) {
                return Ok(CommandOutput::Nothing);
            }
            let to = if next { sn.current + 1 } else { sn.current - 1 };
            let (cs, sel, after) =
                sn.move_to(doc.text(), to, view.selection().primary().head);
            CommandOutput::Transaction(
                Transaction::new(doc.version(), cs)
                    .with_selection(sel)
                    .with_origin(Origin::Command(std::borrow::Cow::Borrowed(cmd.id())))
                    .with_effect(Effect::Snippet(after)),
            )
        }
        C::LeaveSnippet if view.snippet().is_some() => CommandOutput::Transaction(
            Transaction::select(
                doc.version(),
                doc.text().len_bytes(),
                view.selection().clone().into_primary(),
            )
            .with_effect(Effect::Snippet(None)),
        ),
        _ => CommandOutput::Nothing,
    })
}

/// VS Code `SuggestController._insertSuggestion` for the focused item.
fn accept(doc: &Document, view: &EditorView, cmd: &EditorCommand) -> Option<Transaction> {
    let st = view.suggest();
    let f = st.focused_item()?;
    let item = &f.item;
    let c = &item.completion;
    let sel = view.selection();
    let pos = sel.primary().head;
    let text = doc.text();
    let (line, col) = text.byte_to_line_col(pos);
    // overwriteBefore + the columns typed since the request
    let overwrite_before = col.saturating_sub(item.edit_start);
    let end = match doc.config().suggest.insert_mode {
        InsertMode::Insert => item.edit_insert_end,
        InsertMode::Replace => item.edit_replace_end,
    };
    let overwrite_after = end.saturating_sub(item.position) + st.suffix_delta(doc, pos);
    let template = if c.insert_text_rules.insert_as_snippet {
        c.insert_text.clone()
    } else {
        Snippet::escape(&c.insert_text)
    };
    // additional edits after the item on its line shift with what was typed since
    let line_start = text.line_range(line).start;
    let delta = col as isize - item.position as isize;
    let extra: Vec<_> = c
        .additional_text_edits
        .iter()
        .map(|(r, t)| {
            let on_line = text.byte_to_line(r.start.min(text.len_bytes())) == line;
            if on_line && r.start.saturating_sub(line_start) > item.position {
                let multi = text.byte_to_line(r.end.min(text.len_bytes())) != line;
                let s = (r.start as isize + delta).max(0) as usize;
                let e = if multi {
                    r.end
                } else {
                    (r.end as isize + delta).max(0) as usize
                };
                (s..e.max(s), t.clone())
            } else {
                (r.clone(), t.clone())
            }
        })
        .collect();
    let ins = crate::completion::insert::insert_snippet(
        doc,
        sel,
        &template,
        overwrite_before,
        overwrite_after,
        !c.insert_text_rules.keep_whitespace,
        extra,
    )?;
    let retrigger = c
        .command
        .as_ref()
        .is_some_and(|k| k.id == "editor.action.triggerSuggest");
    let command = c.command.clone().filter(|_| !retrigger);
    let mut tr = Transaction::new(doc.version(), ins.changes)
        .with_selection(ins.selection)
        .with_origin(Origin::Command(std::borrow::Cow::Borrowed(cmd.id())))
        .with_group(GroupHint::Isolated)
        .with_effect(Effect::Suggest(SuggestEffect::Accepted { retrigger, command }));
    if ins.session.is_some() {
        tr = tr.with_effect(Effect::Snippet(ins.session));
    }
    Some(tr)
}

/// A transaction inserting `template` (a snippet) at every cursor, replacing the selections,
/// and starting snippet mode when it has placeholders (VS Code `SnippetController2.insert`).
pub fn snippet_transaction(doc: &Document, view: &EditorView, template: &str) -> Option<Transaction> {
    let ins = crate::completion::insert::insert_snippet(
        doc,
        view.selection(),
        template,
        0,
        0,
        true,
        Vec::new(),
    )?;
    Some(
        Transaction::new(doc.version(), ins.changes)
            .with_selection(ins.selection)
            .with_origin(Origin::Command(std::borrow::Cow::Borrowed("editor.action.insertSnippet")))
            .with_group(GroupHint::Isolated)
            .with_effect(Effect::Snippet(ins.session)),
    )
}
