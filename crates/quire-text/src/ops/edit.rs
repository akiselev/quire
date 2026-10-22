//! The edits field commands and quire's matching commands are made of (VS Code
//! `cursorTypeEditOperations.ts`, `cursorDeleteOperations.ts`, `linesOperations.ts`): pure
//! functions from a text and a selection to a change set and the selection after it.
//!
//! `delete_all_left` and `delete_all_right` are translated from Visual Studio Code
//! (`src/vs/editor/contrib/linesOperations/browser/linesOperations.ts`, microsoft/vscode @
//! `bdce63db`). Copyright (c) Microsoft Corporation. All rights reserved. Licensed under the MIT
//! License (see THIRD_PARTY_NOTICES.md for the full text).

use super::model::Model;
use super::{RangeEdit, build};
use crate::change::ChangeSet;
use crate::command::DeleteUnit;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::InputKind;
use crate::words::vscode::{self, WordNav};
use crate::words::{WordRules, WordStop};

/// One edit: what a command changes and the input kind undo grouping sees.
#[derive(Clone, Debug)]
pub struct OpEdit {
    /// The changes.
    pub changes: ChangeSet,
    /// The selection after them.
    pub sel: Selection,
    /// What made the edit.
    pub kind: InputKind,
    /// Close the open undo group before this edit (VS Code `shouldPushStackElementBefore`).
    pub close_before: bool,
}

fn out(text: &Text, changes: ChangeSet, sel: Selection, kind: InputKind) -> Option<OpEdit> {
    if changes.is_empty() {
        return None;
    }
    // VS Code pushes an undo stop before a delete that joins lines.
    let close_before = matches!(kind, InputKind::DeletingLeft | InputKind::DeletingRight)
        && changes
            .edit_iter()
            .any(|e| text.byte_to_line(e.range.start) != text.byte_to_line(e.range.end));
    Some(OpEdit {
        changes,
        sel,
        kind,
        close_before,
    })
}

/// `SimpleCharacterTypeOperation`: `typed` replaces every range. A single space after a space
/// (per the last input kind) is `TypingConsecutiveSpace`, after anything else
/// `TypingFirstSpace`; other text is `Typing`.
pub fn insert_typed(
    text: &Text,
    sel: &Selection,
    typed: &str,
    last_kind: Option<InputKind>,
) -> OpEdit {
    let kind = if typed == " " {
        match last_kind {
            Some(InputKind::TypingFirstSpace) | Some(InputKind::TypingConsecutiveSpace) => {
                InputKind::TypingConsecutiveSpace
            }
            _ => InputKind::TypingFirstSpace,
        }
    } else {
        InputKind::Typing
    };
    let (changes, sel) = super::build_replace_each(text, sel, typed);
    OpEdit {
        changes,
        sel,
        kind,
        close_before: false,
    }
}

/// Paste: `t` replaces every range; with `per_cursor` and as many lines as ranges, one line per
/// range.
pub fn paste(text: &Text, sel: &Selection, t: &str, per_cursor: bool) -> Option<OpEdit> {
    let n = sel.len();
    let lines: Option<Vec<&str>> = if per_cursor && n > 1 {
        let trimmed = t.strip_suffix('\n').unwrap_or(t);
        let v: Vec<&str> = trimmed.split('\n').collect();
        (v.len() == n).then_some(v)
    } else {
        None
    };
    if lines.is_none() {
        let (changes, sel) = super::build_replace_each(text, sel, t);
        return out(text, changes, sel, InputKind::Paste);
    }
    let edits = sel
        .ranges()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let s = lines.as_ref().map_or(t, |v| v[i]);
            Some(RangeEdit::replace(r.range(), s))
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    out(text, changes, sel, InputKind::Paste)
}

