//! Engine tests that run before the quire-core seam exists (string sources).

use crate::highlight;
use crate::lang::Lang;
use crate::source::StrText;
use crate::style;
use crate::tree::{self, EditRef, Layers, Prev};

fn parse(lang: Lang, s: &str) -> Layers {
    tree::parse(lang, &StrText::new(s), None, &|| false)
        .expect("parses")
        .0
}

fn render(layers: &Layers, s: &str) -> String {
    let runs = highlight::highlight(layers, &StrText::new(s), 0..usize::MAX);
    let mut out = String::new();
    for (i, line) in s.split('\n').enumerate() {
        let r = &runs[i];
        for (k, (start, st)) in r.iter().enumerate() {
            let end = r.get(k + 1).map_or(line.len(), |x| x.0 as usize);
            if *st != style::NONE {
                out.push_str(&format!(
                    "{}:{:?} ",
                    style::name(*st),
                    &line[*start as usize..end]
                ));
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
    eprintln!(
        "{}",
        parse(Lang::Svelte, s0).layers[0]
            .data
            .tree
            .root_node()
            .to_sexp()
    );
    let s = "<script lang=\"ts\">\n  let n: number = 1;\n</script>\n<p class=\"a\">{n + 1}</p>\n{#if n > 0}\n  <b>{@html x}</b>\n{/if}\n<style>\n  p { color: red; }\n</style>\n";
    let l = parse(Lang::Svelte, s);
    for x in &l.layers {
        eprintln!("{:?} {:?} depth {}", x.lang, x.span, x.depth);
    }
    eprintln!("{}", render(&l, s));
}

#[path = "../tests/support/fixture_gen.rs"]
mod gen_fixture;

/// Quick E5 timing (run with --release --ignored --nocapture).
#[test]
#[ignore]
fn e5_timing() {
    use std::time::Instant;
    let s = gen_fixture::svelte_component(20_000);
    let src = StrText::new(s.clone());
    let t = Instant::now();
    let (l, st) = tree::parse(Lang::Svelte, &src, None, &|| false).unwrap();
    eprintln!(
        "lines {} bytes {} full parse {:?} {:?}",
        s.lines().count(),
        s.len(),
        t.elapsed(),
        st
    );
    let t = Instant::now();
    let _ = highlight::highlight(&l, &src, 0..usize::MAX);
    eprintln!("full highlight {:?}", t.elapsed());
    // Edit: type a character inside a markup expression in the middle.
    let at = s.find("{item.name}").unwrap() + s.len() / 2;
    let at = at + s[at..].find("{item.name}").unwrap() + 5;
    for (what, at, ins) in [
        ("markup expr", at, "x"),
        ("script", s.find("return xs").unwrap() + 7, "y"),
        (
            "markup text",
            s.find("No items").unwrap() + s.len() / 3,
            "z",
        ),
    ] {
        let mut new = s.clone();
        new.insert_str(at, ins);
        let edits = [EditRef {
            range: at..at,
            insert: ins,
        }];
        let newsrc = StrText::new(new.clone());
        let ie = tree::input_edits(&src, &newsrc, &edits);
        let t = Instant::now();
        let (l2, st) = tree::parse(
            Lang::Svelte,
            &newsrc,
            Some(Prev {
                layers: &l,
                edits: &edits,
                input_edits: &ie,
            }),
            &|| false,
        )
        .unwrap();
        let tp = t.elapsed();
        if std::env::var_os("QS_LOOP").is_some() {
            for _ in 0..300 {
                let _ = tree::parse(
                    Lang::Svelte,
                    &newsrc,
                    Some(Prev {
                        layers: &l,
                        edits: &edits,
                        input_edits: &ie,
                    }),
                    &|| false,
                )
                .unwrap();
            }
        }
        let line = new[..at].matches('\n').count();
        let t = Instant::now();
        let vp = highlight::highlight(&l2, &newsrc, line.saturating_sub(30)..line + 30);
        let th = t.elapsed();
        let full = tree::parse(Lang::Svelte, &newsrc, None, &|| false)
            .unwrap()
            .0;
        assert_eq!(
            vp,
            highlight::highlight(&full, &newsrc, line.saturating_sub(30)..line + 30)
        );
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
    p.set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let r = R {
        start_byte: 500_000,
        end_byte: 500_007,
        start_point: Point {
            row: 0,
            column: 500_000,
        },
        end_point: Point {
            row: 0,
            column: 500_007,
        },
    };
    p.set_included_ranges(&[r]).unwrap();
    let t = Instant::now();
    for _ in 0..100 {
        let _ = p.parse_with_options(
            &mut |b: usize, _: Point| &s.as_bytes()[b.min(s.len())..],
            None,
            None,
        );
    }
    eprintln!("whole-chunk callback: {:?}/parse", t.elapsed() / 100);
    let t = Instant::now();
    for _ in 0..100 {
        let _ = p.parse_with_options(
            &mut |b: usize, _: Point| &s.as_bytes()[b.min(s.len())..(b + 64).min(s.len())],
            None,
            None,
        );
    }
    eprintln!("64-byte callback: {:?}/parse", t.elapsed() / 100);
}

#[test]
#[ignore]
fn fixture_errors() {
    let s = gen_fixture::svelte_component(400);
    let l = parse(Lang::Svelte, &s);
    for x in &l.layers {
        let root = x.data.tree.root_node();
        if root.has_error() {
            let mut c = root.walk();
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                if n.is_error() || n.is_missing() {
                    eprintln!(
                        "{:?} error at {:?}: {:?} in {:?}",
                        x.lang,
                        n.byte_range(),
                        &s[n.byte_range()].chars().take(80).collect::<String>(),
                        n.parent().map(|p| p.kind())
                    );
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
    p.set_language(&tree_sitter_htmlx_svelte::LANGUAGE.into())
        .unwrap();
    let cases: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
        ("text", Box::new(|n| "<p>hello</p>\n".repeat(n))),
        ("expr", Box::new(|n| "<p>{a + b}</p>\n".repeat(n))),
        ("if", Box::new(|n| "{#if a}\n<p>x</p>\n{/if}\n".repeat(n))),
        (
            "each",
            Box::new(|n| "{#each xs as x (x.id)}\n<p>x</p>\n{/each}\n".repeat(n)),
        ),
        (
            "attr-expr",
            Box::new(|n| "<p class:a={b === 1}>x</p>\n".repeat(n)),
        ),
        (
            "on-arrow",
            Box::new(|n| "<li on:click={() => pick(x)}>y</li>\n".repeat(n)),
        ),
        (
            "entity",
            Box::new(|n| "<h2 title=\"S 1\">{t} &amp; more</h2>\n".repeat(n)),
        ),
        ("comment", Box::new(|n| "<!-- c -->\n".repeat(n))),
        (
            "else",
            Box::new(|n| "{#if a}\n<p>x</p>\n{:else}\n<p>y</p>\n{/if}\n".repeat(n)),
        ),
    ];
    for (name, f) in &cases {
        let mut line = format!("{name:>10}:");
        for n in [500, 1000, 2000] {
            let s = format!("<main>\n{}</main>\n", f(n));
            let t = Instant::now();
            let tree = p.parse(&s, None).unwrap();
            line.push_str(&format!(
                " {n}:{:?}{}",
                t.elapsed(),
                if tree.root_node().has_error() {
                    "(ERR)"
                } else {
                    ""
                }
            ));
        }
        eprintln!("{line}");
    }
}

#[test]
#[ignore]
fn svelte_scaling2() {
    use std::time::Instant;
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_htmlx_svelte::LANGUAGE.into())
        .unwrap();
    let block = "  <section>\n    {#if a}\n      <ul>\n        <li>{x}</li>\n      </ul>\n    {:else}\n      <p>No items</p>\n    {/if}\n  </section>\n";
    let flat = "  <section>\n    {#if a}\n      <p>x</p>\n    {:else}\n      <p>No items</p>\n    {/if}\n  </section>\n";
    let noelse = "  <section>\n    {#if a}\n      <ul>\n        <li>{x}</li>\n      </ul>\n    {/if}\n  </section>\n";
    for (name, b) in [("block", block), ("flat", flat), ("noelse", noelse)] {
        let mut line = format!("{name:>8}:");
        for n in [100, 200, 400, 800] {
            let s = format!("<main>\n{}</main>\n", b.repeat(n));
            let t = Instant::now();
            let tree = p.parse(&s, None).unwrap();
            line.push_str(&format!(
                " {n}:{:?}{}",
                t.elapsed(),
                if tree.root_node().has_error() {
                    "(ERR)"
                } else {
                    ""
                }
            ));
        }
        eprintln!("{line}");
    }
}

#[test]
fn byte_range_matches_whole_pattern() {
    use streaming_iterator::StreamingIterator;
    let s = "<div>x</div>\n<script lang=\"ts\">\n  let a = 1;\n  let b = 2;\n</script>\n<p>y</p>\n";
    let l = parse(Lang::Svelte, s);
    let cfg = crate::config::get(Lang::Svelte).unwrap();
    let inj = cfg.injections.as_ref().unwrap();
    let at = s.find("let b").unwrap();
    let mut c = tree_sitter::QueryCursor::new();
    c.set_byte_range(at..at + 1);
    let mut m = c.matches(&inj.query, l.layers[0].data.tree.root_node(), s.as_bytes());
    let mut pats = Vec::new();
    while let Some(x) = m.next() {
        pats.push(x.pattern_index);
    }
    eprintln!("patterns matched in a range inside the script body: {pats:?}");
    assert!(pats.contains(&0), "the lang=\"ts\" pattern must match");
}

#[test]
#[ignore]
fn ranged_query_cost() {
    use std::time::Instant;
    use streaming_iterator::StreamingIterator;
    let s = gen_fixture::svelte_component(20_000);
    let l = parse(Lang::Svelte, &s);
    let cfg = crate::config::get(Lang::Svelte).unwrap();
    let inj = cfg.injections.as_ref().unwrap();
    let hl = &cfg.highlights;
    for frac in [0.05, 0.5, 0.95] {
        let at = (s.len() as f64 * frac) as usize;
        for (name, q) in [("injections", &inj.query), ("highlights", hl)] {
            let mut c = tree_sitter::QueryCursor::new();
            let t = Instant::now();
            let mut n = 0;
            for _ in 0..10 {
                c.set_byte_range(at..at + 2);
                let mut m = c.matches(q, l.layers[0].data.tree.root_node(), s.as_bytes());
                while m.next().is_some() {
                    n += 1;
                }
            }
            eprintln!(
                "{name} at {frac}: {:?} per query ({} matches)",
                t.elapsed() / 10,
                n / 10
            );
        }
    }
}

#[test]
#[ignore]
fn root_reparse_cost() {
    use std::time::Instant;
    use tree_sitter::{InputEdit, Parser, Point};
    let s = gen_fixture::svelte_component(20_000);
    let mut ts_src = String::new();
    while ts_src.len() < s.len() {
        ts_src.push_str("export function f(a: number, b: string): number {\n  const x = { a, b: b.length };\n  return x.a + x.b * 2;\n}\n");
    }
    for (name, lang, text) in [
        (
            "svelte",
            tree_sitter::Language::from(tree_sitter_htmlx_svelte::LANGUAGE),
            &s,
        ),
        (
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &ts_src,
        ),
    ] {
        let mut p = Parser::new();
        p.set_language(&lang).unwrap();
        let t = Instant::now();
        let tree = p.parse(text, None).unwrap();
        let full = t.elapsed();
        let mut line = format!("{name}: full {full:?};");
        for frac in [0.1, 0.5, 0.9] {
            let mut at = (text.len() as f64 * frac) as usize;
            while !text.is_char_boundary(at)
                || text.as_bytes()[at].is_ascii_whitespace()
                || !text.as_bytes()[at].is_ascii_alphabetic()
            {
                at += 1;
            }
            let mut new = text.clone();
            new.insert(at, 'x');
            let src = StrText::new(text.clone());
            let row = crate::source::Source::byte_to_line(&src, at);
            let col = at - crate::source::Source::line_to_byte(&src, row);
            let mut old = tree.clone();
            old.edit(&InputEdit {
                start_byte: at,
                old_end_byte: at,
                new_end_byte: at + 1,
                start_position: Point { row, column: col },
                old_end_position: Point { row, column: col },
                new_end_position: Point {
                    row,
                    column: col + 1,
                },
            });
            let t = Instant::now();
            let n = 20;
            for _ in 0..n {
                let _ = p.parse(&new, Some(&old)).unwrap();
            }
            line.push_str(&format!(" edit@{frac}: {:?}", t.elapsed() / n));
        }
        eprintln!("{line}");
    }
}

#[test]
#[ignore]
fn dump_sexp() {
    let s = std::env::var("QS_SRC").unwrap_or_else(|_| "<p>None &amp; zero</p>\n".into());
    let lang = Lang::from_name(&std::env::var("QS_LANG").unwrap_or_else(|_| "svelte".into())).unwrap();
    for l in &parse(lang, &s).layers {
        eprintln!("{:?} {:?}: {}", l.lang, l.span, l.data.tree.root_node().to_sexp());
    }
}
