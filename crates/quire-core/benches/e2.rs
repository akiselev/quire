//! E2: rope memory against file size, edit latency (p50/p99) at 100 MB, line fetch.
//!
//! `scripts/cargo.sh bench -p quire-core --bench e2`. Memory is the process RSS delta (from
//! `ps`) around building a `Document`, so it includes the token store and history baseline.
//! Results are recorded in docs/design/E2.md.

use criterion::{BenchmarkId, Criterion};
use quire_core::{ChangeSet, Document, EditorConfig, EditorView, Selection, Transaction};
use std::hint::black_box;
use std::time::{Duration, Instant};

const MB: usize = 1024 * 1024;

/// Source-like text of about `bytes` bytes (lines of 20..100 bytes).
fn corpus(bytes: usize) -> String {
    let words = [
        "let",
        "value",
        "=",
        "compute(",
        "x,",
        "y",
        ");",
        "if",
        "{",
        "}",
        "return",
        "self.field",
        "// note",
    ];
    let mut s = String::with_capacity(bytes + 128);
    let mut seed = 0x2545_f491_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    while s.len() < bytes {
        let indent = (next() % 4) as usize * 4;
        s.extend(std::iter::repeat_n(' ', indent));
        let n = 3 + next() % 12;
        for _ in 0..n {
            s.push_str(words[(next() % words.len() as u64) as usize]);
            s.push(' ');
        }
        s.push('\n');
    }
    s
}

fn rss_bytes() -> Option<usize> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kb: usize = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb * 1024)
}

fn memory() {
    println!("\n== E2 memory: RSS delta for Document::new (contents string dropped) ==");
    for mb in [1usize, 10, 100] {
        let src = corpus(mb * MB);
        let len = src.len();
        let before = rss_bytes().unwrap_or(0);
        let t = Instant::now();
        let doc = Document::new(&src, EditorConfig::default());
        let load = t.elapsed();
        drop(src);
        let after = rss_bytes().unwrap_or(0);
        println!(
            "{mb:>4} MB: {} lines, load {:?}, RSS delta {:.1} MB ({:.2}x the text)",
            doc.text().len_lines(),
            load,
            (after.saturating_sub(before)) as f64 / MB as f64,
            after.saturating_sub(before) as f64 / len as f64,
        );
        drop(doc);
    }
}

fn edit_latency() {
    println!(
        "\n== E2 edit latency at 100 MB: Document::apply, 1-char insert at random positions =="
    );
    let src = corpus(100 * MB);
    let mut doc = Document::new(&src, EditorConfig::default());
    drop(src);
    let mut view = EditorView::new(&doc);
    let mut seed = 0x9e37_79b9_u64;
    let mut times: Vec<Duration> = Vec::with_capacity(10_000);
    for i in 0..10_000u64 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let len = doc.text().len_bytes();
        let pos = doc.text().clamp_to_boundary((seed >> 16) as usize % len);
        let changes = ChangeSet::insert_at(len, pos, "x").expect("insert");
        let tr = Transaction::new(doc.version(), changes)
            .with_selection(Selection::point(pos + 1))
            .with_time(i * 2000); // a pause between edits: every edit its own undo group
        let t = Instant::now();
        doc.apply(tr, &mut view).expect("apply");
        times.push(t.elapsed());
    }
    times.sort();
    let pct = |p: f64| times[((times.len() as f64 * p) as usize).min(times.len() - 1)];
    println!(
        "p50 {:?}, p90 {:?}, p99 {:?}, max {:?}",
        pct(0.5),
        pct(0.9),
        pct(0.99),
        times[times.len() - 1]
    );
}

fn benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("e2_line_fetch");
    for mb in [1usize, 100] {
        let doc = Document::new(&corpus(mb * MB), EditorConfig::default());
        let lines = doc.text().len_lines();
        g.bench_with_input(BenchmarkId::new("line_to_string", mb), &doc, |b, doc| {
            let mut i = 0usize;
            b.iter(|| {
                i = (i + 7919) % lines;
                black_box(doc.text().line(i).to_string())
            })
        });
        g.bench_with_input(
            BenchmarkId::new("visible_50_lines_chunks", mb),
            &doc,
            |b, doc| {
                let mut i = 0usize;
                b.iter(|| {
                    i = (i + 7919) % (lines - 50);
                    let mut n = 0;
                    for l in i..i + 50 {
                        for c in doc.text().line(l).chunks() {
                            n += c.len();
                        }
                    }
                    black_box(n)
                })
            },
        );
    }
    g.finish();
    let mut g = c.benchmark_group("e2_edit");
    let doc0 = Document::new(&corpus(100 * MB), EditorConfig::default());
    g.bench_function("apply_insert_100mb", |b| {
        let mut doc = Document::from_text(doc0.text().clone(), EditorConfig::default());
        let mut view = EditorView::new(&doc);
        let mut pos = 12_345usize;
        let mut t = 0u64;
        b.iter(|| {
            let len = doc.text().len_bytes();
            pos = doc.text().clamp_to_boundary((pos + 104_729) % len);
            t += 2000;
            let tr = Transaction::new(
                doc.version(),
                ChangeSet::insert_at(len, pos, "x").expect("insert"),
            )
            .with_selection(Selection::point(pos + 1))
            .with_time(t);
            doc.apply(tr, &mut view).expect("apply");
        })
    });
    g.finish();
}

fn main() {
    memory();
    edit_latency();
    let mut c = Criterion::default()
        .configure_from_args()
        .sample_size(30)
        .measurement_time(Duration::from_secs(3));
    benches(&mut c);
    c.final_summary();
}
