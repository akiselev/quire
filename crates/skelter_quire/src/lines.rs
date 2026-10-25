//! A document line as the editor draws it: the line's text with its injected text (the IME preedit
//! at the primary caret, inlay hints) spliced in, its token paints, and the mapping between model
//! bytes and display bytes.
//!
//! Model bytes are line-local offsets into the document line; display bytes are offsets into
//! [`DisplayLine::text`], which shaping and wrapping see. A model position maps to the display byte
//! before the pieces injected at it, except the preedit, which the caret follows.

use std::ops::Range;

use quire_core::{Document, EditorView, Text};

/// Paint id of inlay hint text ([`crate::EditorTheme::inlay_hint_foreground`]).
pub const PAINT_HINT: u32 = u32::MAX - 1;
/// Paint id of the IME preedit (the foreground, underlined).
pub const PAINT_PREEDIT: u32 = u32::MAX - 2;

/// What an injected piece is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Preedit,
    /// Piece `piece` of inlay hint `hint`.
    Hint {
        hint: usize,
        piece: usize,
    },
}

/// Text shown in a line that is not in the document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    /// The model byte it shows at.
    pub col: usize,
    /// Its display range.
    pub display: Range<usize>,
    pub kind: PieceKind,
}

/// One line ready for shaping.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct DisplayLine {
    pub text: String,
    /// `(display byte, paint)` ascending.
    pub paints: Vec<(usize, u32)>,
    pub pieces: Vec<Piece>,
    /// The model line's length.
    pub model_len: usize,
}

impl DisplayLine {
    /// The display byte of model byte `col` (before pieces at `col`; after a preedit at `col`).
    pub fn to_display(&self, col: usize) -> usize {
        let mut d = col.min(self.model_len);
        for p in &self.pieces {
            if p.col < col || (p.col == col && p.kind == PieceKind::Preedit) {
                d += p.display.len();
            }
        }
        d
    }

    /// The model byte of display byte `d` (inside a piece: the piece's position).
    pub fn to_model(&self, d: usize) -> usize {
        let mut shift = 0;
        for p in &self.pieces {
            if d < p.display.start {
                break;
            }
            if d < p.display.end {
                return p.col;
            }
            shift += p.display.len();
        }
        (d - shift).min(self.model_len)
    }

    /// The piece under display byte `d`.
    pub fn piece_at(&self, d: usize) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.display.contains(&d))
    }
}

