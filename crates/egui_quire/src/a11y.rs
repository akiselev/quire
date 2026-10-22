//! AccessKit: the editor node (`Role::MultilineTextInput`, host-supplied name) with `TextRun`
//! children for a page of lines around the caret (50 lines, 500 when a screen reader is active),
//! following egui 0.37's `text_selection/accesskit_text.rs`. The page shifts when the caret
//! leaves it. Run ids: `Id::unique((editor_id, "run", line))` for a line's first chunk and
//! `Id::unique((editor_id, "run", line, k))` for chunk `k > 0` (AccessKit runs hold ≤ 255 chars).

use egui::accesskit::{self, Role};
use egui::{Id, Rect, Ui, UiBuilder, pos2, vec2};

use crate::editor::FrameCx;
use crate::glue;
use crate::theme::EditorText;

const MAX_CHARS: usize = 255;

/// Page size: 50 lines, 500 with a screen reader.
pub fn page_size(ui: &Ui) -> usize {
    if ui.memory(|m| m.options.screen_reader) {
        500
    } else {
        50
    }
}

/// Id of the text run for `line`, chunk `chunk`.
pub fn run_id(editor: Id, line: usize, chunk: usize) -> Id {
    if chunk == 0 {
        Id::unique((editor, "run", line))
    } else {
        Id::unique((editor, "run", line, chunk))
    }
}

/// Id of the live-region node that announces page shifts.
fn announce_id(editor: Id) -> Id {
    Id::unique((editor, "announce"))
}

