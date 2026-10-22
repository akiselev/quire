//! Document and view behaviour: versions, grouping, host mode, dirty, diagnostics, tokens,
//! rebasing, refusals, settled probe.

use quire_core::syntax::{Token, TokenRuns, TokenStyle, TokenType};
use quire_core::*;

fn doc(s: &str) -> (Document, EditorView) {
    let d = Document::new(s, EditorConfig::default());
    let v = EditorView::new(&d);
    (d, v)
}

fn typ(d: &mut Document, v: &mut EditorView, s: &str, t: u64) -> Applied {
    d.execute(v, &EditorCommand::Type { text: s.into() }, Some(t))
        .unwrap()
        .applied
        .unwrap()
}

#[test]
fn versions_bump_only_on_text_change() {
    let (mut d, mut v) = doc("abc");
    let a = d
        .execute(
            &mut v,
            &EditorCommand::CursorMove {
                to: Motion::Right,
                select: false,
            },
            None,
        )
        .unwrap()
        .applied
        .unwrap();
    assert_eq!(a.version, Version(0));
    assert!(a.selection_changed);
    assert_eq!(v.selection_version(), 1);
    typ(&mut d, &mut v, "x", 0);
    assert_eq!(d.version(), Version(1));
}

#[test]
fn typing_groups_by_word_and_pause() {
    let (mut d, mut v) = doc("");
    for (i, c) in "abc d".chars().enumerate() {
        typ(&mut d, &mut v, &c.to_string(), i as u64 * 100);
    }
    // "abc" and " d" are two groups (word boundary rule)
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "abc");
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "");
    // pause > 1 s closes
    let (mut d, mut v) = doc("");
    typ(&mut d, &mut v, "a", 0);
    typ(&mut d, &mut v, "b", 2000);
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "a");
}

#[test]
fn cursor_jump_and_focus_close_groups() {
    let (mut d, mut v) = doc("0123456789");
    v.set_selection(&d, Selection::point(10));
    typ(&mut d, &mut v, "a", 0);
    v.set_selection(&d, Selection::point(0));
    typ(&mut d, &mut v, "b", 10);
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "0123456789a");
    // focus change
    let (mut d, mut v) = doc("");
    typ(&mut d, &mut v, "a", 0);
    let closed = d.close_undo_group();
    assert!(closed.is_some());
    typ(&mut d, &mut v, "b", 10);
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "a");
}

#[test]
fn host_mode_emits_groups_and_requests() {
    let (d, mut v) = doc("");
    let mut d = d.with_undo_mode(UndoMode::Host);
    typ(&mut d, &mut v, "a", 0);
    typ(&mut d, &mut v, "b", 10);
    assert!(!d.is_settled(), "an open group in host mode is pending");
    let a = d
        .execute(
            &mut v,
            &EditorCommand::Paste {
                text: "XY".into(),
                per_cursor: false,
            },
            Some(20),
        )
        .unwrap()
        .applied
        .unwrap();
    // paste closes "ab" and is its own (closed) group
    assert_eq!(a.closed.len(), 2);
    assert_eq!(a.closed[0].from, Version(0));
    assert_eq!(a.closed[0].to, Version(2));
    assert!(d.is_settled());
    let e = d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(e.request, Some(HostRequest::Undo));
    // the host answers with the inverse
    let inv = a.closed[1].inverse.clone();
    let tr = Transaction::new(d.version(), inv)
        .with_origin(Origin::Undo)
        .with_selection(a.closed[1].selection_before.clone());
    d.apply(tr, &mut v).unwrap();
    assert_eq!(d.text().to_string(), "ab");
    // tick closes after a pause
    typ(&mut d, &mut v, "c", 5000);
    assert!(d.tick(5500).is_none());
    assert!(d.tick(7000).is_some());
}

