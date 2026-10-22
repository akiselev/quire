//! Completion (VS Code's suggest): the data model, scoring, the session state machine and
//! snippet insertion. The host supplies the provider; quire has no language client
//! (ARCH-QUIRE §3.1).
//!
//! **The host seam.**
//! - A view's session asks for items with a [`CompletionRequest`] (drained with
//!   [`EditorView::take_completion_requests`](crate::EditorView::take_completion_requests);
//!   egui_quire turns it into `EditorEvent::CompletionRequested`). It names the request id, the
//!   position and document version, the trigger ([`CompletionTriggerKind`], VS Code's
//!   `CompletionContext`) and, for a trigger character or an incomplete list, the sources to ask.
//! - The host answers, asynchronously, with
//!   [`EditorView::set_completions`](crate::EditorView::set_completions)`(doc, id, version,
//!   lists)`: one [`CompletionList`] per source, ranges in bytes at `version`. Ranges are mapped
//!   forward if the text moved on; an answer older than the recent-changes window is refused
//!   as stale and an answer to a superseded request as [`CompletionRefusal::Superseded`].
//! - Sources and their trigger characters are declared on the document
//!   ([`Document::set_completion_sources`](crate::Document::set_completion_sources)); with none,
//!   nothing triggers automatically.
//!
//! The behaviour, names and defaults follow VS Code's `editor/contrib/suggest` (completionModel,
//! suggestModel, suggestController) at microsoft/vscode `bdce63db5876`.

pub mod filters;
pub mod insert;
mod model;
pub(crate) mod session;
pub mod snippet;

pub use crate::commands::suggest::snippet_transaction;
pub use filters::{FuzzyScore, FuzzyScoreOptions};
pub use insert::{SnippetSession, TabstopGroup};
pub use model::{
    CompletionModel, FilteredItem, LineContext, ModelOptions, SuggestItem, compare_items,
};
pub use session::{SuggestSession, SuggestState, VISIBLE_ROWS, WidgetState, should_auto_trigger};

use crate::diagnostics::SourceId;
use std::ops::Range;
use std::sync::Arc;

/// The kind of a completion item (Monaco `languages.CompletionItemKind`, in its order).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub enum CompletionItemKind {
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Struct,
    Interface,
    Module,
    #[default]
    Property,
    Event,
    Operator,
    Unit,
    Value,
    Constant,
    Enum,
    EnumMember,
    Keyword,
    Text,
    Color,
    File,
    Reference,
    Customcolor,
    Folder,
    TypeParameter,
    User,
    Issue,
    Tool,
    Snippet,
}

/// How the insert text is inserted (Monaco `CompletionItemInsertTextRule`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InsertTextRules {
    /// `InsertAsSnippet`: the insert text is a snippet (`$1`, `${2:name}`, `$0`, ...).
    pub insert_as_snippet: bool,
    /// `KeepWhitespace`: do not adjust the indentation of multi-line insert text.
    pub keep_whitespace: bool,
}

/// What an item replaces: one range, or Monaco's insert-or-replace pair (both start at the same
/// position; `editor.suggest.insertMode` picks one).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CompletionRange {
    /// The range to replace (usually the word before the cursor).
    Replace(Range<usize>),
    /// Insert (up to the cursor) or replace (the whole word).
    InsertReplace {
        /// The insert range.
        insert: Range<usize>,
        /// The replace range.
        replace: Range<usize>,
    },
}

impl CompletionRange {
    /// The insert range.
    pub fn insert(&self) -> &Range<usize> {
        match self {
            CompletionRange::Replace(r) => r,
            CompletionRange::InsertReplace { insert, .. } => insert,
        }
    }

    /// The replace range.
    pub fn replace(&self) -> &Range<usize> {
        match self {
            CompletionRange::Replace(r) => r,
            CompletionRange::InsertReplace { replace, .. } => replace,
        }
    }

    pub(crate) fn map(&mut self, c: &crate::change::ChangeSet) {
        let m = |r: &mut Range<usize>| {
            let s = c.map_pos(r.start, crate::change::Bias::Before);
            let e = c.map_pos(r.end, crate::change::Bias::Before).max(s);
            *r = s..e;
        };
        match self {
            CompletionRange::Replace(r) => m(r),
            CompletionRange::InsertReplace { insert, replace } => {
                m(insert);
                m(replace);
            }
        }
    }
}

