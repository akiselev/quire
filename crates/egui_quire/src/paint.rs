//! Painting: background, current line, find matches, selections, text, preedit, squiggles,
//! carets, gutter and scrollbars. Overlays are cheap shapes rebuilt each frame; line text comes
//! from the galley cache.

use egui::text::TextFormat;
use egui::{Align2, Color32, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::editor::FrameCx;
use crate::galley_cache::{GalleyKey, hash_one};
use crate::glue::{self, Severity};
use crate::wrap::VisualLine;

/// A visible line ready to paint.
#[derive(Debug)]
pub(crate) struct VisLine {
    line: usize,
    start: usize,
    /// Bytes of the line in the document.
    doc_len: usize,
    top: f32,
    layout: VisualLine,
    /// Byte column where an IME preedit was spliced in, and its byte length.
    preedit: Option<(usize, usize)>,
}

impl VisLine {
    /// Column in the layout of a document byte column on this line, accounting for an inline
    /// preedit (text after the caret is shifted by it).
    fn map(&self, col: usize) -> usize {
        match self.preedit {
            Some((at, len)) if col > at => col + len,
            _ => col,
        }
    }

    /// One rect per row covering document columns `a..b` of this line (relative to `left`);
    /// `extra` widens the last row's rect (a range that continues past the line end).
    fn span_rects(&self, a: usize, b: usize, left: f32, row_h: f32, extra: f32) -> SpanRects<'_> {
        SpanRects {
            v: self,
            a: self.map(a),
            b: self.map(b),
            k: 0,
            left,
            row_h,
            extra,
        }
    }
}

/// Iterator behind [`VisLine::span_rects`].
struct SpanRects<'a> {
    v: &'a VisLine,
    a: usize,
    b: usize,
    k: usize,
    left: f32,
    row_h: f32,
    extra: f32,
}

impl Iterator for SpanRects<'_> {
    type Item = Rect;

    fn next(&mut self) -> Option<Rect> {
        let l = &self.v.layout;
        let n = l.n_rows();
        while self.k < n {
            let k = self.k;
            self.k += 1;
            let last = k + 1 == n;
            let rs = l.row_start(k);
            let re = if last { l.full_len() } else { l.row_end(k) };
            if self.b < rs || self.a > re {
                continue;
            }
            // A boundary belongs to the next row's start, unless the span ends there.
            if !last && self.a == re && self.a < self.b {
                continue;
            }
            if k > 0 && self.b == rs && self.a < self.b {
                continue;
            }
            if !last && self.a == re && self.a == self.b {
                continue;
            }
            let x0 = l.x_in_row(k, self.a.max(rs));
            let x1 = l.x_in_row(k, self.b.min(re));
            let top = self.v.top + k as f32 * self.row_h;
            let extra = if last { self.extra } else { 0.0 };
            return Some(Rect::from_min_max(
                pos2(self.left + x0, top),
                pos2(self.left + x1 + extra, top + self.row_h),
            ));
        }
        None
    }
}

