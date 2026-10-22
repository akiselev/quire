//! Layered parse: one tree for the document's language plus one per injection (Svelte's script,
//! style and `{expressions}`, CSS in tagged templates), reparsed incrementally.

use std::collections::HashMap;
use std::ops::{ControlFlow, Range};

use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Node, ParseOptions, Parser, Point, QueryCursor, Range as TsRange, Tree};

use crate::config::{self, InjLang};
use crate::lang::Lang;
use crate::source::{self, Chunks, Source};

/// Deepest injection nesting (svelte > script > css template is 2).
const MAX_DEPTH: u32 = 4;

/// The marker the Svelte injection query uses for `{expressions}`.
const SVELTE_SCRIPT: &str = "svelte.script";

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
    pub tree: Tree,
    /// Included ranges; empty for the root (the whole text).
    pub ranges: Vec<TsRange>,
    /// The bounding byte range of `ranges` (the whole text for the root).
    pub span: Range<usize>,
    pub depth: u32,
}

/// All layers of a document. `layers[0]` is the root; the rest are sorted by span start.
#[derive(Clone, Debug)]
pub(crate) struct Layers {
    pub layers: Vec<Layer>,
}

impl Layers {
    /// Layers whose span intersects `r` (the root always does), root first, then by depth.
    pub fn intersecting(&self, r: Range<usize>) -> impl Iterator<Item = &Layer> + '_ {
        self.layers.iter().enumerate().filter_map(move |(i, l)| {
            (i == 0 || (l.span.start < r.end.max(r.start + 1) && l.span.end >= r.start)).then_some(l)
        })
    }
}

/// tree-sitter edits for `edits` (old coordinates, sorted, non-overlapping), applied in order.
pub(crate) fn input_edits<S: Source + ?Sized>(old: &S, new: &S, edits: &[EditRef<'_>]) -> Vec<InputEdit> {
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

/// The previous parse, the edits from its text to the new one, and the old text.
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
    /// Layers reused from the previous tree without parsing.
    pub reused: usize,
}

struct Ctx<'a> {
    parser: Parser,
    chunks: Chunks<'a>,
    cancel: &'a dyn Fn() -> bool,
    stats: ParseStats,
}

