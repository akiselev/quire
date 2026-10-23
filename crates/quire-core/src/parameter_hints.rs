//! Signature help (VS Code's parameter hints): the data model and the session of one view. The
//! host supplies the provider; quire has no language client.
//!
//! **The host seam.**
//! - Providers and their trigger and retrigger characters are declared on the document
//!   ([`Document::set_signature_help_sources`](crate::Document::set_signature_help_sources)).
//!   With none, nothing triggers (VS Code: no registered `SignatureHelpProvider`).
//! - The session asks with a [`SignatureHelpRequest`] (drained with
//!   [`EditorView::take_parameter_hints_outputs`](crate::EditorView::take_parameter_hints_outputs);
//!   egui_quire turns it into `EditorEvent::SignatureHelpRequested`) carrying the position, the
//!   version and VS Code's `SignatureHelpContext`.
//! - The host answers, now or later, with
//!   [`EditorView::set_signature_help`](crate::EditorView::set_signature_help)`(doc, id, version,
//!   help)`: the first provider's non-empty answer, or `None`. `None` or no signatures closes the
//!   hints, which is how they close when the cursor leaves the call (VS Code retriggers on every
//!   edit and cursor move while they show). An answer to a request that a newer trigger replaced
//!   is refused as [`AnswerRefusal::Superseded`]; one older than the recent-changes window as
//!   [`AnswerRefusal::Stale`].
//! - quire-core reads no clock: triggers wait `DEFAULT_DELAY` (120 ms, VS Code's throttle) on the
//!   host clock passed with commands and to [`ParameterHintsState::tick`].
//!
//! Translated from `editor/contrib/parameterHints/browser/parameterHintsModel.ts` and the
//! label-offset logic of `parameterHintsWidget.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use crate::diagnostics::SourceId;
use crate::document::Document;
use crate::hover::{AnswerRefusal, Timer};
use crate::version::Version;
use std::ops::Range;

/// Text that is either plain or Markdown (VS Code's `string | IMarkdownString`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Documentation {
    /// Shown as is.
    Plain(String),
    /// Markdown source.
    Markdown(String),
}

impl Documentation {
    /// The text (Markdown source for Markdown).
    pub fn as_str(&self) -> &str {
        match self {
            Documentation::Plain(s) | Documentation::Markdown(s) => s,
        }
    }
}

/// A parameter's label: a substring of the signature label, or its byte offsets in it
/// (VS Code `string | [number, number]`; offsets are bytes here).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ParameterLabel {
    /// Found in the signature label as a whole word.
    Text(String),
    /// Byte offsets in the signature label.
    Offsets(Range<usize>),
}

/// One parameter (Monaco `languages.ParameterInformation`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParameterInformation {
    /// The label.
    pub label: ParameterLabel,
    /// Its documentation.
    pub documentation: Option<Documentation>,
}

impl ParameterInformation {
    /// A parameter labelled by a substring of the signature.
    pub fn new(label: impl Into<String>) -> Self {
        ParameterInformation {
            label: ParameterLabel::Text(label.into()),
            documentation: None,
        }
    }
}

/// One signature (Monaco `languages.SignatureInformation`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignatureInformation {
    /// The label, e.g. `fn add(a: i32, b: i32) -> i32`.
    pub label: String,
    /// Its documentation.
    pub documentation: Option<Documentation>,
    /// The parameters.
    pub parameters: Vec<ParameterInformation>,
    /// The active parameter of this signature (overrides [`SignatureHelp::active_parameter`]).
    pub active_parameter: Option<usize>,
}

impl SignatureInformation {
    /// A signature with parameters.
    pub fn new(label: impl Into<String>, parameters: Vec<ParameterInformation>) -> Self {
        SignatureInformation {
            label: label.into(),
            documentation: None,
            parameters,
            active_parameter: None,
        }
    }

