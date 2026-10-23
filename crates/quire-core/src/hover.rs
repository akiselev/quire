//! Hover (VS Code's content hover): the data model, the delayed computation and the shown
//! result of one view. The host supplies the provider; quire has no language client.
//!
//! **The host seam.**
//! - Hover providers are declared on the document
//!   ([`Document::set_hover_sources`](crate::Document::set_hover_sources)). With none, a hover
//!   shows diagnostics only.
//! - When a hover starts, the view asks for content with a [`HoverRequest`] (drained with
//!   [`EditorView::take_hover_outputs`](crate::EditorView::take_hover_outputs); egui_quire turns it
//!   into `EditorEvent::HoverRequested`). It names the request id, the position and the document
//!   version.
//! - The host answers, now or later, with
//!   [`EditorView::set_hover`](crate::EditorView::set_hover)`(doc, id, version, hovers)`. Ranges are
//!   mapped forward if the text moved on; an answer older than the recent-changes window is refused
//!   as [`AnswerRefusal::Stale`], an answer to a request the view no longer waits for as
//!   [`AnswerRefusal::Superseded`].
//! - quire-core reads no clock: the widget passes the time to [`HoverState::tick`] (through
//!   [`EditorView::tick_hover`](crate::EditorView::tick_hover)) and repaints at
//!   [`HoverState::next_deadline`].
//!
//! Translated from `editor/contrib/hover/browser/hoverOperation.ts` (the delayed computation),
//! `contentHoverWidgetWrapper.ts` (showing and updating the result), `contentHoverRendered.ts`
//! (`computeHoverPositions`), `hoverUtils.ts`, `markerHoverParticipant.ts` and
//! `markdownHoverParticipant.ts` at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`.
//! The mouse policy of `contentHoverController.ts` (sticky hover, hiding delay, which keys hide)
//! lives in egui_quire, which has the geometry.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use crate::change::ChangeSet;
use crate::diagnostics::{Diagnostic, SourceId, map_range};
use crate::document::Document;
use crate::version::Version;
use std::ops::Range;

/// One provider's hover (Monaco `languages.Hover`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Hover {
    /// The provider.
    pub source: SourceId,
    /// Markdown parts, shown one under the other (`contents`).
    pub contents: Vec<String>,
    /// The range the hover applies to (bytes at the answer's version); the request position
    /// when absent.
    pub range: Option<Range<usize>>,
}

/// Identifies a hover request; answers carry it back.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HoverRequestId(pub u64);

/// A request for hover content, for the host's providers (VS Code `provideHover`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HoverRequest {
    /// Answer with this id.
    pub id: HoverRequestId,
    /// The position (bytes at `version`).
    pub pos: usize,
    /// The document version.
    pub version: Version,
}

/// Why an answer (hover or signature help) was refused. Nothing changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnswerRefusal {
    /// The view no longer waits for this request.
    Superseded,
    /// The version is older than the recent-changes window (or newer than the document).
    Stale,
}

/// `editor.hover.enabled`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HoverEnabled {
    /// The mouse shows hovers.
    #[default]
    On,
    /// Only `editor.action.showHover` does.
    Off,
    /// The mouse shows hovers while the modifier that is not the multi-cursor modifier is held.
    OnKeyboardModifier,
}

/// The hover options (VS Code `editor.hover.*`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HoverConfig {
    /// `editor.hover.enabled`.
    pub enabled: HoverEnabled,
    /// `editor.hover.delay`: milliseconds the mouse rests before the hover shows.
    pub delay: u64,
    /// `editor.hover.sticky`: the mouse can move onto the hover.
    pub sticky: bool,
    /// `editor.hover.hidingDelay`: milliseconds a sticky hover stays while the mouse moves away.
    pub hiding_delay: u64,
    /// `editor.hover.above`: prefer showing above the line.
    pub above: bool,
}

impl Default for HoverConfig {
    fn default() -> Self {
        HoverConfig {
            enabled: HoverEnabled::On,
            delay: 300,
            sticky: true,
            hiding_delay: 300,
            above: true,
        }
    }
}

/// VS Code's `multiCursorModifier` as hoverUtils reads it.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum MultiCursorModifier {
    /// `altKey` (VS Code's default, and quire's: Alt+click adds a cursor).
    Alt,
    /// `ctrlKey`.
    Ctrl,
    /// `metaKey`.
    Meta,
}