/// Paint the editor; returns the primary caret rect (layer coordinates).
pub(crate) fn paint(f: &mut FrameCx<'_>, ui: &Ui) -> Option<Rect> {
    let g = f.geom;
    let theme = f.theme;
    let painter = ui.painter_at(g.rect);
    painter.rect_filled(g.rect, 0.0, theme.background);

    let text_painter = painter.with_clip_rect(g.text.intersect(painter.clip_rect()));
    let left = g.text_left(f.mem);
    let row_h = g.row_h;

    // Selection state.
    let sel = glue::selection(f.view).clone();
    let primary = sel.primary();
    let head_line = glue::text(f.doc).byte_to_line(primary.head.min(glue::text(f.doc).len_bytes()));
    let preedit = glue::preedit(f.view).cloned();

    // Lay out visible lines. With soft wrap, laying a line out measures its rows; when that
    // changes heights, the visible range is taken again (twice at most).
    let mut vis = std::mem::take(&mut f.mem.vis_buf);
    for _ in 0..3 {
        vis.clear();
        let mut changed = false;
        for line in f.visible_lines() {
            let text = glue::text(f.doc);
            if line >= text.len_lines() {
                break;
            }
            let before = f.mem.heights.get(line);
            if before == 0.0 {
                continue; // folded away
            }
            let start = text.line_to_byte(line);
            let doc_len = text.line(line).len_bytes();
            if let Some(p) = preedit.as_ref().filter(|_| line == head_line) {
                let s = glue::line_str(text, line);
                let col = primary.head - start;
                let layout = preedit_layout(f, &s, col, &p.text);
                f.note_rows(line, layout.n_rows());
                vis.push(VisLine {
                    line,
                    start,
                    doc_len,
                    top: 0.0,
                    layout,
                    preedit: Some((col, p.text.len())),
                });
            } else {
                let layout = f.layout_line(line);
                vis.push(VisLine {
                    line,
                    start,
                    doc_len,
                    top: 0.0,
                    layout,
                    preedit: None,
                });
            }
            changed |= f.mem.heights.get(line) != before;
        }
        if !changed {
            break;
        }
    }
    for v in &mut vis {
        v.top = g.line_top(f.mem, v.line);
        f.mem.max_seen_width = f.mem.max_seen_width.max(v.layout.width() + g.pad_left);
    }

    // Current line (every row of it).
    if primary.is_empty()
        && let Some(v) = vis.iter().find(|v| v.line == head_line)
    {
        let h = v.layout.n_rows() as f32 * row_h;
        text_painter.rect_filled(
            Rect::from_min_size(pos2(g.text.left(), v.top), vec2(g.text.width(), h)),
            0.0,
            theme.current_line,
        );
    }

    // The marker after a line cut to its prefix (under selections, which may cover it).
    for v in &vis {
        let l = &v.layout;
        if let Some((m0, m1)) = l.marker() {
            let top = v.top + (l.n_rows() - 1) as f32 * row_h;
            let r = Rect::from_min_max(
                pos2(left + m0, top + row_h * 0.2),
                pos2(left + m1, top + row_h * 0.8),
            );
            text_painter.rect_filled(r, 2.0, theme.current_line);
            text_painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                "…",
                theme.font.clone(),
                theme.line_number,
            );
        }
    }

    // Find matches.
    let (matches, current) = glue::find_matches(f.view, f.doc);
    if !matches.is_empty() {
        for v in &vis {
            let (ls, le) = (v.start, v.start + v.doc_len);
            let lo = matches.partition_point(|m| m.end < ls);
            for (i, m) in matches.iter().enumerate().skip(lo) {
                if m.start > le {
                    break;
                }
                let a = m.start.max(ls) - ls;
                let b = m.end.min(le) - ls;
                let color = if Some(i) == current {
                    theme.find_match_current
                } else {
                    theme.find_match
                };
                let extra = if m.end > le { g.adv * 0.5 } else { 0.0 };
                for r in v.span_rects(a, b, left, row_h, extra) {
                    text_painter.rect_filled(r, 2.0, color);
                }
            }
        }
    }

    // Selections.
    let sel_color = if f.mem.focused {
        theme.selection
    } else {
        theme.selection_unfocused
    };
    for r in glue::ranges(&sel) {
        if r.is_empty() {
            continue;
        }
        let (from, to) = (r.from(), r.to());
        for v in &vis {
            let (ls, le) = (v.start, v.start + v.doc_len);
            if to < ls || from > le || (to == ls && from < ls) {
                continue;
            }
            let a = from.max(ls) - ls;
            let b = to.min(le) - ls;
            let extra = if to > le { g.adv * 0.5 } else { 0.0 };
            for rect in v.span_rects(a, b, left, row_h, extra) {
                text_painter.rect_filled(rect, 0.0, sel_color);
            }
        }
    }

    // Text.
    for v in &vis {
        let l = &v.layout;
        for k in 0..l.n_rows() {
            let top = v.top + k as f32 * row_h;
            text_painter.galley(
                pos2(left + l.row_x0(k), top),
                l.row_layout(k).galley.clone(),
                theme.foreground,
            );
        }
    }

    // Folded headers: a placeholder after the line's end (VS Code's "⋯").
    let first_vis = vis.first().map_or(0, |v| v.line);
    let last_vis = vis.last().map_or(0, |v| v.line + 1);
    let headers = glue::fold_headers(f.doc, first_vis..last_vis);
    let mut folded_headers = Vec::new();
    for v in vis
        .iter()
        .filter(|v| headers.binary_search(&v.line).is_ok())
    {
        if glue::is_folded_header(f.view, f.doc, v.line) {
            folded_headers.push(v.line);
            let x0 = left + v.layout.end_x() + g.adv * 0.5;
            let top = v.top + (v.layout.n_rows() - 1) as f32 * row_h;
            let r =
                Rect::from_min_size(pos2(x0, top + row_h * 0.2), vec2(g.adv * 2.0, row_h * 0.6));
            text_painter.rect_filled(r, 2.0, theme.current_line);
            text_painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                "⋯",
                theme.font.clone(),
                theme.line_number,
            );
        }
    }

    // Squiggles.
    for d in glue::diagnostics(f.doc) {
        let color = match d.severity {
            Severity::Error => theme.error,
            Severity::Warning => theme.warning,
            Severity::Info => theme.info,
            Severity::Hint => theme.hint,
        };
        for v in &vis {
            let (ls, le) = (v.start, v.start + v.doc_len);
            if d.range.end < ls || d.range.start > le {
                continue;
            }
            let a = d.range.start.max(ls) - ls;
            let b = d.range.end.min(le) - ls;
            for r in v.span_rects(a, b, left, row_h, 0.0) {
                let (x0, x1) = (r.left(), r.right().max(r.left() + g.adv));
                if d.severity == Severity::Hint {
                    let y = r.bottom() - 1.0;
                    text_painter
                        .line_segment([pos2(x0, y), pos2(x0 + g.adv, y)], Stroke::new(1.0, color));
                    break;
                }
                squiggle(&text_painter, x0, x1, r.bottom() - 1.5, color);
            }
        }
    }

    // Carets.
    let mut primary_caret = None;
    let show_caret = f.mem.focused && f.caret_visible(ui);
    for (i, r) in glue::ranges(&sel).iter().enumerate() {
        let text = glue::text(f.doc);
        let head = r.head.min(text.len_bytes());
        let line = text.byte_to_line(head);
        let col = head - text.line_to_byte(line);
        let up = f.upstream(head);
        let (row, x_rel, top) = match vis.iter().find(|v| v.line == line) {
            Some(v) => {
                let (row, x) = match (v.preedit, &preedit) {
                    // Caret inside the preedit: at its cursor (end of the active range, else end).
                    (Some((at, _)), Some(p)) if i == sel.primary_index() => {
                        let bytes = p.active.as_ref().map_or(p.text.len(), |r| r.end);
                        v.layout.locate(at + bytes, false)
                    }
                    _ => v.layout.locate(v.map(col), up),
                };
                (row, x, v.top)
            }
            None => {
                let (row, x) = f.layout_line(line).locate(col, up);
                (row, x, g.line_top(f.mem, line))
            }
        };
        let top = top + row as f32 * row_h;
        let rect = Rect::from_min_size(pos2(left + x_rel, top), vec2(theme.caret_width, row_h));
        if i == sel.primary_index() {
            primary_caret = Some(rect);
        }
        if show_caret && rect.intersects(g.text) {
            text_painter.rect_filled(rect, 0.0, theme.caret);
        }
    }

    // Gutter.
    painter.rect_filled(g.gutter, 0.0, theme.gutter_background);
    let worst = worst_severity_per_line(f, &vis);
    let num_x = g.gutter.right() - crate::editor::fold_w(g.adv) - g.adv * 0.5;
    let gutter_hovered = ui.rect_contains_pointer(g.gutter);
    let gutter_painter = painter.with_clip_rect(g.gutter.intersect(painter.clip_rect()));
    let ppp = f.ctx.pixels_per_point();
    for (i, v) in vis.iter().enumerate() {
        let sev = worst.get(i).copied().flatten();
        let color = if v.line == head_line {
            theme.line_number_active
        } else {
            theme.line_number
        };
        // Line numbers go through the galley cache too, so a clean frame lays out (and
        // allocates) nothing for them.
        let key = GalleyKey::new(
            hash_one(("quire.gutter", v.line + 1, color)),
            f.mem.style_salt,
            &theme.font,
            ppp,
            f32::INFINITY,
        );
        let galley = f.mem.galleys.get_or_insert(key, || {
            f.ctx.fonts_mut(|fo| {
                fo.layout_no_wrap((v.line + 1).to_string(), theme.font.clone(), color)
            })
        });
        let rect = Align2::RIGHT_TOP.anchor_size(pos2(num_x, v.top), galley.size());
        gutter_painter.galley(rect.min, galley, color);
        if headers.binary_search(&v.line).is_ok() {
            let folded = folded_headers.contains(&v.line);
            if folded || gutter_hovered {
                let c = g.fold_col();
                let center = pos2(c.center().x, v.top + row_h * 0.5);
                fold_marker(
                    &gutter_painter,
                    center,
                    row_h * 0.22,
                    folded,
                    theme.line_number,
                );
            }
        }
        if let Some(sev) = sev {
            let c = match sev {
                Severity::Error => theme.error,
                Severity::Warning => theme.warning,
                Severity::Info => theme.info,
                Severity::Hint => theme.hint,
            };
            gutter_painter.circle_filled(pos2(g.gutter.left() + 5.0, v.top + row_h * 0.5), 3.0, c);
        }
    }

    vis.clear();
    f.mem.vis_buf = vis;
    primary_caret
}

