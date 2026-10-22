//! VS Code's snippet parser tests (`src/vs/editor/contrib/snippet/test/browser/
//! snippetParser.test.ts` at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`),
//! translated. Markers are ids in quire's arena (`Snippet`); lengths are bytes.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::completion::snippet::{
    FormatItem, FormatString, Marker, NodeId, Scanner, Snippet, SnippetParser, TokenType as Tk,
    Transform,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum M {
    Text,
    Placeholder,
    Variable,
    Choice,
}
use M::*;

fn kind(s: &Snippet, id: NodeId) -> M {
    match s.marker(id) {
        Marker::Text(_) => Text,
        Marker::Placeholder { .. } => Placeholder,
        Marker::Variable { .. } => Variable,
        Marker::Choice(_) => Choice,
        Marker::Root => panic!("root"),
    }
}

fn parse(s: &str) -> Snippet {
    Snippet::parse(s, false, false)
}

fn top(s: &Snippet) -> Vec<NodeId> {
    s.children(Snippet::ROOT).to_vec()
}

fn assert_text(value: &str, expected: &str) {
    assert_eq!(Snippet::as_insert_text(value), expected, "{value}");
}

fn assert_marker_of(s: &Snippet, ctors: &[M]) {
    let kinds: Vec<M> = top(s).iter().map(|&m| kind(s, m)).collect();
    assert_eq!(kinds, ctors);
}

fn assert_marker(value: &str, ctors: &[M]) {
    assert_marker_of(&parse(value), ctors);
}

fn assert_text_and_marker(value: &str, escaped: &str, ctors: &[M]) {
    assert_text(value, escaped);
    assert_marker(value, ctors);
}

fn index(s: &Snippet, id: NodeId) -> u32 {
    s.placeholder_index(id).unwrap()
}

#[test]
fn scanner() {
    let mut scanner = Scanner::new();
    assert_eq!(scanner.next().ty, Tk::Eof);
    let check = |text: &str, want: &[Tk]| {
        let mut sc = Scanner::new();
        sc.text(text);
        for (i, w) in want.iter().enumerate() {
            assert_eq!(sc.next().ty, *w, "{text} token {i}");
        }
    };
    use Tk::*;
    check("abc", &[VariableName, Eof]);
    check("{{abc}}", &[CurlyOpen, CurlyOpen, VariableName, CurlyClose, CurlyClose, Eof]);
    check("abc() ", &[VariableName, Format, Eof]);
    check("abc 123", &[VariableName, Format, Int, Eof]);
    check("$foo", &[Dollar, VariableName, Eof]);
    check("$foo_bar", &[Dollar, VariableName, Eof]);
    check("$foo-bar", &[Dollar, VariableName, Dash, VariableName, Eof]);
    check("${foo}", &[Dollar, CurlyOpen, VariableName, CurlyClose, Eof]);
    check("${1223:foo}", &[Dollar, CurlyOpen, Int, Colon, VariableName, CurlyClose, Eof]);
    check("\\${}", &[Backslash, Dollar, CurlyOpen, CurlyClose]);
    check(
        "${foo/regex/format/option}",
        &[
            Dollar, CurlyOpen, VariableName, Forwardslash, VariableName, Forwardslash,
            VariableName, Forwardslash, VariableName, CurlyClose, Eof,
        ],
    );
}

#[test]
fn parser_escaped() {
    assert_eq!(Snippet::escape("foo$0"), "foo\\$0");
    assert_eq!(Snippet::escape("foo\\$0"), "foo\\\\\\$0");
    assert_eq!(Snippet::escape("f$1oo$0"), "f\\$1oo\\$0");
    assert_eq!(Snippet::escape("${1:foo}$0"), "\\${1:foo\\}\\$0");
    assert_eq!(Snippet::escape("$"), "\\$");
}

#[test]
fn parser_text() {
    for (v, e) in [
        ("$", "$"),
        ("\\\\$", "\\$"),
        ("{", "{"),
        ("\\}", "}"),
        ("\\abc", "\\abc"),
        ("foo${f:\\}}bar", "foo}bar"),
        ("\\{", "\\{"),
        ("I need \\\\\\$", "I need \\$"),
        ("\\", "\\"),
        ("\\{{", "\\{{"),
        ("{{", "{{"),
        ("{{dd", "{{dd"),
        ("}}", "}}"),
        ("ff}}", "ff}}"),
        ("farboo", "farboo"),
        ("far{{}}boo", "far{{}}boo"),
        ("far{{123}}boo", "far{{123}}boo"),
        ("far\\{{123}}boo", "far\\{{123}}boo"),
        ("far{{id:bern}}boo", "far{{id:bern}}boo"),
        ("far{{id:bern {{basel}}}}boo", "far{{id:bern {{basel}}}}boo"),
        ("far{{id:bern {{id:basel}}}}boo", "far{{id:bern {{id:basel}}}}boo"),
        ("far{{id:bern {{id2:basel}}}}boo", "far{{id:bern {{id2:basel}}}}boo"),
    ] {
        assert_text(v, e);
    }
}

