//! Layered parse: one tree for the document's language plus one per injection (Svelte's script,
//! style and `{expressions}`, CSS in tagged templates), reparsed incrementally.

use std::collections::HashMap;
use std::ops::{ControlFlow, Range};
use std::sync::Arc;

use streaming_iterator::StreamingIterator;
use tree_sitter::{
    InputEdit, Node, ParseOptions, Parser, Point, QueryCursor, Range as TsRange, Tree,
};

use crate::config::{self, InjLang};
use crate::lang::Lang;
use crate::source::{self, Chunks, Source};

/// Deepest injection nesting (svelte > script > css template is 2).
const MAX_DEPTH: u32 = 4;

/// One edit in old-text coordinates.
#[derive(Clone, Debug)]
pub(crate) struct EditRef<'a> {
    pub range: Range<usize>,
    pub insert: &'a str,
}

/// One parse layer.
#[derive(Clone, Debug)]
pub(crate) struct Layer {
    pub lang: Lang,
    pub depth: u32,
    /// The bounding byte range of the included ranges (the whole text for the root); real.
    pub span: Range<usize>,
    /// The parent layer's index (`u32::MAX` for the root).
    pub parent: u32,
    /// Real position = position in `data` (tree and ranges) + `shift`. Layers carried over past
    /// an edit's last line share their data and move lazily instead of paying `Tree::edit`;
    /// the shift is materialised (one synthetic edit) only when the layer is reparsed.
    pub shift: Shift,
    pub data: Arc<LayerData>,
}

/// The shared part of a layer.
#[derive(Debug)]
pub(crate) struct LayerData {
    pub tree: Tree,
    /// Included byte ranges in tree coordinates; empty for the root (the whole text).
    pub ranges: Vec<Range<usize>>,
}

/// A lazy move of a whole tree: bytes and rows (columns never change, see [`Layer::shift`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Shift {
    pub bytes: isize,
    pub rows: isize,
}

impl Shift {
    /// Tree coordinates -> real.
    pub fn real(self, b: usize) -> usize {
        (b as isize + self.bytes) as usize
    }
    /// Real coordinates -> tree (saturating at 0).
    pub fn tree(self, b: usize) -> usize {
        (b as isize - self.bytes).max(0) as usize
    }
    pub fn row(self, r: usize) -> usize {
        (r as isize + self.rows) as usize
    }
}

impl Layer {
    /// The included ranges in real coordinates (empty for the root).
    pub fn ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let sh = self.shift;
        self.data
            .ranges
            .iter()
            .map(move |r| sh.real(r.start)..sh.real(r.end))
    }

    /// The tree in real coordinates (applies the lazy shift as one edit at the start).
    fn materialized(&self) -> Tree {
        let mut t = self.data.tree.clone();
        let Shift { bytes: d, rows: r } = self.shift;
        if d != 0 || r != 0 {
            let (a, b) = ((-d).max(0) as usize, d.max(0) as usize);
            let (ra, rb) = ((-r).max(0) as usize, r.max(0) as usize);
            t.edit(&InputEdit {
                start_byte: 0,
                old_end_byte: a,
                new_end_byte: b,
                start_position: Point { row: 0, column: 0 },
                old_end_position: Point { row: ra, column: 0 },
                new_end_position: Point { row: rb, column: 0 },
            });
        }
        t
    }
}

/// All layers of a document. `layers[0]` is the root; parents come before their children.
#[derive(Clone, Debug)]
pub(crate) struct Layers {
    pub layers: Vec<Layer>,
    /// Indices of the injected layers sorted by span start; `max_end[i]` is the largest span end
    /// among `by_start[..=i]`, so a range lookup is a binary search.
    by_start: Vec<u32>,
    max_end: Vec<usize>,
}

impl Layers {
    fn new(layers: Vec<Layer>) -> Self {
        let mut by_start: Vec<u32> = (1..layers.len() as u32).collect();
        by_start.sort_by_key(|&i| (layers[i as usize].span.start, layers[i as usize].depth));
        let mut max_end = Vec::with_capacity(by_start.len());
        let mut m = 0;
        for &i in &by_start {
            m = m.max(layers[i as usize].span.end);
            max_end.push(m);
        }
        Layers {
            layers,
            by_start,
            max_end,
        }
    }