    /// VS Code `getParameterLabelOffsets`: the byte range of parameter `index` in the label
    /// (`0..0` when absent or not found). A text label is found as a whole word (`(\W|^)label
    /// (?=\W|$)`), first occurrence.
    pub fn parameter_label_offsets(&self, index: usize) -> Range<usize> {
        let Some(p) = self.parameters.get(index) else {
            return 0..0;
        };
        match &p.label {
            ParameterLabel::Offsets(r) => {
                let e = r.end.min(self.label.len());
                r.start.min(e)..e
            }
            ParameterLabel::Text(t) if t.is_empty() => 0..0,
            ParameterLabel::Text(t) => {
                let is_w = |c: char| c.is_ascii_alphanumeric() || c == '_';
                let s = &self.label;
                for (i, _) in s.match_indices(t.as_str()) {
                    let before_ok = s[..i].chars().next_back().is_none_or(|c| !is_w(c));
                    let after_ok = s[i + t.len()..].chars().next().is_none_or(|c| !is_w(c));
                    if before_ok && after_ok {
                        return i..i + t.len();
                    }
                }
                0..0
            }
        }
    }
}

/// A provider's answer (Monaco `languages.SignatureHelp`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignatureHelp {
    /// The signatures (overloads).
    pub signatures: Vec<SignatureInformation>,
    /// The signature shown.
    pub active_signature: usize,
    /// The parameter highlighted (unless the signature names its own).
    pub active_parameter: usize,
}

impl SignatureHelp {
    /// The active parameter of the active signature (VS Code `signature.activeParameter ??
    /// hints.activeParameter`).
    pub fn active_parameter_of(&self, sig: &SignatureInformation) -> usize {
        sig.active_parameter.unwrap_or(self.active_parameter)
    }
}

/// A signature help provider the host has.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignatureHelpSource {
    /// Its id.
    pub id: SourceId,
    /// Typing one of these triggers (`signatureHelpTriggerCharacters`, e.g. `(` and `,`).
    pub trigger_characters: Vec<String>,
    /// Typing one of these triggers again while the hints show
    /// (`signatureHelpRetriggerCharacters`, e.g. `)`).
    pub retrigger_characters: Vec<String>,
}

/// Why signature help was requested (Monaco `SignatureHelpTriggerKind`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SignatureHelpTriggerKind {
    /// `editor.action.triggerParameterHints`.
    Invoke,
    /// A trigger (or retrigger) character was typed.
    TriggerCharacter(String),
    /// The text or the cursor changed while the hints show.
    ContentChange,
}

/// VS Code `SignatureHelpContext`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SignatureHelpContext {
    /// Why.
    pub trigger: SignatureHelpTriggerKind,
    /// The hints were showing or pending.
    pub is_retrigger: bool,
    /// What showed before (the provider may keep the active signature).
    pub active_signature_help: Option<SignatureHelp>,
}

/// Identifies a request; answers carry it back.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignatureHelpRequestId(pub u64);

/// A request for signature help, for the host's providers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SignatureHelpRequest {
    /// Answer with this id.
    pub id: SignatureHelpRequestId,
    /// The primary cursor (bytes at `version`).
    pub pos: usize,
    /// The document version.
    pub version: Version,
    /// The context.
    pub context: SignatureHelpContext,
}

/// What a view's parameter hints want from the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParameterHintsOutput {
    /// Ask the providers.
    Request(SignatureHelpRequest),
}

/// The parameter hints options (VS Code `editor.parameterHints.*`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ParameterHintsConfig {
    /// `editor.parameterHints.enabled`: trigger characters show the hints.
    pub enabled: bool,
    /// `editor.parameterHints.cycle`: next/previous wrap around the overloads.
    pub cycle: bool,
}

impl Default for ParameterHintsConfig {
    fn default() -> Self {
        ParameterHintsConfig {
            enabled: true,
            cycle: true,
        }
    }
}

/// VS Code `ParameterHintsModel.DEFAULT_DELAY` (ms).
pub const DEFAULT_DELAY: u64 = 120;

