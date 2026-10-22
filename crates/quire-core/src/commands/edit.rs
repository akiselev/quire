//! Typing, deleting, indentation, comments and line operations (VS Code
//! `cursorTypeEditOperations.ts`, `cursorDeleteOperations.ts`, `shiftCommand.ts`,
//! `lineCommentCommand.ts`).

use super::model::{
    Model, Pos, col_from_visible, next_indent_tab_stop, prev_indent_tab_stop, visible_col,
};
use super::words::{self, WordNav};
use super::{Ctx, Out, RangeEdit, St, build, track};
use crate::change::{Bias, ChangeSet};
use crate::config::{AutoClosingStrategy, EditorConfig, OvertypeStrategy};
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::transaction::InputKind;
use crate::view::AutoClosed;
use crate::words::CharClass;

/// Per selection range: line edits, cursor at the end of the last edit, selection start stays.
type LineShift = (Vec<(std::ops::Range<usize>, String)>, bool, bool);

fn is_quote(s: &str) -> bool {
    matches!(s, "'" | "\"" | "`")
}

/// VS Code `normalizeIndentation`.
pub(crate) fn normalize_indentation(cfg: &EditorConfig, s: &str) -> String {
    let ws_end = s.find(|c| c != ' ' && c != '\t').unwrap_or(s.len());
    let mut spaces = 0u32;
    let indent = cfg.indent_size.max(1);
    for c in s[..ws_end].chars() {
        if c == '\t' {
            spaces = next_indent_tab_stop(spaces, indent);
        } else {
            spaces += 1;
        }
    }
    let mut out = String::new();
    if !cfg.insert_spaces {
        for _ in 0..spaces / indent {
            out.push('\t');
        }
        spaces %= indent;
    }
    for _ in 0..spaces {
        out.push(' ');
    }
    out.push_str(&s[ws_end..]);
    out
}

fn out(text: &Text, changes: ChangeSet, sel: Selection, kind: InputKind) -> Option<Out> {
    if changes.is_empty() {
        return None;
    }
    // VS Code pushes an undo stop before a delete that joins lines.
    let close_before = matches!(kind, InputKind::DeletingLeft | InputKind::DeletingRight)
        && changes
            .edits()
            .iter()
            .any(|e| text.byte_to_line(e.range.start) != text.byte_to_line(e.range.end));
    Some(Out {
        changes,
        sel,
        kind,
        auto_closed: Vec::new(),
        close_before,
    })
}

fn map_auto_closed(list: &mut Vec<AutoClosed>, c: &ChangeSet) {
    for a in list.iter_mut() {
        let len = a.close.end - a.close.start;
        a.open_start = c.map_pos(a.open_start, Bias::After);
        let s = c.map_pos(a.close.start, Bias::After);
        let e = c.map_pos(a.close.end, Bias::Before).max(s);
        a.close = if e - s == len { s..e } else { s..s };
    }
    list.retain(|a| a.close.start < a.close.end);
}

// ---------------------------------------------------------------------------------------------
// typing

pub(crate) fn type_text(ctx: &Ctx, st: St, typed: &str) -> Option<Out> {
    let mut state = st;
    let mut acc: Option<ChangeSet> = None;
    let mut new_ac: Vec<AutoClosed> = Vec::new();
    let mut kind = InputKind::Typing;
    for ch in typed.chars() {
        let mut buf = [0u8; 4];
        let o = type_char(ctx, &state, ch.encode_utf8(&mut buf))?;
        let next_text = o.changes.apply(&state.text).ok()?;
        map_auto_closed(&mut new_ac, &o.changes);
        map_auto_closed(&mut state.auto_closed, &o.changes);
        new_ac.extend(o.auto_closed.iter().cloned());
        state.auto_closed.extend(o.auto_closed);
        acc = Some(match acc {
            None => o.changes,
            Some(a) => a.compose(&o.changes).ok()?,
        });
        state.text = next_text;
        state.sel = o.sel;
        state.last_kind = Some(o.kind);
        kind = o.kind;
    }
    let changes = acc?;
    if typed.chars().count() > 1 && !matches!(kind, InputKind::Other) {
        kind = InputKind::Typing;
    }
    Some(Out {
        changes,
        sel: state.sel,
        kind,
        auto_closed: new_ac,
        close_before: false,
    })
}

