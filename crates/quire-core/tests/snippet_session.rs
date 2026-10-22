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
