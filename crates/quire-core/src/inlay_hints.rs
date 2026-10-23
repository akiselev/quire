//! Inlay hints (VS Code's `InlayHintsController`): hints the host's providers supply, shown as
//! injected text in the lines (see [`injected`](crate::injected)). quire has no language
//! client.
//!
//! **The host seam.**
//! - Inlay hint providers are declared on the document
//!   ([`Document::set_inlay_hint_sources`](crate::Document::set_inlay_hint_sources)). With none,
//!   or with `editor.inlayHints.enabled` off, nothing is asked and nothing shows.
//! - The view asks for the hints of the lines around what is visible (VS Code: the visible
//!   ranges and one viewport above and below, plus 30 lines) with an [`InlayHintsRequest`],
//!   drained with [`EditorView::take_inlay_hints_outputs`](crate::EditorView::take_inlay_hints_outputs)
//!   (egui_quire turns it into `EditorEvent::InlayHintsRequested`). Requests are debounced by
//!   how long the provider took recently (25 to 625 ms), and repeated when the text changes,
//!   when the visible lines change and when the host says its hints changed
//!   ([`EditorView::refresh_inlay_hints`](crate::EditorView::refresh_inlay_hints), VS Code
//!   `onDidChangeInlayHints`).
//! - The host answers with
//!   [`EditorView::set_inlay_hints`](crate::EditorView::set_inlay_hints)`(doc, id, version,
//!   hints)`: every provider's hints for the requested ranges. Positions are mapped forward if
//!   the text moved on; an answer older than the recent-changes window is refused as
//!   [`AnswerRefusal::Stale`], one the view no longer waits for as
//!   [`AnswerRefusal::Superseded`]. The answer replaces the hints inside the requested ranges.
//! - A hint is attached to the word (or token) at its position, before or after it, and moves
//!   with it as the text is edited. What shows comes from [`InlayHintsState::line_hints`]: the
//!   labels cut to `editor.inlayHints.maximumLength` per line, padding, and, for a while after
//!   typing, the old lengths of the hints before the caret (so the caret does not jump).
//! - Gestures (in egui_quire): Ctrl+click on a label part with a command or a location emits
//!   it for the host; a double click on a hint with text edits applies them
//!   ([`InlayHintsState::edits_transaction`]); the mouse over a hint shows its tooltip.
//!
//! Translated from `editor/contrib/inlayHints/browser/inlayHintsController.ts` and
//! `inlayHints.ts` at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`. Departures:
//! hints are not resolved lazily (the host answers with complete hints); the padding
//! whitespace is one space wide (VS Code: a third of the font size); hints keep the editor's
//! font.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use crate::change::{Bias, ChangeSet};
use crate::completion::CompletionCommand;
use crate::document::Document;
use crate::hover::{AnswerRefusal, Timer};
use crate::injected::CursorStops;
use crate::parameter_hints::Documentation;
use crate::version::Version;
use std::collections::VecDeque;
use std::ops::Range;

/// What a hint shows (Monaco `InlayHintKind`); it picks the colours.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum InlayHintKind {
    /// A type annotation.
    Type,
    /// A parameter name.
    Parameter,
}

/// A location a label part links to (Monaco `Location`): a document the host knows and a
/// byte range in it.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHintLocation {
    /// The document (host-defined, e.g. a URI).
    pub uri: String,
    /// Bytes in that document.
    pub range: Range<usize>,
}

/// One part of a hint's label (Monaco `InlayHintLabelPart`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHintLabelPart {
    /// The text.
    pub label: String,
    /// Shown when the mouse is over this part.
    pub tooltip: Option<Documentation>,
    /// Run by Ctrl+click (the host's registry).
    pub command: Option<CompletionCommand>,
    /// Opened by Ctrl+click (go to definition); wins over `command`.
    pub location: Option<InlayHintLocation>,
}

impl InlayHintLabelPart {
    /// A part with text only.
    pub fn text(label: impl Into<String>) -> Self {
        InlayHintLabelPart {
            label: label.into(),
            ..Default::default()
        }
    }
}

/// An edit a hint applies when double-clicked (`InlayHint.textEdits`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHintEdit {
    /// Bytes at the answer's version.
    pub range: Range<usize>,
    /// The replacement.
    pub text: String,
}

