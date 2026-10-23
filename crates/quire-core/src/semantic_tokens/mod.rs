//! Semantic tokens (VS Code's document semantic tokens): token styles the host's providers
//! compute, laid over the highlighter's tokens. quire has no language client.
//!
//! **The host seam.**
//! - Providers are declared on the document with their legends
//!   ([`Document::set_semantic_tokens_sources`](crate::Document::set_semantic_tokens_sources)).
//! - The document asks for the tokens of the whole document with a [`SemanticTokensRequest`]
//!   (drained with [`Document::take_semantic_tokens_outputs`](crate::Document::take_semantic_tokens_outputs);
//!   egui_quire turns it into `EditorEvent::SemanticTokensRequested`). It names the request
//!   id, the version and the previous result of the provider that answered last, so the
//!   provider can answer with edits to it. Requests are debounced by how long the providers
//!   took (300 to 2000 ms) and repeated after edits; one runs at a time. Time comes in through
//!   [`Document::tick_semantic_tokens`](crate::Document::tick_semantic_tokens) (the widget
//!   calls it every frame).
//! - The host answers with
//!   [`Document::set_semantic_tokens`](crate::Document::set_semantic_tokens)`(id, replies)`:
//!   one [`SemanticTokensReply`] per provider. The first with tokens is used (an error
//!   anywhere fails the request, as `getDocumentSemanticTokens` does). Tokens are LSP's
//!   relative encoding (`deltaLine, deltaStart, length, tokenType, tokenModifiers` per token),
//!   with byte columns; edits apply to the previous result. Edits made while the request ran
//!   move the answer's tokens, and the tokens are asked again.
//! - Token types and modifiers resolve through the legend to quire's [`TokenStyle`] (LSP's
//!   names; a type quire does not know is not shown). A semantic token replaces the type and
//!   modifiers of the highlighter's tokens under it and keeps their language.
//! - Between answers the tokens follow edits (VS Code `SparseMultilineTokens.acceptEdit`).
//!
//! Translated from `editor/contrib/semanticTokens/browser/documentSemanticTokens.ts`
//! (`ModelSemanticColoring`), `editor/contrib/semanticTokens/common/getSemanticTokens.ts` and
//! `editor/common/services/semanticTokensProviderStyling.ts` (`toMultilineTokens2`) at
//! microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`. Departures: no theme lookup
//! (types map to quire's token types; the theme colours those); range providers (viewport
//! tokens) are not asked.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

pub mod store;

use crate::diagnostics::SourceId;
use crate::hover::{AnswerRefusal, Timer};
use crate::syntax::{TokenModifiers, TokenStyle, TokenType};
use crate::version::Version;
use std::collections::VecDeque;
pub use store::{LineRange, SparseMultilineTokens, SparseTokensStore, TokenEdit};

/// The types and modifiers a provider's token data indexes (LSP `SemanticTokensLegend`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokensLegend {
    /// Token type names (LSP's: `"variable"`, `"enumMember"`, ...).
    pub token_types: Vec<String>,
    /// Modifier names (LSP's: `"declaration"`, `"readonly"`, ...), one bit each.
    pub token_modifiers: Vec<String>,
}

/// A semantic tokens provider of the host.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokensSource {
    /// The provider.
    pub id: SourceId,
    /// Its legend.
    pub legend: SemanticTokensLegend,
}

/// A full result (LSP `SemanticTokens`): 5 numbers per token, relative to the previous token
/// (`deltaLine, deltaStart, length, tokenType, tokenModifiers`; columns in bytes).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokens {
    /// Names this result for a later delta request.
    pub result_id: Option<String>,
    /// The token data.
    pub data: Vec<u32>,
}

/// One edit of the previous result's data (LSP `SemanticTokensEdit`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokensEdit {
    /// Index in the data.
    pub start: usize,
    /// Numbers removed.
    pub delete_count: usize,
    /// Numbers inserted.
    pub data: Option<Vec<u32>>,
}

/// A delta result (LSP `SemanticTokensDelta`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokensEdits {
    /// Names this result for a later delta request.
    pub result_id: Option<String>,
    /// Edits of the previous result, sorted by start.
    pub edits: Vec<SemanticTokensEdit>,
}

/// One provider's answer.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SemanticTokensReply {
    /// A full result.
    Tokens(SemanticTokens),
    /// Edits to the previous result.
    Edits(SemanticTokensEdits),
    /// No result.
    None,
    /// The provider failed or was busy (the tokens are asked again later if the text moved).
    Error,
}

