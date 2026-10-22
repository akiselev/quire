//! [`DocRules`]: what text a [`TextDocument`](crate::TextDocument) accepts and how its history
//! behaves. Shared as `Arc<DocRules>`: a table of fields holds one per kind of field.

use crate::history::{UndoMode, UndoRules};

/// How line breaks are treated.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LineMode {
    /// Any number of lines (an editor, a multi-line field).
    #[default]
    Multi,
    /// A single-line field.
    Single {
        /// What happens to line breaks in inserted text.
        breaks: LineBreaks,
    },
}

impl LineMode {
    /// A single-line field that strips inserted line breaks.
    pub const SINGLE: LineMode = LineMode::Single {
        breaks: LineBreaks::Strip,
    };

    /// Whether this is a single-line mode (the host's keymap does not consume Enter).
    pub fn is_single(self) -> bool {
        matches!(self, LineMode::Single { .. })
    }
}

/// Line breaks inserted into a single-line field.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LineBreaks {
    /// Removed (HTML `<input>`). Local transactions inserting a break are refused.
    #[default]
    Strip,
    /// Each break becomes a space. Local transactions inserting a break are refused.
    Spaces,
    /// Kept (GTK 4's `GtkText`, measured by svelters FX3).
    Keep,
}

/// What text a document accepts and how its history behaves. `apply` enforces the content rules
/// (`lines`, `max_chars`, `read_only`) on every local transaction; foreign changes (remote,
/// reload) always apply.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocRules {
    /// Undo grouping rules.
    pub undo: UndoRules,
    /// Who owns undo.
    pub undo_mode: UndoMode,
    /// Monaco's pause rule: a pause longer than this between edits closes the undo group.
    /// `None` turns the rule off (GTK, Qt).
    pub undo_pause_ms: Option<u64>,
    /// At most this many undo steps are kept (the oldest are dropped). `None`: unbounded.
    pub undo_limit: Option<usize>,
    /// Single- or multi-line.
    pub lines: LineMode,
    /// At most this many characters, counted in Unicode code points (GTK's `max-length`). An
    /// insertion is truncated at a code point to fit. HTML's `maxlength` counts UTF-16 code
    /// units instead; a host bridging HTML semantics converts.
    pub max_chars: Option<u32>,
    /// Read-only: local edits are refused; motions, selection and copy still work.
    pub read_only: bool,
    /// How many recent change sets are kept for rebasing and view sync (allocated on the first
    /// edit). Fields: 4; quire's editor: 64.
    pub recent_window: usize,
    /// Record line edits with each recent change (quire-core's tokens and folds follow them).
    /// Off for fields.
    pub track_line_edits: bool,
}

impl Default for DocRules {
    /// Multi-line, Monaco grouping with a 1 s pause, local undo, a window of 64, line edits
    /// tracked: quire's editor defaults.
    fn default() -> Self {
        DocRules {
            undo: UndoRules::Monaco,
            undo_mode: UndoMode::Local,
            undo_pause_ms: Some(1000),
            undo_limit: None,
            lines: LineMode::Multi,
            max_chars: None,
            read_only: false,
            recent_window: 64,
            track_line_edits: true,
        }
    }
}

impl DocRules {
    /// A text field's rules: `lines`, `undo` grouping from the platform, a recent window of 4,
    /// no line edits, no pause rule unless Monaco.
    pub fn field(lines: LineMode, undo: UndoRules) -> Self {
        DocRules {
            undo_pause_ms: (undo == UndoRules::Monaco).then_some(1000),
            undo,
            lines,
            recent_window: 4,
            track_line_edits: false,
            ..DocRules::default()
        }
    }
}
