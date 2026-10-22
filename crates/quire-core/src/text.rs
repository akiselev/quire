//! [`Text`]: the document text, a rope behind a small API.
//!
//! - In memory the text uses `\n` only. [`Text::from_file`] detects the file's line ending (VS
//!   Code's majority rule) and a UTF-8 byte order mark, normalises `\r\n` and lone `\r` to `\n`,
//!   and records both, so [`Text::to_file_string`] writes the file back byte for byte when it used
//!   one line ending consistently.
//! - Positions are UTF-8 byte offsets. Lines are separated by `\n` only; a text with `n` line
//!   feeds has `n + 1` lines (the last may be empty).
//! - Columns are byte offsets inside the line unless a method says UTF-16 or chars.
//! - The rope type (ropey 1.6) never appears in the public API.

use std::fmt;
use std::ops::{Bound, Range, RangeBounds};

/// The line ending a file used on disk.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum LineEnding {
    /// `\n`
    #[default]
    Lf,
    /// `\r\n`
    CrLf,
}

impl LineEnding {
    /// The line ending as a string.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// The document text. Cloning is O(1) (the rope's nodes are shared), and `Text` is `Send + Sync`.
#[derive(Clone, Default)]
pub struct Text {
    rope: ropey::Rope,
    line_ending: LineEnding,
    bom: bool,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Text")
            .field("len_bytes", &self.len_bytes())
            .field("len_lines", &self.len_lines())
            .field("line_ending", &self.line_ending)
            .field("bom", &self.bom)
            .finish()
    }
}

impl PartialEq for Text {
    /// Compares contents only (line ending and BOM are file properties).
    fn eq(&self, other: &Self) -> bool {
        self.rope == other.rope
    }
}
impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        self.rope == other
    }
}
impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        self.rope == *other
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.rope.chunks() {
            f.write_str(c)?;
        }
        Ok(())
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Text::from_file(s)
    }
}

/// Counts of line breaks in a string: (lone `\r`, lone `\n`, `\r\n`).
fn count_eols(s: &str) -> (usize, usize, usize) {
    let b = s.as_bytes();
    if !b.contains(&b'\r') {
        // The common case, counted in a loop the compiler vectorises.
        return (0, b.iter().filter(|&&c| c == b'\n').count(), 0);
    }
    let (mut cr, mut lf, mut crlf) = (0, 0, 0);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    (cr, lf, crlf)
}