/// Identifies a semantic tokens request; answers carry it back.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SemanticTokensRequestId(pub u64);

/// A request for the document's semantic tokens, for every provider (VS Code
/// `provideDocumentSemanticTokens`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SemanticTokensRequest {
    /// Answer with this id.
    pub id: SemanticTokensRequestId,
    /// The document version.
    pub version: Version,
    /// The provider that answered last and its result id: it may answer with edits to that
    /// result (`lastResultId`, given to that provider only).
    pub previous: Option<(SourceId, String)>,
}

/// What the semantic tokens want from the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SemanticTokensOutput {
    /// Ask the providers.
    Request(SemanticTokensRequest),
    /// The document no longer needs result `result_id` of `source` (VS Code
    /// `releaseDocumentSemanticTokens`): a provider keeping results for delta requests may
    /// drop it.
    Release {
        /// The provider.
        source: SourceId,
        /// Its result.
        result_id: Option<String>,
    },
}

/// VS Code `SemanticTokensProviderStyling.getMetadata` for quire: the style of a legend type
/// index and modifier set (`None`: not shown, VS Code's `NO_STYLING`).
pub fn legend_style(
    legend: &SemanticTokensLegend,
    type_index: u32,
    modifier_set: u32,
) -> Option<TokenStyle> {
    let name = legend.token_types.get(type_index as usize)?;
    let ty = TokenType::from_name(name)?;
    if ty == TokenType::Text {
        return None;
    }
    let mut mods = TokenModifiers::NONE;
    let mut set = modifier_set;
    let mut i = 0;
    while set > 0 && i < legend.token_modifiers.len() {
        if set & 1 == 1 {
            mods = mods | modifier_bit(&legend.token_modifiers[i]);
        }
        set >>= 1;
        i += 1;
    }
    Some(TokenStyle::new(ty, mods, 0))
}

fn modifier_bit(name: &str) -> TokenModifiers {
    match name {
        "declaration" => TokenModifiers::DECLARATION,
        "definition" => TokenModifiers::DEFINITION,
        "readonly" => TokenModifiers::READONLY,
        "static" => TokenModifiers::STATIC,
        "deprecated" => TokenModifiers::DEPRECATED,
        "abstract" => TokenModifiers::ABSTRACT,
        "async" => TokenModifiers::ASYNC,
        "modification" => TokenModifiers::MODIFICATION,
        "documentation" => TokenModifiers::DOCUMENTATION,
        "defaultLibrary" => TokenModifiers::DEFAULT_LIBRARY,
        _ => TokenModifiers::NONE,
    }
}

/// The bits of a semantic token's style that replace the highlighter's: type and modifiers
/// (the language stays).
pub fn semantic_mask(_metadata: u32) -> u32 {
    0x00ff_ffff
}

/// `SemanticColoringConstants`.
const DESIRED_TOKENS_PER_AREA: usize = 400;
const DESIRED_MAX_AREAS: usize = 1024;

/// VS Code `toMultilineTokens2`: LSP-encoded token data cut into areas of sparse tokens
/// (about 400 tokens each, a line's tokens kept together). Tokens with a non-positive length,
/// tokens overlapping the previous one and tokens `styling` gives no metadata are dropped.
pub fn to_multiline_tokens(
    data: &[u32],
    mut styling: impl FnMut(u32, u32) -> Option<u32>,
) -> Vec<SparseMultilineTokens> {
    let src = data;
    let count = src.len() / 5;
    let per_area = count
        .div_ceil(DESIRED_MAX_AREAS)
        .max(DESIRED_TOKENS_PER_AREA);
    let mut result = Vec::new();
    let mut i = 0usize;
    let mut last_line: i32 = 1;
    let mut last_start: i32 = 0;
    while i < count {
        let start_index = i;
        let mut end_index = (start_index + per_area).min(count);
        // keep a line's tokens in one area
        if end_index < count {
            let mut small = end_index;
            while small - 1 > start_index && src[5 * small] == 0 {
                small -= 1;
            }
            if small - 1 == start_index {
                // so many tokens on this line that the area would be empty: go right
                let mut big = end_index;
                while big + 1 < count && src[5 * big] == 0 {
                    big += 1;
                }
                end_index = big;
            } else {
                end_index = small;
            }
        }
        let mut dest: Vec<u32> = Vec::with_capacity((end_index - start_index) * 4);
        let mut area_line: i32 = 0;
        let mut prev_line: i32 = 0;
        let mut prev_end: i32 = 0;
        while i < end_index {
            let o = 5 * i;
            let delta_line = src[o];
            let delta_char = src[o + 1];
            let line = last_line.wrapping_add(delta_line as i32);
            let start = if delta_line == 0 {
                last_start.wrapping_add(delta_char as i32)
            } else {
                delta_char as i32
            };
            let len = src[o + 2];
            let end = start.wrapping_add(len as i32);
            if end <= start {
                // invalid (most likely a negative length cast to u32)
            } else if prev_line == line && prev_end > start {
                // overlaps the previous token
            } else if let Some(md) = styling(src[o + 3], src[o + 4]) {
                if area_line == 0 {
                    area_line = line;
                }
                dest.extend([
                    line.wrapping_sub(area_line) as u32,
                    start as u32,
                    end as u32,
                    md,
                ]);
                prev_line = line;
                prev_end = end;
            }
            last_line = line;
            last_start = start;
            i += 1;
        }
        result.push(SparseMultilineTokens::create(area_line as i64, dest));
    }
    result
}

