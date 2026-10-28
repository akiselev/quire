//! Recording: the editor's display lists, one per compositor space, so scrolling never re-records.
//!
//! | List | Space | Holds |
//! |---|---|---|
//! | [`EditorScene::background`] | local | the editor and gutter backgrounds |
//! | [`EditorScene::content`] | content (scrolls in x and y) | current line, find matches, selections, text, inlay hint badges, preedit underline, diagnostics squiggles (wavy lines), secondary carets |
//! | [`EditorScene::gutter`] | gutter (scrolls in y only) | diagnostic marks, line numbers, fold markers |
//! | [`EditorScene::overlay`] | local, above the panes | scrollbars, the suggest list, the hover, the parameter hints |
//!
//! Only the lines of the viewport plus overscan are recorded ([`SkelterEditor::layout`]); a scroll
//! that stays inside them ([`SkelterEditor::covers`]) moves the content and gutter layers by offset.
//! The primary caret is not in any list: it is [`EditorScene::caret`], for the compositor to blink
//! as a fast commit ([`skelter_base::commit::FastCommit::Caret`]); hosts without that draw it with
//! [`EditorScene::caret_prim`].

use std::ops::Range;
use std::sync::Arc;

use quire_core::{Document, EditorView, Severity};
use skelter_base::color::{Color, Fill};
use skelter_base::display::{BorderStyle, DisplayList, Glyphs, Line, LineCap, Prim, Quad};
use skelter_base::geom::{Corners, Insets, Point, Rect, Size, Vec2};
use skelter_base::ids::RunId;
use skelter_base::text::GlyphRun;

use crate::editor::{LaidLine, PAD_Y, SCROLLBAR_W, SkelterEditor, fold_headers};
use crate::lines::{PAINT_HINT, PAINT_PREEDIT, PieceKind};
use crate::shape::ShapedLine;

/// Where recorded glyph runs go: the compositor holds runs by id (skelter-core's `RecordCx` for a
/// node, [`crate::layers::RunTable`] for the editor's own layers).
pub trait RunSink {
    fn add_run(&mut self, run: Arc<GlyphRun>) -> RunId;
}

/// One record of the editor.
#[derive(Clone, Debug, Default)]
pub struct EditorScene {
    pub background: DisplayList,
    pub content: DisplayList,
    pub gutter: DisplayList,
    pub overlay: DisplayList,
    /// The primary caret in content space (`None`: unfocused, or a selection without caret).
    pub caret: Option<Rect>,
    pub caret_color: Color,
    /// Where content space's origin sits in local space at scroll zero.
    pub text_origin: Point,
    pub scroll: Vec2,
    pub content_size: Size,
    pub gutter_width: f32,
    /// The text area in local space (the content layer's clip).
    pub text_clip: Rect,
    /// The IME candidate area (the primary caret, local space).
    pub ime_rect: Option<Rect>,
    /// [`SkelterEditor::content_rev`] at record time.
    pub content_rev: u64,
}

impl EditorScene {
    /// The primary caret as a quad in content space, for hosts that draw it themselves.
    pub fn caret_prim(&self) -> Option<(Prim, Rect)> {
        self.caret.map(|r| (quad(r, self.caret_color), r))
    }
}

fn quad(r: Rect, c: Color) -> Prim {
    Prim::Quad(Quad {
        rect: r,
        radii: Corners::ZERO,
        fill: Fill::Solid(c),
        border_widths: Insets::ZERO,
        border_color: Color::TRANSPARENT,
        border_style: BorderStyle::Solid,
    })
}

fn push_quad(list: &mut DisplayList, r: Rect, c: Color) {
    if c.a > 0.0 && !r.is_empty() {
        list.push(quad(r, c), r);
    }
}

fn push_line(list: &mut DisplayList, from: Point, to: Point, width: f32, color: Color, wavy: bool) {
    let ink = Rect::new(
        from.x.min(to.x),
        from.y.min(to.y),
        from.x.max(to.x),
        from.y.max(to.y),
    )
    .inflate(width * 2.0);
    list.push(
        Prim::Line(Line {
            from,
            to,
            width,
            color,
            cap: LineCap::Butt,
            dash: None,
            wavy,
        }),
        ink,
    );
}

