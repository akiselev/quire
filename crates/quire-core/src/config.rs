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
    /// Keep the current line's indentation on Enter (and indent after an opening bracket).
    pub auto_indent: bool,
    /// The language's line comment token, e.g. `"//"`.
    pub line_comment: Option<String>,
    /// The language's block comment tokens, e.g. `("/*", "*/")`.
    pub block_comment: Option<(String, String)>,
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
            line_comment: Some("//".to_string()),
            block_comment: Some(("/*".to_string(), "*/".to_string())),
            smart_home: true,
            read_only: false,
            recent_changes_window: 64,
            undo_pause_ms: 1000,
            empty_selection_clipboard: true,
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