#[test]
fn dirty_and_saved() {
    let (mut d, mut v) = doc("x");
    assert!(!d.is_dirty());
    typ(&mut d, &mut v, "a", 0);
    assert!(d.is_dirty());
    d.mark_saved(d.version());
    assert!(!d.is_dirty());
    typ(&mut d, &mut v, "b", 5000);
    assert!(d.is_dirty());
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert!(!d.is_dirty(), "undo back to the saved text is clean");
}

#[test]
fn diagnostics_map_forward_and_refuse_stale() {
    let cfg = EditorConfig {
        recent_changes_window: 2,
        ..EditorConfig::default()
    };
    let mut d = Document::new("let x = 1;", cfg);
    let mut v = EditorView::new(&d);
    let at = d.version();
    typ(&mut d, &mut v, "// ", 0); // inserted at 0
    let src = SourceId::new("svelters");
    d.set_diagnostics(
        src.clone(),
        at,
        vec![Diagnostic::new(4..5, Severity::Error, "x")],
    )
    .unwrap();
    assert_eq!(d.diagnostics().of(&src)[0].range, 7..8);
    // later edits keep mapping stored diagnostics
    v.set_selection(&d, Selection::point(0));
    typ(&mut d, &mut v, "a", 5000);
    assert_eq!(d.diagnostics().of(&src)[0].range, 8..9);
    typ(&mut d, &mut v, "b", 9000);
    typ(&mut d, &mut v, "c", 13000);
    let err = d.set_diagnostics(src.clone(), at, vec![]).unwrap_err();
    assert_eq!(err.at, at);
    // replacing atomically
    d.set_diagnostics(src.clone(), d.version(), vec![]).unwrap();
    assert!(d.diagnostics().is_empty());
}

#[test]
fn tokens_shift_with_edits_and_stale_runs_map() {
    let (mut d, mut v) = doc("let a\nlet b");
    let kw = TokenStyle::of(TokenType::Keyword);
    let at = d.version();
    v.set_selection(&d, Selection::point(0));
    typ(&mut d, &mut v, "\n", 0);
    // runs computed at the old version for line 1 ("let b") land on line 2 now
    d.set_tokens(
        at,
        TokenRuns {
            first_line: 1,
            lines: vec![vec![Token::new(0, kw)]],
        },
    )
    .unwrap();
    assert_eq!(d.tokens().style_at(2, 1), kw);
    assert_eq!(d.tokens().len_lines(), 3);
}

#[test]
fn stale_base_transactions_rebase() {
    let (mut d, mut v) = doc("abc");
    let base = d.version();
    let mut other = EditorView::new(&d);
    // another view inserts at the start
    d.apply(
        Transaction::new(d.version(), ChangeSet::insert_at(3, 0, "XX").unwrap()),
        &mut other,
    )
    .unwrap();
    // a transaction computed at the old base (insert at end) is rebased
    let tr = Transaction::new(base, ChangeSet::insert_at(3, 3, "!").unwrap())
        .with_selection(Selection::point(4));
    d.apply(tr, &mut v).unwrap();
    assert_eq!(d.text().to_string(), "XXabc!");
    assert_eq!(v.selection().primary().head, 6);
}

#[test]
fn refusals_leave_state_untouched() {
    let (mut d, mut v) = doc("aé");
    let before = d.version();
    assert!(matches!(
        d.apply(
            Transaction::new(d.version(), ChangeSet::insert_at(3, 2, "x").unwrap()),
            &mut v
        ),
        Err(Refusal::NotCharBoundary(2))
    ));
    assert!(matches!(
        d.apply(
            Transaction::new(d.version(), ChangeSet::insert_at(3, 0, "\r\n").unwrap()),
            &mut v
        ),
        Err(Refusal::CarriageReturn)
    ));
    assert!(matches!(
        d.apply(
            Transaction::new(d.version(), ChangeSet::insert_at(2, 0, "x").unwrap()),
            &mut v
        ),
        Err(Refusal::LengthMismatch { .. })
    ));
    assert!(matches!(
        d.apply(
            Transaction::select(d.version(), 3, Selection::point(2)),
            &mut v
        ),
        Err(Refusal::NotCharBoundary(2))
    ));
    let mut cfg = d.config().clone();
    cfg.read_only = true;
    d.set_config(cfg);
    assert!(matches!(typ_res(&mut d, &mut v), Err(Refusal::ReadOnly)));
    assert!(
        d.apply_remote(
            d.version(),
            ChangeSet::insert_at(3, 0, "r").unwrap(),
            Origin::Remote
        )
        .is_ok()
    );
    assert_eq!(before, Version(0));
}

