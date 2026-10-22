//! The find / replace bar: UI over quire-core's find (the query, options and matches live in the
//! `EditorView`'s find state; the widget keeps only the bar's open state and input strings).

use egui::{Align, Frame, Key, Layout, Margin, Rect, TextEdit, Ui, UiBuilder, pos2, vec2};

use crate::editor::FrameCx;
use crate::glue::{self, FindOptions};
use crate::theme::EditorText;

/// Widget-side find bar state.
#[derive(Clone, Debug, Default)]
pub(crate) struct FindBar {
    pub open: bool,
    pub replace: bool,
    pub query: String,
    pub replacement: String,
    pub options: FindOptions,
    /// Focus the query input on the next show.
    pub focus_query: bool,
}

impl FindBar {
    pub fn open_bar(&mut self, replace: bool, seed: Option<String>) {
        self.open = true;
        self.replace = replace;
        if let Some(s) = seed {
            self.query = s;
        }
        self.focus_query = true;
    }
}

/// Id of the find query input (for hosts and tests).
pub fn query_id(editor: egui::Id) -> egui::Id {
    editor.with("quire.find.query")
}

/// Id of the replace input.
pub fn replace_id(editor: egui::Id) -> egui::Id {
    editor.with("quire.find.replace")
}

pub(crate) fn show(f: &mut FrameCx<'_>, ui: &mut Ui, text: &EditorText) {
    let g = f.geom;
    let width = (g.text.width() - 16.0).clamp(120.0, 460.0);
    let rows = if f.mem.find.replace { 2.0 } else { 1.0 };
    let height = rows * (ui.spacing().interact_size.y + 4.0) + 8.0;
    let rect = Rect::from_min_size(
        pos2(g.rect.right() - width - 14.0, g.rect.top() + 2.0),
        vec2(width, height),
    );
    let id = f.id;
    let mut child = ui.new_child(
        UiBuilder::new()
            .scope_id(id.with("quire.find"))
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let mut bar = std::mem::take(&mut f.mem.find);
    let before = (bar.query.clone(), bar.options);
    let mut next = false;
    let mut prev = false;
    let mut close = false;
    let mut replace_one = false;
    let mut replace_all = false;
    let (matches, current) = glue::find_matches(f.view, f.doc);
    let count = matches.len();

    Frame::popup(child.style())
        .inner_margin(Margin::same(4))
        .show(&mut child, |ui| {
            ui.horizontal(|ui| {
                let q = ui.add(
                    TextEdit::singleline(&mut bar.query)
                        .id(query_id(id))
                        .hint_text(text.find_placeholder.as_str())
                        .desired_width(width * 0.45),
                );
                ui.ctx()
                    .accesskit_node_builder(q.id, |n| n.set_label(text.find_label.clone()));
                if std::mem::take(&mut bar.focus_query) {
                    q.request_focus();
                }
                if q.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    if ui.input(|i| i.modifiers.shift) {
                        prev = true;
                    } else {
                        next = true;
                    }
                    q.request_focus();
                }
                if (q.has_focus() || q.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
                    close = true;
                }
                ui.toggle_value(&mut bar.options.case_sensitive, "Aa")
                    .on_hover_text(text.match_case.as_str());
                ui.toggle_value(&mut bar.options.whole_word, "ab")
                    .on_hover_text(text.whole_word.as_str());
                ui.toggle_value(&mut bar.options.regex, ".*")
                    .on_hover_text(text.regex.as_str());
                let label = if let Some(err) = glue::find_error(f.view) {
                    err
                } else if count == 0 {
                    text.no_results.clone()
                } else {
                    text.match_count
                        .replace(
                            "{current}",
                            &current.map_or("?".to_owned(), |c| (c + 1).to_string()),
                        )
                        .replace("{total}", &count.to_string())
                };
                ui.label(label);
                if ui
                    .small_button("<")
                    .on_hover_text(text.previous_match.as_str())
                    .clicked()
                {
                    prev = true;
                }
                if ui
                    .small_button(">")
                    .on_hover_text(text.next_match.as_str())
                    .clicked()
                {
                    next = true;
                }
                if ui
                    .small_button("×")
                    .on_hover_text(text.close.as_str())
                    .clicked()
                {
                    close = true;
                }
            });
            if bar.replace {
                ui.horizontal(|ui| {
                    let r = ui.add(
                        TextEdit::singleline(&mut bar.replacement)
                            .id(replace_id(id))
                            .hint_text(text.replace_placeholder.as_str())
                            .desired_width(width * 0.45),
                    );
                    ui.ctx()
                        .accesskit_node_builder(r.id, |n| n.set_label(text.replace_label.clone()));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        replace_one = true;
                        r.request_focus();
                    }
                    if ui.button(text.replace_one.as_str()).clicked() {
                        replace_one = true;
                    }
                    if ui.button(text.replace_all.as_str()).clicked() {
                        replace_all = true;
                    }
                });
            }
        });

    if (bar.query.clone(), bar.options) != before
        || glue::find_query_stale(f.view, &bar.query, bar.options)
    {
        glue::set_find(f.doc, f.view, &bar.query, bar.options);
    }
    f.mem.find = bar;
    if next {
        f.dispatch(glue::find_next_cmd());
    }
    if prev {
        f.dispatch(glue::find_prev_cmd());
    }
    if replace_one {
        let r = f.mem.find.replacement.clone();
        f.dispatch(glue::replace_one_cmd(&r));
    }
    if replace_all {
        let r = f.mem.find.replacement.clone();
        f.dispatch(glue::replace_all_cmd(&r));
    }
    if close {
        f.mem.find.open = false;
        glue::clear_find(f.doc, f.view);
        f.ctx.memory_mut(|m| m.request_focus(id));
    }
}