/// The modifier state of a mouse or key event.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct EventModifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Meta / Command.
    pub meta: bool,
}

/// VS Code `isTriggerModifierPressed`: with the multi-cursor modifier on Alt, Ctrl or Meta
/// triggers; otherwise Alt does.
pub fn is_trigger_modifier_pressed(mcm: MultiCursorModifier, m: EventModifiers) -> bool {
    match mcm {
        MultiCursorModifier::Alt => m.ctrl || m.meta,
        _ => m.alt,
    }
}

/// VS Code `shouldShowHover`: whether a mouse move may show the hover.
pub fn should_show_hover(
    enabled: HoverEnabled,
    mcm: MultiCursorModifier,
    m: EventModifiers,
) -> bool {
    match enabled {
        HoverEnabled::On => true,
        HoverEnabled::Off => false,
        HoverEnabled::OnKeyboardModifier => is_trigger_modifier_pressed(mcm, m),
    }
}

/// VS Code `isMousePositionWithinElement`: strictly inside the rectangle (edges are outside).
pub fn is_position_within(left: f32, top: f32, width: f32, height: f32, x: f32, y: f32) -> bool {
    x > left && x < left + width && y > top && y < top + height
}

/// A 0-based line and byte column.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LineCol {
    /// The line.
    pub line: usize,
    /// The byte column.
    pub col: usize,
}

impl LineCol {
    /// A position.
    pub fn new(line: usize, col: usize) -> Self {
        LineCol { line, col }
    }
}

/// VS Code `RenderedContentHover.computeHoverPositions`: where the hover is placed
/// (`showAtPosition`) and where it may fall back to when it does not fit
/// (`showAtSecondaryPosition`: the leftmost start of the parts on the anchor's line, but not
/// left of the visual row the anchor is on). `row_start_col` is the column the anchor's visual
/// row starts at (0 without wrap); `parts` are the parts' ranges with `forceShowAtRange`.
pub fn compute_hover_positions(
    anchor: Range<LineCol>,
    row_start_col: usize,
    parts: &[(Range<LineCol>, bool)],
) -> (LineCol, LineCol) {
    let line = anchor.start.line;
    let mut secondary = anchor.start.col;
    let mut force = None;
    for (r, force_show) in parts {
        if r.start.line == line && r.end.line == line {
            secondary = secondary.min(r.start.col).max(row_start_col);
        }
        if *force_show {
            force = Some(r.start);
        }
    }
    match force {
        Some(p) => (p, p),
        None => (anchor.start, LineCol::new(line, secondary)),
    }
}

/// VS Code `HoverStartMode`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HoverStartMode {
    /// After `editor.hover.delay` (the mouse).
    Delayed,
    /// Now (`editor.action.showHover`).
    Immediate,
}

/// VS Code `HoverStartSource`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HoverStartSource {
    /// The mouse rested.
    Mouse,
    /// The keyboard (`editor.action.showHover`).
    Keyboard,
}

/// What the hover is about (VS Code `HoverRangeAnchor`): a byte range on one line, empty at the
/// position for the mouse and the cursor.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct HoverAnchor {
    /// The range.
    pub range: Range<usize>,
}

/// One part of a hover, in the order shown: diagnostics first, then the providers' content
/// (VS Code's participant order), then "Loading...".
#[derive(Clone, PartialEq, Debug)]
pub enum HoverPart {
    /// A diagnostic under the anchor (VS Code `MarkerHover`).
    Diagnostic {
        /// The diagnostic's range clipped to the anchor's line.
        range: Range<usize>,
        /// Its source.
        source: SourceId,
        /// The diagnostic.
        diagnostic: Diagnostic,
    },
    /// A provider's hover (VS Code `MarkdownHover`).
    Markdown {
        /// The range it applies to.
        range: Range<usize>,
        /// The provider.
        source: SourceId,
        /// Markdown parts.
        contents: Vec<String>,
        /// The provider's order in the answer.
        ordinal: usize,
    },
    /// The providers have not answered after three hover delays (VS Code's "Loading...").
    Loading {
        /// The anchor's range.
        range: Range<usize>,
    },
}

impl HoverPart {
    /// The range the part applies to.
    pub fn range(&self) -> &Range<usize> {
        match self {
            HoverPart::Diagnostic { range, .. }
            | HoverPart::Markdown { range, .. }
            | HoverPart::Loading { range } => range,
        }
    }