/// Layout of a line with the IME preedit spliced in at `col`, underlined. Not cached.
fn preedit_layout(f: &FrameCx<'_>, line: &str, col: usize, pre: &str) -> VisualLine {
    let col = col.min(line.len());
    let font = f.theme.font.clone();
    let plain = TextFormat::simple(font.clone(), f.theme.foreground);
    let under = TextFormat {
        underline: Stroke::new(1.0, f.theme.preedit_underline),
        ..TextFormat::simple(font, f.theme.foreground)
    };
    let spliced: String = [&line[..col], pre, &line[col..]].concat();
    let spans = [(col, under), (col + pre.len(), plain.clone())];
    crate::wrap::shape_uncached(
        f.ctx,
        f.geom.mono,
        f.doc.config().tab_size,
        &spliced,
        &spans,
        &plain,
        f.geom.wrap,
    )
}

/// A chevron: pointing right when folded, down when expanded.
fn fold_marker(p: &egui::Painter, c: egui::Pos2, r: f32, folded: bool, color: Color32) {
    let pts = if folded {
        vec![
            pos2(c.x - r * 0.5, c.y - r),
            pos2(c.x + r * 0.5, c.y),
            pos2(c.x - r * 0.5, c.y + r),
        ]
    } else {
        vec![
            pos2(c.x - r, c.y - r * 0.5),
            pos2(c.x, c.y + r * 0.5),
            pos2(c.x + r, c.y - r * 0.5),
        ]
    };
    p.add(Shape::line(pts, Stroke::new(1.5, color)));
}

