//! Inserting a snippet at every cursor and the snippet session that follows (VS Code
//! `SnippetSession`, `OneSnippet` and `SnippetController2`, `editor/contrib/snippet/browser/`
//! at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`), and accepting a suggestion
//! (`SuggestController._insertSuggestion`).
//!
//! The session keeps one group of ranges per placeholder index (every cursor's snippet, every
//! mirror), ordered by index with the final tabstop `$0` last. The active group's ranges grow
//! when typing at their edges, the others do not. Departures (ours): variables resolve from
//! the selection, the line and the comment tokens only (no clipboard, time, random or workspace
//! variables; unresolved ones insert their default); undo ends the session. A snippet inserted
//! in snippet mode is merged into the session (`SnippetSession.merge`).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use super::snippet::{Marker, NodeId, Snippet, Transform};
use crate::change::{Bias, ChangeSet};
use crate::config::EditorConfig;
use crate::document::Document;
use crate::selection::{SelRange, Selection};
use crate::text::Text;
use crate::view::EditorView;
use std::ops::Range;

/// One placeholder occurrence: a placeholder of one cursor's snippet and its tracked range
/// (VS Code: a `Placeholder` of a `OneSnippet` and its decoration).
#[derive(Clone, PartialEq, Debug)]
struct Stop {
    range: Range<usize>,
    /// The innermost enclosing placeholder occurrence (`enclosingPlaceholders`).
    parent: Option<usize>,
    /// The length of its text in the snippet (`marker.toString().length`), for
    /// `_hasPlaceholderBeenCollapsed`.
    authored_len: usize,
    /// Whether it belongs to the primary cursor's snippet (VS Code `_snippets[0]`).
    primary: bool,
    /// False once a nested snippet replaced it (or an enclosing placeholder).
    live: bool,
    /// A transform applied to its text when leaving it.
    transform: Option<Transform>,
}

/// One placeholder index's occurrences.
#[derive(Clone, PartialEq, Debug)]
pub struct TabstopGroup {
    /// The placeholder index (`0`: the final tabstop). After a merge the indexes are
    /// renumbered `1..` in visiting order (VS Code `_renormalizePlaceholderIndices`).
    pub index: u32,
    /// Its occurrences (every cursor, every mirror), sorted by position.
    stops: Vec<usize>,
    /// A choice placeholder's options.
    pub choice: Option<Vec<String>>,
}

/// An active snippet (VS Code: `SnippetController2` in snippet mode, its `SnippetSession`
/// and one `OneSnippet` per cursor, kept as one list of groups: every cursor's snippet has the
/// same placeholders).
#[derive(Clone, PartialEq, Debug)]
pub struct SnippetSession {
    stops: Vec<Stop>,
    /// The placeholder groups in visiting order.
    groups: Vec<TabstopGroup>,
    /// The active group.
    current: usize,
    /// The number of cursors the snippet was inserted at (`_snippets.length`).
    snippets: usize,
}

/// A parsed snippet's placeholders as stops: `(original index, stop, node)` in walk order.
struct Added {
    stops: Vec<(u32, usize, NodeId)>,
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

    /// The index of the active group.
    pub fn current(&self) -> usize {
        self.current
    }

    /// The placeholder groups in visiting order.
    pub fn groups(&self) -> &[TabstopGroup] {
        &self.groups
    }

    /// The active group.
    pub fn active(&self) -> &TabstopGroup {
        &self.groups[self.current]
    }

    /// The ranges of group `i` (every cursor, every mirror), sorted.
    pub fn ranges(&self, i: usize) -> Vec<Range<usize>> {
        self.groups[i]
            .stops
            .iter()
            .map(|&s| self.stops[s].range.clone())
            .collect()
    }

    /// The active group's ranges.
    pub fn active_ranges(&self) -> Vec<Range<usize>> {
        self.ranges(self.current)
    }

    /// Whether the selection still matches one placeholder index's ranges (VS Code
    /// `isSelectionWithinPlaceholders`): a non-final index whose first range in the primary
    /// cursor's snippet contains a selection, with as many ranges as selections, each
    /// containing the selection at its place.
    pub fn contains_selection(&self, sel: &Selection) -> bool {
        if self.stops.is_empty() || sel.len() < self.snippets {
            return false;
        }
        let mut sels: Vec<Range<usize>> = sel.ranges().iter().map(|r| r.range()).collect();
        sels.sort_by_key(|r| (r.start, r.end));
        let contains = |p: &Range<usize>, r: &Range<usize>| p.start <= r.start && r.end <= p.end;
        self.groups.iter().filter(|g| g.index != 0).any(|g| {
            let first = g
                .stops
                .iter()
                .map(|&s| &self.stops[s])
                .filter(|s| s.primary)
                .map(|s| s.range.clone())
                .min_by_key(|r| (r.start, r.end));
            let Some(first) = first else {
                return false;
            };
            if !sels.iter().any(|r| contains(&first, r)) {
                return false;
            }
            let mut ranges: Vec<Range<usize>> = g
                .stops
                .iter()
                .map(|&s| self.stops[s].range.clone())
                .collect();
            ranges.sort_by_key(|r| (r.start, r.end));
            ranges.len() == sels.len() && ranges.iter().zip(&sels).all(|(p, r)| contains(p, r))
        })
    }