pub(crate) fn update(f: &mut FrameCx<'_>, ui: &mut Ui, name: &str, strings: &EditorText) {
    let id = f.id;
    let enabled = f
        .ctx
        .accesskit_node_builder(id, |n| {
            n.set_role(Role::MultilineTextInput);
            n.set_label(name.to_owned());
            n.add_action(accesskit::Action::Focus);
            n.add_action(accesskit::Action::SetValue);
            n.add_action(accesskit::Action::ReplaceSelectedText);
        })
        .is_some();
    if !enabled {
        return;
    }

    let n = page_size(ui);
    let text = glue::text(f.doc);
    let lines = text.len_lines().max(1);
    let sel = glue::selection(f.view).primary();
    let head_line = text.byte_to_line(sel.head.min(text.len_bytes()));
    if head_line < f.mem.a11y_page_start || head_line >= f.mem.a11y_page_start + n {
        f.mem.a11y_page_start = (head_line / n) * n;
        // The runs a screen reader was reading are gone: say where the caret is now.
        f.mem.a11y_announce = Some(
            strings
                .line_announcement
                .replace("{line}", &(head_line + 1).to_string()),
        );
    }
    if let Some(msg) = f.mem.a11y_announce.clone() {
        let aid = announce_id(id);
        let _child = ui.new_child(
            UiBuilder::new()
                .scope_id(aid)
                .accessibility_parent(id)
                .accessibility_role(Role::Status)
                .max_rect(Rect::from_min_size(f.geom.text.min, vec2(1.0, 1.0))),
        );
        f.ctx.accesskit_node_builder(aid, |n| {
            n.set_role(Role::Status);
            n.set_live(accesskit::Live::Polite);
            n.set_label(msg);
        });
    }
    let page = f.mem.a11y_page_start..(f.mem.a11y_page_start + n).min(lines);

    let to_global = f
        .ctx
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or_default();
    let left = f.geom.text_left(f.mem);
    let row_h = f.geom.row_h;

    // (line, chunk start chars) of the selection ends, mapped to run positions.
    let pos_of = |b: usize| -> Option<accesskit::TextPosition> {
        let text = glue::text(f.doc);
        let b = b.min(text.len_bytes());
        let line = text.byte_to_line(b);
        if !page.contains(&line) {
            return None;
        }
        let col_chars = text
            .slice(text.line_to_byte(line)..b)
            .to_string()
            .chars()
            .count();
        let chunk = if col_chars > 0 && col_chars.is_multiple_of(MAX_CHARS) {
            col_chars / MAX_CHARS - 1
        } else {
            col_chars / MAX_CHARS
        };
        Some(accesskit::TextPosition {
            node: run_id(id, line, chunk).accesskit_id(),
            character_index: col_chars - chunk * MAX_CHARS,
        })
    };
    let anchor = pos_of(sel.anchor);
    let focus = pos_of(sel.head);
    if let (Some(anchor), Some(focus)) = (anchor, focus) {
        f.ctx.accesskit_node_builder(id, |n| {
            n.set_text_selection(accesskit::TextSelection { anchor, focus })
        });
    }

    for line in page.clone() {
        let layout = f.layout_line(line);
        let top = f.geom.line_top(f.mem, line);
        let last = line + 1 >= lines;
        let char_xs = layout.char_xs();
        let mut value = String::new();
        let mut lens: Vec<u8> = Vec::new();
        let mut xs: Vec<f32> = Vec::new();
        let mut ws: Vec<f32> = Vec::new();
        let mut word_starts: Vec<usize> = Vec::new();
        let mut was_word_end = false;
        for (i, chr) in layout.text().chars().enumerate() {
            let is_word = chr.is_alphanumeric() || chr == '_';
            if is_word && was_word_end {
                word_starts.push(lens.len());
            }
            was_word_end = !is_word;
            let before = value.len();
            value.push(chr);
            lens.push((value.len() - before) as u8);
            xs.push(char_xs[i]);
            ws.push(char_xs[i + 1] - char_xs[i]);
        }
        if !last {
            value.push('\n');
            lens.push(1);
            xs.push(layout.width());
            ws.push(0.0);
        }
        let chunks = lens.len().div_ceil(MAX_CHARS).max(1);
        let mut byte = 0usize;
        for k in 0..chunks {
            let c0 = k * MAX_CHARS;
            let c1 = (c0 + MAX_CHARS).min(lens.len());
            let b0 = byte;
            let b1 = b0 + lens[c0..c1].iter().map(|&l| l as usize).sum::<usize>();
            byte = b1;
            let rid = run_id(id, line, k);
            let x0 = xs.get(c0).copied().unwrap_or(0.0);
            let x1 = if c1 > c0 { xs[c1 - 1] + ws[c1 - 1] } else { x0 };
            let rect = Rect::from_min_size(pos2(left + x0, top), vec2((x1 - x0).max(1.0), row_h));
            // A child Ui with an explicit id and the editor as accessibility parent gives the run
            // node its place in the tree (egui's register_accesskit_parent is crate-private).
            let _child = ui.new_child(
                UiBuilder::new()
                    .scope_id(rid)
                    .accessibility_parent(id)
                    .accessibility_role(Role::TextRun)
                    .max_rect(rect),
            );
            let value = value[b0..b1].to_owned();
            let lens = lens[c0..c1].to_vec();
            let pos: Vec<f32> = xs[c0..c1].iter().map(|x| x - x0).collect();
            let wid = ws[c0..c1].to_vec();
            let starts: Vec<u8> = word_starts
                .iter()
                .filter(|&&w| w >= c0 && w < c1)
                .map(|&w| (w - c0) as u8)
                .collect();
            let g = to_global * rect;
            f.ctx.accesskit_node_builder(rid, |n| {
                n.set_role(Role::TextRun);
                n.set_text_direction(accesskit::TextDirection::LeftToRight);
                n.set_bounds(accesskit::Rect {
                    x0: g.min.x.into(),
                    y0: g.min.y.into(),
                    x1: g.max.x.into(),
                    y1: g.max.y.into(),
                });
                n.set_value(value);
                n.set_character_lengths(lens);
                n.set_character_positions(pos);
                n.set_character_widths(wid);
                n.set_word_starts(starts);
                if k > 0 {
                    n.set_previous_on_line(run_id(id, line, k - 1).accesskit_id());
                }
                if k + 1 < chunks {
                    n.set_next_on_line(run_id(id, line, k + 1).accesskit_id());
                }
            });
        }
    }
}
