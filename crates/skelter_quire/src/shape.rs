//! Line shaping: one logical line into glyph runs and the x of every char boundary (svelters
//! `plans/research/skelter/notes/round2/text-stack.md` §5.2).
//!
//! Two paths:
//! - **ASCII monospace fast path.** A line of printable ASCII and tabs in a monospace editor font
//!   needs no shaper: glyph ids and the advance come from a table of the 95 printable characters
//!   ([`MonoTable`]), x = column × advance with tabs to the line's stops. Spaces and tabs draw no
//!   glyph. One run per paint id, so a token colour is one `Glyphs` primitive.
//! - **General path.** skelter-text's [`TextSystem`] lays the whole line out unwrapped, with one span
//!   per token whose paint id is the token type (colour-only spans, so token colours never split
//!   shaping); char boundary x come from the layout's caret geometry.
//!
//! Paint ids: 0 is the foreground, `t + 1` is token type `t` ([`crate::EditorTheme::paint_color`]).
//! Coordinates are points, x from the line's start, y from the baseline.

use std::ops::Range;
use std::sync::Arc;

use skelter_base::geom::Point;
use skelter_base::ids::FontId;
use skelter_base::text::{AaMode, Glyph, GlyphRun, HintMode};
use skelter_text::{RichText, Span, TextStyle, TextSystem};

use crate::cache::{CacheBytes, GenCache, ShapeKey, hash_one};

/// Lines longer than this (bytes) take char boundary x from their runs' advances spread evenly over
/// the run's chars instead of one caret query per char (which is linear in the line).
pub const EXACT_STOPS_MAX: usize = 4096;

/// One glyph run of a shaped line, drawn at `x` on the line's baseline in its paint id's colour.
#[derive(Clone, Debug, PartialEq)]
pub struct LineRun {
    pub run: Arc<GlyphRun>,
    pub x: f32,
    pub paint: u32,
    /// Line-local byte range the run draws.
    pub range: Range<usize>,
}

/// A shaped logical line.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedLine {
    pub runs: Vec<LineRun>,
    /// `(byte, x)` at every char boundary, ascending by byte, the line's end included.
    pub stops: Vec<(u32, f32)>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Shaped by the ASCII monospace fast path.
    pub fast: bool,
}

impl CacheBytes for Arc<ShapedLine> {
    fn cache_bytes(&self) -> usize {
        let glyphs: usize = self
            .runs
            .iter()
            .map(|r| r.run.glyphs.len() * std::mem::size_of::<Glyph>())
            .sum();
        glyphs + self.stops.len() * 8 + self.runs.len() * 64 + 64
    }
}

impl ShapedLine {
    /// Line-local text length in bytes.
    pub fn len(&self) -> usize {
        self.stops.last().map_or(0, |s| s.0 as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The x of `byte` (rounded down to a char boundary; clamped to the end).
    pub fn x_of(&self, byte: usize) -> f32 {
        let b = byte as u32;
        match self.stops.binary_search_by_key(&b, |s| s.0) {
            Ok(i) => self.stops[i].1,
            Err(0) => 0.0,
            Err(i) => self.stops[i - 1].1,
        }
    }

    /// The char boundary nearest to `x`.
    pub fn byte_at(&self, x: f32) -> usize {
        let i = self.stops.partition_point(|s| s.1 < x);
        if i == 0 {
            return self.stops.first().map_or(0, |s| s.0 as usize);
        }
        if i >= self.stops.len() {
            return self.len();
        }
        let (a, b) = (self.stops[i - 1], self.stops[i]);
        if x - a.1 <= b.1 - x {
            a.0 as usize
        } else {
            b.0 as usize
        }
    }

    /// Per-char advances (points), in char order, for wrapping.
    pub fn advances(&self) -> Vec<f32> {
        self.stops
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).max(0.0))
            .collect()
    }
}

/// The 95 printable ASCII glyphs of a monospace font at one size.
#[derive(Clone, Debug, PartialEq)]
pub struct MonoTable {
    pub font: FontId,
    pub size: f32,
    pub variations: u64,
    /// Glyph id of `' ' + i`.
    pub glyphs: [u32; 95],
    pub advance: f32,
    pub ascent: f32,
    pub descent: f32,
    pub hint: HintMode,
    pub aa: AaMode,
}