/// One hint, as a provider supplies it (Monaco `languages.InlayHint`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHint {
    /// Where it shows (bytes at the answer's version).
    pub position: usize,
    /// The label (a plain label is one part).
    pub label: Vec<InlayHintLabelPart>,
    /// Shown when the mouse is over the hint (a part's tooltip wins).
    pub tooltip: Option<Documentation>,
    /// Type or parameter hint (colours).
    pub kind: Option<InlayHintKind>,
    /// Whitespace before the label.
    pub padding_left: bool,
    /// Whitespace after the label.
    pub padding_right: bool,
    /// Edits a double click applies (e.g. inserting the hint as code).
    pub text_edits: Vec<InlayHintEdit>,
}

impl InlayHint {
    /// A hint with a plain label.
    pub fn new(position: usize, label: impl Into<String>) -> Self {
        InlayHint {
            position,
            label: vec![InlayHintLabelPart::text(label)],
            ..Default::default()
        }
    }
}

/// `editor.inlayHints.enabled`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum InlayHintsEnabled {
    /// Shown.
    #[default]
    On,
    /// Shown, hidden while Ctrl+Alt (Ctrl+Option) is held.
    OnUnlessPressed,
    /// Hidden, shown while Ctrl+Alt (Ctrl+Option) is held.
    OffUnlessPressed,
    /// Off: nothing is asked.
    Off,
}

/// The inlay hint options (VS Code `editor.inlayHints.*`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHintsConfig {
    /// `editor.inlayHints.enabled`.
    pub enabled: InlayHintsEnabled,
    /// `editor.inlayHints.padding`: a rounded background with padding around each hint.
    pub padding: bool,
    /// `editor.inlayHints.maximumLength`: the characters of hints per line before they are
    /// cut with `…` (`0`: never).
    pub maximum_length: usize,
}

impl Default for InlayHintsConfig {
    fn default() -> Self {
        InlayHintsConfig {
            enabled: InlayHintsEnabled::On,
            padding: false,
            maximum_length: 43,
        }
    }
}

/// Identifies an inlay hints request; answers carry it back.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InlayHintsRequestId(pub u64);

/// A request for the hints in some ranges, for the host's providers (VS Code
/// `provideInlayHints`, once per range and provider).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InlayHintsRequest {
    /// Answer with this id.
    pub id: InlayHintsRequestId,
    /// Byte ranges at `version`, sorted and disjoint (whole lines).
    pub ranges: Vec<Range<usize>>,
    /// The document version.
    pub version: Version,
}

/// What the inlay hints want from the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InlayHintsOutput {
    /// Ask the providers.
    Request(InlayHintsRequest),
}

/// The text of one injection of a hint: a label part, or padding whitespace.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HintPiece {
    /// What shows.
    pub text: String,
    /// The label part it shows (`None`: padding or the fill that keeps a fixed length).
    pub part: Option<usize>,
    /// Padding whitespace (`paddingLeft`/`paddingRight`).
    pub padding: bool,
    /// Where the caret may rest next to it.
    pub cursor_stops: CursorStops,
}

/// A hint as it shows in a line: where it goes and its pieces.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LineHint {
    /// The document position it shows at.
    pub pos: usize,
    /// The hint ([`InlayHintsState::hint`]).
    pub hint: usize,
    /// Its kind (colours).
    pub kind: Option<InlayHintKind>,
    /// Its injections, in order.
    pub pieces: Vec<HintPiece>,
}

/// A shown hint and the range it is attached to (VS Code `InlayHintItem` and its decoration).
#[derive(Clone, Debug)]
struct Item {
    hint: InlayHint,
    /// The word or token range (grows when typing at its edges).
    anchor: Range<usize>,
    /// Shown after the anchor (else before it).
    after: bool,
    /// The anchor was empty (an empty anchor that collapsed hides the hint otherwise).
    show_if_collapsed: bool,
    /// The length its label keeps while typing goes on before it (`itemFixedLengths`).
    fixed_len: Option<usize>,
}

impl Item {
    fn pos(&self) -> usize {
        if self.after {
            self.anchor.end
        } else {
            self.anchor.start
        }
    }

    fn shown(&self) -> bool {
        self.show_if_collapsed || self.anchor.start < self.anchor.end
    }
}

/// `DynamicCssRules`-free render mode (VS Code `RenderMode`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum RenderMode {
    Normal,
    Invisible,
}

/// The pending request.
#[derive(Clone, Debug)]
struct Pending {
    id: InlayHintsRequestId,
    ranges: Vec<Range<usize>>,
    sent_at: Option<u64>,
}

