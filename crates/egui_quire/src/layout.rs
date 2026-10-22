//! Line layout: one `LayoutJob` section per token run, cached galleys, and byte ↔ x mapping with
//! a monospace fast path for ASCII lines.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Context, FontId, Galley};

use crate::galley_cache::{GalleyCache, GalleyKey, hash_one};

/// Build the layout job of one line (without its line break). `runs` are `(start_byte, colour)`,
/// sorted by start; bytes before the first run use `default`. Tabs keep egui's fixed advance;
/// [`LineShaper::shape`] and [`layout_line_job`] move them to tab stops.
pub fn line_job(
    text: &str,
    runs: &[(usize, Color32)],
    font: &FontId,
    default: Color32,
) -> LayoutJob {
    let spans: Vec<(usize, TextFormat)> = runs
        .iter()
        .map(|&(s, c)| (s, TextFormat::simple(font.clone(), c)))
        .collect();
    build_job(
        text,
        &spans,
        &TextFormat::simple(font.clone(), default),
        &[],
    )
}

/// A layout job for `text` with formats `spans` (`(start_byte, format)`, sorted) and, after the
/// `i`-th tab, `tab_extra[i]` points of extra leading space (missing entries are 0).
pub(crate) fn build_job(
    text: &str,
    spans: &[(usize, TextFormat)],
    default: &TextFormat,
    tab_extra: &[f32],
) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: TextWrapping::no_max_width(),
        break_on_newline: false,
        ..Default::default()
    };
    let mut tab_i = 0usize;
    let mut pending = 0.0f32;
    let mut push = |job: &mut LayoutJob, seg: &str, fmt: &TextFormat| {
        // Split after each tab so the text after it can carry that tab's extra space.
        let mut rest = seg;
        while !rest.is_empty() {
            let cut = rest.find('\t').map_or(rest.len(), |t| t + 1);
            job.append(&rest[..cut], std::mem::take(&mut pending), fmt.clone());
            if rest.as_bytes()[cut - 1] == b'\t' {
                pending = tab_extra.get(tab_i).copied().unwrap_or(0.0);
                tab_i += 1;
            }
            rest = &rest[cut..];
        }
    };
    let mut at = 0usize;
    let mut fmt = default;
    for (start, f) in spans {
        let start = clamp_boundary(text, *start);
        if start > at {
            push(&mut job, &text[at..start], fmt);
            at = start;
        }
        fmt = f;
    }
    if at < text.len() || job.is_empty() {
        push(&mut job, &text[at..], fmt);
        if job.is_empty() {
            job.append("", 0.0, fmt.clone());
        }
    }
    job
}

fn clamp_boundary(text: &str, mut b: usize) -> usize {
    b = b.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// Tab stops: every `tab_size` columns of the font's space advance (VS Code's `tabSize`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabStops {
    /// Columns per tab stop (≥ 1).
    pub(crate) tab_size: u32,
    /// Width of one column (the space advance) in points.
    pub(crate) column: f32,
    /// egui's own advance of a tab character in points (`FontTweak::tab_size` spaces).
    pub(crate) egui_tab: f32,
    /// x (points, in the unwrapped line) where this text starts: 0 for a whole line, the row's
    /// start for a wrapped row, so its tabs still stop at the line's tab stops.
    pub(crate) origin: f32,
}

impl TabStops {
    /// Distance between stops in points.
    pub(crate) fn stop(&self) -> f32 {
        self.tab_size.max(1) as f32 * self.column
    }

    /// x where a tab that starts at `x` ends: the next stop strictly after `x`. `x` may be off by
    /// a fraction of a pixel (glyph origins are snapped), so a tab starting within a quarter
    /// column below a stop is treated as starting on it.
    pub(crate) fn next_stop(&self, x: f32) -> f32 {
        let stop = self.stop();
        ((x / stop + 0.25 / self.tab_size.max(1) as f32).floor() + 1.0) * stop
    }

    /// Visual column where the text starts (the fast path counts columns from it).
    pub(crate) fn origin_col(&self) -> u32 {
        if self.column > 0.0 {
            (self.origin / self.column).round() as u32
        } else {
            0
        }
    }

    /// Like [`Self::next_stop`] for a tab at text-relative `x`, returning text-relative x.
    pub(crate) fn next_stop_rel(&self, x: f32) -> f32 {
        self.next_stop(x + self.origin) - self.origin
    }
}