impl MonoTable {
    /// Probes `style`'s font by laying out the printable ASCII characters: a table when they shape
    /// to one run of 95 glyphs, one per char, all with the same advance (a monospace font without
    /// ligatures among them); `None` otherwise (the general path then serves every line).
    pub fn probe(text: &mut TextSystem, style: &TextStyle) -> Option<MonoTable> {
        let probe: String = (b' '..=b'~').map(char::from).collect();
        let mut st = style.clone();
        st.wrap = false;
        let layout = text.layout(&RichText::plain(probe, st), None);
        let [run] = layout.runs() else { return None };
        let g = &run.run.glyphs;
        if g.len() != 95 || run.range != (0..95) {
            return None;
        }
        let advance = run.run.advance / 95.0;
        if advance <= 0.0 {
            return None;
        }
        for (i, gl) in g.iter().enumerate() {
            if (gl.x - i as f32 * advance).abs() > 0.01 || gl.y != 0.0 {
                return None;
            }
        }
        let mut glyphs = [0u32; 95];
        for (i, gl) in g.iter().enumerate() {
            glyphs[i] = gl.id;
        }
        let r = &run.run;
        Some(MonoTable {
            font: r.font,
            size: r.size,
            variations: r.variations,
            glyphs,
            advance,
            ascent: r.ascent,
            descent: r.descent,
            hint: r.hint,
            aa: r.aa,
        })
    }

    /// Whether `line` can take the fast path: printable ASCII and tabs only.
    pub fn covers(line: &str) -> bool {
        line.bytes()
            .all(|b| b == b'\t' || (b' '..=b'~').contains(&b))
    }

    /// Shapes an ASCII `line` (see [`MonoTable::covers`]) with token `paints` (`(start byte, paint)`
    /// ascending; text before the first is paint 0).
    pub fn shape(&self, line: &str, paints: &[(usize, u32)], tab_size: u32) -> ShapedLine {
        let ts = tab_size.max(1);
        let bytes = line.as_bytes();
        let mut stops = Vec::with_capacity(bytes.len() + 1);
        let mut col = 0u32;
        let mut runs = Vec::new();
        // (paint, first byte, glyphs, run x, end byte)
        let mut cur: Option<(u32, usize, Vec<Glyph>, f32, usize)> = None;
        let paint_at = |b: usize| -> u32 {
            let i = paints.partition_point(|p| p.0 <= b);
            if i == 0 { 0 } else { paints[i - 1].1 }
        };
        let flush = |cur: &mut Option<(u32, usize, Vec<Glyph>, f32, usize)>,
                     runs: &mut Vec<LineRun>| {
            if let Some((paint, start, glyphs, x0, end)) = cur.take()
                && !glyphs.is_empty()
            {
                let advance = glyphs.last().map_or(0.0, |g| g.x + self.advance);
                runs.push(LineRun {
                    run: Arc::new(self.run(glyphs, advance)),
                    x: x0,
                    paint,
                    range: start..end,
                });
            }
        };
        for (i, &b) in bytes.iter().enumerate() {
            let x = col as f32 * self.advance;
            stops.push((i as u32, x));
            if b == b'\t' {
                col += ts - col % ts;
                continue;
            }
            col += 1;
            if b == b' ' {
                continue;
            }
            let paint = paint_at(i);
            if cur.as_ref().is_some_and(|c| c.0 != paint) {
                flush(&mut cur, &mut runs);
            }
            let c = cur.get_or_insert_with(|| (paint, i, Vec::new(), x, i));
            c.2.push(Glyph {
                id: self.glyphs[(b - b' ') as usize],
                x: x - c.3,
                y: 0.0,
            });
            c.4 = i + 1;
        }
        flush(&mut cur, &mut runs);
        let width = col as f32 * self.advance;
        stops.push((bytes.len() as u32, width));
        ShapedLine {
            runs,
            stops,
            width,
            ascent: self.ascent,
            descent: self.descent,
            fast: true,
        }
    }

    fn run(&self, glyphs: Vec<Glyph>, advance: f32) -> GlyphRun {
        GlyphRun {
            font: self.font,
            size: self.size,
            variations: self.variations,
            glyphs,
            ascent: self.ascent,
            descent: self.descent,
            advance,
            hint: self.hint,
            aa: self.aa,
            embolden: 0.0,
            skew_deg: 0.0,
        }
    }
}

