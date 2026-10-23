//! [`EditorConfig`]: per-document editing options (VS Code's setting names under serde).

/// VS Code's default `editor.wordSeparators`.
pub const DEFAULT_WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// When to auto-close brackets or quotes (VS Code `editor.autoClosingBrackets`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AutoClosingStrategy {
    /// Always.
    Always,
    /// Per language (here: always for configured pairs, only before whitespace or a closing
    /// character, VS Code's `autoCloseBefore`).
    #[default]
    LanguageDefined,
    /// Only when the cursor is before whitespace.
    BeforeWhitespace,
    /// Never.
    Never,
}

/// When to type over an auto-inserted closing character (VS Code `editor.autoClosingOvertype`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum OvertypeStrategy {
    /// Always type over a closing character.
    Always,
    /// Only over closing characters the editor inserted (VS Code's default `auto`).
    #[default]
    Auto,
    /// Never.
    Never,
}

/// What Enter does to the indentation of the new line (VS Code `IndentAction`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum IndentAction {
    /// Keep the current line's indentation.
    #[default]
    None,
    /// One level more than the current line.
    Indent,
    /// Two new lines: the first indented one level, the second (with the text after the
    /// cursor) at the current line's level.
    IndentOutdent,
    /// One level less than the current line.
    Outdent,
}

/// The action of an [`OnEnterRule`] (VS Code `EnterAction`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct EnterAction {
    /// The indentation change (`indentAction` in VS Code's JSON).
    #[cfg_attr(feature = "serde", serde(rename = "indentAction", alias = "indent"))]
    pub indent: IndentAction,
    /// Text appended on the new line after the indentation (e.g. `" * "` in a doc comment).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub append_text: Option<String>,
    /// Number of characters removed from the new line's indentation.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub remove_text: Option<usize>,
}

/// A language rule for Enter (VS Code `OnEnterRule`). The patterns are regexes in JavaScript
/// syntax, as in a VS Code `language-configuration.json` (lookaround and backreferences work;
/// flags go inline, e.g. `(?i)`). A rule applies when every given pattern matches; the first rule
/// that applies wins.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OnEnterRule {
    /// Matched against the line's text before the cursor.
    pub before_text: String,
    /// Matched against the line's text after the cursor.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub after_text: Option<String>,
    /// Matched against the text of the line above.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub previous_line_text: Option<String>,
    /// What to do.
    pub action: EnterAction,
}

/// A language's indentation rules (VS Code `IndentationRule`), regexes in JavaScript syntax
/// tested against whole lines. They drive Enter, Tab on a blank line and re-indentation while
/// typing: a line that comes to match `decrease` (e.g. a typed `}` or `end`) is outdented.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IndentationRules {
    /// The lines after a matching line are indented one level (`increaseIndentPattern`).
    #[cfg_attr(feature = "serde", serde(rename = "increaseIndentPattern"))]
    pub increase: String,
    /// A matching line is outdented one level (`decreaseIndentPattern`).
    #[cfg_attr(feature = "serde", serde(rename = "decreaseIndentPattern"))]
    pub decrease: String,
    /// Only the next line after a matching line is indented (`indentNextLinePattern`).
    #[cfg_attr(
        feature = "serde",
        serde(
            rename = "indentNextLinePattern",
            default,
            skip_serializing_if = "Option::is_none"
        )
    )]
    pub indent_next_line: Option<String>,
    /// Matching lines are ignored when computing indentation (`unIndentedLinePattern`).
    #[cfg_attr(
        feature = "serde",
        serde(
            rename = "unIndentedLinePattern",
            default,
            skip_serializing_if = "Option::is_none"
        )
    )]
    pub unindented_line: Option<String>,
}

/// Options of the comment commands (VS Code `editor.comments`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CommentOptions {
    /// Insert a space after a line comment token and inside block comment tokens, and remove it
    /// with the token (`editor.comments.insertSpace`).
    pub insert_space: bool,
    /// Toggling line comments skips empty and whitespace-only lines
    /// (`editor.comments.ignoreEmptyLines`).
    pub ignore_empty_lines: bool,
}

impl Default for CommentOptions {
    fn default() -> Self {
        CommentOptions {
            insert_space: true,
            ignore_empty_lines: true,
        }
    }
}