/// Cut: delete every range; an empty range takes its whole line when `empty_selection_line`
/// (VS Code's `emptySelectionClipboard`).
pub fn cut(text: &Text, sel: &Selection, empty_selection_line: bool) -> Option<OpEdit> {
    let m = Model::new(text);
    let mut last_cut_end_line: Option<usize> = None;
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            if !empty_selection_line {
                return None;
            }
            let p = m.pos(r.head);
            let lc = m.line_count();
            let range = if p.line + 1 < lc {
                text.line_to_byte(p.line)..text.line_to_byte(p.line + 1)
            } else if p.line > 0 && last_cut_end_line != Some(p.line) {
                text.line_range(p.line - 1).end..text.line_range(p.line).end
            } else {
                text.line_range(p.line)
            };
            last_cut_end_line = Some(text.byte_to_line(range.end));
            if range.is_empty() {
                None
            } else {
                Some(RangeEdit::replace(range, ""))
            }
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    out(text, changes, sel, InputKind::Other)
}

/// Backspace: delete every non-empty range; at a cursor, one `unit` back (at a line start: join
/// with the previous line).
pub fn delete_left(text: &Text, sel: &Selection, unit: DeleteUnit) -> Option<OpEdit> {
    let m = Model::new(text);
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            let a = match unit {
                DeleteUnit::VsCode => m.left_delete_offset(r.head),
                DeleteUnit::Grapheme => m.prev_grapheme(r.head),
                DeleteUnit::CodePoint => text.prev_char_boundary(r.head),
            };
            if a == r.head {
                None
            } else {
                Some(RangeEdit::replace(a..r.head, ""))
            }
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    out(text, changes, sel, InputKind::DeletingLeft)
}

/// Delete: delete every non-empty range; at a cursor, one grapheme forward (at a line end: join
/// with the next line).
pub fn delete_right(text: &Text, sel: &Selection) -> Option<OpEdit> {
    let m = Model::new(text);
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            let b = m.next_grapheme(r.head);
            if b == r.head {
                None
            } else {
                Some(RangeEdit::replace(r.head..b, ""))
            }
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    out(text, changes, sel, InputKind::DeletingRight)
}

/// The range a word deletion from cursor `at` removes (`None`: nothing).
pub fn word_delete_range(
    text: &Text,
    at: usize,
    left: bool,
    stop: WordStop,
    words: &WordRules,
    secret: bool,
) -> Option<std::ops::Range<usize>> {
    let (a, b) = if secret {
        if left {
            (0, at)
        } else {
            (at, text.len_bytes())
        }
    } else {
        match words {
            WordRules::VsCode(cls) => {
                let m = Model::new(text);
                let p = m.pos(at);
                let (nav, ws) = match (left, stop) {
                    (true, WordStop::Native | WordStop::StartOrSpace) => (WordNav::WordStart, true),
                    (true, WordStop::Start) => (WordNav::WordStart, false),
                    (true, WordStop::StartFast) => (WordNav::WordStartFast, false),
                    (false, WordStop::Native | WordStop::EndOrSpace) => (WordNav::WordEnd, true),
                    (false, WordStop::Start | WordStop::StartOrSpace | WordStop::StartFast) => {
                        (WordNav::WordStart, false)
                    }
                    (_, WordStop::End | WordStop::EndOrSpace) => (WordNav::WordEnd, false),
                };
                let (a, b) = if left {
                    vscode::delete_word_left(&m, cls, p, nav, ws)?
                } else {
                    vscode::delete_word_right(&m, cls, p, nav, ws)?
                };
                (m.off(a), m.off(b))
            }
            w => {
                if left {
                    (w.prev_native(text, at, stop), at)
                } else {
                    (at, w.next_native(text, at, stop))
                }
            }
        }
    };
    let (a, b) = (a.min(b), a.max(b));
    (a != b).then_some(a..b)
}

/// Word deletion (`deleteWordLeft` and friends): every non-empty range, else from each cursor to
/// the word stop.
pub fn delete_word(
    text: &Text,
    sel: &Selection,
    left: bool,
    stop: WordStop,
    words: &WordRules,
    secret: bool,
) -> Option<OpEdit> {
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            word_delete_range(text, r.head, left, stop, words, secret)
                .map(|range| RangeEdit::replace(range, ""))
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    let kind = if left {
        InputKind::DeletingWordLeft
    } else {
        InputKind::DeletingWordRight
    };
    out(text, changes, sel, kind)
}