#[derive(Clone, PartialEq, Debug, Default)]
enum State {
    #[default]
    Default,
    Pending {
        id: SignatureHelpRequestId,
        previously_active: Option<SignatureHelp>,
    },
    Active(SignatureHelp),
}

/// The parameter hints of one view (VS Code `ParameterHintsModel`).
#[derive(Clone, Debug, Default)]
pub struct ParameterHintsState {
    state: State,
    pending_triggers: Vec<SignatureHelpTriggerKind>,
    /// The throttled delayer: (deadline, trigger id it runs for).
    delayer: Option<(Timer, u64)>,
    trigger_id: u64,
    outputs: Vec<ParameterHintsOutput>,
    /// The delay new triggers wait (VS Code's constructor argument; tests shorten it).
    delay: Option<u64>,
}

impl ParameterHintsState {
    /// The hints shown, if any.
    pub fn active(&self) -> Option<&SignatureHelp> {
        match &self.state {
            State::Active(h) => Some(h),
            _ => None,
        }
    }

    /// Whether the hints show (`parameterHintsVisible`).
    pub fn is_visible(&self) -> bool {
        self.active().is_some()
    }

    /// Whether more than one signature shows (`parameterHintsMultipleSignatures`).
    pub fn multiple_signatures(&self) -> bool {
        self.active().is_some_and(|h| h.signatures.len() > 1)
    }

    /// VS Code `isTriggered`: showing, pending, or a trigger waits.
    pub fn is_triggered(&self) -> bool {
        !matches!(self.state, State::Default) || self.delayer.is_some()
    }

