//! [`CompletionModel`]: the sorted, filtered view of a session's items (VS Code
//! `editor/contrib/suggest/browser/completionModel.ts` and the provider comparators of
//! `suggest.ts`, microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated
//! literally.
//!
//! Columns are byte offsets within the request's line (VS Code uses UTF-16 columns; only
//! differences between columns are used, and they agree for every edit at the cursor).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use super::filters::{
    FuzzyScore, FuzzyScoreOptions, FuzzyScorer, any_score, fuzzy_score,
    fuzzy_score_graceful_aggressive, utf16, utf16_lower,
};
use super::{CompletionItem, CompletionItemKind, SnippetSuggestions};
use crate::diagnostics::SourceId;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// One item of a session with what filtering needs (VS Code's `CompletionItem` wrapper).
#[derive(Clone, Debug)]
pub struct SuggestItem {
    /// The item as the provider supplied it (replaced when resolved).
    pub completion: Arc<CompletionItem>,
    /// Its provider.
    pub source: SourceId,
    /// Its index in the provider's list as supplied.
    pub index: usize,
    /// The provider's list was incomplete.
    pub incomplete: bool,
    /// The column of the request position.
    pub position: usize,
    /// The column the item's range starts at.
    pub edit_start: usize,
    /// The column its insert range ends at.
    pub edit_insert_end: usize,
    /// The column its replace range ends at.
    pub edit_replace_end: usize,
    /// No label, or ranges off the request line, or insert and replace starting apart: never
    /// shown.
    pub invalid: bool,
    label16: Vec<u16>,
    label_low16: Vec<u16>,
    filter16: Option<(Vec<u16>, Vec<u16>)>,
    sort_low: Option<String>,
}

impl SuggestItem {
    /// An item at request column `position`, with its range as columns on the request line
    /// (`on_line`: both ranges lie on that line).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        completion: Arc<CompletionItem>,
        source: SourceId,
        index: usize,
        incomplete: bool,
        position: usize,
        edit_start: usize,
        edit_insert_end: usize,
        edit_replace_end: usize,
        on_line: bool,
    ) -> Self {
        let label16 = utf16(&completion.label);
        let label_low16 = utf16_lower(&label16);
        let filter16 = completion.filter_text.as_deref().map(|f| {
            let u = utf16(f);
            let l = utf16_lower(&u);
            (u, l)
        });
        let sort_low = completion.sort_text.as_deref().map(str::to_lowercase);
        let invalid = completion.label.is_empty() || !on_line;
        SuggestItem {
            completion,
            source,
            index,
            incomplete,
            position,
            edit_start,
            edit_insert_end,
            edit_replace_end,
            invalid,
            label16,
            label_low16,
            filter16,
            sort_low,
        }
    }

    /// The label.
    pub fn label(&self) -> &str {
        &self.completion.label
    }

    /// The label in UTF-16 units (match positions index it).
    pub fn label16(&self) -> &[u16] {
        &self.label16
    }

    pub(crate) fn sort_low(&self) -> Option<&str> {
        self.sort_low.as_deref()
    }
}

/// The provider-side order (VS Code `getSuggestionComparator`): `sortText`, then label, then
/// kind; snippets first or last per `editor.snippetSuggestions`.
pub fn compare_items(a: &SuggestItem, b: &SuggestItem, snippets: SnippetSuggestions) -> Ordering {
    let ka = a.completion.kind;
    let kb = b.completion.kind;
    if ka != kb {
        let snippet = CompletionItemKind::Snippet;
        match snippets {
            SnippetSuggestions::Top if ka == snippet => return Ordering::Less,
            SnippetSuggestions::Top if kb == snippet => return Ordering::Greater,
            SnippetSuggestions::Bottom if ka == snippet => return Ordering::Greater,
            SnippetSuggestions::Bottom if kb == snippet => return Ordering::Less,
            _ => {}
        }
    }
    if let (Some(x), Some(y)) = (a.sort_low(), b.sort_low()) {
        match x.cmp(y) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    match a.completion.label.cmp(&b.completion.label) {
        Ordering::Equal => {}
        o => return o,
    }
    ka.cmp(&kb)
}

/// The line before the cursor and how far the cursor moved since the request (VS Code
/// `LineContext` of the completion model).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LineContext {
    /// The current line up to the cursor.
    pub leading_line_content: String,
    /// Cursor column now minus the request column.
    pub character_count_delta: isize,
}

/// Model options.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ModelOptions {
    /// `editor.suggest.filterGraceful`.
    pub filter_graceful: bool,
    /// `editor.snippetSuggestions` (the model's sort).
    pub snippet_suggestions: SnippetSuggestions,
    /// Scorer options.
    pub fuzzy: FuzzyScoreOptions,
}

