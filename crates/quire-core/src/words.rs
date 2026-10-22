//! Word classification and multi-click helpers (VS Code's `WordCharacterClassifier` and
//! `wordOperations.ts` rules).

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
    let m = crate::commands::model::Model::new(text);
    let (a, b) = crate::commands::words::word_at(&m, cls, m.pos(pos));
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