fn worst_severity_per_line(f: &FrameCx<'_>, vis: &[VisLine]) -> Vec<Option<Severity>> {
    // Empty (no allocation) when there are no diagnostics; callers read missing entries as None.
    if glue::diagnostics(f.doc).next().is_none() {
        return Vec::new();
    }
    let mut out = vec![None; vis.len()];
    let Some(first) = vis.first() else { return out };
    let text = glue::text(f.doc);
    for d in glue::diagnostics(f.doc) {
        let l0 = text.byte_to_line(d.range.start.min(text.len_bytes()));
        let l1 = text.byte_to_line(d.range.end.min(text.len_bytes()));
        for l in l0..=l1 {
            if l < first.line {
                continue;
            }
            let i = l - first.line;
            if i >= out.len() {
                break;
            }
            out[i] = Some(match out[i] {
                Some(s) if s <= d.severity => s,
                _ => d.severity,
            });
        }
    }
    out
}

fn squiggle(p: &egui::Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    let step = 2.0;
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = true;
    while x <= x1 {
        pts.push(pos2(x, if up { y - 1.5 } else { y + 0.5 }));
        x += step;
        up = !up;
    }
    if pts.len() >= 2 {
        p.add(Shape::line(pts, Stroke::new(1.0, color)));
    }
}