/// Replaces `\r\n` and lone `\r` with `\n`. Borrowed when there is nothing to do.
pub fn normalize_line_endings(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('\r') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

impl Text {
    /// An empty text (LF, no BOM).
    pub fn new() -> Self {
        Text::default()
    }

    /// Text from file contents: strips and records a UTF-8 BOM, detects the line ending
    /// (`\r\n` if more than half of the line breaks contain `\r`, VS Code's rule; `\n` when the
    /// file has none) and normalises every line break to `\n`.
    pub fn from_file(contents: &str) -> Self {
        Text::from_file_with_default(contents, LineEnding::Lf)
    }

    /// As [`Text::from_file`], with the line ending to record for a file that has no line breaks.
    pub fn from_file_with_default(contents: &str, default: LineEnding) -> Self {
        let (bom, body) = match contents.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, contents),
        };
        let (cr, lf, crlf) = count_eols(body);
        let total = cr + lf + crlf;
        let line_ending = if total == 0 {
            default
        } else if 2 * (cr + crlf) > total {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        };
        Text {
            rope: ropey::Rope::from_str(&normalize_line_endings(body)),
            line_ending,
            bom,
        }
    }

    /// The text with every line break written as the recorded line ending and the BOM restored.
    pub fn to_file_string(&self) -> String {
        let mut out = String::with_capacity(self.len_bytes() + self.len_lines() + 3);
        if self.bom {
            out.push('\u{feff}');
        }
        let eol = self.line_ending.as_str();
        for chunk in self.rope.chunks() {
            if self.line_ending == LineEnding::Lf {
                out.push_str(chunk);
            } else {
                for (i, part) in chunk.split('\n').enumerate() {
                    if i > 0 {
                        out.push_str(eol);
                    }
                    out.push_str(part);
                }
            }
        }
        out
    }

    /// The line ending recorded for the file.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Changes the line ending used when writing the file.
    pub fn set_line_ending(&mut self, le: LineEnding) {
        self.line_ending = le;
    }

    /// Whether the file had a UTF-8 byte order mark.
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// Sets whether the file is written with a byte order mark.
    pub fn set_bom(&mut self, bom: bool) {
        self.bom = bom;
    }

    /// Length in bytes (in memory, LF line breaks, no BOM).
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Number of lines: line feeds + 1.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Number of chars (Unicode scalar values).
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// The byte at `i`. Panics if out of bounds.
    pub fn byte(&self, i: usize) -> u8 {
        self.rope.byte(i)
    }

    /// Whether `b` is on a char boundary (`b == len_bytes()` counts).
    pub fn is_char_boundary(&self, b: usize) -> bool {
        b == self.len_bytes() || (b < self.len_bytes() && (self.rope.byte(b) & 0xC0) != 0x80)
    }

    /// The char starting at byte `b`, if `b` is a boundary inside the text.
    pub fn char_at(&self, b: usize) -> Option<char> {
        if b >= self.len_bytes() || !self.is_char_boundary(b) {
            return None;
        }
        Some(self.rope.char(self.rope.byte_to_char(b)))
    }

    /// The char ending at byte `b` (the char before `b`), if any.
    pub fn char_before(&self, b: usize) -> Option<char> {
        if b == 0 || b > self.len_bytes() || !self.is_char_boundary(b) {
            return None;
        }
        Some(self.rope.char(self.rope.byte_to_char(b) - 1))
    }

    /// The byte offset of the char boundary after `b` (clamped to the end).
    pub fn next_char_boundary(&self, b: usize) -> usize {
        let len = self.len_bytes();
        if b >= len {
            return len;
        }
        let c = self.rope.byte_to_char(b);
        self.rope.char_to_byte(c + 1)
    }

    /// The byte offset of the char boundary before `b` (clamped to 0).
    pub fn prev_char_boundary(&self, b: usize) -> usize {
        if b == 0 {
            return 0;
        }
        let b = b.min(self.len_bytes());
        let c = self.rope.byte_to_char(b);
        // byte_to_char rounds down to the char containing b.
        if self.rope.char_to_byte(c) < b {
            self.rope.char_to_byte(c)
        } else {
            self.rope.char_to_byte(c - 1)
        }
    }

    /// Clamps `b` to `0..=len_bytes()` and rounds it down to a char boundary.
    pub fn clamp_to_boundary(&self, b: usize) -> usize {
        let b = b.min(self.len_bytes());
        let c = self.rope.byte_to_char(b);
        self.rope.char_to_byte(c)
    }

    /// Line `i` without its line break. Panics if `i >= len_lines()`.
    pub fn line(&self, i: usize) -> TextSlice<'_> {
        let r = self.line_range(i);
        self.slice(r)
    }

    /// The byte range of line `i` without its line break. Panics if `i >= len_lines()`.
    pub fn line_range(&self, i: usize) -> Range<usize> {
        let start = self.rope.line_to_byte(i);
        let end = if i + 1 < self.len_lines() {
            self.rope.line_to_byte(i + 1) - 1
        } else {
            self.len_bytes()
        };
        start..end
    }

    /// Byte offset of the start of line `i` (`i == len_lines()` gives `len_bytes()`).
    pub fn line_to_byte(&self, i: usize) -> usize {
        self.rope.line_to_byte(i)
    }

    /// The line containing byte `b` (`b == len_bytes()` gives the last line).
    pub fn byte_to_line(&self, b: usize) -> usize {
        self.rope.byte_to_line(b.min(self.len_bytes()))
    }

    /// Byte length of line `i` without its line break.
    pub fn line_len(&self, i: usize) -> usize {
        let r = self.line_range(i);
        r.end - r.start
    }

    /// A slice of the text. Panics if the range is out of bounds or not on char boundaries.
    pub fn slice(&self, r: impl RangeBounds<usize>) -> TextSlice<'_> {
        let start = match r.start_bound() {
            Bound::Included(&s) => s,
            Bound::Excluded(&s) => s + 1,
            Bound::Unbounded => 0,
        };
        let end = match r.end_bound() {
            Bound::Included(&e) => e + 1,
            Bound::Excluded(&e) => e,
            Bound::Unbounded => self.len_bytes(),
        };
        TextSlice {
            slice: self.rope.byte_slice(start..end),
        }
    }

    /// The whole text as a slice.
    pub fn as_slice(&self) -> TextSlice<'_> {
        TextSlice {
            slice: self.rope.slice(..),
        }
    }

    /// `(line, column in bytes)` of byte `b` (clamped to the end).
    pub fn byte_to_line_col(&self, b: usize) -> (usize, usize) {
        let b = b.min(self.len_bytes());
        let line = self.rope.byte_to_line(b);
        (line, b - self.rope.line_to_byte(line))
    }

    /// The byte offset of `(line, column in bytes)`. The line is clamped to the last line and the
    /// column to the line's length.
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.len_lines() - 1);
        let r = self.line_range(line);
        r.start + col.min(r.end - r.start)
    }

    /// The byte offset of `(line, column in UTF-16 code units)`, as LSP sends it. The line is
    /// clamped to the last line and the column to the line's length; a column inside a surrogate
    /// pair rounds down.
    pub fn utf16_to_byte(&self, line: usize, col16: usize) -> usize {
        let line = line.min(self.len_lines() - 1);
        let r = self.line_range(line);
        let start_char = self.rope.byte_to_char(r.start);
        let end_char = self.rope.byte_to_char(r.end);
        let start16 = self.rope.char_to_utf16_cu(start_char);
        let end16 = self.rope.char_to_utf16_cu(end_char);
        let target16 = (start16 + col16).min(end16);
        let mut c = self.rope.utf16_cu_to_char(target16);
        // utf16_cu_to_char rounds a position inside a pair down already; keep it on the line.
        c = c.clamp(start_char, end_char);
        self.rope.char_to_byte(c)
    }

    /// `(line, column in UTF-16 code units)` of byte `b` (clamped to the end, rounded down to a
    /// char boundary).
    pub fn byte_to_utf16(&self, b: usize) -> (usize, usize) {
        let b = self.clamp_to_boundary(b);
        let line = self.rope.byte_to_line(b);
        let ls = self.rope.line_to_byte(line);
        let c16 = self.rope.char_to_utf16_cu(self.rope.byte_to_char(b));
        let l16 = self.rope.char_to_utf16_cu(self.rope.byte_to_char(ls));
        (line, c16 - l16)
    }

    /// Char index of byte `b` (rounded down).
    pub fn byte_to_char(&self, b: usize) -> usize {
        self.rope.byte_to_char(b.min(self.len_bytes()))
    }

    /// Byte offset of char index `c` (clamped to the end).
    pub fn char_to_byte(&self, c: usize) -> usize {
        self.rope.char_to_byte(c.min(self.len_chars()))
    }

    /// The text's chunks in order, for regex engines and compilers.
    pub fn chunks(&self) -> impl Iterator<Item = &str> + '_ {
        self.rope.chunks()
    }

    /// The chars of the text from byte `b` forward.
    pub fn chars_at(&self, b: usize) -> impl Iterator<Item = char> + '_ {
        self.rope
            .chars_at(self.rope.byte_to_char(b.min(self.len_bytes())))
    }

    /// The chars of the text before byte `b`, nearest first.
    pub fn chars_before(&self, b: usize) -> impl Iterator<Item = char> + '_ {
        let mut it = self
            .rope
            .chars_at(self.rope.byte_to_char(b.min(self.len_bytes())));
        std::iter::from_fn(move || it.prev())
    }

    // ---- mutation (crate-internal: the only public mutation path is Document::apply) ----

    pub(crate) fn remove(&mut self, r: Range<usize>) {
        let a = self.rope.byte_to_char(r.start);
        let b = self.rope.byte_to_char(r.end);
        self.rope.remove(a..b);
    }

    pub(crate) fn insert(&mut self, at: usize, s: &str) {
        let c = self.rope.byte_to_char(at);
        self.rope.insert(c, s);
    }
}

