//! Inserting a snippet at every cursor and the snippet session that follows (VS Code
//! `SnippetSession`, `OneSnippet` and `SnippetController2`, `editor/contrib/snippet/browser/`
//! at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`), and accepting a suggestion
//! (`SuggestController._insertSuggestion`).
//!
//! The session keeps one group of ranges per placeholder index (every cursor's snippet, every
//! mirror), ordered by index with the final tabstop `$0` last. The active group's ranges grow
//! when typing at their edges, the others do not. Departures (ours): variables resolve from
//! the selection, the line and the comment tokens only (no clipboard, time, random or workspace
//! variables; unresolved ones insert their default); a snippet inserted inside a session
//! replaces it (VS Code merges); undo ends the session.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use super::snippet::{Marker, NodeId, Snippet, Transform};
use crate::change::{Bias, ChangeSet};
use crate::config::EditorConfig;
use crate::document::Document;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use std::ops::Range;

/// One placeholder index's ranges.
#[derive(Clone, PartialEq, Debug)]
pub struct TabstopGroup {
    /// The placeholder index (`0`: the final tabstop).
    pub index: u32,
    /// Its ranges (every cursor, every mirror), sorted.
    pub ranges: Vec<Range<usize>>,
    /// A choice placeholder's options.
    pub choice: Option<Vec<String>>,
    /// A transform applied when leaving it.
    pub transform: Option<Transform>,
}

/// An active snippet (VS Code: `SnippetController2` in snippet mode).
#[derive(Clone, PartialEq, Debug)]
pub struct SnippetSession {
    /// The placeholder groups in visiting order.
    pub groups: Vec<TabstopGroup>,
    /// The active group.
    pub current: usize,
}

impl SnippetSession {
    /// `hasNextTabstop`.
    pub fn has_next(&self) -> bool {
        !self.is_at_last()
    }

    /// `hasPrevTabstop`.
    pub fn has_prev(&self) -> bool {
        self.current > 0
    }

    /// VS Code `isAtLastPlaceholder`.
    pub fn is_at_last(&self) -> bool {
        self.current + 1 >= self.groups.len()
    }

    /// The active group.
    pub fn active(&self) -> &TabstopGroup {
        &self.groups[self.current]
    }

    /// Whether every selection range lies in an active range (VS Code
    /// `isSelectionWithinPlaceholders`).
    pub fn contains_selection(&self, sel: &Selection) -> bool {
        let g = self.active();
        sel.ranges().iter().all(|r| {
            g.ranges
                .iter()
                .any(|p| p.start <= r.from() && r.to() <= p.end)
        })
    }

    /// Maps the ranges through a change: the active group's ranges grow at their edges, the
    /// others stay put.
    pub(crate) fn map(&mut self, c: &ChangeSet) {
        let cur = self.current;
        for (i, g) in self.groups.iter_mut().enumerate() {
            for r in &mut g.ranges {
                let (s, e) = if i == cur {
                    (c.map_pos(r.start, Bias::Before), c.map_pos(r.end, Bias::After))
                } else if r.start == r.end {
                    let p = c.map_pos(r.start, Bias::Before);
                    (p, p)
                } else {
                    (c.map_pos(r.start, Bias::After), c.map_pos(r.end, Bias::Before))
                };
                *r = s..e.max(s);
            }
        }
    }

    /// The selection for group `i` (primary: the range of the primary cursor's snippet).
    pub(crate) fn selection_of(&self, i: usize, primary_hint: usize) -> Selection {
        let g = &self.groups[i];
        let primary = g
            .ranges
            .iter()
            .position(|r| r.start >= primary_hint)
            .unwrap_or(0)
            .min(g.ranges.len().saturating_sub(1));
        Selection::new(
            g.ranges.iter().map(|r| SelRange::new(r.start, r.end)),
            primary,
        )
    }

    /// The edits leaving group `current` applies (its transform), and the move to `to`:
    /// changes, selection and the session afterwards (`None` when it ends).
    pub(crate) fn move_to(
        &self,
        text: &Text,
        to: usize,
        primary_hint: usize,
    ) -> (ChangeSet, Selection, Option<SnippetSession>) {
        let len = text.len_bytes();
        let cur = &self.groups[self.current];
        let edits: Vec<(Range<usize>, String)> = match &cur.transform {
            Some(t) => cur
                .ranges
                .iter()
                .filter_map(|r| {
                    let old = text.slice(r.clone()).to_string();
                    let new = t.resolve(&old);
                    (new != old).then(|| (r.clone(), new))
                })
                .collect(),
            None => Vec::new(),
        };
        let cs = ChangeSet::replace(len, edits).unwrap_or_else(|_| ChangeSet::identity(len));
        let mut next = self.clone();
        next.map(&cs);
        next.current = to;
        let sel = next.selection_of(to, cs.map_pos(primary_hint, Bias::Before));
        let keep = !next.is_at_last();
        (cs, sel, keep.then_some(next))
    }
}