    fn range_mut(&mut self) -> &mut Range<usize> {
        match self {
            HoverPart::Diagnostic { range, .. }
            | HoverPart::Markdown { range, .. }
            | HoverPart::Loading { range } => range,
        }
    }

    /// VS Code `isValidForHoverAnchor`: the part's range covers the anchor.
    pub fn is_valid_for_anchor(&self, anchor: &HoverAnchor) -> bool {
        let r = self.range();
        r.start <= anchor.range.start && r.end >= anchor.range.end
    }

    /// The participant order (markers 1, markdown 3) and the order within it.
    fn order(&self) -> (u8, usize) {
        match self {
            HoverPart::Diagnostic { .. } => (1, 0),
            HoverPart::Markdown { ordinal, .. } => (3, *ordinal),
            HoverPart::Loading { .. } => (3, usize::MAX),
        }
    }

    /// The part as text, for the copy button and screen readers (VS Code
    /// `getAccessibleContent`).
    pub fn accessible_content(&self) -> String {
        match self {
            HoverPart::Diagnostic { diagnostic, .. } => diagnostic.message.clone(),
            HoverPart::Markdown { contents, .. } => contents.join("\n"),
            HoverPart::Loading { .. } => String::new(),
        }
    }
}

/// What a hover shows (VS Code `ContentHoverResult`).
#[derive(Clone, PartialEq, Debug)]
pub struct HoverResult {
    /// The parts, in display order.
    pub parts: Vec<HoverPart>,
    /// Every provider answered.
    pub complete: bool,
    /// What it is about.
    pub anchor: HoverAnchor,
    /// How it started.
    pub source: HoverStartSource,
    /// Take focus when shown.
    pub should_focus: bool,
    insist_on_keeping_visible: bool,
}

impl HoverResult {
    fn filter(&self, anchor: &HoverAnchor) -> HoverResult {
        let mut r = self.clone();
        r.parts.retain(|p| p.is_valid_for_anchor(anchor));
        r
    }
}

/// A scroll request for the shown hover (the hover scroll commands); the widget takes it.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HoverScroll {
    /// `editor.action.scrollUpHover`.
    Up,
    /// `editor.action.scrollDownHover`.
    Down,
    /// `editor.action.scrollLeftHover`.
    Left,
    /// `editor.action.scrollRightHover`.
    Right,
    /// `editor.action.pageUpHover`.
    PageUp,
    /// `editor.action.pageDownHover`.
    PageDown,
    /// `editor.action.goToTopHover`.
    Top,
    /// `editor.action.goToBottomHover`.
    Bottom,
}

/// What a view's hover wants from the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HoverOutput {
    /// Ask the providers.
    Request(HoverRequest),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum OpState {
    Idle,
    FirstWait,
    SecondWait,
    WaitingForAsync,
    WaitingForAsyncShowingLoading,
}

/// A timer: a delay not yet placed on the clock, or a deadline.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Timer {
    After(u64),
    At(u64),
}

impl Timer {
    pub(crate) fn new(now: Option<u64>, delay: u64) -> Self {
        match now {
            Some(n) => Timer::At(n + delay),
            None => Timer::After(delay),
        }
    }

    pub(crate) fn stamp(&mut self, now: u64) {
        if let Timer::After(d) = *self {
            *self = Timer::At(now + d);
        }
    }

    pub(crate) fn due(&self, now: u64) -> bool {
        matches!(*self, Timer::At(t) if t <= now)
    }

