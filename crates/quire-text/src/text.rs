//! [`Text`]: the document text, a rope behind a small API.
//!
//! - In memory the text uses `\n` only. [`Text::from_file`] detects the file's line ending (VS
//!   Code's majority rule) and a UTF-8 byte order mark, normalises `\r\n` and lone `\r` to `\n`,
//!   and records both, so [`Text::to_file_string`] writes the file back byte for byte when it used
//!   one line ending consistently.
//! - Positions are UTF-8 byte offsets. Lines are separated by `\n` only; a text with `n` line
//!   feeds has `n + 1` lines (the last may be empty).
//! - Columns are byte offsets inside the line unless a method says UTF-16 or chars.
//! - **Two backends** behind one API (TC-EVAL T1): a copy-on-write string below
//!   [`SMALL_MAX`] bytes (a field or a short file: no rope nodes, one allocation) and a rope
//!   (ropey 1.6) above. A text is promoted to the rope when it grows past `SMALL_MAX` and demoted
//!   only by `TextDocument::replace_all` to under [`DEMOTE_BELOW`], so editing near the threshold
//!   does not flap. The rope type never appears in the public API.

use std::fmt;
use std::ops::{Bound, Range, RangeBounds};
use std::sync::{Arc, OnceLock};

/// Texts up to this many bytes use the string backend; longer ones the rope.
pub const SMALL_MAX: usize = 64 * 1024;
/// `replace_all` moves a rope text back to the string backend under this many bytes.
pub const DEMOTE_BELOW: usize = 32 * 1024;

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

/// The string backend: the text and, built on the first line query, the offsets of its line
/// feeds. Once built, the index is kept up to date by every mutation (shifted past the edit, with
/// only the inserted text scanned, as ropey keeps its counts), so a line query after an edit
/// costs no rescan.
#[derive(Default)]
struct Small {
    s: String,
    breaks: OnceLock<Vec<u32>>,
}

impl Clone for Small {
    fn clone(&self) -> Self {
        let breaks = OnceLock::new();
        if let Some(b) = self.breaks.get() {
            let _ = breaks.set(b.clone());
        }
        Small {
            s: self.s.clone(),
            breaks,
        }
    }
}

/// Offsets (plus `base`) of the line feeds in `s`, appended to `out`.
fn scan_breaks(s: &str, base: usize, out: &mut Vec<u32>) {
    out.extend(
        s.bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| (base + i) as u32),
    );
}

impl Small {
    fn new(s: String) -> Self {
        Small {
            s,
            breaks: OnceLock::new(),
        }
    }

    /// Offsets of the line feeds.
    fn breaks(&self) -> &[u32] {
        self.breaks.get_or_init(|| {
            let mut v = Vec::new();
            scan_breaks(&self.s, 0, &mut v);
            v
        })
    }

    /// Removes `r` from the text, updating the line-feed index if it is built.
    fn remove(&mut self, r: Range<usize>) {
        if let Some(b) = self.breaks.get_mut() {
            let lo = b.partition_point(|&p| (p as usize) < r.start);
            let hi = b.partition_point(|&p| (p as usize) < r.end);
            let n = r.len() as u32;
            for p in &mut b[hi..] {
                *p -= n;
            }
            b.drain(lo..hi);
        }
        self.s.replace_range(r, "");
    }

    /// Inserts `t` at `at`, updating the line-feed index if it is built.
    fn insert(&mut self, at: usize, t: &str) {
        if let Some(b) = self.breaks.get_mut() {
            let i = b.partition_point(|&p| (p as usize) < at);
            let n = t.len() as u32;
            for p in &mut b[i..] {
                *p += n;
            }
            if t.as_bytes().contains(&b'\n') {
                let mut new = Vec::new();
                scan_breaks(t, at, &mut new);
                b.splice(i..i, new);
            }
        }
        self.s.insert_str(at, t);
    }

    fn len_lines(&self) -> usize {
        if !self.s.contains('\n') {
            return 1;
        }
        self.breaks().len() + 1
    }

    fn line_to_byte(&self, i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        if !self.s.contains('\n') {
            assert!(i == 1, "line {i} out of bounds (1 line)");
            return self.s.len();
        }
        let b = self.breaks();
        if i == b.len() + 1 {
            self.s.len()
        } else {
            b[i - 1] as usize + 1
        }
    }

