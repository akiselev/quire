//! Word classification and multi-click helpers (VS Code's `WordCharacterClassifier` and
//! `wordOperations.ts` rules).

pub mod native;
pub mod vscode;

use crate::text::Text;
use std::ops::Range;

/// A character's class for word motions.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum CharClass {
    /// Part of a word.
    Regular,
    /// Space or tab.
    Whitespace,
    /// A configured word separator.
    Separator,
    /// A line feed.
    LineBreak,
}

/// Classifies characters by the configured word separators.
#[derive(Clone, Debug)]
pub struct WordClassifier {
    separators: Vec<char>,
}

impl WordClassifier {
    /// A classifier for `word_separators`.
    pub fn new(word_separators: &str) -> Self {
        WordClassifier {
            separators: word_separators.chars().collect(),
        }
    }

    /// The class of `c`.
    pub fn class(&self, c: char) -> CharClass {
        if c == '\n' {
            CharClass::LineBreak
        } else if c == ' ' || c == '\t' {
            CharClass::Whitespace
        } else if self.separators.contains(&c) {
            CharClass::Separator
        } else {
            CharClass::Regular
        }
    }
}

/// The word at `pos`, as double-click selects it (VS Code `WordOperations.word`: a word touching
/// the position, else a separator run containing it). Between words, the whitespace run.
pub fn word_range_at(text: &Text, pos: usize, cls: &WordClassifier) -> Range<usize> {
    let pos = text.clamp_to_boundary(pos);
    let m = crate::ops::model::Model::new(text);
    let (a, b) = vscode::word_at(&m, cls, m.pos(pos));
    if a != b {
        return m.off(a)..m.off(b);
    }
    let right = text.char_at(pos).map(|c| cls.class(c));
    let left = text.char_before(pos).map(|c| cls.class(c));
    let class = match (left, right) {
        (_, Some(CharClass::Regular)) => CharClass::Regular,
        (Some(CharClass::Regular), _) => CharClass::Regular,
        (_, Some(CharClass::Separator)) => CharClass::Separator,
        (Some(CharClass::Separator), _) => CharClass::Separator,
        (_, Some(CharClass::Whitespace)) | (Some(CharClass::Whitespace), _) => {
            CharClass::Whitespace
        }
        _ => return pos..pos,
    };
    let mut start = pos;
    for c in text.chars_before(pos) {
        if cls.class(c) != class {
            break;
        }
        start -= c.len_utf8();
    }
    let mut end = pos;
    for c in text.chars_at(pos) {
        if cls.class(c) != class {
            break;
        }
        end += c.len_utf8();
    }
    start..end
}

/// The line at `pos` including its line feed, as triple-click selects it.
pub fn line_range_at(text: &Text, pos: usize) -> Range<usize> {
    let line = text.byte_to_line(pos);
    let start = text.line_to_byte(line);
    let end = if line + 1 < text.len_lines() {
        text.line_to_byte(line + 1)
    } else {
        text.len_bytes()
    };
    start..end
}

/// Where a word motion or deletion stops (VS Code's `WordNavigationType`, plus `deleteWord`'s
/// whitespace-first rule). Pango and Qt have one stop each way and treat every value as
/// [`WordStop::Native`].
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WordStop {
    /// The rule set's own stop: VS Code moves left to a word start (`cursorWordLeft`, "fast")
    /// and right to a word end; it deletes a whitespace run first (`deleteWordLeft/Right`).
    Native,
    /// A word start.
    Start,
    /// A word start, skipping a lone separator before a word (`cursorWordLeft`).
    StartFast,
    /// A word end.
    End,
    /// A word start, a whitespace run first (`deleteWordLeft`).
    StartOrSpace,
    /// A word end, a whitespace run first (`deleteWordRight`).
    EndOrSpace,
}

/// Word boundaries a host brings (an identifier field with `snake_case` as one word).
pub trait WordBoundaries: Sync {
    /// The stop of a forward word move from byte `at`.
    fn next(&self, text: &Text, at: usize, stop: WordStop) -> usize;
    /// The stop of a backward word move from byte `at`.
    fn prev(&self, text: &Text, at: usize, stop: WordStop) -> usize;
    /// The word at `at`, as double-click selects it.
    fn word_at(&self, text: &Text, at: usize) -> Range<usize>;
}

