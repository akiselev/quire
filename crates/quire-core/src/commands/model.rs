//! Line/column helpers over [`Text`] for the command ports. Columns here are char indices in
//! the line (0-based); the public API stays in byte offsets.

use crate::text::Text;
use unicode_segmentation::GraphemeCursor;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Pos { line, col }
    }
}

pub(crate) struct Model<'a> {
    pub text: &'a Text,
}

impl<'a> Model<'a> {
    pub fn new(text: &'a Text) -> Self {
        Model { text }
    }

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    pub fn line_string(&self, line: usize) -> String {
        self.text.line(line).to_string()
    }

    pub fn line_chars(&self, line: usize) -> Vec<char> {
        self.text.line(line).chars().collect()
    }

    /// Char count of a line.
    pub fn max_col(&self, line: usize) -> usize {
        let r = self.text.line_range(line);
        self.text.byte_to_char(r.end) - self.text.byte_to_char(r.start)
    }

    pub fn pos(&self, b: usize) -> Pos {
        let b = self.text.clamp_to_boundary(b);
        let line = self.text.byte_to_line(b);
        let ls = self.text.line_to_byte(line);
        Pos::new(line, self.text.byte_to_char(b) - self.text.byte_to_char(ls))
    }

    pub fn off(&self, p: Pos) -> usize {
        let line = p.line.min(self.line_count() - 1);
        let r = self.text.line_range(line);
        let c0 = self.text.byte_to_char(r.start);
        let c1 = self.text.byte_to_char(r.end);
        self.text.char_to_byte((c0 + p.col).min(c1))
    }

    /// The byte offset of the grapheme boundary before `b` on its line (or the previous line's
    /// end at a line start).
    pub fn prev_grapheme(&self, b: usize) -> usize {
        let line = self.text.byte_to_line(b);
        let r = self.text.line_range(line);
        if b <= r.start {
            return if line == 0 { 0 } else { r.start - 1 };
        }
        let s = self.line_string(line);
        let mut gc = GraphemeCursor::new(b - r.start, s.len(), true);
        match gc.prev_boundary(&s, 0) {
            Ok(Some(p)) => r.start + p,
            _ => self.text.prev_char_boundary(b),
        }
    }

    /// The byte offset of the grapheme boundary after `b` (the next line's start at a line end).
    pub fn next_grapheme(&self, b: usize) -> usize {
        let line = self.text.byte_to_line(b);
        let r = self.text.line_range(line);
        if b >= r.end {
            return if line + 1 < self.line_count() {
                r.end + 1
            } else {
                r.end
            };
        }
        let s = self.line_string(line);
        let mut gc = GraphemeCursor::new(b - r.start, s.len(), true);
        match gc.next_boundary(&s, 0) {
            Ok(Some(p)) => r.start + p,
            _ => self.text.next_char_boundary(b),
        }
    }

    /// VS Code `getLeftDeleteOffset`: one code point back, or a whole grapheme when it is an emoji
    /// sequence (so a ZWJ family or a flag goes in one backspace).
    pub fn left_delete_offset(&self, b: usize) -> usize {
        let g = self.prev_grapheme(b);
        if g + 1 == b || g == b {
            return g;
        }
        let cluster = self.text.slice(g..b).to_string();
        if cluster.chars().any(is_emoji_like) {
            g
        } else {
            self.text.prev_char_boundary(b)
        }
    }

    /// Leading whitespace of a line (spaces and tabs).
    pub fn indentation(&self, line: usize) -> String {
        self.text
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Char index of the first non-whitespace char, `None` for a blank line.
    pub fn first_non_ws(&self, line: usize) -> Option<usize> {
        self.text
            .line(line)
            .chars()
            .position(|c| c != ' ' && c != '\t')
    }
}

/// Whether a char takes two columns (CJK wide/fullwidth and most emoji; an approximation of
/// VS Code's `isFullWidthCharacter || isEmojiImprecise`).
pub(crate) fn is_wide(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF |
        0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 |
        0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// Emoji and emoji components (ZWJ, variation selectors, skin tones, tags, regional indicators).
pub(crate) fn is_emoji_like(c: char) -> bool {
    let u = c as u32;
    matches!(u, 0x200D | 0xFE0E | 0xFE0F | 0x20E3 | 0x2600..=0x27BF | 0x1F000..=0x1FAFF | 0xE0020..=0xE007F)
}

fn next_visible(c: char, vis: u32, tab: u32) -> u32 {
    if c == '\t' {
        (vis / tab + 1) * tab
    } else if is_wide(c) {
        vis + 2
    } else {
        vis + 1
    }
}

/// Grapheme clusters of a char slice as (start char index, end char index).
fn graphemes(chars: &[char]) -> Vec<(usize, usize)> {
    use unicode_segmentation::UnicodeSegmentation;
    let s: String = chars.iter().collect();
    let mut out = Vec::new();
    let mut ci = 0;
    for g in s.graphemes(true) {
        let n = g.chars().count();
        out.push((ci, ci + n));
        ci += n;
    }
    out
}

/// `CursorColumns.visibleColumnFromColumn` with a char column; one step per grapheme cluster.
pub(crate) fn visible_col(chars: &[char], col: usize, tab: u32) -> u32 {
    let tab = tab.max(1);
    let mut v = 0;
    for (a, b) in graphemes(chars) {
        if b > col {
            break;
        }
        v = next_visible(chars[a], v, tab);
        let _ = b;
    }
    v
}

/// `CursorColumns.columnFromVisibleColumn`; lands on grapheme boundaries.
pub(crate) fn col_from_visible(chars: &[char], vis: u32, tab: u32) -> usize {
    let tab = tab.max(1);
    if vis == 0 {
        return 0;
    }
    let mut before_vis = 0;
    let mut before_col = 0;
    for (a, b) in graphemes(chars) {
        let after_vis = next_visible(chars[a], before_vis, tab);
        if after_vis >= vis {
            let before_delta = vis - before_vis;
            let after_delta = after_vis - vis;
            return if after_delta < before_delta {
                b
            } else {
                before_col
            };
        }
        before_vis = after_vis;
        before_col = b;
    }
    chars.len()
}

pub(crate) fn prev_indent_tab_stop(vis: u32, size: u32) -> u32 {
    let size = size.max(1);
    if vis == 0 {
        return 0;
    }
    (vis - 1) - (vis - 1) % size
}

pub(crate) fn next_indent_tab_stop(vis: u32, size: u32) -> u32 {
    let size = size.max(1);
    vis + size - vis % size
}
