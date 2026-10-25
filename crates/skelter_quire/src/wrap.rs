//! Soft wrap: a shaped logical line split into visual rows, and the byte ↔ (row, x) mapping across
//! them.
//!
//! Where a line breaks follows VS Code's `monospaceLineBreaksComputer.ts` (`createLineBreaks`,
//! `canBreak`, `computeWrappedTextIndentLength`), as egui_quire's `wrap.rs` translates it, with the
//! advances measured on the shaped line in points (so proportional fallback glyphs wrap where they
//! end). The line is shaped once; a row is a slice of it, so rewrapping at a new width never
//! reshapes. Rows after the first start at the wrapping indent, and their tabs keep the line's
//! stops.
//!
//! Portions translated from Visual Studio Code:
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md).

use std::ops::Range;
use std::sync::Arc;

use quire_core::WrappingIndent;
use skelter_base::text::Glyph;

use crate::cache::CacheBytes;
use crate::shape::{LineRun, ShapedLine};

/// VS Code's default `editor.wordWrapBreakBeforeCharacters`.
const BREAK_BEFORE: &str = "([{‘“〈《「『【〔（［｛｢£¥＄￡￥+＋";
/// VS Code's default `editor.wordWrapBreakAfterCharacters`.
const BREAK_AFTER: &str = " \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣";

/// VS Code's `CharacterClass`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    None,
    BreakBefore,
    BreakAfter,
    /// Han and Kana: a break is allowed on either side.
    Ideographic,
}

/// `WrappingCharacterClassifier.get`.
fn class(c: char) -> Class {
    let u = c as u32;
    if u < 128 {
        return match c {
            '(' | '[' | '{' | '+' => Class::BreakBefore,
            ' ' | '\t' | '}' | ')' | ']' | '?' | '|' | '/' | '&' | '.' | ',' | ';' => {
                Class::BreakAfter
            }
            _ => Class::None,
        };
    }
    if u >= 256
        && ((0x3040..=0x30FF).contains(&u)
            || (0x3400..=0x4DBF).contains(&u)
            || (0x4E00..=0x9FFF).contains(&u))
    {
        return Class::Ideographic;
    }
    if BREAK_BEFORE.contains(c) {
        Class::BreakBefore
    } else if BREAK_AFTER.contains(c) {
        Class::BreakAfter
    } else {
        Class::None
    }
}

/// `canBreak` (word break "normal"): Kinsoku Shori, never break after a leading character like
/// an open bracket or before a trailing one like a period, and never before a space.
fn can_break(prev: Class, c: char, cls: Class) -> bool {
    c != ' '
        && ((prev == Class::BreakAfter && cls != Class::BreakAfter)
            || (prev != Class::BreakBefore && cls == Class::BreakBefore)
            || (prev == Class::Ideographic && cls != Class::BreakAfter)
            || (cls == Class::Ideographic && prev != Class::BreakBefore))
}

/// `computeWrappedTextIndentLength`: the continuation rows' indentation in columns.
pub(crate) fn indent_cols(text: &str, tab_size: u32, wrap_cols: u32, mode: WrappingIndent) -> u32 {
    if mode == WrappingIndent::None {
        return 0;
    }
    let Some(first) = text.find(|c| c != ' ' && c != '\t') else {
        return 0;
    };
    let ts = tab_size.max(1);
    let mut n = 0u32;
    for b in text[..first].bytes() {
        n += if b == b'\t' { ts - n % ts } else { 1 };
    }
    let extra = match mode {
        WrappingIndent::Indent => 1,
        WrappingIndent::DeepIndent => 2,
        _ => 0,
    };
    for _ in 0..extra {
        n += ts - n % ts;
    }
    // Force sticking to the line start if nothing but the indentation would fit (a full-width
    // character is two columns).
    if n + 2 > wrap_cols { 0 } else { n }
}

/// `createLineBreaks`: the byte offsets where rows 1.. start. `widths[i]` is the advance of the
/// `i`-th char in columns; the first row holds `first` columns, later rows `wrapped`.
pub(crate) fn breaks(text: &str, widths: &[f32], first: f32, wrapped: f32) -> Vec<usize> {
    const EPS: f32 = 1e-3;
    let mut out = Vec::new();
    let mut chars = text.char_indices().zip(widths.iter().copied());
    let Some(((_, c0), w0)) = chars.next() else {
        return out;
    };
    let mut prev = class(c0);
    let mut cur = w0;
    // 0: no break opportunity on this row yet (offset 0 is never a break).
    let mut brk = 0usize;
    let mut brk_w = 0.0f32;
    let mut limit = first;
    for ((i, c), w) in chars {
        let cls = class(c);
        if can_break(prev, c, cls) {
            brk = i;
            brk_w = cur;
        }
        cur += w;
        if cur > limit + EPS {
            // Break at least before this char.
            if brk == 0 || cur - brk_w > wrapped + EPS {
                brk = i;
                brk_w = cur - w;
            }
            let row_start = out.last().copied().unwrap_or(0);
            if brk > row_start {
                out.push(brk);
                limit = brk_w + wrapped;
                brk = 0;
            }
        }
        prev = cls;
    }
    out
}

