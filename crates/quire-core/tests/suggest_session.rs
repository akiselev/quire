//! The suggest session: VS Code's model-level suggestModel tests
//! (`src/vs/editor/contrib/suggest/test/browser/suggestModel.test.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`, translated; a synchronous host answers every
//! request at once, so VS Code's "sync then async" events show as the final state) and
//! quire's own tests of accepting, the list and the host seam.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::completion::{
    AcceptSuggestionOnEnter, CompletionItem, CompletionItemKind as K, CompletionList,
    CompletionRange, CompletionRefusal, CompletionRequest, CompletionSource,
    CompletionTriggerKind as T, InsertMode, SuggestOutput, WidgetState,
};
use quire_core::{
    ChangeSet, Document, EditorCommand as C, EditorConfig, EditorView, Origin, SelRange,
    Selection, SourceId, Transaction,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

type ProvideFn = dyn Fn(&Document, usize, &T) -> Option<(Vec<CompletionItem>, bool)>;

struct Provider {
    id: SourceId,
    triggers: Vec<String>,
    f: Box<ProvideFn>,
    count: Rc<Cell<usize>>,
}

struct Host {
    doc: Document,
    view: EditorView,
    providers: Vec<Provider>,
    requests: Vec<CompletionRequest>,
    auto_answer: bool,
}

/// `(line, col)` 1-based to a byte offset.
fn at(doc: &Document, line: usize, col: usize) -> usize {
    doc.text().line_to_byte(line - 1) + col - 1
}

/// VS Code `getWordUntilPosition` with the default word separators, as byte columns.
fn word_until(doc: &Document, pos: usize) -> (String, usize) {
    let text = doc.text();
    let (line, col) = text.byte_to_line_col(pos);
    let l = text.line(line).to_string();
    let seps = &doc.config().word_separators;
    let mut start = col;
    for (i, c) in l[..col].char_indices().rev() {
        if c.is_whitespace() || seps.contains(c) {
            break;
        }
        start = i;
    }
    (l[start..col].to_string(), text.line_to_byte(line) + start)
}

/// `getDefaultSuggestRange`: the word before the position.
fn word_range(doc: &Document, pos: usize) -> Range<usize> {
    word_until(doc, pos).1..pos
}

/// From the line start to the position (`Range.fromPositions(pos.with(undefined, 1), pos)`).
fn line_start_range(doc: &Document, pos: usize) -> Range<usize> {
    let l = doc.text().byte_to_line(pos);
    doc.text().line_to_byte(l)..pos
}

fn ci(label: &str, kind: K, insert: &str, range: Range<usize>) -> CompletionItem {
    let mut c = CompletionItem::new(label, kind, range);
    c.insert_text = insert.to_string();
    c
}

impl Host {
    fn new(text: &str) -> Self {
        let doc = Document::new(text, EditorConfig::default());
        let view = EditorView::new(&doc);
        Host {
            doc,
            view,
            providers: Vec::new(),
            requests: Vec::new(),
            auto_answer: true,
        }
    }

    fn register(
        &mut self,
        triggers: &[&str],
        f: impl Fn(&Document, usize, &T) -> Option<(Vec<CompletionItem>, bool)> + 'static,
    ) -> Rc<Cell<usize>> {
        let count = Rc::new(Cell::new(0));
        let id = SourceId::new(&format!("p{}", self.providers.len()));
        self.providers.push(Provider {
            id,
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            f: Box::new(f),
            count: count.clone(),
        });
        let sources = self
            .providers
            .iter()
            .map(|p| CompletionSource {
                id: p.id.clone(),
                trigger_characters: p.triggers.clone(),
            })
            .collect();
        self.doc.set_completion_sources(sources);
        count
    }

    /// Answers every request at once (the host's providers).
    fn pump(&mut self) {
        loop {
            let outs = self.view.take_suggest_outputs();
            if outs.is_empty() || !self.auto_answer {
                for o in outs {
                    if let SuggestOutput::Request(r) = o {
                        self.requests.push(r);
                    }
                }
                return;
            }
            for o in outs {
                let SuggestOutput::Request(r) = o else { continue };
                self.requests.push(r.clone());
                let mut lists = Vec::new();
                for p in &self.providers {
                    if r.sources.as_ref().is_some_and(|s| !s.contains(&p.id)) {
                        continue;
                    }
                    p.count.set(p.count.get() + 1);
                    if let Some((items, incomplete)) = (p.f)(&self.doc, r.pos, &r.trigger) {
                        lists.push(CompletionList {
                            source: p.id.clone(),
                            items,
                            incomplete,
                        });
                    }
                }
                let _ = self.view.set_completions(&self.doc, r.id, r.version, lists);
            }
        }
    }

    fn exec(&mut self, cmd: C) {
        self.doc.execute(&mut self.view, &cmd, None).unwrap();
        self.pump();
    }

    fn type_text(&mut self, s: &str) {
        for ch in s.chars() {
            self.exec(C::Type { text: ch.to_string() });
        }
    }

    fn set_value(&mut self, s: &str) {
        let len = self.doc.text().len_bytes();
        let cs = ChangeSet::replace(len, [(0..len, s)]).unwrap();
        self.doc
            .apply(Transaction::new(self.doc.version(), cs), &mut self.view)
            .unwrap();
        self.pump();
    }

    fn set_selection(&mut self, a: (usize, usize), b: (usize, usize)) {
        let sel = Selection::single(at(&self.doc, a.0, a.1), at(&self.doc, b.0, b.1));
        self.exec(C::SetSelection { selection: sel });
    }

    fn set_pos(&mut self, line: usize, col: usize) {
        self.set_selection((line, col), (line, col));
    }

    fn trigger(&mut self, auto: bool) {
        self.view.trigger_suggest(&self.doc, auto);
        self.pump();
    }

    fn state(&self) -> WidgetState {
        self.view.suggest().widget_state()
    }

    fn labels(&self) -> Vec<String> {
        self.view
            .suggest()
            .items()
            .iter()
            .map(|f| f.item.label().to_string())
            .collect()
    }

    fn auto(&self) -> bool {
        self.view.suggest().session().unwrap().auto
    }

    fn active(&self) -> bool {
        self.view.suggest().session().is_some()
    }

    fn text(&self) -> String {
        self.doc.text().to_string()
    }
}

fn always_something(doc: &Document, pos: usize, _t: &T) -> Option<(Vec<CompletionItem>, bool)> {
    let (w, _) = word_until(doc, pos);
    Some((vec![ci(&w, K::Property, "foofoo", word_range(doc, pos))], false))
}

fn always_empty(_: &Document, _: usize, _: &T) -> Option<(Vec<CompletionItem>, bool)> {
    Some((Vec::new(), false))
}

// ---- SuggestModel - Context ----

#[test]
fn context_should_auto_trigger() {
    let doc = Document::new(
        "Das Pferd frisst keinen Gurkensalat - Philipp Reis 1861.\nWer hat's erfunden?",
        EditorConfig::default(),
    );
    let check = |off: usize, expected: bool, msg: &str| {
        let mut v = EditorView::new(&doc);
        v.set_selection(&doc, Selection::point(off));
        assert_eq!(quire_core::completion::should_auto_trigger(&doc, v.selection()), expected, "{msg}");
    };
    check(3, true, "end of word, Das|");
    check(4, false, "no word Das |");
    check(1, true, "typing a single character before a word: D|as");
    check(55, false, "number, 1861|");
}

// ---- SuggestModel - TriggerAndCancelOracle ----

#[test]
fn events_cancel_trigger() {
    let mut h = Host::new("abc def");
    h.auto_answer = false;
    h.trigger(true);
    assert!(h.auto());
    h.view.cancel_suggest();
    assert!(!h.active());
    h.trigger(true);
    assert!(h.auto());
    h.trigger(false);
    assert!(!h.auto());
}

#[test]
fn events_suggest_empty() {
    let mut h = Host::new("abc def");
    h.register(&[], always_empty);
    h.trigger(true);
    assert!(!h.active(), "an automatic session with no items cancels");
    h.trigger(false);
    assert_eq!(h.state(), WidgetState::Empty);
    assert!(!h.auto());
    assert_eq!(h.labels().len(), 0);
}

#[test]
fn trigger_on_type() {
    let mut h = Host::new("abc def");
    h.register(&[], always_something);
    h.set_pos(1, 4);
    h.type_text("d");
    assert!(h.auto());
    assert_eq!(h.labels(), ["abcd"]);
}

#[test]
fn keep_filtering_after_space_17400() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| {
        Some((vec![ci("My Table", K::Property, "My Table", word_range(doc, pos))], false))
    });
    h.trigger(true);
    h.set_pos(1, 1);
    h.type_text("My");
    assert!(h.auto());
    assert_eq!(h.labels(), ["My Table"]);
    h.type_text(" ");
    assert!(h.auto());
    assert_eq!(h.labels(), ["My Table"]);
}