impl Default for ModelOptions {
    /// VS Code's defaults (`EditorOptions.suggest.defaultValue`, `snippetSuggestions: inline`).
    fn default() -> Self {
        ModelOptions {
            filter_graceful: true,
            snippet_suggestions: SnippetSuggestions::Inline,
            fuzzy: FuzzyScoreOptions::default(),
        }
    }
}

/// A shown item: the item, its score and its index in the list it was filtered from.
#[derive(Clone, Debug)]
pub struct FilteredItem {
    /// The item.
    pub item: Arc<SuggestItem>,
    /// Its score against the word before the cursor (matches index the label, or the
    /// filter text's rank with label highlights).
    pub score: FuzzyScore,
    /// Its index in the list it was filtered from (the tie-breaker).
    pub idx: usize,
    /// How many bytes before the cursor it was scored against.
    pub word_len: usize,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Refilter {
    Nothing,
    All,
    Incr,
}

/// Sorted, filtered completion view model.
#[derive(Clone, Debug)]
pub struct CompletionModel {
    items: Vec<Arc<SuggestItem>>,
    column: usize,
    options: ModelOptions,
    line_context: LineContext,
    refilter: Refilter,
    filtered: Vec<FilteredItem>,
    generation: u64,
}

impl CompletionModel {
    /// A model of `items` (already in provider order) requested at `column`.
    pub fn new(
        items: Vec<Arc<SuggestItem>>,
        column: usize,
        line_context: LineContext,
        options: ModelOptions,
    ) -> Self {
        CompletionModel {
            items,
            column,
            options,
            line_context,
            refilter: Refilter::All,
            filtered: Vec::new(),
            generation: 0,
        }
    }

    /// The line context.
    pub fn line_context(&self) -> &LineContext {
        &self.line_context
    }

    /// Changes the line context: typing more refilters the shown items only, anything else
    /// refilters all.
    pub fn set_line_context(&mut self, value: LineContext) {
        if self.line_context != value {
            self.refilter = if self.line_context.character_count_delta < value.character_count_delta
                && self.generation > 0
            {
                Refilter::Incr
            } else {
                Refilter::All
            };
            self.line_context = value;
        }
    }

    /// The shown items, best first.
    pub fn items(&mut self) -> &[FilteredItem] {
        self.ensure();
        &self.filtered
    }

    /// The shown items as last computed (call [`CompletionModel::items`] first).
    pub fn cached_items(&self) -> &[FilteredItem] {
        &self.filtered
    }

    /// Bumps each time the shown items are recomputed.
    pub fn generation(&mut self) -> u64 {
        self.ensure();
        self.generation
    }

    /// All valid items by provider (VS Code `getItemsByProvider`), in source order.
    pub fn items_by_source(&mut self) -> BTreeMap<SourceId, Vec<Arc<SuggestItem>>> {
        self.ensure();
        let mut m: BTreeMap<SourceId, Vec<Arc<SuggestItem>>> = BTreeMap::new();
        for it in &self.items {
            if !it.invalid {
                m.entry(it.source.clone()).or_default().push(it.clone());
            }
        }
        m
    }

    /// Providers whose list was incomplete (VS Code `getIncompleteProvider`).
    pub fn incomplete_sources(&mut self) -> BTreeSet<SourceId> {
        self.items_by_source()
            .into_iter()
            .filter(|(_, v)| v.first().is_some_and(|i| i.incomplete))
            .map(|(k, _)| k)
            .collect()
    }

    /// Every item (valid or not), in provider order.
    pub fn all_items(&self) -> &[Arc<SuggestItem>] {
        &self.items
    }

    /// Replaces one item (a resolved one) everywhere it appears.
    pub(crate) fn replace_item(&mut self, old: &Arc<SuggestItem>, new: Arc<SuggestItem>) {
        for it in &mut self.items {
            if Arc::ptr_eq(it, old) {
                *it = new.clone();
            }
        }
        for f in &mut self.filtered {
            if Arc::ptr_eq(&f.item, old) {
                f.item = new.clone();
            }
        }
    }

    fn ensure(&mut self) {
        if self.refilter != Refilter::Nothing {
            self.create_cached_state();
        }
    }

