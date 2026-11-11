//! WGSL, Python, and WGSL inside TOML strings under a `wgsl.<name>` key (sakkan's
//! docs/design/workspace.md section 10).
#![cfg(all(feature = "toml", feature = "wgsl"))]

mod support;

use quire_syntax::Lang;
use support::{parse, render, style_of};

fn layers(s: &str) -> Vec<(Lang, String)> {
    let (_, _, t) = parse(Lang::Toml, s);
    t.injections()
        .into_iter()
        .map(|(l, r)| (l, s[r].to_string()))
        .collect()
}

const FILE: &str = "\
[style.frame.grade]
pass = \"grade.color\"
wgsl.cbrt1 = '''
fn cbrt1(x: f32) -> f32 { return sign(x) * pow(abs(x), 0.36); }
'''
";

#[test]
fn dotted_key_string_is_wgsl_without_quotes_and_first_newline() {
    assert_eq!(
        layers(FILE),
        vec![(
            Lang::Wgsl,
            "fn cbrt1(x: f32) -> f32 { return sign(x) * pow(abs(x), 0.36); }\n".into()
        )]
    );
}

#[test]
fn both_languages_highlight() {
    // TOML: the key and the string's quotes.
    assert_eq!(style_of(Lang::Toml, FILE, "pass"), "property");
    assert_eq!(style_of(Lang::Toml, FILE, "cbrt1 ="), "property");
    assert_eq!(style_of(Lang::Toml, FILE, "\"grade.color\""), "string");
    assert_eq!(style_of(Lang::Toml, FILE, "'''\nfn"), "string");
    // WGSL inside the string.
    assert_eq!(style_of(Lang::Toml, FILE, "fn cbrt1"), "keyword");
    assert_eq!(style_of(Lang::Toml, FILE, "cbrt1("), "function");
    assert_eq!(style_of(Lang::Toml, FILE, "f32)"), "type+lib");
    assert_eq!(style_of(Lang::Toml, FILE, "sign"), "function+lib");
    assert_eq!(style_of(Lang::Toml, FILE, "0.36"), "number");
    assert_eq!(style_of(Lang::Toml, FILE, "return"), "keyword");
}

#[test]
fn rendered_tokens_of_the_demo() {
    let out = render(Lang::Toml, FILE);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[2].starts_with("property:\"wgsl\""), "{}", lines[2]);
    assert!(
        lines[3].starts_with("keyword:\"fn\" function:\"cbrt1\""),
        "{}",
        lines[3]
    );
}

#[test]
fn single_line_basic_and_literal_strings() {
    let s = "wgsl.a = \"fn a() -> f32 { return 1.0; }\"\nwgsl.b = 'fn b() {}'\n";
    assert_eq!(
        layers(s),
        vec![
            (Lang::Wgsl, "fn a() -> f32 { return 1.0; }".into()),
            (Lang::Wgsl, "fn b() {}".into())
        ]
    );
    assert_eq!(style_of(Lang::Toml, s, "fn a"), "keyword");
    assert_eq!(style_of(Lang::Toml, s, "1.0"), "number");
}

#[test]
fn escapes_in_a_basic_string_stay_toml() {
    let s = "wgsl.a = \"fn a() {\\n let x = 1.0; }\"\n";
    assert_eq!(style_of(Lang::Toml, s, "\\n"), "string.escape");
    assert_eq!(style_of(Lang::Toml, s, "let"), "keyword");
    assert_eq!(style_of(Lang::Toml, s, "fn a"), "keyword");
}

#[test]
fn table_named_wgsl() {
    let s = "[style.wgsl]\nf = '''\nfn f() {}\n'''\n";
    assert_eq!(layers(s), vec![(Lang::Wgsl, "fn f() {}\n".into())]);
    let s = "[wgsl]\nf = \"fn f() {}\"\n";
    assert_eq!(layers(s), vec![(Lang::Wgsl, "fn f() {}".into())]);
}

#[test]
fn other_keys_are_plain_strings() {
    for s in [
        "name = '''fn a() {}'''\n",
        "other.f = \"fn a() {}\"\n",
        "wgslx.f = \"fn a() {}\"\n",
        "[style]\nf = \"fn a() {}\"\n",
    ] {
        assert_eq!(layers(s), vec![], "{s}");
    }
    assert_eq!(
        style_of(Lang::Toml, "name = '''fn a() {}'''\n", "fn"),
        "string"
    );
}

#[test]
fn byte_ranges_are_exact_in_the_toml_text() {
    // Diagnostics map WGSL spans back to the TOML file: the layer range is the code's own bytes.
    let (_, _, t) = parse(Lang::Toml, FILE);
    let (_, r) = t.injections().into_iter().next().unwrap();
    assert_eq!(
        &FILE[r.start..r.start + 2],
        "fn",
        "starts at the code, after the quotes and the first newline"
    );
    assert_eq!(FILE.as_bytes()[r.end], b'\'');
}

#[test]
fn wgsl_alone_and_python() {
    let w = "@fragment\nfn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }\n";
    assert_eq!(style_of(Lang::Wgsl, w, "fragment"), "attribute");
    assert_eq!(style_of(Lang::Wgsl, w, "vec4"), "type+lib");
    assert_eq!(Lang::from_extension("wgsl"), Some(Lang::Wgsl));
}