    /// The active stops and the placeholders enclosing them (VS Code: the decorations that
    /// grow when typing at their edges).
    fn active_set(&self) -> Vec<bool> {
        let mut on = vec![false; self.stops.len()];
        for &s in &self.groups[self.current].stops {
            let mut m = Some(s);
            while let Some(i) = m {
                on[i] = true;
                m = self.stops[i].parent;
            }
        }
        on
    }

    /// Maps the ranges through a change: the active placeholders (and those enclosing them)
    /// grow at their edges, the others do not.
    pub(crate) fn map(&mut self, c: &ChangeSet) {
        let active = self.active_set();
        // An empty inactive placeholder at the end of an active one follows what is typed
        // there (VS Code: `$1$2` keeps `$2` after the text typed into `$1`).
        let active_ends: Vec<usize> = self.groups[self.current]
            .stops
            .iter()
            .map(|&s| self.stops[s].range.end)
            .collect();
        for (i, st) in self.stops.iter_mut().enumerate() {
            if !st.live {
                continue;
            }
            let r = &mut st.range;
            let (s, e) = if active[i] {
                (
                    c.map_pos(r.start, Bias::Before),
                    c.map_pos(r.end, Bias::After),
                )
            } else if r.start == r.end {
                let bias = if active_ends.contains(&r.start) {
                    Bias::After
                } else {
                    Bias::Before
                };
                let p = c.map_pos(r.start, bias);
                (p, p)
            } else {
                (
                    c.map_pos(r.start, Bias::After),
                    c.map_pos(r.end, Bias::Before),
                )
            };
            *r = s..e.max(s);
        }
    }

    /// The selection for group `i` (primary: the primary cursor's first occurrence).
    pub(crate) fn selection_of(&self, i: usize) -> Selection {
        let g = &self.groups[i];
        let primary = g
            .stops
            .iter()
            .position(|&s| self.stops[s].primary)
            .unwrap_or(0);
        Selection::new(
            g.stops.iter().map(|&s| {
                let r = &self.stops[s].range;
                SelRange::new(r.start, r.end)
            }),
            primary,
        )
    }

    /// VS Code `_hasPlaceholderBeenCollapsed`: it, or a placeholder enclosing it, was not
    /// empty when authored and is empty now.
    fn collapsed(&self, s: usize) -> bool {
        let mut m = Some(s);
        while let Some(i) = m {
            let st = &self.stops[i];
            if st.range.is_empty() && st.authored_len > 0 {
                return true;
            }
            m = st.parent;
        }
        false
    }

    /// The edits of the active group's transform.
    fn transform_edits(&self, text: &Text) -> ChangeSet {
        let len = text.len_bytes();
        let edits: Vec<(Range<usize>, String)> = self.groups[self.current]
            .stops
            .iter()
            .filter_map(|&s| {
                let st = &self.stops[s];
                let t = st.transform.as_ref()?;
                let old = text.slice(st.range.clone()).to_string();
                let new = t.resolve(&old);
                (new != old).then_some((st.range.clone(), new))
            })
            .collect();
        ChangeSet::replace(len, edits).unwrap_or_else(|_| ChangeSet::identity(len))
    }

    /// VS Code `OneSnippet.move(fwd)` and the controller's state update: the edits leaving
    /// the active group applies (its transform), the move (skipping groups whose placeholders
    /// were deleted), the selection and the session afterwards (`None` when it ends).
    pub(crate) fn move_by(
        &self,
        text: &Text,
        fwd: bool,
    ) -> (ChangeSet, Selection, Option<SnippetSession>) {
        let mut next = self.clone();
        let mut text = text.clone();
        let mut cs = ChangeSet::identity(text.len_bytes());
        loop {
            let step = next.transform_edits(&text);
            next.map(&step);
            if let Ok(t) = step.apply(&text) {
                text = t;
            }
            cs = cs.compose(&step).unwrap_or(cs);
            let moved = if fwd && next.current + 1 < next.groups.len() {
                next.current += 1;
                true
            } else if !fwd && next.current > 0 {
                next.current -= 1;
                true
            } else {
                false
            };
            let skip = moved
                && next.groups[next.current]
                    .stops
                    .iter()
                    .all(|&s| next.collapsed(s));
            if !skip {
                break;
            }
        }
        let sel = next.selection_of(next.current);
        let keep = !next.is_at_last() && next.contains_selection(&sel);
        (cs, sel, keep.then_some(next))
    }