fn type_char(ctx: &Ctx, st: &St, ch: &str) -> Option<Out> {
    if ch == "\n" {
        return Some(enter(ctx, st));
    }
    if let Some(o) = surround(ctx, st, ch) {
        return Some(o);
    }
    if let Some(o) = overtype(ctx, st, ch) {
        return Some(o);
    }
    if let Some(o) = auto_close(ctx, st, ch) {
        return Some(o);
    }
    let kind = if ch == " " {
        match st.last_kind {
            Some(InputKind::TypingFirstSpace) | Some(InputKind::TypingConsecutiveSpace) => {
                InputKind::TypingConsecutiveSpace
            }
            _ => InputKind::TypingFirstSpace,
        }
    } else {
        InputKind::Typing
    };
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| Some(RangeEdit::replace(r.range(), ch)))
        .collect();
    let (changes, sel) = build(&st.text, &st.sel, edits);
    Some(Out {
        changes,
        sel,
        kind,
        auto_closed: Vec::new(),
        close_before: false,
    })
}

fn indentation_at(m: &Model, b: usize) -> String {
    let p = m.pos(b);
    m.indentation(p.line).chars().take(p.col).collect()
}

/// `EnterOperation._enter` with bracket-based indent rules.
fn enter_edit(ctx: &Ctx, text: &Text, range: std::ops::Range<usize>) -> RangeEdit {
    let cfg = ctx.cfg;
    if !cfg.auto_indent {
        return RangeEdit::replace(range, "\n");
    }
    let m = Model::new(text);
    let indentation = indentation_at(&m, range.start);
    let start_line = text.byte_to_line(range.start);
    let end_line = text.byte_to_line(range.end);
    let before = text
        .slice(text.line_to_byte(start_line)..range.start)
        .to_string();
    let after = text
        .slice(range.end..text.line_range(end_line).end)
        .to_string();
    let before_t = before.trim_end();
    let after_t = after.trim_start();
    let unit = if cfg.insert_spaces {
        " ".repeat(cfg.indent_size.max(1) as usize)
    } else {
        "\t".to_string()
    };
    for (open, close) in &cfg.brackets {
        if !before_t.is_empty()
            && before_t.ends_with(open.as_str())
            && after_t.starts_with(close.as_str())
        {
            let normal = normalize_indentation(cfg, &indentation);
            let increased = normalize_indentation(cfg, &format!("{indentation}{unit}"));
            let insert = format!("\n{increased}\n{normal}");
            let head = 1 + increased.len();
            return RangeEdit {
                range,
                insert,
                anchor: head,
                head,
            };
        }
    }
    for (open, _) in &cfg.brackets {
        if !before_t.is_empty() && before_t.ends_with(open.as_str()) {
            let increased = normalize_indentation(cfg, &format!("{indentation}{unit}"));
            return RangeEdit::replace(range, format!("\n{increased}"));
        }
    }
    RangeEdit::replace(
        range,
        format!("\n{}", normalize_indentation(cfg, &indentation)),
    )
}

fn enter(ctx: &Ctx, st: &St) -> Out {
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| Some(enter_edit(ctx, &st.text, r.range())))
        .collect();
    let (changes, sel) = build(&st.text, &st.sel, edits);
    Out {
        changes,
        sel,
        kind: InputKind::Typing,
        auto_closed: Vec::new(),
        close_before: false,
    }
}

fn surround(ctx: &Ctx, st: &St, ch: &str) -> Option<Out> {
    let cfg = ctx.cfg;
    if !cfg.auto_surround {
        return None;
    }
    let close = cfg
        .surrounding_pairs
        .iter()
        .find(|(o, _)| o == ch)
        .map(|(_, c)| c.clone())?;
    for r in st.sel.ranges() {
        if r.is_empty() {
            return None;
        }
        let s = st.text.slice(r.range()).to_string();
        if s.chars().all(|c| c == ' ' || c == '\t' || c == '\n') {
            return None;
        }
        if is_quote(ch) && s.chars().count() == 1 && is_quote(&s) {
            return None;
        }
    }
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let s = st.text.slice(r.range()).to_string();
            let n = s.len();
            let insert = format!("{ch}{s}{close}");
            let (a, h) = if r.is_backward() {
                (ch.len() + n, ch.len())
            } else {
                (ch.len(), ch.len() + n)
            };
            Some(RangeEdit {
                range: r.range(),
                insert,
                anchor: a,
                head: h,
            })
        })
        .collect();
    let (changes, sel) = build(&st.text, &st.sel, edits);
    Some(Out {
        changes,
        sel,
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: false,
    })
}