fn push_run(
    list: &mut DisplayList,
    sink: &mut dyn RunSink,
    run: &Arc<GlyphRun>,
    origin: Point,
    color: Color,
) {
    if run.glyphs.is_empty() {
        return;
    }
    let ink = Rect::new(
        origin.x,
        origin.y - run.ascent,
        origin.x + run.advance,
        origin.y + run.descent,
    );
    let id = sink.add_run(run.clone());
    list.push(
        Prim::Glyphs(Glyphs {
            run: id,
            origin,
            color,
        }),
        ink,
    );
}

/// Severity colour.
fn severity_color(theme: &crate::EditorTheme, s: Severity) -> Color {
    match s {
        Severity::Error => theme.error,
        Severity::Warning => theme.warning,
        Severity::Info => theme.info,
        Severity::Hint => theme.hint,
    }
}

/// Clips sorted `ranges` (document bytes) to line `l`; yields display ranges and whether the range
/// continues past the line's end (the line feed is selected).
fn line_spans<'r>(
    l: &'r LaidLine,
    start: usize,
    ranges: impl Iterator<Item = Range<usize>> + 'r,
) -> impl Iterator<Item = (Range<usize>, bool)> + 'r {
    let end = start + l.display.model_len;
    ranges.filter_map(move |r| {
        if r.end < start || r.start > end || (r.is_empty() && r.start != start) {
            return None;
        }
        let a = r.start.max(start) - start;
        let b = r.end.min(end) - start;
        let past = r.end > end;
        (a < b || past).then(|| (l.display.to_display(a)..l.display.to_display(b), past))
    })
}

impl SkelterEditor {
    /// The y of line `l`'s first row in content space.
    fn line_top(&self, line: usize) -> f32 {
        self.heights.y_of(line)
    }

    /// Rects of display range `d` on line `l` (content space), one per row it touches; `past`
    /// extends the last row by half a column for the line feed.
    fn span_rects(&self, l: &LaidLine, d: Range<usize>, past: bool, out: &mut Vec<Rect>) {
        let top = self.line_top(l.line);
        let n = l.rows.n_rows();
        for (k, row) in l.rows.rows.iter().enumerate() {
            let s = d.start.max(row.range.start);
            let e = d.end.min(row.range.end);
            let last = k + 1 == n;
            let wrapped_through = !last && d.end > row.range.end && d.start <= row.range.end;
            if s > e || (s == e && !(past && last) && !wrapped_through) {
                continue;
            }
            let x0 = l.rows.x_in_row(k, s);
            let mut x1 = l.rows.x_in_row(k, e);
            if (past && last) || wrapped_through {
                x1 += self.geom.column * 0.5;
            }
            let y = top + k as f32 * self.geom.row_h;
            out.push(Rect::new(x0, y, x1.max(x0), y + self.geom.row_h));
        }
    }

