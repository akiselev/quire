//! E3 (ARCH-QUIRE §3.4, §3.9): large-file thresholds. Generated files around Monaco's thresholds
//! (20 MB, 300k lines, a 10k-character line), each opened, scrolled, typed into and searched
//! through the real widget in a 1280×800 window, with master's `LargeFilePolicy` as is and with
//! the policy switched off (to see what the degraded features cost).
//!
//! Prints a markdown table; results and recommended thresholds: `docs/design/E3.md`.
//! `E3_ONLY=<substring>` runs the matching cases only.

use std::time::{Duration, Instant};

use egui::Event;
use quire_core::{EditorCommand, EditorConfig, FindQuery, LargeFilePolicy, Selection};
use stats_alloc::Region;

#[path = "support/gen.rs"]
mod gen_text;
#[path = "support/rig.rs"]
mod rig;

use rig::{Rig, ms, percentiles, wheel_lines};

#[global_allocator]
static GLOBAL: &rig::Counting = rig::COUNTING;

const FRAMES: usize = 120;

struct Case {
    name: String,
    text: String,
    /// Line to put the caret on for typing (the long line, or the middle).
    caret_line: usize,
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for lines in [100_000, 300_000, 600_000] {
        v.push(Case {
            name: format!("{}k lines × 39 B", lines / 1000),
            text: gen_text::code(lines),
            caret_line: lines / 2,
        });
    }
    // Same byte sizes with fewer, longer lines: 20 MB and 40 MB at ~200 B per line.
    for mb in [20usize, 40] {
        let lines = mb * 1024 * 1024 / 200;
        let mut s = String::with_capacity(mb << 20);
        let base = gen_text::code(lines);
        for l in base.lines() {
            s.push_str(l);
            let mut len = l.len();
            while len < 192 {
                s.push_str(" // pad");
                len += 7;
            }
            s.push('\n');
        }
        let n = s.matches('\n').count();
        v.push(Case {
            name: format!("{} MB, {}k lines", s.len() >> 20, n / 1000),
            text: s,
            caret_line: n / 2,
        });
    }
    for len in [10_000, 100_000, 1_000_000] {
        v.push(Case {
            name: format!("one {}k-char line in 1k lines", len / 1000),
            text: gen_text::with_long_line(1000, len),
            caret_line: 500,
        });
    }
    v
}

struct Row {
    name: String,
    policy: &'static str,
    degraded: String,
    doc_new: Duration,
    first_frame: Duration,
    heap_mb: f64,
    scroll: (Duration, Duration),
    typing: (Duration, Duration),
    find: Duration,
    find_matches: usize,
    typing_find: (Duration, Duration),
}

/// `FRAMES` samples, or fewer if they take more than 10 s (the very long lines).
fn timed(mut f: impl FnMut() -> Duration) -> Vec<Duration> {
    let t = Instant::now();
    let mut v = Vec::new();
    while v.len() < FRAMES && (v.len() < 5 || t.elapsed() < Duration::from_secs(10)) {
        v.push(f());
    }
    v
}

fn mean_p99(mut xs: Vec<Duration>) -> (Duration, Duration) {
    let mean = xs.iter().sum::<Duration>() / xs.len() as u32;
    let (_, p99, _) = percentiles(&mut xs);
    (mean, p99)
}