/// A host command an item runs after it is inserted (Monaco `CompletionItem.command`). The
/// editor runs `editor.action.triggerSuggest` itself (retrigger); any other id is handed to the
/// host's registry.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompletionCommand {
    /// The command id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Arguments as JSON text.
    pub args_json: String,
}

/// One completion item, as a provider supplies it (Monaco `languages.CompletionItem`). Ranges
/// are bytes at the version the list is supplied at; they must lie on the request's line.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompletionItem {
    /// The label (also what is filtered and inserted by default).
    pub label: String,
    /// `CompletionItemLabel.detail`: shown right after the label, dimmed (e.g. a signature).
    pub label_detail: Option<String>,
    /// `CompletionItemLabel.description`: shown right-aligned, dimmed (e.g. a module path).
    pub label_description: Option<String>,
    /// The kind (icon, snippet sorting).
    pub kind: CompletionItemKind,
    /// `CompletionItemTag.Deprecated`: shown struck through.
    pub deprecated: bool,
    /// A human-readable detail (type, signature), shown in the details panel.
    pub detail: Option<String>,
    /// Documentation (plain text or Markdown source), shown in the details panel.
    pub documentation: Option<String>,
    /// Sorts the items before scoring (defaults to the label).
    pub sort_text: Option<String>,
    /// Filtered against instead of the label.
    pub filter_text: Option<String>,
    /// Select this item when the list opens, if it is among the top matches.
    pub preselect: bool,
    /// The text inserted (a snippet with `insert_text_rules.insert_as_snippet`).
    pub insert_text: String,
    /// How the insert text is inserted.
    pub insert_text_rules: InsertTextRules,
    /// What is replaced.
    pub range: CompletionRange,
    /// Typing one of these accepts the item first, then inserts the character.
    pub commit_characters: Vec<String>,
    /// More edits applied with the item (e.g. an import), not touching its range.
    pub additional_text_edits: Vec<(Range<usize>, String)>,
    /// A command run after insertion.
    pub command: Option<CompletionCommand>,
    /// The host can resolve more (detail, documentation, additional edits) for the focused item
    /// (VS Code `resolveCompletionItem`).
    pub needs_resolve: bool,
}

impl CompletionItem {
    /// An item inserting its label over `range`.
    pub fn new(label: impl Into<String>, kind: CompletionItemKind, range: Range<usize>) -> Self {
        let label = label.into();
        CompletionItem {
            insert_text: label.clone(),
            label,
            label_detail: None,
            label_description: None,
            kind,
            deprecated: false,
            detail: None,
            documentation: None,
            sort_text: None,
            filter_text: None,
            preselect: false,
            insert_text_rules: InsertTextRules::default(),
            range: CompletionRange::Replace(range),
            commit_characters: Vec::new(),
            additional_text_edits: Vec::new(),
            command: None,
            needs_resolve: false,
        }
    }

    /// With a snippet as insert text.
    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.insert_text = snippet.into();
        self.insert_text_rules.insert_as_snippet = true;
        self
    }
}

/// One source's items (Monaco `CompletionList`).
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompletionList {
    /// The provider.
    pub source: SourceId,
    /// The items.
    pub items: Vec<CompletionItem>,
    /// `isIncomplete`: typing more asks this source again instead of only filtering.
    pub incomplete: bool,
}

/// A completion provider the host has (VS Code: a registered `CompletionItemProvider`).
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompletionSource {
    /// Its id (as in [`CompletionList::source`]).
    pub id: SourceId,
    /// Characters that trigger it when typed (e.g. `"."`).
    pub trigger_characters: Vec<String>,
}

/// Why completion was requested (Monaco `CompletionTriggerKind`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CompletionTriggerKind {
    /// Invoked (Ctrl+Space, or typing a word: quick suggestions).
    Invoke,
    /// A trigger character was typed.
    TriggerCharacter(String),
    /// Typing more while an incomplete list shows.
    TriggerForIncompleteCompletions,
}

/// Identifies a request; answers carry it back.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompletionRequestId(pub u64);

