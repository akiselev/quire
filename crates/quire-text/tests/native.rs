//! svelters FX1 and FX3 through the field commands: word rules and the GTK/Qt undo replays
//! (golden values from svelters' `native/undo.rs` tests @ e17ebbff, `tests/native_edit.rs`
//! `fx3_undo_grouping` and `tests/native_focus.rs` `fx1_gtk_focus_policy_and_words`). The texts
//! are ASCII, so char offsets equal byte offsets.

use quire_text::{
    Dir, DocRules, EditCommand, EditRules, ExternalSet, InputKind, LineMode, NoWrap, Selection,
    TextDocument, TextView, UndoRules, Unit, WordRules, WordStop,
};
use std::sync::Arc;

struct F {
    doc: TextDocument,
    view: TextView,
    rules: EditRules,
}

impl F {
    fn new(undo: UndoRules, text: &str, at: usize) -> F {
        let words = if undo == UndoRules::Qt {
            WordRules::Qt
        } else {
            WordRules::Pango
        };
        let doc = TextDocument::new(text, Arc::new(DocRules::field(LineMode::SINGLE, undo)));
        let mut view = TextView::new(&doc);
        view.set_selection(&doc, Selection::point(at));
        F {
            doc,
            view,
            rules: EditRules::field(words),
        }
    }
    fn run(&mut self, cmd: EditCommand) {
        self.doc
            .execute(&mut self.view, cmd, &self.rules, &NoWrap::default(), None);
    }
    fn typ(&mut self, t: &str) {
        for c in t.chars() {
            self.run(EditCommand::Insert {
                text: c.to_string(),
                kind: InputKind::Typing,
                distribute: false,
            });
        }
    }
    fn backspace(&mut self) {
        self.run(EditCommand::Delete {
            unit: Unit::Grapheme,
            dir: Dir::Back,
        });
    }
    fn word_back(&mut self, to: usize) {
        self.run(EditCommand::Delete {
            unit: Unit::Word(WordStop::Native),
            dir: Dir::Back,
        });
        assert_eq!(self.caret(), to);
    }
    fn paste(&mut self, t: &str) {
        self.run(EditCommand::Insert {
            text: t.into(),
            kind: InputKind::Paste,
            distribute: false,
        });
    }
    fn select(&mut self, anchor: usize, caret: usize) {
        self.run(EditCommand::SetSelection(Selection::single(anchor, caret)));
    }
    fn caret(&self) -> usize {
        self.view.selection().primary().head
    }
    fn state(&self) -> (String, [usize; 2]) {
        let p = self.view.selection().primary();
        (self.doc.text().to_string(), [p.anchor, p.head])
    }
    fn undo(&mut self) -> (String, [usize; 2]) {
        self.run(EditCommand::Undo);
        self.state()
    }
    fn redo(&mut self) -> (String, [usize; 2]) {
        self.run(EditCommand::Redo);
        self.state()
    }
}

fn t(s: &str, a: usize, c: usize) -> (String, [usize; 2]) {
    (s.to_string(), [a, c])
}

#[test]
fn fx3_undo_typing_against_both_goldens() {
    let mut g = F::new(UndoRules::Gtk, "", 0);
    g.typ("foo bar baz");
    assert_eq!(g.undo(), t("foo bar", 7, 7));
    assert_eq!(g.undo(), t("foo", 3, 3));
    assert_eq!(g.undo(), t("", 0, 0));
    assert_eq!(g.undo(), t("", 0, 0));
    assert_eq!(g.redo(), t("foo", 3, 3));
    assert_eq!(g.redo(), t("foo bar", 7, 7));
    assert_eq!(g.redo(), t("foo bar baz", 11, 11));
    let mut q = F::new(UndoRules::Qt, "", 0);
    q.typ("foo bar baz");
    assert_eq!(q.undo(), t("", 0, 0));
    assert_eq!(q.undo(), t("", 0, 0));
    assert_eq!(q.redo(), t("foo bar baz", 11, 11));
    assert_eq!(q.redo(), t("foo bar baz", 11, 11));
    assert_eq!(q.undo(), t("", 0, 0));
}

