//! VS Code's snippet session and controller tests (`src/vs/editor/contrib/snippet/test/browser/
//! snippetSession.test.ts`, `snippetController2.test.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated where they are model-level, through
//! quire's snippet mode on `EditorView` (`completion::snippet_transaction`, the
//! `jumpTo*SnippetPlaceholder` and `leaveSnippet` commands).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::completion::snippet_transaction;
use quire_core::{Document, EditorCommand as C, EditorConfig, EditorView, SelRange, Selection};

struct Ed {
    doc: Document,
    view: EditorView,
}

impl Ed {
    fn new(text: &str) -> Self {
        let doc = Document::new(text, EditorConfig::default());
        let view = EditorView::new(&doc);
        Ed { doc, view }
    }

    /// The SnippetSession suites' fixture: two cursors.
    fn session_fixture() -> Self {
        let mut e = Ed::new("function foo() {\n    console.log(a);\n}");
        e.set_selections(&[(1, 1, 1, 1), (2, 5, 2, 5)]);
        e
    }

    fn pos(&self, l: usize, c: usize) -> usize {
        self.doc.text().line_to_byte(l - 1) + c - 1
    }

    /// VS Code `Selection(selStartLine, selStartCol, posLine, posCol)`, primary first.
    fn set_selections(&mut self, s: &[(usize, usize, usize, usize)]) {
        let ranges: Vec<SelRange> = s
            .iter()
            .map(|&(a, b, c, d)| SelRange::new(self.pos(a, b), self.pos(c, d)))
            .collect();
        let sel = Selection::new(ranges, 0);
        self.view.set_selection(&self.doc, sel);
    }

    fn insert(&mut self, template: &str) {
        let tr = snippet_transaction(&self.doc, &self.view, template).unwrap();
        self.doc.apply(tr, &mut self.view).unwrap();
    }

    fn exec(&mut self, c: C) {
        self.doc.execute(&mut self.view, &c, None).unwrap();
    }

    fn typ(&mut self, t: &str) {
        self.exec(C::Type { text: t.into() });
    }

    fn next(&mut self) {
        self.exec(C::JumpToNextSnippetPlaceholder);
    }

    fn prev(&mut self) {
        self.exec(C::JumpToPrevSnippetPlaceholder);
    }

    fn value(&self) -> String {
        self.doc.text().to_string()
    }

    /// Selections in document order, as VS Code `(anchorLine, anchorCol, headLine, headCol)`.
    fn assert_selections(&self, want: &[(usize, usize, usize, usize)]) {
        let t = self.doc.text();
        let lc = |b: usize| {
            let (l, c) = t.byte_to_line_col(b);
            (l + 1, c + 1)
        };
        let got: Vec<(usize, usize, usize, usize)> = self
            .view
            .selection()
            .ranges()
            .iter()
            .map(|r| {
                let (a, b) = lc(r.anchor);
                let (c, d) = lc(r.head);
                (a, b, c, d)
            })
            .collect();
        let mut want = want.to_vec();
        want.sort();
        let mut got_sorted = got.clone();
        got_sorted.sort();
        assert_eq!(got_sorted, want);
    }

    fn keys(&self) -> (bool, bool, bool) {
        match self.view.snippet() {
            Some(s) => (true, s.has_prev(), s.has_next()),
            None => (false, false, false),
        }
    }
}

// ---- SnippetSession ----

