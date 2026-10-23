//! The hover and parameter hints commands (VS Code's `hoverActions.ts` and `parameterHints.ts`
//! commands). Each is an effect for the acting view; preconditions as VS Code's.

use crate::change::ChangeSet;
use crate::command::{CommandOutput, EditorCommand};
use crate::document::Document;
use crate::hover::HoverScroll;
use crate::transaction::{Effect, HoverEffect, ParameterHintsEffect, Transaction};
use crate::view::EditorView;

fn effect(doc: &Document, e: Effect) -> CommandOutput {
    CommandOutput::Transaction(
        Transaction::new(doc.version(), ChangeSet::identity(doc.text().len_bytes())).with_effect(e),
    )
}

pub(crate) fn run(doc: &Document, view: &EditorView, cmd: &EditorCommand) -> CommandOutput {
    use EditorCommand as C;
    let hover = view.hover();
    let hints = view.parameter_hints();
    let scroll = |s| {
        if hover.is_focused() {
            effect(doc, Effect::Hover(HoverEffect::Scroll(s)))
        } else {
            CommandOutput::Nothing
        }
    };
    let ph = |e| effect(doc, Effect::ParameterHints(e));
    match cmd {
        C::ShowHover => effect(doc, Effect::Hover(HoverEffect::ShowOrFocus)),
        C::HideHover if hover.is_visible() || hover.is_pending() => {
            effect(doc, Effect::Hover(HoverEffect::Hide))
        }
        C::ScrollUpHover => scroll(HoverScroll::Up),
        C::ScrollDownHover => scroll(HoverScroll::Down),
        C::ScrollLeftHover => scroll(HoverScroll::Left),
        C::ScrollRightHover => scroll(HoverScroll::Right),
        C::PageUpHover => scroll(HoverScroll::PageUp),
        C::PageDownHover => scroll(HoverScroll::PageDown),
        C::GoToTopHover => scroll(HoverScroll::Top),
        C::GoToBottomHover => scroll(HoverScroll::Bottom),
        // precondition: EditorContextKeys.hasSignatureHelpProvider
        C::TriggerParameterHints if !doc.signature_help_sources().is_empty() => {
            ph(ParameterHintsEffect::Trigger)
        }
        C::CloseParameterHints if hints.is_visible() => ph(ParameterHintsEffect::Close),
        C::ShowPrevParameterHint if hints.multiple_signatures() => {
            ph(ParameterHintsEffect::Previous)
        }
        C::ShowNextParameterHint if hints.multiple_signatures() => ph(ParameterHintsEffect::Next),
        _ => CommandOutput::Nothing,
    }
}