    /// The root and the layers whose span intersects `r` (touching counts), root first, then
    /// by depth.
    pub fn intersecting(&self, r: Range<usize>) -> Vec<&Layer> {
        let first = self.max_end.partition_point(|&e| e < r.start);
        let last = self
            .by_start
            .partition_point(|&i| self.layers[i as usize].span.start <= r.end);
        let mut out = vec![&self.layers[0]];
        for &i in &self.by_start[first..last.max(first)] {
            let l = &self.layers[i as usize];
            if l.span.end >= r.start {
                out.push(l);
            }
        }
        out.sort_by_key(|l| l.depth);
        out
    }
}

/// tree-sitter edits for `edits` (old coordinates, sorted, non-overlapping), applied in order.
pub(crate) fn input_edits<S: Source + ?Sized>(
    old: &S,
    new: &S,
    edits: &[EditRef<'_>],
) -> Vec<InputEdit> {
    let old_chunks = Chunks::new(old);
    let mut out = Vec::with_capacity(edits.len());
    let mut delta: isize = 0;
    for e in edits {
        let start = (e.range.start as isize + delta) as usize;
        let old_end = start + e.range.len();
        let new_end = start + e.insert.len();
        let start_position = source::point(new, start);
        let mut old_end_position = start_position;
        for piece in old_chunks.slices(e.range.clone()) {
            for &c in piece {
                if c == b'\n' {
                    old_end_position.row += 1;
                    old_end_position.column = 0;
                } else {
                    old_end_position.column += 1;
                }
            }
        }
        out.push(InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position,
            old_end_position,
            new_end_position: source::point(new, new_end),
        });
        delta += e.insert.len() as isize - e.range.len() as isize;
    }
    out
}

/// Maps an old position through the edits. `after`: positions inside a replaced range (or at
/// an insertion) go to the end of the replacement, else to its start.
fn map_pos(edits: &[EditRef<'_>], pos: usize, after: bool) -> usize {
    let mut delta: isize = 0;
    for e in edits {
        if e.range.start > pos || (e.range.start == pos && !after && !e.range.is_empty()) {
            break;
        }
        if e.range.start == pos && e.range.is_empty() && !after {
            break;
        }
        if pos < e.range.end || (pos == e.range.end && e.range.start == pos) {
            let s = (e.range.start as isize + delta) as usize;
            return if after { s + e.insert.len() } else { s };
        }
        delta += e.insert.len() as isize - e.range.len() as isize;
    }
    (pos as isize + delta) as usize
}

/// The previous parse and the edits from its text to the new one.
pub(crate) struct Prev<'p, 'e> {
    pub layers: &'p Layers,
    pub edits: &'e [EditRef<'e>],
    pub input_edits: &'e [InputEdit],
}

/// Statistics of one parse (benches, E5).
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseStats {
    /// Layers in the result.
    pub layers: usize,
    /// Layers parsed (incrementally or from scratch).
    pub parsed: usize,
    /// Layers whose injections were searched again (all of a layer, or its changed ranges).
    pub searched: usize,
    /// Layers carried over from the previous tree without parsing.
    pub reused: usize,
}

struct Ctx<'a, S: ?Sized> {
    parser: Parser,
    src: &'a S,
    chunks: Chunks<'a>,
    cancel: &'a dyn Fn() -> bool,
    stats: ParseStats,
}

impl<S: Source + ?Sized> Ctx<'_, S> {
    fn parse(&mut self, lang: Lang, ranges: &[TsRange], old: Option<&Tree>) -> Option<Tree> {
        let cfg = config::get(lang)?;
        self.parser.set_language(&cfg.grammar).ok()?;
        self.parser.set_included_ranges(ranges).ok()?;
        let chunks = &self.chunks;
        let cancel = self.cancel;
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancel() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let opts = ParseOptions::new().progress_callback(&mut progress);
        let tree =
            self.parser
                .parse_with_options(&mut |b: usize, _: Point| chunks.at(b), old, Some(opts));
        self.parser.reset();
        self.stats.parsed += 1;
        tree
    }
}

