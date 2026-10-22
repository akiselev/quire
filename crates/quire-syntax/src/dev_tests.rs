//! Engine tests that run before the quire-core seam exists (string sources).

use crate::highlight;
use crate::lang::Lang;
use crate::style;
use crate::tree::{self, EditRef, Layers, Prev};

fn parse(lang: Lang, s: &str) -> Layers {
    tree::parse(lang, s, None, &|| false).expect("parses").0
}

fn render(layers: &Layers, s: &str) -> String {
    let runs = highlight::highlight(layers, s, 0..usize::MAX);
    let mut out = String::new();
    for (i, line) in s.split('\n').enumerate() {
        let r = &runs[i];
        for (k, (start, st)) in r.iter().enumerate() {
            let end = r.get(k + 1).map_or(line.len(), |x| x.0 as usize);
            if *st != style::NONE {
                out.push_str(&format!("{}:{:?} ", style::name(*st), &line[*start as usize..end]));
            }
        }
        out.push('\n');
    }
    out
}

#[test]
fn queries_compile() {
    crate::config::check_queries().unwrap();
}

#[test]
fn svelte_dump() {
    let s0 = "<script lang=\"ts\">let a = 1;</script>\n<div>\n{#if a > 1}\n  <p>x</p>\n{:else if b}\n  <p>y</p>\n{:else}\n  <p>z</p>\n{/if}\n{#each xs as x, i (x.id)}\n  {x}\n{/each}\n<Foo bind:value={v} {w} on:click|once={() => f(1)} />\n</div>\n";
    eprintln!("{}", parse(Lang::Svelte, s0).layers[0].tree.root_node().to_sexp());
    let s = "<script lang=\"ts\">\n  let n: number = 1;\n</script>\n<p class=\"a\">{n + 1}</p>\n{#if n > 0}\n  <b>{@html x}</b>\n{/if}\n<style>\n  p { color: red; }\n</style>\n";
    let l = parse(Lang::Svelte, s);
    for x in &l.layers {
        eprintln!("{:?} {:?} depth {}", x.lang, x.span, x.depth);
    }
    eprintln!("{}", render(&l, s));
}

#[path = "../tests/support/gen.rs"]
mod gen_fixture;

/// Quick E5 timing (run with --release --ignored --nocapture).
#[test]
#[ignore]
fn e5_timing() {
    use std::time::Instant;
    let s = gen_fixture::svelte_component(20_000);
    let t = Instant::now();
    let (l, st) = tree::parse(Lang::Svelte, s.as_str(), None, &|| false).unwrap();
    eprintln!("lines {} bytes {} full parse {:?} {:?}", s.lines().count(), s.len(), t.elapsed(), st);
    let t = Instant::now();
    let _ = highlight::highlight(&l, s.as_str(), 0..usize::MAX);
    eprintln!("full highlight {:?}", t.elapsed());
    // Edit: type a character inside a markup expression in the middle.
    let at = s.find("{item.name}").unwrap() + s.len() / 2;
    let at = at + s[at..].find("{item.name}").unwrap() + 5;
    for (what, at, ins) in [("markup expr", at, "x"), ("script", s.find("return xs").unwrap() + 7, "y"), ("markup text", s.find("No items").unwrap() + s.len() / 3, "z")] {
        let mut new = s.clone();
        new.insert_str(at, ins);
        let edits = [EditRef { range: at..at, insert: ins }];
        let ie = tree::input_edits(s.as_str(), new.as_str(), &edits);
        let t = Instant::now();
        let (l2, st) = tree::parse(Lang::Svelte, new.as_str(), Some(Prev { layers: &l, edits: &edits, input_edits: &ie }), &|| false).unwrap();
        let tp = t.elapsed();
        let line = new[..at].matches('\n').count();
        let t = Instant::now();
        let vp = highlight::highlight(&l2, new.as_str(), line.saturating_sub(30)..line + 30);
        let th = t.elapsed();
        let full = tree::parse(Lang::Svelte, new.as_str(), None, &|| false).unwrap().0;
        assert_eq!(vp, highlight::highlight(&full, new.as_str(), line.saturating_sub(30)..line + 30));
        eprintln!("{what}: reparse {tp:?} {st:?}; viewport highlight (60 lines) {th:?}");
    }
}