/// How rows wrap: at `width` points (`None`: never), continuation rows indented per `indent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wrap {
    pub width: Option<f32>,
    pub indent: WrappingIndent,
    pub tab_size: u32,
    /// The width of one column of the editor font (indent columns to points).
    pub column: f32,
}

impl Wrap {
    pub const NONE: Wrap = Wrap {
        width: None,
        indent: WrappingIndent::None,
        tab_size: 4,
        column: 8.0,
    };
}

/// One visual row of a line.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Line-local bytes of the row (without the line feed).
    pub range: Range<usize>,
    /// The line-space x where the row starts.
    pub x0: f32,
    /// The row's left offset (the wrapping indent; 0 for the first row).
    pub indent: f32,
    /// The glyphs of the row, x relative to the row's left edge.
    pub runs: Vec<LineRun>,
}

/// A line laid out in rows.
#[derive(Clone, Debug, PartialEq)]
pub struct LineRows {
    pub shaped: Arc<ShapedLine>,
    pub rows: Vec<Row>,
}

impl CacheBytes for Arc<LineRows> {
    fn cache_bytes(&self) -> usize {
        // the shaped line is counted by its own cache; a single row shares its runs
        if self.rows.len() == 1 {
            return 96;
        }
        let glyphs: usize = self
            .rows
            .iter()
            .flat_map(|r| &r.runs)
            .map(|r| r.run.glyphs.len() * 12 + 64)
            .sum();
        glyphs + self.rows.len() * 64
    }
}

impl LineRows {
    /// `shaped` (of `text`) wrapped per `wrap`.
    pub fn new(text: &str, shaped: Arc<ShapedLine>, wrap: Wrap) -> Self {
        let len = shaped.len();
        let Some(width) = wrap.width.filter(|w| *w > 0.0 && shaped.width > *w) else {
            let rows = vec![Row {
                range: 0..len,
                x0: 0.0,
                indent: 0.0,
                runs: shaped.runs.clone(),
            }];
            return LineRows { shaped, rows };
        };
        let cols = ((width / wrap.column.max(0.01)).floor() as u32).max(1);
        let indent = indent_cols(text, wrap.tab_size, cols, wrap.indent) as f32 * wrap.column;
        let widths = shaped.advances();
        let starts = breaks(text, &widths, width, (width - indent).max(wrap.column));
        let mut bounds = Vec::with_capacity(starts.len() + 2);
        bounds.push(0);
        bounds.extend(starts);
        bounds.push(len);
        let mut rows: Vec<Row> = bounds
            .windows(2)
            .enumerate()
            .map(|(k, w)| Row {
                range: w[0]..w[1],
                x0: shaped.x_of(w[0]),
                indent: if k == 0 { 0.0 } else { indent },
                runs: Vec::new(),
            })
            .collect();
        split_runs(&shaped, &mut rows);
        LineRows { shaped, rows }
    }

    pub fn n_rows(&self) -> usize {
        self.rows.len()
    }

    /// The row holding line-local `byte`; at a wrap point `upstream` picks the row it ends.
    pub fn row_of(&self, byte: usize, upstream: bool) -> usize {
        let i = self
            .rows
            .partition_point(|r| r.range.start <= byte)
            .saturating_sub(1);
        if upstream && i > 0 && self.rows[i].range.start == byte {
            i - 1
        } else {
            i
        }
    }

    /// The x of `byte` on row `k` (from the row's left edge; the wrapping indent included).
    pub fn x_in_row(&self, k: usize, byte: usize) -> f32 {
        let r = &self.rows[k.min(self.rows.len() - 1)];
        self.shaped.x_of(byte.clamp(r.range.start, r.range.end)) - r.x0 + r.indent
    }

    /// (row, x) of `byte`.
    pub fn locate(&self, byte: usize, upstream: bool) -> (usize, f32) {
        let k = self.row_of(byte, upstream);
        (k, self.x_in_row(k, byte))
    }

    /// The byte on row `k` nearest `x`, and whether it is upstream (the end of a wrapped row).
    pub fn hit(&self, k: usize, x: f32) -> (usize, bool) {
        let k = k.min(self.rows.len() - 1);
        let r = &self.rows[k];
        let b = self
            .shaped
            .byte_at(x - r.indent + r.x0)
            .clamp(r.range.start, r.range.end);
        let last = k + 1 == self.rows.len();
        (b, !last && b == r.range.end)
    }

    /// The row's width (from its left edge, indent included).
    pub fn row_width(&self, k: usize) -> f32 {
        let r = &self.rows[k];
        self.shaped.x_of(r.range.end) - r.x0 + r.indent
    }

    /// The widest row.
    pub fn width(&self) -> f32 {
        (0..self.rows.len())
            .map(|k| self.row_width(k))
            .fold(0.0, f32::max)
    }
}