fn overtype(ctx: &Ctx, st: &St, ch: &str) -> Option<Out> {
    let cfg = ctx.cfg;
    if cfg.auto_closing_overtype == OvertypeStrategy::Never {
        return None;
    }
    if !cfg.auto_closing_pairs.iter().any(|(_, c)| c == ch) {
        return None;
    }
    let text = &st.text;
    for r in st.sel.ranges() {
        if !r.is_empty() {
            return None;
        }
        let line_end = text.line_range(text.byte_to_line(r.head)).end;
        if r.head + ch.len() > line_end || text.slice(r.head..r.head + ch.len()) != ch {
            return None;
        }
        if is_quote(ch) && text.char_before(r.head) == Some('\\') {
            return None;
        }
        if cfg.auto_closing_overtype == OvertypeStrategy::Auto
            && !st.auto_closed.iter().any(|a| a.close.start == r.head)
        {
            return None;
        }
    }
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| Some(RangeEdit::replace(r.head..r.head + ch.len(), ch)))
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    Some(Out {
        changes,
        sel,
        kind: InputKind::Typing,
        auto_closed: Vec::new(),
        close_before: false,
    })
}

fn auto_close(ctx: &Ctx, st: &St, ch: &str) -> Option<Out> {
    let cfg = ctx.cfg;
    let text = &st.text;
    if st.sel.ranges().iter().any(|r| !r.is_empty()) {
        return None;
    }
    // The longest pair whose opening ends with `ch` and matches the text before every cursor.
    let mut best: Option<&(String, String)> = None;
    for pair in &cfg.auto_closing_pairs {
        let (open, _) = pair;
        if !open.ends_with(ch) {
            continue;
        }
        let prefix = &open[..open.len() - ch.len()];
        let ok = st.sel.ranges().iter().all(|r| {
            let ls = text.line_to_byte(text.byte_to_line(r.head));
            r.head >= ls + prefix.len() && text.slice(r.head - prefix.len()..r.head) == prefix
        });
        if ok && best.is_none_or(|b| open.len() > b.0.len()) {
            best = Some(pair);
        }
    }
    let (open, close) = best?;
    let quote = is_quote(ch);
    let strategy = if quote {
        cfg.auto_closing_quotes
    } else {
        cfg.auto_closing_brackets
    };
    if strategy == AutoClosingStrategy::Never {
        return None;
    }
    for r in st.sel.ranges() {
        let line_end = text.line_range(text.byte_to_line(r.head)).end;
        let after = text.slice(r.head..line_end).to_string();
        if let Some(c) = after.chars().next() {
            let before_close = cfg
                .auto_closing_pairs
                .iter()
                .any(|(_, cl)| after.starts_with(cl.as_str()))
                && !cfg
                    .auto_closing_pairs
                    .iter()
                    .any(|(op, _)| after.starts_with(op.as_str()));
            let allowed = match strategy {
                AutoClosingStrategy::Always => true,
                AutoClosingStrategy::BeforeWhitespace => c == ' ' || c == '\t',
                _ => cfg.auto_close_before.contains(c),
            };
            if !before_close && !allowed {
                return None;
            }
        }
        if open.len() == 1
            && (ch == "'" || ch == "\"")
            && strategy != AutoClosingStrategy::Always
            && let Some(b) = text.char_before(r.head)
            && ctx.cls.class(b) == CharClass::Regular
        {
            return None;
        }
    }
    // A contained pair (`(`,`)` inside `(*`,`*)`): when its closing text already follows every
    // cursor, insert only the rest of the closing text (VS Code `_findContainedAutoClosingPair`).
    let mut contained: Option<&(String, String)> = None;
    if open.chars().count() > 1 {
        for cand in &cfg.auto_closing_pairs {
            if cand.0 != *open
                && open.contains(cand.0.as_str())
                && close.ends_with(cand.1.as_str())
                && contained.is_none_or(|c| cand.0.len() > c.0.len())
            {
                contained = Some(cand);
            }
        }
    }
    let inserted_close = match contained {
        Some((_, cc))
            if st.sel.ranges().iter().all(|r| {
                let line_end = text.line_range(text.byte_to_line(r.head)).end;
                text.slice(r.head..line_end)
                    .to_string()
                    .starts_with(cc.as_str())
            }) =>
        {
            &close[..close.len() - cc.len()]
        }
        _ => close.as_str(),
    };
    let mut new_ac = Vec::new();
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let insert = format!("{ch}{inserted_close}");
            Some(RangeEdit {
                range: r.head..r.head,
                insert,
                anchor: ch.len(),
                head: ch.len(),
            })
        })
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    for r in sel.ranges() {
        new_ac.push(AutoClosed {
            open_start: r.head - open.len(),
            close: r.head..r.head + close.len(),
        });
    }
    Some(Out {
        changes,
        sel,
        kind: InputKind::Typing,
        auto_closed: new_ac,
        close_before: false,
    })
}