#[test]
fn parser_tm_text() {
    assert_text_and_marker("foo${1:bar}}", "foobar}", &[Text, Placeholder, Text]);
    assert_text_and_marker("foo${1:bar}${2:foo}}", "foobarfoo}", &[Text, Placeholder, Placeholder, Text]);
    assert_text_and_marker("foo${1:bar\\}${2:foo}}", "foobar}foo", &[Text, Placeholder]);
    let s = parse("foo${1:bar\\}${2:foo}}");
    let p = top(&s)[1];
    assert_eq!(index(&s, p), 1);
    let ch = s.children(p);
    assert_eq!(kind(&s, ch[0]), Text);
    assert_eq!(s.node_string(ch[0]), "bar}");
    assert_eq!(kind(&s, ch[1]), Placeholder);
    assert_eq!(s.node_string(ch[1]), "foo");
}

#[test]
fn parser_placeholder() {
    assert_text_and_marker("farboo", "farboo", &[Text]);
    assert_text_and_marker("far{{}}boo", "far{{}}boo", &[Text]);
    assert_text_and_marker("far{{123}}boo", "far{{123}}boo", &[Text]);
    assert_text_and_marker("far\\{{123}}boo", "far\\{{123}}boo", &[Text]);
}

#[test]
fn parser_literal_code() {
    assert_text_and_marker("far`123`boo", "far`123`boo", &[Text]);
    assert_text_and_marker("far\\`123\\`boo", "far\\`123\\`boo", &[Text]);
}

#[test]
fn parser_variables_tabstop() {
    assert_text_and_marker("$far-boo", "-boo", &[Variable, Text]);
    assert_text_and_marker("\\$far-boo", "$far-boo", &[Text]);
    assert_text_and_marker("far$farboo", "far", &[Text, Variable]);
    assert_text_and_marker("far${farboo}", "far", &[Text, Variable]);
    assert_text_and_marker("$123", "", &[Placeholder]);
    assert_text_and_marker("$farboo", "", &[Variable]);
    assert_text_and_marker("$far12boo", "", &[Variable]);
    assert_text_and_marker("000_${far}_000", "000__000", &[Text, Variable, Text]);
    assert_text_and_marker("FFF_${TM_SELECTED_TEXT}_FFF$0", "FFF__FFF", &[Text, Variable, Text, Placeholder]);
}

#[test]
fn parser_variables_placeholder_with_defaults() {
    assert_text_and_marker("${name:value}", "value", &[Variable]);
    assert_text_and_marker("${1:value}", "value", &[Placeholder]);
    assert_text_and_marker("${1:bar${2:foo}bar}", "barfoobar", &[Placeholder]);
    assert_text_and_marker("${name:value", "${name:value", &[Text]);
    assert_text_and_marker("${1:bar${2:foobar}", "${1:barfoobar", &[Text, Placeholder]);
}

#[test]
fn parser_variable_transforms() {
    assert_text_and_marker("${foo///}", "", &[Variable]);
    assert_text_and_marker("${foo/regex/format/gmi}", "", &[Variable]);
    assert_text_and_marker("${foo/([A-Z][a-z])/format/}", "", &[Variable]);
    // invalid regex
    assert_text_and_marker("${foo/([A-Z][a-z])/format/GMI}", "${foo/([A-Z][a-z])/format/GMI}", &[Text]);
    assert_text_and_marker("${foo/([A-Z][a-z])/format/funky}", "${foo/([A-Z][a-z])/format/funky}", &[Text]);
    assert_text_and_marker("${foo/([A-Z][a-z]/format/}", "${foo/([A-Z][a-z]/format/}", &[Text]);
    // tricky regex
    assert_text_and_marker("${foo/m\\/atch/$1/i}", "", &[Variable]);
    assert_marker("${foo/regex/format/options}", &[Text]);
    // incomplete
    assert_text_and_marker("${foo///", "${foo///", &[Text]);
    assert_text_and_marker("${foo/regex/format/options", "${foo/regex/format/options", &[Text]);
    // format string
    for v in [
        "${foo/.*/${0:fooo}/i}",
        "${foo/.*/${1}/i}",
        "${foo/.*/$1/i}",
        "${foo/.*/This-$1-encloses/i}",
        "${foo/.*/complex${1:else}/i}",
        "${foo/.*/complex${1:-else}/i}",
        "${foo/.*/complex${1:+if}/i}",
        "${foo/.*/complex${1:?if:else}/i}",
        "${foo/.*/complex${1:/upcase}/i}",
    ] {
        assert_marker(v, &[Variable]);
    }
}