/// Shapes lines through the fast path or skelter-text, with the two-generation line cache.
pub struct LineShaper {
    text: TextSystem,
    style: TextStyle,
    mono: Option<MonoTable>,
    scale: f32,
    tab_size: u32,
    font_generation: u64,
    cache: GenCache<ShapeKey, Arc<ShapedLine>>,
}

impl std::fmt::Debug for LineShaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineShaper")
            .field("style", &self.style)
            .field("mono", &self.mono.is_some())
            .field("scale", &self.scale)
            .field("cache", &self.cache)
            .finish()
    }
}

/// The byte budget of a shaper's line cache (lines scrolled away go first).
pub const LINE_CACHE_BUDGET: usize = 16 << 20;

impl LineShaper {
    pub fn new(fonts: Arc<skelter_text::FontStore>, family: &str, size: f32) -> Self {
        let mut text = TextSystem::new(fonts);
        let style = TextStyle {
            family: family.to_owned(),
            size,
            wrap: false,
            ..TextStyle::default()
        };
        let mono = MonoTable::probe(&mut text, &style);
        let font_generation = text.fonts().generation();
        LineShaper {
            text,
            style,
            mono,
            scale: 1.0,
            tab_size: 4,
            font_generation,
            cache: GenCache::new(LINE_CACHE_BUDGET),
        }
    }

    /// The editor font changed (family or size).
    pub fn set_font(&mut self, family: &str, size: f32) {
        if self.style.family == family && self.style.size == size {
            return;
        }
        self.style.family = family.to_owned();
        self.style.size = size;
        self.mono = MonoTable::probe(&mut self.text, &self.style);
        self.cache.clear();
    }

    /// Device pixels per point (part of the cache key: glyph sizes are per ppem).
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    pub fn set_tab_size(&mut self, tab_size: u32) {
        if self.tab_size != tab_size {
            self.tab_size = tab_size;
            self.cache.clear();
        }
    }

    pub fn tab_size(&self) -> u32 {
        self.tab_size
    }

    /// The fast path's table, when the editor font is monospace.
    pub fn mono(&self) -> Option<&MonoTable> {
        self.mono.as_ref()
    }

    /// The width of one column: the monospace advance, or the width of a space.
    pub fn column_width(&mut self) -> f32 {
        if let Some(m) = &self.mono {
            return m.advance;
        }
        let l = self
            .text
            .layout(&RichText::plain("0000000000", self.style.clone()), None);
        l.size().w / 10.0
    }

    /// (ascent, descent) of the editor font.
    pub fn metrics(&mut self) -> (f32, f32) {
        if let Some(m) = &self.mono {
            return (m.ascent, m.descent);
        }
        let l = self
            .text
            .layout(&RichText::plain("Hg", self.style.clone()), None);
        l.runs()
            .first()
            .map_or((self.style.size * 0.8, self.style.size * 0.2), |r| {
                (r.run.ascent, r.run.descent)
            })
    }

    /// (hits, misses) of the line cache; entries held.
    pub fn stats(&self) -> ((u64, u64), usize) {
        (self.cache.stats(), self.cache.len())
    }

    /// Ends a frame of both caches (skelter-text's and the line cache).
    pub fn end_frame(&mut self) {
        self.text.end_frame();
        self.cache.end_frame();
    }

    fn check_fonts(&mut self) {
        let g = self.text.fonts().generation();
        if g != self.font_generation {
            self.font_generation = g;
            self.mono = MonoTable::probe(&mut self.text, &self.style);
            self.cache.clear();
        }
    }

    /// The cache key of `line` with `paints`.
    pub fn key(&self, line: &str, paints: &[(usize, u32)]) -> ShapeKey {
        ShapeKey {
            content: hash_one(line),
            runs: hash_one(paints),
            font_generation: self.font_generation,
            ppem_bits: (self.style.size * self.scale).to_bits(),
            features: hash_one((&self.style.features, &self.style.variations, self.tab_size)),
        }
    }

    /// Shapes `line` (no line feed) with token `paints`, through the cache.
    pub fn shape(&mut self, line: &str, paints: &[(usize, u32)]) -> Arc<ShapedLine> {
        self.check_fonts();
        let key = self.key(line, paints);
        if let Some(s) = self.cache.get(&key) {
            return s;
        }
        let shaped = Arc::new(self.shape_uncached(line, paints));
        self.cache.insert(key, shaped.clone());
        shaped
    }

