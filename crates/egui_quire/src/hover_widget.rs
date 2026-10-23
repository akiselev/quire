//! The content hover (VS Code's `ContentHoverWidget`): the view's
//! [`HoverResult`](quire_core::hover::HoverResult) drawn above (or below) the anchor, with
//! diagnostics first, then the providers' Markdown, each part with a copy button. Placement
//! follows `computeHoverPositions`; the mouse policy of `contentHoverController.ts` (sticky
//! hover, hiding delay) is in `editor.rs`.

use egui::accesskit::Role;
use egui::{Area, Frame, Id, Order, Pos2, Rect, RichText, ScrollArea, Ui, UiBuilder, pos2, vec2};
use quire_core::hover::{HoverPart, HoverScroll, LineCol, compute_hover_positions};

use crate::editor::{EditorEvent, FrameCx};
use crate::glue;
use crate::markdown;
use crate::theme::EditorText;

/// Id of the hover (for hosts and tests).
pub fn hover_id(editor: Id) -> Id {
    editor.with("quire.hover")
}

/// VS Code's hover is at most 500 px wide and as tall as allowed (`editor.hover` max height:
/// a quarter of the editor, at least 250 px).
const MAX_WIDTH: f32 = 500.0;

impl FrameCx<'_> {
    /// Screen rectangle of the row holding `pos` and the x of `pos`, plus the visual row's
    /// start column (for `computeHoverPositions`).
    pub(crate) fn pos_row(&mut self, pos: usize) -> (Rect, f32, usize) {
        let text = glue::text(self.doc);
        let pos = pos.min(text.len_bytes());
        let line = text.byte_to_line(pos);
        let ls = text.line_to_byte(line);
        let vl = self.layout_line(line);
        let (row, x) = vl.locate(pos - ls, false);
        let row_start = vl.row_start(row);
        let top = self.geom.line_top(self.mem, line) + row as f32 * self.geom.row_h;
        let left = self.geom.text_left(self.mem);
        let rect = Rect::from_min_size(
            pos2(left, top),
            vec2(self.geom.text.width(), self.geom.row_h),
        );
        (rect, left + x, row_start)
    }
}

fn line_col(f: &FrameCx<'_>, pos: usize) -> LineCol {
    let text = glue::text(f.doc);
    let (line, col) = text.byte_to_line_col(pos.min(text.len_bytes()));
    LineCol::new(line, col)
}