#[test]
#[ignore]
fn included_range_cost() {
    use std::time::Instant;
    use tree_sitter::{Parser, Point, Range as R};
    let mut s = "x".repeat(500_000);
    s.push_str(" a + b ");
    s.push_str(&"y".repeat(45_000));
    let mut p = Parser::new();
    p.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let r = R { start_byte: 500_000, end_byte: 500_007, start_point: Point { row: 0, column: 500_000 }, end_point: Point { row: 0, column: 500_007 } };
    p.set_included_ranges(&[r]).unwrap();
    let t = Instant::now();
    for _ in 0..100 {
        let _ = p.parse_with_options(&mut |b: usize, _: Point| &s.as_bytes()[b.min(s.len())..], None, None);
    }
    eprintln!("whole-chunk callback: {:?}/parse", t.elapsed() / 100);
    let t = Instant::now();
    for _ in 0..100 {
        let _ = p.parse_with_options(&mut |b: usize, _: Point| &s.as_bytes()[b.min(s.len())..(b + 64).min(s.len())], None, None);
    }
    eprintln!("64-byte callback: {:?}/parse", t.elapsed() / 100);
}

#[test]
#[ignore]
fn fixture_errors() {
    let s = gen_fixture::svelte_component(400);
    let l = parse(Lang::Svelte, &s);
    for x in &l.layers {
        let root = x.tree.root_node();
        if root.has_error() {
            let mut c = root.walk();
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                if n.is_error() || n.is_missing() {
                    eprintln!("{:?} error at {:?}: {:?} in {:?}", x.lang, n.byte_range(), &s[n.byte_range()].chars().take(80).collect::<String>(), n.parent().map(|p| p.kind()));
                    break;
                }
                stack.extend(n.children(&mut c).collect::<Vec<_>>().into_iter().rev());
            }
        }
    }
}

#[test]
#[ignore]
fn svelte_scaling() {
    use std::time::Instant;
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_htmlx_svelte::LANGUAGE.into()).unwrap();
    let cases: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
        ("text", Box::new(|n| "<p>hello</p>\n".repeat(n))),
        ("expr", Box::new(|n| "<p>{a + b}</p>\n".repeat(n))),
        ("if", Box::new(|n| "{#if a}\n<p>x</p>\n{/if}\n".repeat(n))),
        ("each", Box::new(|n| "{#each xs as x (x.id)}\n<p>x</p>\n{/each}\n".repeat(n))),
        ("attr-expr", Box::new(|n| "<p class:a={b === 1}>x</p>\n".repeat(n))),
        ("on-arrow", Box::new(|n| "<li on:click={() => pick(x)}>y</li>\n".repeat(n))),
        ("entity", Box::new(|n| "<h2 title=\"S 1\">{t} &amp; more</h2>\n".repeat(n))),
        ("comment", Box::new(|n| "<!-- c -->\n".repeat(n))),
        ("else", Box::new(|n| "{#if a}\n<p>x</p>\n{:else}\n<p>y</p>\n{/if}\n".repeat(n))),
    ];
    for (name, f) in &cases {
        let mut line = format!("{name:>10}:");
        for n in [500, 1000, 2000] {
            let s = format!("<main>\n{}</main>\n", f(n));
            let t = Instant::now();
            let tree = p.parse(&s, None).unwrap();
            line.push_str(&format!(" {n}:{:?}{}", t.elapsed(), if tree.root_node().has_error() { "(ERR)" } else { "" }));
        }
        eprintln!("{line}");
    }
}

#[test]
#[ignore]
fn svelte_scaling2() {
    use std::time::Instant;
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_htmlx_svelte::LANGUAGE.into()).unwrap();
    let block = "  <section>\n    {#if a}\n      <ul>\n        <li>{x}</li>\n      </ul>\n    {:else}\n      <p>No items</p>\n    {/if}\n  </section>\n";
    let flat = "  <section>\n    {#if a}\n      <p>x</p>\n    {:else}\n      <p>No items</p>\n    {/if}\n  </section>\n";
    let noelse = "  <section>\n    {#if a}\n      <ul>\n        <li>{x}</li>\n      </ul>\n    {/if}\n  </section>\n";
    for (name, b) in [("block", block), ("flat", flat), ("noelse", noelse)] {
        let mut line = format!("{name:>8}:");
        for n in [100, 200, 400, 800] {
            let s = format!("<main>\n{}</main>\n", b.repeat(n));
            let t = Instant::now();
            let tree = p.parse(&s, None).unwrap();
            line.push_str(&format!(" {n}:{:?}{}", t.elapsed(), if tree.root_node().has_error() { "(ERR)" } else { "" }));
        }
        eprintln!("{line}");
    }
}
