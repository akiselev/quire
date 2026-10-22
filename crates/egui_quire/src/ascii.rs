//! E1 P2: an ASCII line-shaping fast path.
//!
//! egui 0.37 shapes every line through harfrust (`Fonts::layout_job`), about 45 µs and 50
//! allocations per newly shown line. A line of printable ASCII and tabs in the editor's monospace
//! font, with colour-only formats, needs no shaping: each char sits at `column × advance`. This
//! module builds such a line's [`Galley`] directly from glyph templates.
//!
//! The templates come from one ordinary layout of every printable ASCII char at each of egui's
//! four subpixel bins (`epaint::text::glyph_atlas::SubpixelBin`: a glyph's physical x is its pen
//! position truncated, with the fraction binned at quarter pixels). A glyph placed at pen
//! position `h` takes the template of `h`'s bin, moved to `h`'s pixel; its quad is tessellated as
//! `tessellate_glyphs` does. The result matches `layout_job`'s galley glyph for glyph (a test
//! compares them), so carets, hit tests and snapshots do not change.
//!
//! The templates hold atlas coordinates. Like every cached galley (ours and egui's), they are
//! valid while the font atlas is; they are rebuilt when the font, the scale or the font
//! definitions change (the key includes the measured advance and row height).

use std::sync::Arc;

use egui::epaint::Mesh;
use egui::epaint::text::{Glyph, Row, RowVisuals};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Context, FontId, Galley, Id, Rect, pos2, vec2};

use crate::galley_cache::hash_one;
use crate::layout::{Mono, TabStops};

/// First and last printable ASCII bytes.
const FIRST: u8 = 0x20;
const LAST: u8 = 0x7e;
const N: usize = (LAST - FIRST + 1) as usize;

/// Glyph templates of one font at one scale.
#[derive(Debug)]
pub(crate) struct Templates {
    key: u64,
    /// `glyphs[(b - FIRST) * 4 + bin]`, glyph x at its template pixel `px[...]`.
    glyphs: Vec<Glyph>,
    px: Vec<i32>,
    /// Whether the font bins subpixel positions (else positions round to whole pixels).
    binning: bool,
    /// A one-row galley and its row, cloned for each line (they carry egui's crate-private
    /// fields).
    galley: Arc<Galley>,
    row: Arc<Row>,
    pixels_per_point: f32,
    /// The atlas the templates point into (see [`AtlasState`]).
    atlas: AtlasState,
}

/// What tells whether egui rebuilt the font atlas: egui clears it when the text options change or
/// when it fills past 80 %, so a different option set or a lower fill ratio means every atlas
/// coordinate laid out before is stale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AtlasState {
    options: egui::epaint::text::TextOptions,
    fill: f32,
}

impl AtlasState {
    pub(crate) fn now(ctx: &Context) -> Self {
        ctx.fonts(|f| Self {
            options: *f.options(),
            fill: f.font_atlas_fill_ratio(),
        })
    }

    /// Whether the atlas was rebuilt since `self` was taken.
    pub(crate) fn rebuilt_by(&self, now: &Self) -> bool {
        now.options != self.options || now.fill < self.fill
    }
}

/// `SubpixelBin::new` for a non-negative position: (physical x, bin).
fn bin(pos: f32) -> (i32, usize) {
    let trunc = pos as i32;
    let fract = pos - trunc as f32;
    if fract < 0.125 {
        (trunc, 0)
    } else if fract < 0.375 {
        (trunc, 1)
    } else if fract < 0.625 {
        (trunc, 2)
    } else if fract < 0.875 {
        (trunc, 3)
    } else {
        (trunc + 1, 0)
    }
}

fn key(font: &FontId, mono: Mono, row_h: f32) -> u64 {
    hash_one((
        font,
        mono.pixels_per_point.to_bits(),
        mono.advance_px.to_bits(),
        row_h.to_bits(),
    ))
}