pub(crate) fn show(f: &mut FrameCx<'_>, ui: &Ui, text: &EditorText) {
    let Some(result) = f.view.hover().shown().cloned() else {
        f.mem.hover_rect = None;
        return;
    };
    let focused = f.view.hover().is_focused();
    let a = result.anchor.range.clone();
    let (row, _, row_start) = f.pos_row(a.start);
    let parts: Vec<_> = result
        .parts
        .iter()
        .map(|p| {
            let r = p.range();
            (line_col(f, r.start)..line_col(f, r.end), false)
        })
        .collect();
    let (show_at, secondary) =
        compute_hover_positions(line_col(f, a.start)..line_col(f, a.end), row_start, &parts);
    let text_ref = glue::text(f.doc);
    let show_pos = text_ref.line_to_byte(show_at.line) + show_at.col;
    let sec_pos = text_ref.line_to_byte(secondary.line) + secondary.col;
    let (_, x_show, _) = f.pos_row(show_pos);
    let (_, x_sec, _) = f.pos_row(sec_pos);

    let screen = f.ctx.content_rect();
    let max_h = (f.geom.rect.height() / 4.0).max(250.0);
    // The size of the last frame decides the placement (the first frame measures).
    let size = f
        .mem
        .hover_size
        .unwrap_or(vec2(MAX_WIDTH * 0.6, f.geom.row_h * 3.0));
    let above_fits = row.top() - size.y >= screen.top();
    let below_fits = row.bottom() + size.y <= screen.bottom();
    let above = if f.doc.config().hover.above {
        above_fits || !below_fits
    } else {
        !below_fits && above_fits
    };
    // showAtPosition, or the secondary position when the hover would leave the screen.
    let mut x = x_show;
    if x + size.x > screen.right() {
        x = x_sec;
    }
    x = x.min(screen.right() - size.x).max(screen.left());
    let pos = if above {
        pos2(x, row.top() - size.y)
    } else {
        pos2(x, row.bottom())
    };

    let scroll_req = f.view.take_hover_scroll();
    let visuals = ui.visuals().clone();
    let style = markdown::Style {
        code_font: f.theme.font.clone(),
        code_bg: visuals.code_bg_color,
        dim: visuals.weak_text_color(),
    };
    let editor_id = f.id;
    let mut link: Option<String> = None;
    let mut action: Option<(String, String)> = None;
    let mut copy: Option<String> = None;
    let row_h = f.geom.row_h;
    let label = result
        .parts
        .iter()
        .map(HoverPart::accessible_content)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let theme = f.theme;
    let mut offset = f.mem.hover_offset;
    let inner = Area::new(hover_id(editor_id))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .constrain(false)
        .show(f.ctx, |ui| {
            let mut frame = Frame::popup(ui.style()).inner_margin(6.0);
            if focused {
                frame = frame.stroke(visuals.selection.stroke);
            }
            frame.show(ui, |ui| {
                let mut tip = ui.new_child(
                    UiBuilder::new()
                        .id_salt("quire.hover.content")
                        .accessibility_role(Role::Tooltip)
                        .max_rect(ui.available_rect_before_wrap()),
                );
                let tid = tip.unique_id();
                f.ctx.accesskit_node_builder(tid, |n| {
                    n.set_role(Role::Tooltip);
                    n.set_label(label.clone());
                });
                tip.set_max_width(MAX_WIDTH);
                let mut sa = ScrollArea::both()
                    .id_salt("quire.hover.scroll")
                    .max_height(max_h)
                    .max_width(MAX_WIDTH)
                    .auto_shrink([true, true]);
                if let Some(s) = scroll_req {
                    let page = max_h - row_h;
                    match s {
                        HoverScroll::Up => offset.y -= row_h,
                        HoverScroll::Down => offset.y += row_h,
                        HoverScroll::Left => offset.x -= row_h,
                        HoverScroll::Right => offset.x += row_h,
                        HoverScroll::PageUp => offset.y -= page,
                        HoverScroll::PageDown => offset.y += page,
                        HoverScroll::Top => offset.y = 0.0,
                        HoverScroll::Bottom => offset.y = f32::MAX / 4.0,
                    }
                    offset = offset.max(egui::Vec2::ZERO);
                    sa = sa.scroll_offset(offset);
                }
                let out = sa.show(&mut tip, |ui| {
                    for (i, part) in result.parts.iter().enumerate() {
                        if i > 0 {
                            ui.separator();
                        }
                        ui.push_id(i, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| match part {
                                    HoverPart::Diagnostic {
                                        diagnostic: d,
                                        source,
                                        ..
                                    } => {
                                        let c = match d.severity {
                                            glue::Severity::Error => theme.error,
                                            glue::Severity::Warning => theme.warning,
                                            glue::Severity::Info => theme.info,
                                            glue::Severity::Hint => theme.hint,
                                        };
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(&d.message).color(c));
                                            let src = match &d.code {
                                                Some(code) => {
                                                    format!("{}({code})", source.as_str())
                                                }
                                                None => source.as_str().to_string(),
                                            };
                                            ui.label(RichText::new(src).color(style.dim));
                                        });
                                        if !d.actions.is_empty() {
                                            ui.horizontal(|ui| {
                                                for a in &d.actions {
                                                    if ui.button(&a.title).clicked() {
                                                        action = Some((
                                                            a.command_id.clone(),
                                                            a.args_json.clone(),
                                                        ));
                                                    }
                                                }
                                            });
                                        }
                                    }
                                    HoverPart::Markdown { contents, .. } => {
                                        for (k, c) in contents.iter().enumerate() {
                                            if k > 0 {
                                                ui.separator();
                                            }
                                            if let Some(h) = markdown::show(ui, c, &style) {
                                                link = Some(h);
                                            }
                                        }
                                    }
                                    HoverPart::Loading { .. } => {
                                        ui.label(&text.hover_loading);
                                    }
                                });
                                let content = part.accessible_content();
                                if !content.is_empty()
                                    && ui.small_button(&text.hover_copy).clicked()
                                {
                                    copy = Some(content);
                                }
                            });
                        });
                    }
                });
                offset = out.state.clamped_offset();
                ui.allocate_space(tip.min_rect().size());
            });
        });
    f.mem.hover_offset = offset;
    let rect = inner.response.rect;
    f.mem.hover_rect = Some(rect);
    f.mem.hover_size = Some(rect.size());
    // A click on the hover's buttons takes egui's focus; the editor keeps it.
    if (link.is_some() || action.is_some() || copy.is_some()) && f.mem.focused {
        f.ctx.memory_mut(|m| m.request_focus(editor_id));
    }
    if let Some(href) = link {
        f.out.events.push(EditorEvent::HoverLinkActivated { href });
    }
    if let Some((command_id, args)) = action {
        let args = serde_json::from_str(&args).unwrap_or(serde_json::Value::Null);
        f.out
            .events
            .push(EditorEvent::DiagnosticAction { command_id, args });
    }
    if let Some(c) = copy {
        f.ctx.copy_text(c);
    }
}

/// Whether `p` is on the hover drawn last frame (VS Code `isMousePositionWithinElement`).
pub(crate) fn contains(f: &FrameCx<'_>, p: Pos2) -> bool {
    f.mem.hover_rect.is_some_and(|r| {
        quire_core::hover::is_position_within(r.left(), r.top(), r.width(), r.height(), p.x, p.y)
    })
}