/// `ModelSemanticColoring`'s debounce: min 300 ms, max 2000 ms, default 450 ms.
const DEBOUNCE_MIN: u64 = 300;
const DEBOUNCE_MAX: u64 = 2000;
const DEBOUNCE_DEFAULT: u64 = 450;

/// The provider's last full result (`SemanticTokensResponse`).
#[derive(Clone, Debug)]
struct Response {
    source: SourceId,
    result_id: Option<String>,
    data: Vec<u32>,
}

/// The request in flight.
#[derive(Clone, Debug)]
struct InFlight {
    id: SemanticTokensRequestId,
    sent_at: Option<u64>,
    /// The edits of each change made since (`pendingChanges`).
    pending: Vec<Vec<TokenEdit>>,
}

/// The semantic tokens of one document (VS Code `ModelSemanticColoring` and the model's
/// `SparseTokensStore`).
#[derive(Clone, Debug)]
pub struct SemanticTokensState {
    store: SparseTokensStore,
    sources: Vec<SemanticTokensSource>,
    scheduler: Option<Timer>,
    durations: VecDeque<u64>,
    delay: u64,
    response: Option<Response>,
    in_flight: Option<InFlight>,
    providers_changed: bool,
    next_id: u64,
    outputs: Vec<SemanticTokensOutput>,
    now: Option<u64>,
    generation: u64,
}

impl Default for SemanticTokensState {
    fn default() -> Self {
        SemanticTokensState {
            store: SparseTokensStore::new(),
            sources: Vec::new(),
            scheduler: None,
            durations: VecDeque::new(),
            delay: DEBOUNCE_DEFAULT,
            response: None,
            in_flight: None,
            providers_changed: false,
            next_id: 0,
            outputs: Vec::new(),
            now: None,
            generation: 0,
        }
    }
}

impl SemanticTokensState {
    /// The tokens held.
    pub fn store(&self) -> &SparseTokensStore {
        &self.store
    }

    /// The providers.
    pub fn sources(&self) -> &[SemanticTokensSource] {
        &self.sources
    }

    /// Bumps whenever the tokens change (render caches).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The request waiting for an answer.
    pub fn pending_request(&self) -> Option<SemanticTokensRequestId> {
        self.in_flight.as_ref().map(|f| f.id)
    }

    /// Whether a request is scheduled.
    pub fn is_scheduled(&self) -> bool {
        self.scheduler.is_some()
    }

    /// The current debounce (ms).
    pub fn delay(&self) -> u64 {
        self.delay
    }

    /// Whether a request is scheduled or in flight.
    pub fn is_pending(&self) -> bool {
        self.scheduler.is_some() || self.in_flight.is_some()
    }

    /// Takes what the semantic tokens want from the host.
    pub fn take_outputs(&mut self) -> Vec<SemanticTokensOutput> {
        std::mem::take(&mut self.outputs)
    }

    /// The earliest deadline (host clock, ms) at which [`SemanticTokensState::tick`] has work.
    pub fn next_deadline(&self) -> Option<u64> {
        self.scheduler.and_then(|t| t.at())
    }

    /// Whether a timer waits to be placed on the clock.
    pub fn needs_tick(&self) -> bool {
        self.scheduler.is_some_and(|t| t.at().is_none())
    }

    fn schedule(&mut self, delay: Option<u64>) {
        self.scheduler = Some(Timer::new(self.now, delay.unwrap_or(self.delay)));
    }

