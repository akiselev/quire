//! Soft wrap (QE4 / ED2) and long-line prefixes (E3 F2): a document line laid out as one or more
//! visual rows, and the byte ↔ (row, x) mapping across them.
//!
//! Where a line breaks follows VS Code's `monospaceLineBreaksComputer.ts` (`createLineBreaks`,
//! `canBreak`, `computeWrappedTextIndentLength`), translated to Rust with character advances
//! measured in columns of the editor font. Each row is laid out as its own galley; rows after the
//! first start at the wrapping indent, and their tabs stop at the line's tab stops.
//!
//! Portions translated from Visual Studio Code:
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md).

use std::sync::Arc;

use egui::text::TextFormat;
use egui::{Color32, Context};
use quire_core::WrappingIndent;

use crate::galley_cache::{GalleyCache, GalleyKey, hash_one};
use crate::layout::{self, LineLayout, LineShaper, Mono};

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

/// Wrapping of one frame: rows hold `cols` columns of the editor font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Wrap {
    pub cols: u32,
    pub indent: WrappingIndent,
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

/// Per-char advances in columns: arithmetic for ASCII (tabs to the line's stops), else from a
/// one-row layout of the whole text.
fn widths_of(text: &str, tab_size: u32, one: Option<&LineLayout>, adv: f32) -> Vec<f32> {
    if text.is_ascii() || one.is_none() {
        let ts = tab_size.max(1);
        let mut col = 0u32;
        return text
            .chars()
            .map(|c| {
                let w = if c == '\t' { ts - col % ts } else { 1 };
                col += w;
                w as f32
            })
            .collect();
    }
    let xs = one.map(|l| l.char_xs()).unwrap_or_default();
    xs.windows(2)
        .map(|w| ((w[1] - w[0]) / adv).max(0.0))
        .collect()
}

/// One row of a wrapped line.
#[derive(Clone, Debug)]
pub struct RowPart {
    /// Byte offset of the row's first char in the line.
    pub start: usize,
    pub layout: LineLayout,
}

/// The rows of a wrapped line (cached per line content, style and wrap width).
#[derive(Debug)]
pub struct WrappedRows {
    rows: Vec<RowPart>,
    /// x where rows after the first start (the wrapping indent), in points.
    indent: f32,
}

#[derive(Clone, Debug)]
enum Rows {
    One(LineLayout),
    Many(Arc<WrappedRows>),
}

/// A document line as laid out on screen: one row, or several when wrapped; possibly only a
/// prefix of the line (long lines), followed by a marker.
#[derive(Clone, Debug)]
pub struct VisualLine {
    rows: Rows,
    /// Bytes of the document line (more than the rendered text when the line is cut).
    full_len: usize,
    /// x range of the marker after a cut line's last row (row-relative, indent included).
    marker: Option<(f32, f32)>,
}

impl VisualLine {
    fn new(rows: Rows, full_len: usize, adv: f32) -> Self {
        let mut v = Self {
            rows,
            full_len,
            marker: None,
        };
        if full_len > v.rendered_len() {
            let k = v.n_rows() - 1;
            let end = v.row_x0(k) + v.row_layout(k).width();
            v.marker = Some((end + adv * 0.5, end + adv * 2.5));
        }
        v
    }

    /// One unwrapped row covering the whole line.
    pub fn single(layout: LineLayout) -> Self {
        let n = layout.text().len();
        Self {
            rows: Rows::One(layout),
            full_len: n,
            marker: None,
        }
    }

    pub fn n_rows(&self) -> usize {
        match &self.rows {
            Rows::One(_) => 1,
            Rows::Many(w) => w.rows.len(),
        }
    }

    /// Byte offset where row `k` starts.
    pub fn row_start(&self, k: usize) -> usize {
        match &self.rows {
            Rows::One(_) => 0,
            Rows::Many(w) => w.rows[k].start,
        }
    }

    /// Byte offset where row `k` ends (the next row's start, or the end of the rendered text).
    pub fn row_end(&self, k: usize) -> usize {
        self.row_start(k) + self.row_layout(k).text().len()
    }