// ---------------------------------------------------------------------------------------------
// paste, cut, delete

pub(crate) fn paste(st: &St, t: &str, per_cursor: bool) -> Option<Out> {
    let n = st.sel.len();
    let lines: Option<Vec<&str>> = if per_cursor && n > 1 {
        let trimmed = t.strip_suffix('\n').unwrap_or(t);
        let v: Vec<&str> = trimmed.split('\n').collect();
        (v.len() == n).then_some(v)
    } else {
        None
    };
    let edits = st
        .sel
        .ranges()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let s = lines.as_ref().map_or(t, |v| v[i]);
            Some(RangeEdit::replace(r.range(), s))
        })
        .collect();
    let (changes, sel) = build(&st.text, &st.sel, edits);
    out(&st.text, changes, sel, InputKind::Paste)
}

pub(crate) fn cut(ctx: &Ctx, st: &St) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    let mut last_cut_end_line: Option<usize> = None;
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            if !ctx.cfg.empty_selection_clipboard {
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
    let (changes, sel) = build(text, &st.sel, edits);
    out(text, changes, sel, InputKind::Other)
}

fn is_auto_closing_pair_delete(ctx: &Ctx, st: &St) -> bool {
    let cfg = ctx.cfg;
    if (cfg.auto_closing_brackets == AutoClosingStrategy::Never
        && cfg.auto_closing_quotes == AutoClosingStrategy::Never)
        || !cfg.auto_closing_delete
    {
        return false;
    }
    let text = &st.text;
    st.sel.ranges().iter().all(|r| {
        if !r.is_empty() {
            return false;
        }
        let lr = text.line_range(text.byte_to_line(r.head));
        if r.head == lr.start || r.head >= lr.end {
            return false;
        }
        let (Some(before), Some(after)) = (text.char_before(r.head), text.char_at(r.head)) else {
            return false;
        };
        let (b, a) = (before.to_string(), after.to_string());
        let strategy = if is_quote(&b) {
            cfg.auto_closing_quotes
        } else {
            cfg.auto_closing_brackets
        };
        if strategy == AutoClosingStrategy::Never {
            return false;
        }
        cfg.auto_closing_pairs
            .iter()
            .any(|(o, c)| *o == b && *c == a)
            && st.auto_closed.iter().any(|ac| ac.close.start == r.head)
    })
}

