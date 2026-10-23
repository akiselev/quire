//! [`EditorCommand`]: editing behaviour as data, with VS Code's command ids as the serde tag
//! (ARCH-QUIRE §3.5), and [`Motion`].
//!
//! [`Document::command`](crate::Document::command) turns a command into a [`CommandOutput`]
//! without applying it (the pure preview/validate step); [`Document::execute`](crate::Document::execute)
//! applies it. Only commands that are implemented are listed here.

use crate::find::FindQuery;
use crate::selection::Selection;
use crate::transaction::Transaction;

/// Where a cursor moves.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Motion {
    /// One char left (a selection collapses to its start).
    Left,
    /// One char right (a selection collapses to its end).
    Right,
    /// One line up, keeping the goal column.
    Up,
    /// One line down, keeping the goal column.
    Down,
    /// To the previous word start, skipping a lone separator before a word (VS Code
    /// `cursorWordLeft`, "word start fast").
    WordLeft,
    /// To the previous word start (`cursorWordStartLeft`).
    WordStartLeft,
    /// To the previous word end (`cursorWordEndLeft`).
    WordEndLeft,
    /// To the next word start (`cursorWordStartRight`).
    WordStartRight,
    /// To the next word end (`cursorWordEndRight`, the default `cursorWordRight`).
    WordEndRight,
    /// Home: the first non-whitespace column, or column 0 if already there (smart home; plain
    /// column 0 when `EditorConfig::smart_home` is off).
    LineStart,
    /// End of line.
    LineEnd,
    /// Start of the document.
    DocumentStart,
    /// End of the document.
    DocumentEnd,
    /// Up by a page of `lines` lines.
    PageUp {
        /// Lines per page (the widget knows the viewport).
        lines: u32,
    },
    /// Down by a page of `lines` lines.
    PageDown {
        /// Lines per page.
        lines: u32,
    },
}