    /// x where row `k`'s text starts (the wrapping indent for rows after the first).
    pub fn row_x0(&self, k: usize) -> f32 {
        match &self.rows {
            Rows::Many(w) if k > 0 => w.indent,
            _ => 0.0,
        }
    }

    pub fn row_layout(&self, k: usize) -> &LineLayout {
        match &self.rows {
            Rows::One(l) => l,
            Rows::Many(w) => &w.rows[k].layout,
        }
    }

    /// Bytes laid out (the whole line unless it is cut).
    pub fn rendered_len(&self) -> usize {
        self.row_end(self.n_rows() - 1)
    }

    /// Bytes of the document line.
    pub fn full_len(&self) -> usize {
        self.full_len
    }

    /// The marker's x range after a cut line's last row.
    pub fn marker(&self) -> Option<(f32, f32)> {
        self.marker
    }

    /// x of byte `col` placed on row `k` (clamped to the row; past a cut line's prefix, the
    /// marker's end).
    pub fn x_in_row(&self, k: usize, col: usize) -> f32 {
        let last = k + 1 == self.n_rows();
        if let (true, Some((_, m1))) = (last, self.marker)
            && col > self.rendered_len()
        {
            return m1;
        }
        let (rs, re) = (self.row_start(k), self.row_end(k));
        self.row_x0(k) + self.row_layout(k).x_of(col.clamp(rs, re) - rs)
    }

    /// The row holding byte `col`: at a row boundary the next row's start, or with `upstream`
    /// the previous row's end.
    pub fn row_of(&self, col: usize, upstream: bool) -> usize {
        match &self.rows {
            Rows::One(_) => 0,
            Rows::Many(w) => {
                let k = w.rows.partition_point(|r| r.start <= col).max(1) - 1;
                if upstream && k > 0 && w.rows[k].start == col {
                    k - 1
                } else {
                    k
                }
            }
        }
    }

    /// (row, x) of byte `col`.
    pub fn locate(&self, col: usize, upstream: bool) -> (usize, f32) {
        let k = self.row_of(col, upstream);
        (k, self.x_in_row(k, col))
    }

    /// The byte nearest to `x` on row `k`, and whether it is the end of a row that is not the
    /// last (the caret then belongs at that row's end, not the next row's start).
    pub fn hit(&self, k: usize, x: f32) -> (usize, bool) {
        let k = k.min(self.n_rows() - 1);
        let last = k + 1 == self.n_rows();
        if let (true, Some((m0, m1))) = (last, self.marker)
            && x >= (m0 + m1) * 0.5
        {
            return (self.full_len, false);
        }
        let (rs, re) = (self.row_start(k), self.row_end(k));
        let c = rs + self.row_layout(k).col_at(x - self.row_x0(k));
        if !last && c >= re {
            (re, true)
        } else {
            (c.min(re), false)
        }
    }

    /// x of the end of the last row (where a fold placeholder goes), after a marker.
    pub fn end_x(&self) -> f32 {
        if let Some((_, m1)) = self.marker {
            return m1;
        }
        let k = self.n_rows() - 1;
        self.row_x0(k) + self.row_layout(k).width()
    }

    /// The widest row's end.
    pub fn width(&self) -> f32 {
        (0..self.n_rows())
            .map(|k| self.row_x0(k) + self.row_layout(k).width())
            .fold(self.end_x(), f32::max)
    }
}

