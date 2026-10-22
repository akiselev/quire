//! The suggest widget: VS Code's completion list under the caret, with the details panel at its
//! side. The list's state (items, focus, scroll, details) is the view's
//! [`SuggestState`](quire_core::completion::SuggestState); this module only draws it and turns
//! clicks and wheel turns into view calls.

use egui::accesskit::Role;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Area, Color32, FontId, Frame, Id, Order, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use quire_core::EditorCommand;
use quire_core::completion::{
    CompletionItemKind as K, FilteredItem, VISIBLE_ROWS, WidgetState, filters::create_matches,
};

use crate::editor::FrameCx;
use crate::theme::EditorText;

/// Id of the list (for hosts and tests).
pub fn list_id(editor: Id) -> Id {
    editor.with("quire.suggest.list")
}

/// Id of the details panel.
pub fn details_id(editor: Id) -> Id {
    editor.with("quire.suggest.details")
}

/// VS Code shows "Loading..." only after this long (manual trigger).
const LOADING_DELAY: f64 = 0.05;

fn kind_glyph(k: K) -> (char, Color32) {
    let purple = Color32::from_rgb(0xb1, 0x80, 0xd7);
    let blue = Color32::from_rgb(0x75, 0xbe, 0xff);
    let orange = Color32::from_rgb(0xee, 0x9d, 0x28);
    let grey = Color32::from_rgb(0x9a, 0x9a, 0x9a);
    match k {
        K::Method => ('m', purple),
        K::Function => ('f', purple),
        K::Constructor => ('c', purple),
        K::Field => ('F', blue),
        K::Variable => ('v', blue),
        K::Property => ('p', grey),
        K::Class => ('C', orange),
        K::Struct => ('S', blue),
        K::Interface => ('I', blue),
        K::Module => ('M', grey),
        K::Event => ('e', orange),
        K::Operator => ('o', grey),
        K::Unit => ('u', grey),
        K::Value => ('V', blue),
        K::Constant => ('k', grey),
        K::Enum => ('E', orange),
        K::EnumMember => ('e', blue),
        K::Keyword => ('K', grey),
        K::Text => ('t', grey),
        K::Color => ('#', grey),
        K::File => ('d', grey),
        K::Reference => ('r', grey),
        K::Customcolor => ('#', grey),
        K::Folder => ('D', grey),
        K::TypeParameter => ('T', grey),
        K::User => ('U', grey),
        K::Issue => ('!', grey),
        K::Tool => ('x', grey),
        K::Snippet => ('s', grey),
    }
}

/// The label with its matched characters highlighted (VS Code's
/// `editorSuggestWidget.highlightForeground`), then the label detail, dimmed.
fn label_job(f: &FilteredItem, font: &FontId, fg: Color32, hl: Color32, dim: Color32) -> LayoutJob {
    let c = &f.item.completion;
    let mut job = LayoutJob::default();
    let matches = create_matches(Some(&f.score));
    let mut u16pos = 0usize;
    let strike = if c.deprecated {
        Stroke::new(1.0, fg)
    } else {
        Stroke::NONE
    };
    for ch in c.label.chars() {
        let hit = matches.iter().any(|m| m.contains(&u16pos));
        u16pos += ch.len_utf16();
        let mut buf = [0u8; 4];
        job.append(
            ch.encode_utf8(&mut buf),
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: if hit { hl } else { fg },
                strikethrough: strike,
                ..Default::default()
            },
        );
    }
    if let Some(d) = &c.label_detail {
        job.append(
            d,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: dim,
                ..Default::default()
            },
        );
    }
    job
}