/// An editor command. The serde tag `id` is VS Code's command id.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "id"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum EditorCommand {
    /// Move every cursor; `select` extends the selections instead.
    #[cfg_attr(feature = "serde", serde(rename = "cursorMove"))]
    CursorMove {
        /// The motion.
        to: Motion,
        /// Extend the selection.
        #[cfg_attr(feature = "serde", serde(default))]
        select: bool,
    },
    /// Replace the selection (host methods, mouse).
    #[cfg_attr(feature = "serde", serde(rename = "setSelection"))]
    SetSelection {
        /// The new selection.
        selection: Selection,
    },
    /// Select the whole document.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.selectAll"))]
    SelectAll,
    /// Select the current line (repeat: extend by a line).
    #[cfg_attr(feature = "serde", serde(rename = "expandLineSelection"))]
    ExpandLineSelection,
    /// Keep only the primary selection.
    #[cfg_attr(feature = "serde", serde(rename = "removeSecondaryCursors"))]
    RemoveSecondaryCursors,
    /// Type text at every cursor (auto-close, auto-surround, overtype, indent on Enter).
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    Type {
        /// The text.
        text: String,
    },
    /// Paste; with `per_cursor` and as many lines as cursors, one line per cursor.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "editor.action.clipboardPasteAction")
    )]
    Paste {
        /// The clipboard text.
        text: String,
        /// Distribute lines over cursors when the counts match.
        #[cfg_attr(feature = "serde", serde(default))]
        per_cursor: bool,
    },
    /// Delete the selections (whole lines for empty selections with
    /// `empty_selection_clipboard`). The widget copies first.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.clipboardCutAction"))]
    Cut,
    /// Backspace.
    #[cfg_attr(feature = "serde", serde(rename = "deleteLeft"))]
    DeleteLeft,
    /// Delete.
    #[cfg_attr(feature = "serde", serde(rename = "deleteRight"))]
    DeleteRight,
    /// Delete to the previous word start (whitespace runs first).
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordLeft"))]
    DeleteWordLeft,
    /// Delete to the next word end (whitespace runs first).
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordRight"))]
    DeleteWordRight,
    /// Delete to the previous word start.
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordStartLeft"))]
    DeleteWordStartLeft,
    /// Delete to the previous word end.
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordEndLeft"))]
    DeleteWordEndLeft,
    /// Delete to the next word start.
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordStartRight"))]
    DeleteWordStartRight,
    /// Delete to the next word end.
    #[cfg_attr(feature = "serde", serde(rename = "deleteWordEndRight"))]
    DeleteWordEndRight,
    /// Tab: indent selected lines, or insert an indent at cursors.
    #[cfg_attr(feature = "serde", serde(rename = "tab"))]
    Tab,
    /// Shift+Tab: outdent.
    #[cfg_attr(feature = "serde", serde(rename = "outdent"))]
    Outdent,
    /// Indent the selected lines.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.indentLines"))]
    IndentLines,
    /// Outdent the selected lines.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.outdentLines"))]
    OutdentLines,
    /// Toggle line comments with the configured token.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.commentLine"))]
    CommentLine,
    /// Add line comments to the selected lines (lines that are already comments get another).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.addCommentLine"))]
    AddCommentLine,
    /// Remove line comments from the selected lines that have one.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.removeCommentLine"))]
    RemoveCommentLine,
    /// Toggle a block comment around each selection (an empty selection gets an empty comment).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.blockComment"))]
    BlockComment,
    /// Replace each tab in every line's indentation with `tab_size` spaces, then set
    /// `insert_spaces` (VS Code "Convert Indentation to Spaces").
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.indentationToSpaces"))]
    IndentationToSpaces,
    /// Replace each run of `tab_size` spaces in every line's indentation with a tab, then clear
    /// `insert_spaces` (VS Code "Convert Indentation to Tabs").
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.indentationToTabs"))]
    IndentationToTabs,
    /// Re-indent the whole document with the language's `indentation_rules` (VS Code "Reindent
    /// Lines"; nothing without rules).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.reindentlines"))]
    ReindentLines,
    /// Flip the document's `EditorConfig::word_wrap` between `Off` and `On` (VS Code "View:
    /// Toggle Word Wrap", Alt+Z). A config change: undo does not revert it, read-only documents
    /// allow it, and [`Applied::config_changed`](crate::Applied::config_changed) reports it.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.toggleWordWrap"))]
    ToggleWordWrap,
    /// Insert a line below and move there (Ctrl+Enter).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.insertLineAfter"))]
    InsertLineAfter,
    /// Insert a line above and move there (Ctrl+Shift+Enter).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.insertLineBefore"))]
    InsertLineBefore,
    /// Delete the selected lines.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.deleteLines"))]
    DeleteLines,
    /// Add a cursor one line above each selection (VS Code `addCursorUp`, logical lines).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.insertCursorAbove"))]
    InsertCursorAbove,
    /// Add a cursor one line below each selection.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.insertCursorBelow"))]
    InsertCursorBelow,
    /// Select the word at an empty cursor, else add the next occurrence of the selected text
    /// (Ctrl+D).
    #[cfg_attr(
        feature = "serde",
        serde(rename = "editor.action.addSelectionToNextFindMatch")
    )]
    AddNextMatch,
    /// Move the selected lines up one line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.moveLinesUpAction"))]
    MoveLinesUp,
    /// Move the selected lines down one line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.moveLinesDownAction"))]
    MoveLinesDown,
    /// Duplicate the selected lines above (the selection stays on the upper copy).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.copyLinesUpAction"))]
    CopyLinesUp,
    /// Duplicate the selected lines below (the selection moves to the lower copy).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.copyLinesDownAction"))]
    CopyLinesDown,
    /// Delete from each cursor to its line start.
    #[cfg_attr(feature = "serde", serde(rename = "deleteAllLeft"))]
    DeleteAllLeft,
    /// Delete from each cursor to its line end (at the line end: join with the next line).
    #[cfg_attr(feature = "serde", serde(rename = "deleteAllRight"))]
    DeleteAllRight,
    /// Column selection: extend the head one visual column left.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectLeft"))]
    ColumnSelectLeft,
    /// Column selection: extend one visual column right.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectRight"))]
    ColumnSelectRight,
    /// Column selection: extend one line up.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectUp"))]
    ColumnSelectUp,
    /// Column selection: extend one line down.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectDown"))]
    ColumnSelectDown,
    /// Column selection: extend a page up.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectPageUp"))]
    ColumnSelectPageUp {
        /// Lines per page.
        lines: u32,
    },
    /// Column selection: extend a page down.
    #[cfg_attr(feature = "serde", serde(rename = "cursorColumnSelectPageDown"))]
    ColumnSelectPageDown {
        /// Lines per page.
        lines: u32,
    },
    /// Column (box) selection from an anchor to a head, e.g. Alt+Shift+drag. Lines are 0-based;
    /// columns are visual (tabs and wide characters expanded) and may lie past a line's end.
    #[cfg_attr(feature = "serde", serde(rename = "columnSelect"))]
    ColumnSelect {
        /// Anchor line.
        anchor_line: usize,
        /// Anchor visual column.
        anchor_column: u32,
        /// Head line.
        head_line: usize,
        /// Head visual column.
        head_column: u32,
    },
    /// Fold the innermost unfolded range at each cursor's line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.fold"))]
    Fold,
    /// Unfold the innermost folded range at each cursor's line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.unfold"))]
    Unfold,
    /// Fold or unfold the innermost range at each cursor's line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.toggleFold"))]
    ToggleFold,
    /// Fold every range.
    #[cfg_attr(feature = "serde", serde(rename = "editor.foldAll"))]
    FoldAll,
    /// Unfold everything.
    #[cfg_attr(feature = "serde", serde(rename = "editor.unfoldAll"))]
    UnfoldAll,
    /// IME: delete `replace_prev_chars` characters before and `replace_next_chars` after each
    /// cursor (on its line; ignored for a non-empty selection), then type `text` through the
    /// typing rules (auto-surround, auto-close: VS Code's composition end), as one transaction in
    /// its own undo group. A commit is `{text, 0, 0}`; DeleteSurrounding has empty `text`.
    #[cfg_attr(feature = "serde", serde(rename = "compositionType"))]
    CompositionType {
        /// The committed text.
        text: String,
        /// Characters (code points) to delete before each cursor.
        #[cfg_attr(feature = "serde", serde(default))]
        replace_prev_chars: usize,
        /// Characters (code points) to delete after each cursor.
        #[cfg_attr(feature = "serde", serde(default))]
        replace_next_chars: usize,
    },
    /// Undo (a host request in host mode).
    #[cfg_attr(feature = "serde", serde(rename = "undo"))]
    Undo,
    /// Redo (a host request in host mode).
    #[cfg_attr(feature = "serde", serde(rename = "redo"))]
    Redo,
    /// Save: a host request only.
    #[cfg_attr(feature = "serde", serde(rename = "workbench.action.files.save"))]
    Save,
    /// Set the find query and select the first match from the cursor.
    #[cfg_attr(feature = "serde", serde(rename = "actions.find"))]
    Find {
        /// The query.
        query: FindQuery,
    },
    /// Select the next match.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.nextMatchFindAction"))]
    FindNext,
    /// Select the previous match.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "editor.action.previousMatchFindAction")
    )]
    FindPrevious,
    /// Replace the selected match (if the selection is one) and select the next.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.replaceOne"))]
    ReplaceOne {
        /// The replacement (`$1` groups for regex queries).
        replacement: String,
    },
    /// Replace every match.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.replaceAll"))]
    ReplaceAll {
        /// The replacement.
        replacement: String,
    },
    /// Clear the find query.
    #[cfg_attr(feature = "serde", serde(rename = "closeFindWidget"))]
    CloseFind,
    /// Open the suggest list (Ctrl+Space): ask the host's providers (VS Code
    /// `editor.action.triggerSuggest`, a manual trigger).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.triggerSuggest"))]
    TriggerSuggest,
    /// Insert the focused suggestion (Enter or Tab).
    #[cfg_attr(feature = "serde", serde(rename = "acceptSelectedSuggestion"))]
    AcceptSelectedSuggestion,
    /// Close the suggest list (Escape).
    #[cfg_attr(feature = "serde", serde(rename = "hideSuggestWidget"))]
    HideSuggestWidget,
    /// Focus the next suggestion (wraps around).
    #[cfg_attr(feature = "serde", serde(rename = "selectNextSuggestion"))]
    SelectNextSuggestion,
    /// Focus the previous suggestion (wraps around).
    #[cfg_attr(feature = "serde", serde(rename = "selectPrevSuggestion"))]
    SelectPrevSuggestion,
    /// Focus a page further down the list.
    #[cfg_attr(feature = "serde", serde(rename = "selectNextPageSuggestion"))]
    SelectNextPageSuggestion,
    /// Focus a page further up the list.
    #[cfg_attr(feature = "serde", serde(rename = "selectPrevPageSuggestion"))]
    SelectPrevPageSuggestion,
    /// Show or hide the details panel of the suggest list.
    #[cfg_attr(feature = "serde", serde(rename = "toggleSuggestionDetails"))]
    ToggleSuggestionDetails,
    /// Select the next snippet placeholder (Tab in a snippet).
    #[cfg_attr(feature = "serde", serde(rename = "jumpToNextSnippetPlaceholder"))]
    JumpToNextSnippetPlaceholder,
    /// Select the previous snippet placeholder (Shift+Tab in a snippet).
    #[cfg_attr(feature = "serde", serde(rename = "jumpToPrevSnippetPlaceholder"))]
    JumpToPrevSnippetPlaceholder,
    /// Leave snippet mode, keeping the primary cursor (Escape in a snippet).
    #[cfg_attr(feature = "serde", serde(rename = "leaveSnippet"))]
    LeaveSnippet,
    /// Show the hover at the primary cursor, or focus it when shown (VS Code
    /// `editor.action.showHover`, Ctrl+K Ctrl+I).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.showHover"))]
    ShowHover,
    /// Hide the hover.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.hideHover"))]
    HideHover,
    /// Scroll the focused hover up a line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.scrollUpHover"))]
    ScrollUpHover,
    /// Scroll the focused hover down a line.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.scrollDownHover"))]
    ScrollDownHover,
    /// Scroll the focused hover left.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.scrollLeftHover"))]
    ScrollLeftHover,
    /// Scroll the focused hover right.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.scrollRightHover"))]
    ScrollRightHover,
    /// Scroll the focused hover up a page.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.pageUpHover"))]
    PageUpHover,
    /// Scroll the focused hover down a page.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.pageDownHover"))]
    PageDownHover,
    /// Scroll the focused hover to its top.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.goToTopHover"))]
    GoToTopHover,
    /// Scroll the focused hover to its bottom.
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.goToBottomHover"))]
    GoToBottomHover,
    /// Ask the host for signature help at the primary cursor (VS Code
    /// `editor.action.triggerParameterHints`, Ctrl+Shift+Space).
    #[cfg_attr(feature = "serde", serde(rename = "editor.action.triggerParameterHints"))]
    TriggerParameterHints,
    /// Close the parameter hints (Escape).
    #[cfg_attr(feature = "serde", serde(rename = "closeParameterHints"))]
    CloseParameterHints,
    /// Show the previous signature (Up).
    #[cfg_attr(feature = "serde", serde(rename = "showPrevParameterHint"))]
    ShowPrevParameterHint,
    /// Show the next signature (Down).
    #[cfg_attr(feature = "serde", serde(rename = "showNextParameterHint"))]
    ShowNextParameterHint,
    /// Scroll a line into view.
    #[cfg_attr(feature = "serde", serde(rename = "revealLine"))]
    RevealLine {
        /// The line.
        line: usize,
    },
}