pub(crate) fn delete_left(ctx: &Ctx, st: &St) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    if is_auto_closing_pair_delete(ctx, st) {
        let edits = st
            .sel
            .ranges()
            .iter()
            .map(|r| {
                let a = text.prev_char_boundary(r.head);
                let b = text.next_char_boundary(r.head);
                Some(RangeEdit::replace(a..b, ""))
            })
            .collect();
        let (changes, sel) = build(text, &st.sel, edits);
        return out(text, changes, sel, InputKind::DeletingLeft);
    }
    let tab = ctx.cfg.tab_size;
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            let p = m.pos(r.head);
            if ctx.cfg.use_tab_stops && p.col > 0 {
                let chars = m.line_chars(p.line);
                let last_indent_col = m.first_non_ws(p.line).unwrap_or(chars.len());
                if p.col <= last_indent_col {
                    let from_vis = visible_col(&chars, p.col, tab);
                    let to_vis = prev_indent_tab_stop(from_vis, ctx.cfg.indent_size);
                    let to_col = col_from_visible(&chars, to_vis, tab);
                    let a = m.off(Pos::new(p.line, to_col));
                    return Some(RangeEdit::replace(a..r.head, ""));
                }
            }
            let a = m.left_delete_offset(r.head);
            if a == r.head {
                None
            } else {
                Some(RangeEdit::replace(a..r.head, ""))
            }
        })
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    out(text, changes, sel, InputKind::DeletingLeft)
}

pub(crate) fn delete_right(st: &St) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    let edits = st
        .sel
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
    let (changes, sel) = build(text, &st.sel, edits);
    out(text, changes, sel, InputKind::DeletingRight)
}

pub(crate) fn delete_word(ctx: &Ctx, st: &St, left: bool, nav: WordNav, ws: bool) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    let pair_delete = left && is_auto_closing_pair_delete(ctx, st);
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            if !r.is_empty() {
                return Some(RangeEdit::replace(r.range(), ""));
            }
            if pair_delete {
                let a = text.prev_char_boundary(r.head);
                let b = text.next_char_boundary(r.head);
                return Some(RangeEdit::replace(a..b, ""));
            }
            let p = m.pos(r.head);
            let res = if left {
                words::delete_word_left(&m, &ctx.cls, p, nav, ws)
            } else {
                words::delete_word_right(&m, &ctx.cls, p, nav, ws)
            };
            let (a, b) = res?;
            let (a, b) = (m.off(a), m.off(b));
            let (a, b) = (a.min(b), a.max(b));
            if a == b {
                None
            } else {
                Some(RangeEdit::replace(a..b, ""))
            }
        })
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    out(text, changes, sel, InputKind::Other)
}

// ---------------------------------------------------------------------------------------------
// indentation

/// `ShiftCommand.shiftIndent` / `unshiftIndent` for the indentation before visible column
/// `content_vis`.
fn shifted_indent(cfg: &EditorConfig, content_vis: u32, unshift: bool) -> String {
    if cfg.insert_spaces {
        let size = cfg.indent_size.max(1);
        let stop = if unshift {
            prev_indent_tab_stop(content_vis, size)
        } else {
            next_indent_tab_stop(content_vis, size)
        };
        " ".repeat(stop as usize)
    } else {
        let size = cfg.tab_size.max(1);
        let stop = if unshift {
            prev_indent_tab_stop(content_vis, size)
        } else {
            next_indent_tab_stop(content_vis, size)
        };
        "\t".repeat((stop / size) as usize)
    }
}

/// Line edits of a shift over the lines of one selection range, VS Code `ShiftCommand` with
/// `useTabStops`. Returns `(edits, last_edit_end_for_cursor)`.
fn shift_range(
    ctx: &Ctx,
    text: &Text,
    r: &SelRange,
    unshift: bool,
) -> (Vec<(std::ops::Range<usize>, String)>, bool) {
    let cfg = ctx.cfg;
    let m = Model::new(text);
    let start_line = text.byte_to_line(r.from());
    let mut end_line = text.byte_to_line(r.to());
    if r.to() == text.line_to_byte(end_line) && start_line != end_line {
        end_line -= 1;
    }
    let indent_empty = start_line == end_line;
    let mut use_last = false;
    if r.is_empty() && m.first_non_ws(start_line).is_none() {
        use_last = true;
    }
    let mut out = Vec::new();
    for line in start_line..=end_line {
        let chars = m.line_chars(line);
        let first = m.first_non_ws(line);
        if unshift && (chars.is_empty() || first == Some(0)) {
            continue;
        }
        if !indent_empty && !unshift && chars.is_empty() {
            continue;
        }
        let ind_end = first.unwrap_or(chars.len());
        if unshift && ind_end == 0 {
            continue;
        }
        let vis = visible_col(&chars, ind_end, cfg.tab_size);
        let desired = shifted_indent(cfg, vis, unshift);
        let ls = text.line_to_byte(line);
        let range = ls..m.off(Pos::new(line, ind_end));
        if text.slice(range.clone()) != desired.as_str() {
            out.push((range, desired));
        }
    }
    (out, use_last)
}