    /// Replaces the stored tokens (VS Code `tokenization.setSemanticTokens(pieces,
    /// isComplete)`).
    pub fn set_tokens(&mut self, pieces: Option<Vec<SparseMultilineTokens>>, complete: bool) {
        self.store.set(pieces, complete);
        self.generation += 1;
    }

    /// Declares the providers (`handleRegistryChange`; the first declaration fetches at once).
    pub(crate) fn set_sources(&mut self, sources: Vec<SemanticTokensSource>) {
        let first = self.sources.is_empty();
        self.sources = sources;
        self.schedule(if first { Some(0) } else { None });
    }

    /// The host's provider `source` changed its tokens (`handleProviderDidChange`).
    pub(crate) fn refresh(&mut self, source: &SourceId) {
        if !self.sources.iter().any(|s| &s.id == source) {
            return;
        }
        if self.in_flight.is_some() {
            self.providers_changed = true;
            return;
        }
        self.schedule(Some(0));
    }

    /// The tokens follow a change (its edits in document order, old coordinates); a change
    /// while a request runs is recorded for its answer; a request is scheduled.
    pub(crate) fn accept_change(&mut self, edits: Vec<TokenEdit>) {
        for e in edits.iter().rev() {
            self.store.accept_edit(e);
        }
        self.generation += 1;
        if let Some(f) = &mut self.in_flight {
            f.pending.push(edits);
        }
        if !self.sources.is_empty() && self.scheduler.is_none() {
            self.schedule(None);
        }
    }

    /// Runs the request due at `now` (host clock, ms) and places a pending timer on the clock.
    pub fn tick(&mut self, version: Version, now: u64) {
        self.now = Some(now);
        if let Some(t) = &mut self.scheduler {
            t.stamp(now);
        }
        if self.scheduler.is_some_and(|t| t.due(now)) {
            self.scheduler = None;
            self.fetch_now(version);
        }
    }

    /// `_fetchDocumentSemanticTokensNow`.
    fn fetch_now(&mut self, version: Version) {
        if self.in_flight.is_some() {
            return;
        }
        if self.sources.is_empty() {
            if self.response.is_some() {
                self.set_tokens(None, false);
            }
            return;
        }
        self.next_id += 1;
        let id = SemanticTokensRequestId(self.next_id);
        let previous = self
            .response
            .as_ref()
            .and_then(|r| Some((r.source.clone(), r.result_id.clone()?)));
        self.in_flight = Some(InFlight {
            id,
            sent_at: self.now,
            pending: Vec::new(),
        });
        self.providers_changed = false;
        self.outputs
            .push(SemanticTokensOutput::Request(SemanticTokensRequest {
                id,
                version,
                previous,
            }));
    }

    fn reschedule_if_needed(&mut self, pending: bool) {
        if (pending || self.providers_changed) && self.scheduler.is_none() {
            self.schedule(None);
        }
    }