impl EditorCommand {
    /// The VS Code id.
    pub fn id(&self) -> &'static str {
        use EditorCommand::*;
        match self {
            CursorMove { .. } => "cursorMove",
            SetSelection { .. } => "setSelection",
            SelectAll => "editor.action.selectAll",
            ExpandLineSelection => "expandLineSelection",
            RemoveSecondaryCursors => "removeSecondaryCursors",
            Type { .. } => "type",
            Paste { .. } => "editor.action.clipboardPasteAction",
            Cut => "editor.action.clipboardCutAction",
            DeleteLeft => "deleteLeft",
            DeleteRight => "deleteRight",
            DeleteWordLeft => "deleteWordLeft",
            DeleteWordRight => "deleteWordRight",
            DeleteWordStartLeft => "deleteWordStartLeft",
            DeleteWordEndLeft => "deleteWordEndLeft",
            DeleteWordStartRight => "deleteWordStartRight",
            DeleteWordEndRight => "deleteWordEndRight",
            Tab => "tab",
            Outdent => "outdent",
            IndentLines => "editor.action.indentLines",
            OutdentLines => "editor.action.outdentLines",
            CommentLine => "editor.action.commentLine",
            AddCommentLine => "editor.action.addCommentLine",
            RemoveCommentLine => "editor.action.removeCommentLine",
            BlockComment => "editor.action.blockComment",
            InsertLineAfter => "editor.action.insertLineAfter",
            InsertLineBefore => "editor.action.insertLineBefore",
            DeleteLines => "editor.action.deleteLines",
            InsertCursorAbove => "editor.action.insertCursorAbove",
            InsertCursorBelow => "editor.action.insertCursorBelow",
            AddNextMatch => "editor.action.addSelectionToNextFindMatch",
            MoveLinesUp => "editor.action.moveLinesUpAction",
            MoveLinesDown => "editor.action.moveLinesDownAction",
            CopyLinesUp => "editor.action.copyLinesUpAction",
            CopyLinesDown => "editor.action.copyLinesDownAction",
            DeleteAllLeft => "deleteAllLeft",
            DeleteAllRight => "deleteAllRight",
            IndentationToSpaces => "editor.action.indentationToSpaces",
            IndentationToTabs => "editor.action.indentationToTabs",
            ReindentLines => "editor.action.reindentlines",
            ToggleWordWrap => "editor.action.toggleWordWrap",
            ColumnSelectLeft => "cursorColumnSelectLeft",
            ColumnSelectRight => "cursorColumnSelectRight",
            ColumnSelectUp => "cursorColumnSelectUp",
            ColumnSelectDown => "cursorColumnSelectDown",
            ColumnSelectPageUp { .. } => "cursorColumnSelectPageUp",
            ColumnSelectPageDown { .. } => "cursorColumnSelectPageDown",
            ColumnSelect { .. } => "columnSelect",
            Fold => "editor.fold",
            Unfold => "editor.unfold",
            ToggleFold => "editor.toggleFold",
            FoldAll => "editor.foldAll",
            UnfoldAll => "editor.unfoldAll",
            CompositionType { .. } => "compositionType",
            Undo => "undo",
            Redo => "redo",
            Save => "workbench.action.files.save",
            Find { .. } => "actions.find",
            FindNext => "editor.action.nextMatchFindAction",
            FindPrevious => "editor.action.previousMatchFindAction",
            ReplaceOne { .. } => "editor.action.replaceOne",
            ReplaceAll { .. } => "editor.action.replaceAll",
            CloseFind => "closeFindWidget",
            RevealLine { .. } => "revealLine",
            TriggerSuggest => "editor.action.triggerSuggest",
            AcceptSelectedSuggestion => "acceptSelectedSuggestion",
            HideSuggestWidget => "hideSuggestWidget",
            SelectNextSuggestion => "selectNextSuggestion",
            SelectPrevSuggestion => "selectPrevSuggestion",
            SelectNextPageSuggestion => "selectNextPageSuggestion",
            SelectPrevPageSuggestion => "selectPrevPageSuggestion",
            ToggleSuggestionDetails => "toggleSuggestionDetails",
            JumpToNextSnippetPlaceholder => "jumpToNextSnippetPlaceholder",
            JumpToPrevSnippetPlaceholder => "jumpToPrevSnippetPlaceholder",
            LeaveSnippet => "leaveSnippet",
            ShowHover => "editor.action.showHover",
            HideHover => "editor.action.hideHover",
            ScrollUpHover => "editor.action.scrollUpHover",
            ScrollDownHover => "editor.action.scrollDownHover",
            ScrollLeftHover => "editor.action.scrollLeftHover",
            ScrollRightHover => "editor.action.scrollRightHover",
            PageUpHover => "editor.action.pageUpHover",
            PageDownHover => "editor.action.pageDownHover",
            GoToTopHover => "editor.action.goToTopHover",
            GoToBottomHover => "editor.action.goToBottomHover",
            TriggerParameterHints => "editor.action.triggerParameterHints",
            CloseParameterHints => "closeParameterHints",
            ShowPrevParameterHint => "showPrevParameterHint",
            ShowNextParameterHint => "showNextParameterHint",
        }
    }

    /// The command for an argument-free VS Code id, including the cursor commands VS Code
    /// registers separately (`cursorLeft`, `cursorWordLeftSelect`, `cursorHome`, …), so imported
    /// keymaps resolve. Page motions use `page_lines`.
    pub fn from_vscode_id(id: &str, page_lines: u32) -> Option<EditorCommand> {
        use EditorCommand::*;
        let (base, select) = match id.strip_suffix("Select") {
            Some(b) if b.starts_with("cursor") && !b.starts_with("cursorColumn") => (b, true),
            _ => (id, false),
        };
        let motion = match base {
            "cursorLeft" => Some(Motion::Left),
            "cursorRight" => Some(Motion::Right),
            "cursorUp" => Some(Motion::Up),
            "cursorDown" => Some(Motion::Down),
            "cursorWordLeft" => Some(Motion::WordLeft),
            "cursorWordStartLeft" => Some(Motion::WordStartLeft),
            "cursorWordEndLeft" => Some(Motion::WordEndLeft),
            "cursorWordStartRight" => Some(Motion::WordStartRight),
            "cursorWordRight" | "cursorWordEndRight" => Some(Motion::WordEndRight),
            "cursorHome" => Some(Motion::LineStart),
            "cursorEnd" => Some(Motion::LineEnd),
            "cursorTop" => Some(Motion::DocumentStart),
            "cursorBottom" => Some(Motion::DocumentEnd),
            "cursorPageUp" => Some(Motion::PageUp { lines: page_lines }),
            "cursorPageDown" => Some(Motion::PageDown { lines: page_lines }),
            _ => None,
        };
        if let Some(to) = motion {
            return Some(CursorMove { to, select });
        }
        Some(match id {
            "editor.action.selectAll" => SelectAll,
            "expandLineSelection" => ExpandLineSelection,
            "removeSecondaryCursors" => RemoveSecondaryCursors,
            "editor.action.clipboardCutAction" => Cut,
            "deleteLeft" => DeleteLeft,
            "deleteRight" => DeleteRight,
            "deleteWordLeft" => DeleteWordLeft,
            "deleteWordRight" => DeleteWordRight,
            "deleteWordStartLeft" => DeleteWordStartLeft,
            "deleteWordEndLeft" => DeleteWordEndLeft,
            "deleteWordStartRight" => DeleteWordStartRight,
            "deleteWordEndRight" => DeleteWordEndRight,
            "tab" => Tab,
            "outdent" => Outdent,
            "editor.action.indentLines" => IndentLines,
            "editor.action.outdentLines" => OutdentLines,
            "editor.action.commentLine" => CommentLine,
            "editor.action.addCommentLine" => AddCommentLine,
            "editor.action.removeCommentLine" => RemoveCommentLine,
            "editor.action.blockComment" => BlockComment,
            "editor.action.insertLineAfter" => InsertLineAfter,
            "editor.action.insertLineBefore" => InsertLineBefore,
            "editor.action.deleteLines" => DeleteLines,
            "editor.action.insertCursorAbove" => InsertCursorAbove,
            "editor.action.insertCursorBelow" => InsertCursorBelow,
            "editor.action.addSelectionToNextFindMatch" => AddNextMatch,
            "editor.action.moveLinesUpAction" => MoveLinesUp,
            "editor.action.moveLinesDownAction" => MoveLinesDown,
            "editor.action.copyLinesUpAction" => CopyLinesUp,
            "editor.action.copyLinesDownAction" => CopyLinesDown,
            "deleteAllLeft" => DeleteAllLeft,
            "deleteAllRight" => DeleteAllRight,
            "editor.action.indentationToSpaces" => IndentationToSpaces,
            "editor.action.indentationToTabs" => IndentationToTabs,
            "editor.action.reindentlines" => ReindentLines,
            "editor.action.toggleWordWrap" => ToggleWordWrap,
            "cursorColumnSelectLeft" => ColumnSelectLeft,
            "cursorColumnSelectRight" => ColumnSelectRight,
            "cursorColumnSelectUp" => ColumnSelectUp,
            "cursorColumnSelectDown" => ColumnSelectDown,
            "cursorColumnSelectPageUp" => ColumnSelectPageUp { lines: page_lines },
            "cursorColumnSelectPageDown" => ColumnSelectPageDown { lines: page_lines },
            "editor.fold" => Fold,
            "editor.unfold" => Unfold,
            "editor.toggleFold" => ToggleFold,
            "editor.foldAll" => FoldAll,
            "editor.unfoldAll" => UnfoldAll,
            "undo" => Undo,
            "redo" => Redo,
            "workbench.action.files.save" => Save,
            "editor.action.nextMatchFindAction" => FindNext,
            "editor.action.previousMatchFindAction" => FindPrevious,
            "closeFindWidget" => CloseFind,
            "editor.action.triggerSuggest" => TriggerSuggest,
            "acceptSelectedSuggestion" => AcceptSelectedSuggestion,
            "hideSuggestWidget" => HideSuggestWidget,
            "selectNextSuggestion" => SelectNextSuggestion,
            "selectPrevSuggestion" => SelectPrevSuggestion,
            "selectNextPageSuggestion" => SelectNextPageSuggestion,
            "selectPrevPageSuggestion" => SelectPrevPageSuggestion,
            "toggleSuggestionDetails" => ToggleSuggestionDetails,
            "jumpToNextSnippetPlaceholder" => JumpToNextSnippetPlaceholder,
            "jumpToPrevSnippetPlaceholder" => JumpToPrevSnippetPlaceholder,
            "leaveSnippet" => LeaveSnippet,
            "editor.action.showHover" => ShowHover,
            "editor.action.hideHover" => HideHover,
            "editor.action.scrollUpHover" => ScrollUpHover,
            "editor.action.scrollDownHover" => ScrollDownHover,
            "editor.action.scrollLeftHover" => ScrollLeftHover,
            "editor.action.scrollRightHover" => ScrollRightHover,
            "editor.action.pageUpHover" => PageUpHover,
            "editor.action.pageDownHover" => PageDownHover,
            "editor.action.goToTopHover" => GoToTopHover,
            "editor.action.goToBottomHover" => GoToBottomHover,
            "editor.action.triggerParameterHints" => TriggerParameterHints,
            "closeParameterHints" => CloseParameterHints,
            "showPrevParameterHint" => ShowPrevParameterHint,
            "showNextParameterHint" => ShowNextParameterHint,
            _ => return None,
        })
    }
}

/// Something only the host can do.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum HostRequest {
    /// Save the document (`SaveRequested`).
    Save,
    /// Undo, in [`UndoMode::Host`](crate::UndoMode::Host).
    Undo,
    /// Redo, in host mode.
    Redo,
}

/// What a command resolves to.
#[derive(Clone, PartialEq, Debug)]
#[allow(clippy::large_enum_variant)] // short-lived; boxing would cost every command an allocation
pub enum CommandOutput {
    /// A transaction to apply (possibly selection-only or effects-only).
    Transaction(Transaction),
    /// A request for the host.
    Request(HostRequest),
    /// Nothing to do (e.g. undo with an empty stack, delete at the document start).
    Nothing,
}