/// Distributes the line's glyphs to `rows` by x (glyph origins ascend within the line for LTR text;
/// a right-to-left run inside a wrapped line keeps its glyphs on the row its origin falls in).
fn split_runs(shaped: &ShapedLine, rows: &mut [Row]) {
    let starts: Vec<f32> = rows.iter().map(|r| r.x0).collect();
    for run in &shaped.runs {
        let mut parts: Vec<Option<(f32, Vec<Glyph>)>> = vec![None; rows.len()];
        for g in &run.run.glyphs {
            let gx = run.x + g.x;
            let k = starts
                .partition_point(|s| *s <= gx + 0.001)
                .saturating_sub(1);
            let p = parts[k].get_or_insert_with(|| (gx, Vec::new()));
            p.1.push(Glyph {
                id: g.id,
                x: gx - p.0,
                y: g.y,
            });
        }
        for (k, p) in parts.into_iter().enumerate() {
            let Some((x0, glyphs)) = p else { continue };
            let row = &mut rows[k];
            let advance = glyphs.last().map_or(0.0, |g| g.x) + glyph_advance_estimate(&run.run);
            let mut sub = (*run.run).clone();
            sub.glyphs = glyphs;
            sub.advance = advance;
            let start = row.range.start.max(run.range.start);
            let end = row.range.end.min(run.range.end).max(start);
            row.runs.push(LineRun {
                run: Arc::new(sub),
                x: x0 - row.x0 + row.indent,
                paint: run.paint,
                range: start..end,
            });
        }
    }
}

/// The advance of a run's last glyph, estimated as the run's mean (used only for ink bounds).
fn glyph_advance_estimate(run: &skelter_base::text::GlyphRun) -> f32 {
    if run.glyphs.is_empty() {
        0.0
    } else {
        run.advance / run.glyphs.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::MonoTable;
    use skelter_base::ids::FontId;
    use skelter_base::text::{AaMode, HintMode};

    fn mono() -> MonoTable {
        let mut glyphs = [0u32; 95];
        for (i, g) in glyphs.iter_mut().enumerate() {
            *g = i as u32;
        }
        MonoTable {
            font: FontId(1),
            size: 13.0,
            variations: 0,
            glyphs,
            advance: 10.0,
            ascent: 10.0,
            descent: 3.0,
            hint: HintMode::Vertical,
            aa: AaMode::Grayscale,
        }
    }

    fn rows(text: &str, width: f32, indent: WrappingIndent) -> LineRows {
        let shaped = Arc::new(mono().shape(text, &[], 4));
        LineRows::new(
            text,
            shaped,
            Wrap {
                width: Some(width),
                indent,
                tab_size: 4,
                column: 10.0,
            },
        )
    }

    #[test]
    fn breaks_after_spaces_like_vs_code() {
        let r = rows("aaaa bbbb cccc", 100.0, WrappingIndent::None);
        let ranges: Vec<Range<usize>> = r.rows.iter().map(|r| r.range.clone()).collect();
        assert_eq!(ranges, vec![0..10, 10..14]);
    }

    #[test]
    fn short_lines_are_one_row_sharing_the_shaped_runs() {
        let r = rows("short", 100.0, WrappingIndent::Same);
        assert_eq!(r.n_rows(), 1);
        assert_eq!(r.rows[0].runs, r.shaped.runs);
    }

    #[test]
    fn continuation_rows_take_the_indent() {
        let r = rows("    aaaa bbbb cccc", 120.0, WrappingIndent::Same);
        assert!(r.n_rows() >= 2);
        assert_eq!(r.rows[1].indent, 40.0);
        let start = r.rows[1].range.start;
        assert_eq!(r.x_in_row(1, start), 40.0);
        // the row's first glyph sits at the indent
        assert_eq!(r.rows[1].runs[0].x, 40.0);
    }

    #[test]
    fn locate_and_hit_agree_at_wrap_points() {
        let r = rows("aaaa bbbb cccc", 100.0, WrappingIndent::None);
        let wrap = r.rows[1].range.start;
        assert_eq!(r.locate(wrap, false).0, 1);
        assert_eq!(r.locate(wrap, true), (0, 100.0));
        assert_eq!(
            r.hit(0, 1000.0),
            (wrap, true),
            "past the end of a wrapped row is upstream"
        );
        assert_eq!(r.hit(1, 1000.0), (14, false), "the last row's end is not");
        assert_eq!(r.hit(1, 0.0), (wrap, false));
    }

    #[test]
    fn every_glyph_lands_on_exactly_one_row() {
        let r = rows("aaaa bbbb cccc dddd eeee", 70.0, WrappingIndent::None);
        let total: usize = r
            .rows
            .iter()
            .flat_map(|row| &row.runs)
            .map(|run| run.run.glyphs.len())
            .sum();
        let shaped: usize = r.shaped.runs.iter().map(|run| run.run.glyphs.len()).sum();
        assert_eq!(total, shaped);
    }
}
