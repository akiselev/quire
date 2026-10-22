//! The longest line, for the horizontal scroll extent, kept up to date incrementally.
//!
//! Each line's width is its visible column count (tabs to tab stops, one column per other
//! character). The table follows the document through its recent changes (`LineEdit`s), so an
//! edit re-measures only the lines it touched; the maximum is recomputed from the table (not the
//! text) only when a line at the maximum shrinks or goes away. A full measure happens on first
//! use, when the tab size changes, and when the table is older than the document's recent-changes
//! window.

use quire_core::{Document, Version};

/// Visible columns per line and their maximum.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineWidths {
    cols: Vec<u32>,
    /// Document version the table describes (None: never measured).
    version: Option<Version>,
    tab_size: u32,
    max: u32,
    max_dirty: bool,
    /// Lines measured since creation (tests check that edits stay incremental).
    pub(crate) measured: u64,
    /// Lines re-measured by incremental syncs since the caller last cleared it (soft wrap
    /// re-estimates their heights).
    pub(crate) changed: Vec<usize>,
    /// Line splices applied by incremental syncs since the caller last took them, in order:
    /// (first line, lines removed, lines inserted). The height index follows them (W1).
    pub(crate) splices: Vec<(usize, usize, usize)>,
    /// A full measure happened since the caller last cleared it (splices are then incomplete).
    pub(crate) rebuilt: bool,
}

/// Visible columns of `chunks` (one line without its break) with tabs every `tab_size` columns.
fn columns<'a>(chunks: impl Iterator<Item = &'a str>, tab_size: u32) -> u32 {
    chunks.fold(0, |col, chunk| advance(col, chunk, tab_size))
}

/// The column after `s` (no line break inside), starting at column `col`.
fn advance(mut col: u32, s: &str, tab_size: u32) -> u32 {
    let ts = tab_size.max(1);
    if s.is_ascii() {
        if !s.as_bytes().contains(&b'\t') {
            return col + s.len() as u32;
        }
        for b in s.bytes() {
            col += if b == b'\t' { ts - col % ts } else { 1 };
        }
    } else {
        for c in s.chars() {
            col += if c == '\t' { ts - col % ts } else { 1 };
        }
    }
    col
}

impl LineWidths {
    fn measure(&mut self, doc: &Document, line: usize) -> u32 {
        self.measured += 1;
        columns(doc.text().line(line).chunks(), self.tab_size)
    }

    /// Measures every line in one pass over the text's chunks (looking each line up in the
    /// rope instead costs about 1.3 µs per line, 130 ms at 100k lines; E1).
    fn rebuild(&mut self, doc: &Document) {
        self.tab_size = doc.config().tab_size;
        let n = doc.text().len_lines();
        self.cols.clear();
        self.cols.reserve(n);
        let mut col = 0u32;
        for chunk in doc.text().chunks() {
            for piece in chunk.split_inclusive('\n') {
                match piece.strip_suffix('\n') {
                    Some(body) => {
                        self.cols.push(advance(col, body, self.tab_size));
                        col = 0;
                    }
                    None => col = advance(col, piece, self.tab_size),
                }
            }
        }
        self.cols.push(col);
        debug_assert_eq!(self.cols.len(), n);
        self.measured += n as u64;
        self.max = self.cols.iter().copied().max().unwrap_or(0);
        self.max_dirty = false;
        self.version = Some(doc.version());
        self.rebuilt = true;
    }

    /// Bring the table up to `doc`'s version.
    pub(crate) fn sync(&mut self, doc: &Document) {
        if self.version == Some(doc.version()) && self.tab_size == doc.config().tab_size {
            return;
        }
        let (Some(at), true) = (self.version, self.tab_size == doc.config().tab_size) else {
            return self.rebuild(doc);
        };
        let Ok(changes) = doc.recent().since(at) else {
            return self.rebuild(doc);
        };
        // Lines whose content is not known yet (measured against the final text at the end).
        let mut dirty: Vec<usize> = Vec::new();
        for change in changes {
            // Edits are in the before-text's coordinates, in order: apply the last first so the
            // earlier ones' line numbers stay valid.
            for e in change.line_edits.iter().rev() {
                let old = e.start_line..(e.end_line + 1).min(self.cols.len());
                let new_n = e.inserted_lines + 1;
                for &c in &self.cols[old.clone()] {
                    if c >= self.max {
                        self.max_dirty = true;
                    }
                }
                let delta = new_n as isize - old.len() as isize;
                for d in &mut dirty {
                    if *d >= old.end {
                        *d = (*d as isize + delta) as usize;
                    } else if *d >= old.start {
                        // Replaced; its replacement lines are marked below.
                        *d = usize::MAX;
                    }
                }
                dirty.retain(|&d| d != usize::MAX);
                self.cols.splice(old.clone(), std::iter::repeat_n(0, new_n));
                if old.len() != new_n {
                    self.splices.push((old.start, old.len(), new_n));
                }
                dirty.extend(old.start..old.start + new_n);
            }
        }
        let n = doc.text().len_lines();
        if self.cols.len() != n {
            // Should not happen (the edits chain); recover with a full measure.
            return self.rebuild(doc);
        }
        dirty.sort_unstable();
        dirty.dedup();
        for d in dirty {
            self.changed.push(d);
            let c = self.measure(doc, d);
            self.cols[d] = c;
            if c > self.max {
                self.max = c;
            }
        }
        self.version = Some(doc.version());
    }

