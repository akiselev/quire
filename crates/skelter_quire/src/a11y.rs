//! AccessKit text runs (Cradle CQ-2): the editor's text as `Role::TextRun` nodes for a page of lines
//! around the caret, as egui_quire's `a11y.rs` projects them (which follows egui 0.37's
//! `text_selection/accesskit_text.rs`).
//!
//! A page is [`PAGE_LINES`] visible lines (lines hidden by folds get no run); it shifts by whole
//! pages when the caret leaves it. A line's runs are its visual rows (soft wrap), each cut into
//! chunks of at most [`MAX_RUN_CHARS`] chars (AccessKit's limit), linked as previous/next on the
//! row; a line followed by another ends with a line feed of width 0, an empty last line has one
//! empty run, and any other empty line has one `"\n"` run. Runs carry the text the editor shows for the document (a projection's replacement
//! text included), never injected text (inlay hints, a preedit). Geometry is in the node's local
//! space (points), as `code-editor` draws it.
//!
//! [`crate::kind::CodeEditorKind`] adds the runs as skelter-core virtual children and points the
//! node's text selection at them.

use quire_core::{Document, EditorView};
use skelter_base::geom::Rect;

use crate::editor::{PAD_Y, SkelterEditor};

/// Lines in one page (egui_quire's page without a screen reader).
pub const PAGE_LINES: usize = 50;
/// The most chars in one run (AccessKit's `character_lengths` are `u8`-indexed).
pub const MAX_RUN_CHARS: usize = 255;

/// One `Role::TextRun`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// The document line.
    pub line: usize,
    /// The visual row of the line.
    pub row: usize,
    /// The chunk of the row, and how many the row has.
    pub chunk: usize,
    pub chunks: usize,
    /// The index in the line's shown chars of the run's first char.
    pub char_start: usize,
    /// Bounds in the node's local space.
    pub rect: Rect,
    pub text: String,
    /// UTF-8 length of each char.
    pub lengths: Vec<u8>,
    /// Each char's x from `rect.min.x`.
    pub positions: Vec<f32>,
    pub widths: Vec<f32>,
    /// Indices of the chars that start a word (after the first).
    pub word_starts: Vec<u8>,
}

/// A position in the runs: (run index, char index in the run).
pub type RunPosition = (usize, usize);

/// The runs of one page and the primary selection mapped onto them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextRuns {
    pub runs: Vec<TextRun>,
    /// The first line of the page.
    pub page_start: usize,
    pub anchor: Option<RunPosition>,
    pub focus: Option<RunPosition>,
}

impl SkelterEditor {
    /// The text runs of the page that holds the caret. `page_start` is the first line of the last
    /// page (0 at first); the page moves only when the caret is outside it. At most `max_runs`
    /// runs (the page is cut short past it).
    pub fn a11y_runs(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        page_start: usize,
        max_runs: usize,
    ) -> TextRuns {
        let t = doc.text();
        let lines = t.len_lines().max(1);
        let row_h = self.geom.row_h.max(f32::EPSILON);
        // pages count visible rows: a folded line has height 0, every visible line one row at its
        // top, so a line's row is y / row_h and the line at row r is the one at mid-row r
        let row_of =
            |e: &SkelterEditor, line: usize| (e.heights.y_of(line) / row_h).round() as usize;
        let line_at = |e: &SkelterEditor, row: usize| e.heights.line_at((row as f32 + 0.5) * row_h);
        let sel = view.selection().primary();
        let head_line = t.byte_to_line(sel.head.min(t.len_bytes()));
        let head_row = row_of(self, head_line);
        let mut start_row = row_of(self, page_start.min(lines - 1));
        if head_row < start_row || head_row >= start_row + PAGE_LINES {
            start_row = (head_row / PAGE_LINES) * PAGE_LINES;
        }
        let page_start = line_at(self, start_row).min(lines - 1);
        let total_rows = (self.heights.total() / row_h).round() as usize;
        let mut page: Vec<usize> = (start_row..(start_row + PAGE_LINES).min(total_rows.max(1)))
            .map(|r| line_at(self, r))
            .filter(|&l| l < lines)
            .collect();
        // a wrapped line spans several rows
        page.dedup();
        if page.is_empty() {
            page.push(page_start);
        }

        let left = self.geom.text_left() - self.scroll.x;
        let mut runs = Vec::new();
        'lines: for &line in &page {
            let l = self.laid_or_layout(doc, view, line);
            let top = PAD_Y - self.scroll.y + self.heights.y_of(line);
            let last_line = line + 1 >= lines;
            let mut char0 = 0usize;
            for k in 0..l.rows.n_rows() {
                let range = l.rows.rows[k].range.clone();
                let text = &l.display.text[range.clone()];
                // (char, x from the text's left edge, width)
                let mut chars: Vec<(char, f32, f32)> = text
                    .char_indices()
                    .map(|(b, c)| (range.start + b, c))
                    .filter(|&(d, _)| l.display.piece_at(d).is_none())
                    .map(|(d, c)| {
                        let x = l.rows.x_in_row(k, d);
                        let x1 = l.rows.x_in_row(k, d + c.len_utf8());
                        (c, x, (x1 - x).max(0.0))
                    })
                    .collect();
                let shown = chars.len();
                if k + 1 == l.rows.n_rows() && !last_line {
                    chars.push(('\n', l.rows.row_width(k), 0.0));
                }
                let row_top = top + k as f32 * row_h;
                let chunks = chars.len().div_ceil(MAX_RUN_CHARS).max(1);
                for chunk in 0..chunks {
                    if runs.len() >= max_runs {
                        break 'lines;
                    }
                    let c0 = chunk * MAX_RUN_CHARS;
                    let c1 = (c0 + MAX_RUN_CHARS).min(chars.len());
                    runs.push(run(
                        line,
                        k,
                        (chunk, chunks),
                        char0 + c0,
                        &chars[c0..c1],
                        left,
                        row_top,
                        row_h,
                    ));
                }
                char0 += shown;
            }
        }