#[test]
fn trigger_character_forces_new_session_21484() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| {
        Some((vec![ci("foo.bar", K::Property, "foo.bar", line_start_range(doc, pos))], false))
    });
    h.register(&["."], |doc, pos, _| {
        let dot = doc.text().char_before(pos) == Some('.');
        let start = if dot { pos } else { pos - 1 };
        Some((vec![ci("boom", K::Property, "boom", start..pos)], false))
    });
    h.set_pos(1, 1);
    h.type_text("foo");
    assert!(h.auto());
    assert_eq!(h.labels(), ["foo.bar"]);
    h.type_text(".");
    assert!(h.auto());
    assert_eq!(h.labels(), ["foo.bar", "boom"]);
}

#[test]
fn space_after_equal_sign_29353_1() {
    let mut h = Host::new("fo");
    h.register(&[], always_something);
    h.set_pos(1, 3);
    h.trigger(false);
    assert!(!h.auto());
    assert_eq!(h.state(), WidgetState::Open);
    assert_eq!(h.labels().len(), 1);
    h.type_text("+");
    assert!(!h.active());
}

#[test]
fn space_after_equal_sign_29353_2() {
    let mut h = Host::new("fo");
    h.register(&[], always_something);
    h.set_pos(1, 3);
    h.trigger(false);
    assert_eq!(h.state(), WidgetState::Open);
    h.type_text(" ");
    assert!(!h.active());
}