/// An injection found in a parent layer, before its tree exists.
struct Found {
    lang: Lang,
    ranges: Vec<TsRange>,
    span: Range<usize>,
}

/// How a new layer relates to the previous parse.
enum Origin {
    /// No previous layer: search the whole tree for injections.
    Fresh,
    /// Reparsed from the previous layer `old`; `before` is the old tree after the edits, for
    /// `changed_ranges`.
    Reparsed { old: usize, before: Tree },
}

/// Parses `src` as `lang`, reusing `prev` when given. `None` if cancelled or the grammar is
/// missing.
pub(crate) fn parse<S: Source + ?Sized>(
    lang: Lang,
    src: &S,
    prev: Option<Prev<'_, '_>>,
    cancel: &dyn Fn() -> bool,
) -> Option<(Layers, ParseStats)> {
    let mut ctx = Ctx {
        parser: Parser::new(),
        src,
        chunks: Chunks::new(src),
        cancel,
        stats: ParseStats::default(),
    };
    let len = src.len_bytes();
    let first_edit = prev
        .as_ref()
        .and_then(|p| p.edits.first())
        .map_or(usize::MAX, |e| e.range.start);
    let edited = |p: &Prev<'_, '_>, old: &Layer| {
        let mut t = old.materialized();
        if old.span.end >= first_edit {
            for e in p.input_edits {
                t.edit(e);
            }
        }
        t
    };
    // Edited ranges in new coordinates.
    let new_edits: Vec<Range<usize>> = prev.as_ref().map_or_else(Vec::new, |p| {
        let mut delta: isize = 0;
        p.edits
            .iter()
            .map(|e| {
                let s = (e.range.start as isize + delta) as usize;
                delta += e.insert.len() as isize - e.range.len() as isize;
                s..s + e.insert.len()
            })
            .collect()
    });
    let eidx = prev
        .as_ref()
        .map(|p| EditIndex::new(p.edits, p.input_edits))
        .unwrap_or(EditIndex {
            old_end: Vec::new(),
            cum: Vec::new(),
        });
    // Old children per old layer.
    let old_children: Vec<Vec<u32>> = prev.as_ref().map_or_else(Vec::new, |p| {
        let mut c = vec![Vec::new(); p.layers.layers.len()];
        for (i, l) in p.layers.layers.iter().enumerate().skip(1) {
            c[l.parent as usize].push(i as u32);
        }
        c
    });

    // Root.
    let (root, origin) = match &prev {
        Some(p) => {
            let before = edited(p, &p.layers.layers[0]);
            let t = ctx.parse(lang, &[], Some(&before))?;
            (t, Origin::Reparsed { old: 0, before })
        }
        None => (ctx.parse(lang, &[], None)?, Origin::Fresh),
    };
    let mut out = vec![Layer {
        lang,
        span: 0..len,
        depth: 0,
        parent: u32::MAX,
        shift: Shift::default(),
        data: Arc::new(LayerData {
            tree: root,
            ranges: Vec::new(),
        }),
    }];
    let mut queue: Vec<(usize, Origin)> = vec![(0, origin)];

    while let Some((idx, origin)) = queue.pop() {
        let (plang, pdepth) = (out[idx].lang, out[idx].depth);
        let pranges: Vec<Range<usize>> = out[idx].ranges().collect();
        if pdepth + 1 > MAX_DEPTH {
            continue;
        }
        let depth = pdepth + 1;
        ctx.stats.searched += 1;
        match origin {
            Origin::Fresh => {
                let found = find_injections(&out[idx].data.tree, plang, &pranges, &ctx, None);
                for f in found {
                    let tree = ctx.parse(f.lang, &f.ranges, None)?;
                    let ranges = f.ranges.iter().map(|r| r.start_byte..r.end_byte).collect();
                    out.push(Layer {
                        lang: f.lang,
                        span: f.span,
                        depth,
                        parent: idx as u32,
                        shift: Shift::default(),
                        data: Arc::new(LayerData { tree, ranges }),
                    });
                    queue.push((out.len() - 1, Origin::Fresh));
                }
            }
            Origin::Reparsed { old, before } => {
                let p = prev.as_ref().expect("reparsed implies prev");
                // Where the injections may have changed: what tree-sitter reports, plus the edits.
                let mut changed: Vec<Range<usize>> = before
                    .changed_ranges(&out[idx].data.tree)
                    .map(|r| r.start_byte..r.end_byte)
                    .collect();
                changed.extend(
                    new_edits
                        .iter()
                        .filter(|r| r.start <= out[idx].span.end && r.end >= out[idx].span.start)
                        .cloned(),
                );
                let changed = merge(changed);

                let found = if changed.is_empty() {
                    Vec::new()
                } else {
                    find_injections(&out[idx].data.tree, plang, &pranges, &ctx, Some(&changed))
                };

                let hits = |r: &Range<usize>| {
                    changed.iter().any(|c| c.start <= r.end && c.end >= r.start)
                        || found
                            .iter()
                            .any(|f| f.span.start <= r.end && f.span.end >= r.start)
                };
                // Old children outside the changed ranges carry over with their subtrees; the
                // rest are candidates for incremental reparse of what was found.
                let mut cands: Vec<(usize, Range<usize>, bool)> = Vec::new();
                for &c in &old_children[old] {
                    let ol = &p.layers.layers[c as usize];
                    let mapped =
                        map_pos(p.edits, ol.span.start, true)..map_pos(p.edits, ol.span.end, false);
                    let touched = p
                        .edits
                        .iter()
                        .any(|e| e.range.start <= ol.span.end && e.range.end >= ol.span.start);
                    if touched || hits(&mapped) {
                        cands.push((c as usize, mapped, false));
                    } else {
                        carry(
                            p,
                            &eidx,
                            ctx.src,
                            c as usize,
                            idx as u32,
                            &old_children,
                            &mut out,
                            &mut ctx.stats,
                        );
                    }
                }
                for f in found {
                    let i = cands.iter().position(|(c, m, used)| {
                        !used
                            && p.layers.layers[*c].lang == f.lang
                            && m.start <= f.span.end
                            && m.end >= f.span.start
                    });
                    // tree-sitter's lexer can crash when the old tree does not cover the new
                    // included ranges (tree-house carries the same guard, helix#12972).
                    let reuse = i
                        .map(|i| (i, edited(p, &p.layers.layers[cands[i].0])))
                        .filter(|(_, t)| {
                            let r = t.root_node().byte_range();
                            r.start <= f.span.start && r.end >= f.span.end
                        });
                    let (tree, origin) = match reuse {
                        Some((i, before)) => {
                            cands[i].2 = true;
                            let t = ctx.parse(f.lang, &f.ranges, Some(&before))?;
                            (
                                t,
                                Origin::Reparsed {
                                    old: cands[i].0,
                                    before,
                                },
                            )
                        }
                        None => (ctx.parse(f.lang, &f.ranges, None)?, Origin::Fresh),
                    };
                    let ranges = f.ranges.iter().map(|r| r.start_byte..r.end_byte).collect();
                    out.push(Layer {
                        lang: f.lang,
                        span: f.span,
                        depth,
                        parent: idx as u32,
                        shift: Shift::default(),
                        data: Arc::new(LayerData { tree, ranges }),
                    });
                    queue.push((out.len() - 1, origin));
                }
            }
        }
    }
    ctx.stats.layers = out.len();
    Some((Layers::new(out), ctx.stats))
}

