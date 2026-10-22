//! Svelte and JavaScript injection fixtures.

mod support;

use quire_syntax::Lang;
use support::{parse, style_of};

fn layers(lang: Lang, s: &str) -> Vec<(Lang, String)> {
    let (_, _, t) = parse(lang, s);
    t.injections()
        .into_iter()
        .map(|(l, r)| (l, s[r].to_string()))
        .collect()
}

// tree-sitter-htmlx-svelte's raw_text starts after the newline that follows the start tag.
#[test]
fn script_lang_ts_is_typescript() {
    let s = "<script lang=\"ts\">\nlet a: number = 1;\n</script>\n";
    assert_eq!(
        layers(Lang::Svelte, s),
        vec![(Lang::TypeScript, "let a: number = 1;\n".into())]
    );
    assert_eq!(style_of(Lang::Svelte, s, "number"), "type+lib");
}

#[test]
fn script_lang_typescript_and_unquoted() {
    for open in [
        "<script lang=\"typescript\">",
        "<script lang='ts'>",
        "<script lang=ts>",
        "<script context=\"module\" lang=\"ts\">",
    ] {
        let s = format!("{open}let a: string;</script>\n");
        let l = layers(Lang::Svelte, &s);
        assert_eq!(l.len(), 1, "{open}: {l:?}");
        assert_eq!(l[0].0, Lang::TypeScript, "{open}");
    }
}

#[test]
fn plain_script_is_javascript() {
    let s = "<script>\nlet a = 1;\n</script>\n<p>{a}</p>\n";
    assert_eq!(
        layers(Lang::Svelte, s),
        vec![
            (Lang::JavaScript, "let a = 1;\n".into()),
            (Lang::JavaScript, "a".into())
        ]
    );
}

#[test]
fn style_is_css_and_other_style_langs_are_skipped() {
    let s = "<style>\np { color: red; }\n</style>\n";
    assert_eq!(
        layers(Lang::Svelte, s),
        vec![(Lang::Css, "p { color: red; }\n".into())]
    );
    assert_eq!(style_of(Lang::Svelte, s, "color"), "property");
    let scss = "<style lang=\"scss\">\n$x: 1;\np { color: $x; }\n</style>\n";
    assert_eq!(
        layers(Lang::Svelte, scss),
        vec![],
        "scss is not parsed as CSS"
    );
}

#[test]
fn expressions_follow_the_script_language() {
    let ts = "<script lang=\"ts\">let n = 1;</script>\n<p title={n as number}>{n + 1}</p>\n{#if n > 0}<b>{@html n}</b>{/if}\n";
    let l = layers(Lang::Svelte, ts);
    assert!(
        l.iter().skip(1).all(|(lang, _)| *lang == Lang::TypeScript),
        "{l:?}"
    );
    let exprs: Vec<&str> = l.iter().skip(1).map(|(_, s)| s.as_str()).collect();
    assert_eq!(exprs, vec!["n as number", "n + 1", "n > 0", "n"]);
    assert_eq!(style_of(Lang::Svelte, ts, "as number"), "keyword");

    let js = "<script>let n = 1;</script>\n<p>{n + 1}</p>\n";
    let l = layers(Lang::Svelte, js);
    assert_eq!(l[1], (Lang::JavaScript, "n + 1".into()));

    let none = "<p>{n + 1}</p>\n";
    assert_eq!(
        layers(Lang::Svelte, none),
        vec![(Lang::JavaScript, "n + 1".into())]
    );
}

#[test]
fn each_block_parts_and_event_handlers() {
    let s = "<script lang=\"ts\"></script>\n{#each items as item, i (item.id)}\n<li on:click={() => pick(item)}>{i}</li>\n{/each}\n";
    let exprs: Vec<String> = layers(Lang::Svelte, s)
        .into_iter()
        .skip(1)
        .map(|(_, s)| s)
        .collect();
    for want in ["items", "item", "i", "item.id", "() => pick(item)"] {
        assert!(exprs.iter().any(|e| e == want), "{want:?} not in {exprs:?}");
    }
    assert_eq!(style_of(Lang::Svelte, s, "pick"), "function");
    assert_eq!(style_of(Lang::Svelte, s, "=>"), "operator");
}

#[test]
fn css_in_tagged_templates() {
    let s = "const a = css`\n  .btn { color: blue; }\n`;\nconst b = styled.div`margin: 0;`;\nconst c = html`<p></p>`;\n";
    let l = layers(Lang::JavaScript, s);
    assert_eq!(l.len(), 1, "{l:?}");
    assert_eq!(l[0].0, Lang::Css);
    assert_eq!(style_of(Lang::JavaScript, s, "btn"), "property");
}

#[test]
fn nested_injection_svelte_script_css_template() {
    let s = "<script>\nconst a = css`.x { color: red; }`;\n</script>\n";
    let l = layers(Lang::Svelte, s);
    assert_eq!(
        l.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
        vec![Lang::JavaScript, Lang::Css]
    );
    assert_eq!(style_of(Lang::Svelte, s, "color"), "property");
    assert_eq!(style_of(Lang::Svelte, s, "const"), "keyword");
}

#[test]
fn script_style_highlights_do_not_leak() {
    // Markup between the blocks keeps Svelte styles; the CSS layer does not paint over it.
    let s = "<style>p{}</style>\n<p class=\"a\">x</p>\n<script>let x;</script>\n";
    assert_eq!(style_of(Lang::Svelte, s, "class"), "attribute");
    assert_eq!(style_of(Lang::Svelte, s, "let"), "keyword");
}