#[test]
fn normalize_whitespace() {
    let doc = Document::new(
        "function foo() {\n    console.log(a);\n}",
        EditorConfig::default(),
    );
    let check = |line: usize, col: usize, input: &str, expected: &str| {
        let pos = doc.text().line_to_byte(line - 1) + col - 1;
        let mut s = quire_core::completion::snippet::Snippet::parse(input, false, false);
        quire_core::completion::insert::adjust_snippet_whitespace(&doc, pos, &mut s);
        assert_eq!(s.to_textmate_string(), expected, "{input}");
    };
    check(1, 1, "foo", "foo");
    check(1, 1, "foo\rbar", "foo\nbar");
    check(1, 1, "foo\rbar", "foo\nbar");
    check(2, 5, "foo\r\tbar", "foo\n        bar");
    check(2, 3, "foo\r\tbar", "foo\n    bar");
    check(2, 5, "foo\r\tbar\nfoo", "foo\n        bar\n    foo");
    check(2, 5, "a\nb${1|foo,\nbar|}", "a\n    b${1|foo,\nbar|}");
}

#[test]
fn text_edits_and_selection() {
    let mut e = Ed::session_fixture();
    e.insert("foo${1:bar}foo$0");
    assert_eq!(
        e.value(),
        "foobarfoofunction foo() {\n    foobarfooconsole.log(a);\n}"
    );
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    e.next();
    e.assert_selections(&[(1, 10, 1, 10), (2, 14, 2, 14)]);
}

#[test]
fn text_edit_with_reversed_selection() {
    let mut e = Ed::session_fixture();
    e.set_selections(&[(2, 5, 2, 5), (1, 1, 1, 1)]);
    e.insert("${1:bar}$0");
    assert_eq!(e.value(), "barfunction foo() {\n    barconsole.log(a);\n}");
    e.assert_selections(&[(2, 5, 2, 8), (1, 1, 1, 4)]);
    assert_eq!(
        e.view.selection().primary().head,
        e.pos(2, 8),
        "the primary cursor stays primary"
    );
}

#[test]
fn repeated_tabstops() {
    let mut e = Ed::session_fixture();
    e.insert("${1:abc}foo${1:abc}$0");
    e.assert_selections(&[(1, 1, 1, 4), (1, 7, 1, 10), (2, 5, 2, 8), (2, 11, 2, 14)]);
    e.next();
    e.assert_selections(&[(1, 10, 1, 10), (2, 14, 2, 14)]);
}

#[test]
fn just_text() {
    let mut e = Ed::session_fixture();
    e.insert("foobar");
    assert_eq!(
        e.value(),
        "foobarfunction foo() {\n    foobarconsole.log(a);\n}"
    );
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
}

#[test]
fn selections_and_new_text_with_newlines() {
    let mut e = Ed::session_fixture();
    e.insert("foo\n\t${1:bar}\n$0");
    assert_eq!(
        e.value(),
        "foo\n    bar\nfunction foo() {\n    foo\n        bar\n    console.log(a);\n}"
    );
    e.assert_selections(&[(2, 5, 2, 8), (5, 9, 5, 12)]);
    e.next();
    e.assert_selections(&[(3, 1, 3, 1), (6, 5, 6, 5)]);
}

#[test]
fn selections_next_prev() {
    let mut e = Ed::session_fixture();
    e.insert("f$1oo${2:bar}foo$0");
    e.assert_selections(&[(1, 2, 1, 2), (2, 6, 2, 6)]);
    e.next();
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    e.prev();
    e.assert_selections(&[(1, 2, 1, 2), (2, 6, 2, 6)]);
    e.next();
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    e.next();
    e.assert_selections(&[(1, 10, 1, 10), (2, 14, 2, 14)]);
}

#[test]
fn selections_and_typing() {
    let mut e = Ed::session_fixture();
    e.insert("f${1:oo}_$2_$0");
    e.typ("X");
    e.next();
    e.typ("bar");
    e.prev();
    e.assert_selections(&[(1, 2, 1, 3), (2, 6, 2, 7)]);
    e.next();
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    e.next();
    assert_eq!(
        e.value(),
        "fX_bar_function foo() {\n    fX_bar_console.log(a);\n}"
    );
    e.assert_selections(&[(1, 8, 1, 8), (2, 12, 2, 12)]);
}

