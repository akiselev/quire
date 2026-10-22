//! VS Code's fuzzyScore tests (`src/vs/base/test/common/filters.test.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated. One Rust test per VS Code test;
//! the map is `tests/VSCODE-MAP.md` (`tests/port/hand.json`).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::completion::filters::{
    FuzzyScoreOptions, FuzzyScorer, any_score, create_matches, fuzzy_score, fuzzy_score_graceful,
    fuzzy_score_graceful_aggressive, utf16, utf16_lower,
};

#[derive(Default)]
struct Opts {
    pattern_pos: usize,
    word_pos: usize,
    first_match_can_be_weak: bool,
}

fn any_scorer(
    p: &[u16],
    lp: &[u16],
    pp: usize,
    w: &[u16],
    lw: &[u16],
    wp: usize,
    _o: FuzzyScoreOptions,
) -> Option<quire_core::completion::FuzzyScore> {
    Some(any_score(p, lp, pp, w, lw, wp))
}

fn assert_matches(pattern: &str, word: &str, decorated: Option<&str>, filter: FuzzyScorer, o: Opts) {
    let p = utf16(pattern);
    let w = utf16(word);
    let r = filter(
        &p,
        &utf16_lower(&p),
        o.pattern_pos,
        &w,
        &utf16_lower(&w),
        o.word_pos,
        FuzzyScoreOptions {
            first_match_can_be_weak: o.first_match_can_be_weak,
            boost_full_match: true,
        },
    );
    assert_eq!(decorated.is_none(), r.is_none(), "{pattern} / {word}: {r:?}");
    if let Some(r) = r {
        let matches = create_matches(Some(&r));
        let mut actual: Vec<u16> = Vec::new();
        let mut pos = 0;
        for m in matches {
            actual.extend_from_slice(&w[pos..m.start]);
            for &u in &w[m.start..m.end] {
                actual.push(b'^' as u16);
                actual.push(u);
            }
            pos = m.end;
        }
        actual.extend_from_slice(&w[pos..]);
        assert_eq!(String::from_utf16_lossy(&actual), decorated.unwrap(), "{pattern} / {word}");
    }
}

fn am(pattern: &str, word: &str, decorated: Option<&str>) {
    assert_matches(pattern, word, decorated, fuzzy_score, Opts::default());
}

fn assert_top_score(filter: FuzzyScorer, pattern: &str, expected: usize, words: &[&str]) {
    let mut top_score = -(100 * 10);
    let mut top_idx = 0;
    let p = utf16(pattern);
    for (i, word) in words.iter().enumerate() {
        let w = utf16(word);
        if let Some(m) = filter(
            &p,
            &utf16_lower(&p),
            0,
            &w,
            &utf16_lower(&w),
            0,
            FuzzyScoreOptions::default(),
        ) && m.score > top_score
        {
            top_score = m.score;
            top_idx = i;
        }
    }
    assert_eq!(
        top_idx, expected,
        "{pattern} -> actual={} <> expected={}",
        words[top_idx], words[expected]
    );
}

#[test]
fn fuzzy_score_23215() {
    am("tit", "win.tit", Some("win.^t^i^t"));
    am("title", "win.title", Some("win.^t^i^t^l^e"));
    am("WordCla", "WordCharacterClassifier", Some("^W^o^r^dCharacter^C^l^assifier"));
    am("WordCCla", "WordCharacterClassifier", Some("^W^o^r^d^Character^C^l^assifier"));
}

#[test]
fn fuzzy_score_23332() {
    am("dete", "\"editor.quickSuggestionsDelay\"", None);
}

#[test]
fn fuzzy_score_23190() {
    am("c:\\do", "& 'C:\\Documents and Settings'", Some("& '^C^:^\\^D^ocuments and Settings'"));
    am("c:\\do", "& 'c:\\Documents and Settings'", Some("& '^c^:^\\^D^ocuments and Settings'"));
}