#[test]
fn incomplete_results_retrigger_28400_1() {
    let mut h = Host::new("foo");
    h.register(&[], |doc, pos, _| {
        Some((vec![ci("foo", K::Property, "foo", line_start_range(doc, pos))], true))
    });
    h.set_pos(1, 4);
    h.trigger(false);
    assert!(!h.auto());
    assert_eq!(h.labels().len(), 1);
    h.type_text(";");
    assert!(!h.active());
}

#[test]
fn incomplete_results_retrigger_28400_2() {
    let mut h = Host::new("foo");
    h.register(&[], |doc, pos, _| {
        Some((vec![ci("foo;", K::Property, "foo", line_start_range(doc, pos))], true))
    });
    h.set_pos(1, 4);
    h.trigger(false);
    assert_eq!(h.labels().len(), 1);
    h.type_text(";");
    assert!(!h.auto());
    assert_eq!(h.labels().len(), 1);
}

#[test]
fn trigger_character_in_suggest_context() {
    let mut h = Host::new("");
    let seen: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let s2 = seen.clone();
    h.register(&["."], move |doc, pos, t| {
        if *t == T::TriggerCharacter(".".into()) {
            s2.set(true);
        }
        Some((vec![ci("foo.bar", K::Property, "foo.bar", line_start_range(doc, pos))], false))
    });
    h.set_pos(1, 1);
    h.type_text("foo.");
    assert!(seen.get());
}

#[test]
fn backspace_should_not_always_cancel_36491() {
    let mut h = Host::new("abc def");
    h.register(&[], always_something);
    h.set_pos(1, 4);
    h.type_text("d");
    assert!(h.auto());
    assert_eq!(h.labels().len(), 1);
    h.exec(C::DeleteLeft);
    assert!(h.auto());
    assert_eq!(h.labels().len(), 1);
}

#[test]
fn completion_code_action_text_changes_39893() {
    let mut h = Host::new("ba; import { foo } from \"./b\"");
    h.register(&[], |_doc, pos, _| {
        let mut c = ci("bar", K::Property, "bar", pos - 2..pos);
        c.additional_text_edits = vec![(16..16, ", bar".to_string())];
        Some((vec![c], true))
    });
    h.set_pos(1, 3);
    h.trigger(false);
    assert_eq!(h.labels(), ["bar"]);
    h.exec(C::AcceptSelectedSuggestion);
    assert_eq!(h.text(), "bar; import { foo, bar } from \"./b\"");
}

