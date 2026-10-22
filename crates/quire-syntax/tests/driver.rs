//! SyntaxDriver: debounce, jobs on a Spawner, results by version, settled probe.
// One view shows one range of lines: `&[0..10]` is the visible ranges of one view.
#![allow(clippy::single_range_in_vec_init)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use quire_core::syntax::{InlineSpawner, Spawner, TokenType};
use quire_core::{ChangeSet, Document, EditorConfig, EditorView, Transaction};
use quire_syntax::{Lang, SyntaxDriver, TreeSitterHighlighter};

fn driver(spawner: Arc<dyn Spawner>) -> SyntaxDriver<TreeSitterHighlighter> {
    SyntaxDriver::new(
        TreeSitterHighlighter::new(Lang::TypeScript).unwrap(),
        spawner,
    )
}

fn edit(doc: &mut Document, view: &mut EditorView, at: usize, ins: &str) {
    let c = ChangeSet::replace(doc.text().len_bytes(), [(at..at, ins)]).unwrap();
    doc.apply(Transaction::new(doc.version(), c), view).unwrap();
}

fn first_type(doc: &Document, line: usize) -> TokenType {
    doc.tokens()
        .line(line)
        .first()
        .map_or(TokenType::Text, |t| t.style.ty())
}

#[test]
fn inline_spawner_tokenises_on_first_update_and_settles() {
    let mut doc = Document::new("let a = 1;\nconst b = 2;\n", EditorConfig::default());
    let mut d = driver(Arc::new(InlineSpawner));
    d.update(&mut doc, &[0..10], 0);
    assert_eq!(first_type(&doc, 0), TokenType::Keyword);
    assert_eq!(first_type(&doc, 1), TokenType::Keyword);
    assert!(!d.is_pending());
    assert!(doc.is_settled());
    assert!(d.tree_at(doc.version()).is_some());
}

#[test]
fn edits_wait_for_the_debounce_and_hold_the_settled_probe() {
    let mut doc = Document::new("let a = 1;\n", EditorConfig::default());
    let mut view = EditorView::new(&doc);
    let mut d = driver(Arc::new(InlineSpawner)).with_debounce(50);
    d.update(&mut doc, &[0..10], 0);
    let probe = doc.settled_probe();
    edit(&mut doc, &mut view, 0, "const x = 2;\n");
    d.update(&mut doc, &[0..10], 100);
    assert!(d.is_pending(), "debouncing");
    assert!(!probe.is_settled());
    assert_eq!(d.next_deadline(), Some(150));
    d.update(&mut doc, &[0..10], 120);
    assert!(d.is_pending());
    d.update(&mut doc, &[0..10], 150);
    assert!(!d.is_pending());
    assert!(probe.is_settled());
    assert_eq!(first_type(&doc, 0), TokenType::Keyword);
    assert_eq!(d.tree().unwrap().0.stats().layers, 1);
    assert!(d.tree().unwrap().0.stats().parsed >= 1);
}

#[test]
fn scrolling_highlights_more_lines_without_reparsing() {
    let text: String = (0..1000).map(|i| format!("let v{i} = {i};\n")).collect();
    let mut doc = Document::new(&text, EditorConfig::default());
    let mut d = driver(Arc::new(InlineSpawner)).with_margin(10);
    d.update(&mut doc, &[0..20], 0);
    assert_eq!(first_type(&doc, 5), TokenType::Keyword);
    assert_eq!(
        first_type(&doc, 500),
        TokenType::Text,
        "not highlighted yet"
    );
    let tree = d.tree().unwrap().0.clone();
    d.update(&mut doc, &[495..515], 1);
    assert_eq!(first_type(&doc, 500), TokenType::Keyword);
    assert!(
        Arc::ptr_eq(&tree, d.tree().unwrap().0),
        "same tree, no reparse"
    );
}

/// A test-only spawner on real threads (the crates themselves never spawn).
struct ThreadSpawner(Arc<AtomicUsize>);
impl Spawner for ThreadSpawner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(job);
    }
}

