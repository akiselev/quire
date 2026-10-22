//! VS Code's completion model tests (`src/vs/editor/contrib/suggest/test/browser/
//! completionModel.test.ts` at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`),
//! translated. Columns are shifted by 10 (VS Code's can go below 1 in these fixtures; only
//! differences matter).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::SourceId;
use quire_core::completion::{
    CompletionItem, CompletionItemKind as K, CompletionModel, LineContext, ModelOptions,
    SnippetSuggestions, SuggestItem, compare_items,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SHIFT: usize = 10;
static PROVIDER: AtomicUsize = AtomicUsize::new(0);

/// VS Code `createSuggestItem(label, overwriteBefore, kind, incomplete, position, sortText,
/// filterText)`; every item gets its own provider, as there.
fn item(
    label: &str,
    overwrite_before: usize,
    kind: K,
    incomplete: bool,
    column: usize,
    sort_text: Option<&str>,
    filter_text: Option<&str>,
) -> Arc<SuggestItem> {
    let mut c = CompletionItem::new(label, kind, 0..0);
    c.sort_text = sort_text.map(str::to_string);
    c.filter_text = filter_text.map(str::to_string);
    let pos = column + SHIFT;
    let src = SourceId::new(&format!("p{}", PROVIDER.fetch_add(1, Ordering::Relaxed)));
    Arc::new(SuggestItem::new(
        Arc::new(c),
        src,
        0,
        incomplete,
        pos,
        pos - overwrite_before,
        pos,
        pos,
        true,
    ))
}

fn it(label: &str, ob: usize) -> Arc<SuggestItem> {
    item(label, ob, K::Property, false, 1, None, None)
}

fn ctx(leading: &str, delta: isize) -> LineContext {
    LineContext {
        leading_line_content: leading.to_string(),
        character_count_delta: delta,
    }
}

fn model(items: Vec<Arc<SuggestItem>>, column: usize, c: LineContext, snippets: SnippetSuggestions) -> CompletionModel {
    CompletionModel::new(
        items,
        column + SHIFT,
        c,
        ModelOptions {
            snippet_suggestions: snippets,
            ..ModelOptions::default()
        },
    )
}

fn setup() -> CompletionModel {
    model(
        vec![it("foo", 3), it("Foo", 3), it("foo", 2)],
        1,
        ctx("foo", 0),
        SnippetSuggestions::Inline,
    )
}

fn labels(m: &mut CompletionModel) -> Vec<String> {
    m.items().iter().map(|f| f.item.label().to_string()).collect()
}

#[test]
fn filtering_cached() {
    let mut m = setup();
    let now = m.generation();
    assert_eq!(m.generation(), now);
    m.set_line_context(ctx("foo", 0));
    assert_eq!(m.generation(), now);
    m.set_line_context(ctx("foo1", 1));
    assert_ne!(m.generation(), now);
}

#[test]
fn complete_incomplete() {
    let mut m = setup();
    assert_eq!(m.incomplete_sources().len(), 0);
    let mut inc = model(
        vec![item("foo", 3, K::Property, true, 1, None, None), it("foo", 2)],
        1,
        ctx("foo", 0),
        SnippetSuggestions::Inline,
    );
    assert_eq!(inc.incomplete_sources().len(), 1);
}

#[test]
fn fuzzy_matching_of_snippets_49895() {
    let c = |l: &str, inc: bool| item(l, 1, K::Property, inc, 2, None, None);
    let mut m = model(
        vec![
            c("foobar1", false),
            c("foobar2", false),
            c("foobar3", false),
            c("foobar4", false),
            c("foobar5", false),
            c("foofoo1", true),
        ],
        2,
        ctx("f", 0),
        SnippetSuggestions::Inline,
    );
    assert_eq!(m.incomplete_sources().len(), 1);
    assert_eq!(m.items().len(), 6);
}

#[test]
fn proper_current_word_when_length_0_16380() {
    let mut m = model(
        vec![it("    </div", 4), it("a", 0), it("p", 0), it("    </tag", 4), it("    XYZ", 4)],
        1,
        ctx("   <", 0),
        SnippetSuggestions::Inline,
    );
    assert_eq!(m.items().len(), 4);
    assert_eq!(labels(&mut m), ["    </div", "    </tag", "a", "p"]);
}

#[test]
fn keep_snippet_sorting_top_25495() {
    let mut m = model(
        vec![
            item("Snippet1", 1, K::Snippet, false, 1, None, None),
            item("tnippet2", 1, K::Snippet, false, 1, None, None),
            item("semver", 1, K::Property, false, 1, None, None),
        ],
        1,
        ctx("s", 0),
        SnippetSuggestions::Top,
    );
    assert_eq!(m.items().len(), 2);
    assert_eq!(labels(&mut m), ["Snippet1", "semver"]);
    let i = m.items();
    assert!(i[0].score.score < i[1].score.score); // snippet really promoted
}

#[test]
fn keep_snippet_sorting_bottom_25495() {
    let mut m = model(
        vec![
            item("snippet1", 1, K::Snippet, false, 1, None, None),
            item("tnippet2", 1, K::Snippet, false, 1, None, None),
            item("Semver", 1, K::Property, false, 1, None, None),
        ],
        1,
        ctx("s", 0),
        SnippetSuggestions::Bottom,
    );
    assert_eq!(m.items().len(), 2);
    assert_eq!(labels(&mut m), ["Semver", "snippet1"]);
    let i = m.items();
    assert!(i[0].score.score < i[1].score.score); // snippet really demoted
}

#[test]
fn keep_snippet_sorting_inline_25495() {
    let mut m = model(
        vec![
            item("snippet1", 1, K::Snippet, false, 1, None, None),
            item("tnippet2", 1, K::Snippet, false, 1, None, None),
            it("Semver", 1),
        ],
        1,
        ctx("s", 0),
        SnippetSuggestions::Inline,
    );
    assert_eq!(m.items().len(), 2);
    assert_eq!(labels(&mut m), ["snippet1", "Semver"]);
    let i = m.items();
    assert!(i[0].score.score > i[1].score.score);
}

#[test]
fn filter_text_seems_ignored_26874() {
    let item1 = item("Map - java.util", 1, K::Property, false, 1, None, Some("Map"));
    let item2 = it("Map - java.util", 1);
    let mut m = model(vec![item1, item2], 1, ctx("M", 0), SnippetSuggestions::Inline);
    assert_eq!(m.items().len(), 2);
    m.set_line_context(ctx("Map ", 3));
    assert_eq!(m.items().len(), 1);
}

#[test]
fn sort_text_from_language_server_26096() {
    let item1 = item("<- groups", 2, K::Property, false, 3, Some("00002"), Some("  groups"));
    let item2 = item("source", 0, K::Property, false, 3, Some("00001"), Some("source"));
    let mut items = vec![item1, item2];
    items.sort_by(|a, b| compare_items(a, b, SnippetSuggestions::Inline));
    let mut m = model(items, 3, ctx("  ", 0), SnippetSuggestions::Inline);
    assert_eq!(m.items().len(), 2);
    assert_eq!(labels(&mut m), ["source", "<- groups"]);
}

#[test]
fn sorting_with_label_details_153026() {
    let zzz = item("ZZZ", 0, K::Operator, false, 1, None, None);
    let aaa = item("AAA", 0, K::Operator, false, 1, None, None);
    let iii = item("III", 0, K::Operator, false, 1, None, None);
    let mut actual = vec![zzz, aaa, iii];
    actual.sort_by(|a, b| compare_items(a, b, SnippetSuggestions::Inline));
    let l: Vec<&str> = actual.iter().map(|i| i.label()).collect();
    assert_eq!(l, ["AAA", "III", "ZZZ"]);
}

#[test]
fn score_only_filtered_items_when_typing_more() {
    let mut m = model(
        vec![it("console", 0), it("co_new", 0), it("bar", 0), it("car", 0), it("foo", 0)],
        1,
        ctx("", 0),
        SnippetSuggestions::Inline,
    );
    assert_eq!(m.items().len(), 5);
    m.set_line_context(ctx("c", 1));
    assert_eq!(m.items().len(), 3);
    m.set_line_context(ctx("cn", 2));
    assert_eq!(m.items().len(), 2);
    m.set_line_context(ctx("", 0));
    assert_eq!(m.items().len(), 5);
}

#[test]
fn more_relaxed_suggest_matching_15419() {
    let mut m = model(
        vec![it("result", 0), it("replyToUser", 0), it("randomLolut", 0), it("car", 0), it("foo", 0)],
        1,
        ctx("", 0),
        SnippetSuggestions::Inline,
    );
    m.set_line_context(ctx("rlut", 4));
    assert_eq!(m.items().len(), 3);
    assert_eq!(labels(&mut m), ["result", "replyToUser", "randomLolut"]);
}

#[test]
fn emmet_suggestion_jsx_39518() {
    let mut m = model(
        vec![
            it("from", 0),
            it("form", 0),
            it("form:get", 0),
            it("testForeignMeasure", 0),
            it("fooRoom", 0),
        ],
        1,
        ctx("", 0),
        SnippetSuggestions::Inline,
    );
    m.set_line_context(ctx("form", 4));
    assert_eq!(m.items().len(), 5);
    assert_eq!(labels(&mut m)[..3], ["form", "form:get", "from"]);
}