/// Whose word rules a field follows (a host-chosen policy): quire uses VS Code's, svelters'
/// fields each platform's.
#[derive(Clone)]
#[non_exhaustive]
pub enum WordRules {
    /// VS Code's word operations over a classifier (built once; callers cache it).
    VsCode(WordClassifier),
    /// GTK over Pango's default word attributes (svelters FX1).
    Pango,
    /// Qt's `SkipWords` (svelters FX1).
    Qt,
    /// The host's own.
    Custom(&'static dyn WordBoundaries),
}

impl std::fmt::Debug for WordRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WordRules::VsCode(c) => f.debug_tuple("VsCode").field(c).finish(),
            WordRules::Pango => f.write_str("Pango"),
            WordRules::Qt => f.write_str("Qt"),
            WordRules::Custom(_) => f.write_str("Custom"),
        }
    }
}

impl Default for WordRules {
    /// VS Code's rules with VS Code's default separators.
    fn default() -> Self {
        WordRules::VsCode(WordClassifier::new(DEFAULT_WORD_SEPARATORS))
    }
}

/// VS Code's default `editor.wordSeparators`.
pub const DEFAULT_WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

impl WordRules {
    /// The classifier, for VS Code's rules.
    pub fn classifier(&self) -> Option<&WordClassifier> {
        match self {
            WordRules::VsCode(c) => Some(c),
            _ => None,
        }
    }

    /// The word at `pos`, as double-click selects it.
    pub fn word_at(&self, text: &Text, pos: usize) -> Range<usize> {
        match self {
            WordRules::VsCode(c) => word_range_at(text, pos, c),
            WordRules::Pango | WordRules::Qt => {
                let n = self.native().expect("native rules");
                let pos = text.clamp_to_boundary(pos);
                let line = text.line_range(text.byte_to_line(pos));
                let s = text.slice(line.clone()).to_string();
                let col = text.byte_to_char(pos) - text.byte_to_char(line.start);
                let (a, b) = n.word_at(&s, col);
                let to_byte =
                    |c: usize| line.start + s.char_indices().nth(c).map_or(s.len(), |(i, _)| i);
                to_byte(a)..to_byte(b)
            }
            WordRules::Custom(b) => b.word_at(text, pos),
        }
    }

    fn native(&self) -> Option<native::NativeWords> {
        match self {
            WordRules::Pango => Some(native::NativeWords::Pango),
            WordRules::Qt => Some(native::NativeWords::Qt),
            _ => None,
        }
    }

    /// The stop of a forward word move (or the end of a forward word deletion) from byte `at`,
    /// for the native and custom rules (VS Code's go through [`vscode`]). Crosses a line break as
    /// GtkTextView does: from a line's end, to the first stop on the next line.
    pub fn next_native(&self, text: &Text, at: usize, stop: WordStop) -> usize {
        if let WordRules::Custom(b) = self {
            return b.next(text, at, stop);
        }
        let n = self.native().expect("native rules");
        let mut line = text.byte_to_line(at);
        let mut r = text.line_range(line);
        if at >= r.end {
            if line + 1 >= text.len_lines() {
                return text.len_bytes();
            }
            line += 1;
            r = text.line_range(line);
            if r.is_empty() {
                return r.start;
            }
            return in_line(text, r.clone(), r.start, |s, c| n.next(s, c));
        }
        in_line(text, r, at, |s, c| n.next(s, c))
    }

    /// The stop of a backward word move (or the start of a backward word deletion) from byte
    /// `at`; from a line's start, to the last stop on the previous line.
    pub fn prev_native(&self, text: &Text, at: usize, stop: WordStop) -> usize {
        if let WordRules::Custom(b) = self {
            return b.prev(text, at, stop);
        }
        let n = self.native().expect("native rules");
        let mut line = text.byte_to_line(at);
        let mut r = text.line_range(line);
        if at <= r.start {
            if line == 0 {
                return 0;
            }
            line -= 1;
            r = text.line_range(line);
            if r.is_empty() {
                return r.start;
            }
            return in_line(text, r.clone(), r.end, |s, c| n.prev(s, c));
        }
        in_line(text, r, at, |s, c| n.prev(s, c))
    }
}

/// Runs a char-offset word function over one line.
fn in_line(text: &Text, line: Range<usize>, at: usize, f: impl Fn(&str, usize) -> usize) -> usize {
    let s = text.slice(line.clone()).to_cow();
    let col = text.byte_to_char(at) - text.byte_to_char(line.start);
    let c = f(&s, col);
    line.start + s.char_indices().nth(c).map_or(s.len(), |(i, _)| i)
}
