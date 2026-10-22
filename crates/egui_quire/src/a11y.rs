//! AccessKit: the editor node (`Role::MultilineTextInput`, host-supplied name) with `TextRun`
//! children for a page of lines around the caret (50 lines, 500 when a screen reader is active),
//! following egui 0.37's `text_selection/accesskit_text.rs`. The page counts visible lines only:
//! lines hidden by folds get no run. It shifts when the caret leaves it. A line's runs are its
//! visual rows (soft wrap), each split into runs of at most 255 chars (AccessKit's limit). Run
//! ids: `Id::unique((editor_id, "run", line))` for a line's first run and
//! `Id::unique((editor_id, "run", line, k))` for run `k > 0`.

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
    // Pages are counted in visible rows: hidden (folded) lines have zero height, every visible
    // line one row, so a line's row is y / row_h and the line at row r is line_at(mid-row r).
    let row_h = f.geom.row_h;
    let row_of = |line: usize| (f.mem.heights.y_of(line) / row_h).round() as usize;
    let head_row = row_of(head_line);
    let start_row = row_of(f.mem.a11y_page_start);
    if head_row < start_row || head_row >= start_row + n {
        let row = (head_row / n) * n;
        f.mem.a11y_page_start = f.mem.heights.line_at((row as f32 + 0.5) * row_h);
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
    let first_row = row_of(f.mem.a11y_page_start);
    let rows = (f.mem.heights.total() / row_h).round() as usize;
    let mut page: Vec<usize> = (first_row..(first_row + n).min(rows))
        .map(|r| f.mem.heights.line_at((r as f32 + 0.5) * row_h))
        .filter(|&l| l < lines)
        .collect();
    // A wrapped line spans several rows.
    page.dedup();

    let to_global = f
        .ctx
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or_default();
    let left = f.geom.text_left(f.mem);

    // Runs of each page line: a wrapped line's rows are split into runs of at most 255 chars.
    let mut runs: Vec<LineRuns> = Vec::with_capacity(page.len());
    for &line in &page {
        let vl = f.layout_line(line);
        let top = f.geom.line_top(f.mem, line);
        let last_line = line + 1 >= lines;
        let mut out = LineRuns {
            line,
            runs: Vec::new(),
        };
        let mut char0 = 0usize;
        for k in 0..vl.n_rows() {
            let row = vl.row_layout(k);
            let x0 = vl.row_x0(k);
            let char_xs = row.char_xs();
            let mut chars: Vec<(char, f32, f32)> = row
                .text()
                .chars()
                .enumerate()
                .map(|(i, c)| (c, x0 + char_xs[i], char_xs[i + 1] - char_xs[i]))
                .collect();
            if k + 1 == vl.n_rows() && !last_line {
                chars.push(('\n', x0 + row.width(), 0.0));
            }
            let row_top = top + k as f32 * row_h;
            let pieces = chars.len().div_ceil(MAX_CHARS).max(1);
            for p in 0..pieces {
                let c0 = p * MAX_CHARS;
                let c1 = (c0 + MAX_CHARS).min(chars.len());
                out.runs.push(Run {
                    char_start: char0 + c0,
                    top: row_top,
                    chars: chars[c0..c1].to_vec(),
                });
            }
            char0 += row.text().chars().count();
        }
        runs.push(out);
    }

    // Selection ends mapped to run positions.
    let pos_of = |b: usize| -> Option<accesskit::TextPosition> {
        let text = glue::text(f.doc);
        let b = b.min(text.len_bytes());
        let line = text.byte_to_line(b);
        let lr = runs.iter().find(|r| r.line == line)?;
        let col_chars = text.slice(text.line_to_byte(line)..b).chars().count();
        // The run holding the char; at a run boundary the earlier run's end.
        let k = lr
            .runs
            .iter()
            .rposition(|r| r.char_start < col_chars)
            .unwrap_or(0);
        let r = &lr.runs[k];
        Some(accesskit::TextPosition {
            node: run_id(id, line, k).accesskit_id(),
            character_index: (col_chars - r.char_start.min(col_chars)).min(r.chars.len()),
        })
    };
    let anchor = pos_of(sel.anchor);
    let focus = pos_of(sel.head);
    if let (Some(anchor), Some(focus)) = (anchor, focus) {
        f.ctx.accesskit_node_builder(id, |n| {
            n.set_text_selection(accesskit::TextSelection { anchor, focus })
        });
    }

    for lr in &runs {
        let count = lr.runs.len();
        for (k, run) in lr.runs.iter().enumerate() {
            let rid = run_id(id, lr.line, k);
            let x0 = run.chars.first().map_or(0.0, |c| c.1);
            let x1 = run.chars.last().map_or(x0, |c| c.1 + c.2);
            let rect =
                Rect::from_min_size(pos2(left + x0, run.top), vec2((x1 - x0).max(1.0), row_h));
            // A child Ui with an explicit id and the editor as accessibility parent gives the run
            // node its place in the tree (egui's register_accesskit_parent is crate-private).
            let _child = ui.new_child(
                UiBuilder::new()
                    .scope_id(rid)
                    .accessibility_parent(id)
                    .accessibility_role(Role::TextRun)
                    .max_rect(rect),
            );
            let mut value = String::new();
            let mut lens = Vec::with_capacity(run.chars.len());
            let mut word_starts = Vec::new();
            let mut was_word_end = false;
            for (i, (c, _, _)) in run.chars.iter().enumerate() {
                let is_word = c.is_alphanumeric() || *c == '_';
                if is_word && was_word_end {
                    word_starts.push(i as u8);
                }
                was_word_end = !is_word;
                lens.push(c.len_utf8() as u8);
                value.push(*c);
            }
            let pos: Vec<f32> = run.chars.iter().map(|c| c.1 - x0).collect();
            let wid: Vec<f32> = run.chars.iter().map(|c| c.2).collect();
            let g = to_global * rect;
            let line = lr.line;
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
                n.set_word_starts(word_starts);
                if k > 0 {
                    n.set_previous_on_line(run_id(id, line, k - 1).accesskit_id());
                }
                if k + 1 < count {
                    n.set_next_on_line(run_id(id, line, k + 1).accesskit_id());
                }
            });
        }
    }
}

/// The text runs of one line.
struct LineRuns {
    line: usize,
    runs: Vec<Run>,
}

/// One text run: (char, x, width) with x relative to the text left, on the row at `top`.
struct Run {
    /// Index in the line of the run's first char.
    char_start: usize,
    top: f32,
    chars: Vec<(char, f32, f32)>,
}