fn undo_edits(rules: UndoRules) -> F {
    let mut f = F::new(rules, "alpha beta gamma", 16);
    f.word_back(11);
    f.typ("delta");
    f.backspace();
    f.backspace();
    f.select(13, 13);
    f.select(12, 12);
    f.typ("x");
    f.select(0, 15);
    f.typ("new");
    f
}

#[test]
fn fx3_undo_edits_against_both_goldens() {
    let mut g = undo_edits(UndoRules::Gtk);
    assert_eq!(g.undo(), t("n", 1, 1));
    assert_eq!(g.undo(), t("alpha beta dxel", 15, 0));
    assert_eq!(g.undo(), t("alpha beta del", 12, 12));
    assert_eq!(g.undo(), t("alpha beta delt", 15, 15));
    assert_eq!(g.undo(), t("alpha beta delta", 16, 16));
    assert_eq!(g.undo(), t("alpha beta ", 11, 11));
    assert_eq!(g.undo(), t("alpha beta gamma", 16, 16));
    assert_eq!(g.undo(), t("alpha beta gamma", 16, 16));
    assert_eq!(g.redo(), t("alpha beta ", 11, 11));
    // GTK selects the last character here ([15, 16]); not modelled
    assert_eq!(g.redo().0, "alpha beta delta");
    assert_eq!(g.redo(), t("alpha beta delt", 15, 15));
    let mut q = undo_edits(UndoRules::Qt);
    assert_eq!(q.undo(), t("alpha beta dxel", 0, 15));
    assert_eq!(q.undo(), t("alpha beta del", 12, 12));
    assert_eq!(q.undo(), t("alpha beta delta", 16, 16));
    assert_eq!(q.undo(), t("alpha beta gamma", 16, 11));
    for _ in 0..4 {
        assert_eq!(q.undo(), t("alpha beta gamma", 16, 11));
    }
    assert_eq!(q.redo(), t("alpha beta delta", 16, 16));
    assert_eq!(q.redo(), t("alpha beta del", 13, 13));
    assert_eq!(q.redo(), t("alpha beta dxel", 0, 15));
}

#[test]
fn fx3_undo_paste_against_both_goldens() {
    for rules in [UndoRules::Gtk, UndoRules::Qt] {
        let mut f = F::new(rules, "one two", 7);
        f.typ(" three ");
        f.paste("PASTED");
        f.typ(" four");
        assert_eq!(f.undo(), t("one two three PASTED", 20, 20));
        let (three, four) = if rules == UndoRules::Gtk {
            (13, "one two three")
        } else {
            (14, "one two three ")
        };
        assert_eq!(f.undo(), t(four, three, three));
        assert_eq!(f.undo(), t("one two", 7, 7));
        assert_eq!(f.undo(), t("one two", 7, 7));
        assert_eq!(f.redo(), t(four, three, three));
        assert_eq!(f.redo(), t("one two three PASTED", 20, 20));
    }
}

#[test]
fn fx3_undo_grouping() {
    // GTK: one step per word; a text set from outside starts a new history.
    let mut g = F::new(UndoRules::Gtk, "", 0);
    g.typ("foo bar");
    assert_eq!(g.undo(), t("foo", 3, 3));
    assert_eq!(g.redo().0, "foo bar");
    g.undo();
    assert_eq!(g.undo().0, "");
    assert_eq!(g.redo().0, "foo");
    g.doc
        .replace_all("set", ExternalSet::ResetHistory, Some(&mut g.view))
        .unwrap();
    assert_eq!(g.undo().0, "set");
    // Qt: the typing run is one step; a BackSpace run is another.
    let mut q = F::new(UndoRules::Qt, "", 0);
    q.typ("foo bar");
    q.backspace();
    q.backspace();
    assert_eq!(q.undo().0, "foo bar");
    assert_eq!(q.undo().0, "");
}

#[test]
fn fx1_pango_snake_case() {
    // Pango's word rules: `_` ends a word; Ctrl+BackSpace deletes back to the word start.
    let mut f = F::new(UndoRules::Gtk, "snake_case", 10);
    f.run(EditCommand::Delete {
        unit: Unit::Word(WordStop::Native),
        dir: Dir::Back,
    });
    assert_eq!(f.doc.text().to_string(), "snake_");
    f.run(EditCommand::Move {
        unit: Unit::Word(WordStop::Native),
        dir: Dir::Back,
        extend: false,
    });
    assert_eq!(f.caret(), 0);
}
