//! Our query runner over `QueryCursor` (E5): highlights, folds, brackets and indents across
//! layers.

use std::ops::Range;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor};

use crate::config;
use crate::source::{Chunks, Source};
use crate::style;
use crate::tree::{Layer, Layers};

/// Per-line token runs: for each line, `(start byte within the line, style)`; a run lasts until
/// the next one or the end of the line. Style `0` is "no token".
pub(crate) type LineRuns = Vec<Vec<(u32, u32)>>;

/// Highlights `lines` (end exclusive, clamped to the text).
pub(crate) fn highlight<S: Source + ?Sized>(
    layers: &Layers,
    src: &S,
    lines: Range<usize>,
) -> LineRuns {
    let n_lines = src.len_lines();
    let lines = lines.start.min(n_lines)..lines.end.min(n_lines);
    if lines.is_empty() {
        return Vec::new();
    }
    let start = src.line_to_byte(lines.start);
    let end = if lines.end >= n_lines {
        src.len_bytes()
    } else {
        src.line_to_byte(lines.end)
    };
    let chunks = Chunks::new(src);
    let mut paint = vec![style::NONE; end - start];

    // (depth, start, Reverse(end), order) sorts parents first, then outer before inner; the
    // stable order keeps later patterns on the same node after earlier ones (last wins).
    let mut spans: Vec<(u32, usize, usize, u32)> = Vec::new();
    let mut clipped = Vec::new();
    let mut cursor = QueryCursor::new();
    for layer in layers.intersecting(start..end) {
        let Some(cfg) = config::get(layer.lang) else {
            continue;
        };
        let sh = layer.shift;
        cursor.set_byte_range(sh.tree(start)..sh.tree(end));
        let text = |n: Node<'_>| chunks.slices(sh.real(n.start_byte())..sh.real(n.end_byte()));
        let mut caps = cursor.captures(&cfg.highlights, layer.data.tree.root_node(), text);
        while let Some((m, i)) = caps.next() {
            let c = m.captures[*i];
            let Some(Some(st)) = cfg.capture_styles.get(c.index as usize) else {
                continue;
            };
            clipped.clear();
            clip(
                layer,
                sh.real(c.node.start_byte())..sh.real(c.node.end_byte()),
                &mut clipped,
            );
            spans.extend(clipped.iter().map(|&(s, e)| (layer.depth, s, e, *st)));
        }
    }
    spans.sort_by(|a, b| (a.0, a.1, b.2).cmp(&(b.0, b.1, a.2)));
    for (_, s, e, st) in spans {
        let (s, e) = (s.max(start), e.min(end));
        if s < e {
            paint[s - start..e - start].fill(st);
        }
    }

    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let ls = src.line_to_byte(line);
        let mut le = if line + 1 >= n_lines {
            src.len_bytes()
        } else {
            src.line_to_byte(line + 1)
        };
        if le > ls && chunks.at(le - 1).first() == Some(&b'\n') {
            le -= 1;
        }
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut last = style::NONE;
        for b in ls..le {
            let st = paint[b - start];
            if st != last {
                runs.push(((b - ls) as u32, st));
                last = st;
            }
        }
        out.push(runs);
    }
    out
}

/// A node's range clipped to the layer's included ranges (an injected node may span gaps).
fn clip(layer: &Layer, r: Range<usize>, out: &mut Vec<(usize, usize)>) {
    if layer.data.ranges.is_empty() {
        out.push((r.start, r.end));
        return;
    }
    for x in layer.ranges() {
        let s = r.start.max(x.start);
        let e = r.end.min(x.end);
        if s < e {
            out.push((s, e));
        }
    }
}

/// A fold: lines `start..=end`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Fold {
    pub start: usize,
    pub end: usize,
    pub comment: bool,
}

/// Multi-line `@fold` nodes of every layer, one per start line (the longest), sorted.
pub(crate) fn folds(layers: &Layers) -> Vec<Fold> {
    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    for layer in &layers.layers {
        let Some(cfg) = config::get(layer.lang) else {
            continue;
        };
        let Some(q) = &cfg.folds else { continue };
        let Some(fold) = q.a else { continue };
        let mut caps = cursor.captures(&q.query, layer.data.tree.root_node(), &[] as &[u8]);
        while let Some((m, i)) = caps.next() {
            let c = m.captures[*i];
            if c.index != fold {
                continue;
            }
            let (mut s, mut e) = (c.node.start_position(), c.node.end_position());
            s.row = layer.shift.row(s.row);
            e.row = layer.shift.row(e.row);
            if e.column == 0 && e.row > s.row {
                e.row -= 1;
            }
            if e.row > s.row {
                out.push(Fold {
                    start: s.row,
                    end: e.row,
                    comment: c.node.kind().contains("comment"),
                });
            }
        }
    }
    out.sort_by(|a, b| (a.start, b.end).cmp(&(b.start, a.end)));
    out.dedup_by_key(|f| f.start);
    out
}