    pub(crate) fn at(&self) -> Option<u64> {
        match *self {
            Timer::At(t) => Some(t),
            Timer::After(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
struct OpOptions {
    anchor: HoverAnchor,
    source: HoverStartSource,
    should_focus: bool,
    insist_on_keeping_visible: bool,
}

/// VS Code `HoverOperation`: with the default 300 ms delay, the providers are asked at 150 ms,
/// diagnostics are added and the result shown at 300 ms, "Loading..." is added at 900 ms if the
/// providers have not answered.
#[derive(Clone, Debug)]
struct Operation {
    state: OpState,
    async_timer: Option<Timer>,
    sync_timer: Option<Timer>,
    loading_timer: Option<Timer>,
    request: Option<HoverRequestId>,
    async_done: bool,
    result: Vec<HoverPart>,
    options: Option<OpOptions>,
}

impl Default for Operation {
    fn default() -> Self {
        Operation {
            state: OpState::Idle,
            async_timer: None,
            sync_timer: None,
            loading_timer: None,
            request: None,
            async_done: false,
            result: Vec::new(),
            options: None,
        }
    }
}

/// The hover of one view: the computation in flight and the result shown (VS Code's
/// `ContentHoverWidgetWrapper` with its `HoverOperation`).
#[derive(Clone, Debug, Default)]
pub struct HoverState {
    op: Operation,
    current: Option<HoverResult>,
    focused: bool,
    next_id: u64,
    outputs: Vec<HoverOutput>,
    scroll: Option<HoverScroll>,
}

impl HoverState {
    /// The result shown, if any.
    pub fn shown(&self) -> Option<&HoverResult> {
        self.current.as_ref()
    }

    /// Whether a hover is shown.
    pub fn is_visible(&self) -> bool {
        self.current.is_some()
    }

    /// Whether the hover has keyboard focus (`editorHoverFocused`): the hover scroll commands
    /// apply.
    pub fn is_focused(&self) -> bool {
        self.focused && self.current.is_some()
    }

    /// Whether a computation is under way (a timer runs or providers are awaited).
    pub fn is_pending(&self) -> bool {
        self.op.state != OpState::Idle
    }

    /// The anchor of the computation in flight, else of the result shown.
    pub fn anchor(&self) -> Option<&HoverAnchor> {
        self.op
            .options
            .as_ref()
            .map(|o| &o.anchor)
            .or(self.current.as_ref().map(|c| &c.anchor))
    }

    /// The earliest deadline (host clock, ms) at which [`HoverState::tick`] has work.
    pub fn next_deadline(&self) -> Option<u64> {
        [
            self.op.async_timer,
            self.op.sync_timer,
            self.op.loading_timer,
        ]
        .iter()
        .flatten()
        .filter_map(Timer::at)
        .min()
    }

    /// Whether a timer waits to be placed on the clock (call [`HoverState::tick`]).
    pub fn needs_tick(&self) -> bool {
        [
            self.op.async_timer,
            self.op.sync_timer,
            self.op.loading_timer,
        ]
        .iter()
        .flatten()
        .any(|t| t.at().is_none())
    }

    /// Takes what the hover wants from the host.
    pub fn take_outputs(&mut self) -> Vec<HoverOutput> {
        std::mem::take(&mut self.outputs)
    }

    /// Takes a pending scroll request.
    pub fn take_scroll(&mut self) -> Option<HoverScroll> {
        self.scroll.take()
    }

    pub(crate) fn request_scroll(&mut self, s: HoverScroll) {
        if self.is_focused() {
            self.scroll = Some(s);
        }
    }

    /// Gives the hover keyboard focus (`editor.action.showHover` on a visible hover).
    pub fn focus(&mut self) {
        if self.current.is_some() {
            self.focused = true;
        }
    }

    /// Hides the hover and cancels the computation (VS Code `ContentHoverWidgetWrapper.hide`).
    pub fn hide(&mut self) {
        self.op_cancel();
        self.set_current(None);
    }

    /// VS Code `_startShowingOrUpdateHover`: shows a hover for `anchor` (or hides it when `None`
    /// and the mouse is not moving onto a sticky hover). `getting_closer`: the mouse moves
    /// towards the shown hover and hovers are sticky. `now`: the host clock, if known (else the
    /// next [`HoverState::tick`] starts the timers). Returns whether a hover shows or will show.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        doc: &Document,
        anchor: Option<HoverAnchor>,
        mode: HoverStartMode,
        source: HoverStartSource,
        focus: bool,
        getting_closer: bool,
        now: Option<u64>,
    ) -> bool {
        let Some(cur) = self.current.clone() else {
            if let Some(a) = anchor {
                self.start_op_if_necessary(doc, a, mode, source, focus, false, now);
                return true;
            }
            return false;
        };
        if getting_closer {
            if let Some(a) = anchor {
                self.start_op_if_necessary(doc, a, mode, source, focus, true, now);
            }
            return true;
        }
        let Some(a) = anchor else {
            self.set_current(None);
            return false;
        };
        if cur.anchor == a {
            return true;
        }
        // HoverRangeAnchor.canAdoptVisibleHover: the shown hover is on the anchor's line.
        let text = doc.text();
        let line_of = |p: usize| text.byte_to_line(p.min(text.len_bytes()));
        if line_of(cur.anchor.range.start) != line_of(a.range.start) {
            self.set_current(None);
            self.start_op_if_necessary(doc, a, mode, source, focus, false, now);
            return true;
        }
        self.set_current(Some(cur.filter(&a)));
        self.start_op_if_necessary(doc, a, mode, source, focus, false, now);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn start_op_if_necessary(
        &mut self,
        doc: &Document,
        anchor: HoverAnchor,
        mode: HoverStartMode,
        source: HoverStartSource,
        focus: bool,
        insist: bool,
        now: Option<u64>,
    ) {
        if self.op.options.as_ref().is_some_and(|o| o.anchor == anchor) {
            return;
        }
        self.op_cancel();
        let options = OpOptions {
            anchor,
            source,
            should_focus: focus,
            insist_on_keeping_visible: insist,
        };
        let delay = doc.config().hover.delay;
        match mode {
            HoverStartMode::Delayed => {
                self.op.options = Some(options);
                self.op.state = OpState::FirstWait;
                self.op.async_timer = Some(Timer::new(now, delay / 2));
                self.op.loading_timer = Some(Timer::new(now, 3 * delay));
            }
            HoverStartMode::Immediate => {
                self.trigger_async(doc, options.clone(), now);
                self.op.sync_timer = None;
                self.trigger_sync(doc, options);
            }
        }
    }

    fn op_cancel(&mut self) {
        self.op = Operation::default();
    }

    fn set_state(&mut self, state: OpState, options: OpOptions) {
        self.op.options = Some(options.clone());
        self.op.state = state;
        self.fire_result(options);
    }

    fn trigger_async(&mut self, doc: &Document, options: OpOptions, now: Option<u64>) {
        let second = doc.config().hover.delay - doc.config().hover.delay / 2;
        self.op.async_timer = None;
        self.set_state(OpState::SecondWait, options.clone());
        self.op.sync_timer = Some(Timer::new(now, second));
        if doc.hover_sources().is_empty() {
            self.op.async_done = true;
        } else {
            self.op.async_done = false;
            self.next_id += 1;
            let id = HoverRequestId(self.next_id);
            self.op.request = Some(id);
            self.outputs.push(HoverOutput::Request(HoverRequest {
                id,
                pos: options.anchor.range.start,
                version: doc.version(),
            }));
        }
    }

    fn trigger_sync(&mut self, doc: &Document, options: OpOptions) {
        self.op.sync_timer = None;
        let diags = diagnostic_parts(doc, &options.anchor);
        self.op.result.extend(diags);
        let state = if self.op.async_done {
            OpState::Idle
        } else {
            OpState::WaitingForAsync
        };
        self.set_state(state, options);
    }

    fn trigger_loading(&mut self, options: OpOptions) {
        self.op.loading_timer = None;
        if self.op.state == OpState::WaitingForAsync {
            self.set_state(OpState::WaitingForAsyncShowingLoading, options);
        }
    }

    /// Runs the timers due at `now` (host clock, ms) and places pending ones on the clock.
    pub fn tick(&mut self, doc: &Document, now: u64) {
        for t in [
            &mut self.op.async_timer,
            &mut self.op.sync_timer,
            &mut self.op.loading_timer,
        ]
        .into_iter()
        .flatten()
        {
            t.stamp(now);
        }
        // In deadline order, as the three schedulers would fire.
        loop {
            let Some(options) = self.op.options.clone() else {
                return;
            };
            let mut due: Vec<(u64, u8)> = Vec::new();
            for (i, t) in [
                self.op.async_timer,
                self.op.sync_timer,
                self.op.loading_timer,
            ]
            .iter()
            .enumerate()
            {
                if let Some(t) = t
                    && t.due(now)
                {
                    due.push((t.at().unwrap_or(0), i as u8));
                }
            }
            let Some(&(at, which)) = due.iter().min() else {
                return;
            };
            match which {
                // The second wait runs from when the first ended, not from this tick.
                0 => self.trigger_async(doc, options, Some(at)),
                1 => self.trigger_sync(doc, options),
                _ => self.trigger_loading(options),
            }
        }
    }

    /// The host's answer to a [`HoverRequest`]. Hovers with no content are dropped.
    pub fn set_hover(
        &mut self,
        doc: &Document,
        id: HoverRequestId,
        at: Version,
        hovers: Vec<Hover>,
    ) -> Result<(), AnswerRefusal> {
        if self.op.request != Some(id) || self.op.async_done {
            return Err(AnswerRefusal::Superseded);
        }
        let Some(options) = self.op.options.clone() else {
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
        let len = doc.text().len_bytes();
        for (ordinal, h) in hovers.into_iter().enumerate() {
            if h.contents.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            let range = match h.range {
                Some(mut r) => {
                    for c in &since {
                        r = map_range(&r, c);
                    }
                    r.start.min(len)..r.end.min(len)
                }
                None => options.anchor.range.clone(),
            };
            self.op.result.push(HoverPart::Markdown {
                range,
                source: h.source,
                contents: h.contents,
                ordinal,
            });
        }
        self.op.async_done = true;
        if matches!(
            self.op.state,
            OpState::WaitingForAsync | OpState::WaitingForAsyncShowingLoading
        ) {
            self.op.loading_timer = None;
            self.set_state(OpState::Idle, options);
        } else {
            // Results before the hover time are not sent out yet (they show with the sync part).
            self.fire_result(options);
        }
        Ok(())
    }

    fn fire_result(&mut self, options: OpOptions) {
        if matches!(self.op.state, OpState::FirstWait | OpState::SecondWait) {
            return;
        }
        let complete = self.op.state == OpState::Idle;
        let mut parts = self.op.result.clone();
        if self.op.state == OpState::WaitingForAsyncShowingLoading {
            parts.push(HoverPart::Loading {
                range: options.anchor.range.clone(),
            });
        }
        parts.sort_by_key(HoverPart::order);
        let r = HoverResult {
            parts,
            complete,
            anchor: options.anchor,
            source: options.source,
            should_focus: options.should_focus,
            insist_on_keeping_visible: options.insist_on_keeping_visible,
        };
        self.with_result(r);
    }

    /// VS Code `_withResult`.
    fn with_result(&mut self, r: HoverResult) {
        let prev_complete = self.current.as_ref().is_some_and(|c| c.complete);
        if !prev_complete {
            self.set_current(Some(r.clone()));
        }
        if !r.complete {
            return;
        }
        if r.parts.is_empty() && r.insist_on_keeping_visible {
            return;
        }
        self.set_current(Some(r));
    }

    fn set_current(&mut self, r: Option<HoverResult>) {
        let r = r.filter(|r| !r.parts.is_empty());
        if self.current == r {
            return;
        }
        let focus = r.as_ref().is_some_and(|r| r.should_focus);
        if r.is_none() {
            self.focused = false;
        } else if focus {
            self.focused = true;
        }
        self.current = r;
    }

    pub(crate) fn map(&mut self, c: &ChangeSet) {
        let m = |r: &mut Range<usize>| *r = map_range(r, c);
        if let Some(cur) = &mut self.current {
            m(&mut cur.anchor.range);
            for p in &mut cur.parts {
                m(p.range_mut());
            }
        }
        if let Some(o) = &mut self.op.options {
            m(&mut o.anchor.range);
        }
        for p in &mut self.op.result {
            m(p.range_mut());
        }
    }
}

/// VS Code's marker participant over `ContentHoverComputer._getLineDecorations`: the
/// diagnostics whose range, clipped to the anchor's line, covers the anchor (with one character
/// of slack, as marker decorations are `showIfCollapsed`).
fn diagnostic_parts(doc: &Document, anchor: &HoverAnchor) -> Vec<HoverPart> {
    let text = doc.text();
    let len = text.len_bytes();
    let a = anchor.range.start.min(len)..anchor.range.end.min(len);
    let line = text.byte_to_line(a.start);
    let lr = text.line_range(line);
    let (ls, le) = (lr.start, lr.end);
    let line_str = text.line(line).to_string();
    let next = |p: usize| {
        let c = p.saturating_sub(ls).min(line_str.len());
        line_str[c..]
            .chars()
            .next()
            .map_or(p, |ch| p + ch.len_utf8())
    };
    let prev = |p: usize| {
        let c = p.saturating_sub(ls).min(line_str.len());
        line_str[..c]
            .chars()
            .next_back()
            .map_or(p, |ch| p - ch.len_utf8())
    };
    let mut out = Vec::new();
    for (src, d) in doc.diagnostics().all() {
        if d.range.end < ls || d.range.start > le {
            continue;
        }
        let s = d.range.start.max(ls);
        let e = d.range.end.min(le);
        if s > next(a.start) || prev(a.end) > e {
            continue;
        }
        out.push(HoverPart::Diagnostic {
            range: s..e,
            source: src.clone(),
            diagnostic: d.clone(),
        });
    }
    out
}