#[test]
fn parser_placeholder_transforms() {
    assert_text_and_marker("${1///}", "", &[Placeholder]);
    assert_text_and_marker("${1/regex/format/gmi}", "", &[Placeholder]);
    assert_text_and_marker("${1/([A-Z][a-z])/format/}", "", &[Placeholder]);
    assert_text_and_marker("${1/m\\/atch/$1/i}", "", &[Placeholder]);
    assert_marker("${1/regex/format/options}", &[Text]);
    assert_text_and_marker("${1///", "${1///", &[Text]);
    assert_text_and_marker("${1/regex/format/options", "${1/regex/format/options", &[Text]);
}

#[test]
fn escape_forward_slash_in_regex_36715() {
    assert_marker("${TM_DIRECTORY/src\\//$1/}", &[Variable]);
}

#[test]
fn escape_forward_slash_in_format_37562() {
    assert_marker("${TM_SELECTED_TEXT/a/\\/$1/g}", &[Variable]);
    assert_marker("${TM_SELECTED_TEXT/a/in\\/$1ner/g}", &[Variable]);
    assert_marker("${TM_SELECTED_TEXT/a/end\\//g}", &[Variable]);
}

#[test]
fn parser_placeholder_with_choice() {
    assert_text_and_marker("${1|one,two,three|}", "one", &[Placeholder]);
    assert_text_and_marker("${1|one|}", "one", &[Placeholder]);
    assert_text_and_marker("${1|one1,two2|}", "one1", &[Placeholder]);
    assert_text_and_marker("${1|one1\\,two2|}", "one1,two2", &[Placeholder]);
    assert_text_and_marker("${1|one1\\|two2|}", "one1|two2", &[Placeholder]);
    assert_text_and_marker("${1|one1\\atwo2|}", "one1\\atwo2", &[Placeholder]);
    assert_text_and_marker("${1|one,two,three,|}", "${1|one,two,three,|}", &[Text]);
    assert_text_and_marker("${1|one,", "${1|one,", &[Text]);
    let s = parse("${1|one,two,three|}");
    let mut seen = Vec::new();
    s.walk(|m| {
        seen.push(m);
        true
    });
    assert_eq!(seen.len(), 2);
    assert_eq!(kind(&s, seen[0]), Placeholder);
    assert!(matches!(s.marker(seen[1]), Marker::Choice(o) if o.len() == 3));
}

#[test]
fn choices_escape_comma_and_pipe_31521() {
    assert_text_and_marker(
        "console.log(${1|not\\, not, five, 5, 1   23|});",
        "console.log(not, not);",
        &[Text, Placeholder, Text],
    );
}

#[test]
fn marker_to_textmate_string() {
    for (i, e) in [
        ("$1", "$1"),
        ("\\$1", "\\$1"),
        ("console.log(${1|not\\, not, five, 5, 1   23|});", "console.log(${1|not\\, not, five, 5, 1   23|});"),
        ("console.log(${1|not\\, not, \\| five, 5, 1   23|});", "console.log(${1|not\\, not, \\| five, 5, 1   23|});"),
        ("${1|cho\\,ices,wi\\|th,esc\\\\aping,chall\\\\\\,enges|}", "${1|cho\\,ices,wi\\|th,esc\\\\aping,chall\\\\\\,enges|}"),
        ("this is text", "this is text"),
        ("this ${1:is ${2:nested with $var}}", "this ${1:is ${2:nested with ${var}}}"),
        ("this ${1:is ${2:nested with $var}}}", "this ${1:is ${2:nested with ${var}}}\\}"),
    ] {
        assert_eq!(parse(i).to_textmate_string(), e, "{i}");
    }
}

fn same_tree(a: &Snippet, x: NodeId, b: &Snippet, y: NodeId) {
    assert_eq!(std::mem::discriminant(a.marker(x)), std::mem::discriminant(b.marker(y)));
    assert_eq!(a.children(x).len(), b.children(y).len());
    assert_eq!(a.node_string(x), b.node_string(y));
    for (cx, cy) in a.children(x).iter().zip(b.children(y)) {
        same_tree(a, *cx, b, *cy);
    }
}