    /// The widest line in columns.
    pub(crate) fn max_cols(&mut self) -> u32 {
        if self.max_dirty {
            self.max = self.cols.iter().copied().max().unwrap_or(0);
            self.max_dirty = false;
        }
        self.max
    }

    /// Columns of `line` (0 past the end).
    pub(crate) fn cols(&self, line: usize) -> u32 {
        self.cols.get(line).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_core::{ChangeSet, EditorView, Transaction};

    fn edit(doc: &mut Document, view: &mut EditorView, range: std::ops::Range<usize>, s: &str) {
        let cs = ChangeSet::replace(doc.text().len_bytes(), [(range, s)]).unwrap();
        doc.apply(Transaction::new(doc.version(), cs), view)
            .unwrap();
    }

    fn check(w: &mut LineWidths, doc: &Document) {
        let mut fresh = LineWidths::default();
        fresh.rebuild(doc);
        assert_eq!(w.cols, fresh.cols, "text {:?}", doc.text().to_string());
        assert_eq!(w.max_cols(), fresh.max_cols());
    }

    #[test]
    fn follows_edits_incrementally() {
        let mut doc = Document::new("ab\n\tx\nlongest line\nz", Default::default());
        let mut view = EditorView::new(&doc);
        let mut w = LineWidths::default();
        w.sync(&doc);
        assert_eq!(w.max_cols(), 12);
        assert_eq!(w.cols(1), 5);
        let base = w.measured;
        // Shrink the longest line: the max drops.
        edit(&mut doc, &mut view, 6..13, "");
        w.sync(&doc);
        check(&mut w, &doc);
        assert_eq!(w.measured - base, 1, "one line re-measured");
        // Split and join lines, several edits in one change set, several change sets per sync.
        edit(&mut doc, &mut view, 1..1, "1234567890\n\n");
        let len = doc.text().len_bytes();
        let cs = ChangeSet::replace(len, [(0..1, "q\nqq"), (len - 1..len, "w\nwww")]).unwrap();
        doc.apply(Transaction::new(doc.version(), cs), &mut view)
            .unwrap();
        w.sync(&doc);
        check(&mut w, &doc);
        let all = doc.text().len_bytes();
        edit(&mut doc, &mut view, 0..all, "");
        w.sync(&doc);
        check(&mut w, &doc);
        assert_eq!(w.max_cols(), 0);
    }

    #[test]
    fn one_pass_rebuild_matches_per_line_measure() {
        // Long enough that the rope splits lines across chunks; tabs and wide chars mixed in.
        let mut src = String::new();
        for i in 0..3000 {
            match i % 5 {
                0 => src.push_str("\tfn x() {}\n"),
                1 => src.push_str("let s = \"grüße\t😀\";\n"),
                2 => src.push('\n'),
                3 => src.push_str(&"ab\t".repeat(i % 37)),
                _ => src.push_str("plain ascii line\n"),
            }
        }
        let doc = Document::new(&src, Default::default());
        let mut w = LineWidths::default();
        w.rebuild(&doc);
        let per_line: Vec<u32> = (0..doc.text().len_lines())
            .map(|i| w.measure(&doc, i))
            .collect();
        assert_eq!(w.cols, per_line);
    }

    #[test]
    fn tab_size_change_remeasures() {
        let mut doc = Document::new("\tx", Default::default());
        let mut w = LineWidths::default();
        w.sync(&doc);
        assert_eq!(w.max_cols(), 5);
        let mut cfg = doc.config().clone();
        cfg.tab_size = 8;
        doc.set_config(cfg);
        w.sync(&doc);
        assert_eq!(w.max_cols(), 9);
    }
}
