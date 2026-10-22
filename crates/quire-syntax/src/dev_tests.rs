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
    let s = "<script lang=\"ts\">\n  let n: number = 1;\n</script>\n<p class=\"a\">{n + 1}</p>\n{#if n > 0}\n  <b>{@html x}</b>\n{/if}\n<style>\n  p { color: red; }\n</style>\n";
    let l = parse(Lang::Svelte, s);
    for x in &l.layers {
        eprintln!("{:?} {:?} depth {}", x.lang, x.span, x.depth);
    }
    eprintln!("{}", render(&l, s));
}