#[test]
fn marker_to_textmate_string_identity() {
    for input in [
        "$1",
        "\\$1",
        "console.log(${1|not\\, not, five, 5, 1   23|});",
        "console.log(${1|not\\, not, \\| five, 5, 1   23|});",
        "this is text",
        "this ${1:is ${2:nested with $var}}",
        "this ${1:is ${2:nested with $var}}}",
        "this ${1:is ${2:nested with $var}} and repeating $1",
    ] {
        let s = parse(input);
        let s2 = parse(&s.to_textmate_string());
        same_tree(&s, Snippet::ROOT, &s2, Snippet::ROOT);
    }
}

#[test]
fn parser_choice_marker() {
    let s = parse("${1|one,two,three|}");
    let ps = s.placeholders();
    assert_eq!(ps.len(), 1);
    assert_eq!(s.choice(ps[0]).map(<[String]>::len), Some(3));
    assert_text("${1|one,two,three|}", "one");
    assert_text("\\${1|one,two,three|}", "${1|one,two,three|}");
    assert_text("${1\\|one,two,three|}", "${1\\|one,two,three|}");
    assert_text("${1||}", "${1||}");
}

#[test]
fn backslash_escape_in_choice_58494() {
    let s = parse("${1|\\,,},$,\\|,\\\\|}");
    let ps = s.placeholders();
    assert_eq!(ps.len(), 1);
    assert!(s.choice(ps[0]).is_some());
}

#[test]
fn parser_only_textmate() {
    assert_marker("far{{}}boo", &[Text]);
    assert_marker("far{{123}}boo", &[Text]);
    assert_marker("far\\{{123}}boo", &[Text]);
    assert_marker("far$0boo", &[Text, Placeholder, Text]);
    assert_marker("far${123}boo", &[Text, Placeholder, Text]);
    assert_marker("far\\${123}boo", &[Text]);
}

#[test]
fn parser_real_world() {
    let s = parse("console.warn(${1: $TM_SELECTED_TEXT })");
    let m = top(&s);
    assert_eq!(s.node_string(m[0]), "console.warn(");
    assert_eq!(kind(&s, m[1]), Placeholder);
    assert_eq!(s.node_string(m[2]), ")");
    let p = m[1];
    assert_eq!(index(&s, p), 1);
    let ch = s.children(p);
    assert_eq!(ch.iter().map(|&c| kind(&s, c)).collect::<Vec<_>>(), [Text, Variable, Text]);
    assert_eq!(s.node_string(ch[0]), " ");
    assert_eq!(s.node_string(ch[1]), "");
    assert_eq!(s.node_string(ch[2]), " ");
    assert!(matches!(s.marker(ch[1]), Marker::Variable { name, .. } if name == "TM_SELECTED_TEXT"));
    assert_eq!(s.children(ch[1]).len(), 0);
    let s = parse("$TM_SELECTED_TEXT");
    assert_marker_of(&s, &[Variable]);
}

#[test]
fn parser_transform_example() {
    let s = parse("${1:name} : ${2:type}${3/\\s:=(.*)/${1:+ :=}${1}/};\n$0");
    let c = top(&s);
    assert_eq!(kind(&s, c[0]), Placeholder);
    assert_eq!(s.node_string(c[0]), "name");
    assert!(matches!(s.marker(c[0]), Marker::Placeholder { transform: None, .. }));
    assert_eq!(s.node_string(c[1]), " : ");
    assert_eq!(s.node_string(c[2]), "type");
    assert_eq!(s.children(c[3]).len(), 0);
    let Marker::Placeholder {
        transform: Some(t), ..
    } = s.marker(c[3])
    else {
        panic!("transform")
    };
    assert_eq!(t.source, "\\s:=(.*)");
    assert_eq!(t.items.len(), 2);
    assert!(matches!(&t.items[0], FormatItem::Format(f) if f.index == 1 && f.if_value.as_deref() == Some(" :=")));
    assert!(matches!(&t.items[1], FormatItem::Format(f) if f.index == 1));
    assert_eq!(s.node_string(c[4]), ";\n");
}

#[test]
fn parser_default_placeholder_values() {
    assert_marker("errorContext: `${1:err}`, error: $1", &[Text, Placeholder, Text, Placeholder]);
    let s = parse("errorContext: `${1:err}`, error:$1");
    let c = top(&s);
    for p in [c[1], c[3]] {
        assert_eq!(index(&s, p), 1);
        assert_eq!(s.children(p).len(), 1);
        assert_eq!(s.node_string(s.children(p)[0]), "err");
    }
}