/// `_cursorInfo`: where the caret was when the text changed, and until when the hints before it
/// keep their lengths.
#[derive(Clone, Copy, Debug)]
struct CursorInfo {
    pos: usize,
    not_earlier_than: Option<u64>,
    wait: u64,
}

/// VS Code's debounce for inlay hint requests: min 25 ms, max 625 ms, default 37 ms.
const DEBOUNCE_MIN: u64 = 25;
const DEBOUNCE_MAX: u64 = 625;
const DEBOUNCE_DEFAULT: u64 = 37;
/// `_MAX_DECORATORS`.
const MAX_DECORATORS: usize = 1500;
/// Lines asked beyond the visible ones (`_getHintsRanges`' `extra`).
const EXTRA_LINES: usize = 30;

/// The inlay hints of one view (VS Code `InlayHintsController`).
#[derive(Clone, Debug)]
pub struct InlayHintsState {
    items: Vec<Item>,
    /// The `RunOnceScheduler` (deadline of the next request).
    scheduler: Option<Timer>,
    /// Its delay (the debounce).
    delay: u64,
    /// Recent provider durations (`SlidingWindowAverage(6)`).
    durations: VecDeque<u64>,
    pending: Option<Pending>,
    cursor: Option<CursorInfo>,
    /// Refetch at once when the typing pause ends.
    cursor_timer: Option<Timer>,
    /// The text changed since the last tick.
    content_changed: bool,
    /// A double click applied a hint's edits: ask at once after that change.
    refetch_after_edit: bool,
    /// Visible lines (from the widget; `None`: the whole document).
    visible: Option<Range<usize>>,
    /// Ctrl+Alt held (the `UnlessPressed` modes).
    pressed: bool,
    mode: RenderMode,
    /// What the session was started with (`_update`): enabled, providers declared.
    session: Option<(InlayHintsEnabled, bool)>,
    next_id: u64,
    outputs: Vec<InlayHintsOutput>,
    /// The host time of the last tick.
    now: Option<u64>,
}

impl Default for InlayHintsState {
    fn default() -> Self {
        InlayHintsState {
            items: Vec::new(),
            scheduler: None,
            delay: DEBOUNCE_DEFAULT,
            durations: VecDeque::new(),
            pending: None,
            cursor: None,
            cursor_timer: None,
            content_changed: false,
            refetch_after_edit: false,
            visible: None,
            pressed: false,
            mode: RenderMode::Normal,
            session: None,
            next_id: 0,
            outputs: Vec::new(),
            now: None,
        }
    }
}

impl InlayHintsState {
    /// The number of hints held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no hint is held.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Hint `i` (an index from [`LineHint::hint`]).
    pub fn hint(&self, i: usize) -> Option<&InlayHint> {
        self.items.get(i).map(|it| &it.hint)
    }

    /// The position hint `i` shows at now.
    pub fn hint_pos(&self, i: usize) -> Option<usize> {
        self.items.get(i).map(Item::pos)
    }

    /// The request waiting for an answer.
    pub fn pending_request(&self) -> Option<InlayHintsRequestId> {
        self.pending.as_ref().map(|p| p.id)
    }

    /// The current request debounce (ms).
    pub fn delay(&self) -> u64 {
        self.delay
    }

    /// Whether a request is scheduled.
    pub fn is_scheduled(&self) -> bool {
        self.scheduler.is_some()
    }

