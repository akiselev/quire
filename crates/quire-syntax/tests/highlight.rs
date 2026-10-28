//! Grammar fixtures: expected token runs per language, plus spot checks that do not depend on
//! the blessed files.

mod support;

use quire_syntax::Lang;
use support::{expect_file, render, style_of};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn queries_compile() {
    quire_syntax::check_queries().unwrap();
    for lang in Lang::ALL {
        if lang == Lang::Toml {
            // an optional feature
            assert_eq!(lang.is_available(), cfg!(feature = "toml"));
            continue;
        }
        assert!(lang.is_available(), "{lang} is a default feature");
    }
}

#[test]
fn svelte_fixture() {
    expect_file(
        "component.svelte.tokens",
        &render(Lang::Svelte, &fixture("component.svelte")),
    );
}

#[test]
fn typescript_fixture() {
    expect_file(
        "sample.ts.tokens",
        &render(Lang::TypeScript, &fixture("sample.ts")),
    );
}

#[test]
fn tsx_fixture() {
    expect_file(
        "sample.tsx.tokens",
        &render(Lang::Tsx, &fixture("sample.tsx")),
    );
}

#[test]
fn javascript_fixture() {
    expect_file(
        "sample.js.tokens",
        &render(Lang::JavaScript, &fixture("sample.js")),
    );
}

#[test]
fn css_fixture() {
    expect_file(
        "sample.css.tokens",
        &render(Lang::Css, &fixture("sample.css")),
    );
}

#[test]
fn typescript_spots() {
    let s = fixture("sample.ts");
    assert_eq!(style_of(Lang::TypeScript, &s, "interface"), "keyword");
    assert_eq!(style_of(Lang::TypeScript, &s, "Point"), "type");
    assert_eq!(style_of(Lang::TypeScript, &s, "number"), "type+lib");
    assert_eq!(style_of(Lang::TypeScript, &s, "load"), "function");
    assert_eq!(style_of(Lang::TypeScript, &s, "path: string"), "parameter");
    assert_eq!(style_of(Lang::TypeScript, &s, "\"utf8\""), "string");
    assert_eq!(style_of(Lang::TypeScript, &s, "// A comment"), "comment");
    assert_eq!(style_of(Lang::TypeScript, &s, "/ab+c/"), "string.special");
    assert_eq!(style_of(Lang::TypeScript, &s, "null"), "constant+lib");
    assert_eq!(
        style_of(Lang::TypeScript, &s, "<T extends"),
        "punctuation.bracket"
    );
}

#[test]
fn javascript_spots() {
    let s = fixture("sample.js");
    assert_eq!(style_of(Lang::JavaScript, &s, "MAX_SIZE"), "constant");
    assert_eq!(style_of(Lang::JavaScript, &s, "name, {"), "parameter");
    assert_eq!(style_of(Lang::JavaScript, &s, "console"), "variable+lib");
    assert_eq!(style_of(Lang::JavaScript, &s, "toUpperCase"), "method");
    assert_eq!(style_of(Lang::JavaScript, &s, "typeof"), "keyword");
    assert_eq!(style_of(Lang::JavaScript, &s, "div"), "tag");
    assert_eq!(style_of(Lang::JavaScript, &s, "className"), "attribute");
}

#[test]
fn css_spots() {
    let s = fixture("sample.css");
    assert_eq!(style_of(Lang::Css, &s, "@media"), "keyword");
    assert_eq!(style_of(Lang::Css, &s, "--accent"), "variable");
    assert_eq!(style_of(Lang::Css, &s, "btn"), "property");
    assert_eq!(style_of(Lang::Css, &s, "color:"), "property");
    assert_eq!(style_of(Lang::Css, &s, "1.5"), "number");
    assert_eq!(style_of(Lang::Css, &s, "em;"), "type");
    assert_eq!(style_of(Lang::Css, &s, "/* Buttons */"), "comment");
    assert_eq!(style_of(Lang::Css, &s, "var("), "function");
    assert_eq!(style_of(Lang::Css, &s, "!important"), "keyword");
}

#[cfg(feature = "toml")]
#[test]
fn toml_fixture() {
    expect_file(
        "sample.toml.tokens",
        &render(Lang::Toml, &fixture("sample.toml")),
    );
}

#[cfg(feature = "toml")]
#[test]
fn toml_spots() {
    let s = fixture("sample.toml");
    assert_eq!(style_of(Lang::Toml, &s, "# A Cargo"), "comment");
    assert_eq!(style_of(Lang::Toml, &s, "package"), "type");
    assert_eq!(style_of(Lang::Toml, &s, "profile"), "type");
    assert_eq!(style_of(Lang::Toml, &s, "release"), "type");
    assert_eq!(style_of(Lang::Toml, &s, "bin"), "type");
    assert_eq!(style_of(Lang::Toml, &s, "name ="), "property");
    assert_eq!(style_of(Lang::Toml, &s, "\"quoted key\""), "property");
    assert_eq!(style_of(Lang::Toml, &s, "workspace"), "property");
    assert_eq!(style_of(Lang::Toml, &s, "features"), "property");
    assert_eq!(style_of(Lang::Toml, &s, "\"quire-demo\""), "string");
    assert_eq!(style_of(Lang::Toml, &s, "'literal'"), "string");
    assert_eq!(style_of(Lang::Toml, &s, "\\t"), "string.escape");
    assert_eq!(style_of(Lang::Toml, &s, "false"), "boolean");
    assert_eq!(style_of(Lang::Toml, &s, "3\n"), "number");
    assert_eq!(style_of(Lang::Toml, &s, "1.5e3"), "number");
    assert_eq!(style_of(Lang::Toml, &s, "1979-05-27T"), "string.special");
    assert_eq!(style_of(Lang::Toml, &s, "= \"quire"), "operator");
    assert_eq!(style_of(Lang::Toml, &s, "[["), "punctuation.bracket");
    assert_eq!(
        style_of(Lang::Toml, &s, ".workspace"),
        "punctuation.delimiter"
    );
    // by name and extension, as the other languages
    assert_eq!(Lang::from_name("TOML"), Some(Lang::Toml));
    assert_eq!(Lang::from_extension("toml"), Some(Lang::Toml));
    assert_eq!(Lang::Toml.id(), "toml");
}

#[test]
fn svelte_spots() {
    let s = fixture("component.svelte");
    assert_eq!(style_of(Lang::Svelte, &s, "main class"), "tag");
    assert_eq!(style_of(Lang::Svelte, &s, "Counter"), "type");
    assert_eq!(style_of(Lang::Svelte, &s, "<!-- A comment -->"), "comment");
    // tree-sitter-htmlx-svelte keeps entities inside the text token (no `entity` node there).
    assert_eq!(style_of(Lang::Svelte, &s, "if count"), "keyword");
    assert_eq!(style_of(Lang::Svelte, &s, "\"app\""), "string");
    assert_eq!(style_of(Lang::Svelte, &s, "bind:"), "keyword");
}