#[test]
fn insert_shorter_snippet_into_non_empty_selection() {
    let mut e = Ed::new("foo_bar_foo");
    e.set_selections(&[(1, 1, 1, 4), (1, 9, 1, 12)]);
    e.insert("x$0");
    assert_eq!(e.value(), "x_bar_x");
    e.assert_selections(&[(1, 2, 1, 2), (1, 8, 1, 8)]);
}

#[test]
fn insert_longer_snippet_into_non_empty_selection() {
    let mut e = Ed::new("foo_bar_foo");
    e.set_selections(&[(1, 1, 1, 4), (1, 9, 1, 12)]);
    e.insert("LONGER$0");
    assert_eq!(e.value(), "LONGER_bar_LONGER");
    e.assert_selections(&[(1, 7, 1, 7), (1, 18, 1, 18)]);
}

#[test]
fn dont_merge_touching_tabstops_2() {
    let mut e = Ed::session_fixture();
    e.insert("$1$2$3$0");
    e.assert_selections(&[(1, 1, 1, 1), (2, 5, 2, 5)]);
    e.typ("111");
    e.next();
    e.assert_selections(&[(1, 4, 1, 4), (2, 8, 2, 8)]);
    e.typ("222");
    e.next();
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    e.typ("333");
    e.next();
    assert!(e.view.snippet().is_none(), "at the last placeholder");
}

// ---- SnippetController2 ----

fn controller_fixture() -> Ed {
    let mut e = Ed::new("if\n    $state\nfi");
    e.set_selections(&[(1, 1, 1, 1), (2, 5, 2, 5)]);
    e
}

