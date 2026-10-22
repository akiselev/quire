//! TC0 §4.7.4 and TC-EVAL's gates: creating a field allocates only its text; an idle field fits
//! 512 bytes; inserting a character makes at most 8 allocations. One test function, so no other
//! test thread allocates while a region is measured.

use quire_text::{
    DocRules, EditCommand, EditRules, InputKind, LineMode, NoWrap, Text, TextDocument, TextView,
    UndoRules, WordRules,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::sync::Arc;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn insert(c: &str) -> EditCommand {
    EditCommand::Insert {
        text: c.into(),
        kind: InputKind::Typing,
        distribute: false,
    }
}

#[test]
fn field_allocation_budget() {
    let rules = Arc::new(DocRules::field(LineMode::SINGLE, UndoRules::Gtk));

    // §4.7.4: TextDocument::new allocates no history or ring, only the text.
    let r = Region::new(GLOBAL);
    let t = Text::from("short");
    let text_allocs = r.change().allocations;
    drop(t);
    let r = Region::new(GLOBAL);
    let doc = TextDocument::new("short", rules.clone());
    let doc_allocs = r.change().allocations;
    let r = Region::new(GLOBAL);
    let view = TextView::new(&doc);
    let view_allocs = r.change().allocations;
    eprintln!("new(\"short\"): text {text_allocs} allocations, document {doc_allocs}, view {view_allocs}");
    assert_eq!(doc_allocs, text_allocs, "TextDocument::new allocates more than its text");
    assert_eq!(view_allocs, 0, "TextView::new allocates");
    drop((doc, view));

    // TC-EVAL: an idle field (single-line, empty, shared rules) fits 512 bytes.
    let r = Region::new(GLOBAL);
    let doc = TextDocument::new("", rules.clone());
    let view = TextView::new(&doc);
    let heap = r.change();
    let live = heap.bytes_allocated as isize - heap.bytes_deallocated as isize;
    let size = std::mem::size_of::<TextDocument>() + std::mem::size_of::<TextView>();
    let idle = size as isize + live;
    eprintln!(
        "idle field: {idle} B (TextDocument {} B + TextView {} B inline, {live} B heap)",
        std::mem::size_of::<TextDocument>(),
        std::mem::size_of::<TextView>()
    );
    assert!(idle <= 512, "idle field {idle} B > 512 B");
    drop((doc, view));

    // TC-EVAL: one inserted character on a warm field makes at most 8 allocations.
    for undo in [UndoRules::Gtk, UndoRules::Qt, UndoRules::Monaco] {
        let rules = Arc::new(DocRules::field(LineMode::SINGLE, undo));
        let edit = EditRules::field(WordRules::Pango);
        let layout = NoWrap::default();
        let mut doc = TextDocument::new("", rules);
        let mut view = TextView::new(&doc);
        for c in ["a", "b", "c"] {
            doc.execute(&mut view, insert(c), &edit, &layout, None);
        }
        let cmd = insert("d");
        let r = Region::new(GLOBAL);
        let ex = doc.execute(&mut view, cmd, &edit, &layout, None);
        let n = r.change().allocations;
        drop(ex);
        eprintln!("insert one character ({undo:?} undo): {n} allocations");
        assert_eq!(doc.text().to_string(), "abcd");
        assert!(n <= 8, "{undo:?}: {n} allocations > 8");
    }
}
