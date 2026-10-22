//! Line layout: one `LayoutJob` section per token run, cached galleys, and byte ↔ x mapping with
//! a monospace fast path for ASCII lines.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Context, FontId, Galley};

use crate::galley_cache::{GalleyCache, GalleyKey, hash_one};

/// Build the layout job of one line (without its line break). `runs` are `(start_byte, colour)`,
/// sorted by start; bytes before the first run use `default`.
pub fn line_job(
    text: &str,
    runs: &[(usize, Color32)],
    font: &FontId,
    default: Color32,
) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: TextWrapping::no_max_width(),
        break_on_newline: false,
        ..Default::default()
    };
    let fmt = |c: Color32| TextFormat::simple(font.clone(), c);
    let mut at = 0usize;
    let mut color = default;
    for &(start, c) in runs {
        let start = clamp_boundary(text, start);
        if start > at {
            job.append(&text[at..start], 0.0, fmt(color));
            at = start;
        }
        color = c;
    }
    if at < text.len() || job.is_empty() {
        job.append(&text[at..], 0.0, fmt(color));
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

/// A laid-out line and how to map byte columns to x.
#[derive(Clone, Debug)]
pub struct LineLayout {
    pub galley: Arc<Galley>,
    /// Set when the line is ASCII without tabs: column → x is arithmetic (the fast path).
    pub mono: Option<Mono>,
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
}

impl LineLayout {
    /// x offset of byte column `col` from the start of the line.
    pub fn x_of(&self, col: usize) -> f32 {
        let text = self.galley.text();
        let col = clamp_boundary(text, col);
        if let Some(m) = self.mono {
            return m.x_of(col);
        }
        let ci = text[..col].chars().count();
        self.galley
            .pos_from_cursor(egui::text::CCursor::new(ci))
            .min
            .x
    }

    /// Byte column nearest to x.
    pub fn col_at(&self, x: f32) -> usize {
        let text = self.galley.text();
        if let Some(m) = self.mono {
            return m.col_at(x).min(text.len());
        }
        let cc = self
            .galley
            .cursor_from_pos(egui::vec2(x, self.galley.size().y * 0.5));
        let ci: usize = cc.index.0;
        text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
    }

    pub fn text(&self) -> &str {
        self.galley.text()
    }

    pub fn width(&self) -> f32 {
        self.galley.size().x
    }
}

/// Shapes lines through a [`GalleyCache`].
#[derive(Debug)]
pub struct LineShaper<'a> {
    pub cache: &'a mut GalleyCache,
    pub ctx: &'a Context,
    pub font: &'a FontId,
    pub pixels_per_point: f32,
    pub mono: Mono,
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
        let key = GalleyKey::new(
            hash_one(text),
            style_hash,
            self.font,
            self.pixels_per_point,
            f32::INFINITY,
        );
        let galley = self.cache.get_or_insert(key, || {
            let job = line_job(text, runs, self.font, default);
            self.ctx.fonts_mut(|f| f.layout_job(job))
        });
        let mono = (text.is_ascii() && !text.contains('\t')).then_some(self.mono);
        LineLayout { galley, mono }
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
    Mono {
        advance_px: w * ppp / N as f32,
        pixels_per_point: ppp,
    }
}

/// Row height of the font.
pub fn row_height(ctx: &Context, font: &FontId) -> f32 {
    ctx.fonts_mut(|f| f.row_height(font))
}