/// VS Code `normalizeIndentation`: the leading whitespace rewritten with tabs or spaces.
pub(crate) fn normalize_indentation(s: &str, config: &EditorConfig) -> String {
    let ws_len = s.len() - s.trim_start_matches([' ', '\t']).len();
    if ws_len == 0 {
        return s.to_string();
    }
    let tab = config.tab_size.max(1) as usize;
    let indent = config.indent_size.max(1) as usize;
    let mut col = 0usize;
    for c in s[..ws_len].chars() {
        col = if c == '\t' { (col / tab + 1) * tab } else { col + 1 };
    }
    let mut out = String::new();
    if !config.insert_spaces {
        for _ in 0..col / tab {
            out.push('\t');
        }
        col %= tab;
    } else {
        let _ = indent;
    }
    for _ in 0..col {
        out.push(' ');
    }
    out + &s[ws_len..]
}

fn first_non_ws(text: &Text, line: usize) -> usize {
    let l = text.line(line).to_string();
    l.len() - l.trim_start_matches([' ', '\t']).len()
}

/// VS Code `SnippetSession.adjustWhitespace`: indent the snippet's lines like the insert line
/// (and normalise their indentation).
fn adjust_whitespace(snippet: &mut Snippet, line_leading_ws: &str, adjust: bool, cfg: &EditorConfig) {
    if !adjust {
        return;
    }
    let mut texts: Vec<NodeId> = Vec::new();
    snippet.walk(|m| {
        if let Marker::Text(_) = snippet.marker(m)
            && !snippet
                .parent(m)
                .is_some_and(|p| matches!(snippet.marker(p), Marker::Choice(_)))
        {
            texts.push(m);
        }
        true
    });
    for m in texts {
        let Marker::Text(value) = snippet.marker(m).clone() else {
            continue;
        };
        let mut lines: Vec<String> = value.split('\n').map(str::to_string).collect();
        let offset = snippet.offset(m).unwrap_or(0);
        if offset == 0 {
            lines[0] = normalize_indentation(&lines[0], cfg);
        } else {
            let s = snippet.to_string();
            if s.as_bytes().get(offset - 1) == Some(&b'\n') {
                lines[0] = normalize_indentation(&format!("{line_leading_ws}{}", lines[0]), cfg);
            }
        }
        for l in lines.iter_mut().skip(1) {
            *l = normalize_indentation(&format!("{line_leading_ws}{l}"), cfg);
        }
        let new = lines.join("\n");
        if new != value {
            snippet.set_text(m, new);
        }
    }
}

/// The selection-, line- and comment-based snippet variables of one cursor.
fn resolve_variable(
    doc: &Document,
    sel: SelRange,
    idx: usize,
    name: &str,
) -> Option<String> {
    let text = doc.text();
    let (line, col) = text.byte_to_line_col(sel.head);
    let cfg = doc.config();
    match name {
        "SELECTION" | "TM_SELECTED_TEXT" => {
            let v = text.slice(sel.range()).to_string();
            (!v.is_empty()).then_some(v)
        }
        "TM_CURRENT_LINE" => Some(text.line(line).to_string()),
        "TM_CURRENT_WORD" => {
            super::session::word_at(&text.line(line).to_string(), col, &cfg.word_separators)
                .map(|w| w.word)
        }
        "TM_LINE_INDEX" => Some(line.to_string()),
        "TM_LINE_NUMBER" => Some((line + 1).to_string()),
        "CURSOR_INDEX" => Some(idx.to_string()),
        "CURSOR_NUMBER" => Some((idx + 1).to_string()),
        "LINE_COMMENT" => cfg.line_comment.clone(),
        "BLOCK_COMMENT_START" => cfg.block_comment.as_ref().map(|b| b.0.clone()),
        "BLOCK_COMMENT_END" => cfg.block_comment.as_ref().map(|b| b.1.clone()),
        _ => None,
    }
}

/// A snippet inserted at every cursor: the change set (with `extra` edits applied in the same
/// step), the selection and the session (`None` without placeholders).
pub(crate) struct Inserted {
    pub changes: ChangeSet,
    pub selection: Selection,
    pub session: Option<SnippetSession>,
}