    /// Records the editor (call after [`Self::layout`]); the overlay's runs go to `overlay_sink`
    /// when given (a host that re-records the overlay alone, on scroll, keeps them apart).
    pub fn record(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        sink: &mut dyn RunSink,
        overlay_sink: Option<&mut dyn RunSink>,
    ) -> EditorScene {
        let theme = self.theme.clone();
        let g = self.geom;
        let text = doc.text();
        let mut sc = EditorScene {
            text_origin: Point::new(g.text_left(), PAD_Y),
            scroll: self.scroll,
            content_size: self.content_size(),
            gutter_width: g.gutter_w,
            text_clip: Rect::new(g.gutter_w, 0.0, g.size.w, g.size.h),
            caret_color: theme.caret,
            content_rev: self.content_rev,
            ..EditorScene::default()
        };
        // background
        push_quad(
            &mut sc.background,
            Rect::from_origin_size(Point::ZERO, g.size),
            theme.background,
        );
        if g.gutter_w > 0.0 {
            push_quad(&mut sc.background, g.gutter_rect(), theme.gutter_background);
        }
        let laid = std::mem::take(&mut self.laid);
        let content_w = sc.content_size.w.max(g.size.w);
        let sel = view.selection().clone();
        let primary = sel.primary();
        let mut rects = Vec::new();

        // current line (VS Code `renderLineHighlight: line`, while the selection is empty)
        if self.is_focused() && primary.is_empty() {
            let up = self.upstream(doc, view, primary.head);
            let (caret, _) = self.caret_content_rect(doc, view, primary.head, up);
            push_quad(
                &mut sc.content,
                Rect::new(-g.pad_left, caret.min.y, content_w, caret.max.y),
                theme.current_line,
            );
        }

        // find matches
        if let Some(f) = view.find() {
            let current = view.find_current_match();
            for l in &laid {
                let start = text.line_to_byte(l.line);
                let end = start + l.display.model_len;
                let lo = f.matches.partition_point(|m| m.end < start);
                for (i, m) in f.matches[lo..]
                    .iter()
                    .enumerate()
                    .take_while(|(_, m)| m.start <= end)
                {
                    let color = if current == Some(lo + i) {
                        theme.find_match_current
                    } else {
                        theme.find_match
                    };
                    rects.clear();
                    for (d, past) in line_spans(l, start, std::iter::once(m.clone())) {
                        self.span_rects(l, d, past, &mut rects);
                    }
                    for r in &rects {
                        push_quad(&mut sc.content, *r, color);
                    }
                }
            }
        }

        // selections
        let sel_color = if self.is_focused() {
            theme.selection
        } else {
            theme.selection_unfocused
        };
        for l in &laid {
            let start = text.line_to_byte(l.line);
            rects.clear();
            for (d, past) in line_spans(
                l,
                start,
                sel.ranges()
                    .iter()
                    .filter(|r| !r.is_empty())
                    .map(|r| r.range()),
            ) {
                self.span_rects(l, d, past, &mut rects);
            }
            for r in &rects {
                push_quad(&mut sc.content, *r, sel_color);
            }
        }

        // text, inlay hint badges, preedit underline
        let baseline = g.baseline();
        for l in &laid {
            let top = self.line_top(l.line);
            for (k, row) in l.rows.rows.iter().enumerate() {
                let y = top + k as f32 * g.row_h;
                for p in &l.display.pieces {
                    let s = p.display.start.max(row.range.start);
                    let e = p.display.end.min(row.range.end);
                    if s >= e {
                        continue;
                    }
                    let (x0, x1) = (l.rows.x_in_row(k, s), l.rows.x_in_row(k, e));
                    match p.kind {
                        PieceKind::Hint { .. } => {
                            let (_, bg) = theme.inlay_hint_colors();
                            push_quad(
                                &mut sc.content,
                                Rect::new(x0, y + 1.0, x1, y + g.row_h - 1.0),
                                bg,
                            );
                        }
                        PieceKind::Preedit => {
                            let uy = y + baseline + 2.0;
                            push_line(
                                &mut sc.content,
                                Point::new(x0, uy),
                                Point::new(x1, uy),
                                1.0,
                                theme.preedit_underline,
                                false,
                            );
                        }
                    }
                }
                for run in &row.runs {
                    let color = match run.paint {
                        PAINT_HINT => theme.inlay_hint_foreground,
                        PAINT_PREEDIT => theme.foreground,
                        p => theme.paint_color(p),
                    };
                    push_run(
                        &mut sc.content,
                        sink,
                        &run.run,
                        Point::new(run.x, y + baseline),
                        color,
                    );
                }
            }
        }

        // diagnostics squiggles
        let mut diags: Vec<(Range<usize>, Severity)> = doc
            .diagnostics()
            .all()
            .map(|(_, d)| (d.range.clone(), d.severity))
            .collect();
        diags.sort_by_key(|(r, _)| r.start);
        for l in &laid {
            let start = text.line_to_byte(l.line);
            let end = start + l.display.model_len;
            for (r, sev) in diags
                .iter()
                .filter(|(r, _)| r.start <= end && r.end >= start)
            {
                // an empty range squiggles the char after it (VS Code widens it to a word)
                let r = if r.is_empty() {
                    r.start..text.next_char_boundary(r.start).min(end.max(r.start))
                } else {
                    r.clone()
                };
                rects.clear();
                for (d, past) in line_spans(l, start, std::iter::once(r)) {
                    self.span_rects(l, d, past, &mut rects);
                }
                for q in &rects {
                    let w = (q.max.x - q.min.x).max(g.column * 0.5);
                    let y = q.max.y - 2.0;
                    push_line(
                        &mut sc.content,
                        Point::new(q.min.x, y),
                        Point::new(q.min.x + w, y),
                        1.0,
                        severity_color(&theme, *sev),
                        true,
                    );
                }
            }
        }

        // carets: secondary ones in the content list, the primary for the compositor
        if self.is_focused() {
            for (i, r) in sel.ranges().iter().enumerate() {
                let up = self.upstream(doc, view, r.head);
                let (rect, _) = self.caret_content_rect(doc, view, r.head, up);
                if i == sel.primary_index() {
                    sc.caret = Some(rect);
                    sc.ime_rect = Some(rect.translate(Vec2::new(
                        sc.text_origin.x - self.scroll.x,
                        sc.text_origin.y - self.scroll.y,
                    )));
                } else {
                    push_quad(&mut sc.content, rect, theme.caret);
                }
            }
        }

        // gutter
        if g.gutter_w > 0.0 {
            self.record_gutter(doc, view, &laid, &diags, sink, &mut sc.gutter);
        }

        self.laid = laid;
        match overlay_sink {
            Some(o) => self.record_overlay(doc, view, o, &mut sc),
            None => self.record_overlay(doc, view, sink, &mut sc),
        }
        sc
    }