fn typ_res(d: &mut Document, v: &mut EditorView) -> Result<Executed, Refusal> {
    d.execute(v, &EditorCommand::Type { text: "x".into() }, None)
}

#[test]
fn two_views_keep_separate_carets() {
    let (mut d, mut a) = doc("hello world");
    let mut b = EditorView::new(&d);
    b.set_selection(&d, Selection::point(11));
    a.set_selection(&d, Selection::point(0));
    typ(&mut d, &mut a, ">> ", 0);
    assert_eq!(a.selection().primary().head, 3);
    assert_eq!(b.sync(&d), SyncOutcome::Mapped);
    assert_eq!(b.selection().primary().head, 14);
    typ(&mut d, &mut b, "!", 10);
    a.sync(&d);
    assert_eq!(a.selection().primary().head, 3);
    assert_eq!(d.text().to_string(), ">> hello world!");
}

#[test]
fn reload_keeps_carets() {
    let (mut d, mut v) = doc("one\ntwo\nthree\n");
    v.set_selection(&d, Selection::point(10)); // "th|ree"
    let new = Text::from_file("one\n2\nthree\n");
    let c = ChangeSet::diff(d.text(), &new);
    d.apply_remote(d.version(), c, Origin::Reload).unwrap();
    v.sync(&d);
    assert_eq!(d.text().to_string(), "one\n2\nthree\n");
    assert_eq!(
        d.text().byte_to_line_col(v.selection().primary().head),
        (2, 2)
    );
}

#[test]
fn settled_probe_counts_pending_work() {
    let (d, _) = doc("");
    let probe = d.settled_probe();
    let t = d.pending_work().begin();
    assert!(!probe.is_settled());
    drop(t);
    assert!(probe.is_settled());
}

#[test]
fn undo_restores_selection_and_skips_remote() {
    let (mut d, mut v) = doc("abc");
    v.set_selection(&d, Selection::point(3));
    typ(&mut d, &mut v, "d", 0);
    d.apply_remote(
        d.version(),
        ChangeSet::insert_at(4, 0, "R").unwrap(),
        Origin::Remote,
    )
    .unwrap();
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "Rabc");
    assert_eq!(v.selection().primary().head, 4);
    d.execute(&mut v, &EditorCommand::Redo, None).unwrap();
    assert_eq!(d.text().to_string(), "Rabcd");
}

#[test]
fn language_rules_from_vscode_json() {
    // The shape of a VS Code language-configuration.json (regex strings, JavaScript syntax).
    let cfg: EditorConfig = serde_json::from_str(
        r#"{
            "onEnterRules": [{
                "beforeText": "^\\s*\\/\\*\\*(?!\\/)([^\\*]|\\*(?!\\/))*$",
                "afterText": "^\\s*\\*\\/$",
                "action": { "indentAction": "indentOutdent", "appendText": " * " }
            }],
            "indentationRules": {
                "increaseIndentPattern": "^.*\\{[^}\"']*$",
                "decreaseIndentPattern": "^\\s*\\}"
            }
        }"#,
    )
    .expect("config");
    assert_eq!(
        cfg.on_enter_rules[0].action.indent,
        IndentAction::IndentOutdent
    );
    let mut d = Document::new("/** */\nfn a() {\n    x;\n    ", cfg);
    let mut v = EditorView::new(&d);
    // Enter between `/**` and ` */`.
    v.set_selection(&d, Selection::point(3));
    typ(&mut d, &mut v, "\n", 0);
    assert_eq!(d.text().line(1).to_string(), " * ");
    // `}` on the last line outdents it (decreaseIndentPattern).
    let end = d.text().len_bytes();
    v.set_selection(&d, Selection::point(end));
    typ(&mut d, &mut v, "}", 10_000);
    assert_eq!(d.text().line(d.text().len_lines() - 1).to_string(), "}");
}

