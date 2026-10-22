//! `AuthorId`: who made a change, through `Applied`, `RecentChanges` and undo groups.

use quire_core::*;

fn doc(s: &str) -> (Document, EditorView) {
    let d = Document::new(s, EditorConfig::default());
    let v = EditorView::new(&d);
    (d, v)
}

fn type_as(d: &mut Document, v: &mut EditorView, s: &str, t: u64, a: AuthorId) -> Applied {
    d.execute_as(v, &EditorCommand::Type { text: s.into() }, Some(t), a)
        .unwrap()
        .applied
        .unwrap()
}

const HOST: AuthorId = AuthorId(7);

#[test]
fn author_round_trips_through_applied_and_recent_changes() {
    let (mut d, mut v) = doc("");
    assert_eq!(
        Transaction::new(d.version(), ChangeSet::identity(0)).author,
        AuthorId::USER
    );
    assert_eq!(AuthorId::default(), AuthorId::USER);
    let a = type_as(&mut d, &mut v, "a", 0, AuthorId::USER);
    assert_eq!(a.author, AuthorId::USER);
    let b = type_as(&mut d, &mut v, "b", 10, HOST);
    assert_eq!(b.author, HOST);
    let len = d.text().len_bytes();
    let tr = Transaction::new(
        d.version(),
        ChangeSet::replace(len, [(len..len, "c")]).unwrap(),
    )
    .with_author(AuthorId(9));
    assert_eq!(d.apply(tr, &mut v).unwrap().author, AuthorId(9));
    let len = d.text().len_bytes();
    let r = d
        .apply_remote_by(
            d.version(),
            ChangeSet::replace(len, [(0..0, "r")]).unwrap(),
            Origin::Remote,
            AuthorId(42),
        )
        .unwrap();
    assert_eq!(r.author, AuthorId(42));
    let authors: Vec<AuthorId> = d
        .recent()
        .since(Version(0))
        .unwrap()
        .map(|c| c.author)
        .collect();
    assert_eq!(authors, [AuthorId::USER, HOST, AuthorId(9), AuthorId(42)]);
}

#[test]
fn an_author_change_closes_the_undo_group() {
    let (mut d, mut v) = doc("");
    // Typing within the pause by one author coalesces; another author's typing does not join.
    type_as(&mut d, &mut v, "a", 0, AuthorId::USER);
    type_as(&mut d, &mut v, "b", 10, AuthorId::USER);
    let c = type_as(&mut d, &mut v, "c", 20, HOST);
    assert_eq!(c.closed.len(), 1, "the user's group closed");
    assert_eq!(c.closed[0].author, AuthorId::USER);
    type_as(&mut d, &mut v, "d", 30, HOST);
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "ab", "the host's group undone alone");
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "");
}

#[test]
fn undo_and_redo_keep_the_group_author() {
    let (mut d, mut v) = doc("");
    type_as(&mut d, &mut v, "host text", 0, HOST);
    // The user undoes the host's edit: the undo is still the host's.
    let u = d
        .execute_as(&mut v, &EditorCommand::Undo, None, AuthorId::USER)
        .unwrap()
        .applied
        .unwrap();
    assert_eq!(d.text().to_string(), "");
    assert_eq!(u.author, HOST);
    assert_eq!(d.history().redo_stack().last().unwrap().author, HOST);
    let r = d
        .execute_as(&mut v, &EditorCommand::Redo, None, AuthorId(3))
        .unwrap()
        .applied
        .unwrap();
    assert_eq!(d.text().to_string(), "host text");
    assert_eq!(r.author, HOST);
    assert_eq!(d.history().undo_stack().last().unwrap().author, HOST);
}