    /// Shapes without the cache (for tests and one-off text such as popups).
    pub fn shape_uncached(&mut self, line: &str, paints: &[(usize, u32)]) -> ShapedLine {
        if let Some(m) = &self.mono
            && MonoTable::covers(line)
        {
            return m.shape(line, paints, self.tab_size);
        }
        self.shape_general(line, paints)
    }

    fn shape_general(&mut self, line: &str, paints: &[(usize, u32)]) -> ShapedLine {
        let mut rich = RichText::plain(line, self.style.clone());
        rich.tab_size = Some(self.tab_size as f32);
        for (i, &(start, paint)) in paints.iter().enumerate() {
            let end = paints
                .get(i + 1)
                .map_or(line.len(), |p| p.0)
                .min(line.len());
            let start = floor_boundary(line, start.min(line.len()));
            let end = floor_boundary(line, end);
            if start < end && paint != 0 {
                rich.spans.push(Span {
                    range: start..end,
                    style: TextStyle {
                        paint,
                        ..self.style.clone()
                    },
                });
            }
        }
        let layout = self.text.layout(&rich, None);
        let baseline = layout.first_baseline();
        let mut runs = Vec::with_capacity(layout.runs().len());
        let (mut ascent, mut descent) = (0.0f32, 0.0f32);
        for r in layout.runs() {
            ascent = ascent.max(r.run.ascent);
            descent = descent.max(r.run.descent);
            debug_assert!((r.origin.y - baseline).abs() < 0.5, "one unwrapped line");
            runs.push(LineRun {
                run: r.run.clone(),
                x: r.origin.x,
                paint: r.style,
                range: r.range.clone(),
            });
        }
        if runs.is_empty() {
            let (a, d) = self.metrics();
            ascent = a;
            descent = d;
        }
        let stops = if line.len() <= EXACT_STOPS_MAX {
            line.char_indices()
                .map(|(b, _)| b)
                .chain(std::iter::once(line.len()))
                .map(|b| (b as u32, layout.caret_rect(b).min.x))
                .collect()
        } else {
            spread_stops(line, &runs)
        };
        ShapedLine {
            runs,
            stops,
            width: layout.size().w,
            ascent,
            descent,
            fast: false,
        }
    }
}

/// Char boundary x of a long line from its runs: each run's advance spread evenly over its chars
/// (exact for monospace text; an estimate otherwise, see [`EXACT_STOPS_MAX`]).
fn spread_stops(line: &str, runs: &[LineRun]) -> Vec<(u32, f32)> {
    let mut out = Vec::with_capacity(line.len() / 2 + 1);
    let mut sorted: Vec<&LineRun> = runs.iter().collect();
    sorted.sort_by_key(|r| r.range.start);
    let mut x_end = 0.0f32;
    let mut b = 0usize;
    for r in sorted {
        let start = floor_boundary(line, r.range.start.min(line.len())).max(b);
        let end = floor_boundary(line, r.range.end.min(line.len()));
        // a gap between runs (unshaped whitespace) keeps the previous x
        for (i, _) in line[b..start].char_indices() {
            out.push(((b + i) as u32, x_end));
        }
        if start >= end {
            b = start;
            continue;
        }
        let n = line[start..end].chars().count().max(1) as f32;
        for (k, (i, _)) in line[start..end].char_indices().enumerate() {
            out.push(((start + i) as u32, r.x + r.run.advance * k as f32 / n));
        }
        x_end = r.x + r.run.advance;
        b = end;
    }
    for (i, _) in line[b..].char_indices() {
        out.push(((b + i) as u32, x_end));
    }
    out.push((line.len() as u32, x_end));
    out
}

