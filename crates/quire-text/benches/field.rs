//! TC-EVAL T6 timing gates for a text field: creating one (document and view, shared rules)
//! ≤ 1 µs, inserting one character into a warm field ≤ 2 µs.
//!
//! `scripts/cargo.sh bench -p quire-text --bench field`. Run with no other builder on the
//! machine; the report names the load.

use criterion::{BatchSize, Criterion};
use quire_text::{
    DocRules, EditCommand, EditRules, InputKind, LineMode, NoWrap, TextDocument, TextView,
    UndoRules, WordRules,
};
use std::hint::black_box;
use std::sync::Arc;

fn insert(c: &str) -> EditCommand {
    EditCommand::Insert {
        text: c.into(),
        kind: InputKind::Typing,
        distribute: false,
    }
}

fn benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("field");
    let rules = Arc::new(DocRules::field(LineMode::SINGLE, UndoRules::Gtk));
    g.bench_function("create", |b| {
        b.iter(|| {
            let doc = TextDocument::new(black_box(""), rules.clone());
            let view = TextView::new(&doc);
            black_box((doc, view))
        })
    });
    for undo in [UndoRules::Gtk, UndoRules::Qt, UndoRules::Monaco] {
        let rules = Arc::new(DocRules::field(LineMode::SINGLE, undo));
        let edit = EditRules::field(WordRules::Pango);
        let layout = NoWrap::default();
        let warm = || {
            let mut doc = TextDocument::new("", rules.clone());
            let mut view = TextView::new(&doc);
            for ch in ["a", "b", "c"] {
                doc.execute(&mut view, insert(ch), &edit, &layout, None);
            }
            (doc, view, insert("d"))
        };
        g.bench_function(format!("insert_char_{undo:?}").to_lowercase(), |b| {
            b.iter_batched_ref(
                warm,
                |(doc, view, cmd)| {
                    black_box(doc.execute(view, cmd.clone(), &edit, &layout, None));
                },
                BatchSize::SmallInput,
            )
        });
    }
    g.finish();
}

fn main() {
    let mut c = Criterion::default().configure_from_args();
    benches(&mut c);
    c.final_summary();
}