    /// Records the overlay alone (scrollbars and popups move with the scroll offset) into a fresh
    /// scene whose other lists are empty.
    pub fn record_overlay_only(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        sink: &mut dyn RunSink,
    ) -> EditorScene {
        let g = self.geom;
        let mut sc = EditorScene {
            text_origin: Point::new(g.text_left(), PAD_Y),
            scroll: self.scroll,
            content_size: self.content_size(),
            gutter_width: g.gutter_w,
            text_clip: Rect::new(g.gutter_w, 0.0, g.size.w, g.size.h),
            caret_color: self.theme.caret,
            content_rev: self.content_rev,
            ..EditorScene::default()
        };
        if self.is_focused() {
            let primary = view.selection().primary();
            let up = self.upstream(doc, view, primary.head);
            let (rect, _) = self.caret_content_rect(doc, view, primary.head, up);
            sc.caret = Some(rect);
            sc.ime_rect = Some(rect.translate(Vec2::new(
                sc.text_origin.x - self.scroll.x,
                sc.text_origin.y - self.scroll.y,
            )));
        }
        self.record_overlay(doc, view, sink, &mut sc);
        sc
    }

    fn record_gutter(
        &mut self,
        doc: &Document,
        view: &EditorView,
        laid: &[LaidLine],
        diags: &[(Range<usize>, Severity)],
        sink: &mut dyn RunSink,
        out: &mut DisplayList,
    ) {
        let theme = self.theme.clone();
        let g = self.geom;
        let text = doc.text();
        let head_line = text.byte_to_line(view.selection().primary().head.min(text.len_bytes()));
        let baseline = g.baseline();
        let digits_right = g.gutter_w - crate::editor::fold_w(g.column) - g.column * 0.5;
        let (Some(first), Some(last)) = (laid.first(), laid.last()) else {
            return;
        };
        let headers = fold_headers(doc, first.line..last.line + 1);
        for l in laid {
            let y = self.line_top(l.line);
            // the worst diagnostic of the line
            let start = text.line_to_byte(l.line);
            let end = start + l.display.model_len;
            let worst = diags
                .iter()
                .filter(|(r, _)| r.start <= end && r.end >= start)
                .map(|(_, s)| *s)
                .min_by_key(|s| match s {
                    Severity::Error => 0,
                    Severity::Warning => 1,
                    Severity::Info => 2,
                    Severity::Hint => 3,
                });
            if let Some(s) = worst {
                push_quad(
                    out,
                    Rect::new(2.0, y + 2.0, crate::editor::MARK_W - 2.0, y + g.row_h - 2.0),
                    severity_color(&theme, s),
                );
            }
            let number = (l.line + 1).to_string();
            let shaped: Arc<ShapedLine> = self.shaper.shape(&number, &[]);
            let color = if l.line == head_line {
                theme.line_number_active
            } else {
                theme.line_number
            };
            let x0 = digits_right - shaped.width;
            for run in &shaped.runs {
                push_run(
                    out,
                    sink,
                    &run.run,
                    Point::new(x0 + run.x, y + baseline),
                    color,
                );
            }
            if headers.binary_search(&l.line).is_ok() {
                let folded = view.fold_at_line(text, l.line).is_some();
                let fc = g.fold_col();
                let c = Point::new((fc.min.x + fc.max.x) * 0.5, y + g.row_h * 0.5);
                let r = (g.row_h * 0.18).max(3.0);
                // a chevron: right (folded) or down (open)
                let (a, b, d) = if folded {
                    (
                        Point::new(c.x - r * 0.5, c.y - r),
                        Point::new(c.x + r * 0.5, c.y),
                        Point::new(c.x - r * 0.5, c.y + r),
                    )
                } else {
                    (
                        Point::new(c.x - r, c.y - r * 0.5),
                        Point::new(c.x, c.y + r * 0.5),
                        Point::new(c.x + r, c.y - r * 0.5),
                    )
                };
                push_line(out, a, b, 1.2, theme.line_number, false);
                push_line(out, b, d, 1.2, theme.line_number, false);
            }
        }
    }