#[test]
fn parser_default_placeholder_values_and_one_transform() {
    assert_marker("errorContext: `${1:err}`, error: ${1/err/ok/}", &[Text, Placeholder, Text, Placeholder]);
    let s = parse("errorContext: `${1:err}`, error:${1/err/ok/}");
    let c = top(&s);
    assert_eq!(s.node_string(c[1]), "err");
    assert!(matches!(s.marker(c[1]), Marker::Placeholder { transform: None, .. }));
    assert_eq!(s.children(c[3]).len(), 1);
    assert_eq!(s.node_string(s.children(c[3])[0]), "err");
    assert!(matches!(s.marker(c[3]), Marker::Placeholder { transform: Some(_), .. }));
}

#[test]
fn repeated_placeholder_should_inherit_31040() {
    assert_text("${1:foo}-abc-$1", "foo-abc-foo");
    assert_text("${1:foo}-abc-${1}", "foo-abc-foo");
    assert_text("${1:foo}-abc-${1:bar}", "foo-abc-foo");
    assert_text("${1}-abc-${1:foo}", "foo-abc-foo");
}

#[test]
fn backspace_escape_in_tm_only_16212() {
    assert_eq!(Snippet::as_insert_text("Foo \\\\${abc}bar"), "Foo \\bar");
}

#[test]
fn colon_as_variable_placeholder_value_16717() {
    assert_eq!(Snippet::as_insert_text("${TM_SELECTED_TEXT:foo:bar}"), "foo:bar");
    assert_eq!(Snippet::as_insert_text("${1:foo:bar}"), "foo:bar");
}

#[test]
fn incomplete_placeholder() {
    assert_text_and_marker("${1:}", "", &[Placeholder]);
}

#[test]
fn marker_len() {
    let check = |t: &str, lengths: &[usize]| {
        let s = Snippet::parse(t, true, false);
        let mut got = Vec::new();
        s.walk(|m| {
            got.push(s.len_of(m));
            true
        });
        assert_eq!(got, lengths, "{t}");
    };
    check("text$0", &[4, 0]);
    check("$1text$0", &[0, 4, 0]);
    check("te$1xt$0", &[2, 0, 2, 0]);
    check("errorContext: `${1:err}`, error: $0", &[15, 0, 3, 10, 0]);
    check("errorContext: `${1:err}`, error: $1$0", &[15, 0, 3, 10, 0, 3, 0]);
    check("$TM_SELECTED_TEXT$0", &[0, 0]);
    check("${TM_SELECTED_TEXT:def}$0", &[0, 3, 0]);
}

#[test]
fn parser_parent_node() {
    let s = Snippet::parse("This ${1:is ${2:nested}}$0", true, false);
    let ps = s.placeholders();
    assert_eq!(ps.len(), 3);
    assert_eq!((index(&s, ps[0]), index(&s, ps[1])), (1, 2));
    assert_eq!(s.parent(ps[1]), Some(ps[0]));
    assert_eq!(s.parent(ps[0]), Some(Snippet::ROOT));
    let s = Snippet::parse("${VAR:default${1:value}}$0", true, false);
    let ps = s.placeholders();
    assert_eq!(ps.len(), 2);
    assert_eq!(index(&s, ps[0]), 1);
    assert_eq!(kind(&s, top(&s)[0]), Variable);
    assert_eq!(s.parent(ps[0]), Some(top(&s)[0]));
}

#[test]
fn enclosing_placeholders() {
    let s = Snippet::parse("This ${1:is ${2:nested}}$0", true, false);
    let ps = s.placeholders();
    assert!(s.enclosing_placeholders(ps[0]).is_empty());
    assert_eq!(s.enclosing_placeholders(ps[1]), [ps[0]]);
}

#[test]
fn snippet_offset() {
    let s = Snippet::parse("te$1xt", true, false);
    let c = top(&s);
    assert_eq!(s.offset(c[0]), Some(0));
    assert_eq!(s.offset(c[1]), Some(2));
    assert_eq!(s.offset(c[2]), Some(2));
    let s = Snippet::parse("${TM_SELECTED_TEXT:def}", true, false);
    let v = top(&s)[0];
    assert_eq!(s.offset(v), Some(0));
    assert_eq!(s.offset(s.children(v)[0]), Some(0));
    // a foreign marker
    let other = parse("foo");
    let mut s2 = s.clone();
    let foreign = s2.graft(&other, &top(&other))[0];
    assert_eq!(s2.offset(foreign), None);
}

#[test]
fn snippet_placeholder() {
    for (t, n) in [("te$1xt$0", 2), ("te$1xt$1$0", 3), ("te$1xt$2$0", 3), ("${1:bar${2:foo}bar}$0", 3)] {
        assert_eq!(Snippet::parse(t, true, false).placeholders().len(), n, "{t}");
    }
}