    fn byte_to_line(&self, b: usize) -> usize {
        if !self.s.contains('\n') {
            return 0;
        }
        self.breaks().partition_point(|&p| (p as usize) < b)
    }
}

/// The largest char boundary of `s` at or below `b` (`b <= s.len()`).
fn floor_boundary(s: &str, mut b: usize) -> usize {
    while !s.is_char_boundary(b) {
        b -= 1;
    }
    b
}

#[derive(Clone)]
enum Repr {
    Small(Arc<Small>),
    Rope(ropey::Rope),
}

/// One of two iterators (the backends' iterators behind one `impl Iterator`).
enum Either<A, B> {
    A(A),
    B(B),
}

impl<T, A: Iterator<Item = T>, B: Iterator<Item = T>> Iterator for Either<A, B> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self {
            Either::A(a) => a.next(),
            Either::B(b) => b.next(),
        }
    }
}

/// The document text. Cloning is O(1) (the string or the rope's nodes are shared, copied on the
/// next write), and `Text` is `Send + Sync`.
#[derive(Clone)]
pub struct Text {
    repr: Repr,
    line_ending: LineEnding,
    bom: bool,
}

impl Default for Text {
    fn default() -> Self {
        Text::from_string(String::new(), LineEnding::Lf, false)
    }
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

/// Whether two chunk sequences spell the same text.
fn chunks_eq<'a, 'b>(a: impl Iterator<Item = &'a str>, b: impl Iterator<Item = &'b str>) -> bool {
    let mut a = a.filter(|c| !c.is_empty());
    let mut b = b.filter(|c| !c.is_empty());
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    loop {
        if x.is_empty() {
            x = a.next().map_or(&[][..], str::as_bytes);
        }
        if y.is_empty() {
            y = b.next().map_or(&[][..], str::as_bytes);
        }
        if x.is_empty() || y.is_empty() {
            return x.is_empty() && y.is_empty();
        }
        let k = x.len().min(y.len());
        if x[..k] != y[..k] {
            return false;
        }
        x = &x[k..];
        y = &y[k..];
    }
}

impl PartialEq for Text {
    /// Compares contents only (line ending and BOM are file properties).
    fn eq(&self, other: &Self) -> bool {
        match (&self.repr, &other.repr) {
            (Repr::Small(a), Repr::Small(b)) => a.s == b.s,
            (Repr::Rope(a), Repr::Rope(b)) => a == b,
            _ => self.len_bytes() == other.len_bytes() && chunks_eq(self.chunks(), other.chunks()),
        }
    }
}
impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        match &self.repr {
            Repr::Small(a) => a.s == other,
            Repr::Rope(r) => r == other,
        }
    }
}
impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.chunks() {
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

    /// A text of LF-only contents on the backend its length calls for.
    fn from_string(s: String, line_ending: LineEnding, bom: bool) -> Self {
        let repr = if s.len() > SMALL_MAX {
            Repr::Rope(ropey::Rope::from_str(&s))
        } else {
            Repr::Small(Arc::new(Small::new(s)))
        };
        Text {
            repr,
            line_ending,
            bom,
        }
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
        let body = normalize_line_endings(body);
        if body.len() > SMALL_MAX {
            return Text {
                repr: Repr::Rope(ropey::Rope::from_str(&body)),
                line_ending,
                bom,
            };
        }
        Text::from_string(body.into_owned(), line_ending, bom)
    }

    /// The text with every line break written as the recorded line ending and the BOM restored.
    pub fn to_file_string(&self) -> String {
        let mut out = String::with_capacity(self.len_bytes() + self.len_lines() + 3);
        if self.bom {
            out.push('\u{feff}');
        }
        let eol = self.line_ending.as_str();
        for chunk in self.chunks() {
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

    /// The whole text as one `&str` when it is stored contiguously (the string backend, below
    /// [`SMALL_MAX`]); `None` for a rope.
    pub fn as_str(&self) -> Option<&str> {
        match &self.repr {
            Repr::Small(a) => Some(&a.s),
            Repr::Rope(_) => None,
        }
    }

    /// Length in bytes (in memory, LF line breaks, no BOM).
    pub fn len_bytes(&self) -> usize {
        match &self.repr {
            Repr::Small(a) => a.s.len(),
            Repr::Rope(r) => r.len_bytes(),
        }
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// Number of lines: line feeds + 1.
    pub fn len_lines(&self) -> usize {
        match &self.repr {
            Repr::Small(a) => a.len_lines(),
            Repr::Rope(r) => r.len_lines(),
        }
    }

    /// Number of chars (Unicode scalar values).
    pub fn len_chars(&self) -> usize {
        match &self.repr {
            Repr::Small(a) => a.s.chars().count(),
            Repr::Rope(r) => r.len_chars(),
        }
    }

    /// The byte at `i`. Panics if out of bounds.
    pub fn byte(&self, i: usize) -> u8 {
        match &self.repr {
            Repr::Small(a) => a.s.as_bytes()[i],
            Repr::Rope(r) => r.byte(i),
        }
    }

    /// Whether `b` is on a char boundary (`b == len_bytes()` counts).
    pub fn is_char_boundary(&self, b: usize) -> bool {
        match &self.repr {
            Repr::Small(a) => a.s.is_char_boundary(b),
            Repr::Rope(r) => {
                b == r.len_bytes() || (b < r.len_bytes() && (r.byte(b) & 0xC0) != 0x80)
            }
        }
    }

    /// The char starting at byte `b`, if `b` is a boundary inside the text.
    pub fn char_at(&self, b: usize) -> Option<char> {
        if b >= self.len_bytes() || !self.is_char_boundary(b) {
            return None;
        }
        match &self.repr {
            Repr::Small(a) => a.s[b..].chars().next(),
            Repr::Rope(r) => Some(r.char(r.byte_to_char(b))),
        }
    }

    /// The char ending at byte `b` (the char before `b`), if any.
    pub fn char_before(&self, b: usize) -> Option<char> {
        if b == 0 || b > self.len_bytes() || !self.is_char_boundary(b) {
            return None;
        }
        match &self.repr {
            Repr::Small(a) => a.s[..b].chars().next_back(),
            Repr::Rope(r) => Some(r.char(r.byte_to_char(b) - 1)),
        }
    }

    /// The byte offset of the char boundary after `b` (clamped to the end).
    pub fn next_char_boundary(&self, b: usize) -> usize {
        let len = self.len_bytes();
        if b >= len {
            return len;
        }
        match &self.repr {
            Repr::Small(a) => {
                let start = floor_boundary(&a.s, b);
                start + a.s[start..].chars().next().map_or(0, char::len_utf8)
            }
            Repr::Rope(r) => r.char_to_byte(r.byte_to_char(b) + 1),
        }
    }

    /// The byte offset of the char boundary before `b` (clamped to 0).
    pub fn prev_char_boundary(&self, b: usize) -> usize {
        if b == 0 {
            return 0;
        }
        let b = b.min(self.len_bytes());
        if b == 0 {
            return 0;
        }
        match &self.repr {
            Repr::Small(a) => {
                let f = floor_boundary(&a.s, b);
                if f < b {
                    f
                } else {
                    floor_boundary(&a.s, b - 1)
                }
            }
            Repr::Rope(r) => {
                let c = r.byte_to_char(b);
                // byte_to_char rounds down to the char containing b.
                if r.char_to_byte(c) < b {
                    r.char_to_byte(c)
                } else {
                    r.char_to_byte(c - 1)
                }
            }
        }
    }

    /// Clamps `b` to `0..=len_bytes()` and rounds it down to a char boundary.
    pub fn clamp_to_boundary(&self, b: usize) -> usize {
        let b = b.min(self.len_bytes());
        match &self.repr {
            Repr::Small(a) => floor_boundary(&a.s, b),
            Repr::Rope(r) => r.char_to_byte(r.byte_to_char(b)),
        }
    }

    /// The grapheme cluster boundary after `b` (clamped to the end; `b` is rounded down to a
    /// char boundary first).
    pub fn grapheme_next(&self, b: usize) -> usize {
        let b = self.clamp_to_boundary(b);
        let len = self.len_bytes();
        let mut gc = unicode_segmentation::GraphemeCursor::new(b, len, true);
        let mut chunks = self.chunk_starts();
        let (mut chunk, mut start) = chunks.next().unwrap_or(("", 0));
        while start + chunk.len() <= b && start + chunk.len() < len {
            (chunk, start) = chunks.next().unwrap_or(("", len));
        }
        loop {
            match gc.next_boundary(chunk, start) {
                Ok(Some(p)) => return p,
                Ok(None) => return len,
                Err(unicode_segmentation::GraphemeIncomplete::NextChunk) => {
                    (chunk, start) = chunks.next().unwrap_or(("", len));
                }
                Err(unicode_segmentation::GraphemeIncomplete::PreContext(n)) => {
                    let (c, s) = self.chunk_containing(n - 1);
                    gc.provide_context(c, s);
                }
                Err(_) => return self.next_char_boundary(b),
            }
        }
    }

    /// The grapheme cluster boundary before `b` (clamped to 0; `b` is rounded down to a char
    /// boundary first).
    pub fn grapheme_prev(&self, b: usize) -> usize {
        let b = self.clamp_to_boundary(b);
        if b == 0 {
            return 0;
        }
        let len = self.len_bytes();
        let mut gc = unicode_segmentation::GraphemeCursor::new(b, len, true);
        let (mut chunk, mut start) = self.chunk_containing(b - 1);
        loop {
            match gc.prev_boundary(chunk, start) {
                Ok(Some(p)) => return p,
                Ok(None) => return 0,
                Err(unicode_segmentation::GraphemeIncomplete::PrevChunk) => {
                    (chunk, start) = self.chunk_containing(start - 1);
                }
                Err(unicode_segmentation::GraphemeIncomplete::PreContext(n)) => {
                    let (c, s) = self.chunk_containing(n - 1);
                    gc.provide_context(c, s);
                }
                Err(_) => return self.prev_char_boundary(b),
            }
        }
    }

    /// The chunks with their byte offsets.
    fn chunk_starts(&self) -> impl Iterator<Item = (&str, usize)> + '_ {
        let mut at = 0;
        self.chunks().map(move |c| {
            let s = at;
            at += c.len();
            (c, s)
        })
    }

    /// The chunk holding byte `b` (`b < len_bytes()`) and its offset.
    fn chunk_containing(&self, b: usize) -> (&str, usize) {
        match &self.repr {
            Repr::Small(a) => (&a.s, 0),
            Repr::Rope(r) => {
                let (c, start, _, _) = r.chunk_at_byte(b);
                (c, start)
            }
        }
    }

    /// Line `i` without its line break. Panics if `i >= len_lines()`.
    pub fn line(&self, i: usize) -> TextSlice<'_> {
        let r = self.line_range(i);
        self.slice(r)
    }

    /// The byte range of line `i` without its line break. Panics if `i >= len_lines()`.
    pub fn line_range(&self, i: usize) -> Range<usize> {
        let start = self.line_to_byte(i);
        let end = if i + 1 < self.len_lines() {
            self.line_to_byte(i + 1) - 1
        } else {
            self.len_bytes()
        };
        start..end
    }

    /// Byte offset of the start of line `i` (`i == len_lines()` gives `len_bytes()`).
    pub fn line_to_byte(&self, i: usize) -> usize {
        match &self.repr {
            Repr::Small(a) => a.line_to_byte(i),
            Repr::Rope(r) => r.line_to_byte(i),
        }
    }

    /// The line containing byte `b` (`b == len_bytes()` gives the last line).
    pub fn byte_to_line(&self, b: usize) -> usize {
        let b = b.min(self.len_bytes());
        match &self.repr {
            Repr::Small(a) => a.byte_to_line(b),
            Repr::Rope(r) => r.byte_to_line(b),
        }
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
        match &self.repr {
            Repr::Small(a) => TextSlice {
                slice: SliceRepr::Str(&a.s[start..end]),
            },
            Repr::Rope(r) => TextSlice {
                slice: SliceRepr::Rope(r.byte_slice(start..end)),
            },
        }
    }

    /// The whole text as a slice.
    pub fn as_slice(&self) -> TextSlice<'_> {
        self.slice(..)
    }

    /// `(line, column in bytes)` of byte `b` (clamped to the end).
    pub fn byte_to_line_col(&self, b: usize) -> (usize, usize) {
        let b = b.min(self.len_bytes());
        let line = self.byte_to_line(b);
        (line, b - self.line_to_byte(line))
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
        match &self.repr {
            Repr::Small(a) => {
                let (mut pos, mut acc) = (r.start, 0);
                for c in a.s[r].chars() {
                    if acc + c.len_utf16() > col16 {
                        break;
                    }
                    acc += c.len_utf16();
                    pos += c.len_utf8();
                }
                pos
            }
            Repr::Rope(rope) => {
                let start_char = rope.byte_to_char(r.start);
                let end_char = rope.byte_to_char(r.end);
                let start16 = rope.char_to_utf16_cu(start_char);
                let end16 = rope.char_to_utf16_cu(end_char);
                let target16 = (start16 + col16).min(end16);
                let mut c = rope.utf16_cu_to_char(target16);
                // utf16_cu_to_char rounds a position inside a pair down already; keep it on the line.
                c = c.clamp(start_char, end_char);
                rope.char_to_byte(c)
            }
        }
    }

    /// `(line, column in UTF-16 code units)` of byte `b` (clamped to the end, rounded down to a
    /// char boundary).
    pub fn byte_to_utf16(&self, b: usize) -> (usize, usize) {
        let b = self.clamp_to_boundary(b);
        let line = self.byte_to_line(b);
        let ls = self.line_to_byte(line);
        match &self.repr {
            Repr::Small(a) => (line, a.s[ls..b].chars().map(char::len_utf16).sum()),
            Repr::Rope(r) => {
                let c16 = r.char_to_utf16_cu(r.byte_to_char(b));
                let l16 = r.char_to_utf16_cu(r.byte_to_char(ls));
                (line, c16 - l16)
            }
        }
    }

    /// The length in UTF-16 code units (what TSF, NSTextInputClient and UIA count in).
    pub fn len_utf16(&self) -> usize {
        match &self.repr {
            Repr::Small(a) => a.s.chars().map(char::len_utf16).sum(),
            Repr::Rope(r) => r.len_utf16_cu(),
        }
    }

    /// The document-wide UTF-16 offset of byte `b` (clamped to the end, rounded down to a char
    /// boundary).
    pub fn byte_to_utf16_offset(&self, b: usize) -> usize {
        let b = self.clamp_to_boundary(b);
        match &self.repr {
            Repr::Small(a) => a.s[..b].chars().map(char::len_utf16).sum(),
            Repr::Rope(r) => r.char_to_utf16_cu(r.byte_to_char(b)),
        }
    }

    /// The byte offset of the document-wide UTF-16 offset `u` (clamped to the end; an offset inside
    /// a surrogate pair rounds down to the pair's start).
    pub fn utf16_offset_to_byte(&self, u: usize) -> usize {
        match &self.repr {
            Repr::Small(a) => {
                let (mut pos, mut acc) = (0, 0);
                for c in a.s.chars() {
                    if acc + c.len_utf16() > u {
                        break;
                    }
                    acc += c.len_utf16();
                    pos += c.len_utf8();
                }
                pos
            }
            Repr::Rope(r) => r.char_to_byte(r.utf16_cu_to_char(u.min(r.len_utf16_cu()))),
        }
    }

    /// Char index of byte `b` (rounded down).
    pub fn byte_to_char(&self, b: usize) -> usize {
        let b = b.min(self.len_bytes());
        match &self.repr {
            Repr::Small(a) => a.s[..floor_boundary(&a.s, b)].chars().count(),
            Repr::Rope(r) => r.byte_to_char(b),
        }
    }

    /// Byte offset of char index `c` (clamped to the end).
    pub fn char_to_byte(&self, c: usize) -> usize {
        match &self.repr {
            Repr::Small(a) => a.s.char_indices().nth(c).map_or(a.s.len(), |(i, _)| i),
            Repr::Rope(r) => r.char_to_byte(c.min(r.len_chars())),
        }
    }

    /// The text's chunks in order, for regex engines and compilers.
    pub fn chunks(&self) -> impl Iterator<Item = &str> + '_ {
        match &self.repr {
            Repr::Small(a) => Either::A(std::iter::once(a.s.as_str())),
            Repr::Rope(r) => Either::B(r.chunks()),
        }
    }

    /// The chars of the text from byte `b` forward.
    pub fn chars_at(&self, b: usize) -> impl Iterator<Item = char> + '_ {
        let b = b.min(self.len_bytes());
        match &self.repr {
            Repr::Small(a) => Either::A(a.s[floor_boundary(&a.s, b)..].chars()),
            Repr::Rope(r) => Either::B(r.chars_at(r.byte_to_char(b))),
        }
    }

    /// The chars of the text before byte `b`, nearest first.
    pub fn chars_before(&self, b: usize) -> impl Iterator<Item = char> + '_ {
        let b = b.min(self.len_bytes());
        match &self.repr {
            Repr::Small(a) => Either::A(a.s[..floor_boundary(&a.s, b)].chars().rev()),
            Repr::Rope(r) => {
                let mut it = r.chars_at(r.byte_to_char(b));
                Either::B(std::iter::from_fn(move || it.prev()))
            }
        }
    }

    // ---- mutation (crate-internal: the only public mutation path is Document::apply) ----

    pub(crate) fn remove(&mut self, r: Range<usize>) {
        match &mut self.repr {
            Repr::Small(a) => {
                Arc::make_mut(a).remove(r);
            }
            Repr::Rope(rope) => {
                let a = rope.byte_to_char(r.start);
                let b = rope.byte_to_char(r.end);
                rope.remove(a..b);
            }
        }
    }

    pub(crate) fn insert(&mut self, at: usize, s: &str) {
        match &mut self.repr {
            Repr::Small(a) => {
                let m = Arc::make_mut(a);
                m.insert(at, s);
                if m.s.len() > SMALL_MAX {
                    self.repr = Repr::Rope(ropey::Rope::from_str(&m.s));
                }
            }
            Repr::Rope(rope) => {
                let c = rope.byte_to_char(at);
                rope.insert(c, s);
            }
        }
    }

    /// Moves a rope text under [`DEMOTE_BELOW`] bytes back to the string backend
    /// (`TextDocument::replace_all` only, so editing near the threshold does not flap).
    pub(crate) fn demote_if_short(&mut self) {
        if let Repr::Rope(r) = &self.repr
            && r.len_bytes() < DEMOTE_BELOW
        {
            self.repr = Repr::Small(Arc::new(Small::new(r.to_string())));
        }
    }

    /// Whether the text uses the rope backend.
    #[cfg(test)]
    fn is_rope(&self) -> bool {
        matches!(self.repr, Repr::Rope(_))
    }
}