fn apply_line_edits(
    text: &Text,
    sel: &Selection,
    per_range: Vec<LineShift>,
    kind: InputKind,
) -> Option<Out> {
    // per_range: (edits, cursor at end of last edit, start column stays put)
    let mut all: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (edits, _, _) in &per_range {
        for e in edits {
            if !all.iter().any(|(r, _)| r.start == e.0.start) {
                all.push(e.clone());
            }
        }
    }
    if all.is_empty() {
        return None;
    }
    all.sort_by_key(|(r, _)| r.start);
    let changes = ChangeSet::replace(
        text.len_bytes(),
        all.iter().map(|(r, s)| (r.clone(), s.as_str())),
    )
    .ok()?;
    let tracked = track(text, &changes, sel);
    let empty: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let ranges: Vec<SelRange> = sel
        .ranges()
        .iter()
        .zip(
            tracked
                .ranges()
                .iter()
                .copied()
                .chain(std::iter::repeat(SelRange::point(0))),
        )
        .enumerate()
        .map(|(i, (orig, t))| {
            let (edits, use_last, stays) = per_range
                .get(i)
                .map_or((&empty, false, false), |(e, u, s)| (e, *u, *s));
            if use_last && let Some((r, s)) = edits.last() {
                let p = changes.map_pos(r.start, Bias::Before) + s.len();
                return SelRange::point(p);
            }
            if stays && !orig.is_empty() {
                // Keep the selection start where it was (we inserted after it).
                let start = orig.from();
                if t.from() > start {
                    return if t.is_backward() {
                        SelRange::new(t.to(), start)
                    } else {
                        SelRange::new(start, t.to())
                    };
                }
            }
            t
        })
        .collect();
    let sel = if ranges.len() == sel.len() {
        Selection::new(ranges, sel.primary_index())
    } else {
        tracked
    };
    Some(Out {
        changes,
        sel,
        kind,
        auto_closed: Vec::new(),
        close_before: false,
    })
}

pub(crate) fn shift(ctx: &Ctx, st: &St, unshift: bool) -> Option<Out> {
    let text = &st.text;
    let per: Vec<_> = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let (edits, use_last) = shift_range(ctx, text, r, unshift);
            let stays = !unshift
                && !r.is_empty()
                && edits.first().is_some_and(|(er, _)| {
                    text.byte_to_line(er.start) == text.byte_to_line(r.from()) && r.from() <= er.end
                });
            (edits, use_last, stays)
        })
        .collect();
    apply_line_edits(text, &st.sel, per, InputKind::Other)
}

/// The indentation a blank line should get from the previous non-blank line.
fn good_indent_for_line(ctx: &Ctx, text: &Text, line: usize) -> Option<String> {
    let m = Model::new(text);
    let mut l = line;
    while l > 0 {
        l -= 1;
        if m.first_non_ws(l).is_some() {
            let ind = m.indentation(l);
            let content = text.line(l).to_string();
            let t = content.trim_end();
            let opens = ctx
                .cfg
                .brackets
                .iter()
                .any(|(o, _)| t.ends_with(o.as_str()));
            let ind = if opens {
                format!("{ind}{}", ctx.cfg.indent_unit())
            } else {
                ind
            };
            return (!ind.is_empty()).then_some(ind);
        }
    }
    None
}