/// Language rules may come from extensions, so a catastrophic rule regex must not hang the editor:
/// a match that runs out of backtracking is no match (`lang` module doc).
#[test]
fn catastrophic_rule_regex_is_no_match() {
    let line = format!("{}b", "a".repeat(30));
    for pat in [
        r"^(a+)+$",
        r"^(a+)+(?=$)",
        r"^(?=a)(a+)+\1?$",
        r"^(a|a)+(?!x)$",
        r"^(a|aa)+\1?$",
    ] {
        let cfg: EditorConfig = serde_json::from_value(serde_json::json!({
            "brackets": [["{", "}"]],
            "onEnterRules": [{
                "beforeText": pat,
                "action": { "indentAction": "indent", "appendText": "X" }
            }],
            "indentationRules": { "increaseIndentPattern": pat, "decreaseIndentPattern": pat }
        }))
        .expect("config");
        let mut d = Document::new(&line, cfg);
        let mut v = EditorView::new(&d);
        let end = d.text().len_bytes();
        v.set_selection(&d, Selection::point(end));
        let t = std::time::Instant::now();
        typ(&mut d, &mut v, "\n", 0);
        let enter = t.elapsed();
        // No rule matched: the new line has no indentation and no appended text.
        assert_eq!(d.text().line(1).to_string(), "", "{pat}");
        // An electric character re-tests the decrease pattern on the line.
        v.set_selection(&d, Selection::point(end));
        let t = std::time::Instant::now();
        typ(&mut d, &mut v, "}", 10_000);
        let electric = t.elapsed();
        assert_eq!(d.text().line(0).to_string(), format!("{line}}}"), "{pat}");
        eprintln!("{pat}: enter {enter:?}, electric {electric:?}");
        assert!(
            enter.as_millis() < 50 && electric.as_millis() < 50,
            "{pat}: {enter:?} {electric:?}"
        );
    }
}

#[test]
fn indentation_to_spaces_sets_insert_spaces() {
    let cfg = EditorConfig {
        insert_spaces: false,
        tab_size: 2,
        ..Default::default()
    };
    let mut d = Document::new("\tx\n\t\ty", cfg);
    let mut v = EditorView::new(&d);
    let ex = d
        .execute(&mut v, &EditorCommand::IndentationToSpaces, None)
        .unwrap();
    assert_eq!(d.text().to_string(), "  x\n    y");
    assert!(d.config().insert_spaces);
    assert!(ex.applied.unwrap().config_changed);
    // Undo restores the text, not the option (VS Code's updateOptions).
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "\tx\n\t\ty");
    assert!(d.config().insert_spaces);
    // Nothing to convert: the option still changes.
    let mut d = Document::new("a", d.config().clone());
    let mut v = EditorView::new(&d);
    let ex = d
        .execute(&mut v, &EditorCommand::IndentationToTabs, None)
        .unwrap();
    assert!(!d.config().insert_spaces);
    let a = ex.applied.unwrap();
    assert!(a.config_changed && !a.changed_text());
    // A read-only document refuses it.
    let mut d = Document::new(
        "a",
        EditorConfig {
            read_only: true,
            ..Default::default()
        },
    );
    let mut v = EditorView::new(&d);
    assert!(
        d.execute(&mut v, &EditorCommand::IndentationToTabs, None)
            .is_err()
    );
    assert!(d.config().insert_spaces);
}