    /// Adds a parsed snippet's placeholders inserted at `base`; top-level ones get `parent`.
    fn add_stops(
        &mut self,
        snippet: &Snippet,
        base: usize,
        parent: Option<usize>,
        primary: bool,
    ) -> Added {
        let mut by_node: Vec<(NodeId, usize)> = Vec::new();
        let mut stops = Vec::new();
        for p in snippet.placeholders() {
            let index = snippet.placeholder_index(p).expect("placeholder");
            let start = base + snippet.offset(p).unwrap_or(0);
            let len = snippet.full_len(p);
            let up = snippet
                .enclosing_placeholders(p)
                .first()
                .and_then(|e| by_node.iter().find(|(n, _)| n == e).map(|&(_, s)| s))
                .or(parent);
            let transform = match snippet.marker(p) {
                Marker::Placeholder { transform, .. } => transform.clone(),
                _ => None,
            };
            let id = self.stops.len();
            self.stops.push(Stop {
                range: start..start + len,
                parent: up,
                authored_len: len,
                primary,
                live: true,
                transform,
            });
            by_node.push((p, id));
            stops.push((index, id, p));
        }
        Added { stops }
    }

    /// Groups added stops by index: other indexes ascending, then the final tabstop.
    fn group_added(&self, added: &[(Added, &Snippet)]) -> Vec<TabstopGroup> {
        let mut groups: Vec<TabstopGroup> = Vec::new();
        for (a, snippet) in added {
            for &(index, id, p) in &a.stops {
                match groups.iter_mut().find(|g| g.index == index) {
                    Some(g) => g.stops.push(id),
                    None => groups.push(TabstopGroup {
                        index,
                        stops: vec![id],
                        choice: snippet.choice(p).map(<[String]>::to_vec),
                    }),
                }
            }
        }
        groups.sort_by(|a, b| match (a.index, b.index) {
            (x, y) if x == y => std::cmp::Ordering::Equal,
            (0, _) => std::cmp::Ordering::Greater,
            (_, 0) => std::cmp::Ordering::Less,
            (x, y) => x.cmp(&y),
        });
        for g in &mut groups {
            g.stops
                .sort_by_key(|&s| (self.stops[s].range.start, self.stops[s].range.end));
        }
        groups
    }

    /// VS Code `OneSnippet.merge`: each active placeholder is replaced by the nested snippet
    /// inserted in its place (`nested[k]` at `bases[k]`, paired by position). The nested
    /// placeholders are visited right after the replaced one, its final tabstop last; then
    /// the indexes are renumbered. The active group index stays (`_placeholderGroupsIdx`), so
    /// the first nested placeholder becomes active.
    fn merge(&mut self, nested: &[(usize, &Snippet)]) {
        let mut active: Vec<usize> = self.groups[self.current].stops.clone();
        active.sort_by_key(|&s| (self.stops[s].range.start, self.stops[s].range.end));
        let mut added = Vec::new();
        for (&old, &(base, snippet)) in active.iter().zip(nested) {
            let st = self.stops[old].clone();
            let a = self.add_stops(snippet, base, st.parent, st.primary);
            // the enclosing placeholders' text now has the nested text in place of the old
            let new_len = snippet.to_string().len();
            let mut m = st.parent;
            while let Some(i) = m {
                let s = &mut self.stops[i];
                s.authored_len = s.authored_len.saturating_sub(st.authored_len) + new_len;
                m = s.parent;
            }
            added.push((a, snippet));
        }
        // the replaced placeholders and everything inside them leave the snippet
        for i in 0..self.stops.len() {
            let mut m = Some(i);
            while let Some(j) = m {
                if active.contains(&j) {
                    self.stops[i].live = false;
                    break;
                }
                m = self.stops[j].parent;
            }
        }
        let mut nested_groups = self.group_added(&added);
        for g in &mut nested_groups {
            g.index = u32::MAX; // renumbered below; the nested final tabstop is not final
        }
        let cur = self.current;
        let stops = &self.stops;
        let mut groups: Vec<TabstopGroup> = Vec::new();
        for (i, g) in std::mem::take(&mut self.groups).into_iter().enumerate() {
            if i == cur {
                groups.append(&mut nested_groups);
                continue;
            }
            let mut g = g;
            g.stops.retain(|&s| stops[s].live);
            if !g.stops.is_empty() {
                groups.push(g);
            }
        }
        let mut n = 0;
        for g in &mut groups {
            if g.index != 0 {
                n += 1;
                g.index = n;
            }
        }
        self.current = cur.min(groups.len().saturating_sub(1));
        self.groups = groups;
    }
}