#[test]
fn fuzzy_score_23581() {
    am("close", "css.lint.importStatement", Some("^css.^lint.imp^ort^Stat^ement"));
    am("close", "css.colorDecorators.enable", Some("^css.co^l^orDecorator^s.^enable"));
    am("close", "workbench.quickOpen.closeOnFocusOut", Some("workbench.quickOpen.^c^l^o^s^eOnFocusOut"));
    assert_top_score(
        fuzzy_score,
        "close",
        2,
        &["css.lint.importStatement", "css.colorDecorators.enable", "workbench.quickOpen.closeOnFocusOut"],
    );
}

#[test]
fn fuzzy_score_23458() {
    am("highlight", "editorHoverHighlight", Some("editorHover^H^i^g^h^l^i^g^h^t"));
    am("hhighlight", "editorHoverHighlight", Some("editor^Hover^H^i^g^h^l^i^g^h^t"));
    am("dhhighlight", "editorHoverHighlight", None);
}

#[test]
fn fuzzy_score_23746() {
    am("-moz", "-moz-foo", Some("^-^m^o^z-foo"));
    am("moz", "-moz-foo", Some("-^m^o^z-foo"));
    am("moz", "-moz-animation", Some("-^m^o^z-animation"));
    am("moza", "-moz-animation", Some("-^m^o^z-^animation"));
}

#[test]
fn fuzzy_score_basic() {
    am("ab", "abA", Some("^a^bA"));
    am("ccm", "cacmelCase", Some("^ca^c^melCase"));
    am("bti", "the_black_knight", None);
    am("ccm", "camelCase", None);
    am("cmcm", "camelCase", None);
    am("BK", "the_black_knight", Some("the_^black_^knight"));
    am("KeyboardLayout=", "KeyboardLayout", None);
    am("LLL", "SVisualLoggerLogsList", Some("SVisual^Logger^Logs^List"));
    am("LLLL", "SVilLoLosLi", None);
    am("LLLL", "SVisualLoggerLogsList", None);
    am("TEdit", "TextEdit", Some("^Text^E^d^i^t"));
    am("TEdit", "TextEditor", Some("^Text^E^d^i^tor"));
    am("TEdit", "Textedit", Some("^Text^e^d^i^t"));
    am("TEdit", "text_edit", Some("^text_^e^d^i^t"));
    am("TEditDit", "TextEditorDecorationType", Some("^Text^E^d^i^tor^Decorat^ion^Type"));
    am("TEdit", "TextEditorDecorationType", Some("^Text^E^d^i^torDecorationType"));
    am("Tedit", "TextEdit", Some("^Text^E^d^i^t"));
    am("ba", "?AB?", None);
    am("bkn", "the_black_knight", Some("the_^black_^k^night"));
    am("bt", "the_black_knight", Some("the_^black_knigh^t"));
    am("ccm", "camelCasecm", Some("^camel^Casec^m"));
    am("fdm", "findModel", Some("^fin^d^Model"));
    am("fob", "foobar", Some("^f^oo^bar"));
    am("fobz", "foobar", None);
    am("foobar", "foobar", Some("^f^o^o^b^a^r"));
    am("form", "editor.formatOnSave", Some("editor.^f^o^r^matOnSave"));
    am("g p", "Git: Pull", Some("^Git:^ ^Pull"));
    am("g p", "Git: Pull", Some("^Git:^ ^Pull"));
    am("gip", "Git: Pull", Some("^G^it: ^Pull"));
    am("gip", "Git: Pull", Some("^G^it: ^Pull"));
    am("gp", "Git: Pull", Some("^Git: ^Pull"));
    am("gp", "Git_Git_Pull", Some("^Git_Git_^Pull"));
    am("is", "ImportStatement", Some("^Import^Statement"));
    am("is", "isValid", Some("^i^sValid"));
    am("lowrd", "lowWord", Some("^l^o^wWo^r^d"));
    am("myvable", "myvariable", Some("^m^y^v^aria^b^l^e"));
    am("no", "", None);
    am("no", "match", None);
    am("ob", "foobar", None);
    am("sl", "SVisualLoggerLogsList", Some("^SVisual^LoggerLogsList"));
    am("sllll", "SVisualLoggerLogsList", Some("^SVisua^l^Logger^Logs^List"));
    am("Three", "HTMLHRElement", None);
    am("Three", "Three", Some("^T^h^r^e^e"));
    am("fo", "barfoo", None);
    am("fo", "bar_foo", Some("bar_^f^oo"));
    am("fo", "bar_Foo", Some("bar_^F^oo"));
    am("fo", "bar foo", Some("bar ^f^oo"));
    am("fo", "bar.foo", Some("bar.^f^oo"));
    am("fo", "bar/foo", Some("bar/^f^oo"));
    am("fo", "bar\\foo", Some("bar\\^f^oo"));
}