    fn record_overlay(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        sink: &mut dyn RunSink,
        sc: &mut EditorScene,
    ) {
        let g = self.geom;
        let theme = self.theme.clone();
        // vertical scrollbar
        let view_h = g.size.h;
        if sc.content_size.h > view_h + 0.5 {
            let track = Rect::new(g.size.w - SCROLLBAR_W, 0.0, g.size.w, view_h);
            let h = (view_h * view_h / sc.content_size.h).max(20.0).min(view_h);
            let y = (self.scroll.y / (sc.content_size.h - view_h)).clamp(0.0, 1.0) * (view_h - h);
            push_quad(
                &mut sc.overlay,
                Rect::new(track.min.x + 1.0, y, track.max.x - 1.0, y + h),
                theme.scrollbar,
            );
        }
        let view_w = g.size.w - g.text_left();
        if self.horizontal_scrollbar_shown() {
            let w = (view_w * view_w / sc.content_size.w).max(20.0).min(view_w);
            let x = g.text_left()
                + (self.scroll.x / (sc.content_size.w - view_w)).clamp(0.0, 1.0) * (view_w - w);
            push_quad(
                &mut sc.overlay,
                Rect::new(x, g.size.h - SCROLLBAR_W + 1.0, x + w, g.size.h - 1.0),
                theme.scrollbar,
            );
        }
        self.popups.clear();
        let Some(caret) = sc.ime_rect else {
            self.record_hover(doc, view, sink, sc);
            return;
        };
        // parameter hints above the caret
        if view.parameter_hints().is_visible()
            && let Some(help) = view.parameter_hints().active()
            && let Some(sig) = help.signatures.get(help.active_signature)
        {
            let lines = vec![(sig.label.clone(), false)];
            let r = self.popup(
                &lines,
                Point::new(caret.min.x, caret.min.y),
                true,
                sink,
                sc,
                None,
            );
            // underline the active parameter
            let p = help.active_parameter_of(sig);
            let span = sig.parameter_label_offsets(p);
            if span.start < span.end && span.end <= sig.label.len() {
                let shaped = self.shaper.shape(&sig.label, &[]);
                let x0 = r.min.x + POPUP_PAD + shaped.x_of(span.start);
                let x1 = r.min.x + POPUP_PAD + shaped.x_of(span.end);
                let y = r.min.y + POPUP_PAD + g.baseline() + 2.0;
                push_line(
                    &mut sc.overlay,
                    Point::new(x0, y),
                    Point::new(x1, y),
                    1.0,
                    theme.popup_foreground,
                    false,
                );
            }
            self.popups.push(Popup::ParameterHints(r));
        }
        // the suggest list below the caret
        let st = view.suggest();
        if st.is_visible() && !st.items().is_empty() {
            let top = st.scroll_top();
            let focused = st.focused();
            let lines: Vec<(String, bool)> = st
                .items()
                .iter()
                .enumerate()
                .skip(top)
                .take(SUGGEST_ROWS)
                .map(|(i, it)| (it.item.completion.label.clone(), focused == Some(i)))
                .collect();
            let r = self.popup(
                &lines,
                Point::new(caret.min.x, caret.max.y),
                false,
                sink,
                sc,
                Some(theme.popup_selected),
            );
            self.popups.push(Popup::Suggest { rect: r, top });
        }
        self.record_hover(doc, view, sink, sc);
    }