/// Lay out `text` with formats `spans`, moving tabs to tab stops: tabs are split into their own
/// sections and the text after each gets the leading space that moves it to the next stop. ASCII
/// lines compute the stops from columns; other lines are laid out once to find where each tab
/// starts, then again with the corrections.
pub(crate) fn layout_line_job(
    ctx: &Context,
    text: &str,
    spans: &[(usize, TextFormat)],
    default: &TextFormat,
    mono: Mono,
    tabs: TabStops,
) -> Arc<Galley> {
    let n_tabs = text.bytes().filter(|&b| b == b'\t').count();
    if n_tabs == 0 {
        return ctx.fonts_mut(|f| f.layout_job(build_job(text, spans, default, &[])));
    }
    let mut extra = Vec::with_capacity(n_tabs);
    if text.is_ascii() {
        let adv = mono.advance();
        let mut col = tabs.origin_col();
        let ts = tabs.tab_size.max(1);
        for b in text.bytes() {
            if b == b'\t' {
                let w = ts - col % ts;
                extra.push(w as f32 * adv - tabs.egui_tab);
                col += w;
            } else {
                col += 1;
            }
        }
    } else {
        let first = ctx.fonts_mut(|f| f.layout_job(build_job(text, spans, default, &[])));
        let mut shift = 0.0f32;
        if let Some(row) = first.rows.first() {
            for g in row.glyphs.iter().filter(|g| g.chr == '\t') {
                let x0 = g.pos.x + shift;
                let e = tabs.next_stop_rel(x0) - x0 - tabs.egui_tab;
                extra.push(e);
                shift += e;
            }
        }
    }
    ctx.fonts_mut(|f| f.layout_job(build_job(text, spans, default, &extra)))
}

/// A laid-out line and how to map byte columns to x.
#[derive(Clone, Debug)]
pub struct LineLayout {
    pub galley: Arc<Galley>,
    /// Set when the line is ASCII: column → x is arithmetic (the fast path), tabs included.
    pub mono: Option<Mono>,
    /// Tab stops the line was laid out with.
    pub(crate) tabs: TabStops,
    /// x of the end of the line (after a trailing tab's stop).
    end_x: f32,
}

/// Monospace metrics for the fast path: column → x is `col × advance` (the unrounded pen
/// position). egui 0.37 snaps each glyph's origin to a physical pixel (with subpixel bins), so
/// glyph origins differ from this by less than one physical pixel; carets and hit tests do not
/// need more.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mono {
    /// Advance of one character in physical pixels (unrounded).
    pub advance_px: f32,
    pub pixels_per_point: f32,
    /// egui's advance of a tab character in physical pixels.
    pub tab_px: f32,
}

impl Mono {
    pub fn x_of(&self, col: usize) -> f32 {
        col as f32 * self.advance_px / self.pixels_per_point
    }

    /// The column whose x is nearest to `x`.
    pub fn col_at(&self, x: f32) -> usize {
        (x * self.pixels_per_point / self.advance_px)
            .round()
            .max(0.0) as usize
    }

    /// Advance in points.
    pub fn advance(&self) -> f32 {
        self.advance_px / self.pixels_per_point
    }

    /// Tab stops every `tab_size` columns of this font.
    pub(crate) fn tab_stops(&self, tab_size: u32) -> TabStops {
        TabStops {
            tab_size: tab_size.max(1),
            column: self.advance(),
            egui_tab: self.tab_px / self.pixels_per_point,
            origin: 0.0,
        }
    }
}

impl LineLayout {
    /// Wrap a galley laid out by [`layout_line_job`] with `tabs`; `mono` is used only for ASCII
    /// text.
    pub(crate) fn new(galley: Arc<Galley>, mono: Mono, tabs: TabStops) -> Self {
        let mono = galley.text().is_ascii().then_some(mono);
        let end_x = match galley.rows.first().and_then(|r| r.glyphs.last()) {
            Some(g) if g.chr == '\t' => tabs.next_stop_rel(g.pos.x),
            Some(g) => g.pos.x + g.advance_width,
            None => 0.0,
        };
        let mut s = Self {
            galley,
            mono,
            tabs,
            end_x,
        };
        if let Some(m) = s.mono {
            s.end_x = s.mono_x(m, s.galley.text().len());
        }
        s
    }

    /// The same galley mapped through glyph positions only (no fast path); tests compare the two.
    pub fn without_fast_path(&self) -> Self {
        let mut s = self.clone();
        s.mono = None;
        s.end_x = match s.galley.rows.first().and_then(|r| r.glyphs.last()) {
            Some(g) if g.chr == '\t' => s.tabs.next_stop_rel(g.pos.x),
            Some(g) => g.pos.x + g.advance_width,
            None => 0.0,
        };
        s
    }

    /// Fast path: x of byte column `col` of an ASCII line, tabs at stops.
    fn mono_x(&self, m: Mono, col: usize) -> f32 {
        let text = self.galley.text();
        let col = col.min(text.len());
        if !text.as_bytes()[..col].contains(&b'\t') {
            return m.x_of(col);
        }
        let o = self.tabs.origin_col();
        m.x_of((visual_col(o, &text.as_bytes()[..col], self.tabs.tab_size) - o) as usize)
    }

    /// x offset of byte column `col` from the start of the line.
    pub fn x_of(&self, col: usize) -> f32 {
        let text = self.galley.text();
        let col = clamp_boundary(text, col);
        if let Some(m) = self.mono {
            return self.mono_x(m, col);
        }
        let ci = text[..col].chars().count();
        match self.galley.rows.first().and_then(|r| r.glyphs.get(ci)) {
            Some(g) => g.pos.x,
            None => self.end_x,
        }
    }