/// Editing options of one document.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct EditorConfig {
    /// Columns per tab stop.
    pub tab_size: u32,
    /// Columns per indentation level (VS Code `indentSize`; defaults to `tab_size`).
    pub indent_size: u32,
    /// Insert spaces for Tab.
    pub insert_spaces: bool,
    /// Characters that separate words, beside whitespace.
    pub word_separators: String,
    /// Bracket pairs for indentation on Enter (`{` then Enter indents; between `{}` the closing
    /// bracket moves to its own line).
    pub brackets: Vec<(String, String)>,
    /// Backspace in leading whitespace deletes to the previous indent stop (VS Code
    /// `editor.useTabStops`).
    pub use_tab_stops: bool,
    /// Bracket and quote pairs for auto-close, auto-surround and overtype.
    pub auto_closing_pairs: Vec<(String, String)>,
    /// Pairs that wrap a selection when the opening character is typed over it (VS Code
    /// `surroundingPairs`; usually the same as `auto_closing_pairs`).
    pub surrounding_pairs: Vec<(String, String)>,
    /// Characters before which auto-closing happens (VS Code's `autoCloseBefore`).
    pub auto_close_before: String,
    /// Bracket auto-close policy.
    pub auto_closing_brackets: AutoClosingStrategy,
    /// Quote auto-close policy.
    pub auto_closing_quotes: AutoClosingStrategy,
    /// Overtype policy.
    pub auto_closing_overtype: OvertypeStrategy,
    /// Delete an auto-closed pair together on backspace between them.
    pub auto_closing_delete: bool,
    /// Wrap a selection in a typed bracket or quote.
    pub auto_surround: bool,
    /// Keep the current line's indentation on Enter (and indent after an opening bracket), and
    /// apply `on_enter_rules` and `indentation_rules` (VS Code `editor.autoIndent: "full"`).
    pub auto_indent: bool,
    /// Language rules for Enter, tried before the bracket rules (VS Code `onEnterRules`).
    pub on_enter_rules: Vec<OnEnterRule>,
    /// Language indentation rules (VS Code `indentationRules`).
    pub indentation_rules: Option<IndentationRules>,
    /// The language's line comment token, e.g. `"//"`.
    pub line_comment: Option<String>,
    /// The language's block comment tokens, e.g. `("/*", "*/")`.
    pub block_comment: Option<(String, String)>,
    /// Comment command options (VS Code's `editor.comments.*` settings).
    pub comments: CommentOptions,
    /// Line comments go at column 0 instead of the line's indentation (the language's
    /// `comments.lineComment.noIndent`).
    pub line_comment_no_indent: bool,
    /// Smart Home: Home goes to the first non-whitespace column first.
    pub smart_home: bool,
    /// Edits refused outside foreign (remote/reload) changes.
    pub read_only: bool,
    /// How many change sets the document keeps for mapping stale positions forward.
    pub recent_changes_window: usize,
    /// A typing pause longer than this closes the undo group (milliseconds).
    pub undo_pause_ms: u64,
    /// Copy or cut the whole line when the selection is empty (VS Code
    /// `editor.emptySelectionClipboard`).
    pub empty_selection_clipboard: bool,
    /// The large-file thresholds ([`LargeFilePolicy`](crate::LargeFilePolicy)).
    pub large_file: crate::large::LargeFilePolicy,
    /// Soft wrap (VS Code `editor.wordWrap`).
    pub word_wrap: WordWrap,
    /// Indentation of wrapped rows (VS Code `editor.wrappingIndent`).
    pub wrapping_indent: WrappingIndent,
    /// A line with more characters than this renders only its first this many, followed by a
    /// marker (VS Code `editor.stopRenderingLineAfter`, 10 000). Editing the whole line still
    /// works. `usize::MAX` renders every line in full.
    pub stop_rendering_line_after: usize,
    /// Completion options (VS Code `editor.suggest.*`, `editor.quickSuggestions`, ...).
    pub suggest: crate::completion::SuggestConfig,
    /// Hover options (VS Code `editor.hover.*`).
    pub hover: crate::hover::HoverConfig,
    /// Parameter hints options (VS Code `editor.parameterHints.*`).
    pub parameter_hints: crate::parameter_hints::ParameterHintsConfig,
}

impl Default for EditorConfig {
    fn default() -> Self {
        EditorConfig {
            tab_size: 4,
            indent_size: 4,
            insert_spaces: true,
            word_separators: DEFAULT_WORD_SEPARATORS.to_string(),
            brackets: [("(", ")"), ("[", "]"), ("{", "}")]
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            use_tab_stops: true,
            auto_closing_pairs: [
                ("(", ")"),
                ("[", "]"),
                ("{", "}"),
                ("\"", "\""),
                ("'", "'"),
                ("`", "`"),
            ]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
            surrounding_pairs: [
                ("(", ")"),
                ("[", "]"),
                ("{", "}"),
                ("\"", "\""),
                ("'", "'"),
                ("`", "`"),
            ]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
            auto_close_before: ";:.,=}])> \n\t".to_string(),
            auto_closing_brackets: AutoClosingStrategy::LanguageDefined,
            auto_closing_quotes: AutoClosingStrategy::LanguageDefined,
            auto_closing_overtype: OvertypeStrategy::Auto,
            auto_closing_delete: true,
            auto_surround: true,
            auto_indent: true,
            on_enter_rules: Vec::new(),
            indentation_rules: None,
            line_comment: Some("//".to_string()),
            block_comment: Some(("/*".to_string(), "*/".to_string())),
            comments: CommentOptions::default(),
            line_comment_no_indent: false,
            smart_home: true,
            read_only: false,
            recent_changes_window: 64,
            undo_pause_ms: 1000,
            empty_selection_clipboard: true,
            large_file: crate::large::LargeFilePolicy::default(),
            word_wrap: WordWrap::Off,
            wrapping_indent: WrappingIndent::Same,
            stop_rendering_line_after: 10_000,
            suggest: crate::completion::SuggestConfig::default(),
            hover: crate::hover::HoverConfig::default(),
            parameter_hints: crate::parameter_hints::ParameterHintsConfig::default(),
        }
    }
}

impl EditorConfig {
    /// One indentation unit as text.
    pub fn indent_unit(&self) -> String {
        if self.insert_spaces {
            " ".repeat(self.indent_size.max(1) as usize)
        } else {
            "\t".to_string()
        }
    }
}

/// Soft wrap (VS Code `editor.wordWrap`). Only `off` and `on` are implemented.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WordWrap {
    /// Lines never wrap.
    #[default]
    Off,
    /// Lines wrap at the viewport width.
    On,
}

/// Indentation of a wrapped line's continuation rows (VS Code `editor.wrappingIndent`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WrappingIndent {
    /// Continuation rows start at column 0.
    None,
    /// Continuation rows get the line's indentation.
    #[default]
    Same,
    /// The line's indentation plus one tab stop.
    Indent,
    /// The line's indentation plus two tab stops.
    DeepIndent,
}