#[test]
fn insert_tab_tab_done() {
    let mut e = controller_fixture();
    e.insert("${1:one}${2:two}$0");
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    assert_eq!(e.keys(), (true, true, true));
    e.next();
    assert_eq!(e.keys(), (false, false, false));
    e.typ("\t");
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_cursor_moves_out_left_right() {
    let mut e = controller_fixture();
    e.insert("foo${1:bar}foo$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    e.set_selections(&[(1, 12, 1, 12), (2, 16, 2, 16)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_cursor_moves_out_up_down() {
    let mut e = controller_fixture();
    e.insert("foo${1:bar}foo$0");
    e.set_selections(&[(2, 4, 2, 7), (3, 8, 3, 11)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_plain_text_no_snippet_mode() {
    let mut e = controller_fixture();
    e.insert("foobar");
    assert_eq!(e.keys(), (false, false, false));
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
}

#[test]
fn cancelling_discards_added_cursors_68512_hard() {
    let mut e = Ed::new("");
    e.insert(".REGION ${2:FUNCTION_NAME}\nCREATE.FUNCTION ${1:VOID} ${2:FUNCTION_NAME}(${3:})\n\t${4:}\nEND\n.ENDREGION$0");
    e.assert_selections(&[(2, 17, 2, 21)]);
    e.next();
    e.assert_selections(&[(1, 9, 1, 22), (2, 22, 2, 35)]);
    assert_eq!(e.keys(), (true, true, true));
    e.set_selections(&[(1, 22, 1, 22), (2, 35, 2, 35)]);
    assert_eq!(e.keys(), (true, true, true));
    e.exec(C::LeaveSnippet);
    assert_eq!(e.keys(), (false, false, false));
    e.assert_selections(&[(1, 22, 1, 22)]);
}

// ---- Nested snippets (SnippetSession.merge) ----

#[test]
fn insert_nested_trivial_snippet() {
    let mut e = controller_fixture();
    e.insert("${1:foo}bar$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 4), (2, 5, 2, 8)]);
    e.insert("FOO$0");
    e.assert_selections(&[(1, 4, 1, 4), (2, 8, 2, 8)]);
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_nested_snippet() {
    let mut e = controller_fixture();
    e.insert("${1:foobar}$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 7), (2, 5, 2, 11)]);
    e.insert("far$1boo$0");
    e.assert_selections(&[(1, 4, 1, 4), (2, 8, 2, 8)]);
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    assert_eq!(e.keys(), (true, true, true));
    e.next();
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_nested_plain_text() {
    let mut e = controller_fixture();
    e.insert("${1:foobar}$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 7), (2, 5, 2, 11)]);
    e.insert("farboo");
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    e.assert_selections(&[(1, 7, 1, 7), (2, 11, 2, 11)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn nested_without_final_placeholder_jumps_to_next_outer_27898() {
    let mut e = controller_fixture();
    e.insert("for(const ${1:element} of ${2:array}) {$0}");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 11, 1, 18), (2, 15, 2, 22)]);
    e.next();
    assert_eq!(e.keys(), (true, true, true));
    e.assert_selections(&[(1, 22, 1, 27), (2, 26, 2, 31)]);
    e.insert("document");
    assert_eq!(e.keys(), (true, true, true));
    e.assert_selections(&[(1, 30, 1, 30), (2, 34, 2, 34)]);
    e.next();
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn recursive_snippets_tab_shift_tab_27543() {
    let mut e = controller_fixture();
    e.insert("1_calize(${1:nl}, '${2:value}')$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 10, 1, 12), (2, 14, 2, 16)]);
    e.insert("2_calize(${1:nl}, '${2:value}')$0");
    e.assert_selections(&[(1, 19, 1, 21), (2, 23, 2, 25)]);
    e.next(); // inner `value`
    e.assert_selections(&[(1, 24, 1, 29), (2, 28, 2, 33)]);
    e.next(); // inner `$0`
    e.assert_selections(&[(1, 31, 1, 31), (2, 35, 2, 35)]);
    e.next(); // outer `value`
    e.assert_selections(&[(1, 34, 1, 39), (2, 38, 2, 43)]);
    e.prev(); // inner `$0`
    e.assert_selections(&[(1, 31, 1, 31), (2, 35, 2, 35)]);
}

#[test]
fn html_snippets_combine_32211() {
    let cfg = EditorConfig {
        insert_spaces: false,
        tab_size: 4,
        ..EditorConfig::default()
    };
    let doc = Document::new("", cfg);
    let view = EditorView::new(&doc);
    let mut e = Ed { doc, view };
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("\n\t\t\t<!DOCTYPE html>\n\t\t\t<html lang=\"en\">\n\t\t\t<head>\n\t\t\t\t<meta charset=\"UTF-8\">\n\t\t\t\t<meta name=\"viewport\" content=\"width=${2:device-width}, initial-scale=${3:1.0}\">\n\t\t\t\t<meta http-equiv=\"X-UA-Compatible\" content=\"${5:ie=edge}\">\n\t\t\t\t<title>${7:Document}</title>\n\t\t\t</head>\n\t\t\t<body>\n\t\t\t\t${8}\n\t\t\t</body>\n\t\t\t</html>\n\t\t");
    e.next();
    e.next();
    e.next();
    e.next();
    e.assert_selections(&[(11, 5, 11, 5)]);
    e.insert("<input type=\"${2:text}\">");
    e.assert_selections(&[(11, 18, 11, 22)]);
}

#[test]
fn nested_snippet_insertion_39594() {
    let mut e = Ed::new("");
    e.insert("$1 = ConvertTo-Json $1");
    e.assert_selections(&[(1, 1, 1, 1), (1, 19, 1, 19)]);
    e.set_selections(&[(1, 19, 1, 19)]);
    // snippet mode stops: $1 has two occurrences and one selection is left
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn nested_snippet_insertion_39594_part2() {
    // the selection-change-to-cancel logic is not too aggressive
    let mut e = Ed::new("a-\naaa-");
    e.set_selections(&[(2, 5, 2, 5), (1, 3, 1, 3)]);
    e.insert("log($1);$0");
    e.assert_selections(&[(2, 9, 2, 9), (1, 7, 1, 7)]);
    assert_eq!(e.keys(), (true, false, true));
}

#[test]
fn nested_snippets_terminating_abruptly_42012() {
    let mut e = Ed::new("");
    e.insert("var ${2:${1:name}} = ${1:name} + 1;${0}");
    e.assert_selections(&[(1, 5, 1, 9), (1, 12, 1, 16)]);
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    assert_eq!(e.keys(), (true, true, true));
}

/// SnippetSession's two sessions, one inserted inside the other: through the controller the
/// second merges into the first, with the same selections.
#[test]
fn nested_sessions() {
    let mut e = Ed::new("");
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("foo${2:bar}foo$0");
    assert_eq!(e.value(), "foobarfoo");
    e.assert_selections(&[(1, 4, 1, 7)]);
    e.insert("ba${1:zzzz}$0");
    assert_eq!(e.value(), "foobazzzzfoo");
    e.assert_selections(&[(1, 6, 1, 10)]);
    e.next();
    e.assert_selections(&[(1, 10, 1, 10)]);
    e.next();
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 13, 1, 13)]);
}

/// VS Code moves onto the final tabstop and back (`next`, `prev`) before typing; snippet
/// mode ends at the final tabstop in the controller, so that pair is left out.
#[test]
fn merge() {
    let mut e = Ed::new("function foo() {\n    console.log(a);\n}");
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("This ${1:is ${2:nested}}.$0");
    e.next();
    e.assert_selections(&[(1, 9, 1, 15)]);
    e.insert("really ${1:nested}$0");
    e.assert_selections(&[(1, 16, 1, 22)]);
    e.next();
    e.assert_selections(&[(1, 22, 1, 22)]);
    assert!(e.view.snippet().is_some_and(|s| !s.is_at_last()));
    e.typ("AAA");
    // back to `really ${1:nested}`
    e.prev();
    e.assert_selections(&[(1, 16, 1, 22)]);
    // back to `${1:is ...}`, which grew
    e.prev();
    e.assert_selections(&[(1, 6, 1, 25)]);
}

#[test]
fn deep_merge_no_phantom_cursors_279349() {
    let mut e = Ed::new("");
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("\\sin\\left(${1:x}\\right) ");
    for i in 0..25 {
        e.insert("\\sin\\left(${1:x}\\right) ");
        assert_eq!(
            e.view.selection().len(),
            1,
            "selection count after {} merges",
            i + 1
        );
        assert!(
            e.view.snippet().is_some(),
            "snippet mode after {} merges",
            i + 1
        );
    }
}

#[test]
fn merge_preserves_mirrors_in_nested_snippet() {
    let mut e = Ed::new("");
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("(${1:x})");
    e.insert("${1:y}-${1}");
    assert_eq!(e.value(), "(y-y)");
    assert_eq!(e.view.selection().len(), 2);
}

/// `$1$1` puts two empty occurrences at one position; the selection merges them, so the
/// controller leaves snippet mode at once and the second snippet is inserted on its own.
#[test]
fn merge_placeholder_occurrences_collapse() {
    let mut e = Ed::new("");
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("$1$1$0");
    e.insert("${1:nested}$0");
    assert_eq!(e.value(), "nested");
}

// ---- More controller and session tests the stop model covers ----

#[test]
fn insert_insert_cursors_collapse() {
    let mut e = controller_fixture();
    e.insert("foo${1:bar}foo$0");
    assert!(e.view.snippet().is_some());
    e.assert_selections(&[(1, 4, 1, 7), (2, 8, 2, 11)]);
    // bad selection change
    e.set_selections(&[(1, 4, 1, 7)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn insert_delete_snippet_text() {
    let mut e = controller_fixture();
    e.insert("${1:foobar}$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 7), (2, 5, 2, 11)]);
    e.exec(C::Cut);
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 1), (2, 5, 2, 5)]);
    e.typ("abc");
    assert_eq!(e.keys(), (true, false, true));
    e.next();
    assert_eq!(e.keys(), (false, false, false));
    e.exec(C::Tab);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn tab_through_deleted_tab_stops_31619() {
    let mut e = Ed::new("");
    e.insert("foo${1:a${2:bar}baz}end$0");
    e.assert_selections(&[(1, 4, 1, 11)]);
    e.exec(C::Cut);
    e.assert_selections(&[(1, 4, 1, 4)]);
    e.next();
    e.assert_selections(&[(1, 7, 1, 7)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn placeholders_order_58267() {
    let mut e = Ed::new("");
    e.insert("\\pth{$1}$0");
    e.assert_selections(&[(1, 6, 1, 6)]);
    assert_eq!(e.keys(), (true, false, true));
    e.insert("\\itv{${1:left}}{${2:right}}{${3:left_value}}{${4:right_value}}$0");
    e.assert_selections(&[(1, 11, 1, 15)]);
    e.next();
    e.assert_selections(&[(1, 17, 1, 22)]);
    e.next();
    e.assert_selections(&[(1, 24, 1, 34)]);
    e.next();
    e.assert_selections(&[(1, 36, 1, 47)]);
    e.next();
    e.assert_selections(&[(1, 48, 1, 48)]);
    e.next();
    e.assert_selections(&[(1, 49, 1, 49)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn user_defined_tab_stops_72862() {
    let mut e = Ed::new("");
    e.insert("export default $1");
    assert_eq!(e.keys(), (true, false, true));
}

#[test]
fn optional_tabstop_72358() {
    let mut e = Ed::new("");
    e.insert("${1:prop: {$2\\},}\nmore$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 1, 1, 10)]);
    e.exec(C::Cut);
    e.assert_selections(&[(1, 1, 1, 1)]);
    e.next();
    e.assert_selections(&[(2, 5, 2, 5)]);
    assert_eq!(e.keys(), (false, false, false));
}

#[test]
fn tabstop_selecting_previously_entered_variable_23728() {
    let mut e = Ed::new("");
    e.insert("import ${2:${1:module}} from '${1:module}'$0");
    assert_eq!(e.keys(), (true, false, true));
    e.assert_selections(&[(1, 8, 1, 14), (1, 21, 1, 27)]);
    e.insert("foo");
    e.assert_selections(&[(1, 11, 1, 11), (1, 21, 1, 21)]);
    e.next(); // ${2:...}
    e.assert_selections(&[(1, 8, 1, 11)]);
}

/// VS Code moves onto the final tabstop with `session.next()` and checks
/// `isAtLastPlaceholder`; in the controller, snippet mode ends there.
#[test]
fn overwriting_nested_placeholder() {
    let mut e = Ed::session_fixture();
    e.insert("log(${1:\"$2\"});$0");
    e.assert_selections(&[(1, 5, 1, 7), (2, 9, 2, 11)]);
    e.typ("XXX");
    assert_eq!(
        e.value(),
        "log(XXX);function foo() {\n    log(XXX);console.log(a);\n}"
    );
    e.next();
    assert!(e.view.snippet().is_some_and(|s| !s.is_at_last()));
    e.next();
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 10, 1, 10), (2, 14, 2, 14)]);
}

#[test]
fn typing_with_nested_placeholder() {
    let mut e = Ed::session_fixture();
    e.set_selections(&[(1, 1, 1, 1)]);
    e.insert("This ${1:is ${2:nested}}.$0");
    e.assert_selections(&[(1, 6, 1, 15)]);
    e.next();
    e.assert_selections(&[(1, 9, 1, 15)]);
    e.exec(C::Cut);
    e.assert_selections(&[(1, 9, 1, 9)]);
    e.typ("XXX");
    e.prev();
    e.assert_selections(&[(1, 6, 1, 12)]);
}

/// `isSelectionWithinPlaceholders` through the controller: a selection outside the
/// placeholders ends snippet mode, so each probe starts a fresh insert.
#[test]
fn selections_and_snippet_ranges() {
    let template = "${1:foo}farboo${2:bar}$0";
    let probe = |sels: &[(usize, usize, usize, usize)]| {
        let mut e = Ed::session_fixture();
        e.insert(template);
        e.set_selections(sels);
        e.view.snippet().is_some()
    };
    let mut e = Ed::session_fixture();
    e.insert(template);
    assert_eq!(
        e.value(),
        "foofarboobarfunction foo() {\n    foofarboobarconsole.log(a);\n}"
    );
    e.assert_selections(&[(1, 1, 1, 4), (2, 5, 2, 8)]);
    assert!(e.view.snippet().is_some());
    assert!(!probe(&[(1, 1, 1, 1)]));
    assert!(
        !probe(&[(1, 6, 1, 6), (2, 10, 2, 10)]),
        "in snippet, outside placeholder"
    );
    assert!(
        !probe(&[(1, 6, 1, 6), (2, 10, 2, 10), (1, 1, 1, 1)]),
        "in snippet, outside placeholder"
    );
    assert!(!probe(&[(1, 6, 1, 6), (2, 10, 2, 10), (2, 20, 2, 21)]));
    e.next();
    assert!(e.view.snippet().is_some());
    e.assert_selections(&[(1, 10, 1, 13), (2, 14, 2, 17)]);
    e.next();
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 13, 1, 13), (2, 17, 2, 17)]);
}

#[test]
fn transform() {
    let mut e = Ed::new("");
    e.insert("${1/foo/bar/}$0");
    e.assert_selections(&[(1, 1, 1, 1)]);
    e.typ("foo");
    e.next();
    assert_eq!(e.value(), "bar");
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 4, 1, 4)]);
}

#[test]
fn multi_placeholder_same_index_one_transform() {
    let mut e = Ed::new("");
    e.insert("$1 baz ${1/foo/bar/}$0");
    e.assert_selections(&[(1, 1, 1, 1), (1, 6, 1, 6)]);
    e.typ("foo");
    e.next();
    assert_eq!(e.value(), "foo baz bar");
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 12, 1, 12)]);
}

#[test]
fn transform_adjacent_to_previous_placeholder_96545() {
    let mut e = Ed::new("");
    e.insert("${1:{}${2:fff}${1/{/}/}");
    e.assert_selections(&[(1, 1, 1, 2), (1, 5, 1, 6)]);
    e.next();
    assert_eq!(e.value(), "{fff}");
    e.assert_selections(&[(1, 2, 1, 5)]);
    e.typ("ggg");
    e.next();
    assert_eq!(e.value(), "{ggg}");
    assert!(e.view.snippet().is_none(), "at the last placeholder");
    e.assert_selections(&[(1, 6, 1, 6)]);
}

#[test]
fn tab_stop_selection_96545() {
    let mut e = Ed::new("");
    e.insert("${1:{}${2:fff}${1/[\\{]/}/}$0");
    assert_eq!(e.value(), "{fff{");
    e.assert_selections(&[(1, 1, 1, 2), (1, 5, 1, 6)]);
    e.next();
    e.assert_selections(&[(1, 2, 1, 5)]);
}

/// The merges go through the controller (`insert` in snippet mode).
#[test]
fn placeholder_index_after_snippets_in_a_row_30769() {
    let mut e = Ed::new("");
    e.insert("test ${1:replaceme}");
    e.typ("1");
    e.typ("\n");
    assert_eq!(e.value(), "test 1\n");
    for n in 2..=4 {
        e.insert("test ${1:replaceme}");
        e.typ(&n.to_string());
        e.typ("\n");
    }
    assert_eq!(e.value(), "test 1\ntest 2\ntest 3\ntest 4\n");
}