/// VS Code `OneSnippet.isTrivialSnippet`: no placeholder, or only a final tabstop at the end.
fn is_trivial(snippet: &Snippet) -> bool {
    let ps = snippet.placeholders();
    match ps.as_slice() {
        [] => true,
        [p] => {
            snippet.placeholder_index(*p) == Some(0)
                && snippet.right_most_descendant(Snippet::ROOT) == *p
        }
        _ => false,
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
        col = if c == '\t' {
            (col / tab + 1) * tab
        } else {
            col + 1
        };
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
fn adjust_whitespace(
    snippet: &mut Snippet,
    line_leading_ws: &str,
    adjust: bool,
    cfg: &EditorConfig,
) {
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
        let value_n = crate::text::normalize_line_endings(&value).into_owned();
        let mut lines: Vec<String> = value_n.split('\n').map(str::to_string).collect();
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

/// VS Code `SnippetSession.adjustWhitespace(model, position, true, snippet)`: indents and
/// normalises a snippet's text for insertion at `pos`.
pub fn adjust_snippet_whitespace(doc: &Document, pos: usize, snippet: &mut Snippet) {
    let text = doc.text();
    let line = text.byte_to_line(pos);
    let start = text.line_to_byte(line);
    let upto = text.slice(start..pos).to_string();
    let lead = &upto[..upto.len() - upto.trim_start_matches([' ', '\t']).len()];
    adjust_whitespace(snippet, lead, true, doc.config());
}

/// The selection-, line- and comment-based snippet variables of one cursor.
fn resolve_variable(doc: &Document, sel: SelRange, idx: usize, name: &str) -> Option<String> {
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
/// step), the selection and the session (`None` without placeholders, or when the snippet
/// mode ends).
pub(crate) struct Inserted {
    pub changes: ChangeSet,
    pub selection: Selection,
    pub session: Option<SnippetSession>,
}

/// VS Code `SnippetSession.createEditsAndSnippetsFromSelections` + `insert` (or `merge` into
/// the view's snippet mode): each cursor's range grows by
/// `overwrite_before`/`overwrite_after` (when that grows the primary cursor's range over the
/// same text), the template is parsed, indented and resolved per cursor.
pub(crate) fn insert_snippet(
    doc: &Document,
    view: &EditorView,
    template: &str,
    overwrite_before: usize,
    overwrite_after: usize,
    adjust_ws: bool,
    extra: Vec<(Range<usize>, String)>,
) -> Option<Inserted> {
    let sel = view.selection();
    let session = view.snippet();
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
    let first_before = text.slice(extend(primary, overwrite_before, 0)).to_string();
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
        let mut snippet = Snippet::parse(template, true, session.is_some());
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

    let primary_idx = sel.primary_index();
    let ends: Vec<SelRange> = edits
        .iter()
        .map(|(range, snippet, _)| {
            SelRange::point(changes.map_pos(range.start, Bias::Before) + snippet.to_string().len())
        })
        .collect();
    let at_ends = || Selection::new(ends.iter().copied(), primary_idx);

    if let Some(outer) = session {
        // VS Code `SnippetSession.merge`: a non-trivial snippet inserted at each active
        // placeholder is merged into the session; otherwise the text is inserted and the
        // session goes on with the cursors after it.
        let mut next = outer.clone();
        next.map(&changes);
        let trivial = is_trivial(&edits[primary_idx].1);
        let can_merge = edits.len() == outer.active().stops.len();
        let (selection, next) = if !trivial && can_merge {
            let mut nested: Vec<(usize, &Snippet)> = edits
                .iter()
                .map(|(range, snippet, _)| (changes.map_pos(range.start, Bias::Before), snippet))
                .collect();
            nested.sort_by_key(|(base, _)| *base);
            next.merge(&nested);
            (next.selection_of(next.current), next)
        } else {
            (at_ends(), next)
        };
        let keep = !next.is_at_last() && next.contains_selection(&selection);
        return Some(Inserted {
            changes,
            selection,
            session: keep.then_some(next),
        });
    }

    let mut session = SnippetSession {
        stops: Vec::new(),
        groups: Vec::new(),
        current: 0,
        snippets: edits.len(),
    };
    let mut added = Vec::new();
    for (range, snippet, idx) in &edits {
        let base = changes.map_pos(range.start, Bias::Before);
        let a = session.add_stops(snippet, base, None, *idx == primary_idx);
        added.push((a, snippet));
    }
    session.groups = session.group_added(&added);
    if session.groups.is_empty() {
        // no placeholders: cursors after the inserted text
        return Some(Inserted {
            changes,
            selection: at_ends(),
            session: None,
        });
    }
    let selection = session.selection_of(0);
    // SnippetController2._updateState: snippet mode ends at the last placeholder and when the
    // selection does not match the placeholders.
    let keep = !session.is_at_last() && session.contains_selection(&selection);
    Some(Inserted {
        changes,
        selection,
        session: keep.then_some(session),
    })
}