/// Lay out the rows of `text` at the breaks for `wrap`. `widths` are per-char columns.
#[allow(clippy::too_many_arguments)]
fn build_rows(
    ctx: &Context,
    mono: Mono,
    tab_size: u32,
    text: &str,
    spans: &[(usize, TextFormat)],
    default: &TextFormat,
    wrap: Wrap,
    widths: &[f32],
) -> WrappedRows {
    let adv = mono.advance();
    let indent = indent_cols(text, tab_size, wrap.cols, wrap.indent);
    let brk = breaks(
        text,
        widths,
        wrap.cols as f32,
        wrap.cols.saturating_sub(indent).max(1) as f32,
    );
    let mut starts = Vec::with_capacity(brk.len() + 1);
    starts.push(0);
    starts.extend(brk);
    let mut rows = Vec::with_capacity(starts.len());
    // Columns before each row start (for its tab stops).
    let mut chars = text.char_indices().zip(widths.iter().copied()).peekable();
    let mut col = 0.0f32;
    for (k, &rs) in starts.iter().enumerate() {
        let re = starts.get(k + 1).copied().unwrap_or(text.len());
        while let Some(&((b, _), w)) = chars.peek() {
            if b >= rs {
                break;
            }
            col += w;
            chars.next();
        }
        let fmt_at = spans
            .iter()
            .take_while(|(s, _)| *s <= rs)
            .last()
            .map_or(default, |(_, f)| f);
        let mut row_spans: Vec<(usize, TextFormat)> = vec![(0, fmt_at.clone())];
        row_spans.extend(
            spans
                .iter()
                .filter(|(s, _)| *s > rs && *s < re)
                .map(|(s, f)| (s - rs, f.clone())),
        );
        let mut tabs = mono.tab_stops(tab_size);
        tabs.origin = col * adv;
        let galley = layout::layout_line_job(ctx, &text[rs..re], &row_spans, default, mono, tabs);
        rows.push(RowPart {
            start: rs,
            layout: LineLayout::new(galley, mono, tabs),
        });
    }
    WrappedRows {
        rows,
        indent: indent as f32 * adv,
    }
}

/// Lay out a document line through the caches: unwrapped (or fitting) lines through the galley
/// cache, wrapped lines through `wraps`. `text` is the rendered part of a line of `full_len`
/// bytes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_line(
    sh: &mut LineShaper<'_>,
    wraps: &mut GalleyCache<Arc<WrappedRows>>,
    text: &str,
    runs: &[(usize, Color32)],
    style_hash: u64,
    default: Color32,
    wrap: Option<Wrap>,
    full_len: usize,
) -> VisualLine {
    let adv = sh.mono.advance();
    let Some(w) = wrap else {
        let l = sh.shape(text, runs, style_hash, default);
        return VisualLine::new(Rows::One(l), full_len, adv);
    };
    // A char is at least one byte and at most two columns per byte... except a tab: a line
    // with no more bytes than columns and no tab fits without measuring.
    let has_tab = text.contains('\t');
    if text.len() <= w.cols as usize && !has_tab {
        let l = sh.shape(text, runs, style_hash, default);
        return VisualLine::new(Rows::One(l), full_len, adv);
    }
    let mut one = None;
    if text.is_ascii() {
        let ts = sh.tab_size.max(1);
        let cols = text.bytes().fold(0u32, |c, b| {
            if b == b'\t' { c + ts - c % ts } else { c + 1 }
        });
        if cols <= w.cols {
            let l = sh.shape(text, runs, style_hash, default);
            return VisualLine::new(Rows::One(l), full_len, adv);
        }
    } else {
        let l = sh.shape(text, runs, style_hash, default);
        if l.width() <= w.cols as f32 * adv + 0.5 {
            return VisualLine::new(Rows::One(l), full_len, adv);
        }
        one = Some(l);
    }
    let key = GalleyKey::new(
        hash_one(text),
        style_hash ^ hash_one(("quire.wrap", w, sh.tab_size)),
        sh.font,
        sh.pixels_per_point,
        w.cols as f32,
    );
    let (ctx, font, mono, ts) = (sh.ctx, sh.font, sh.mono, sh.tab_size);
    let rows = wraps.get_or_insert(key, || {
        let spans: Vec<(usize, TextFormat)> = runs
            .iter()
            .map(|&(s, c)| (s, TextFormat::simple(font.clone(), c)))
            .collect();
        let widths = widths_of(text, ts, one.as_ref(), adv);
        Arc::new(build_rows(
            ctx,
            mono,
            ts,
            text,
            &spans,
            &TextFormat::simple(font.clone(), default),
            w,
            &widths,
        ))
    });
    VisualLine::new(Rows::Many(rows), full_len, adv)
}