/// IME delete-surrounding: `prev` code points before and `next` after each cursor, on its line
/// (non-empty ranges are left alone). The first half of VS Code's `compositionType`.
pub fn delete_surrounding(
    text: &Text,
    sel: &Selection,
    prev: usize,
    next: usize,
) -> Option<(ChangeSet, Selection)> {
    if prev == 0 && next == 0 {
        return None;
    }
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return None;
            }
            let line = text.line_range(text.byte_to_line(r.head));
            let mut a = r.head;
            for c in text.chars_before(r.head).take(prev) {
                if a <= line.start {
                    break;
                }
                a -= c.len_utf8();
            }
            let mut b = r.head;
            for c in text.chars_at(r.head).take(next) {
                if b >= line.end {
                    break;
                }
                b += c.len_utf8();
            }
            Some(RangeEdit::replace(a..b, ""))
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    (!changes.is_empty()).then_some((changes, sel))
}

fn lines_out(changes: ChangeSet, sel: Selection) -> Option<OpEdit> {
    if changes.is_empty() {
        return None;
    }
    Some(OpEdit {
        changes,
        sel,
        kind: InputKind::Other,
        close_before: true,
    })
}

/// `DeleteAllLeftAction`: delete from each cursor to its line start (at column 0: join with the
/// previous line); a selection deletes from its first line's start to its end.
pub fn delete_all_left(text: &Text, sel: &Selection) -> Option<OpEdit> {
    let edits = sel
        .ranges()
        .iter()
        .map(|r| {
            let line = text.byte_to_line(r.from());
            let ls = text.line_to_byte(line);
            let range = if r.is_empty() {
                if r.head == ls {
                    if line == 0 {
                        0..0
                    } else {
                        text.line_range(line - 1).end..ls
                    }
                } else {
                    ls..r.head
                }
            } else {
                ls..r.to()
            };
            Some(RangeEdit::replace(range, ""))
        })
        .collect();
    let (changes, sel) = build(text, sel, edits);
    lines_out(changes, sel)
}

/// `DeleteAllRightAction`: delete from each cursor to its line end (at the line end: join with
/// the next line) and each selection; overlapping ranges merge; the cursors end at the starts,
/// the primary one first.
pub fn delete_all_right(text: &Text, sel: &Selection) -> Option<OpEdit> {
    let primary = sel.primary();
    let mut ranges: Vec<std::ops::Range<usize>> = sel
        .ranges()
        .iter()
        .map(|r| {
            if r.is_empty() {
                let lr = text.line_range(text.byte_to_line(r.head));
                if r.head == lr.end {
                    r.head..(lr.end + 1).min(text.len_bytes())
                } else {
                    r.head..lr.end
                }
            } else {
                r.range()
            }
        })
        .collect();
    ranges.sort_by_key(|r| (r.start, r.end));
    // Merge ranges that intersect (touching counts, as in VS Code's `Range.intersectRanges`).
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
            continue;
        }
        merged.push(r);
    }
    let changes =
        ChangeSet::replace(text.len_bytes(), merged.iter().map(|r| (r.clone(), ""))).ok()?;
    if changes.is_empty() {
        return None;
    }
    let mut cursors: Vec<SelRange> = Vec::new();
    let mut primary_cursor: Option<SelRange> = None;
    let mut removed = 0;
    for r in &merged {
        let c = SelRange::point(r.start - removed);
        removed += r.end - r.start;
        let p = primary.range();
        let intersects = r.start <= p.end && p.start <= r.end;
        if intersects && primary_cursor.is_none() {
            primary_cursor = Some(c);
        } else {
            cursors.push(c);
        }
    }
    if let Some(p) = primary_cursor {
        cursors.insert(0, p);
    }
    lines_out(changes, Selection::new(cursors, 0))
}