/// VS Code `SnippetSession.createEditsAndSnippetsFromSelections` + `insert`: each cursor's
/// range grows by `overwrite_before`/`overwrite_after` (when that grows the primary cursor's
/// range over the same text), the template is parsed, indented and resolved per cursor.
pub(crate) fn insert_snippet(
    doc: &Document,
    sel: &Selection,
    template: &str,
    overwrite_before: usize,
    overwrite_after: usize,
    adjust_ws: bool,
    extra: Vec<(Range<usize>, String)>,
) -> Option<Inserted> {
    let text = doc.text();
    let cfg = doc.config();
    let len = text.len_bytes();
    let line_bounds = |p: usize| text.line_range(text.byte_to_line(p));
    // VS Code adjustSelection: the range around the position, on its line.
    let extend = |r: SelRange, before: usize, after: usize| -> Range<usize> {
        if before == 0 && after == 0 {
            return r.range();
        }
        let lb = line_bounds(r.head);
        let s = text.clamp_to_boundary(r.head.saturating_sub(before).max(lb.start));
        let e = text.clamp_to_boundary((r.head + after).min(lb.end));
        s..e
    };
    let primary = sel.primary();
    let first_before = text
        .slice(extend(primary, overwrite_before, 0))
        .to_string();
    let first_after = text.slice(extend(primary, 0, overwrite_after)).to_string();
    let first_nonws = first_non_ws(text, text.byte_to_line(primary.head));

    let mut edits: Vec<(Range<usize>, Snippet, usize)> = Vec::new();
    for (idx, r) in sel.ranges().iter().enumerate() {
        let mut before = extend(*r, overwrite_before, 0);
        let mut after = extend(*r, 0, overwrite_after);
        if text.slice(before.clone()).to_string() != first_before {
            before = r.range();
        }
        if text.slice(after.clone()).to_string() != first_after {
            after = r.range();
        }
        let range = before.start..after.end.max(before.start);
        let mut snippet = Snippet::parse(template, true, false);
        let start_line = text.byte_to_line(range.start);
        let lb = text.line_range(start_line);
        let line = text.line(start_line).to_string();
        let upto = &line[..range.start - lb.start];
        let lead = &upto[..upto.len() - upto.trim_start_matches([' ', '\t']).len()];
        let is_primary = idx == sel.primary_index();
        let adjust = adjust_ws
            || (!is_primary && first_nonws != first_non_ws(text, text.byte_to_line(r.head)));
        adjust_whitespace(&mut snippet, lead, adjust, cfg);
        let resolver = |name: &str| resolve_variable(doc, *r, idx, name);
        snippet.resolve_variables(&resolver);
        edits.push((range, snippet, idx));
    }
    let mut all: Vec<(Range<usize>, String)> = edits
        .iter()
        .map(|(r, s, _)| (r.clone(), s.to_string()))
        .collect();
    let snippet_only = all.clone();
    all.extend(extra);
    let changes = ChangeSet::replace(len, all)
        .or_else(|_| ChangeSet::replace(len, snippet_only))
        .ok()?;

    // Placeholder groups in the new text.
    let mut groups: Vec<TabstopGroup> = Vec::new();
    let mut ends: Vec<(usize, bool)> = Vec::new(); // end of each insertion, primary?
    for (range, snippet, idx) in &edits {
        let base = changes.map_pos(range.start, Bias::Before);
        let ins_len = snippet.to_string().len();
        ends.push((base + ins_len, *idx == sel.primary_index()));
        for p in snippet.placeholders() {
            let index = snippet.placeholder_index(p).expect("placeholder");
            let start = base + snippet.offset(p).unwrap_or(0);
            let r = start..start + snippet.full_len(p);
            let transform = match snippet.marker(p) {
                Marker::Placeholder { transform, .. } => transform.clone(),
                _ => None,
            };
            match groups.iter_mut().find(|g| g.index == index) {
                Some(g) => {
                    g.ranges.push(r);
                    if g.transform.is_none() {
                        g.transform = transform;
                    }
                }
                None => groups.push(TabstopGroup {
                    index,
                    ranges: vec![r],
                    choice: snippet.choice(p).map(<[String]>::to_vec),
                    transform,
                }),
            }
        }
    }
    let primary_new = changes.map_pos(primary.head, Bias::Before);
    if groups.is_empty() {
        // no placeholders: cursors after the inserted text
        let p = ends.iter().position(|(_, p)| *p).unwrap_or(0);
        let selection = Selection::new(ends.iter().map(|(e, _)| SelRange::point(*e)), p);
        return Some(Inserted {
            changes,
            selection,
            session: None,
        });
    }
    groups.sort_by(|a, b| match (a.index, b.index) {
        (x, y) if x == y => std::cmp::Ordering::Equal,
        (0, _) => std::cmp::Ordering::Greater,
        (_, 0) => std::cmp::Ordering::Less,
        (x, y) => x.cmp(&y),
    });
    for g in &mut groups {
        g.ranges.sort_by_key(|r| (r.start, r.end));
        g.ranges.dedup();
    }
    let session = SnippetSession { groups, current: 0 };
    let selection = session.selection_of(0, primary_new);
    let keep = !session.is_at_last();
    Some(Inserted {
        changes,
        selection,
        session: keep.then_some(session),
    })
}
