//! Field rules: single-line breaks, the character limit (code points), secret and read-only
//! fields, PRIMARY, and the two-view sync law.

use proptest::prelude::*;
use quire_text::{
    ChangeSet, DocRules, EditCommand, EditRules, HostRequest, InputKind, LineBreaks, LineMode,
    NoWrap, PrimaryRule, Refusal, Rejected, Selection, TextDocument, TextView, Transaction,
    UndoRules, WordRules,
};
use std::sync::Arc;

fn field(text: &str, rules: DocRules) -> (TextDocument, TextView) {
    let doc = TextDocument::new(text, Arc::new(rules));
    let mut view = TextView::new(&doc);
    view.set_selection(&doc, Selection::point(doc.text().len_bytes()));
    (doc, view)
}

fn paste(t: &str) -> EditCommand {
    EditCommand::Insert {
        text: t.into(),
        kind: InputKind::Paste,
        distribute: false,
    }
}

#[test]
fn single_line_breaks() {
    for (breaks, want) in [
        (LineBreaks::Strip, "abcd"),
        (LineBreaks::Spaces, "ab c d"),
        (LineBreaks::Keep, "ab\nc\nd"),
    ] {
        let (mut doc, mut view) = field(
            "ab",
            DocRules::field(LineMode::Single { breaks }, UndoRules::Gtk),
        );
        let rules = EditRules::field(WordRules::Pango);
        doc.execute(
            &mut view,
            paste("\nc\r\nd"),
            &rules,
            &NoWrap::default(),
            None,
        );
        assert_eq!(doc.text().to_string(), want, "{breaks:?}");
    }
    // apply refuses a local break in a stripping single-line document; foreign changes apply
    let (mut doc, mut view) = field("ab", DocRules::field(LineMode::SINGLE, UndoRules::Gtk));
    let tr: Transaction =
        Transaction::new(doc.version(), ChangeSet::insert_at(2, 2, "\n").unwrap());
    assert_eq!(doc.apply(tr, &mut view), Err(Refusal::LineBreak));
}

#[test]
fn max_chars_counts_code_points() {
    let mut rules = DocRules::field(LineMode::SINGLE, UndoRules::Gtk);
    rules.max_chars = Some(4);
    let (mut doc, mut view) = field("ä", rules);
    let edit = EditRules::field(WordRules::Pango);
    // "ä" is 2 bytes, 1 code point: three more fit, the rest is cut (GTK truncates)
    let ex = doc.execute(&mut view, paste("😀bcd"), &edit, &NoWrap::default(), None);
    assert_eq!(doc.text().to_string(), "ä😀bc");
    assert_eq!(ex.rejected, Some(Rejected::MaxChars));
    let ex = doc.execute(&mut view, paste("x"), &edit, &NoWrap::default(), None);
    assert_eq!(ex.rejected, Some(Rejected::MaxChars));
    assert!(ex.applied.is_none());
}

#[test]
fn secret_fields_never_copy() {
    let (mut doc, mut view) = field(
        "pass word",
        DocRules::field(LineMode::SINGLE, UndoRules::Gtk),
    );
    let mut edit = EditRules::field(WordRules::Pango);
    edit.secret = true;
    edit.primary = PrimaryRule::SetAndClear;
    let layout = NoWrap::default();
    let ex = doc.execute(&mut view, EditCommand::SelectAll, &edit, &layout, None);
    assert!(
        ex.requests.is_empty(),
        "no SetPrimary in a secret field: {:?}",
        ex.requests
    );
    for cmd in [EditCommand::Copy, EditCommand::Cut] {
        let ex = doc.execute(&mut view, cmd, &edit, &layout, None);
        assert_eq!(ex.rejected, Some(Rejected::Secret));
        assert!(ex.requests.is_empty());
    }
    assert_eq!(doc.text().to_string(), "pass word");
    // word motions go to the ends
    let ex = doc.execute(
        &mut view,
        EditCommand::Move {
            unit: quire_text::Unit::Word(quire_text::WordStop::Native),
            dir: quire_text::Dir::Back,
            extend: false,
        },
        &edit,
        &layout,
        None,
    );
    assert!(ex.applied.is_some());
    assert_eq!(view.selection().primary().head, 0);
}

#[test]
fn primary_follows_the_selection() {
    let (mut doc, mut view) = field("hello", DocRules::field(LineMode::SINGLE, UndoRules::Gtk));
    let mut edit = EditRules::field(WordRules::Pango);
    edit.primary = PrimaryRule::SetAndClear;
    let layout = NoWrap::default();
    let ex = doc.execute(&mut view, EditCommand::SelectAll, &edit, &layout, None);
    assert_eq!(
        ex.requests.as_slice(),
        [HostRequest::SetPrimary("hello".into())]
    );
    let ex = doc.execute(
        &mut view,
        EditCommand::SetSelection(Selection::point(2)),
        &edit,
        &layout,
        None,
    );
    assert_eq!(ex.requests.as_slice(), [HostRequest::ClearPrimary]);
}

#[test]
fn read_only_allows_motion_and_copy() {
    let mut rules = DocRules::field(LineMode::SINGLE, UndoRules::Gtk);
    rules.read_only = true;
    let (mut doc, mut view) = field("text", rules);
    let edit = EditRules::field(WordRules::Pango);
    let layout = NoWrap::default();
    let ex = doc.execute(&mut view, paste("x"), &edit, &layout, None);
    assert_eq!(ex.rejected, Some(Rejected::ReadOnly));
    doc.execute(&mut view, EditCommand::SelectAll, &edit, &layout, None);
    let ex = doc.execute(&mut view, EditCommand::Copy, &edit, &layout, None);
    assert_eq!(
        ex.requests.as_slice(),
        [HostRequest::SetClipboard("text".into())]
    );
    assert_eq!(doc.text().to_string(), "text");
}

proptest! {
    /// Two views of one document, edits through either: both selections stay valid after every
    /// step (SEAM-CHANGES 6).
    #[test]
    fn two_views_stay_valid(steps in proptest::collection::vec((any::<bool>(), 0usize..40, 0usize..4, "[a-z\n]{0,3}"), 1..40)) {
        let doc_rules = Arc::new(DocRules { recent_window: 4, ..DocRules::default() });
        let mut doc = TextDocument::new("hello\nworld", doc_rules);
        let mut a = TextView::new(&doc);
        let mut b = TextView::new(&doc);
        let edit = EditRules::default();
        for (first, at, del, ins) in steps {
            let v = if first { &mut a } else { &mut b };
            v.sync(&doc);
            let len = doc.text().len_bytes();
            let p = doc.text().clamp_to_boundary(at.min(len));
            let q = doc.text().clamp_to_boundary((p + del).min(len));
            v.set_selection(&doc, Selection::single(p, q));
            doc.execute(v, paste(&ins), &edit, &NoWrap::default(), None);
            for v in [&mut a, &mut b] {
                v.sync(&doc);
                prop_assert!(v.selection().is_valid_in(doc.text()));
            }
        }
    }
}