/// Line `i` of `text` without its line feed.
pub fn line_string(text: &Text, i: usize) -> String {
    let mut s = text.line(i).to_cow().into_owned();
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// Line `i` without its line feed, cut after `stop` chars (copying only that prefix).
pub fn line_prefix(text: &Text, i: usize, stop: usize) -> String {
    let slice = text.line(i);
    let mut out = String::with_capacity(slice.len_bytes().min(stop * 4));
    let mut chars = 0usize;
    for chunk in slice.chunks() {
        let chunk = chunk.strip_suffix('\n').unwrap_or(chunk);
        match chunk.char_indices().nth(stop - chars) {
            Some((b, _)) => {
                out.push_str(&chunk[..b]);
                return out;
            }
            None => {
                out.push_str(chunk);
                chars += chunk.chars().count();
            }
        }
    }
    out
}

/// The token paints of `line` (`(model byte, paint)`; paint `t + 1` for token type `t`), semantic
/// tokens laid over the highlighter's as egui_quire's `glue::line_runs` does.
pub fn token_paints(doc: &Document, line: usize, out: &mut Vec<(usize, u32)>) {
    out.clear();
    let tokens = doc.tokens();
    if line >= tokens.len_lines() {
        return;
    }
    if !doc.semantic_tokens().store().is_empty() {
        for t in &doc.line_tokens(line) {
            out.push((t.start as usize, t.style.ty() as u32 + 1));
        }
        return;
    }
    for t in tokens.line(line) {
        out.push((t.start as usize, t.style.ty() as u32 + 1));
    }
}

/// Builds the display line of `line`: its text (at most `stop_rendering_line_after` chars), its
/// paints and its injected pieces.
pub fn display_line(
    doc: &Document,
    view: &mut EditorView,
    line: usize,
    hints: bool,
) -> DisplayLine {
    let text = doc.text();
    // VS Code `stopRenderingLineAfter` (E3 F2): a long line shows (and shapes) only its prefix
    let model = line_prefix(text, line, doc.config().stop_rendering_line_after.max(1));
    let mut paints = Vec::new();
    token_paints(doc, line, &mut paints);
    let start = text.line_to_byte(line);
    // (model col, text, kind)
    let mut inject: Vec<(usize, String, PieceKind)> = Vec::new();
    if hints {
        for h in view.line_inlay_hints(doc, line) {
            let col = h.pos.saturating_sub(start).min(model.len());
            for (k, piece) in h.pieces.iter().enumerate() {
                inject.push((
                    col,
                    piece.text.clone(),
                    PieceKind::Hint {
                        hint: h.hint,
                        piece: k,
                    },
                ));
            }
        }
    }
    if let Some(p) = view.preedit() {
        let head = view.selection().primary().head;
        if (start..=start + model.len()).contains(&head) && !p.text.is_empty() {
            inject.push((head - start, p.text.clone(), PieceKind::Preedit));
        }
    }
    if inject.is_empty() {
        return DisplayLine {
            text: model.clone(),
            paints,
            pieces: Vec::new(),
            model_len: model.len(),
        };
    }
    // stable: hints keep their order, a preedit goes after hints at its position
    inject.sort_by_key(|(col, _, kind)| (*col, matches!(kind, PieceKind::Preedit)));
    let mut out = String::with_capacity(model.len() + 16);
    let mut pieces = Vec::with_capacity(inject.len());
    let mut out_paints = Vec::with_capacity(paints.len() + inject.len() * 2);
    let mut at = 0usize;
    let paint_at = |b: usize| -> u32 {
        let i = paints.partition_point(|p| p.0 <= b);
        if i == 0 { 0 } else { paints[i - 1].1 }
    };
    let push_model =
        |out: &mut String, out_paints: &mut Vec<(usize, u32)>, from: usize, to: usize| {
            if from >= to {
                return;
            }
            let base = out.len();
            out_paints.push((base, paint_at(from)));
            for &(b, p) in paints.iter().filter(|(b, _)| *b > from && *b < to) {
                out_paints.push((base + b - from, p));
            }
            out.push_str(&model[from..to]);
        };
    for (col, s, kind) in inject {
        push_model(&mut out, &mut out_paints, at, col);
        at = col;
        let d0 = out.len();
        out_paints.push((
            d0,
            if kind == PieceKind::Preedit {
                PAINT_PREEDIT
            } else {
                PAINT_HINT
            },
        ));
        out.push_str(&s);
        pieces.push(Piece {
            col,
            display: d0..out.len(),
            kind,
        });
    }
    push_model(&mut out, &mut out_paints, at, model.len());
    out_paints.dedup_by_key(|p| p.0);
    DisplayLine {
        text: out,
        paints: out_paints,
        pieces,
        model_len: model.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, pieces: &[(usize, &str, PieceKind)]) -> DisplayLine {
        let mut out = String::new();
        let mut ps = Vec::new();
        let mut at = 0;
        for &(col, s, kind) in pieces {
            out.push_str(&text[at..col]);
            at = col;
            let d0 = out.len();
            out.push_str(s);
            ps.push(Piece {
                col,
                display: d0..out.len(),
                kind,
            });
        }
        out.push_str(&text[at..]);
        DisplayLine {
            text: out,
            paints: Vec::new(),
            pieces: ps,
            model_len: text.len(),
        }
    }

    #[test]
    fn positions_map_around_a_hint() {
        // "let x = 1" with ": i32" after x
        let l = line(
            "let x = 1",
            &[(5, ": i32", PieceKind::Hint { hint: 0, piece: 0 })],
        );
        assert_eq!(l.text, "let x: i32 = 1");
        assert_eq!(l.to_display(5), 5, "before the hint");
        assert_eq!(l.to_display(6), 11);
        assert_eq!(l.to_model(7), 5, "inside the hint is its position");
        assert_eq!(l.to_model(11), 6);
        assert_eq!(l.to_model(99), 9);
        assert!(l.piece_at(6).is_some() && l.piece_at(10).is_none());
    }

    #[test]
    fn long_lines_keep_a_prefix() {
        let t = Text::from_file(&format!("{}\nb", "é".repeat(100)));
        assert_eq!(line_prefix(&t, 0, 10), "é".repeat(10));
        assert_eq!(line_prefix(&t, 0, 1_000), "é".repeat(100), "no line feed");
        assert_eq!(line_prefix(&t, 1, 10), "b");
    }

    #[test]
    fn the_caret_follows_a_preedit() {
        let l = line("ab", &[(1, "かな", PieceKind::Preedit)]);
        assert_eq!(l.to_display(1), 1 + "かな".len());
        assert_eq!(l.to_display(0), 0);
        assert_eq!(l.to_model(l.text.len()), 2);
    }
}