/// A borrowed piece of a [`Text`]. It hides the rope type.
#[derive(Clone, Copy)]
pub struct TextSlice<'a> {
    slice: ropey::RopeSlice<'a>,
}

impl fmt::Debug for TextSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl fmt::Display for TextSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.slice.chunks() {
            f.write_str(c)?;
        }
        Ok(())
    }
}

impl PartialEq<str> for TextSlice<'_> {
    fn eq(&self, other: &str) -> bool {
        self.slice == other
    }
}
impl PartialEq<&str> for TextSlice<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.slice == *other
    }
}

impl<'a> TextSlice<'a> {
    /// Length in bytes.
    pub fn len_bytes(&self) -> usize {
        self.slice.len_bytes()
    }

    /// Whether the slice is empty.
    pub fn is_empty(&self) -> bool {
        self.slice.len_bytes() == 0
    }

    /// The slice as one `&str` if it is stored contiguously (no allocation).
    pub fn as_str(&self) -> Option<&'a str> {
        self.slice.as_str()
    }

    /// The slice's chunks in order.
    pub fn chunks(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.slice.chunks()
    }

    /// The slice's chars.
    pub fn chars(&self) -> impl Iterator<Item = char> + 'a {
        self.slice.chars()
    }

    /// The byte at `i` within the slice.
    pub fn byte(&self, i: usize) -> u8 {
        self.slice.byte(i)
    }

    /// A sub-slice (byte range relative to this slice).
    pub fn slice(&self, r: Range<usize>) -> TextSlice<'a> {
        TextSlice {
            slice: self.slice.byte_slice(r),
        }
    }

    /// The slice as a `Cow`: borrowed when contiguous.
    pub fn to_cow(&self) -> std::borrow::Cow<'a, str> {
        match self.slice.as_str() {
            Some(s) => std::borrow::Cow::Borrowed(s),
            None => std::borrow::Cow::Owned(self.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_and_bom_round_trip() {
        let src = "\u{feff}a\r\nb\r\n\r\nc";
        let t = Text::from_file(src);
        assert!(t.has_bom());
        assert_eq!(t.line_ending(), LineEnding::CrLf);
        assert_eq!(t.to_string(), "a\nb\n\nc");
        assert_eq!(t.len_lines(), 4);
        assert_eq!(t.to_file_string(), src);
    }

    #[test]
    fn majority_rule_and_lone_cr() {
        let t = Text::from_file("a\nb\nc\r\nd");
        assert_eq!(t.line_ending(), LineEnding::Lf);
        let t = Text::from_file("a\rb\r\nc\nd");
        assert_eq!(t.line_ending(), LineEnding::CrLf);
        assert_eq!(t.to_string(), "a\nb\nc\nd");
        assert_eq!(Text::from_file("x").line_ending(), LineEnding::Lf);
        let t = Text::from_file_with_default("x", LineEnding::CrLf);
        assert_eq!(t.line_ending(), LineEnding::CrLf);
    }

    #[test]
    fn only_lf_breaks_lines() {
        let t = Text::from_file("a\u{2028}b\u{85}c\nd");
        assert_eq!(t.len_lines(), 2);
    }

    #[test]
    fn lines_and_columns() {
        let t = Text::from_file("héllo\nwörld\n");
        assert_eq!(t.len_lines(), 3);
        assert_eq!(t.line(0), "héllo");
        assert_eq!(t.line(1), "wörld");
        assert_eq!(t.line(2), "");
        assert_eq!(t.line_range(1), 7..13);
        assert_eq!(t.byte_to_line_col(8), (1, 1));
        assert_eq!(t.line_col_to_byte(1, 1), 8);
        assert_eq!(t.line_col_to_byte(1, 99), 13);
        assert_eq!(t.line_col_to_byte(99, 0), 14);
        assert_eq!(t.byte_to_line_col(t.len_bytes()), (2, 0));
    }

    #[test]
    fn utf16_conversions() {
        // 'a' 1 cu, '😀' 2 cu (4 bytes), 'é' 1 cu (2 bytes)
        let t = Text::from_file("x\na😀é");
        assert_eq!(t.utf16_to_byte(1, 0), 2);
        assert_eq!(t.utf16_to_byte(1, 1), 3);
        assert_eq!(t.utf16_to_byte(1, 3), 7);
        assert_eq!(t.utf16_to_byte(1, 4), 9);
        assert_eq!(t.utf16_to_byte(1, 99), 9);
        assert_eq!(t.byte_to_utf16(7), (1, 3));
        assert_eq!(t.byte_to_utf16(9), (1, 4));
        // inside the surrogate pair rounds down
        assert_eq!(t.utf16_to_byte(1, 2), 3);
    }

    #[test]
    fn boundaries() {
        let t = Text::from_file("aé😀");
        assert!(t.is_char_boundary(1));
        assert!(!t.is_char_boundary(2));
        assert_eq!(t.next_char_boundary(1), 3);
        assert_eq!(t.prev_char_boundary(3), 1);
        assert_eq!(t.prev_char_boundary(5), 3);
        assert_eq!(t.clamp_to_boundary(5), 3);
        assert_eq!(t.char_at(3), Some('😀'));
        assert_eq!(t.char_before(3), Some('é'));
        assert_eq!(t.chars_before(3).collect::<String>(), "éa");
    }
}
