//! Folds, bracket pairs and indent hints.

mod support;

use quire_core::syntax::{FoldKind, Highlighter, IndentHint};
use quire_syntax::Lang;
use support::parse;

#[test]
fn typescript_folds() {
    let s = "/**\n * doc\n */\nfunction f(\n  a: number,\n) {\n  if (a) {\n    return 1;\n  }\n}\n";
    let (h, _, t) = parse(Lang::TypeScript, s);
    let f: Vec<(usize, usize, FoldKind)> = h
        .folds(&t)
        .into_iter()
        .map(|f| (f.start_line, f.end_line, f.kind))
        .collect();
    assert_eq!(
        f,
        vec![
            (0, 2, FoldKind::Comment),
            (3, 5, FoldKind::Other),
            (5, 9, FoldKind::Other),
            (6, 8, FoldKind::Other)
        ]
    );
}

#[test]
fn svelte_folds_include_injected_layers() {
    let s = "<script>\nfunction f() {\n  return 1;\n}\n</script>\n{#if a}\n  <p>\n    x\n  </p>\n{/if}\n<style>\np {\n  color: red;\n}\n</style>\n";
    let (h, _, t) = parse(Lang::Svelte, s);
    let starts: Vec<usize> = h.folds(&t).into_iter().map(|f| f.start_line).collect();
    // script element, JS function body, if block, <p>, style element, CSS block
    for line in [0, 1, 5, 6, 10, 11] {
        assert!(starts.contains(&line), "fold at line {line}: {starts:?}");
    }
}

#[test]
fn brackets_in_a_line_range() {
    let s = "const a = [1, (2 + 3)];\nlet b = {};\n";
    let (h, _, t) = parse(Lang::JavaScript, s);
    let pairs: Vec<(String, String)> = h
        .brackets(&t, 0..1)
        .into_iter()
        .map(|b| (s[b.open].to_string(), s[b.close].to_string()))
        .collect();
    assert_eq!(
        pairs,
        vec![("[".into(), "]".into()), ("(".into(), ")".into())]
    );
    let all = h.brackets(&t, 0..2);
    assert_eq!(all.len(), 3);
    assert_eq!(&s[all[2].open.clone()], "{");
}

#[test]
fn svelte_brackets_cover_tags_and_expressions() {
    let s = "<p>{a}</p>\n";
    let (h, _, t) = parse(Lang::Svelte, s);
    let opens: Vec<&str> = h
        .brackets(&t, 0..1)
        .into_iter()
        .map(|b| &s[b.open])
        .collect();
    assert!(
        opens.contains(&"<") && opens.contains(&"{") && opens.contains(&"</"),
        "{opens:?}"
    );
}

#[test]
fn indent_levels_and_hints() {
    let s = "function f() {\n  if (a) {\n    b();\n  }\n}\n";
    let (h, sn, t) = parse(Lang::JavaScript, s);
    let levels: Vec<Option<u32>> = (0..5).map(|l| h.indent_level(&t, l)).collect();
    assert_eq!(levels, vec![Some(0), Some(1), Some(2), Some(1), Some(0)]);
    assert_eq!(h.indent(&t, &sn, 1), Some(IndentHint::Indent));
    assert_eq!(h.indent(&t, &sn, 3), Some(IndentHint::Outdent));
    assert_eq!(h.indent(&t, &sn, 2), Some(IndentHint::Indent));
}

#[test]
fn indent_of_a_fresh_empty_line_inside_a_block() {
    let s = "function f() {\n\n}\n";
    let (h, _, t) = parse(Lang::JavaScript, s);
    assert_eq!(h.indent_level(&t, 1), Some(1));
}

#[test]
fn svelte_indent_sums_layers() {
    let s = "<div>\n  {#if a}\n    <p>x</p>\n  {:else}\n    <b>y</b>\n  {/if}\n</div>\n<script>\n  function f() {\n    return 1;\n  }\n</script>\n";
    let (h, _, t) = parse(Lang::Svelte, s);
    let levels: Vec<Option<u32>> = (0..12).map(|l| h.indent_level(&t, l)).collect();
    assert_eq!(
        levels,
        vec![
            Some(0),
            Some(1),
            Some(2),
            Some(1),
            Some(2),
            Some(1),
            Some(0),
            Some(0),
            Some(1),
            Some(2),
            Some(1),
            Some(0)
        ]
    );
}

#[test]
fn css_indent() {
    let s = "a {\n  color: red;\n}\n";
    let (h, _, t) = parse(Lang::Css, s);
    assert_eq!(
        (0..3).map(|l| h.indent_level(&t, l)).collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(0)]
    );
}