impl Templates {
    fn build(ctx: &Context, font: &FontId, mono: Mono) -> Option<Self> {
        let ppp = mono.pixels_per_point;
        let row_h = ctx.fonts_mut(|f| f.row_height(font));
        // Each template char gets its own cell, far enough apart that leading space is never
        // negative; inside the cell its pen position has fraction 0, ¼, ½ or ¾.
        let cell = mono.advance_px.ceil() + 4.0;
        let mut job = LayoutJob {
            wrap: TextWrapping::no_max_width(),
            break_on_newline: false,
            ..Default::default()
        };
        let fmt = TextFormat::simple(font.clone(), Color32::WHITE);
        let mut targets = Vec::with_capacity(N * 4);
        let mut pen = 0.0f32;
        for (i, b) in (FIRST..=LAST).enumerate() {
            for q in 0..4 {
                let target = (i * 4 + q) as f32 * cell + q as f32 * 0.25;
                let s = (b as char).to_string();
                job.append(&s, (target - pen) / ppp, fmt.clone());
                targets.push(target);
                pen = target + mono.advance_px;
            }
        }
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        let row = galley.rows.first()?;
        if row.row.glyphs.len() != N * 4 || galley.rows.len() != 1 {
            return None;
        }
        let mut glyphs = Vec::with_capacity(N * 4);
        let mut px = Vec::with_capacity(N * 4);
        for (i, g) in row.row.glyphs.iter().enumerate() {
            if g.chr as u32 != u32::from(FIRST) + (i / 4) as u32 {
                return None;
            }
            glyphs.push(*g);
            px.push((g.pos.x * ppp).round() as i32);
        }
        // Binning is off when the four bins of a visible glyph share one bitmap.
        let m = (b'M' - FIRST) as usize * 4;
        let binning = glyphs[m].uv_rect != glyphs[m + 2].uv_rect;
        if !binning {
            // Without binning egui rounds the pen position; the template pixels were found by
            // truncation above, which is what `bin` reproduces only with binning on.
            for (i, &t) in targets.iter().enumerate() {
                if px[i] != t.round() as i32 {
                    return None;
                }
            }
        } else {
            for (i, &t) in targets.iter().enumerate() {
                if px[i] != bin(t).0 {
                    return None;
                }
            }
        }
        // A small galley to clone per line.
        let small =
            ctx.fonts_mut(|f| f.layout_job(LayoutJob::single_section("M".into(), fmt.clone())));
        let small_row = small.rows.first()?.row.clone();
        Some(Self {
            key: key(font, mono, row_h),
            glyphs,
            px,
            binning,
            galley: small,
            row: small_row,
            pixels_per_point: ppp,
            atlas: AtlasState::now(ctx),
        })
    }

    /// The template of byte `b` at pen position `h` (physical pixels), moved there.
    fn glyph_at(&self, b: u8, h: f32) -> Glyph {
        let (x, q) = if self.binning {
            bin(h)
        } else {
            (h.round() as i32, 0)
        };
        let i = (b - FIRST) as usize * 4 + q;
        let mut g = self.glyphs[i];
        g.pos.x += (x - self.px[i]) as f32 / self.pixels_per_point;
        g
    }
}