pub(crate) fn tab(ctx: &Ctx, st: &St) -> Option<Out> {
    let cfg = ctx.cfg;
    let text = &st.text;
    let m = Model::new(text);
    // Line shifts for multi-line or whole-line selections; replacements otherwise.
    let mut per: Vec<LineShift> = Vec::new();
    for r in st.sel.ranges() {
        let line = text.byte_to_line(r.from());
        let jump = |r: &SelRange| {
            let p = m.pos(r.from());
            let chars = m.line_chars(p.line);
            let s = if cfg.insert_spaces {
                let vis = visible_col(&chars, p.col, cfg.tab_size);
                let size = cfg.indent_size.max(1);
                " ".repeat((size - vis % size) as usize)
            } else {
                "\t".to_string()
            };
            (r.range(), s)
        };
        if r.is_empty() {
            let content = text.line(line).to_string();
            if content.chars().all(char::is_whitespace) {
                let good =
                    good_indent_for_line(ctx, text, line).unwrap_or_else(|| "\t".to_string());
                let typ = normalize_indentation(cfg, &good);
                if !content.starts_with(&typ) {
                    per.push((vec![(text.line_range(line), typ)], true, false));
                    continue;
                }
            }
            per.push((vec![jump(r)], true, false));
        } else {
            let end_line = text.byte_to_line(r.to());
            let lr = text.line_range(line);
            if line == end_line && (r.from() != lr.start || r.to() != lr.end) {
                let (range, s) = jump(r);
                per.push((vec![(range, s)], true, false));
                continue;
            }
            let (edits, use_last) = shift_range(ctx, text, r, false);
            let stays = edits
                .first()
                .is_some_and(|(er, _)| text.byte_to_line(er.start) == line && r.from() <= er.end);
            per.push((edits, use_last, stays));
        }
    }
    apply_line_edits(text, &st.sel, per, InputKind::Other)
}

// ---------------------------------------------------------------------------------------------
// comments and lines

pub(crate) fn comment_line(ctx: &Ctx, st: &St) -> Option<Out> {
    let token = ctx.cfg.line_comment.clone()?;
    let text = &st.text;
    let m = Model::new(text);
    let indent = ctx.cfg.indent_size.max(1);
    let mut all: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut done_lines: Vec<usize> = Vec::new();
    let mut delta_cols: Vec<usize> = vec![0; st.sel.len()];
    for (ri, r) in st.sel.ranges().iter().enumerate() {
        let start_line = text.byte_to_line(r.from());
        let mut end_line = text.byte_to_line(r.to());
        if r.to() == text.line_to_byte(end_line) && start_line != end_line {
            end_line -= 1;
        }
        let lines: Vec<usize> = (start_line..=end_line)
            .filter(|l| !done_lines.contains(l))
            .collect();
        done_lines.extend(lines.iter().copied());
        // (line, ignore, offset chars, len chars)
        let mut data: Vec<(usize, bool, usize, usize)> = Vec::new();
        let mut only_ws = true;
        let mut remove = true;
        for &l in &lines {
            let chars = m.line_chars(l);
            match m.first_non_ws(l) {
                None => data.push((l, true, chars.len(), 0)),
                Some(off) => {
                    only_ws = false;
                    let s: String = chars[off..].iter().collect();
                    if s.starts_with(token.as_str()) {
                        let tl = token.chars().count();
                        let extra = usize::from(chars.get(off + tl) == Some(&' '));
                        data.push((l, false, off, tl + extra));
                    } else {
                        remove = false;
                        data.push((l, false, off, 0));
                    }
                }
            }
        }
        if only_ws {
            remove = false;
            for d in &mut data {
                d.1 = false;
            }
        }
        if remove {
            for (l, ignore, off, len) in &data {
                if *ignore {
                    continue;
                }
                let a = m.off(Pos::new(*l, *off));
                let b = m.off(Pos::new(*l, off + len));
                all.push((a..b, String::new()));
            }
        } else {
            // _normalizeInsertionPoint
            let mut min_vis = u32::MAX;
            for (l, ignore, off, _) in &data {
                if *ignore {
                    continue;
                }
                let chars = m.line_chars(*l);
                let mut v = 0u32;
                for c in chars.iter().take(*off) {
                    if v >= min_vis {
                        break;
                    }
                    v = if *c == '\t' {
                        next_indent_tab_stop(v, indent)
                    } else {
                        v + 1
                    };
                }
                min_vis = min_vis.min(v);
            }
            if min_vis == u32::MAX {
                min_vis = 0;
            }
            min_vis = min_vis / indent * indent;
            let cursor = r.head;
            for (l, ignore, off, _) in &data {
                if *ignore {
                    continue;
                }
                let chars = m.line_chars(*l);
                let mut v = 0u32;
                let mut j = 0;
                while j < *off && v < min_vis {
                    v = if chars[j] == '\t' {
                        next_indent_tab_stop(v, indent)
                    } else {
                        v + 1
                    };
                    j += 1;
                }
                let col = if v > min_vis { j - 1 } else { j };
                let at = m.off(Pos::new(*l, col));
                let ins = format!("{token} ");
                if at == cursor && r.is_empty() && text.line_range(*l).end == cursor {
                    delta_cols[ri] = ins.len();
                }
                all.push((at..at, ins));
            }
        }
    }
    if all.is_empty() {
        return None;
    }
    all.sort_by_key(|(r, _)| r.start);
    let changes = ChangeSet::replace(
        text.len_bytes(),
        all.iter().map(|(r, s)| (r.clone(), s.as_str())),
    )
    .ok()?;
    let tracked = track(text, &changes, &st.sel);
    let ranges: Vec<SelRange> = tracked
        .ranges()
        .iter()
        .zip(delta_cols.iter().chain(std::iter::repeat(&0)))
        .map(|(t, d)| SelRange::new(t.anchor + d, t.head + d))
        .collect();
    let sel = if ranges.len() == st.sel.len() {
        Selection::new(ranges, st.sel.primary_index())
    } else {
        tracked
    };
    Some(Out {
        changes,
        sel,
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: false,
    })
}