    /// The request waiting for an answer.
    pub fn pending_request(&self) -> Option<SignatureHelpRequestId> {
        match &self.state {
            State::Pending { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// Sets the throttle delay for later triggers (VS Code's `ParameterHintsModel` delay
    /// argument; default [`DEFAULT_DELAY`]).
    pub fn set_delay(&mut self, ms: u64) {
        self.delay = Some(ms);
    }

    /// The earliest deadline (host clock, ms) at which [`ParameterHintsState::tick`] has work.
    pub fn next_deadline(&self) -> Option<u64> {
        self.delayer.and_then(|(t, _)| t.at())
    }

    /// Whether a trigger waits to be placed on the clock (call [`ParameterHintsState::tick`]).
    pub fn needs_tick(&self) -> bool {
        self.delayer.is_some_and(|(t, _)| t.at().is_none())
    }

    /// Takes what the hints want from the host.
    pub fn take_outputs(&mut self) -> Vec<ParameterHintsOutput> {
        std::mem::take(&mut self.outputs)
    }

    /// VS Code `cancel`: hide, drop the waiting trigger and the pending request.
    pub fn cancel(&mut self) {
        self.state = State::Default;
        self.delayer = None;
    }

    /// VS Code `trigger(context, delay)`: queue a trigger, run after `delay` ms of quiet (the
    /// default delay when `None`). `now`: the host clock, if known.
    pub fn trigger(
        &mut self,
        doc: &Document,
        kind: SignatureHelpTriggerKind,
        delay: Option<u64>,
        now: Option<u64>,
    ) {
        if doc.signature_help_sources().is_empty() {
            return;
        }
        self.trigger_id += 1;
        self.pending_triggers.push(kind);
        let d = delay.unwrap_or(self.delay.unwrap_or(DEFAULT_DELAY));
        self.delayer = Some((Timer::new(now, d), self.trigger_id));
    }

    /// Runs the waiting trigger if due at `now` (host clock, ms); a request is made at `cursor`.
    pub fn tick(&mut self, doc: &Document, cursor: usize, now: u64) {
        let Some((mut t, id)) = self.delayer else {
            return;
        };
        t.stamp(now);
        self.delayer = Some((t, id));
        if t.due(now) {
            self.delayer = None;
            self.do_trigger(doc, cursor, id);
        }
    }

    fn do_trigger(&mut self, doc: &Document, cursor: usize, trigger_id: u64) {
        let is_retrigger = !matches!(self.state, State::Default);
        let active = match &self.state {
            State::Active(h) => Some(h.clone()),
            State::Pending {
                previously_active, ..
            } => previously_active.clone(),
            State::Default => None,
        };
        self.state = State::Default;
        if self.pending_triggers.is_empty() {
            return;
        }
        let kind = self
            .pending_triggers
            .drain(..)
            .reduce(|prev, cur| match cur {
                SignatureHelpTriggerKind::ContentChange => prev,
                _ => cur,
            })
            .unwrap_or(SignatureHelpTriggerKind::Invoke);
        let id = SignatureHelpRequestId(trigger_id);
        let pos = cursor.min(doc.text().len_bytes());
        self.outputs
            .push(ParameterHintsOutput::Request(SignatureHelpRequest {
                id,
                pos,
                version: doc.version(),
                context: SignatureHelpContext {
                    trigger: kind,
                    is_retrigger,
                    active_signature_help: active.clone(),
                },
            }));
        self.state = State::Pending {
            id,
            previously_active: active,
        };
    }

    /// The host's answer to a [`SignatureHelpRequest`].
    pub fn set_signature_help(
        &mut self,
        doc: &Document,
        id: SignatureHelpRequestId,
        at: Version,
        help: Option<SignatureHelp>,
    ) -> Result<(), AnswerRefusal> {
        // "Check that we are still resolving the correct signature help."
        if self.pending_request() != Some(id) || id.0 != self.trigger_id {
            return Err(AnswerRefusal::Superseded);
        }
        if at > doc.version() || doc.recent().since(at).is_err() {
            return Err(AnswerRefusal::Stale);
        }
        match help {
            // Kept as given (VS Code): a widget shows nothing for an out-of-range signature.
            Some(h) if !h.signatures.is_empty() => self.state = State::Active(h),
            _ => self.cancel(),
        }
        Ok(())
    }

    /// VS Code `next` (`showNextParameterHint`).
    pub fn next(&mut self, cycle: bool) {
        let State::Active(h) = &mut self.state else {
            return;
        };
        let len = h.signatures.len();
        let last = h.active_signature % len == len - 1;
        if (len < 2 || last) && !cycle {
            self.cancel();
            return;
        }
        h.active_signature = if last && cycle {
            0
        } else {
            h.active_signature + 1
        };
    }

    /// VS Code `previous` (`showPrevParameterHint`).
    pub fn previous(&mut self, cycle: bool) {
        let State::Active(h) = &mut self.state else {
            return;
        };
        let len = h.signatures.len();
        let first = h.active_signature == 0;
        if (len < 2 || first) && !cycle {
            self.cancel();
            return;
        }
        h.active_signature = if first && cycle {
            len - 1
        } else {
            h.active_signature - 1
        };
    }

    /// VS Code `onDidType`: the last typed character triggers (or retriggers while triggered).
    pub(crate) fn on_did_type(&mut self, doc: &Document, text: &str, now: Option<u64>) {
        if !doc.config().parameter_hints.enabled {
            return;
        }
        let Some(ch) = text.chars().next_back() else {
            return;
        };
        let has = |list: &[String]| list.iter().any(|s| s.starts_with(ch));
        let sources = doc.signature_help_sources();
        let trigger = sources.iter().any(|s| has(&s.trigger_characters));
        let retrigger = sources
            .iter()
            .any(|s| has(&s.trigger_characters) || has(&s.retrigger_characters));
        if trigger || (self.is_triggered() && retrigger) {
            self.trigger(
                doc,
                SignatureHelpTriggerKind::TriggerCharacter(ch.to_string()),
                None,
                now,
            );
        }
    }

    /// VS Code `onModelContentChange` / `onCursorChange` (not the mouse).
    pub(crate) fn on_change(&mut self, doc: &Document, now: Option<u64>) {
        if self.is_triggered() {
            self.trigger(doc, SignatureHelpTriggerKind::ContentChange, None, now);
        }
    }
}
