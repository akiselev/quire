//! E5: re-highlight after an edit on a ~20k-line Svelte component (target <= 2 ms off-thread).
//!
//! - `reparse`: incremental parse after a one-byte edit (all layers);
//! - `viewport`: highlight 60 lines around the edit with the new tree;
//! - `edit_total`: both, i.e. what a SyntaxDriver job does per keystroke;
//! - `root_floor`: tree-sitter's own incremental reparse of the root tree alone, for reference.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use quire_core::syntax::{CancelFlag, Highlighter};
use quire_core::{ChangeSet, Snapshot, Text, Version};
use quire_syntax::{Lang, TreeSitterHighlighter};

#[path = "../tests/support/fixture_gen.rs"]
mod fixture_gen;

fn bench(c: &mut Criterion) {
    let src = fixture_gen::svelte_component(20_000);
    let h = TreeSitterHighlighter::new(Lang::Svelte).unwrap();
    let cancel = CancelFlag::new();
    let base = Snapshot {
        text: Text::from(src.as_str()),
        version: Version(0),
    };
    let tree = h.parse(&base, None, &cancel).unwrap();
    let mid = src.len() / 2;
    let cases = [
        (
            "markup_expr",
            mid + src[mid..].find("{item.name}").unwrap() + 5,
        ),
        (
            "markup_text",
            mid + src[mid..].find("No items").unwrap() + 2,
        ),
        ("script", src.find("return xs").unwrap() + 7),
    ];
    let mut g = c.benchmark_group("e5_svelte_20k");
    g.sample_size(40);
    for (name, at) in cases {
        let changes = ChangeSet::replace(src.len(), [(at..at, "x")]).unwrap();
        let snap = Snapshot {
            text: changes.apply(&base.text).unwrap(),
            version: Version(1),
        };
        let line = snap.text.byte_to_line(at);
        let view = line.saturating_sub(30)..line + 30;
        g.bench_function(format!("reparse/{name}"), |b| {
            b.iter(|| black_box(h.parse(&snap, Some((&tree, &changes)), &cancel).unwrap()))
        });
        let new = h.parse(&snap, Some((&tree, &changes)), &cancel).unwrap();
        g.bench_function(format!("viewport/{name}"), |b| {
            b.iter(|| black_box(h.highlights(&new, &snap, view.clone())))
        });
        g.bench_function(format!("edit_total/{name}"), |b| {
            b.iter(|| {
                let t = h.parse(&snap, Some((&tree, &changes)), &cancel).unwrap();
                black_box(h.highlights(&t, &snap, view.clone()))
            })
        });
    }
    // tree-sitter alone: the root tree's incremental reparse.
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_htmlx_svelte::LANGUAGE.into())
        .unwrap();
    let root = parser.parse(&src, None).unwrap();
    let at = cases[0].1;
    let mut new_src = src.clone();
    new_src.insert(at, 'x');
    let row = src[..at].matches('\n').count();
    let col = at - src[..at].rfind('\n').map_or(0, |i| i + 1);
    let p = |c| tree_sitter::Point { row, column: c };
    let mut edited = root.clone();
    edited.edit(&tree_sitter::InputEdit {
        start_byte: at,
        old_end_byte: at,
        new_end_byte: at + 1,
        start_position: p(col),
        old_end_position: p(col),
        new_end_position: p(col + 1),
    });
    g.bench_function("root_floor/markup_expr", |b| {
        b.iter(|| black_box(parser.parse(&new_src, Some(&edited)).unwrap()))
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