/// A request for items, for the host's providers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompletionRequest {
    /// Answer with this id.
    pub id: CompletionRequestId,
    /// The cursor (bytes at `version`).
    pub pos: usize,
    /// The document version.
    pub version: crate::Version,
    /// Why.
    pub trigger: CompletionTriggerKind,
    /// `false` for Ctrl+Space (VS Code's manual trigger), `true` for quick suggestions, trigger
    /// characters and retriggers of an automatic session.
    pub auto: bool,
    /// The sources to ask (`None`: all).
    pub sources: Option<Vec<SourceId>>,
}

/// What a view's suggest session wants the host to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SuggestOutput {
    /// Ask the providers.
    Request(CompletionRequest),
    /// Resolve the focused item (`needs_resolve`); answer with
    /// [`EditorView::resolve_completion_item`](crate::EditorView::resolve_completion_item).
    Resolve {
        /// The request the item came from.
        id: CompletionRequestId,
        /// Its source.
        source: SourceId,
        /// Its index in that source's list as supplied.
        index: usize,
        /// The item as supplied.
        item: Arc<CompletionItem>,
    },
    /// An accepted item's command, for the host's registry.
    Command(CompletionCommand),
}

/// Why an answer was refused. Nothing changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CompletionRefusal {
    /// No session, or a newer request replaced this one.
    Superseded,
    /// The version is older than the recent-changes window (or newer than the document).
    Stale,
}

/// `editor.suggest.insertMode`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum InsertMode {
    /// Insert, keeping the text after the cursor.
    #[default]
    Insert,
    /// Replace the whole word.
    Replace,
}

/// `editor.snippetSuggestions`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SnippetSuggestions {
    /// Snippets first.
    Top,
    /// Snippets last.
    Bottom,
    /// Snippets sorted with the rest.
    #[default]
    Inline,
    /// No snippet items.
    None,
}

/// `editor.acceptSuggestionOnEnter`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AcceptSuggestionOnEnter {
    /// Enter accepts.
    #[default]
    On,
    /// Enter accepts only when that changes the text (else it inserts a line break).
    Smart,
    /// Only Tab accepts.
    Off,
}

/// `editor.quickSuggestions`: suggest while typing, per token type.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct QuickSuggestions {
    /// Outside comments and strings.
    pub other: bool,
    /// In comments.
    pub comments: bool,
    /// In strings.
    pub strings: bool,
}

impl Default for QuickSuggestions {
    /// VS Code's default `{ other: "on", comments: "off", strings: "off" }`.
    fn default() -> Self {
        QuickSuggestions {
            other: true,
            comments: false,
            strings: false,
        }
    }
}

/// The suggest options (VS Code `editor.suggest.*` and the related top-level settings).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SuggestConfig {
    /// `editor.quickSuggestions`.
    pub quick_suggestions: QuickSuggestions,
    /// `editor.suggestOnTriggerCharacters`.
    pub suggest_on_trigger_characters: bool,
    /// `editor.acceptSuggestionOnEnter`.
    pub accept_suggestion_on_enter: AcceptSuggestionOnEnter,
    /// `editor.acceptSuggestionOnCommitCharacter`.
    pub accept_suggestion_on_commit_character: bool,
    /// `editor.snippetSuggestions`.
    pub snippet_suggestions: SnippetSuggestions,
    /// `editor.suggest.insertMode`.
    pub insert_mode: InsertMode,
    /// `editor.suggest.filterGraceful`: tolerate swapped characters.
    pub filter_graceful: bool,
    /// `editor.suggest.matchOnWordStartOnly`: the first typed character must match a word
    /// start.
    pub match_on_word_start_only: bool,
    /// `editor.suggest.snippetsPreventQuickSuggestions`.
    pub snippets_prevent_quick_suggestions: bool,
    /// `editor.suggest.showInlineDetails`: the focused row shows its detail when the details
    /// panel is closed.
    pub show_inline_details: bool,
}

impl Default for SuggestConfig {
    fn default() -> Self {
        SuggestConfig {
            quick_suggestions: QuickSuggestions::default(),
            suggest_on_trigger_characters: true,
            accept_suggestion_on_enter: AcceptSuggestionOnEnter::On,
            accept_suggestion_on_commit_character: true,
            snippet_suggestions: SnippetSuggestions::Inline,
            insert_mode: InsertMode::Insert,
            filter_graceful: true,
            match_on_word_start_only: true,
            snippets_prevent_quick_suggestions: false,
            show_inline_details: true,
        }
    }
}