#[test]
fn two_views_disjoint_ranges_are_both_highlighted_from_one_parse() {
    let text: String = (0..1000).map(|i| format!("let v{i} = {i};\n")).collect();
    let mut doc = Document::new(&text, EditorConfig::default());
    let mut d = driver(Arc::new(InlineSpawner)).with_margin(10);
    d.update(&mut doc, &[0..20, 600..620], 0);
    assert_eq!(first_type(&doc, 5), TokenType::Keyword);
    assert_eq!(first_type(&doc, 610), TokenType::Keyword);
    assert_eq!(first_type(&doc, 300), TokenType::Text, "between the views");
    let tree = d.tree().unwrap().0.clone();
    // Both covered: another update starts nothing.
    d.update(&mut doc, &[600..620, 0..20], 1);
    assert!(Arc::ptr_eq(&tree, d.tree().unwrap().0));
    assert!(!d.is_pending());

    // With a background spawner the ranges get one job each, the second without a reparse.
    let mut doc = Document::new(&text, EditorConfig::default());
    let jobs = Arc::new(AtomicUsize::new(0));
    let woke = Arc::new(AtomicUsize::new(0));
    let mut d = driver(Arc::new(ThreadSpawner(jobs.clone())))
        .with_margin(10)
        .with_debounce(0);
    let w = woke.clone();
    d.set_waker(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    let views = [0..20, 600..620];
    for n in 1..=2 {
        d.update(&mut doc, &views, n as u64);
        while woke.load(Ordering::SeqCst) < n {
            std::thread::yield_now();
        }
    }
    d.update(&mut doc, &views, 3);
    assert_eq!(jobs.load(Ordering::SeqCst), 2);
    assert!(!d.is_pending());
    assert_eq!(first_type(&doc, 5), TokenType::Keyword);
    assert_eq!(first_type(&doc, 610), TokenType::Keyword);
    assert_eq!(
        d.tree().unwrap().0.stats().parsed,
        1,
        "one parse for both ranges"
    );
}

#[test]
fn thread_spawner_stale_result_applies_and_a_new_job_follows() {
    let mut doc = Document::new("let a = 1;\nlet b = 2;\n", EditorConfig::default());
    let mut view = EditorView::new(&doc);
    let jobs = Arc::new(AtomicUsize::new(0));
    let woke = Arc::new(AtomicUsize::new(0));
    let mut d = driver(Arc::new(ThreadSpawner(jobs.clone()))).with_debounce(0);
    let w = woke.clone();
    d.set_waker(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    d.update(&mut doc, &[0..10], 0);
    assert!(d.is_pending());
    assert!(!doc.is_settled());
    // The document moves on while the job runs: a line appended below the highlighted ones.
    let end = doc.text().len_bytes();
    edit(&mut doc, &mut view, end, "let c = 3;\n");
    while woke.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }
    d.update(&mut doc, &[0..10], 1);
    // The stale result still applied (mapped through the recent changes); a new job started
    // for the new version.
    assert_eq!(first_type(&doc, 1), TokenType::Keyword);
    assert!(d.is_pending());
    while woke.load(Ordering::SeqCst) < 2 {
        std::thread::yield_now();
    }
    d.update(&mut doc, &[0..10], 2);
    assert!(!d.is_pending());
    assert!(doc.is_settled());
    assert_eq!(jobs.load(Ordering::SeqCst), 2);
    assert!(d.tree_at(doc.version()).is_some());
    assert!(d.tree().unwrap().0.stats().reused + d.tree().unwrap().0.stats().parsed >= 1);
}

#[test]
fn fresh_trees_set_the_documents_fold_ranges() {
    // No indentation: the indentation provider finds nothing; the tree does.
    let text = "function f() {\nconst a = 1;\nreturn a;\n}\nlet z = 0;\n";
    let mut doc = Document::new(text, EditorConfig::default());
    assert!(doc.fold_ranges().is_empty());
    let mut d = driver(Arc::new(InlineSpawner));
    d.update(&mut doc, &[0..10], 0);
    let folds: Vec<(usize, usize)> = doc
        .fold_ranges()
        .iter()
        .map(|r| (r.start_line, r.end_line))
        .collect();
    assert!(
        folds.iter().any(|&(s, e)| s == 0 && e >= 2),
        "the function body folds from the syntax tree: {folds:?}"
    );
}
