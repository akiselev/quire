//! E1 (headless part, ARCH-QUIRE §3.9): a 100k-line file in a 1280×800 window.
//!
//! Targets: editor CPU ≤ 2 ms per frame while scrolling and typing; open to first frame
//! ≤ 300 ms; no allocation per frame for clean lines.
//!
//! - criterion groups (`e1_100k/...`): mean time of one frame (`CodeEditor::show_in` inside a
//!   full `Context::run_ui`) for wheel scrolling by 3 lines, by a page, typing, and an idle frame;
//! - then a report on stdout: open to first frame (cold and warm context), per-frame p50 / p99 /
//!   max of the editor alone and of the whole pass, and allocations per frame (counted by a
//!   global allocator that wraps `System`).
//!
//! `E1_REPORT_ONLY=1` skips criterion; `E1_OPEN_ONLY=1` also skips the frame runs. Results: `docs/design/E1.md`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::Criterion;
use egui::{Event, Key, Modifiers};

#[path = "support/gen.rs"]
mod gen_text;
#[path = "support/rig.rs"]
mod rig;

use rig::{Frame, Rig, ms, percentiles, wheel_lines};

#[global_allocator]
static GLOBAL: &rig::Counting = rig::COUNTING;

const LINES: usize = 100_000;

fn enter() -> Event {
    Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

/// The n-th keystroke of the typing run: a short statement, then Enter.
fn keystroke(n: usize) -> Event {
    const LINE: &str = "let x = a + 1;";
    let i = n % (LINE.len() + 1);
    match LINE.get(i..=i) {
        Some(c) => Event::Text(c.to_owned()),
        None => enter(),
    }
}

/// A rig on the 100k-line file, focused, pointer over the text, caret mid-file.
fn rig(text: &str) -> Rig {
    let mut r = Rig::new(Rig::warm_context(), text);
    r.frame(vec![]);
    r.focus();
    let mid = r.doc.text().line_to_byte(LINES / 2);
    r.caret_to(mid);
    r
}

/// One wheel frame; wraps to the top before the end of the file.
fn scroll_frame(r: &mut Rig, lines: f32) -> Frame {
    if r.doc.text().byte_to_line(r.view.scroll().anchor) + 200 > r.doc.text().len_lines() {
        r.scroll_to_line(0);
    }
    r.frame(vec![wheel_lines(lines)])
}

fn bench(c: &mut Criterion, text: &str) {
    let mut g = c.benchmark_group("e1_100k");
    g.sample_size(30);
    let mut r = rig(text);
    g.bench_function("scroll_3_lines", |b| {
        b.iter(|| black_box(scroll_frame(&mut r, 3.0)))
    });
    g.bench_function("scroll_page", |b| {
        b.iter(|| black_box(scroll_frame(&mut r, 40.0)))
    });
    let mut r = rig(text);
    let mut n = 0;
    g.bench_function("type", |b| {
        b.iter(|| {
            n += 1;
            black_box(r.frame(vec![keystroke(n)]))
        })
    });
    g.bench_function("idle", |b| b.iter(|| black_box(r.frame(vec![]))));
    g.finish();
}

struct Run {
    name: &'static str,
    frames: Vec<Frame>,
}

impl Run {
    fn print(&self) {
        let mut ed: Vec<Duration> = self.frames.iter().map(|f| f.editor).collect();
        let mut pass: Vec<Duration> = self.frames.iter().map(|f| f.pass).collect();
        let mean = ed.iter().sum::<Duration>() / ed.len() as u32;
        let (e50, e99, emax) = percentiles(&mut ed);
        let (p50, p99, pmax) = percentiles(&mut pass);
        let mut allocs: Vec<usize> = self.frames.iter().map(|f| f.editor_allocs).collect();
        allocs.sort();
        let mut bytes: Vec<usize> = self.frames.iter().map(|f| f.editor_bytes).collect();
        bytes.sort();
        let mut pass_allocs: Vec<usize> = self.frames.iter().map(|f| f.pass_allocs).collect();
        pass_allocs.sort();
        let vis = self.frames[0].visible;
        println!(
            "| {} | {} | {} | {} | {} | {} / {} / {} | {} ({} B) / max {} | {} | {} |",
            self.name,
            ms(mean),
            ms(e50),
            ms(e99),
            ms(emax),
            ms(p50),
            ms(p99),
            ms(pmax),
            allocs[allocs.len() / 2],
            bytes[bytes.len() / 2],
            allocs.last().unwrap(),
            pass_allocs[pass_allocs.len() / 2],
            vis,
        );
    }
}

fn report(text: &str) {
    println!(
        "\nE1 report: {LINES} lines, {} bytes, window {}x{}",
        text.len(),
        rig::SCREEN_W,
        rig::SCREEN_H
    );

    // Open to first frame. Cold: a new egui context (font atlas built in the same frame).
    // Warm: the host already runs egui.
    for _ in 0..2 {
        let t = Instant::now();
        let mut r = Rig::new(egui::Context::default(), text);
        let doc_t = t.elapsed();
        r.frame(vec![]);
        println!(
            "open cold: Document::new {} ; to first frame {}",
            ms(doc_t),
            ms(t.elapsed())
        );
    }
    let ctx = Rig::warm_context();
    let mut opens = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let mut r = Rig::new(ctx.clone(), text);
        let doc_t = t.elapsed();
        r.salt = "fresh";
        let f = r.frame(vec![]);
        let total = t.elapsed();
        opens.push((doc_t, f.editor, total));
        // Forget the editor's memory so the next open starts clean.
        ctx.memory_mut(|m| m.data.clear());
    }
    for (d, e, t) in &opens {
        println!(
            "open warm: Document::new {} + first show_in {} = {}",
            ms(*d),
            ms(*e),
            ms(*t)
        );
    }

    if std::env::var_os("E1_OPEN_ONLY").is_some() {
        return;
    }
    println!(
        "\n| run (600 frames) | editor mean | editor p50 | editor p99 | editor max | pass p50 / p99 / max | editor allocs p50 (bytes) / max | pass allocs p50 | lines |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let n = 600;
    let mut r = rig(text);
    let run = |name, r: &mut Rig, f: &mut dyn FnMut(&mut Rig, usize) -> Frame| {
        // A few frames first so each run starts in its steady state.
        for i in 0..10 {
            f(r, i);
        }
        Run {
            name,
            frames: (0..n).map(|i| f(r, i)).collect(),
        }
        .print()
    };
    run("idle (clean lines)", &mut r, &mut |r, _| r.frame(vec![]));
    run("scroll 3 lines", &mut r, &mut |r, _| scroll_frame(r, 3.0));
    // Scrolling back over lines already laid out: the galley cache hits.
    r.scroll_to_line(LINES / 2);
    run(
        "scroll 3 lines, back and forth over 60 lines",
        &mut r,
        &mut |r, i| {
            r.frame(vec![wheel_lines(if (i / 20) % 2 == 0 {
                3.0
            } else {
                -3.0
            })])
        },
    );
    run("scroll page (40 lines)", &mut r, &mut |r, _| {
        scroll_frame(r, 40.0)
    });
    let mut r = rig(text);
    run("type (14 chars + Enter)", &mut r, &mut |r, i| {
        r.frame(vec![keystroke(i)])
    });
    run("idle after typing", &mut r, &mut |r, _| r.frame(vec![]));
    // Typing with a plain find query active (the widget refreshes the matches every edit).
    let caret = r.view.selection().primary();
    r.doc
        .execute(
            &mut r.view,
            &quire_core::EditorCommand::Find {
                query: quire_core::FindQuery {
                    search_string: "value_1".into(),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    r.caret_to(caret.head);
    run("type with find active", &mut r, &mut |r, i| {
        r.frame(vec![keystroke(i)])
    });
}

fn main() {
    let text = gen_text::code(LINES);
    if std::env::var_os("E1_REPORT_ONLY").is_none() {
        let mut c = Criterion::default().configure_from_args();
        bench(&mut c, &text);
        c.final_summary();
    }
    report(&text);
}