/// Per edit, for lazy shifts: the old end byte, and after this edit the cumulative byte and
/// row deltas and the new end row.
pub(crate) struct EditIndex {
    old_end: Vec<usize>,
    cum: Vec<(isize, isize, usize)>,
}

impl EditIndex {
    fn new(edits: &[EditRef<'_>], input: &[InputEdit]) -> Self {
        let mut d = 0;
        let mut r = 0;
        let mut cum = Vec::with_capacity(edits.len());
        for (e, ie) in edits.iter().zip(input) {
            d += e.insert.len() as isize - e.range.len() as isize;
            r += ie.new_end_position.row as isize - ie.old_end_position.row as isize;
            cum.push((d, r, ie.new_end_position.row));
        }
        EditIndex {
            old_end: edits.iter().map(|e| e.range.end).collect(),
            cum,
        }
    }

    /// For an untouched span starting at old byte `start`: the byte and row deltas of the
    /// edits before it, and the row the last of them ends on (new coordinates).
    fn before(&self, start: usize) -> Option<(isize, isize, usize)> {
        let n = self.old_end.partition_point(|&e| e <= start);
        (n > 0).then(|| self.cum[n - 1])
    }
}

/// Copies old layer `old` and its subtree (untouched by the edits) under new parent `parent`.
#[allow(clippy::too_many_arguments)]
fn carry<S: Source + ?Sized>(
    p: &Prev<'_, '_>,
    idx: &EditIndex,
    src: &S,
    old: usize,
    parent: u32,
    children: &[Vec<u32>],
    out: &mut Vec<Layer>,
    stats: &mut ParseStats,
) {
    let ol = &p.layers.layers[old];
    let (data, shift, d) = match idx.before(ol.span.start) {
        None => (ol.data.clone(), ol.shift, 0),
        Some((d, r, end_row)) => {
            let new_start = (ol.span.start as isize + d) as usize;
            if end_row < src.byte_to_line(new_start) {
                // Only whole lines moved: share the data, shift lazily.
                let shift = Shift {
                    bytes: ol.shift.bytes + d,
                    rows: ol.shift.rows + r,
                };
                (ol.data.clone(), shift, d)
            } else {
                let mut tree = ol.materialized();
                for e in p.input_edits {
                    tree.edit(e);
                }
                let ranges = ol
                    .ranges()
                    .map(|r| map_pos(p.edits, r.start, false)..map_pos(p.edits, r.end, false))
                    .collect();
                (Arc::new(LayerData { tree, ranges }), Shift::default(), d)
            }
        }
    };
    let m = |b: usize| (b as isize + d) as usize;
    out.push(Layer {
        lang: ol.lang,
        span: m(ol.span.start)..m(ol.span.end),
        depth: ol.depth,
        parent,
        shift,
        data,
    });
    stats.reused += 1;
    let me = (out.len() - 1) as u32;
    for &c in &children[old] {
        carry(p, idx, src, c as usize, me, children, out, stats);
    }
}

/// Sorts and merges ranges, widening each by one byte so touching edits count.
fn merge(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(v.len());
    for r in v {
        let r = r.start.saturating_sub(1)..r.end + 1;
        match out.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Runs the injection query of `lang` over `tree` (only matches touching `within`, if given).
fn find_injections<S: Source + ?Sized>(
    tree: &Tree,
    lang: Lang,
    parent_ranges: &[Range<usize>],
    ctx: &Ctx<'_, S>,
    within: Option<&[Range<usize>]>,
) -> Vec<Found> {
    let chunks = &ctx.chunks;
    let Some(cfg) = config::get(lang) else {
        return Vec::new();
    };
    let Some(inj) = &cfg.injections else {
        return Vec::new();
    };
    struct Hit<'t> {
        pattern: usize,
        node: Node<'t>,
        /// `None`: a language this crate lacks; the node is still claimed, so a later default
        /// pattern does not take it.
        lang: Option<Lang>,
    }
    let mut hits: Vec<Hit<'_>> = Vec::new();
    let mut cursor = QueryCursor::new();
    let whole = std::slice::from_ref(&(0..usize::MAX));
    for r in within.unwrap_or(whole) {
        cursor.set_byte_range(r.clone());
        let text = |n: Node<'_>| chunks.slices(n.byte_range());
        let mut matches = cursor.matches(&inj.query, tree.root_node(), text);
        while let Some(m) = matches.next() {
            let pat = &inj.patterns[m.pattern_index];
            let Some(content) = m
                .captures
                .iter()
                .find(|c| c.index == inj.content)
                .map(|c| c.node)
            else {
                continue;
            };
            let name = match &pat.lang {
                Some(InjLang::Fixed(s)) => s.clone(),
                Some(InjLang::Captured) => {
                    match m.captures.iter().find(|c| Some(c.index) == inj.language) {
                        Some(c) => chunks.string(c.node.byte_range()),
                        None => continue,
                    }
                }
                None => continue,
            };
            let lang = Lang::from_name(&name).filter(|l| l.is_available());
            hits.push(Hit {
                pattern: m.pattern_index,
                node: content,
                lang,
            });
        }
    }
    // First pattern wins per content node.
    hits.sort_by_key(|h| {
        (
            h.node.start_byte(),
            h.node.end_byte(),
            h.node.id(),
            h.pattern,
        )
    });
    hits.dedup_by_key(|h| h.node.id());

    let mut out: Vec<Found> = Vec::new();
    let mut combined: HashMap<(usize, Lang), usize> = HashMap::new();
    for h in hits {
        let Some(lang) = h.lang else { continue };
        let pat = &inj.patterns[h.pattern];
        let mut ranges = content_ranges(h.node, pat.include_children);
        if !parent_ranges.is_empty() {
            ranges = intersect(&ranges, parent_ranges, ctx.src);
        }
        if ranges.is_empty() {
            continue;
        }
        if pat.combined {
            if let Some(&i) = combined.get(&(h.pattern, lang)) {
                let f = &mut out[i];
                f.span.end = f.span.end.max(ranges.last().expect("non-empty").end_byte);
                f.ranges.extend(ranges);
                continue;
            }
            combined.insert((h.pattern, lang), out.len());
        }
        let span = ranges[0].start_byte..ranges.last().expect("non-empty").end_byte;
        out.push(Found { lang, ranges, span });
    }
    for f in &mut out {
        f.ranges.sort_by_key(|r| r.start_byte);
    }
    out
}

/// The node's range, minus its named children unless `include_children`.
fn content_ranges(node: Node<'_>, include_children: bool) -> Vec<TsRange> {
    let whole = node.range();
    if include_children || node.named_child_count() == 0 {
        return vec![whole];
    }
    let mut out = Vec::new();
    let (mut sb, mut sp) = (whole.start_byte, whole.start_point);
    let mut c = node.walk();
    for child in node.named_children(&mut c) {
        if child.start_byte() > sb {
            out.push(TsRange {
                start_byte: sb,
                end_byte: child.start_byte(),
                start_point: sp,
                end_point: child.start_position(),
            });
        }
        sb = child.end_byte();
        sp = child.end_position();
    }
    if whole.end_byte > sb {
        out.push(TsRange {
            start_byte: sb,
            end_byte: whole.end_byte,
            start_point: sp,
            end_point: whole.end_point,
        });
    }
    out
}

/// The parts of `a` inside `b` (both sorted, non-overlapping).
fn intersect<S: Source + ?Sized>(a: &[TsRange], b: &[Range<usize>], src: &S) -> Vec<TsRange> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let s = x.start_byte.max(y.start);
            let e = x.end_byte.min(y.end);
            if s < e {
                let sp = if s == x.start_byte {
                    x.start_point
                } else {
                    source::point(src, s)
                };
                let ep = if e == x.end_byte {
                    x.end_point
                } else {
                    source::point(src, e)
                };
                out.push(TsRange {
                    start_byte: s,
                    end_byte: e,
                    start_point: sp,
                    end_point: ep,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_pos_basics() {
        let e = [EditRef {
            range: 2..4,
            insert: "xyz",
        }];
        assert_eq!(map_pos(&e, 0, false), 0);
        assert_eq!(map_pos(&e, 2, false), 2);
        assert_eq!(map_pos(&e, 3, false), 2);
        assert_eq!(map_pos(&e, 3, true), 5);
        assert_eq!(map_pos(&e, 4, false), 5);
        assert_eq!(map_pos(&e, 10, false), 11);
        let ins = [EditRef {
            range: 2..2,
            insert: "ab",
        }];
        assert_eq!(map_pos(&ins, 2, false), 2);
        assert_eq!(map_pos(&ins, 2, true), 4);
        assert_eq!(map_pos(&ins, 3, false), 5);
    }
}