    /// Whether hints are being fetched or a fetch is scheduled.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some() || self.scheduler.is_some() || self.cursor_timer.is_some()
    }

    /// Takes what the hints want from the host.
    pub fn take_outputs(&mut self) -> Vec<InlayHintsOutput> {
        std::mem::take(&mut self.outputs)
    }

    /// The earliest deadline (host clock, ms) at which [`InlayHintsState::tick`] has work.
    pub fn next_deadline(&self) -> Option<u64> {
        [self.scheduler, self.cursor_timer]
            .iter()
            .flatten()
            .filter_map(Timer::at)
            .min()
    }

    /// Whether a timer waits to be placed on the clock (call [`InlayHintsState::tick`]).
    pub fn needs_tick(&self) -> bool {
        self.content_changed
            || self.session.is_none()
            || [self.scheduler, self.cursor_timer]
                .iter()
                .flatten()
                .any(|t| t.at().is_none())
    }

    /// `scheduler.schedule(delay)`: (re)starts the request timer.
    fn schedule(&mut self, delay: Option<u64>) {
        let d = delay.unwrap_or(self.delay);
        self.scheduler = Some(Timer::new(self.now, d));
    }

    /// VS Code `_update`: the session restarts when the options or the providers change.
    fn sync_session(&mut self, doc: &Document) {
        let cfg = &doc.config().inlay_hints;
        let key = (cfg.enabled, !doc.inlay_hint_sources().is_empty());
        if self.session == Some(key) {
            return;
        }
        self.session = Some(key);
        self.items.clear();
        self.pending = None;
        self.scheduler = None;
        self.cursor_timer = None;
        self.cursor = None;
        if cfg.enabled == InlayHintsEnabled::Off || !key.1 {
            return;
        }
        self.mode = self.render_mode(cfg.enabled);
        self.schedule(Some(0));
    }

    fn render_mode(&self, enabled: InlayHintsEnabled) -> RenderMode {
        let (default, alt) = match enabled {
            InlayHintsEnabled::On => return RenderMode::Normal,
            InlayHintsEnabled::OnUnlessPressed => (RenderMode::Normal, RenderMode::Invisible),
            _ => (RenderMode::Invisible, RenderMode::Normal),
        };
        if self.pressed { alt } else { default }
    }

    fn active(&self) -> bool {
        matches!(self.session, Some((e, true)) if e != InlayHintsEnabled::Off)
    }

    /// The visible lines changed (VS Code `onDidScrollChange`: a scroll reschedules; so does
    /// any change while nothing is scheduled).
    pub fn set_visible_lines(&mut self, lines: Range<usize>) {
        if self.visible.as_ref() == Some(&lines) {
            return;
        }
        let top_changed = self.visible.as_ref().is_none_or(|v| v.start != lines.start);
        self.visible = Some(lines);
        if self.active() && (top_changed || self.scheduler.is_none()) {
            self.schedule(None);
        }
    }

    /// Ctrl+Alt (without Shift or Meta) pressed or released: the `UnlessPressed` modes show or
    /// hide the hints, and they are fetched again.
    pub fn set_modifier_pressed(&mut self, doc: &Document, pressed: bool) {
        self.pressed = pressed;
        if !self.active() {
            return;
        }
        let mode = self.render_mode(doc.config().inlay_hints.enabled);
        if mode != self.mode {
            self.mode = mode;
            self.schedule(Some(0));
        }
    }

    /// The host's providers changed their hints (VS Code `onDidChangeInlayHints`).
    pub fn refresh(&mut self) {
        if self.active() && self.scheduler.is_none() {
            self.schedule(None);
        }
    }

    /// Maps the anchors through a change (`AlwaysGrowsWhenTypingAtEdges`; a non-empty anchor
    /// inside a replaced range collapses, `collapseOnReplaceEdit`) and cancels the request in
    /// flight.
    pub(crate) fn map(&mut self, c: &ChangeSet) {
        let edits: Vec<(Range<usize>, bool)> = c
            .edits()
            .iter()
            .map(|e| (e.range.clone(), !e.insert.is_empty()))
            .collect();
        for it in &mut self.items {
            let a = it.anchor.clone();
            let replaced = a.start < a.end
                && edits.iter().any(|(r, ins)| {
                    *ins && r.start <= a.start && a.end <= r.end && r.start < r.end
                });
            let s = c.map_pos(a.start, Bias::Before);
            it.anchor = if replaced {
                s..s
            } else {
                s..c.map_pos(a.end, Bias::After).max(s)
            };
        }
        self.content_changed = true;
    }

    /// The view could not map its state (older than the recent-changes window): the hints
    /// go and are asked again.
    pub(crate) fn clamped(&mut self) {
        self.items.clear();
        self.content_changed = true;
    }

    /// VS Code `onDidChangeModelContent`: the request in flight is dropped, the hints before
    /// the caret keep their lengths for a while, and a request is scheduled (and one more,
    /// without delay, when the pause ends).
    fn on_content_change(&mut self, cursor: usize) {
        self.pending = None;
        if !self.active() {
            return;
        }
        let wait = self.delay.max(800);
        self.cursor = Some(CursorInfo {
            pos: cursor,
            not_earlier_than: self.now.map(|n| n + wait),
            wait,
        });
        self.cursor_timer = Some(Timer::new(self.now, wait));
        self.schedule(None);
    }

    /// Runs what is due at `now` (host clock, ms) and places pending timers on the clock.
    pub fn tick(&mut self, doc: &Document, cursor: usize, now: u64) {
        self.now = Some(now);
        self.sync_session(doc);
        for t in [&mut self.scheduler, &mut self.cursor_timer]
            .into_iter()
            .flatten()
        {
            t.stamp(now);
        }
        if let Some(c) = &mut self.cursor
            && c.not_earlier_than.is_none()
        {
            c.not_earlier_than = Some(now + c.wait);
        }
        if std::mem::take(&mut self.content_changed) {
            self.on_content_change(cursor);
        }
        if std::mem::take(&mut self.refetch_after_edit) && self.active() {
            self.schedule(Some(0));
        }
        if self.cursor_timer.is_some_and(|t| t.due(now)) {
            self.cursor_timer = None;
            self.schedule(Some(0));
        }
        if self.scheduler.is_some_and(|t| t.due(now)) {
            self.scheduler = None;
            self.request(doc);
        }
    }

    /// `_getHintsRanges`: the visible lines and 30 more on each side, as whole-line byte
    /// ranges (the whole document when the visible lines are unknown).
    fn hints_ranges(&self, doc: &Document) -> Vec<Range<usize>> {
        let text = doc.text();
        let n = text.len_lines();
        let lines = match &self.visible {
            Some(v) => {
                // the viewport above and below (`getVisibleRangesPlusViewportAboveBelow`)
                let h = v.end.saturating_sub(v.start);
                v.start.saturating_sub(h + EXTRA_LINES)..(v.end + h + EXTRA_LINES).min(n)
            }
            None => 0..n,
        };
        let last = lines.end.max(lines.start + 1).min(n).saturating_sub(1);
        let start = text.line_to_byte(lines.start.min(n.saturating_sub(1)));
        let end = text.line_range(last).end;
        std::iter::once(start..end.max(start)).collect()
    }

    fn request(&mut self, doc: &Document) {
        if !self.active() {
            return;
        }
        self.next_id += 1;
        let id = InlayHintsRequestId(self.next_id);
        let ranges = self.hints_ranges(doc);
        self.pending = Some(Pending {
            id,
            ranges: ranges.clone(),
            sent_at: self.now,
        });
        self.outputs
            .push(InlayHintsOutput::Request(InlayHintsRequest {
                id,
                ranges,
                version: doc.version(),
            }));
    }

    /// The host's answer to an [`InlayHintsRequest`]: every provider's hints in the requested
    /// ranges (positions and edits in bytes at `at`).
    pub fn set_inlay_hints(
        &mut self,
        doc: &Document,
        id: InlayHintsRequestId,
        at: Version,
        hints: Vec<InlayHint>,
    ) -> Result<(), AnswerRefusal> {
        let Some(p) = self.pending.clone().filter(|p| p.id == id) else {
            return Err(AnswerRefusal::Superseded);
        };
        if at > doc.version() {
            return Err(AnswerRefusal::Stale);
        }
        let since: Vec<ChangeSet> = doc
            .recent()
            .since(at)
            .map_err(|_| AnswerRefusal::Stale)?
            .map(|e| e.changes.clone())
            .collect();
        self.pending = None;
        // the debounce follows how long the provider took (`_debounceInfo.update`)
        if let (Some(sent), Some(now)) = (p.sent_at, self.now) {
            if self.durations.len() == 6 {
                self.durations.pop_front();
            }
            self.durations.push_back(now.saturating_sub(sent));
            let avg = self.durations.iter().sum::<u64>() / self.durations.len() as u64;
            self.delay = avg.clamp(DEBOUNCE_MIN, DEBOUNCE_MAX);
        }
        let len = doc.text().len_bytes();
        let map = |mut pos: usize| {
            for c in &since {
                pos = c.map_pos(pos.min(c.len()), Bias::Before);
            }
            pos.min(len)
        };
        let ranges: Vec<Range<usize>> = p
            .ranges
            .iter()
            .map(|r| {
                let mut r = r.clone();
                for c in &since {
                    r = c.map_pos(r.start.min(c.len()), Bias::Before)
                        ..c.map_pos(r.end.min(c.len()), Bias::After);
                }
                r
            })
            .collect();
        let mut items: Vec<Item> = hints
            .into_iter()
            .map(|mut h| {
                h.position = map(h.position);
                for e in &mut h.text_edits {
                    let s = map(e.range.start);
                    e.range = s..map(e.range.end).max(s);
                }
                let (anchor, after) = anchor_of(doc, h.position);
                let show_if_collapsed = anchor.start == anchor.end;
                Item {
                    hint: h,
                    anchor,
                    after,
                    show_if_collapsed,
                    fixed_len: None,
                }
            })
            .collect();
        items.sort_by_key(|it| it.hint.position);
        self.update_items(doc, &ranges, items);
        Ok(())
    }

    /// `_updateHintsDecorators`: the items replace those inside `ranges`.
    fn update_items(&mut self, doc: &Document, ranges: &[Range<usize>], mut items: Vec<Item>) {
        let contains = |r: &Range<usize>, a: &Range<usize>| r.start <= a.start && a.end <= r.end;
        if let Some(ci) = self.cursor
            && ci
                .not_earlier_than
                .is_some_and(|t| self.now.is_some_and(|n| t > n))
            && ranges.iter().any(|r| r.start <= ci.pos && ci.pos <= r.end)
        {
            // Hints on the caret's line before it keep the lengths they showed with.
            self.cursor = None;
            let text = doc.text();
            let line = text.byte_to_line(ci.pos.min(text.len_bytes()));
            let lengths: Vec<usize> = self
                .line_hints(doc, line)
                .iter()
                .filter(|lh| self.items[lh.hint].anchor.start <= ci.pos)
                .map(|lh| {
                    lh.pieces
                        .iter()
                        .filter(|p| !p.padding)
                        .map(|p| p.text.chars().count())
                        .sum()
                })
                .collect();
            let mut targets: VecDeque<usize> = items
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    text.byte_to_line(it.anchor.start.min(text.len_bytes())) == line
                        && it.anchor.end <= ci.pos
                })
                .map(|(i, _)| i)
                .collect();
            let mut fixed: VecDeque<usize> = lengths.into();
            loop {
                let target = targets.pop_front();
                let len = fixed.pop_front();
                if len.unwrap_or(0) == 0 && target.is_none() {
                    break;
                }
                if let Some(t) = target {
                    items[t].fixed_len = Some(len.unwrap_or(0));
                } else if len.is_some_and(|l| l > 0) {
                    // VS Code adds the rest to the last item's length but does not store it.
                    break;
                }
            }
        }
        // `_MAX_DECORATORS`: one decoration per padding and label part.
        let mut count = 0;
        let mut keep = items.len();
        for (i, it) in items.iter().enumerate() {
            count += it.hint.label.len().max(1)
                + usize::from(it.hint.padding_left)
                + usize::from(it.hint.padding_right);
            if count > MAX_DECORATORS {
                keep = i + 1;
                break;
            }
        }
        items.truncate(keep);
        self.items
            .retain(|it| !ranges.iter().any(|r| contains(r, &it.anchor)));
        self.items.extend(items);
        self.items.sort_by_key(Item::pos);
    }

    /// The hints showing in `line`, cut to `maximumLength` per line (VS Code
    /// `_updateHintsDecorators`' rendering); empty while hidden (`UnlessPressed` modes).
    pub fn line_hints(&self, doc: &Document, line: usize) -> Vec<LineHint> {
        if self.mode == RenderMode::Invisible || !self.active() {
            return Vec::new();
        }
        let text = doc.text();
        if line >= text.len_lines() {
            return Vec::new();
        }
        let lr = text.line_range(line);
        let max = doc.config().inlay_hints.maximum_length;
        let mut total = 0usize;
        let mut out = Vec::new();
        let first = self.items.partition_point(|it| it.pos() < lr.start);
        for (i, it) in self.items.iter().enumerate().skip(first) {
            let pos = it.pos();
            if pos > lr.end {
                break;
            }
            if !it.shown() || text.byte_to_line(it.anchor.start.min(text.len_bytes())) != line {
                continue;
            }
            if max != 0 && total > max {
                continue;
            }
            let h = &it.hint;
            let mut pieces = Vec::new();
            if h.padding_left {
                pieces.push(HintPiece {
                    text: " ".into(),
                    part: None,
                    padding: true,
                    cursor_stops: CursorStops::None,
                });
            }
            let mut actual = 0usize;
            let n = h.label.len();
            for (k, part) in h.label.iter().enumerate() {
                let is_last = k + 1 == n;
                let mut label: String = part.label.replace('\t', " ");
                let len = label.chars().count();
                total += len;
                let mut too_long = false;
                let over = if max != 0 {
                    total.saturating_sub(max)
                } else {
                    0
                };
                if over > 0 {
                    label = cut(&label, over) + "…";
                    too_long = true;
                }
                actual += label.chars().count();
                if let Some(fixed) = it.fixed_len
                    && actual >= fixed
                {
                    let over_fixed = actual - fixed;
                    actual -= over_fixed;
                    label = cut(&label, 1 + over_fixed) + "…";
                    too_long = true;
                }
                pieces.push(HintPiece {
                    text: label,
                    part: Some(k),
                    padding: false,
                    cursor_stops: if is_last && !h.padding_right {
                        CursorStops::Right
                    } else {
                        CursorStops::None
                    },
                });
                if too_long {
                    break;
                }
            }
            if let Some(fixed) = it.fixed_len
                && actual < fixed
            {
                pieces.push(HintPiece {
                    text: " ".repeat(fixed - actual),
                    part: None,
                    padding: false,
                    cursor_stops: CursorStops::None,
                });
            }
            if h.padding_right {
                pieces.push(HintPiece {
                    text: " ".into(),
                    part: None,
                    padding: true,
                    cursor_stops: CursorStops::Right,
                });
            }
            out.push(LineHint {
                pos,
                hint: i,
                kind: h.kind,
                pieces,
            });
        }
        out
    }

    /// VS Code `_installDblClickGesture`: the edits a double click on hint `i` applies (`None`
    /// without edits). A request follows at once.
    pub fn edits_transaction(
        &mut self,
        doc: &Document,
        i: usize,
    ) -> Option<crate::transaction::Transaction> {
        let it = self.items.get(i)?;
        if it.hint.text_edits.is_empty() {
            return None;
        }
        let len = doc.text().len_bytes();
        let edits: Vec<(Range<usize>, String)> = it
            .hint
            .text_edits
            .iter()
            .map(|e| (e.range.start.min(len)..e.range.end.min(len), e.text.clone()))
            .collect();
        let cs = ChangeSet::replace(len, edits).ok()?;
        self.refetch_after_edit = true;
        Some(
            crate::transaction::Transaction::new(doc.version(), cs).with_origin(
                crate::transaction::Origin::Command(std::borrow::Cow::Borrowed(
                    "inlayHint.default",
                )),
            ),
        )
    }
}