pub(crate) fn insert_line(ctx: &Ctx, st: &St, after: bool) -> Option<Out> {
    let text = &st.text;
    let edits = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let line = text.byte_to_line(r.head);
            if after {
                let end = text.line_range(line).end;
                Some(enter_edit(ctx, text, end..end))
            } else if line == 0 {
                Some(RangeEdit {
                    range: 0..0,
                    insert: "\n".into(),
                    anchor: 0,
                    head: 0,
                })
            } else {
                let end = text.line_range(line - 1).end;
                Some(enter_edit(ctx, text, end..end))
            }
        })
        .collect();
    let (changes, sel) = build(text, &st.sel, edits);
    out(text, changes, sel, InputKind::Other)
}

pub(crate) fn delete_lines(st: &St) -> Option<Out> {
    let text = &st.text;
    let m = Model::new(text);
    // Merge line spans of all ranges.
    let mut spans: Vec<(usize, usize, usize)> = st
        .sel
        .ranges()
        .iter()
        .map(|r| {
            let s = text.byte_to_line(r.from());
            let mut e = text.byte_to_line(r.to());
            if r.to() == text.line_to_byte(e) && e > s {
                e -= 1;
            }
            (s, e, m.pos(r.head).col)
        })
        .collect();
    spans.sort();
    let mut merged: Vec<(usize, usize, usize)> = Vec::new();
    for sp in spans {
        if let Some(last) = merged.last_mut()
            && sp.0 <= last.1 + 1
        {
            last.1 = last.1.max(sp.1);
            continue;
        }
        merged.push(sp);
    }
    let lc = text.len_lines();
    let mut edits = Vec::new();
    for (s, e, _) in &merged {
        let range = if e + 1 < lc {
            text.line_to_byte(*s)..text.line_to_byte(e + 1)
        } else if *s > 0 {
            text.line_range(s - 1).end..text.len_bytes()
        } else {
            0..text.len_bytes()
        };
        edits.push((range, ""));
    }
    let changes = ChangeSet::replace(text.len_bytes(), edits).ok()?;
    if changes.is_empty() {
        return None;
    }
    let new_text = changes.apply(text).ok()?;
    let nm = Model::new(&new_text);
    let mut removed_before = 0;
    let ranges: Vec<SelRange> = merged
        .iter()
        .map(|(s, e, col)| {
            let line = (s - removed_before).min(new_text.len_lines() - 1);
            removed_before += e - s + 1;
            SelRange::point(nm.off(Pos::new(line, (*col).min(nm.max_col(line)))))
        })
        .collect();
    let sel = Selection::new(ranges, 0);
    Some(Out {
        changes,
        sel,
        kind: InputKind::Other,
        auto_closed: Vec::new(),
        close_before: false,
    })
}