fn weak() -> Opts {
    Opts {
        first_match_can_be_weak: true,
        ..Opts::default()
    }
}

#[test]
fn fuzzy_score_first_match_can_be_weak() {
    assert_matches("Three", "HTMLHRElement", Some("H^TML^H^R^El^ement"), fuzzy_score, weak());
    assert_matches("tor", "constructor", Some("construc^t^o^r"), fuzzy_score, weak());
    assert_matches("ur", "constructor", Some("constr^ucto^r"), fuzzy_score, weak());
    assert_top_score(fuzzy_score, "tor", 2, &["constructor", "Thor", "cTor"]);
}

#[test]
fn fuzzy_score_many_matches() {
    let word = "a".repeat(311);
    let decorated = format!("^a^a^a^a^a^a{}", "a".repeat(305));
    am("aaaaaa", &word, Some(&decorated));
}

#[test]
fn freeze_when_fjfj_jfjf_91807() {
    am("jfjfj", "fjfjfjfjfjfjfjfjfjfjfj", None);
    am("jfjfjfjfjfjfjfjfjfj", "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj", None);
    am(
        "jfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfjjfjfjfjfjfjfjfjfjfj",
        "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj",
        None,
    );
    am(
        "jfjfjfjfjfjfjfjfjfj",
        "fJfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj",
        Some("f^J^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^jfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj"),
    );
    assert_matches(
        "jfjfjfjfjfjfjfjfjfj",
        "fjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj",
        Some("f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^j^f^jfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfjfj"),
        fuzzy_score,
        weak(),
    );
}

#[test]
fn fuzzy_score_26423() {
    am("baba", "abababab", None);
    am("fsfsfs", "dsafdsafdsafdsafdsafdsafdsafasdfdsa", None);
    am(
        "fsfsfsfsfsfsfsf",
        "dsafdsafdsafdsafdsafdsafdsafasdfdsafdsafdsafdsafdsfdsafdsfdfdfasdnfdsajfndsjnafjndsajlknfdsa",
        None,
    );
}

#[test]
fn haxe_metadata_26995() {
    am("f", ":Foo", Some(":^Foo"));
    am("f", ":foo", Some(":^foo"));
}

#[test]
fn separator_only_match_79558() {
    am(".", "foo.bar", Some("foo^.bar"));
}

#[test]
fn cannot_set_property_26511() {
    let word = "a".repeat(122);
    let pattern = "a".repeat(119);
    let (p, w) = (utf16(&pattern), utf16(&word));
    let _ = fuzzy_score(&p, &utf16_lower(&p), 0, &w, &utf16_lower(&w), 0, FuzzyScoreOptions::default());
}

fn pp(n: usize) -> Opts {
    Opts {
        pattern_pos: n,
        ..Opts::default()
    }
}

#[test]
fn sort_text_26096() {
    assert_matches("  ", "  group", None, fuzzy_score, pp(2));
    assert_matches("  g", "  group", Some("  ^group"), fuzzy_score, pp(2));
    am("g", "  group", Some("  ^group"));
    am("g g", "  groupGroup", None);
    am("g g", "  group Group", Some("  ^group^ ^Group"));
    assert_matches(" g g", "  group Group", Some("  ^group^ ^Group"), fuzzy_score, pp(1));
    am("zz", "zzGroup", Some("^z^zGroup"));
    am("zzg", "zzGroup", Some("^z^z^Group"));
    am("g", "zzGroup", Some("zz^Group"));
}