/// The templates for `font` at `mono`'s scale, kept in egui memory (rebuilt when the key
/// changes). None when the font does not lay out as expected; callers then use `layout_job`.
fn templates(ctx: &Context, font: &FontId, mono: Mono) -> Option<Arc<Templates>> {
    let id = Id::unique("quire.ascii_templates");
    let row_h = ctx.fonts_mut(|f| f.row_height(font));
    let k = key(font, mono, row_h);
    if let Some(t) = ctx.data(|d| d.get_temp::<Arc<Templates>>(id))
        && t.key == k
        && !t.atlas.rebuilt_by(&AtlasState::now(ctx))
    {
        return Some(t);
    }
    let t = Arc::new(Templates::build(ctx, font, mono)?);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// Whether `text` with `spans` can take the fast path: printable ASCII and tabs only, and every
/// format the plain editor font in some colour.
pub(crate) fn eligible(text: &str, spans: &[(usize, TextFormat)], default: &TextFormat) -> bool {
    let plain = |f: &TextFormat| *f == TextFormat::simple(default.font_id.clone(), f.color);
    text.bytes()
        .all(|b| (FIRST..=LAST).contains(&b) || b == b'\t')
        && plain(default)
        && spans.iter().all(|(_, f)| plain(f))
}

/// Lay out an eligible line: chars at `column × advance` (tabs to `tabs`' stops), coloured by
/// `spans` (`(start_byte, format)`, sorted). None if the templates are unavailable.
pub(crate) fn layout(
    ctx: &Context,
    text: &str,
    spans: &[(usize, TextFormat)],
    default: &TextFormat,
    mono: Mono,
    tabs: TabStops,
) -> Option<Arc<Galley>> {
    let t = templates(ctx, &default.font_id, mono)?;
    let ppp = t.pixels_per_point;
    let adv = mono.advance_px;
    let ts = tabs.tab_size.max(1);
    let origin = tabs.origin_col();
    let mut glyphs = Vec::with_capacity(text.len());
    let mut mesh = Mesh::with_texture(t.row.visuals.mesh.texture_id);
    mesh.reserve_vertices(text.len() * 4);
    mesh.reserve_triangles(text.len() * 2);
    let mut span = 0usize;
    let mut color = default.color;
    let mut col = origin;
    for (i, b) in text.bytes().enumerate() {
        while span < spans.len() && spans[span].0 <= i {
            color = spans[span].1.color;
            span += 1;
        }
        let h = (col - origin) as f32 * adv;
        let (glyph_byte, w) = if b == b'\t' {
            (b' ', ts - col % ts)
        } else {
            (b, 1)
        };
        let mut g = t.glyph_at(glyph_byte, h);
        g.chr = b as char;
        g.advance_width = w as f32 * adv / ppp;
        g.first_vertex = mesh.vertices.len() as u32;
        let uv_rect = g.uv_rect;
        if b != b'\t' && !uv_rect.is_nothing() {
            let mut lt = g.pos + uv_rect.offset;
            lt.x = (lt.x * ppp).round() / ppp;
            lt.y = (lt.y * ppp).round() / ppp;
            let rect = Rect::from_min_max(lt, lt + uv_rect.size);
            let uv = Rect::from_min_max(
                pos2(uv_rect.min[0] as f32, uv_rect.min[1] as f32),
                pos2(uv_rect.max[0] as f32, uv_rect.max[1] as f32),
            );
            mesh.add_rect_with_uv(rect, uv, color);
        }
        glyphs.push(g);
        col += w;
    }
    let width = (col - origin) as f32 * adv / ppp;
    let n_vertices = mesh.vertices.len();
    let n_indices = mesh.indices.len();
    let mesh_bounds = mesh.calc_bounds();
    let mut row = (*t.row).clone();
    let row_h = row.size.y;
    row.size = vec2(width, row_h);
    row.glyphs = glyphs;
    row.visuals = RowVisuals {
        mesh,
        mesh_bounds,
        glyph_index_start: 0,
        glyph_vertex_range: 0..n_vertices,
    };
    let mut galley = (*t.galley).clone();
    let mut job = (*galley.job).clone();
    job.text = text.to_owned();
    job.sections.truncate(1);
    if let Some(s) = job.sections.first_mut() {
        s.byte_range = egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len());
        s.format.color = default.color;
    }
    galley.job = Arc::new(job);
    galley.rows[0].row = Arc::new(row);
    galley.rows[0].pos = pos2(0.0, 0.0);
    galley.rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, row_h));
    galley.mesh_bounds = mesh_bounds;
    galley.num_vertices = n_vertices;
    galley.num_indices = n_indices;
    Some(Arc::new(galley))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{self, layout_line_job_shaped};

    /// The fast path's galley equals egui's for the same line: glyph positions, atlas
    /// coordinates, vertices and colours.
    fn compare_at(ppp: f32) {
        let ctx = Context::default();
        ctx.set_pixels_per_point(ppp);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let font = FontId::monospace(13.0);
        let mono = layout::mono(&ctx, &font);
        let default = TextFormat::simple(font.clone(), Color32::WHITE);
        let red = TextFormat::simple(font.clone(), Color32::RED);
        let lines = [
            "fn main() { let x = 42; }",
            "\tlet s = \"tab\tinside\";",
            "  ~!@#$%^&*()_+{}|:\"<>?`-=[]\\;',./ 0123456789",
            "",
            " ",
            &"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".repeat(4),
        ];
        for (n, line) in lines.iter().enumerate() {
            for origin in [0.0, 3.0] {
                let mut tabs = mono.tab_stops(4);
                tabs.origin = origin * mono.advance();
                let spans = [(0usize, default.clone()), (line.len().min(3), red.clone())];
                assert!(eligible(line, &spans, &default));
                let fast = layout(&ctx, line, &spans, &default, mono, tabs).expect("templates");
                let slow = layout_line_job_shaped(&ctx, line, &spans, &default, mono, tabs);
                let (fr, sr) = (&fast.rows[0].row, &slow.rows[0].row);
                assert_eq!(fr.glyphs.len(), sr.glyphs.len(), "line {n}");
                for (a, b) in fr.glyphs.iter().zip(&sr.glyphs) {
                    assert_eq!(a.chr, b.chr);
                    assert!(
                        (a.pos.x - b.pos.x).abs() < 1e-3 && (a.pos.y - b.pos.y).abs() < 1e-3,
                        "ppp {ppp} line {n} {:?}: {:?} vs {:?}",
                        a.chr,
                        a.pos,
                        b.pos
                    );
                    assert_eq!(a.uv_rect, b.uv_rect, "ppp {ppp} line {n} {:?}", a.chr);
                }
                let (fm, sm) = (&fr.visuals.mesh, &sr.visuals.mesh);
                assert_eq!(fm.vertices.len(), sm.vertices.len(), "line {n}");
                for (a, b) in fm.vertices.iter().zip(&sm.vertices) {
                    assert!(
                        (a.pos - b.pos).length() < 1e-3,
                        "{:?} vs {:?}",
                        a.pos,
                        b.pos
                    );
                    assert_eq!(a.uv, b.uv);
                    assert_eq!(a.color, b.color);
                }
                assert_eq!(fm.indices, sm.indices);
                assert_eq!(fast.text(), slow.text());
                assert!((fast.rect.height() - slow.rect.height()).abs() < 1e-3);
            }
        }
        // Not eligible: non-ASCII, control chars, other formats.
        assert!(!eligible("é", &[], &default));
        assert!(!eligible("a\u{7}", &[], &default));
        let mut u = default.clone();
        u.italics = true;
        assert!(!eligible("a", &[(0, u)], &default));
    }

    #[test]
    fn fast_path_matches_egui_layout() {
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            compare_at(ppp);
        }
    }
}