#[test]
fn toggle_word_wrap_flips_config() {
    let mut d = Document::new("abc\ndef", EditorConfig::default());
    let mut v = EditorView::new(&d);
    typ(&mut d, &mut v, "x", 0);
    let before = d.text().to_string();
    let ex = d
        .execute(&mut v, &EditorCommand::ToggleWordWrap, None)
        .unwrap();
    assert_eq!(d.config().word_wrap, WordWrap::On);
    let a = ex.applied.unwrap();
    assert!(a.config_changed && !a.changed_text());
    assert_eq!(d.text().to_string(), before);
    // Undo reverts the typing, not the option, and the toggle left no undo stop of its own.
    d.execute(&mut v, &EditorCommand::Undo, None).unwrap();
    assert_eq!(d.text().to_string(), "abc\ndef");
    assert_eq!(d.config().word_wrap, WordWrap::On);
    d.execute(&mut v, &EditorCommand::ToggleWordWrap, None)
        .unwrap();
    assert_eq!(d.config().word_wrap, WordWrap::Off);
    assert_eq!(
        EditorCommand::from_vscode_id("editor.action.toggleWordWrap", 10),
        Some(EditorCommand::ToggleWordWrap)
    );
    assert_eq!(
        EditorCommand::ToggleWordWrap.id(),
        "editor.action.toggleWordWrap"
    );
    // A read-only document allows it (VS Code: no precondition; it is a view option).
    let mut d = Document::new(
        "a",
        EditorConfig {
            read_only: true,
            ..Default::default()
        },
    );
    let mut v = EditorView::new(&d);
    let ex = d
        .execute(&mut v, &EditorCommand::ToggleWordWrap, None)
        .unwrap();
    assert!(ex.applied.unwrap().config_changed);
    assert_eq!(d.config().word_wrap, WordWrap::On);
}

fn reindent(
    text: &str,
    rules: serde_json::Value,
    caret: (usize, usize),
) -> (String, (usize, usize)) {
    let mut cfg: EditorConfig =
        serde_json::from_value(serde_json::json!({ "indentationRules": rules })).unwrap();
    cfg.tab_size = 4;
    cfg.indent_size = 4;
    let mut d = Document::new(text, cfg);
    let mut v = EditorView::new(&d);
    let p = d.text().line_col_to_byte(caret.0, caret.1);
    v.set_selection(&d, Selection::point(p));
    d.execute(&mut v, &EditorCommand::ReindentLines, None)
        .unwrap();
    let head = v.selection().primary().head;
    (d.text().to_string(), d.text().byte_to_line_col(head))
}

#[test]
fn reindent_lines_follows_indentation_rules() {
    let braces = serde_json::json!({
        "increaseIndentPattern": "^.*\\{[^}\"']*$",
        "decreaseIndentPattern": "^\\s*\\}",
        "indentNextLinePattern": "^\\s*(if|while)\\s*\\(.*\\)\\s*$",
        "unIndentedLinePattern": "^\\s*#"
    });
    // Blocks, an outdenting `}`, a line indented only by `indentNextLinePattern`, and an
    // unindented line that keeps its own indentation without affecting the next line.
    let (t, caret) = reindent(
        "function f() {\nx;\n  if (a) {\ny;\n}\nif (b)\nz;\n#pragma\nw;\n        }",
        braces.clone(),
        (1, 0),
    );
    assert_eq!(
        t,
        "function f() {\n    x;\n    if (a) {\n        y;\n    }\n    if (b)\n        z;\n    #pragma\n    w;\n}"
    );
    // The caret before `x` moves after the inserted indentation (`replaceMove`).
    assert_eq!(caret, (1, 4));
    // The first line is the base and is not re-indented; levels snap to indent stops
    // (`ShiftCommand.shiftIndent`).
    let (t, _) = reindent("  a {\nb\n}", braces.clone(), (0, 0));
    assert_eq!(t, "  a {\n    b\n}");
    // Without indentation rules nothing changes.
    let mut d = Document::new("a {\nb\n}", EditorConfig::default());
    let mut v = EditorView::new(&d);
    let ex = d
        .execute(&mut v, &EditorCommand::ReindentLines, None)
        .unwrap();
    assert!(ex.applied.is_none());
}