    /// The host's answer to a [`SemanticTokensRequest`]: one reply per provider.
    pub fn set_semantic_tokens(
        &mut self,
        id: SemanticTokensRequestId,
        replies: Vec<(SourceId, SemanticTokensReply)>,
    ) -> Result<(), AnswerRefusal> {
        let Some(flight) = self.in_flight.take_if(|f| f.id == id) else {
            return Err(AnswerRefusal::Superseded);
        };
        if let (Some(sent), Some(now)) = (flight.sent_at, self.now) {
            if self.durations.len() == 6 {
                self.durations.pop_front();
            }
            self.durations.push_back(now.saturating_sub(sent));
            let avg = self.durations.iter().sum::<u64>() / self.durations.len() as u64;
            self.delay = avg.clamp(DEBOUNCE_MIN, DEBOUNCE_MAX);
        }
        let pending = !flight.pending.is_empty();
        // getDocumentSemanticTokens: an error fails the request; else the first result with
        // tokens; else the first result.
        let replies: Vec<(SourceId, SemanticTokensReply)> = replies
            .into_iter()
            .filter(|(s, _)| self.sources.iter().any(|x| &x.id == s))
            .collect();
        if replies
            .iter()
            .any(|(_, r)| matches!(r, SemanticTokensReply::Error))
        {
            self.reschedule_if_needed(pending);
            return Ok(());
        }
        let picked = replies
            .iter()
            .position(|(_, r)| {
                matches!(
                    r,
                    SemanticTokensReply::Tokens(_) | SemanticTokensReply::Edits(_)
                )
            })
            .or((!replies.is_empty()).then_some(0));
        // `_setDocumentSemanticTokens` disposes the current response first.
        let current = self.response.take();
        if let Some(c) = &current {
            self.release(&c.source, c.result_id.clone());
        }
        let Some(i) = picked else {
            // no provider
            self.set_tokens(None, false);
            return Ok(());
        };
        let (source, reply) = replies.into_iter().nth(i).expect("picked");
        let Some(legend) = self
            .sources
            .iter()
            .find(|s| s.id == source)
            .map(|s| s.legend.clone())
        else {
            self.set_tokens(None, false);
            return Ok(());
        };
        let tokens = match reply {
            SemanticTokensReply::Tokens(t) => t,
            SemanticTokensReply::Edits(e) => {
                let Some(cur) = current else {
                    // edits without a previous result: rejected, not retried
                    self.release(&source, e.result_id.clone());
                    self.set_tokens(None, true);
                    return Ok(());
                };
                match apply_edits(&cur.data, &e.edits) {
                    Some(data) => SemanticTokens {
                        result_id: e.result_id,
                        data,
                    },
                    None => {
                        // invalid edits: rejected, a full refresh at once
                        self.release(&source, e.result_id.clone());
                        self.set_tokens(None, true);
                        self.schedule(Some(0));
                        return Ok(());
                    }
                }
            }
            SemanticTokensReply::None | SemanticTokensReply::Error => {
                self.set_tokens(None, true);
                self.reschedule_if_needed(pending);
                return Ok(());
            }
        };
        let mut pieces = to_multiline_tokens(&tokens.data, |t, m| {
            legend_style(&legend, t, m).map(|s| s.0)
        });
        for change in &flight.pending {
            for area in &mut pieces {
                for e in change.iter().rev() {
                    area.accept_edit(e);
                }
            }
        }
        self.response = Some(Response {
            source,
            result_id: tokens.result_id,
            data: tokens.data,
        });
        self.set_tokens(Some(pieces), true);
        self.reschedule_if_needed(pending);
        Ok(())
    }

    /// The provider whose result is held, and that result's id.
    pub fn result(&self) -> Option<(&SourceId, Option<&str>)> {
        self.response
            .as_ref()
            .map(|r| (&r.source, r.result_id.as_deref()))
    }

    fn release(&mut self, source: &SourceId, result_id: Option<String>) {
        self.outputs.push(SemanticTokensOutput::Release {
            source: source.clone(),
            result_id,
        });
    }

    /// The tokens of `line` (0-based) merged over its base tokens `base` (`(end, style)`).
    pub fn merge_line(&self, line: usize, base: &[(u32, u32)]) -> Vec<(u32, u32)> {
        self.store
            .add_sparse_tokens(line as i64 + 1, base, semantic_mask)
    }
}

/// Applies semantic token edits to the previous data (`_setDocumentSemanticTokens`); `None`
/// when they are invalid (they delete more than there is, or start past the end).
pub fn apply_edits(src: &[u32], edits: &[SemanticTokensEdit]) -> Option<Vec<u32>> {
    if edits.is_empty() {
        return Some(src.to_vec());
    }
    let delta: isize = edits
        .iter()
        .map(|e| e.data.as_ref().map_or(0, Vec::len) as isize - e.delete_count as isize)
        .sum();
    let dest_len = src.len() as isize + delta;
    if dest_len < 0 {
        return None;
    }
    let mut dest = vec![0u32; dest_len as usize];
    let copy = |src: &[u32], so: usize, dest: &mut [u32], d_o: usize, len: usize| {
        let len = len
            .min(dest.len() - d_o.min(dest.len()))
            .min(src.len() - so.min(src.len()));
        dest[d_o..d_o + len].copy_from_slice(&src[so..so + len]);
    };
    let mut src_last = src.len();
    let mut dest_last = dest.len();
    for e in edits.iter().rev() {
        if e.start > src.len() {
            return None;
        }
        let copy_count = src_last as isize - (e.start + e.delete_count) as isize;
        if copy_count > 0 {
            let c = copy_count as usize;
            if dest_last < c {
                return None;
            }
            copy(src, src_last - c, &mut dest, dest_last - c, c);
            dest_last -= c;
        }
        if let Some(d) = &e.data {
            if dest_last < d.len() {
                return None;
            }
            copy(d, 0, &mut dest, dest_last - d.len(), d.len());
            dest_last -= d.len();
        }
        src_last = e.start;
    }
    if src_last > 0 {
        copy(src, 0, &mut dest, 0, src_last);
    }
    Some(dest)
}