impl Ctx<'_> {
    fn parse(&mut self, lang: Lang, ranges: &[TsRange], old: Option<&Tree>) -> Option<Tree> {
        let cfg = config::get(lang)?;
        self.parser.set_language(&cfg.grammar).ok()?;
        if ranges.is_empty() {
            self.parser.set_included_ranges(&[]).ok()?;
        } else {
            self.parser.set_included_ranges(ranges).ok()?;
        }
        let chunks = &self.chunks;
        let cancel = self.cancel;
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancel() { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
        };
        let opts = ParseOptions::new().progress_callback(&mut progress);
        let tree = self.parser.parse_with_options(&mut |b: usize, _: Point| chunks.at(b), old, Some(opts));
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

/// Parses `src` as `lang`, reusing `prev` when given. `None` if cancelled or the grammar is
/// missing.
pub(crate) fn parse<S: Source + ?Sized>(
    lang: Lang,
    src: &S,
    prev: Option<Prev<'_, '_>>,
    cancel: &dyn Fn() -> bool,
) -> Option<(Layers, ParseStats)> {
    let mut ctx = Ctx { parser: Parser::new(), chunks: Chunks::new(src), cancel, stats: ParseStats::default() };
    let len = src.len_bytes();

    // Root.
    let old_root = prev.as_ref().map(|p| {
        let mut t = p.layers.layers[0].tree.clone();
        for e in p.input_edits {
            t.edit(e);
        }
        t
    });
    let root = ctx.parse(lang, &[], old_root.as_ref())?;
    let mut out = vec![Layer { lang, tree: root, ranges: Vec::new(), span: 0..len, depth: 0 }];

    // Old injected layers by (lang, depth), sorted by mapped span start.
    struct Cand {
        idx: usize,
        mapped: Range<usize>,
        touched: bool,
        used: bool,
    }
    let mut cands: HashMap<(Lang, u32), Vec<Cand>> = HashMap::new();
    if let Some(p) = &prev {
        for (idx, l) in p.layers.layers.iter().enumerate().skip(1) {
            let mapped = map_pos(p.edits, l.span.start, true)..map_pos(p.edits, l.span.end, false);
            let touched =
                p.edits.iter().any(|e| e.range.start <= l.span.end && e.range.end >= l.span.start);
            cands.entry((l.lang, l.depth)).or_default().push(Cand { idx, mapped, touched, used: false });
        }
        for v in cands.values_mut() {
            v.sort_by_key(|c| c.mapped.start);
        }
    }

    // Breadth-first over layers: find each layer's injections, then build their trees.
    let mut next = 0;
    while next < out.len() {
        let (parent_lang, parent_depth, parent_ranges) = {
            let l = &out[next];
            (l.lang, l.depth, l.ranges.clone())
        };
        next += 1;
        if parent_depth + 1 > MAX_DEPTH {
            continue;
        }
        let found = find_injections(&out[next - 1].tree, parent_lang, &parent_ranges, &ctx.chunks);
        for f in found {
            let depth = parent_depth + 1;
            let mut tree = None;
            if let (Some(p), Some(list)) = (&prev, cands.get_mut(&(f.lang, depth))) {
                let i = list.partition_point(|c| c.mapped.end < f.span.start);
                if let Some(c) = list[i..]
                    .iter_mut()
                    .take_while(|c| c.mapped.start <= f.span.end)
                    .find(|c| !c.used)
                {
                    c.used = true;
                    let old = &p.layers.layers[c.idx];
                    let mut t = old.tree.clone();
                    if old.span.end >= p.edits.first().map_or(usize::MAX, |e| e.range.start) {
                        for e in p.input_edits {
                            t.edit(e);
                        }
                    }
                    let same = !c.touched
                        && old.ranges.len() == f.ranges.len()
                        && old.ranges.iter().zip(&f.ranges).all(|(a, b)| {
                            map_pos(p.edits, a.start_byte, true) == b.start_byte
                                && map_pos(p.edits, a.end_byte, false) == b.end_byte
                        });
                    if same {
                        ctx.stats.reused += 1;
                        tree = Some(t);
                    } else {
                        tree = Some(ctx.parse(f.lang, &f.ranges, Some(&t))?);
                    }
                }
            }
            let tree = match tree {
                Some(t) => t,
                None => ctx.parse(f.lang, &f.ranges, None)?,
            };
            out.push(Layer { lang: f.lang, tree, ranges: f.ranges, span: f.span, depth });
        }
    }
    out[1..].sort_by_key(|l| (l.span.start, l.depth));
    ctx.stats.layers = out.len();
    Some((Layers { layers: out }, ctx.stats))
}

/// Runs the injection query of `lang` over `tree`.
fn find_injections(tree: &Tree, lang: Lang, parent_ranges: &[TsRange], chunks: &Chunks<'_>) -> Vec<Found> {
    let Some(cfg) = config::get(lang) else { return Vec::new() };
    let Some(inj) = &cfg.injections else { return Vec::new() };
    struct Hit<'t> {
        pattern: usize,
        node: Node<'t>,
        /// `None`: a language this crate lacks; the node is still claimed, so a later default
        /// pattern does not take it.
        lang: Option<Lang>,
    }
    let mut hits: Vec<Hit<'_>> = Vec::new();
    let mut unresolved: Vec<(usize, Node<'_>)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let text = |n: Node<'_>| chunks.slices(n.byte_range());
    let mut matches = cursor.matches(&inj.query, tree.root_node(), text);
    while let Some(m) = matches.next() {
        let pat = &inj.patterns[m.pattern_index];
        let Some(content) = m.captures().iter().find(|c| c.index == inj.content).map(|c| c.node) else {
            continue;
        };
        let name = match &pat.lang {
            Some(InjLang::Fixed(s)) => s.clone(),
            Some(InjLang::Captured) => match m.captures().iter().find(|c| Some(c.index) == inj.language) {
                Some(c) => chunks.string(c.node.byte_range()),
                None => continue,
            },
            None => continue,
        };
        if name == SVELTE_SCRIPT {
            unresolved.push((m.pattern_index, content));
            continue;
        }
        let lang = Lang::from_name(&name).filter(|l| l.is_available());
        hits.push(Hit { pattern: m.pattern_index, node: content, lang });
    }
    // Svelte expressions take the component script's language.
    if !unresolved.is_empty() {
        let script = if hits.iter().any(|h| h.lang == Some(Lang::TypeScript)) {
            Lang::TypeScript
        } else {
            Lang::JavaScript
        };
        let lang = script.is_available().then_some(script);
        hits.extend(unresolved.into_iter().map(|(pattern, node)| Hit { pattern, node, lang }));
    }
    // First pattern wins per content node.
    hits.sort_by_key(|h| (h.node.start_byte(), h.node.end_byte(), h.node.id(), h.pattern));
    hits.dedup_by_key(|h| h.node.id());

    let mut out: Vec<Found> = Vec::new();
    let mut combined: HashMap<(usize, Lang), usize> = HashMap::new();
    for h in hits {
        let Some(lang) = h.lang else { continue };
        let pat = &inj.patterns[h.pattern];
        let mut ranges = content_ranges(h.node, pat.include_children);
        if !parent_ranges.is_empty() {
            ranges = intersect(&ranges, parent_ranges);
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
            out.push(TsRange { start_byte: sb, end_byte: child.start_byte(), start_point: sp, end_point: child.start_position() });
        }
        sb = child.end_byte();
        sp = child.end_position();
    }
    if whole.end_byte > sb {
        out.push(TsRange { start_byte: sb, end_byte: whole.end_byte, start_point: sp, end_point: whole.end_point });
    }
    out
}

/// The parts of `a` inside `b` (both sorted, non-overlapping).
fn intersect(a: &[TsRange], b: &[TsRange]) -> Vec<TsRange> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let s = x.start_byte.max(y.start_byte);
            let e = x.end_byte.min(y.end_byte);
            if s < e {
                let sp = if s == x.start_byte { x.start_point } else { y.start_point };
                let ep = if e == x.end_byte { x.end_point } else { y.end_point };
                out.push(TsRange { start_byte: s, end_byte: e, start_point: sp, end_point: ep });
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
        let e = [EditRef { range: 2..4, insert: "xyz" }];
        assert_eq!(map_pos(&e, 0, false), 0);
        assert_eq!(map_pos(&e, 2, false), 2);
        assert_eq!(map_pos(&e, 3, false), 2);
        assert_eq!(map_pos(&e, 3, true), 5);
        assert_eq!(map_pos(&e, 4, false), 5);
        assert_eq!(map_pos(&e, 10, false), 11);
        let ins = [EditRef { range: 2..2, insert: "ab" }];
        assert_eq!(map_pos(&ins, 2, false), 2);
        assert_eq!(map_pos(&ins, 2, true), 4);
        assert_eq!(map_pos(&ins, 3, false), 5);
    }
}