#[test]
fn snippet_replace_1() {
    let mut s = Snippet::parse("aaa${1:bbb${2:ccc}}$0", true, false);
    assert_eq!(s.placeholders().len(), 3);
    let second = s.placeholders()[1];
    assert_eq!(index(&s, second), 2);
    let enc = s.enclosing_placeholders(second);
    assert_eq!(enc.len(), 1);
    assert_eq!(index(&s, enc[0]), 1);
    let nested = Snippet::parse("ddd$1eee$0", true, false);
    let grafted = s.graft(&nested, &top(&nested));
    s.replace(second, &grafted);
    assert_eq!(s.to_string(), "aaabbbdddeee");
    let ps = s.placeholders();
    assert_eq!(ps.len(), 4);
    assert_eq!(ps.iter().map(|&p| index(&s, p)).collect::<Vec<_>>(), [1, 1, 0, 0]);
    let enc = s.enclosing_placeholders(ps[1]);
    assert_eq!(enc, [ps[0]]);
}

#[test]
fn snippet_replace_2() {
    let mut s = Snippet::parse("aaa${1:bbb${2:ccc}}$0", true, false);
    let second = s.placeholders()[1];
    let nested = Snippet::parse("dddeee$0", true, false);
    let grafted = s.graft(&nested, &top(&nested));
    s.replace(second, &grafted);
    assert_eq!(s.to_string(), "aaabbbdddeee");
    assert_eq!(s.placeholders().len(), 3);
}

#[test]
fn placeholder_order_28185() {
    let s = parse("$10$2");
    let c = top(&s);
    assert_eq!(s.compare_by_index(c[0], c[1]), std::cmp::Ordering::Greater);
}

#[test]
fn maximum_call_stack_28983() {
    let _ = parse("${1:${foo:${1}}}");
}

#[test]
fn snippet_can_freeze_the_editor_30407() {
    for t in [
        "class ${1:${TM_FILENAME/(?:\\A|_)([A-Za-z0-9]+)(?:\\.rb)?/(?2::\\u$1)/g}} < ${2:Application}Controller\n  $3\nend",
        "${1:${FOO:abc$1def}}",
    ] {
        let s = parse(t);
        let mut seen = std::collections::BTreeSet::new();
        s.walk(|m| {
            assert!(seen.insert(m));
            true
        });
    }
}

#[test]
fn parser_ignores_choice_on_final_tabstop_31599() {
    assert_text_and_marker("${0|foo,bar|}", "${0|foo,bar|}", &[Text]);
    assert_text_and_marker("${1|foo,bar|}", "foo", &[Placeholder]);
}

fn fs(sh: Option<&str>, i: Option<&str>, e: Option<&str>) -> FormatString {
    FormatString {
        index: 1,
        shorthand_name: sh.map(str::to_string),
        if_value: i.map(str::to_string),
        else_value: e.map(str::to_string),
    }
}