    fn create_cached_state(&mut self) {
        let LineContext {
            leading_line_content,
            character_count_delta,
        } = &self.line_context;
        let mut word = String::new();
        let mut word16: Vec<u16> = Vec::new();
        let mut word_low: Vec<u16> = Vec::new();
        let mut word_len_cached: Option<usize> = None;

        // Filter fewer items when typing more.
        let source: Vec<(Arc<SuggestItem>, Option<FuzzyScore>)> = if self.refilter == Refilter::All
        {
            self.items.iter().map(|i| (i.clone(), None)).collect()
        } else {
            std::mem::take(&mut self.filtered)
                .into_iter()
                .map(|f| (f.item, None))
                .collect()
        };
        let mut target: Vec<FilteredItem> = Vec::new();

        // The score function, by the number of items and the configuration.
        let score_fn: FuzzyScorer = if !self.options.filter_graceful || source.len() > 2000 {
            fuzzy_score
        } else {
            fuzzy_score_graceful_aggressive
        };

        for (i, (item, _)) in source.into_iter().enumerate() {
            if item.invalid {
                continue;
            }
            // `word` is the remainder of the current line scored against. Each item could use a
            // different word, in practice they don't: cached.
            let overwrite_before = item.position as isize - item.edit_start as isize;
            let word_len = (overwrite_before + character_count_delta
                - (item.position as isize - self.column as isize))
                .max(0) as usize;
            if word_len_cached != Some(word_len) {
                word = if word_len == 0 {
                    String::new()
                } else {
                    let l = leading_line_content.len();
                    let mut from = l.saturating_sub(word_len);
                    while !leading_line_content.is_char_boundary(from) {
                        from -= 1;
                    }
                    leading_line_content[from..].to_string()
                };
                word16 = utf16(&word);
                word_low = utf16_lower(&word16);
                word_len_cached = Some(word_len);
            }

            let score = if word_len == 0 {
                // Nothing to score against: a constant rank, the initial order decides.
                FuzzyScore::default_score()
            } else {
                // Skip whitespace in the word until the replace range (overwriteBefore).
                let wb = word.as_bytes();
                let mut word_pos = 0usize;
                while (word_pos as isize) < overwrite_before {
                    match wb.get(word_pos) {
                        Some(b' ') | Some(b'\t') => word_pos += 1,
                        _ => break,
                    }
                }
                if word_pos >= word.len() {
                    // The scoring position is the whole word: as with no word.
                    FuzzyScore::default_score()
                } else if let Some((filter, filter_low)) = &item.filter16 {
                    // A filter text must match the word; the label then gives highlights.
                    let Some(m) = score_fn(
                        &word16,
                        &word_low,
                        word_pos,
                        filter,
                        filter_low,
                        0,
                        self.options.fuzzy,
                    ) else {
                        continue; // no match
                    };
                    if compare_ignore_case(
                        item.completion.filter_text.as_deref().unwrap_or(""),
                        &item.completion.label,
                    ) == Ordering::Equal
                    {
                        m
                    } else {
                        let mut s = any_score(
                            &word16,
                            &word_low,
                            word_pos,
                            &item.label16,
                            &item.label_low16,
                            0,
                        );
                        s.score = m.score; // the filter text's rank
                        s
                    }
                } else {
                    let Some(m) = score_fn(
                        &word16,
                        &word_low,
                        word_pos,
                        &item.label16,
                        &item.label_low16,
                        0,
                        self.options.fuzzy,
                    ) else {
                        continue; // no match
                    };
                    m
                }
            };
            target.push(FilteredItem {
                item,
                score,
                idx: i,
                word_len,
            });
        }

        let snippets = self.options.snippet_suggestions;
        target.sort_by(|a, b| compare_filtered(a, b, snippets));
        self.filtered = target;
        self.refilter = Refilter::Nothing;
        self.generation += 1;
    }
}

fn compare_filtered(a: &FilteredItem, b: &FilteredItem, snippets: SnippetSuggestions) -> Ordering {
    let ka = a.item.completion.kind;
    let kb = b.item.completion.kind;
    if ka != kb {
        let snippet = CompletionItemKind::Snippet;
        match snippets {
            SnippetSuggestions::Top if ka == snippet => return Ordering::Less,
            SnippetSuggestions::Top if kb == snippet => return Ordering::Greater,
            SnippetSuggestions::Bottom if ka == snippet => return Ordering::Greater,
            SnippetSuggestions::Bottom if kb == snippet => return Ordering::Less,
            _ => {}
        }
    }
    // score desc, then (word distance: always 0, no locality bonus), then idx
    b.score.score.cmp(&a.score.score).then(a.idx.cmp(&b.idx))
}

/// VS Code `compareIgnoreCase` (ASCII case-insensitive; other text by lower case).
fn compare_ignore_case(a: &str, b: &str) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    for (i, (&x, &y)) in ab.iter().zip(bb).enumerate() {
        if x == y {
            continue;
        }
        if x >= 128 || y >= 128 {
            return a[floor(a, i)..]
                .to_lowercase()
                .cmp(&b[floor(b, i)..].to_lowercase());
        }
        match x.to_ascii_uppercase().cmp(&y.to_ascii_uppercase()) {
            Ordering::Equal => continue,
            o => return o,
        }
    }
    ab.len().cmp(&bb.len())
}

fn floor(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