    /// Byte column nearest to x.
    pub fn col_at(&self, x: f32) -> usize {
        let text = self.galley.text();
        if let Some(m) = self.mono
            && !text.contains('\t')
        {
            return m.col_at(x).min(text.len());
        }
        // The column is the first character whose midpoint lies right of x.
        let xs = self.char_xs();
        for ((b, _), w) in text.char_indices().zip(xs.windows(2)) {
            if x < (w[0] + w[1]) * 0.5 {
                return b;
            }
        }
        text.len()
    }

    /// x of each character's start and of the line end (`chars + 1` entries), for hit tests
    /// and accessibility.
    pub(crate) fn char_xs(&self) -> Vec<f32> {
        let text = self.galley.text();
        let mut v = Vec::with_capacity(text.len() + 1);
        if let Some(m) = self.mono {
            let ts = self.tabs.tab_size.max(1) as usize;
            let o = self.tabs.origin_col() as usize;
            let mut col = o;
            for b in text.bytes() {
                v.push(m.x_of(col - o));
                col += if b == b'\t' { ts - col % ts } else { 1 };
            }
        } else {
            let glyphs = self.galley.rows.first().map_or(&[][..], |r| &r.glyphs[..]);
            for (i, _) in text.chars().enumerate() {
                v.push(glyphs.get(i).map_or(self.end_x, |g| g.pos.x));
            }
        }
        v.push(self.end_x);
        v
    }

    pub fn text(&self) -> &str {
        self.galley.text()
    }

    /// Width of the line (to the end of a trailing tab).
    pub fn width(&self) -> f32 {
        self.end_x
    }
}

/// Visible column after `bytes` with tabs at stops every `tab_size` columns, one column per
/// other byte (callers pass ASCII).
fn visual_col(start: u32, bytes: &[u8], tab_size: u32) -> u32 {
    let ts = tab_size.max(1);
    bytes.iter().fold(start, |col, &b| {
        if b == b'\t' {
            col + ts - col % ts
        } else {
            col + 1
        }
    })
}

/// Shapes lines through a [`GalleyCache`].
#[derive(Debug)]
pub struct LineShaper<'a> {
    pub cache: &'a mut GalleyCache,
    pub ctx: &'a Context,
    pub font: &'a FontId,
    pub pixels_per_point: f32,
    pub mono: Mono,
    /// Columns per tab stop (`EditorConfig::tab_size`).
    pub tab_size: u32,
}

impl LineShaper<'_> {
    /// Lay out `text` with colour runs; `style_hash` must change whenever `runs` or the theme do.
    pub fn shape(
        &mut self,
        text: &str,
        runs: &[(usize, Color32)],
        style_hash: u64,
        default: Color32,
    ) -> LineLayout {
        let tabs = self.mono.tab_stops(self.tab_size);
        let has_tab = text.contains('\t');
        let key = GalleyKey::new(
            hash_one(text),
            if has_tab {
                style_hash ^ hash_one(("tabs", tabs.tab_size))
            } else {
                style_hash
            },
            self.font,
            self.pixels_per_point,
            f32::INFINITY,
        );
        let (ctx, font, mono) = (self.ctx, self.font, self.mono);
        let galley = self.cache.get_or_insert(key, || {
            let spans: Vec<(usize, TextFormat)> = runs
                .iter()
                .map(|&(s, c)| (s, TextFormat::simple(font.clone(), c)))
                .collect();
            layout_line_job(
                ctx,
                text,
                &spans,
                &TextFormat::simple(font.clone(), default),
                mono,
                tabs,
            )
        });
        LineLayout::new(galley, mono, tabs)
    }
}

/// Monospace metrics of `font` at the context's current scale, measured from a laid-out run
/// (the galley width is the unrounded cursor position).
pub fn mono(ctx: &Context, font: &FontId) -> Mono {
    const N: usize = 256;
    let ppp = ctx.pixels_per_point();
    let w = ctx.fonts_mut(|f| {
        f.layout_no_wrap("M".repeat(N), font.clone(), Color32::WHITE)
            .size()
            .x
    });
    // A run of tabs ending in a visible glyph: that glyph starts after N tab advances.
    let tab = ctx.fonts_mut(|f| {
        let g = f.layout_no_wrap("\t".repeat(N) + "M", font.clone(), Color32::WHITE);
        g.rows
            .first()
            .and_then(|r| r.glyphs.last())
            .map_or(w / N as f32 * 4.0 * N as f32, |gl| gl.pos.x)
    });
    Mono {
        advance_px: w * ppp / N as f32,
        pixels_per_point: ppp,
        tab_px: tab * ppp / N as f32,
    }
}

/// Row height of the font.
pub fn row_height(ctx: &Context, font: &FontId) -> f32 {
    ctx.fonts_mut(|f| f.row_height(font))
}
