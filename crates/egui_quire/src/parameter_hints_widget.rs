//! The parameter hints widget (VS Code's `ParameterHintsWidget`): the active signature above the
//! cursor (below when it does not fit), its active parameter highlighted, the overload counter
//! with up/down arrows when there are several, and the parameter's and signature's
//! documentation. The state is the view's
//! [`ParameterHintsState`](quire_core::parameter_hints::ParameterHintsState).

use egui::accesskit::{Live, Role};
use egui::text::{LayoutJob, TextFormat};
use egui::{Area, Frame, Id, Order, Rect, RichText, Ui, UiBuilder, pos2, vec2};
use quire_core::EditorCommand;
use quire_core::parameter_hints::{Documentation, SignatureHelp};

use crate::editor::{EditorEvent, FrameCx};
use crate::markdown;
use crate::theme::EditorText;

/// Id of the widget (for hosts and tests).
pub fn parameter_hints_id(editor: Id) -> Id {
    editor.with("quire.parameter_hints")
}

/// The text a screen reader announces for the active parameter (VS Code: "{label}, {param
/// doc}, {signature doc}, hint").
pub(crate) fn announcement(h: &SignatureHelp, text: &EditorText) -> Option<String> {
    let sig = h.signatures.get(h.active_signature)?;
    let idx = h.active_parameter_of(sig);
    let param = sig.parameters.get(idx)?;
    let r = sig.parameter_label_offsets(idx);
    let mut s = match &param.label {
        quire_core::parameter_hints::ParameterLabel::Text(t) => t.clone(),
        quire_core::parameter_hints::ParameterLabel::Offsets(_) => sig.label[r].to_string(),
    };
    if let Some(d) = &param.documentation {
        s.push_str(", ");
        s.push_str(d.as_str());
    }
    if let Some(d) = &sig.documentation {
        s.push_str(", ");
        s.push_str(d.as_str());
    }
    Some(text.parameter_hint.replace("{0}", &s))
}

fn docs(ui: &mut Ui, d: &Documentation, style: &markdown::Style) -> Option<String> {
    match d {
        Documentation::Plain(s) => {
            ui.add(egui::Label::new(s.as_str()).wrap());
            None
        }
        Documentation::Markdown(s) => markdown::show(ui, s, style),
    }
}

pub(crate) fn show(f: &mut FrameCx<'_>, ui: &mut Ui, caret: Option<Rect>, text: &EditorText) {
    let Some(h) = f.view.parameter_hints().active().cloned() else {
        f.mem.parameter_hints_size = None;
        return;
    };
    let Some(caret) = caret else { return };
    // VS Code renders nothing for an out-of-range active signature.
    let Some(sig) = h.signatures.get(h.active_signature).cloned() else {
        return;
    };
    let visuals = ui.visuals().clone();
    let font = f.theme.font.clone();
    let style = markdown::Style {
        code_font: font.clone(),
        code_bg: visuals.code_bg_color,
        dim: visuals.weak_text_color(),
    };
    let multiple = h.signatures.len() > 1;
    let idx = h.active_parameter_of(&sig);
    let range = if sig.parameters.is_empty() {
        0..0
    } else {
        sig.parameter_label_offsets(idx)
    };
    let fg = visuals.text_color();
    // `editorHoverWidget.highlightForeground` (listHighlightForeground).
    let hl = visuals.hyperlink_color;
    let mut job = LayoutJob::default();
    let fmt = |color, underline: bool| TextFormat {
        font_id: font.clone(),
        color,
        underline: if underline {
            egui::Stroke::new(1.0, color)
        } else {
            egui::Stroke::NONE
        },
        ..Default::default()
    };
    job.append(&sig.label[..range.start], 0.0, fmt(fg, false));
    job.append(&sig.label[range.clone()], 0.0, fmt(hl, true));
    job.append(&sig.label[range.end..], 0.0, fmt(fg, false));
    job.wrap.max_width = 480.0;

    let size = f.mem.parameter_hints_size.unwrap_or(vec2(300.0, f.geom.row_h * 2.0));
    let screen = f.ctx.content_rect();
    // Preference ABOVE, then BELOW.
    let y = if caret.top() - size.y >= screen.top() {
        caret.top() - size.y
    } else {
        caret.bottom()
    };
    let x = caret.left().min(screen.right() - size.x).max(screen.left());

    let announce = announcement(&h, text);
    let counter = format!(
        "{:0width$}/{}",
        h.active_signature + 1,
        h.signatures.len(),
        width = h.signatures.len().to_string().len()
    );
    let mut nav: Option<EditorCommand> = None;
    let mut link = None;
    let inner = Area::new(parameter_hints_id(f.id))
        .order(Order::Foreground)
        .fixed_pos(pos2(x, y))
        .constrain(false)
        .show(f.ctx, |ui| {
            Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                let mut tip = ui.new_child(
                    UiBuilder::new()
                        .id_salt("quire.parameter_hints.content")
                        .accessibility_role(Role::Tooltip)
                        .max_rect(ui.available_rect_before_wrap()),
                );
                let tid = tip.unique_id();
                let sig_label = sig.label.clone();
                let ann = announce.clone();
                f.ctx.accesskit_node_builder(tid, |n| {
                    n.set_role(Role::Tooltip);
                    n.set_label(sig_label);
                    if let Some(a) = ann {
                        n.set_description(a);
                        n.set_live(Live::Assertive);
                    }
                });
                tip.set_max_width(500.0);
                tip.horizontal_top(|ui| {
                    if multiple {
                        ui.vertical(|ui| {
                            if ui.small_button("▲").on_hover_text(&text.previous_signature).clicked() {
                                nav = Some(EditorCommand::ShowPrevParameterHint);
                            }
                            ui.label(RichText::new(&counter).color(style.dim).small());
                            if ui.small_button("▼").on_hover_text(&text.next_signature).clicked() {
                                nav = Some(EditorCommand::ShowNextParameterHint);
                            }
                        });
                    }
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(job.clone()));
                        let param_doc = sig.parameters.get(idx).and_then(|p| p.documentation.clone());
                        if param_doc.is_some() || sig.documentation.is_some() {
                            ui.separator();
                        }
                        if let Some(d) = &param_doc
                            && let Some(hr) = docs(ui, d, &style)
                        {
                            link = Some(hr);
                        }
                        if let Some(d) = &sig.documentation
                            && let Some(hr) = docs(ui, d, &style)
                        {
                            link = Some(hr);
                        }
                    });
                });
                ui.allocate_space(tip.min_rect().size());
            });
        });
    f.mem.parameter_hints_size = Some(inner.response.rect.size());
    if let Some(c) = nav {
        f.dispatch(c);
        f.ctx.memory_mut(|m| m.request_focus(f.id));
    }
    if let Some(href) = link {
        f.out.events.push(EditorEvent::HoverLinkActivated { href });
    }
}