pub(crate) fn show(f: &mut FrameCx<'_>, ui: &mut Ui, caret: Option<egui::Rect>, text: &EditorText) {
    let state = f.view.suggest().widget_state();
    let Some(caret) = caret else {
        return;
    };
    if state == WidgetState::Hidden {
        f.mem.suggest_loading_since = None;
        return;
    }
    let now = f.ctx.input(|i| i.time);
    let id = f.id;
    if state == WidgetState::Loading {
        let sid = f.view.suggest().session().map(|s| s.id);
        match f.mem.suggest_loading_since {
            Some((s, t)) if Some(s) == sid => {
                if now - t < LOADING_DELAY {
                    f.ctx
                        .request_repaint_after(std::time::Duration::from_secs_f64(LOADING_DELAY));
                    return;
                }
            }
            _ => {
                if let Some(s) = sid {
                    f.mem.suggest_loading_since = Some((s, now));
                }
                f.ctx
                    .request_repaint_after(std::time::Duration::from_secs_f64(LOADING_DELAY));
                return;
            }
        }
    }
    let font = f.theme.font.clone();
    let row_h = (f.geom.row_h + 4.0).round();
    let adv = f.geom.adv;
    let visuals = ui.visuals().clone();
    let fg = visuals.text_color();
    let dim = visuals.weak_text_color();
    let hl = visuals.hyperlink_color;
    let st = f.view.suggest();
    let items: Vec<FilteredItem> = st.items().to_vec();
    let focused = st.focused();
    let top = st.scroll_top();
    let details = st.details_visible();
    let inline_details = f.doc.config().suggest.show_inline_details && !details;

    let icon_w = row_h;
    let longest = items
        .iter()
        .map(|i| {
            let c = &i.item.completion;
            c.label.chars().count()
                + c.label_detail.as_ref().map_or(0, |d| d.chars().count())
                + c.label_description
                    .as_ref()
                    .map_or(0, |d| d.chars().count() + 2)
        })
        .max()
        .unwrap_or(16);
    let width = (icon_w + 12.0 + longest as f32 * adv + 24.0).clamp(240.0, 560.0);
    let rows = match state {
        WidgetState::Open | WidgetState::Frozen => items.len().min(VISIBLE_ROWS),
        _ => 1,
    };
    let height = rows as f32 * row_h + 4.0;
    // Below the caret, above when it does not fit (VS Code), clamped into the editor.
    let bounds = f.geom.rect;
    let mut pos = pos2(caret.left() - icon_w - 4.0, caret.bottom());
    if pos.y + height > bounds.bottom() && caret.top() - height >= bounds.top() {
        pos.y = caret.top() - height;
    }
    pos.x = pos
        .x
        .clamp(bounds.left(), (bounds.right() - width).max(bounds.left()));

    let mut clicked: Option<usize> = None;
    let mut wheel: Option<isize> = None;
    let area = Area::new(id.with("quire.suggest"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .constrain(false);
    let list_rect = area
        .show(f.ctx, |ui| {
            Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
                ui.set_width(width);
                let mut list = ui.new_child(
                    UiBuilder::new()
                        .id_salt("quire.suggest.list")
                        .accessibility_role(Role::ListBox)
                        .max_rect(Rect::from_min_size(
                            ui.cursor().min,
                            vec2(width, rows as f32 * row_h),
                        )),
                );
                let lid = list.unique_id();
                f.ctx.accesskit_node_builder(lid, |n| {
                    n.set_role(Role::ListBox);
                    n.set_label(text.suggest_label.clone());
                });
                match state {
                    WidgetState::Loading | WidgetState::Empty => {
                        let msg = if state == WidgetState::Loading {
                            &text.suggest_loading
                        } else {
                            &text.suggest_empty
                        };
                        let (r, _) = list.allocate_exact_size(vec2(width, row_h), Sense::hover());
                        list.painter().text(
                            pos2(r.left() + 6.0, r.center().y),
                            Align2::LEFT_CENTER,
                            msg,
                            font.clone(),
                            dim,
                        );
                    }
                    _ => {
                        for (row, it) in items.iter().enumerate().skip(top).take(VISIBLE_ROWS) {
                            let (r, resp) =
                                list.allocate_exact_size(vec2(width, row_h), Sense::click());
                            let is_focus = Some(row) == focused;
                            if is_focus {
                                list.painter()
                                    .rect_filled(r, 3.0, visuals.selection.bg_fill);
                            }
                            let c = &it.item.completion;
                            let (g, col) = kind_glyph(c.kind);
                            list.painter().text(
                                pos2(r.left() + icon_w * 0.5, r.center().y),
                                Align2::CENTER_CENTER,
                                g,
                                font.clone(),
                                col,
                            );
                            let job = label_job(it, &font, fg, hl, dim);
                            let galley = list.fonts_mut(|fo| fo.layout_job(job));
                            let label_x = r.left() + icon_w + 4.0;
                            list.painter().galley(
                                pos2(label_x, r.center().y - galley.size().y * 0.5),
                                galley.clone(),
                                fg,
                            );
                            // Right side: the label description, or the focused row's detail
                            // while the details panel is closed (showInlineDetails).
                            let right = if is_focus && inline_details && c.detail.is_some() {
                                c.detail.clone()
                            } else {
                                c.label_description.clone()
                            };
                            if let Some(rt) = right {
                                let max = (r.right() - 6.0) - (label_x + galley.size().x + 12.0);
                                if max > adv * 3.0 {
                                    let n = (max / adv) as usize;
                                    let shown: String = if rt.chars().count() > n {
                                        rt.chars().take(n.saturating_sub(1)).chain(['…']).collect()
                                    } else {
                                        rt
                                    };
                                    list.painter().text(
                                        pos2(r.right() - 6.0, r.center().y),
                                        Align2::RIGHT_CENTER,
                                        shown,
                                        font.clone(),
                                        dim,
                                    );
                                }
                            }
                            let label = c.label.clone();
                            f.ctx.accesskit_node_builder(resp.id, |n| {
                                n.set_bounds(egui::accesskit::Rect {
                                    x0: r.min.x.into(),
                                    y0: r.min.y.into(),
                                    x1: r.max.x.into(),
                                    y1: r.max.y.into(),
                                });
                                n.set_role(Role::ListBoxOption);
                                n.set_label(label);
                                n.set_selected(is_focus);
                            });
                            // On press: a click elsewhere would take focus from the
                            // editor (and close the list) before the release.
                            if resp.hovered() && list.input(|i| i.pointer.primary_pressed()) {
                                clicked = Some(row);
                            }
                        }
                        let resp =
                            list.interact(list.min_rect(), lid.with("wheel"), Sense::hover());
                        if resp.hovered() {
                            let dy = list.input(|i| i.smooth_scroll_delta.y);
                            if dy.abs() > 0.5 {
                                wheel = Some(if dy > 0.0 { -1 } else { 1 });
                            }
                        }
                    }
                }
                ui.allocate_space(vec2(width, rows as f32 * row_h));
            });
        })
        .response
        .rect;

    // The details panel at the side (VS Code: right of the list, left if it does not fit).
    if details
        && let Some(row) = focused
        && let Some(it) = items.get(row)
    {
        let c = &it.item.completion;
        if c.detail.is_some() || c.documentation.is_some() {
            let dw = 320.0f32;
            let right_fits = list_rect.right() + dw <= f.ctx.content_rect().right();
            let dpos = if right_fits {
                pos2(list_rect.right(), list_rect.top())
            } else {
                pos2(list_rect.left() - dw, list_rect.top())
            };
            Area::new(details_id(id))
                .order(Order::Foreground)
                .fixed_pos(dpos)
                .constrain(false)
                .show(f.ctx, |ui| {
                    Frame::popup(ui.style()).inner_margin(6.0).show(ui, |ui| {
                        ui.set_width(dw - 12.0);
                        ui.set_max_height((list_rect.height() * 1.5).max(120.0));
                        if let Some(d) = &c.detail {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(d).font(font.clone()).color(dim),
                                )
                                .wrap(),
                            );
                        }
                        if let Some(doc) = &c.documentation {
                            ui.add(egui::Label::new(doc.as_str()).wrap());
                        }
                    });
                });
        }
    }

    if let Some(w) = wheel {
        let t = f.view.suggest().scroll_top() as isize + w;
        f.view.suggest_scroll_to(t.max(0) as usize);
    }
    if let Some(row) = clicked {
        // VS Code: a click selects and inserts the item (here on press); the editor keeps focus.
        f.view.suggest_focus_row(row);
        f.dispatch(EditorCommand::AcceptSelectedSuggestion);
        f.ctx.memory_mut(|m| m.request_focus(id));
    }
}