fn run(case: &Case, policy: &'static str, ctx: &egui::Context) -> Row {
    let large_file = match policy {
        "default" => LargeFilePolicy::default(),
        _ => LargeFilePolicy {
            max_bytes: usize::MAX,
            max_lines: usize::MAX,
            max_line_len: usize::MAX,
        },
    };
    let config = EditorConfig {
        large_file,
        ..EditorConfig::default()
    };
    ctx.memory_mut(|m| m.data.clear());
    let heap = Region::new(rig::COUNTING);
    let t = Instant::now();
    let mut r = Rig::with_config(ctx.clone(), &case.text, config);
    let doc_new = t.elapsed();
    r.frame(vec![]);
    let first_frame = t.elapsed();
    let st = heap.change();
    let heap_mb = (st.bytes_allocated as f64 - st.bytes_deallocated as f64) / (1 << 20) as f64;
    let degraded = match r.doc.degraded() {
        None => "no".to_owned(),
        Some(d) => d
            .reasons
            .iter()
            .map(|r| match r {
                quire_core::DegradedReason::FileSize { .. } => "size",
                quire_core::DegradedReason::LineCount { .. } => "lines",
                quire_core::DegradedReason::LongLine { .. } => "long line",
            })
            .collect::<Vec<_>>()
            .join("+"),
    };
    r.focus();

    // Scroll from the middle.
    let lines = r.doc.text().len_lines();
    r.scroll_to_line(lines / 2);
    for _ in 0..10 {
        r.frame(vec![wheel_lines(3.0)]);
    }
    let scroll = mean_p99(timed(|| r.frame(vec![wheel_lines(3.0)]).editor));

    // Type at the caret line (the long line's middle, or the file's middle line).
    let line = case.caret_line.min(lines - 1);
    let range = r.doc.text().line_range(line);
    let mut at = range.start + (range.end - range.start) / 2;
    while !r.doc.text().is_char_boundary(at) {
        at -= 1;
    }
    r.caret_to(at);
    const TYPED: &str = "let x = 1; ";
    let mut i = 0;
    let typing = mean_p99(timed(|| {
        i += 1;
        let c = &TYPED[i % TYPED.len()..][..1];
        r.frame(vec![Event::Text(c.to_owned())]).editor
    }));

    // Find: a query with many matches, from the document start (core, no frame).
    r.doc
        .execute(
            &mut r.view,
            &EditorCommand::SetSelection {
                selection: Selection::point(0),
            },
            None,
        )
        .unwrap();
    let t = Instant::now();
    r.doc
        .execute(
            &mut r.view,
            &EditorCommand::Find {
                query: FindQuery {
                    search_string: "value_1".into(),
                    ..FindQuery::default()
                },
            },
            None,
        )
        .unwrap();
    let find = t.elapsed();
    let find_matches = r.view.find().map_or(0, |f| f.matches.len());

    // Type with the find query active: the widget refreshes the matches every edit.
    r.caret_to(at);
    let mut i = 0;
    let typing_find = mean_p99(timed(|| {
        i += 1;
        let c = &TYPED[i % TYPED.len()..][..1];
        r.frame(vec![Event::Text(c.to_owned())]).editor
    }));

    Row {
        name: case.name.clone(),
        policy,
        degraded,
        doc_new,
        first_frame,
        heap_mb,
        scroll,
        typing,
        find,
        find_matches,
        typing_find,
    }
}

fn main() {
    let only = std::env::var("E3_ONLY").ok();
    let ctx = Rig::warm_context();
    println!(
        "| file | policy | degraded | Document::new | open to first frame | heap after open | scroll mean / p99 | type mean / p99 | find (matches) | type with find active mean / p99 |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for case in cases() {
        if only
            .as_ref()
            .is_some_and(|o| !case.name.contains(o.as_str()))
        {
            continue;
        }
        for policy in ["default", "off"] {
            let r = run(&case, policy, &ctx);
            println!(
                "| {} | {} | {} | {} | {} | {:.0} MB | {} / {} | {} / {} | {} ({}) | {} / {} |",
                r.name,
                r.policy,
                r.degraded,
                ms(r.doc_new),
                ms(r.first_frame),
                r.heap_mb,
                ms(r.scroll.0),
                ms(r.scroll.1),
                ms(r.typing.0),
                ms(r.typing.1),
                ms(r.find),
                r.find_matches,
                ms(r.typing_find.0),
                ms(r.typing_find.1),
            );
        }
    }
}