#[test]
fn pattern_pos_79815() {
    assert_matches("p", "prop", Some("^prop"), fuzzy_score, pp(0));
    assert_matches(":p", "prop", Some("^prop"), fuzzy_score, pp(1));
    assert_matches(":p", "prop", None, fuzzy_score, pp(2));
    assert_matches(
        ":p",
        "proP",
        Some("pro^P"),
        fuzzy_score,
        Opts {
            pattern_pos: 1,
            word_pos: 1,
            ..Opts::default()
        },
    );
    assert_matches(
        ":p",
        "aprop",
        Some("a^prop"),
        fuzzy_score,
        Opts {
            pattern_pos: 1,
            first_match_can_be_weak: true,
            ..Opts::default()
        },
    );
    assert_matches(":p", "aprop", None, fuzzy_score, pp(1));
}

#[test]
fn top_score_fuzzy_score() {
    let f = fuzzy_score;
    assert_top_score(f, "cons", 2, &["ArrayBufferConstructor", "Console", "console"]);
    assert_top_score(f, "Foo", 1, &["foo", "Foo", "foo"]);
    assert_top_score(f, "onMess", 1, &["onmessage", "onMessage", "onThisMegaEscape"]);
    assert_top_score(f, "CC", 1, &["camelCase", "CamelCase"]);
    assert_top_score(f, "cC", 0, &["camelCase", "CamelCase"]);
    assert_top_score(f, "p", 4, &["parse", "posix", "pafdsa", "path", "p"]);
    assert_top_score(f, "pa", 0, &["parse", "pafdsa", "path"]);
    assert_top_score(
        f,
        "log",
        3,
        &["HTMLOptGroupElement", "ScrollLogicalPosition", "SVGFEMorphologyElement", "log", "logger"],
    );
    assert_top_score(f, "e", 2, &["AbstractWorker", "ActiveXObject", "else"]);
    assert_top_score(
        f,
        "workbench.sideb",
        1,
        &["workbench.editor.defaultSideBySideLayout", "workbench.sideBar.location"],
    );
    assert_top_score(
        f,
        "editor.r",
        2,
        &[
            "diffEditor.renderSideBySide",
            "editor.overviewRulerlanes",
            "editor.renderControlCharacter",
            "editor.renderWhitespace",
        ],
    );
    assert_top_score(f, "-mo", 1, &["-ms-ime-mode", "-moz-columns"]);
    assert_top_score(
        f,
        "convertModelPosition",
        0,
        &["convertModelPositionToViewPosition", "convertViewToModelPosition"],
    );
    assert_top_score(f, "is", 0, &["isValidViewletId", "import statement"]);
    assert_top_score(f, "title", 1, &["files.trimTrailingWhitespace", "window.title"]);
    assert_top_score(f, "const", 1, &["constructor", "const", "cuOnstrul"]);
}

#[test]
fn unexpected_suggestion_scoring_28791() {
    assert_top_score(fuzzy_score, "_lines", 1, &["_lineStarts", "_lines"]);
    assert_top_score(fuzzy_score, "_lines", 1, &["_lineS", "_lines"]);
    assert_top_score(fuzzy_score, "_lineS", 0, &["_lineS", "_lines"]);
}

#[test]
fn html_closing_tag_38880() {
    assert_matches("\t\t<", "\t\t</body>", Some("^\t^\t^</body>"), fuzzy_score, pp(0));
    assert_matches("\t\t<", "\t\t</body>", Some("\t\t^</body>"), fuzzy_score, pp(2));
    assert_matches("\t<", "\t</body>", Some("\t^</body>"), fuzzy_score, pp(1));
}