#[test]
fn second_keypress_in_snippet_43523() {
    let mut h = Host::new("");
    h.register(&[], always_something);
    h.set_value("d");
    h.set_selection((1, 1), (1, 2));
    h.type_text("e");
    assert!(h.auto());
    assert_eq!(h.labels().len(), 1);
}

#[test]
fn trigger_full_when_incomplete_active_99504() {
    let mut h = Host::new("");
    let a = h.register(&[], |doc, pos, _| {
        Some((vec![ci("Z aaa", K::Class, "Z aaa", 0..pos.max(doc.text().line_to_byte(0)))], false))
    });
    let b = h.register(&[], |doc, pos, _| {
        let (w, _) = word_until(doc, pos);
        if !w.starts_with('a') {
            return None;
        }
        Some((vec![ci("aaa", K::Folder, "aaa", word_range(doc, pos))], false))
    });
    h.set_value("");
    h.set_pos(1, 1);
    h.type_text("Z");
    assert!(h.auto());
    assert_eq!(h.labels(), ["Z aaa"]);
    h.type_text(" a");
    assert!(h.auto());
    assert_eq!(h.labels(), ["Z aaa", "aaa"]);
    assert_eq!(a.get(), 1);
    assert_eq!(b.get(), 2);
}

#[test]
fn letters_as_trigger_characters_127815() {
    let mut h = Host::new("");
    h.register(&[], |_, pos, _| Some((vec![ci("AAAA", K::Class, "WordTriggerA", pos..pos)], false)));
    h.register(&["a", "."], |_, pos, _| {
        Some((vec![ci("AAAA", K::Class, "AutoTriggerA", pos..pos)], false))
    });
    h.set_value("");
    h.set_pos(1, 1);
    h.type_text(".");
    assert!(h.auto());
    assert_eq!(h.labels().len(), 1);
    h.view.cancel_suggest();
    h.set_value("");
    h.set_pos(1, 1);
    h.type_text("a");
    assert!(h.auto());
    assert_eq!(h.labels().len(), 2);
}

#[test]
fn unexpected_suggest_scoring_167242() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| Some((vec![ci("pull", K::Text, "pull", word_range(doc, pos))], false)));
    h.register(&[], |doc, pos, _| {
        Some((vec![ci("git.pull", K::Class, "git.pull", line_start_range(doc, pos))], false))
    });
    h.set_value("gi");
    h.set_pos(1, 3);
    h.type_text("t");
    assert!(h.auto());
    assert_eq!(h.labels(), ["git.pull"]);
    h.type_text(".");
    h.type_text("p");
    assert!(h.auto());
    assert_eq!(h.labels(), ["git.pull"]);
}

#[test]
fn digit_after_word_separator_169390() {
    let mut h = Host::new("");
    let a = h.register(&[], |doc, pos, _| {
        let r = line_start_range(doc, pos);
        Some((vec![ci("foo-20", K::Text, "foo-20", r.clone()), ci("foo-hello", K::Text, "foo-hello", r)], false))
    });
    let b = h.register(&["2"], |doc, pos, t| {
        if !matches!(t, T::TriggerCharacter(_)) {
            return None;
        }
        Some((vec![ci("foo-210", K::Class, "foo-210", line_start_range(doc, pos))], false))
    });
    h.set_value("foo");
    h.set_pos(1, 4);
    h.trigger(false);
    assert!(!h.auto());
    assert_eq!(h.labels(), ["foo-20", "foo-hello"]);
    h.type_text("-");
    h.type_text("2");
    assert!(h.auto());
    assert_eq!(h.labels(), ["foo-20", "foo-210"]);
    assert_eq!((a.get(), b.get()), (1, 2));
}

