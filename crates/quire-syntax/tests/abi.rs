//! E5 grammar ABI check: every grammar loads into tree-sitter 0.26 through `LanguageFn`.
use tree_sitter::{Language, Parser};

fn check(name: &str, lang: Language, src: &str) {
    let abi = lang.abi_version();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi),
        "{name}: ABI {abi}"
    );
    let mut p = Parser::new();
    p.set_language(&lang).unwrap();
    let tree = p.parse(src, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{name}: {}",
        tree.root_node().to_sexp()
    );
    eprintln!("{name}: ABI {abi}, {} node kinds", lang.node_kind_count());
}

#[test]
fn grammars_load() {
    check(
        "svelte",
        tree_sitter_htmlx_svelte::LANGUAGE.into(),
        "<script lang=\"ts\">let a = 1;</script>\n<p>{a}</p>\n<style>p{color:red}</style>\n",
    );
    check(
        "typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "let a: number = 1;",
    );
    check(
        "tsx",
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "const x = <div>{1}</div>;",
    );
    check(
        "javascript",
        tree_sitter_javascript::LANGUAGE.into(),
        "const a = () => 1;",
    );
    check("css", tree_sitter_css::LANGUAGE.into(), "a { color: red; }");
}

#[cfg(feature = "toml")]
#[test]
fn toml_grammar_loads() {
    check(
        "toml",
        tree_sitter_toml_ng::LANGUAGE.into(),
        "[package]\nname = \"a\"\n[[bin]]\nx = { y = [1, 2.5] }\n",
    );
}
