//! The editor as skelter's [`TextInputClient`] (svelters architecture §11.3): what the platform's
//! IME bridges (IMM32/TSF, NSTextInputClient, text-input-v3) read and write.
//!
//! The IME sees a *virtual text*: the document with the composition (quire keeps it as the view's
//! preedit, not in the document) spliced in at the primary caret. Offsets are UTF-16 code units of
//! that whole text, through quire-text's document-wide UTF-16 conversions. Edits go through the
//! editor's one mutation path: a commit is core's `compositionType` (typing rules, its own undo
//! group), a replacement elsewhere one input transaction.

use std::cell::RefCell;
use std::ops::Range;

use quire_core::{Document, EditorCommand, EditorView, Preedit};
use skelter_base::geom::{Affine, Point, Rect};
use skelter_base::input::{InputPurpose, TextInputClient, Utf16Range};

use crate::editor::{SkelterEditor, composition_cmd, replace_range};

/// One editor, its document and view, and where its local space is in the window.
#[derive(Debug)]
pub struct ImeTarget<'a> {
    pub editor: &'a mut SkelterEditor,
    pub doc: &'a mut Document,
    pub view: &'a mut EditorView,
    /// Editor local space → window points.
    pub to_window: Affine,
}

/// Where a virtual UTF-16 offset falls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualPos {
    /// A document byte.
    Doc(usize),
    /// A byte of the preedit text.
    Preedit(usize),
}