#[derive(Clone, Copy)]
enum SliceRepr<'a> {
    Str(&'a str),
    Rope(ropey::RopeSlice<'a>),
}

/// A borrowed piece of a [`Text`]. It hides the backend.
#[derive(Clone, Copy)]
pub struct TextSlice<'a> {
    slice: SliceRepr<'a>,
}

impl fmt::Debug for TextSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl fmt::Display for TextSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.chunks() {
            f.write_str(c)?;
        }
        Ok(())
    }
}

impl PartialEq<str> for TextSlice<'_> {
    fn eq(&self, other: &str) -> bool {
        match self.slice {
            SliceRepr::Str(s) => s == other,
            SliceRepr::Rope(r) => r == other,
        }
    }
}
impl PartialEq<&str> for TextSlice<'_> {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl<'a> TextSlice<'a> {
    /// Length in bytes.
    pub fn len_bytes(&self) -> usize {
        match self.slice {
            SliceRepr::Str(s) => s.len(),
            SliceRepr::Rope(r) => r.len_bytes(),
        }
    }

    /// Whether the slice is empty.
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// The slice as one `&str` if it is stored contiguously (no allocation).
    pub fn as_str(&self) -> Option<&'a str> {
        match self.slice {
            SliceRepr::Str(s) => Some(s),
            SliceRepr::Rope(r) => r.as_str(),
        }
    }

    /// The slice's chunks in order.
    pub fn chunks(&self) -> impl Iterator<Item = &'a str> + 'a {
        match self.slice {
            SliceRepr::Str(s) => Either::A(std::iter::once(s)),
            SliceRepr::Rope(r) => Either::B(r.chunks()),
        }
    }

    /// The slice's chars.
    pub fn chars(&self) -> impl Iterator<Item = char> + 'a {
        match self.slice {
            SliceRepr::Str(s) => Either::A(s.chars()),
            SliceRepr::Rope(r) => Either::B(r.chars()),
        }
    }

    /// The byte at `i` within the slice.
    pub fn byte(&self, i: usize) -> u8 {
        match self.slice {
            SliceRepr::Str(s) => s.as_bytes()[i],
            SliceRepr::Rope(r) => r.byte(i),
        }
    }

    /// A sub-slice (byte range relative to this slice).
    pub fn slice(&self, r: Range<usize>) -> TextSlice<'a> {
        TextSlice {
            slice: match self.slice {
                SliceRepr::Str(s) => SliceRepr::Str(&s[r]),
                SliceRepr::Rope(rope) => SliceRepr::Rope(rope.byte_slice(r)),
            },
        }
    }

    /// The slice as a `Cow`: borrowed when contiguous.
    pub fn to_cow(&self) -> std::borrow::Cow<'a, str> {
        match self.as_str() {
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
    fn document_utf16_offsets() {
        // "x\n" 2 cu, 'a' 1, '😀' 2 (4 bytes), 'é' 1 (2 bytes): 6 cu, 9 bytes
        for t in [
            Text::from_file("x\na😀é"),
            rope_of(&Text::from_file("x\na😀é")),
        ] {
            assert_eq!(t.len_utf16(), 6);
            assert_eq!(t.byte_to_utf16_offset(0), 0);
            assert_eq!(t.byte_to_utf16_offset(3), 3);
            assert_eq!(t.byte_to_utf16_offset(7), 5);
            assert_eq!(t.byte_to_utf16_offset(99), 6);
            assert_eq!(t.utf16_offset_to_byte(3), 3);
            assert_eq!(t.utf16_offset_to_byte(4), 3, "inside the pair rounds down");
            assert_eq!(t.utf16_offset_to_byte(5), 7);
            assert_eq!(t.utf16_offset_to_byte(6), 9);
            assert_eq!(t.utf16_offset_to_byte(99), 9);
        }
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

    /// The same contents on the rope backend, whatever their length.
    fn rope_of(t: &Text) -> Text {
        Text {
            repr: Repr::Rope(ropey::Rope::from_str(&t.to_string())),
            line_ending: t.line_ending,
            bom: t.bom,
        }
    }

    #[test]
    fn backend_hysteresis() {
        use crate::{DocRules, ExternalSet, TextDocument};
        let rules = std::sync::Arc::new(DocRules::default());
        let mut doc = TextDocument::new(&"x".repeat(SMALL_MAX - 1), rules);
        assert!(!doc.text().is_rope());
        let mut view = crate::TextView::new(&doc);
        let end = doc.text().len_bytes();
        let tr = crate::Transaction::new(
            doc.version(),
            crate::ChangeSet::insert_at(end, end, "ab").unwrap(),
        );
        doc.apply(tr, &mut view).unwrap();
        assert!(doc.text().is_rope(), "promoted past SMALL_MAX");
        let len = doc.text().len_bytes();
        let tr = crate::Transaction::new(
            doc.version(),
            crate::ChangeSet::delete_range(len, 10..len).unwrap(),
        );
        doc.apply(tr, &mut view).unwrap();
        assert!(doc.text().is_rope(), "edits never demote");
        doc.replace_all(
            &"y".repeat(DEMOTE_BELOW),
            ExternalSet::Undoable,
            Some(&mut view),
        )
        .unwrap();
        assert!(
            doc.text().is_rope(),
            "replace_all keeps the rope at DEMOTE_BELOW"
        );
        doc.replace_all("short", ExternalSet::Undoable, Some(&mut view))
            .unwrap();
        assert!(
            !doc.text().is_rope(),
            "replace_all demotes under DEMOTE_BELOW"
        );
        assert_eq!(doc.text(), "short");
        assert!(Text::from_file(&"z\n".repeat(SMALL_MAX)).is_rope());
    }

    proptest::proptest! {
        /// Both backends answer every query alike.
        #[test]
        fn backends_agree(src in "[a\n\u{e9}\u{1F600}\u{301} ]{0,40}", probes in proptest::collection::vec(0usize..200, 8)) {
            let a = Text::from_file(&src);
            let b = rope_of(&a);
            proptest::prop_assert!(!a.is_rope() && b.is_rope());
            proptest::prop_assert!(a == b && b == a);
            proptest::prop_assert_eq!(a.to_string(), b.to_string());
            proptest::prop_assert_eq!(a.len_lines(), b.len_lines());
            proptest::prop_assert_eq!(a.len_chars(), b.len_chars());
            for i in 0..=a.len_lines() {
                proptest::prop_assert_eq!(a.line_to_byte(i), b.line_to_byte(i));
            }
            for i in 0..a.len_lines() {
                proptest::prop_assert_eq!(a.line_range(i), b.line_range(i));
                proptest::prop_assert_eq!(a.line(i).to_string(), b.line(i).to_string());
            }
            for &p in &probes {
                let p = p % (a.len_bytes() + 2);
                proptest::prop_assert_eq!(a.byte_to_line(p), b.byte_to_line(p));
                proptest::prop_assert_eq!(a.is_char_boundary(p), b.is_char_boundary(p));
                proptest::prop_assert_eq!(a.char_at(p), b.char_at(p));
                proptest::prop_assert_eq!(a.char_before(p), b.char_before(p));
                proptest::prop_assert_eq!(a.next_char_boundary(p), b.next_char_boundary(p));
                proptest::prop_assert_eq!(a.prev_char_boundary(p), b.prev_char_boundary(p));
                proptest::prop_assert_eq!(a.clamp_to_boundary(p), b.clamp_to_boundary(p));
                proptest::prop_assert_eq!(a.byte_to_char(p), b.byte_to_char(p));
                proptest::prop_assert_eq!(a.char_to_byte(p), b.char_to_byte(p));
                proptest::prop_assert_eq!(a.byte_to_line_col(p), b.byte_to_line_col(p));
                proptest::prop_assert_eq!(a.byte_to_utf16(p), b.byte_to_utf16(p));
                proptest::prop_assert_eq!(a.grapheme_next(p), b.grapheme_next(p));
                proptest::prop_assert_eq!(a.grapheme_prev(p), b.grapheme_prev(p));
                proptest::prop_assert_eq!(a.chars_at(p).collect::<String>(), b.chars_at(p).collect::<String>());
                proptest::prop_assert_eq!(a.chars_before(p).collect::<String>(), b.chars_before(p).collect::<String>());
                let (l, c) = (p % (a.len_lines() + 1), p % 7);
                proptest::prop_assert_eq!(a.line_col_to_byte(l, c), b.line_col_to_byte(l, c));
                proptest::prop_assert_eq!(a.utf16_to_byte(l, c), b.utf16_to_byte(l, c));
            }
            // In-place edits keep agreeing.
            let (mut a, mut b) = (a, b);
            let at = a.clamp_to_boundary(probes[0] % (a.len_bytes() + 1));
            a.insert(at, "q\n\u{1F600}");
            b.insert(at, "q\n\u{1F600}");
            let end = a.next_char_boundary(at);
            a.remove(at..end);
            b.remove(at..end);
            proptest::prop_assert_eq!(a.to_string(), b.to_string());
            proptest::prop_assert_eq!(a.len_lines(), b.len_lines());
            proptest::prop_assert_eq!(a.byte_to_line(at + 1), b.byte_to_line(at + 1));
        }

        /// The string backend's line-feed index, kept up to date through edits, equals a fresh
        /// scan (TC0b).
        #[test]
        fn incremental_line_index(
            src in "[ab\n]{0,40}",
            edits in proptest::collection::vec((0usize..64, 0usize..6, "[xy\n]{0,4}"), 1..12),
        ) {
            let mut a = Text::from_file(&src);
            a.len_lines(); // build the index
            for (at, del, ins) in edits {
                let at = at % (a.len_bytes() + 1);
                let end = (at + del).min(a.len_bytes());
                a.remove(at..end);
                a.insert(at, &ins);
                let fresh = Text::from_file(&a.to_string());
                proptest::prop_assert_eq!(a.len_lines(), fresh.len_lines());
                for i in 0..=a.len_lines() {
                    proptest::prop_assert_eq!(a.line_to_byte(i), fresh.line_to_byte(i));
                }
                for p in 0..=a.len_bytes() {
                    proptest::prop_assert_eq!(a.byte_to_line(p), fresh.byte_to_line(p));
                }
                // A copy-on-write copy carries the index along.
                let mut c = a.clone();
                c.insert(0, "\n");
                proptest::prop_assert_eq!(c.len_lines(), a.len_lines() + 1);
            }
        }
    }
}