#[test]
fn fuzzy_score_graceful_test() {
    let d = Opts::default;
    assert_matches("rlut", "result", None, fuzzy_score, d());
    assert_matches("rlut", "result", Some("^res^u^l^t"), fuzzy_score_graceful, d());
    assert_matches("cno", "console", Some("^co^ns^ole"), fuzzy_score, d());
    assert_matches("cno", "console", Some("^co^ns^ole"), fuzzy_score_graceful, d());
    assert_matches("cno", "console", Some("^c^o^nsole"), fuzzy_score_graceful_aggressive, d());
    assert_matches("cno", "co_new", Some("^c^o_^new"), fuzzy_score_graceful, d());
    assert_matches("cno", "co_new", Some("^c^o_^new"), fuzzy_score_graceful_aggressive, d());
}

#[test]
fn list_highlight_filter_66923() {
    am(
        "foo",
        "barbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo",
        Some("barbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_^f^o^o"),
    );
}

#[test]
fn truncated_filter_text_74133() {
    am(
        "foo",
        "ffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo",
        Some("ffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_^f^o^o"),
    );
    am(
        "Aoo",
        "Affffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo",
        Some("^Affffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_f^o^o"),
    );
    am(
        "foo",
        "Gffffffffffffffffffffffffffffbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbarbar_foo",
        None,
    );
}

#[test]
fn go_to_symbol_exact_method_name_84787() {
    let m = fuzzy_score(
        &utf16(":get"),
        &utf16(":get"),
        1,
        &utf16("get"),
        &utf16("get"),
        0,
        FuzzyScoreOptions {
            first_match_can_be_weak: true,
            boost_full_match: true,
        },
    );
    assert!(m.is_some());
}

#[test]
fn wrong_highlight_after_emoji_113404() {
    am("di", "\u{2728}div classname=\"\"></div>", Some("\u{2728}^d^iv classname=\"\"></div>"));
    am("di", "adiv classname=\"\"></div>", Some("adiv classname=\"\"></^d^iv>"));
}

#[test]
fn suggestion_not_highlighted_85826() {
    am("SemanticTokens", "SemanticTokensEdits", Some("^S^e^m^a^n^t^i^c^T^o^k^e^n^sEdits"));
    assert_matches(
        "SemanticTokens",
        "SemanticTokensEdits",
        Some("^S^e^m^a^n^t^i^c^T^o^k^e^n^sEdits"),
        fuzzy_score_graceful_aggressive,
        Opts::default(),
    );
}

#[test]
fn highlighting_text_in_front_of_cursor_115250() {
    am("lo", "log", Some("^l^og"));
    assert_matches(".lo", "log", Some("^l^og"), any_scorer, Opts::default());
    assert_matches(".", "log", Some("log"), any_scorer, Opts::default());
}

#[test]
fn any_score_no_strong_first_match() {
    assert_matches("bar", "foobAr", Some("foo^b^A^r"), any_scorer, Opts::default());
    assert_matches("bar", "foobar", Some("foo^b^a^r"), any_scorer, Opts::default());
}

#[test]
fn configurable_full_match_boost() {
    let score = |p: &str, w: &str, ws: usize, boost: bool| {
        let (pu, wu) = (utf16(p), utf16(w));
        fuzzy_score(
            &pu,
            &pu,
            0,
            &wu,
            &utf16_lower(&wu),
            ws,
            FuzzyScoreOptions {
                first_match_can_be_weak: true,
                boost_full_match: boost,
            },
        )
        .unwrap()
        .score
    };
    let prefix = "create";
    let a = "createModelServices";
    let b = "create";
    assert!(score(prefix, a, 0, true) < score(prefix, b, 0, true));
    let wp = "$(symbol-function) ";
    let n = utf16(wp).len();
    assert!(score(prefix, &format!("{wp}{a}"), n, true) < score(prefix, &format!("{wp}{b}"), n, true));
    assert_eq!(score(prefix, a, 0, false), score(prefix, b, 0, false));
}

#[test]
fn whole_word_match_147423() {
    am("i", "machine/{id}", Some("machine/{^id}"));
    am("ok", "obobobf{ok}/user", Some("^obobobf{o^k}/user"));
}
