//! Painting: background, current line, find matches, selections, text, preedit, squiggles,
//! carets, gutter and scrollbars. Overlays are cheap shapes rebuilt each frame; line text comes
//! from the galley cache.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::editor::FrameCx;
use crate::glue::{self, Severity};
use crate::layout::LineLayout;

/// A visible line ready to paint.
struct VisLine {
    line: usize,
    start: usize,
    top: f32,
    layout: LineLayout,
    /// Byte column where an IME preedit was spliced in, and its byte length.
    preedit: Option<(usize, usize)>,
}

impl VisLine {
    /// x (relative to text left) of a document byte column on this line, accounting for an
    /// inline preedit (text after the caret is shifted by it).
    fn x_of(&self, col: usize) -> f32 {
        match self.preedit {
            Some((at, len)) if col > at => self.layout.x_of(col + len),
            _ => self.layout.x_of(col),
        }
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
    let preedit = f.mem.preedit.clone();

    // Lay out visible lines.
    let mut vis: Vec<VisLine> = Vec::new();
    for line in f.visible_lines() {
        let text = glue::text(f.doc);
        if line >= text.len_lines() {
            break;
        }
        let start = text.line_to_byte(line);
        let top = g.line_top(f.mem, line);
        if let Some(p) = preedit.as_ref().filter(|_| line == head_line) {
            let s = glue::line_str(text, line);
            let col = primary.head - start;
            let layout = preedit_layout(f, &s, col, &p.text);
            vis.push(VisLine { line, start, top, layout, preedit: Some((col, p.text.len())) });
        } else {
            let layout = f.layout_line(line);
            vis.push(VisLine { line, start, top, layout, preedit: None });
        }
    }
    for v in &vis {
        f.mem.max_seen_width = f.mem.max_seen_width.max(v.layout.width() + g.pad_left);
    }

    let full_row = |top: f32| Rect::from_min_size(pos2(g.text.left(), top), vec2(g.text.width(), row_h));
    let span = |v: &VisLine, a: usize, b: usize, extra: f32| {
        Rect::from_min_max(pos2(left + v.x_of(a), v.top), pos2(left + v.x_of(b) + extra, v.top + row_h))
    };

    // Current line.
    if primary.is_empty() {
        if let Some(v) = vis.iter().find(|v| v.line == head_line) {
            text_painter.rect_filled(full_row(v.top), 0.0, theme.current_line);
        }
    }

    // Find matches.
    let (matches, current) = glue::find_matches(f.view, f.doc);
    if !matches.is_empty() {
        for v in &vis {
            let len = v.layout.text().len();
            let (ls, le) = (v.start, v.start + len);
            let lo = matches.partition_point(|m| m.end < ls);
            for (i, m) in matches.iter().enumerate().skip(lo) {
                if m.start > le {
                    break;
                }
                let a = m.start.max(ls) - ls;
                let b = m.end.min(le) - ls;
                let color = if Some(i) == current { theme.find_match_current } else { theme.find_match };
                text_painter.rect_filled(span(v, a, b, if m.end > le { g.adv * 0.5 } else { 0.0 }), 2.0, color);
            }
        }
    }

    // Selections.
    let sel_color = if f.mem.focused { theme.selection } else { theme.selection_unfocused };
    for r in glue::ranges(&sel) {
        if r.is_empty() {
            continue;
        }
        let (from, to) = (r.from(), r.to());
        for v in &vis {
            let len = v.layout.text().len();
            let (ls, le) = (v.start, v.start + len);
            if to < ls || from > le || (to == ls && from < ls) {
                continue;
            }
            let a = from.max(ls) - ls;
            let b = to.min(le) - ls;
            let extra = if to > le { g.adv * 0.5 } else { 0.0 };
            text_painter.rect_filled(span(v, a, b, extra), 0.0, sel_color);
        }
    }

    // Text.
    for v in &vis {
        text_painter.galley(pos2(left, v.top), v.layout.galley.clone(), theme.foreground);
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
            let len = v.layout.text().len();
            let (ls, le) = (v.start, v.start + len);
            if d.range.end < ls || d.range.start > le {
                continue;
            }
            let a = d.range.start.max(ls) - ls;
            let b = d.range.end.min(le) - ls;
            let x0 = left + v.x_of(a);
            let x1 = (left + v.x_of(b)).max(x0 + g.adv);
            if d.severity == Severity::Hint {
                let y = v.top + row_h - 1.0;
                text_painter.line_segment([pos2(x0, y), pos2(x0 + g.adv, y)], Stroke::new(1.0, color));
            } else {
                squiggle(&text_painter, x0, x1, v.top + row_h - 1.5, color);
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
        let x_rel = match vis.iter().find(|v| v.line == line) {
            Some(v) => {
                let extra = match (v.preedit, &preedit) {
                    // Caret inside the preedit: at its cursor (end of the active range, else end).
                    (Some((at, _)), Some(p)) if i == sel.primary_index() => {
                        let chars = p.active_chars.as_ref().map_or(p.text.chars().count(), |r| r.end);
                        let bytes: usize = p.text.chars().take(chars).map(char::len_utf8).sum();
                        Some(v.layout.x_of(at + bytes))
                    }
                    _ => None,
                };
                extra.unwrap_or_else(|| v.x_of(col))
            }
            None => f.layout_line(line).x_of(col),
        };
        let top = g.line_top(f.mem, line);
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
    let num_x = g.gutter.right() - g.adv * 0.75;
    let gutter_painter = painter.with_clip_rect(g.gutter.intersect(painter.clip_rect()));
    for (v, sev) in vis.iter().zip(worst) {
        let color = if v.line == head_line { theme.line_number_active } else { theme.line_number };
        gutter_painter.text(pos2(num_x, v.top), Align2::RIGHT_TOP, (v.line + 1).to_string(), theme.font.clone(), color);
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

    primary_caret
}

/// Layout of a line with the IME preedit spliced in at `col`, underlined. Not cached.
fn preedit_layout(f: &FrameCx<'_>, line: &str, col: usize, pre: &str) -> LineLayout {
    let col = col.min(line.len());
    let font = f.theme.font.clone();
    let mut job = LayoutJob { wrap: TextWrapping::no_max_width(), break_on_newline: false, ..Default::default() };
    let plain = TextFormat::simple(font.clone(), f.theme.foreground);
    let under = TextFormat {
        underline: Stroke::new(1.0, f.theme.preedit_underline),
        ..TextFormat::simple(font, f.theme.foreground)
    };
    job.append(&line[..col], 0.0, plain.clone());
    job.append(pre, 0.0, under);
    job.append(&line[col..], 0.0, plain);
    let galley: Arc<egui::Galley> = f.ctx.fonts_mut(|fo| fo.layout_job(job));
    let spliced: String = [&line[..col], pre, &line[col..]].concat();
    let mono = (spliced.is_ascii() && !spliced.contains('\t')).then_some(f.geom.adv);
    LineLayout { galley, mono_advance: mono }
}

fn worst_severity_per_line(f: &FrameCx<'_>, vis: &[VisLine]) -> Vec<Option<Severity>> {
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
    let content_w = f.mem.max_seen_width + g.adv * 4.0;
    let bar = 8.0;
    let color = ui.visuals().widgets.inactive.bg_fill;
    let color_active = ui.visuals().widgets.active.bg_fill;

    if content_h > g.text.height() + 0.5 {
        let track = Rect::from_min_max(pos2(g.rect.right() - bar, g.text.top()), g.rect.right_bottom());
        let frac = (g.text.height() / content_h).clamp(0.05, 1.0);
        let thumb_h = track.height() * frac;
        let max_scroll = content_h - g.text.height();
        let t = (f.mem.scroll.y / max_scroll).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(pos2(track.left(), track.top() + t * (track.height() - thumb_h)), vec2(bar, thumb_h));
        let r = ui.interact(track, f.id.with("quire.vscroll"), Sense::drag());
        if r.dragged() {
            f.mem.scroll.y += r.drag_delta().y * max_scroll / (track.height() - thumb_h).max(1.0);
            f.mem.scroll.y = f.mem.scroll.y.clamp(0.0, max_scroll);
        }
        let c = if r.dragged() || r.hovered() { color_active } else { color };
        ui.painter().rect_filled(thumb.shrink(1.5), 3.0, c);
    }
    if content_w > g.text.width() + 0.5 {
        let track = Rect::from_min_max(pos2(g.text.left(), g.rect.bottom() - bar), pos2(g.rect.right() - bar, g.rect.bottom()));
        let frac = (g.text.width() / content_w).clamp(0.05, 1.0);
        let thumb_w = track.width() * frac;
        let max_scroll = content_w - g.text.width();
        let t = (f.mem.scroll.x / max_scroll).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(pos2(track.left() + t * (track.width() - thumb_w), track.top()), vec2(thumb_w, bar));
        let r = ui.interact(track, f.id.with("quire.hscroll"), Sense::drag());
        if r.dragged() {
            f.mem.scroll.x += r.drag_delta().x * max_scroll / (track.width() - thumb_w).max(1.0);
            f.mem.scroll.x = f.mem.scroll.x.clamp(0.0, max_scroll);
        }
        let c = if r.dragged() || r.hovered() { color_active } else { color };
        ui.painter().rect_filled(thumb.shrink(1.5), 3.0, c);
    }
}