impl ImeTarget<'_> {
    /// (caret byte, preedit) when composing.
    fn composition(&self) -> Option<(usize, &Preedit)> {
        let p = self.view.preedit()?;
        (!p.text.is_empty()).then(|| {
            (
                self.view
                    .selection()
                    .primary()
                    .head
                    .min(self.doc.text().len_bytes()),
                p,
            )
        })
    }

    fn preedit_len16(&self) -> usize {
        self.composition()
            .map_or(0, |(_, p)| p.text.encode_utf16().count())
    }

    /// The virtual UTF-16 offset of document byte `b`.
    pub fn doc_to_u16(&self, b: usize) -> usize {
        let t = self.doc.text();
        let u = t.byte_to_utf16_offset(b);
        match self.composition() {
            Some((h, _)) if b > h => u + self.preedit_len16(),
            _ => u,
        }
    }

    /// Where virtual UTF-16 offset `u` falls.
    pub fn locate(&self, u: usize) -> VirtualPos {
        let t = self.doc.text();
        let Some((h, p)) = self.composition() else {
            return VirtualPos::Doc(t.utf16_offset_to_byte(u));
        };
        let h16 = t.byte_to_utf16_offset(h);
        let p16 = p.text.encode_utf16().count();
        if u < h16 {
            VirtualPos::Doc(t.utf16_offset_to_byte(u))
        } else if u < h16 + p16 {
            VirtualPos::Preedit(utf16_to_byte_in(&p.text, u - h16))
        } else {
            VirtualPos::Doc(t.utf16_offset_to_byte(u - p16))
        }
    }

    /// The document byte of `u` (inside the preedit: the caret).
    fn doc_byte(&self, u: usize) -> usize {
        match self.locate(u) {
            VirtualPos::Doc(b) => b,
            VirtualPos::Preedit(_) => self.view.selection().primary().head,
        }
    }

    pub fn text_for_range(&self, r: Utf16Range) -> Option<String> {
        let total = self.doc.text().len_utf16() + self.preedit_len16();
        if r.start > r.end || r.start > total {
            return None;
        }
        let r = r.start..r.end.min(total);
        let mut out = String::new();
        let mut u = r.start;
        while u < r.end {
            match self.locate(u) {
                VirtualPos::Doc(b) => {
                    let c = self.doc.text().char_at(b)?;
                    out.push(c);
                    u += c.len_utf16();
                }
                VirtualPos::Preedit(b) => {
                    let (_, p) = self.composition()?;
                    let c = p.text[b..].chars().next()?;
                    out.push(c);
                    u += c.len_utf16();
                }
            }
        }
        Some(out)
    }

    pub fn selected_range(&self) -> Option<Utf16Range> {
        if let Some((h, p)) = self.composition() {
            let base = self.doc.text().byte_to_utf16_offset(h);
            return Some(match &p.active {
                Some(a) => base + utf16_len(&p.text[..a.start])..base + utf16_len(&p.text[..a.end]),
                None => {
                    let end = base + utf16_len(&p.text);
                    end..end
                }
            });
        }
        let r = self.view.selection().primary();
        Some(self.doc_to_u16(r.from())..self.doc_to_u16(r.to()))
    }

    pub fn marked_range(&self) -> Option<Utf16Range> {
        let (h, p) = self.composition()?;
        let base = self.doc.text().byte_to_utf16_offset(h);
        Some(base..base + utf16_len(&p.text))
    }

    fn doc_range(&self, r: &Utf16Range) -> Range<usize> {
        let a = self.doc_byte(r.start);
        let b = self.doc_byte(r.end);
        a.min(b)..a.max(b)
    }

    pub fn replace_text_in_range(&mut self, range: Option<Utf16Range>, text: &str) {
        let marked = self.marked_range();
        if self.view.preedit().is_some() {
            self.view.set_preedit(None);
        }
        let target = range
            .filter(|r| Some(r) != marked.as_ref())
            .map(|r| self.doc_range(&r));
        let sel = self.view.selection().primary().range();
        match target {
            Some(r) if r != sel => {
                let author = self.editor.options.author;
                if !self.editor.options.read_only
                    && let Some(a) = replace_range(self.doc, self.view, r, text, true, author)
                {
                    self.editor.note_applied(a);
                }
            }
            _ if !text.is_empty() => {
                self.editor
                    .dispatch(self.doc, self.view, composition_cmd(text, 0, 0))
            }
            _ => {}
        }
    }

    pub fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Utf16Range>,
        text: &str,
        new_selected: Option<Utf16Range>,
    ) {
        let marked = self.marked_range();
        // a range other than the composition is replaced first (reconversion of committed text)
        if let Some(r) = range.filter(|r| Some(r) != marked.as_ref()) {
            let r = self.doc_range(&r);
            if !r.is_empty() && !self.editor.options.read_only {
                let author = self.editor.options.author;
                self.view.set_preedit(None);
                if let Some(a) = replace_range(self.doc, self.view, r, "", true, author) {
                    self.editor.note_applied(a);
                }
            }
        } else if self.view.preedit().is_none()
            && !self.view.selection().primary().is_empty()
            && !self.editor.options.read_only
        {
            // composition replaces the selection
            self.editor.dispatch(
                self.doc,
                self.view,
                EditorCommand::Type {
                    text: String::new(),
                },
            );
        }
        if text.is_empty() {
            self.view.set_preedit(None);
            return;
        }
        let active =
            new_selected.map(|s| utf16_to_byte_in(text, s.start)..utf16_to_byte_in(text, s.end));
        self.view.set_preedit(Some(Preedit {
            text: text.to_owned(),
            active,
        }));
    }

    pub fn unmark_text(&mut self) {
        let Some(p) = self.view.preedit().cloned() else {
            return;
        };
        self.view.set_preedit(None);
        if !p.text.is_empty() {
            self.editor
                .dispatch(self.doc, self.view, composition_cmd(&p.text, 0, 0));
        }
    }

    /// The window rect of `range` on its first row (candidate windows anchor there).
    pub fn bounds_for_range(&mut self, range: Utf16Range) -> Option<Rect> {
        let head = self.view.selection().primary().head;
        let (start, end) = (self.locate(range.start), self.locate(range.end));
        let editor = &mut *self.editor;
        let (doc, view) = (&*self.doc, &mut *self.view);
        let rect_of = |editor: &mut SkelterEditor, view: &mut EditorView, v: VirtualPos| -> Rect {
            match v {
                VirtualPos::Doc(b) => editor.caret_content_rect(doc, view, b, false).0,
                VirtualPos::Preedit(pb) => {
                    // the preedit is injected after the caret: offset by its prefix
                    let (r, (line, row)) = editor.caret_content_rect(doc, view, head, false);
                    let l = editor.laid(line).cloned();
                    match l {
                        Some(l) => {
                            let col = head - doc.text().line_to_byte(line);
                            let d = l.display.to_display(col);
                            // `to_display` lands after the preedit: step back to its start
                            let plen = view.preedit().map_or(0, |p| p.text.len());
                            let x = l.rows.x_in_row(row, d - plen + pb);
                            Rect::new(x, r.min.y, x + 1.0, r.max.y)
                        }
                        None => r,
                    }
                }
            }
        };
        let a = rect_of(editor, view, start);
        let b = rect_of(editor, view, end);
        let r = if (a.min.y - b.min.y).abs() < 0.5 {
            Rect::new(a.min.x, a.min.y, b.max.x.max(a.max.x), a.max.y)
        } else {
            a
        };
        Some(
            self.to_window
                .apply_rect(&r.translate(content_to_local(editor))),
        )
    }

    pub fn character_index_for_point(&mut self, p: Point) -> Option<usize> {
        let local = self.to_window.inverse()?.apply(p);
        let (b, _) = self.editor.pos_at(self.doc, self.view, local);
        Some(self.doc_to_u16(b))
    }

    pub fn surrounding_text(&self, max_bytes: usize) -> Option<(String, Range<usize>)> {
        let t = self.doc.text();
        let sel = self.view.selection().primary();
        let line = t.byte_to_line(sel.head.min(t.len_bytes()));
        let lr = t.line_range(line);
        let half = max_bytes / 2;
        let mut a = sel.from().saturating_sub(half).max(lr.start);
        let mut b = (sel.to() + half).min(lr.end);
        a = t.clamp_to_boundary(a);
        while a < lr.end && !t.is_char_boundary(a) {
            a += 1;
        }
        b = t.clamp_to_boundary(b);
        if b < a {
            b = a;
        }
        let s = t.slice(a..b).to_string();
        let from = sel.from().clamp(a, b) - a;
        let to = sel.to().clamp(a, b) - a;
        Some((s, from..to))
    }
}