#[test]
fn set_refilter_flag_keep_trigger_kind() {
    let mut h = Host::new("");
    h.register(&["."], |doc, pos, _| {
        let (w, _) = word_until(doc, pos);
        let w = if w.is_empty() { "hello".to_string() } else { w };
        Some((vec![ci(&w, K::Property, "foofoo", word_range(doc, pos))], false))
    });
    h.set_value("foo");
    h.set_pos(1, 4);
    h.type_text("o");
    let s = h.view.suggest().session().unwrap();
    assert!(s.auto);
    assert_eq!(s.trigger, T::Invoke);
    assert_eq!(h.labels().len(), 1);
    h.type_text(".");
    assert_eq!(h.view.suggest().session().unwrap().trigger, T::TriggerCharacter(".".into()));
    assert_eq!(h.labels().len(), 1);
    h.type_text("h");
    assert_eq!(h.view.suggest().session().unwrap().trigger, T::TriggerCharacter(".".into()));
    assert_eq!(h.labels().len(), 1);
}

// ---- quire: the host seam, the list and accepting ----

#[test]
fn stale_and_superseded_answers_are_refused() {
    let mut h = Host::new("abc");
    h.register(&[], always_something);
    h.auto_answer = false;
    h.set_pos(1, 4);
    h.trigger(false);
    let r1 = h.requests.pop().unwrap();
    assert_eq!(r1.trigger, T::Invoke);
    assert!(!r1.auto);
    h.view.cancel_suggest();
    h.trigger(false);
    let r2 = h.requests.pop().unwrap();
    let list = |_doc: &Document| CompletionList {
        source: SourceId::new("p0"),
        items: vec![ci("abcdef", K::Text, "abcdef", 0..3)],
        incomplete: false,
    };
    assert_eq!(
        h.view.set_completions(&h.doc, r1.id, r1.version, vec![list(&h.doc)]),
        Err(CompletionRefusal::Superseded)
    );
    assert_eq!(h.state(), WidgetState::Loading);
    // the text moved on: the answer is mapped forward
    let len = h.doc.text().len_bytes();
    h.doc
        .apply(
            Transaction::new(h.doc.version(), ChangeSet::insert_at(len, 0, "xy").unwrap())
                .with_origin(Origin::Remote),
            &mut h.view,
        )
        .unwrap();
    let _ = h.view.take_suggest_outputs();
    if h.view.suggest().session().is_some() {
        h.view
            .set_completions(&h.doc, r2.id, r2.version, vec![list(&h.doc)])
            .unwrap();
    }
}

fn open_with(h: &mut Host, labels: &[&str]) {
    let owned: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
    h.register(&[], move |doc, pos, _| {
        Some((
            owned
                .iter()
                .map(|l| ci(l, K::Variable, l, word_range(doc, pos)))
                .collect(),
            false,
        ))
    });
}

#[test]
fn accept_replaces_the_typed_word_and_hides() {
    let mut h = Host::new("let x = co");
    open_with(&mut h, &["console", "const"]);
    h.set_pos(1, 11);
    h.trigger(false);
    assert_eq!(h.labels(), ["console", "const"]);
    h.exec(C::SelectNextSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(1));
    h.exec(C::AcceptSelectedSuggestion);
    assert_eq!(h.text(), "let x = const");
    assert_eq!(h.view.selection().primary(), SelRange::point(13));
    assert!(!h.active());
    h.exec(C::Undo);
    assert_eq!(h.text(), "let x = co", "accepting is one undo step");
}