    fn record_hover(
        &mut self,
        doc: &Document,
        view: &mut EditorView,
        sink: &mut dyn RunSink,
        sc: &mut EditorScene,
    ) {
        let Some(shown) = view.hover().shown().cloned() else {
            return;
        };
        let mut lines = Vec::new();
        for part in &shown.parts {
            let s = match part {
                quire_core::hover::HoverPart::Loading { .. } => "Loading...".to_owned(),
                p => p.accessible_content(),
            };
            for line in s.lines().take(HOVER_LINES) {
                lines.push((line.to_owned(), false));
            }
        }
        if lines.is_empty() {
            return;
        }
        let a = shown.anchor.range.start;
        let (rect, _) = self.caret_content_rect(doc, view, a, false);
        let at = Point::new(
            rect.min.x + sc.text_origin.x - self.scroll.x,
            rect.min.y + sc.text_origin.y - self.scroll.y,
        );
        let r = self.popup(&lines, at, true, sink, sc, None);
        self.popups.push(Popup::Hover(r));
    }

    /// A box of text lines at `at` (above it when `above`), clamped into the editor; returns its
    /// rect (local).
    fn popup(
        &mut self,
        lines: &[(String, bool)],
        at: Point,
        above: bool,
        sink: &mut dyn RunSink,
        sc: &mut EditorScene,
        selected: Option<Color>,
    ) -> Rect {
        let g = self.geom;
        let theme = self.theme.clone();
        let shaped: Vec<Arc<ShapedLine>> = lines
            .iter()
            .map(|(s, _)| self.shaper.shape(s, &[]))
            .collect();
        let w = shaped
            .iter()
            .map(|s| s.width)
            .fold(0.0, f32::max)
            .min(POPUP_MAX_W)
            + POPUP_PAD * 2.0;
        let h = lines.len() as f32 * g.row_h + POPUP_PAD * 2.0;
        let mut x = at.x.min(g.size.w - w).max(0.0);
        let mut y = if above { at.y - h } else { at.y };
        if above && y < 0.0 {
            y = at.y + g.row_h;
        }
        if !above && y + h > g.size.h && at.y - g.row_h - h >= 0.0 {
            y = at.y - g.row_h - h;
        }
        x = x.max(0.0);
        let r = Rect::new(x, y, x + w, y + h);
        sc.overlay.push(
            Prim::Quad(Quad {
                rect: r,
                radii: Corners::all(3.0),
                fill: Fill::Solid(theme.popup_background),
                border_widths: Insets::all(1.0),
                border_color: theme.popup_border,
                border_style: BorderStyle::Solid,
            }),
            r,
        );
        sc.overlay.push(
            Prim::PushClip(skelter_base::geom::RoundedRect {
                rect: r,
                radii: Corners::all(3.0),
            }),
            Rect::ZERO,
        );
        for (i, (s, (_, sel))) in shaped.iter().zip(lines).enumerate() {
            let ry = y + POPUP_PAD + i as f32 * g.row_h;
            if *sel && let Some(c) = selected {
                push_quad(
                    &mut sc.overlay,
                    Rect::new(x + 1.0, ry, x + w - 1.0, ry + g.row_h),
                    c,
                );
            }
            for run in &s.runs {
                push_run(
                    &mut sc.overlay,
                    sink,
                    &run.run,
                    Point::new(x + POPUP_PAD + run.x, ry + g.baseline()),
                    theme.popup_foreground,
                );
            }
        }
        sc.overlay.push(Prim::PopClip, Rect::ZERO);
        r
    }
}

/// Rows the suggest list shows (VS Code's default 12).
pub const SUGGEST_ROWS: usize = 12;
/// Lines of hover text shown per part.
pub const HOVER_LINES: usize = 20;
pub const POPUP_PAD: f32 = 4.0;
pub const POPUP_MAX_W: f32 = 600.0;

/// A popup's place in the last record (for pointer input).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Popup {
    Suggest { rect: Rect, top: usize },
    Hover(Rect),
    ParameterHints(Rect),
}

impl Popup {
    pub fn rect(&self) -> Rect {
        match *self {
            Popup::Suggest { rect, .. } | Popup::Hover(rect) | Popup::ParameterHints(rect) => rect,
        }
    }
}