/// Lay out `text` with formats `spans` (uncached; the IME preedit line), wrapped when `wrap` is
/// set and the text is wider than a row.
pub(crate) fn shape_uncached(
    ctx: &Context,
    mono: Mono,
    tab_size: u32,
    text: &str,
    spans: &[(usize, TextFormat)],
    default: &TextFormat,
    wrap: Option<Wrap>,
) -> VisualLine {
    let adv = mono.advance();
    let tabs = mono.tab_stops(tab_size);
    let galley = layout::layout_line_job(ctx, text, spans, default, mono, tabs);
    let one = LineLayout::new(galley, mono, tabs);
    let full = text.len();
    match wrap {
        Some(w) if one.width() > w.cols as f32 * adv + 0.5 => {
            let widths = widths_of(text, tab_size, Some(&one), adv);
            let rows = build_rows(ctx, mono, tab_size, text, spans, default, w, &widths);
            VisualLine::new(Rows::Many(Arc::new(rows)), full, adv)
        }
        _ => VisualLine::new(Rows::One(one), full, adv),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, cols: u32, indent: WrappingIndent) -> Vec<&str> {
        let w = widths_of(text, 4, None, 1.0);
        let ind = indent_cols(text, 4, cols, indent);
        let b = breaks(text, &w, cols as f32, (cols - ind) as f32);
        let mut out = Vec::new();
        let mut at = 0;
        for x in b.into_iter().chain([text.len()]) {
            out.push(&text[at..x]);
            at = x;
        }
        out
    }

    #[test]
    fn breaks_after_spaces_and_punctuation() {
        assert_eq!(
            rows("aaa bbb ccc ddd", 8, WrappingIndent::None),
            ["aaa bbb ", "ccc ddd"]
        );
        // Break after a run of break-after chars, before an opening bracket.
        assert_eq!(rows("foo.bar.baz(x)", 9, WrappingIndent::None), [
            "foo.bar.", "baz(x)"
        ]);
        assert_eq!(rows("abc(def)ghi", 5, WrappingIndent::None), [
            "abc", "(def)", "ghi"
        ]);
        // No opportunity: break anywhere.
        assert_eq!(rows("abcdefghij", 4, WrappingIndent::None), [
            "abcd", "efgh", "ij"
        ]);
        // Fits: no break.
        assert_eq!(rows("abcd", 4, WrappingIndent::None), ["abcd"]);
    }

    #[test]
    fn continuation_rows_keep_the_indent() {
        // Indent 4 ("same"): later rows hold 10 - 4 = 6 columns.
        let r = rows("    aaa bbb ccc ddd eee", 10, WrappingIndent::Same);
        assert_eq!(r, ["    aaa ", "bbb ", "ccc ", "ddd ", "eee"]);
        assert_eq!(indent_cols("\tx", 4, 10, WrappingIndent::Indent), 8);
        assert_eq!(indent_cols("\tx", 4, 80, WrappingIndent::DeepIndent), 12);
        // Indent leaves no room: none.
        assert_eq!(indent_cols("        x", 4, 9, WrappingIndent::Same), 0);
        assert_eq!(indent_cols("    ", 4, 80, WrappingIndent::Same), 0);
    }

    #[test]
    fn ideographs_break_anywhere_but_not_before_closing_punctuation() {
        let t = "日本語の文章。です";
        let w: Vec<f32> = t.chars().map(|_| 2.0).collect();
        let b = breaks(t, &w, 8.0, 8.0);
        // Four chars per row; "。" may not start a row, so the row before it gives one up.
        let first = &t[..b[0]];
        assert_eq!(first, "日本語の");
        let second = &t[b[0]..b[1]];
        assert!(!t[b[1]..].starts_with('。'), "{second}");
    }
}
