//! The text a tree is parsed from, as tree-sitter wants it: chunk lookup by byte offset and
//! row/column points.

use std::ops::Range;

/// What the engine needs from a text. Implemented for `quire_core::Text` and for `str` (tests).
pub(crate) trait Source {
    fn len_bytes(&self) -> usize;
    fn chunk_list(&self) -> Vec<&str>;
    fn byte_to_line(&self, b: usize) -> usize;
    fn line_to_byte(&self, line: usize) -> usize;
    fn len_lines(&self) -> usize;
}

impl Source for quire_core::Text {
    fn len_bytes(&self) -> usize {
        quire_core::Text::len_bytes(self)
    }
    fn chunk_list(&self) -> Vec<&str> {
        self.chunks().collect()
    }
    fn byte_to_line(&self, b: usize) -> usize {
        quire_core::Text::byte_to_line(self, b)
    }
    fn line_to_byte(&self, line: usize) -> usize {
        quire_core::Text::line_to_byte(self, line)
    }
    fn len_lines(&self) -> usize {
        quire_core::Text::len_lines(self)
    }
}

/// A string with a line index (tests and benches before the quire-core seam).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct StrText {
    pub s: String,
    starts: Vec<usize>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl StrText {
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        let mut starts = vec![0];
        starts.extend(s.match_indices('\n').map(|(i, _)| i + 1));
        StrText { s, starts }
    }
}

impl Source for StrText {
    fn len_bytes(&self) -> usize {
        self.s.len()
    }
    fn chunk_list(&self) -> Vec<&str> {
        vec![&self.s]
    }
    fn byte_to_line(&self, b: usize) -> usize {
        self.starts.partition_point(|x| *x <= b) - 1
    }
    fn line_to_byte(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.s.len())
    }
    fn len_lines(&self) -> usize {
        self.starts.len()
    }
}

/// Chunks with their start offsets, for O(log n) lookup.
pub(crate) struct Chunks<'a> {
    starts: Vec<usize>,
    chunks: Vec<&'a str>,
    len: usize,
}

impl<'a> Chunks<'a> {
    pub fn new<S: Source + ?Sized>(src: &'a S) -> Self {
        let chunks = src.chunk_list();
        let mut starts = Vec::with_capacity(chunks.len());
        let mut at = 0;
        for c in &chunks {
            starts.push(at);
            at += c.len();
        }
        Chunks {
            starts,
            chunks,
            len: at,
        }
    }

    /// The bytes from `b` to the end of its chunk (empty at the end).
    pub fn at(&self, b: usize) -> &'a [u8] {
        if b >= self.len {
            return &[];
        }
        let i = self.starts.partition_point(|s| *s <= b) - 1;
        &self.chunks[i].as_bytes()[b - self.starts[i]..]
    }

    /// The bytes of `r`, chunk by chunk.
    pub fn slices(&self, r: Range<usize>) -> impl Iterator<Item = &'a [u8]> + '_ {
        let mut at = r.start.min(self.len);
        let end = r.end.min(self.len);
        std::iter::from_fn(move || {
            if at >= end {
                return None;
            }
            let s = self.at(at);
            let n = s.len().min(end - at);
            at += n;
            Some(&s[..n])
        })
    }

    /// Whether `r`'s text equals `s` (no allocation).
    #[allow(dead_code)]
    pub fn eq(&self, r: Range<usize>, s: &str) -> bool {
        if r.len() != s.len() {
            return false;
        }
        let mut off = 0;
        for piece in self.slices(r) {
            if piece != &s.as_bytes()[off..off + piece.len()] {
                return false;
            }
            off += piece.len();
        }
        true
    }

    /// `r`'s text as a string (lossy at broken UTF-8, which a `Text` never has).
    pub fn string(&self, r: Range<usize>) -> String {
        let mut v = Vec::with_capacity(r.len());
        for p in self.slices(r) {
            v.extend_from_slice(p);
        }
        String::from_utf8(v).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }
}

/// The tree-sitter point (row, byte column) of `b`.
pub(crate) fn point<S: Source + ?Sized>(src: &S, b: usize) -> tree_sitter::Point {
    let row = src.byte_to_line(b);
    tree_sitter::Point {
        row,
        column: b - src.line_to_byte(row),
    }
}