/// Content space → local space offset of the editor.
fn content_to_local(editor: &SkelterEditor) -> skelter_base::geom::Vec2 {
    let g = editor.geom();
    let s = editor.scroll();
    skelter_base::geom::Vec2::new(g.text_left() - s.x, crate::editor::PAD_Y - s.y)
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The byte of UTF-16 offset `u` in `s` (rounded down to a char start; clamped).
pub fn utf16_to_byte_in(s: &str, u: usize) -> usize {
    let mut acc = 0;
    for (b, c) in s.char_indices() {
        if acc + c.len_utf16() > u {
            return b;
        }
        acc += c.len_utf16();
    }
    s.len()
}

/// An [`ImeTarget`] as a [`TextInputClient`] (whose queries take `&self`).
#[derive(Debug)]
pub struct ImeClient<'a>(pub RefCell<ImeTarget<'a>>);

impl<'a> ImeClient<'a> {
    pub fn new(target: ImeTarget<'a>) -> Self {
        ImeClient(RefCell::new(target))
    }
}

impl TextInputClient for ImeClient<'_> {
    fn text_for_range(&self, range: Utf16Range) -> Option<String> {
        self.0.borrow().text_for_range(range)
    }
    fn selected_range(&self) -> Option<Utf16Range> {
        self.0.borrow().selected_range()
    }
    fn marked_range(&self) -> Option<Utf16Range> {
        self.0.borrow().marked_range()
    }
    fn replace_text_in_range(&mut self, range: Option<Utf16Range>, text: &str) {
        self.0.get_mut().replace_text_in_range(range, text)
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Utf16Range>,
        text: &str,
        new_selected: Option<Utf16Range>,
    ) {
        self.0
            .get_mut()
            .replace_and_mark_text_in_range(range, text, new_selected)
    }
    fn unmark_text(&mut self) {
        self.0.get_mut().unmark_text()
    }
    fn bounds_for_range(&self, range: Utf16Range) -> Option<Rect> {
        self.0.borrow_mut().bounds_for_range(range)
    }
    fn character_index_for_point(&self, p: Point) -> Option<usize> {
        self.0.borrow_mut().character_index_for_point(p)
    }
    fn surrounding_text(&self, max_bytes: usize) -> Option<(String, Range<usize>)> {
        self.0.borrow().surrounding_text(max_bytes)
    }
    fn purpose(&self) -> InputPurpose {
        InputPurpose::Code
    }
}