#[test]
fn transform_format_string_resolve() {
    let sh = |n: &str, v: &str| fs(Some(n), None, None).resolve(Some(v));
    assert_eq!(sh("upcase", "foo"), "FOO");
    assert_eq!(sh("downcase", "FOO"), "foo");
    assert_eq!(sh("capitalize", "bar"), "Bar");
    assert_eq!(sh("capitalize", "bar no repeat"), "Bar no repeat");
    assert_eq!(sh("pascalcase", "bar-foo"), "BarFoo");
    assert_eq!(sh("pascalcase", "bar-42-foo"), "Bar42Foo");
    assert_eq!(sh("pascalcase", "snake_AndPascalCase"), "SnakeAndPascalCase");
    assert_eq!(sh("pascalcase", "kebab-AndPascalCase"), "KebabAndPascalCase");
    assert_eq!(sh("pascalcase", "_justPascalCase"), "JustPascalCase");
    assert_eq!(sh("camelcase", "bar-foo"), "barFoo");
    assert_eq!(sh("camelcase", "bar-42-foo"), "bar42Foo");
    assert_eq!(sh("camelcase", "snake_AndCamelCase"), "snakeAndCamelCase");
    assert_eq!(sh("camelcase", "kebab-AndCamelCase"), "kebabAndCamelCase");
    assert_eq!(sh("camelcase", "_JustCamelCase"), "justCamelCase");
    assert_eq!(sh("kebabcase", "barFoo"), "bar-foo");
    assert_eq!(sh("kebabcase", "BarFoo"), "bar-foo");
    assert_eq!(sh("kebabcase", "ABarFoo"), "a-bar-foo");
    assert_eq!(sh("kebabcase", "bar42Foo"), "bar42-foo");
    assert_eq!(sh("kebabcase", "snake_AndPascalCase"), "snake-and-pascal-case");
    assert_eq!(sh("kebabcase", "kebab-AndCamelCase"), "kebab-and-camel-case");
    assert_eq!(sh("kebabcase", "_justPascalCase"), "just-pascal-case");
    assert_eq!(sh("kebabcase", "__UPCASE__"), "upcase");
    assert_eq!(sh("kebabcase", "__BAR_FOO__"), "bar-foo");
    assert_eq!(sh("snakecase", "bar-foo"), "bar_foo");
    assert_eq!(sh("snakecase", "bar-42-foo"), "bar_42_foo");
    assert_eq!(sh("snakecase", "snake_AndPascalCase"), "snake_and_pascal_case");
    assert_eq!(sh("snakecase", "kebab-AndPascalCase"), "kebab_and_pascal_case");
    assert_eq!(sh("snakecase", "_justPascalCase"), "_just_pascal_case");
    assert_eq!(sh("notKnown", "input"), "input");
    // if
    assert_eq!(fs(None, Some("foo"), None).resolve(None), "");
    assert_eq!(fs(None, Some("foo"), None).resolve(Some("")), "");
    assert_eq!(fs(None, Some("foo"), None).resolve(Some("bar")), "foo");
    // else
    assert_eq!(fs(None, None, Some("foo")).resolve(None), "foo");
    assert_eq!(fs(None, None, Some("foo")).resolve(Some("")), "foo");
    assert_eq!(fs(None, None, Some("foo")).resolve(Some("bar")), "bar");
    // if-else
    assert_eq!(fs(None, Some("bar"), Some("foo")).resolve(None), "foo");
    assert_eq!(fs(None, Some("bar"), Some("foo")).resolve(Some("")), "foo");
    assert_eq!(fs(None, Some("bar"), Some("foo")).resolve(Some("baz")), "bar");
}

#[test]
fn unicode_variable_transformations() {
    let resolver = |name: &str| -> Option<String> {
        Some(
            match name {
                "RUSSIAN" => "одинДва",
                "GREEK" => "έναςΔύο",
                "TURKISH" => "istanbulLı",
                "JAPANESE" => "こんにちは",
                _ => return None,
            }
            .to_string(),
        )
    };
    let check = |tr: &str, var: &str, expected: &str| {
        let mut s = parse(&format!("${{{var}/(.*)/${{1:/{tr}}}/}}"));
        s.resolve_variables(&resolver);
        assert_eq!(s.to_string(), expected, "{tr} failed for {var}");
    };
    check("kebabcase", "RUSSIAN", "один-два");
    check("kebabcase", "GREEK", "ένας-δύο");
    check("snakecase", "RUSSIAN", "один_два");
    check("snakecase", "GREEK", "ένας_δύο");
    check("camelcase", "RUSSIAN", "одинДва");
    check("camelcase", "GREEK", "έναςΔύο");
    check("pascalcase", "RUSSIAN", "ОдинДва");
    check("pascalcase", "GREEK", "ΈναςΔύο");
    check("upcase", "RUSSIAN", "ОДИНДВА");
    check("downcase", "RUSSIAN", "одиндва");
    check("kebabcase", "TURKISH", "istanbul-lı");
    check("pascalcase", "TURKISH", "IstanbulLı");
    check("upcase", "JAPANESE", "こんにちは");
    check("kebabcase", "JAPANESE", "こんにちは");
}

#[test]
fn transform_with_complicated_regex_and_dollars_55627() {
    let s = parse("const fileName = \"${TM_FILENAME/(.*)\\..+$/$1/}\"");
    assert_eq!(s.to_textmate_string(), "const fileName = \"${TM_FILENAME/(.*)\\..+$/${1}/}\"");
}

#[test]
fn html_attribute_end_position_33147() {
    let s = Snippet::parse("src=\"$1\"", true, false);
    let ps = s.placeholders();
    assert_eq!(ps.len(), 2);
    assert_eq!((index(&s, ps[0]), index(&s, ps[1])), (1, 0));
}

#[test]
fn optional_transforms_reusing_variable_37702() {
    let t = Transform::new(
        "^(.)|-(.)",
        "g",
        vec![
            FormatItem::Format(fs(Some("upcase"), None, None)),
            FormatItem::Format(FormatString {
                index: 2,
                ..fs(Some("upcase"), None, None)
            }),
        ],
    )
    .unwrap();
    assert_eq!(t.resolve("my-file-name"), "MyFileName");
    let clone = t.clone();
    assert_eq!(clone.resolve("my-file-name"), "MyFileName");
}