fn floor_boundary(s: &str, mut b: usize) -> usize {
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// A point on a shaped line's baseline, for callers placing runs.
pub fn run_origin(run: &LineRun, baseline: Point) -> Point {
    Point::new(baseline.x + run.x, baseline.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> MonoTable {
        let mut glyphs = [0u32; 95];
        for (i, g) in glyphs.iter_mut().enumerate() {
            *g = 100 + i as u32;
        }
        MonoTable {
            font: FontId(1),
            size: 13.0,
            variations: 0,
            glyphs,
            advance: 8.0,
            ascent: 10.0,
            descent: 3.0,
            hint: HintMode::Vertical,
            aa: AaMode::Grayscale,
        }
    }

    #[test]
    fn fast_path_places_columns_and_tab_stops() {
        let s = table().shape("a\tb c", &[], 4);
        // a at col 0, tab to col 4, b at 4, space at 5, c at 6
        let xs: Vec<f32> = s.stops.iter().map(|s| s.1).collect();
        assert_eq!(xs, vec![0.0, 8.0, 32.0, 40.0, 48.0, 56.0]);
        assert_eq!(s.width, 56.0);
        assert_eq!(s.runs.len(), 1, "one paint, one run");
        let run = &s.runs[0].run;
        let ids: Vec<u32> = run.glyphs.iter().map(|g| g.id).collect();
        assert_eq!(
            ids,
            vec![
                100 + (b'a' - b' ') as u32,
                100 + (b'b' - b' ') as u32,
                100 + (b'c' - b' ') as u32
            ]
        );
        let gx: Vec<f32> = run.glyphs.iter().map(|g| g.x).collect();
        assert_eq!(gx, vec![0.0, 32.0, 48.0], "spaces and tabs draw nothing");
    }

    #[test]
    fn fast_path_splits_runs_by_paint() {
        let s = table().shape("let x", &[(0, 5), (3, 0), (4, 7)], 4);
        let paints: Vec<(u32, Range<usize>, f32)> = s
            .runs
            .iter()
            .map(|r| (r.paint, r.range.clone(), r.x))
            .collect();
        assert_eq!(paints, vec![(5, 0..3, 0.0), (7, 4..5, 32.0)]);
        assert_eq!(
            s.runs[1].run.glyphs[0].x, 0.0,
            "glyphs are relative to their run"
        );
    }

    #[test]
    fn x_and_byte_round_trip() {
        let s = table().shape("abcd", &[], 4);
        assert_eq!(s.x_of(2), 16.0);
        assert_eq!(s.byte_at(17.0), 2);
        assert_eq!(s.byte_at(21.0), 3, "nearer the next boundary");
        assert_eq!(s.byte_at(-5.0), 0);
        assert_eq!(s.byte_at(500.0), 4);
        assert_eq!(s.advances(), vec![8.0; 4]);
    }

    #[test]
    fn covers_only_printable_ascii_and_tabs() {
        assert!(MonoTable::covers("fn main() {\t}"));
        assert!(!MonoTable::covers("é"));
        assert!(!MonoTable::covers("a\u{7f}"));
    }

    #[test]
    fn spread_stops_cover_every_char_boundary() {
        let run = |range: Range<usize>, x: f32, advance: f32| LineRun {
            run: Arc::new(table().run(Vec::new(), advance)),
            x,
            paint: 0,
            range,
        };
        let line = "ab cé";
        let stops = spread_stops(line, &[run(0..2, 0.0, 20.0), run(3..6, 30.0, 20.0)]);
        let bytes: Vec<u32> = stops.iter().map(|s| s.0).collect();
        assert_eq!(bytes, vec![0, 1, 2, 3, 4, 6]);
        assert_eq!(stops[1].1, 10.0);
        assert_eq!(stops[2].1, 20.0, "the gap keeps the previous run's end");
        assert_eq!(stops[4].1, 40.0);
        assert_eq!(stops[5].1, 50.0);
    }

    #[test]
    fn general_path_shapes_with_the_bundled_fonts() {
        let fonts = skelter_text::FontStore::new(skelter_text::FontConfig::testing());
        let mut sh = LineShaper::new(fonts, skelter_text::test_fonts::SANS_FAMILY, 13.0);
        assert!(sh.mono().is_none(), "Noto Sans is proportional");
        let s = sh.shape("héllo wörld", &[(0, 3), (6, 4)]);
        assert!(!s.fast);
        assert!(s.width > 0.0);
        assert_eq!(s.len(), "héllo wörld".len());
        assert!(
            s.stops
                .windows(2)
                .all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1),
            "{:?}",
            s.stops
        );
        assert!(s.runs.iter().any(|r| r.paint == 3) && s.runs.iter().any(|r| r.paint == 4));
        let again = sh.shape("héllo wörld", &[(0, 3), (6, 4)]);
        assert!(Arc::ptr_eq(&s, &again), "the second shape is a cache hit");
    }
}