/// `label.slice(0, -n)` in characters.
fn cut(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().take(count.saturating_sub(n)).collect()
}

/// VS Code `InlayHintsFragments`: the range a hint at `pos` is attached to (the word at it,
/// else the token) and whether it shows after that range.
fn anchor_of(doc: &Document, pos: usize) -> (Range<usize>, bool) {
    let (start, end) = range_at_position(doc, pos);
    if start < pos {
        (start..pos, true)
    } else {
        (pos..end.max(pos), false)
    }
}

/// `_getRangeAtPosition`: the word at `pos`, else its token (a one-character token at whose
/// edge `pos` is gives way to the token before or after).
fn range_at_position(doc: &Document, pos: usize) -> (usize, usize) {
    let text = doc.text();
    let line = text.byte_to_line(pos.min(text.len_bytes()));
    let lr = text.line_range(line);
    let s = text.slice(lr.clone()).to_string();
    let col = pos - lr.start;
    if let Some(w) = crate::completion::session::word_at(&s, col, &doc.config().word_separators) {
        return (lr.start + w.start, lr.start + w.end);
    }
    let tokens = doc.tokens();
    let toks: Vec<usize> = if line < tokens.len_lines() {
        tokens.line(line).iter().map(|t| t.start as usize).collect()
    } else {
        Vec::new()
    };
    let starts: Vec<usize> = if toks.first() == Some(&0) {
        toks
    } else {
        std::iter::once(0).chain(toks).collect()
    };
    let end_of = |i: usize| starts.get(i + 1).copied().unwrap_or(s.len()).min(s.len());
    let idx = starts.partition_point(|&t| t <= col).max(1) - 1;
    let (mut a, mut b) = (starts[idx].min(s.len()), end_of(idx));
    if b - a == 1 {
        if a == col && idx > 1 {
            a = starts[idx - 1];
            b = end_of(idx - 1);
        } else if b == col && idx + 1 < starts.len() {
            a = starts[idx + 1];
            b = end_of(idx + 1);
        }
    }
    (lr.start + a, lr.start + b)
}