#[test]
fn filter_as_you_type_and_wrap_and_page() {
    let mut h = Host::new("");
    let many: Vec<String> = (0..30).map(|i| format!("item{i:02}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    open_with(&mut h, &refs);
    h.type_text("i");
    assert_eq!(h.labels().len(), 30);
    h.exec(C::SelectPrevSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(29), "wraps to the end");
    h.exec(C::SelectNextSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(0));
    h.exec(C::SelectNextPageSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(11), "the last visible row");
    h.exec(C::SelectNextPageSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(22));
    assert_eq!(h.view.suggest().scroll_top(), 11);
    h.exec(C::SelectPrevPageSuggestion);
    assert_eq!(h.view.suggest().focused(), Some(11), "the first visible row");
    h.type_text("tem1");
    // item01, item10..item19, item21
    assert_eq!(h.labels().len(), 12);
    assert_eq!(h.view.suggest().focused(), Some(0), "refiltering focuses the first item");
    h.exec(C::HideSuggestWidget);
    assert!(!h.active());
}

#[test]
fn replace_mode_and_insert_mode() {
    for (mode, want) in [(InsertMode::Insert, "fooBarbar"), (InsertMode::Replace, "fooBar")] {
        let mut h = Host::new("foobar");
        let mut cfg = h.doc.config().clone();
        cfg.suggest.insert_mode = mode;
        h.doc.set_config(cfg);
        h.register(&[], |_, pos, _| {
            let mut c = ci("fooBar", K::Field, "fooBar", 0..pos);
            c.range = CompletionRange::InsertReplace {
                insert: 0..pos,
                replace: 0..6,
            };
            Some((vec![c], false))
        });
        h.set_pos(1, 4);
        h.trigger(false);
        h.exec(C::AcceptSelectedSuggestion);
        assert_eq!(h.text(), want, "{mode:?}");
    }
}

#[test]
fn snippet_completion_selects_first_placeholder_and_tabs() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| {
        Some((
            vec![CompletionItem::new("fn", K::Snippet, word_range(doc, pos))
                .with_snippet("fn ${1:name}(${2}) {\n\t$0\n}")],
            false,
        ))
    });
    h.type_text("f");
    h.exec(C::AcceptSelectedSuggestion);
    assert_eq!(h.text(), "fn name() {\n    \n}");
    assert_eq!(h.view.selection().primary(), SelRange::new(3, 7));
    assert!(h.view.snippet().is_some());
    h.type_text("main");
    h.exec(C::JumpToNextSnippetPlaceholder);
    assert_eq!(h.view.selection().primary(), SelRange::point(8));
    h.exec(C::JumpToPrevSnippetPlaceholder);
    assert_eq!(h.view.selection().primary(), SelRange::new(3, 7));
    h.exec(C::JumpToNextSnippetPlaceholder);
    h.exec(C::JumpToNextSnippetPlaceholder);
    assert_eq!(h.view.selection().primary(), SelRange::point(16), "the final tabstop");
    assert!(h.view.snippet().is_none(), "the session ends at $0");
}

#[test]
fn commit_character_accepts() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| {
        let mut c = ci("console", K::Variable, "console", word_range(doc, pos));
        c.commit_characters = vec![".".into()];
        Some((vec![c], false))
    });
    h.type_text("con");
    assert!(h.view.suggest().accepts_on(&h.doc, "."));
    assert!(!h.view.suggest().accepts_on(&h.doc, ";"));
}

#[test]
fn smart_accept_on_enter_makes_text_edit() {
    let mut h = Host::new("");
    let mut cfg = h.doc.config().clone();
    cfg.suggest.accept_suggestion_on_enter = AcceptSuggestionOnEnter::Smart;
    h.doc.set_config(cfg);
    open_with(&mut h, &["foo"]);
    h.type_text("fo");
    assert!(h.view.suggest().makes_text_edit(&h.doc, h.view.selection()));
    h.type_text("o");
    assert!(!h.view.suggest().makes_text_edit(&h.doc, h.view.selection()));
}

#[test]
fn item_command_retriggers_or_goes_to_the_host() {
    let mut h = Host::new("");
    h.register(&[], |doc, pos, _| {
        let mut a = ci("aa", K::Text, "aa", word_range(doc, pos));
        a.command = Some(quire_core::completion::CompletionCommand {
            id: "host.cmd".into(),
            title: "x".into(),
            args_json: "[]".into(),
        });
        Some((vec![a], false))
    });
    h.auto_answer = true;
    h.type_text("a");
    let _ = h.view.take_suggest_outputs();
    h.doc
        .execute(&mut h.view, &C::AcceptSelectedSuggestion, None)
        .unwrap();
    let outs = h.view.take_suggest_outputs();
    assert!(matches!(&outs[..], [SuggestOutput::Command(c)] if c.id == "host.cmd"));
}

#[test]
fn quick_suggestions_off() {
    let mut h = Host::new("abc def");
    h.register(&[], always_something);
    let mut cfg = h.doc.config().clone();
    cfg.suggest.quick_suggestions = quire_core::completion::QuickSuggestions {
        other: false,
        comments: false,
        strings: false,
    };
    h.doc.set_config(cfg);
    h.set_pos(1, 4);
    h.type_text("d");
    assert!(!h.active());
}