/// Vertical and horizontal scrollbars (drawn and dragged).
pub(crate) fn scrollbars(f: &mut FrameCx<'_>, ui: &Ui) {
    let g = f.geom;
    let content_h = f.mem.heights.total() + g.text.height() * 0.5;
    let content_w = f.content_width();
    let bar = crate::editor::SCROLLBAR_W;
    let color = ui.visuals().widgets.inactive.bg_fill;
    let color_active = ui.visuals().widgets.active.bg_fill;

    if content_h > g.text.height() + 0.5 {
        let track = Rect::from_min_max(
            pos2(g.rect.right() - bar, g.text.top()),
            g.rect.right_bottom(),
        );
        let frac = (g.text.height() / content_h).clamp(0.05, 1.0);
        let thumb_h = track.height() * frac;
        let max_scroll = content_h - g.text.height();
        let t = (f.mem.scroll.y / max_scroll).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(
            pos2(track.left(), track.top() + t * (track.height() - thumb_h)),
            vec2(bar, thumb_h),
        );
        let r = ui.interact(track, f.id.with("quire.vscroll"), Sense::drag());
        if r.dragged() {
            f.mem.scroll.y += r.drag_delta().y * max_scroll / (track.height() - thumb_h).max(1.0);
            f.mem.scroll.y = f.mem.scroll.y.clamp(0.0, max_scroll);
        }
        let c = if r.dragged() || r.hovered() {
            color_active
        } else {
            color
        };
        ui.painter().rect_filled(thumb.shrink(1.5), 3.0, c);
    }
    if content_w > g.text.width() + 0.5 {
        let track = Rect::from_min_max(
            pos2(g.text.left(), g.rect.bottom() - bar),
            pos2(g.rect.right() - bar, g.rect.bottom()),
        );
        let frac = (g.text.width() / content_w).clamp(0.05, 1.0);
        let thumb_w = track.width() * frac;
        let max_scroll = content_w - g.text.width();
        let t = (f.mem.scroll.x / max_scroll).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(
            pos2(track.left() + t * (track.width() - thumb_w), track.top()),
            vec2(thumb_w, bar),
        );
        let r = ui.interact(track, f.id.with("quire.hscroll"), Sense::drag());
        if r.dragged() {
            f.mem.scroll.x += r.drag_delta().x * max_scroll / (track.width() - thumb_w).max(1.0);
            f.mem.scroll.x = f.mem.scroll.x.clamp(0.0, max_scroll);
        }
        let c = if r.dragged() || r.hovered() {
            color_active
        } else {
            color
        };
        ui.painter().rect_filled(thumb.shrink(1.5), 3.0, c);
    }
}
