//! TC0b: the string backend's line-feed index after an edit. One edit (alternately inserting and
//! deleting one character mid-text, so the size stays put) followed by the first line query of the
//! new version, on texts of 8, 16, 32 and 64 KiB (all on the string backend). Editor rules (line
//! edits tracked, Monaco undo, limited history).
//!
//! `scripts/cargo.sh bench -p quire-text --bench lines`. Run with no other builder on the
//! machine; the report names the load and the thermal state.

use criterion::{BenchmarkId, Criterion};
use quire_text::{ChangeSet, DocRules, TextDocument, TextView, Transaction};
use std::hint::black_box;
use std::sync::Arc;

/// About `kib` KiB of code-like lines (40 bytes each, no line longer than 80).
fn text(kib: usize) -> String {
    let line = "    let value = compute(alpha, beta);  \n";
    let mut s = String::with_capacity(kib * 1024);
    while s.len() + line.len() <= kib * 1024 {
        s.push_str(line);
    }
    s
}

fn benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("lines");
    let rules = Arc::new(DocRules {
        undo_limit: Some(64),
        ..DocRules::default()
    });
    for kib in [8usize, 16, 32, 64] {
        let src = text(kib);
        let mut doc = TextDocument::new(&src, rules.clone());
        let mut view = TextView::new(&doc);
        let mid = src.len() / 2;
        let mut inserted = false;
        // Warm: the index exists for the current version.
        black_box(doc.text().byte_to_line(mid));
        g.bench_function(BenchmarkId::new("edit_then_line_query", kib), |b| {
            b.iter(|| {
                let len = doc.text().len_bytes();
                let cs = if inserted {
                    ChangeSet::delete_range(len, mid..mid + 1).unwrap()
                } else {
                    ChangeSet::insert_at(len, mid, "x").unwrap()
                };
                inserted = !inserted;
                doc.apply(Transaction::new(doc.version(), cs), &mut view)
                    .unwrap();
                black_box(doc.text().byte_to_line(black_box(mid + 1)))
            })
        });
    }
    g.finish();
}

fn main() {
    let mut c = Criterion::default().configure_from_args();
    benches(&mut c);
    c.final_summary();
}