        // a selection end off the page is clamped to the page's first or last position
        let pos_of =
            |e: &mut SkelterEditor, view: &mut EditorView, b: usize| -> Option<RunPosition> {
                let first = runs.first()?;
                let last = runs.len() - 1;
                let b = b.min(t.len_bytes());
                let line = t.byte_to_line(b);
                if line < first.line {
                    return Some((0, 0));
                }
                let mut ks = runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.line == line)
                    .map(|(k, _)| k)
                    .peekable();
                let Some(&k0) = ks.peek() else {
                    return Some((last, runs[last].lengths.len()));
                };
                let col = b - t.line_to_byte(line);
                let l = e.laid_or_layout(doc, view, line);
                let d = l.display.to_display(col).min(l.display.text.len());
                let col_chars = l.display.text[..d]
                    .char_indices()
                    .filter(|&(d, _)| l.display.piece_at(d).is_none())
                    .count();
                // the run holding the char; at a run boundary the earlier run's end
                let k = ks.rfind(|&k| runs[k].char_start < col_chars).unwrap_or(k0);
                let r = &runs[k];
                let i = col_chars.saturating_sub(r.char_start).min(r.lengths.len());
                Some((k, i))
            };
        let anchor = pos_of(self, view, sel.anchor);
        let focus = pos_of(self, view, sel.head);
        TextRuns {
            runs,
            page_start,
            anchor,
            focus,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    line: usize,
    row: usize,
    (chunk, chunks): (usize, usize),
    char_start: usize,
    chars: &[(char, f32, f32)],
    left: f32,
    top: f32,
    row_h: f32,
) -> TextRun {
    let x0 = chars.first().map_or(0.0, |c| c.1);
    let x1 = chars.last().map_or(x0, |c| c.1 + c.2);
    let mut text = String::with_capacity(chars.len());
    let mut lengths = Vec::with_capacity(chars.len());
    let mut word_starts = Vec::new();
    let mut after_word_end = false;
    for (i, &(c, _, _)) in chars.iter().enumerate() {
        let is_word = c.is_alphanumeric() || c == '_';
        if is_word && after_word_end {
            word_starts.push(i as u8);
        }
        after_word_end = !is_word;
        lengths.push(c.len_utf8() as u8);
        text.push(c);
    }
    TextRun {
        line,
        row,
        chunk,
        chunks,
        char_start,
        rect: Rect::new(left + x0, top, left + x0 + (x1 - x0).max(1.0), top + row_h),
        text,
        lengths,
        positions: chars.iter().map(|c| c.1 - x0).collect(),
        widths: chars.iter().map(|c| c.2).collect(),
        word_starts,
    }
}