#[test]
fn problem_with_snippets_regex_40570() {
    assert_marker("${TM_DIRECTORY/.*src[\\/](.*)/$1/}", &[Variable]);
}

#[test]
fn undefined_variables_in_same_snippet_51769() {
    let t = Transform::new("foo", "gi", vec![FormatItem::Text("bar".into())]).unwrap();
    assert_eq!(t.to_textmate_string(), "/foo/bar/ig");
}

#[test]
fn transform_serialization_joins_children_without_comma() {
    let t = Transform::new(
        "foo",
        "g",
        vec![FormatItem::Format(fs(Some("upcase"), None, None)), FormatItem::Text("_".into())],
    )
    .unwrap();
    let ser = t.to_textmate_string();
    assert_eq!(ser, "/foo/${1:/upcase}_/g");
    let s = parse(&format!("${{TM_FILENAME{ser}}}"));
    assert_eq!(s.to_textmate_string(), format!("${{TM_FILENAME{ser}}}"));
}

#[test]
fn snippet_parser_freeze_53144() {
    assert_marker("${1/(void$)|(.+)/${1:?-\treturn nil;}/}", &[Placeholder]);
}

#[test]
fn variable_not_resolved_in_json_proposal_52931() {
    assert_text_and_marker("FOO${1:/bin/bash}", "FOO/bin/bash", &[Text, Placeholder]);
}

#[test]
fn mirroring_nested_placeholders_backjumping_58736() {
    let s = parse("${3:nest1 ${1:nest2 ${2:nest3}}} $3");
    let c = top(&s);
    assert_eq!(c.iter().map(|&m| kind(&s, m)).collect::<Vec<_>>(), [Placeholder, Text, Placeholder]);
    fn reaches_root(s: &Snippet, m: NodeId) {
        for &k in s.children(m) {
            reaches_root(s, k);
        }
        if s.placeholder_index(m).is_none() {
            return;
        }
        let mut p = s.parent(m);
        let mut found = false;
        while let Some(id) = p {
            if id == Snippet::ROOT {
                found = true;
                break;
            }
            p = s.parent(id);
        }
        assert!(found);
    }
    reaches_root(&s, c[2]);
}

#[test]
fn backspace_escaped_in_variable_transforms_65412() {
    assert_marker("namespace ${TM_DIRECTORY/[\\/]/\\\\/g};", &[Text, Variable, Text]);
}

fn only_format(s: &Snippet) -> FormatString {
    let c = top(s);
    assert_eq!(c.len(), 1);
    let Marker::Variable {
        transform: Some(t), ..
    } = s.marker(c[0])
    else {
        panic!("variable with transform")
    };
    assert_eq!(t.items.len(), 1);
    let FormatItem::Format(f) = &t.items[0] else {
        panic!("format string")
    };
    f.clone()
}

#[test]
fn escape_closing_bracket_in_conditional_78883() {
    let f = only_format(&parse("${TM_DIRECTORY/(.+)/${1:+import { hello \\} from world}/}"));
    assert_eq!(f.if_value.as_deref(), Some("import { hello } from world"));
    assert_eq!(f.else_value, None);
}

#[test]
fn escape_backslashes_in_conditional_80394() {
    let f = only_format(&parse("${CURRENT_YEAR/(.+)/${1:+\\\\}/}"));
    assert_eq!(f.if_value.as_deref(), Some("\\"));
    assert_eq!(f.else_value, None);
}

#[test]
fn placeholder_empty_right_after_expansion_152553() {
    assert_eq!(parse("${1:prog}: ${2:$1.cc} - $2").to_string(), "prog: prog.cc - prog.cc");
    assert_eq!(
        parse("${1:prog}: ${3:${2:$1.cc}.33} - $2 $3").to_string(),
        "prog: prog.cc.33 - prog.cc prog.cc.33"
    );
    assert_eq!(parse("${1:$2.one} <> ${2:$1.two}").to_string(), ".two.one.two.one <> .one.two.one.two");
}

#[test]
fn choices_incorrectly_escaped_180132() {
    assert_text_and_marker("${1|aaa$aaa|}bbb\\$bbb", "aaa$aaabbb$bbb", &[Placeholder, Text]);
}

#[test]
fn parser_api_is_reusable() {
    let mut p = SnippetParser::new();
    assert_eq!(p.parse("$1", false, false).placeholders().len(), 1);
    assert_eq!(p.parse("a", false, false).to_string(), "a");
}