/// A bracket pair: the byte ranges of the open and close tokens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Brackets {
    pub open: Range<usize>,
    pub close: Range<usize>,
}

/// Bracket pairs with either bracket inside bytes `r`, sorted by open position.
pub(crate) fn brackets(layers: &Layers, r: Range<usize>) -> Vec<Brackets> {
    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    for layer in layers.intersecting(r.clone()) {
        let Some(cfg) = config::get(layer.lang) else {
            continue;
        };
        let Some(q) = &cfg.brackets else { continue };
        let (Some(open), Some(close)) = (q.a, q.b) else {
            continue;
        };
        let sh = layer.shift;
        cursor.set_byte_range(sh.tree(r.start)..sh.tree(r.end));
        let mut ms = cursor.matches(&q.query, layer.data.tree.root_node(), &[] as &[u8]);
        while let Some(m) = ms.next() {
            let o = m.captures.iter().find(|c| c.index == open);
            let c = m.captures.iter().find(|c| c.index == close);
            if let (Some(o), Some(c)) = (o, c) {
                if o.node.is_missing() || c.node.is_missing() {
                    continue;
                }
                let real = |n: Node<'_>| sh.real(n.start_byte())..sh.real(n.end_byte());
                let (o, c) = (real(o.node), real(c.node));
                if (o.start < r.end && o.end > r.start) || (c.start < r.end && c.end > r.start) {
                    out.push(Brackets { open: o, close: c });
                }
            }
        }
    }
    out.sort_by_key(|b| (b.open.start, b.close.start));
    out.dedup();
    out
}

/// The indent level of `line`: `@indent` nodes that start on an earlier line and still contain
/// the line's first non-blank byte, minus `@outdent` tokens starting at that byte; summed over
/// the layers that contain it.
pub(crate) fn indent<S: Source + ?Sized>(layers: &Layers, src: &S, line: usize) -> Option<u32> {
    if line >= src.len_lines() {
        return None;
    }
    let ls = src.line_to_byte(line);
    let le = if line + 1 >= src.len_lines() {
        src.len_bytes()
    } else {
        src.line_to_byte(line + 1)
    };
    let chunks = Chunks::new(src);
    let mut b = ls;
    while b < le {
        match chunks.at(b).first() {
            Some(b' ' | b'\t') => b += 1,
            _ => break,
        }
    }
    let at_eol = b >= le || chunks.at(b).first() == Some(&b'\n');
    let mut level: i64 = 0;
    let mut any = false;
    let mut cursor = QueryCursor::new();
    for layer in layers.intersecting(b..b + 1) {
        if !layer.data.ranges.is_empty() && !layer.ranges().any(|r| r.start <= b && b <= r.end) {
            continue;
        }
        let Some(cfg) = config::get(layer.lang) else {
            continue;
        };
        let Some(q) = &cfg.indents else { continue };
        any = true;
        let sh = layer.shift;
        cursor.set_byte_range(sh.tree(b.saturating_sub(1))..sh.tree(b + 1));
        let mut seen = Vec::new();
        let mut caps = cursor.captures(&q.query, layer.data.tree.root_node(), &[] as &[u8]);
        while let Some((m, i)) = caps.next() {
            let c = m.captures[*i];
            let n = c.node;
            if Some(c.index) == q.a {
                if seen.contains(&n.id()) {
                    continue;
                }
                seen.push(n.id());
                let (ns, ne) = (sh.real(n.start_byte()), sh.real(n.end_byte()));
                if ns < ls && (ne > b || (at_eol && ne >= b && ne > ls)) {
                    level += 1;
                }
            } else if Some(c.index) == q.b
                && sh.real(n.start_byte()) == b
                && !at_eol
                && !n.is_missing()
            {
                level -= 1;
            }
        }
    }
    any.then_some(level.max(0) as u32)
}
