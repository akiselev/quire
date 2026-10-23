//! VS Code's semantic tokens tests, translated for `quire_core::semantic_tokens`:
//! `semanticTokensProviderStyling.test.ts` (`toMultilineTokens2` on bad data),
//! `documentSemanticTokens.test.ts` (`ModelSemanticColoring` through `Document`'s requests and
//! answers) and `getSemanticTokens.test.ts`, at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`; then quire's own tests of the legend styles and
//! the merge over the highlighter's tokens.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::semantic_tokens::{
    SemanticTokens, SemanticTokensEdit, SemanticTokensEdits, SemanticTokensLegend,
    SemanticTokensOutput, SemanticTokensReply, SemanticTokensRequest, SemanticTokensSource,
    SparseMultilineTokens, legend_style, to_multiline_tokens,
};
use quire_core::syntax::{Token, TokenModifiers, TokenRuns, TokenStyle, TokenType};
use quire_core::{ChangeSet, Document, EditorConfig, EditorView, SourceId, Transaction};

// ---- semanticTokensProviderStyling.test.ts ----

/// `toString` of the areas: `(line,start-end)` per token.
fn areas_string(areas: &[SparseMultilineTokens]) -> String {
    let mut s = Vec::new();
    for a in areas {
        for l in a.start_line()..=a.end_line() {
            for (st, en, _) in a.line_tokens(l).unwrap_or_default() {
                s.push(format!("({l},{st}-{en})"));
            }
        }
    }
    format!("[{}]", s.join(","))
}

/// The tests' theme: `stN` has foreground `N` (`st0`: foreground 0, which is no style).
fn st_styling(types: usize) -> impl FnMut(u32, u32) -> Option<u32> {
    move |t, _| (t > 0 && (t as usize) < types).then_some(t << 15 | 16)
}

fn expect(rows: &[(u32, u32, u32)]) -> String {
    format!(
        "[{}]",
        rows.iter()
            .map(|(l, a, b)| format!("({l},{a}-{b})"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[test]
fn invalid_semantic_tokens_handled_better_134973() {
    let data = [
        0, 13, 16, 1, 0, 1, 2, 6, 2, 0, 0, 7, 6, 3, 0, 0, 15, 8, 4, 0, 0, 17, 1, 5, 0, 0, 7, 5, 6,
        0, 1, 12, 8, 7, 0, 0, 19, 5, 8, 0, 0, 7, 1, 9, 0, 0, 4294967294, 5, 10, 0,
    ];
    let r = to_multiline_tokens(&data, st_styling(11));
    assert_eq!(
        areas_string(&r),
        expect(&[
            (1, 13, 29),
            (2, 2, 8),
            (2, 9, 15),
            (2, 24, 32),
            (2, 41, 42),
            (2, 48, 53),
            (3, 12, 20),
            (3, 31, 36),
            (3, 38, 39)
        ])
    );
}

#[test]
fn negative_values_from_the_language_service_148651() {
    let data = [
        0, 0, 3, 0, 0, 0, 4, 2, 2, 0, 0, 2, 3, 8, 0, 0, 3, 1, 9, 0, 0, 1, 1, 10, 0, 0, 1, 4, 8, 0,
        0, 4, 4294967292, 2, 0, 0, 4294967292, 4294967294, 8, 0, 0, 4294967294, 1, 9, 0, 0, 1, 1,
        10, 0, 0, 1, 3, 8, 0, 0, 3, 4294967291, 8, 0, 0, 4294967291, 1, 9, 0, 0, 1, 1, 10, 0, 0, 1,
        4, 8, 0,
    ];
    let r = to_multiline_tokens(&data, st_styling(10));
    assert_eq!(
        areas_string(&r),
        expect(&[(1, 4, 6), (1, 6, 9), (1, 9, 10), (1, 11, 15)])
    );
}

#[test]
fn bracket_pair_colorization_freeze_149130() {
    let data = [0, 11, 1, 1, 0, 0, 4, 1, 1, 0, 0, 4294967289, 1, 1, 0];
    let r = to_multiline_tokens(&data, st_styling(6));
    assert_eq!(areas_string(&r), expect(&[(1, 11, 12), (1, 15, 16)]));
}

// ---- documentSemanticTokens.test.ts / getSemanticTokens.test.ts ----

struct Host {
    doc: Document,
    view: EditorView,
    now: u64,
    released: Vec<Option<String>>,
}

fn class_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec!["class".into()],
        token_modifiers: vec![],
    }
}

impl Host {
    fn new(text: &str, sources: &[&str]) -> Self {
        let mut doc = Document::new(text, EditorConfig::default());
        doc.set_semantic_tokens_sources(
            sources
                .iter()
                .map(|s| SemanticTokensSource {
                    id: SourceId::new(s),
                    legend: class_legend(),
                })
                .collect(),
        );
        let view = EditorView::new(&doc);
        Host {
            doc,
            view,
            now: 0,
            released: Vec::new(),
        }
    }

    fn tick(&mut self, ms: u64) {
        self.now += ms;
        self.doc.tick_semantic_tokens(self.now);
    }

    fn drain(&mut self) -> Vec<SemanticTokensRequest> {
        let mut out = Vec::new();
        for o in self.doc.take_semantic_tokens_outputs() {
            match o {
                SemanticTokensOutput::Request(r) => out.push(r),
                SemanticTokensOutput::Release { result_id, .. } => self.released.push(result_id),
            }
        }
        out
    }

    /// Ticks until a request goes out (at most `ms`).
    fn wait_request(&mut self, ms: u64) -> Option<SemanticTokensRequest> {
        for _ in 0..ms / 10 {
            self.tick(10);
            if let Some(r) = self.drain().pop() {
                return Some(r);
            }
        }
        None
    }

    fn answer(&mut self, r: &SemanticTokensRequest, replies: Vec<(&str, SemanticTokensReply)>) {
        self.doc
            .set_semantic_tokens(
                r.id,
                replies
                    .into_iter()
                    .map(|(s, x)| (SourceId::new(s), x))
                    .collect(),
            )
            .unwrap();
        self.drain_releases();
    }

    fn drain_releases(&mut self) {
        let pending = self.drain();
        assert!(pending.is_empty(), "no request while answering");
    }

    fn type_at_start(&mut self, s: &str) {
        let cs = ChangeSet::replace(self.doc.text().len_bytes(), [(0..0, s)]).unwrap();
        self.doc
            .apply(Transaction::new(self.doc.version(), cs), &mut self.view)
            .unwrap();
    }
}

fn full(id: Option<&str>, data: &[u32]) -> SemanticTokensReply {
    SemanticTokensReply::Tokens(SemanticTokens {
        result_id: id.map(String::from),
        data: data.to_vec(),
    })
}

#[test]
fn fetched_again_when_the_result_is_empty_with_pending_changes() {
    let mut h = Host::new("Hello world", &["p"]);
    let r1 = h.wait_request(100).expect("first call");
    // the text changes while the provider runs
    h.type_at_start("x");
    h.answer(&r1, vec![("p", SemanticTokensReply::None)]);
    let r2 = h.wait_request(5000).expect("called again");
    assert!(r2.id > r1.id);
}

#[test]
fn bad_semantic_token_data_does_not_hang_149412() {
    let mut h = Host::new("", &["p"]);
    let r1 = h.wait_request(100).unwrap();
    assert_eq!(r1.previous, None);
    h.answer(
        &r1,
        vec![(
            "p",
            full(Some("1"), &[4294967293, 0, 7, 16, 0, 1, 4, 3, 11, 1]),
        )],
    );
    h.type_at_start("foo");
    let r2 = h.wait_request(5000).unwrap();
    assert_eq!(r2.previous, Some((SourceId::new("p"), "1".to_string())));
    h.answer(
        &r2,
        vec![(
            "p",
            SemanticTokensReply::Edits(SemanticTokensEdits {
                result_id: Some("2".into()),
                edits: vec![SemanticTokensEdit {
                    start: 4294967276,
                    delete_count: 0,
                    data: Some(vec![2, 0, 3, 11, 0]),
                }],
            }),
        )],
    );
    // the bad edit is refused and the tokens are asked for in full
    let r3 = h.wait_request(100).unwrap();
    assert_eq!(r3.previous, None);
}

#[test]
fn edits_cannot_delete_more_than_the_previous_result_322571() {
    let mut h = Host::new("a", &["p"]);
    let mut last_result_ids = Vec::new();
    let r = h.wait_request(100).unwrap();
    last_result_ids.push(r.previous.clone().map(|p| p.1));
    h.answer(&r, vec![("p", full(Some("full"), &[0, 0, 1, 0, 0]))]);
    h.type_at_start("b");
    let r = h.wait_request(5000).unwrap();
    last_result_ids.push(r.previous.clone().map(|p| p.1));
    h.answer(
        &r,
        vec![(
            "p",
            SemanticTokensReply::Edits(SemanticTokensEdits {
                result_id: Some("invalid-edit".into()),
                edits: vec![SemanticTokensEdit {
                    start: 0,
                    delete_count: 6,
                    data: None,
                }],
            }),
        )],
    );
    let r = h.wait_request(100).unwrap();
    last_result_ids.push(r.previous.clone().map(|p| p.1));
    h.answer(
        &r,
        vec![("p", full(Some("recovered-full"), &[0, 0, 1, 0, 0]))],
    );
    assert_eq!(last_result_ids, vec![None, Some("full".to_string()), None]);
    assert_eq!(
        h.released,
        vec![Some("full".to_string()), Some("invalid-edit".to_string())]
    );
}

#[test]
fn invalid_edits_without_a_previous_result_are_not_retried() {
    let mut h = Host::new("a", &["p"]);
    let mut count = 0;
    let r = h.wait_request(100).unwrap();
    count += 1;
    h.answer(
        &r,
        vec![(
            "p",
            SemanticTokensReply::Edits(SemanticTokensEdits {
                result_id: Some("invalid-edit".into()),
                edits: vec![SemanticTokensEdit {
                    start: 0,
                    delete_count: 1,
                    data: None,
                }],
            }),
        )],
    );
    if h.wait_request(1000).is_some() {
        count += 1;
    }
    assert_eq!(count, 1);
    assert_eq!(h.released, vec![Some("invalid-edit".to_string())]);
}

#[test]
fn provider_change_during_a_request_triggers_another_161573() {
    let mut h = Host::new("", &["p"]);
    let r = h.wait_request(100).unwrap();
    h.tick(1000);
    h.doc.refresh_semantic_tokens(&SourceId::new("p"));
    h.tick(1000);
    h.answer(&r, vec![("p", SemanticTokensReply::None)]);
    assert!(h.wait_request(5000).is_some(), "asked twice");
}

#[test]
fn picks_the_provider_with_actual_items() {
    let mut doc = Document::new("Hello world 2", EditorConfig::default());
    let legend = |t: &str| SemanticTokensLegend {
        token_types: vec![t.into()],
        token_modifiers: vec![],
    };
    doc.set_semantic_tokens_sources(vec![
        SemanticTokensSource {
            id: SourceId::new("one"),
            legend: legend("class"),
        },
        SemanticTokensSource {
            id: SourceId::new("two"),
            legend: legend("enum"),
        },
    ]);
    doc.tick_semantic_tokens(0);
    let SemanticTokensOutput::Request(r) = doc.take_semantic_tokens_outputs().remove(0) else {
        panic!()
    };
    // both providers are asked; the first answers with items, the second with none
    doc.set_semantic_tokens(
        r.id,
        vec![
            (
                SourceId::new("one"),
                full(Some("1"), &[0, 1, 1, 0, 0, 0, 2, 1, 0, 0]),
            ),
            (SourceId::new("two"), SemanticTokensReply::None),
        ],
    )
    .unwrap();
    let (src, id) = doc.semantic_tokens().result().unwrap();
    assert_eq!((src.as_str(), id), ("one", Some("1")));
    // the first's legend: `class`
    let toks = doc.line_tokens(0);
    assert!(
        toks.iter()
            .any(|t| t.style == TokenStyle::of(TokenType::Class))
    );
    // a second request names the provider that answered and its result
    doc.refresh_semantic_tokens(&SourceId::new("one"));
    doc.tick_semantic_tokens(1);
    let SemanticTokensOutput::Request(r) = doc
        .take_semantic_tokens_outputs()
        .into_iter()
        .find(|o| matches!(o, SemanticTokensOutput::Request(_)))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(r.previous, Some((SourceId::new("one"), "1".to_string())));
    doc.set_semantic_tokens(
        r.id,
        vec![
            (
                SourceId::new("one"),
                full(None, &[0, 1, 1, 0, 0, 0, 2, 1, 0, 0]),
            ),
            (SourceId::new("two"), SemanticTokensReply::None),
        ],
    )
    .unwrap();
    let (src, id) = doc.semantic_tokens().result().unwrap();
    assert_eq!((src.as_str(), id), ("one", None));
}

/// `getSemanticTokens`' #136540: a provider that throws (cancelled) fails the request; the
/// document keeps its tokens.
#[test]
fn a_failing_provider_fails_the_request_136540() {
    let mut h = Host::new("example", &["p"]);
    let r = h.wait_request(100).unwrap();
    h.answer(&r, vec![("p", full(Some("1"), &[0, 0, 7, 0, 0]))]);
    h.doc.refresh_semantic_tokens(&SourceId::new("p"));
    let r = h.wait_request(100).unwrap();
    h.answer(&r, vec![("p", SemanticTokensReply::Error)]);
    assert!(!h.doc.semantic_tokens().store().is_empty(), "tokens kept");
    assert!(
        h.wait_request(3000).is_none(),
        "not asked again without changes"
    );
}

// ---- quire's own ----

#[test]
fn legend_types_and_modifiers_map_to_token_styles() {
    let legend = SemanticTokensLegend {
        token_types: vec!["variable".into(), "enumMember".into(), "madeUp".into()],
        token_modifiers: vec!["declaration".into(), "unknown".into(), "readonly".into()],
    };
    assert_eq!(
        legend_style(&legend, 0, 0b101),
        Some(TokenStyle::new(
            TokenType::Variable,
            TokenModifiers::DECLARATION | TokenModifiers::READONLY,
            0
        ))
    );
    assert_eq!(
        legend_style(&legend, 1, 0),
        Some(TokenStyle::of(TokenType::EnumMember))
    );
    // not in quire's types, out of the legend: not shown
    assert_eq!(legend_style(&legend, 2, 0), None);
    assert_eq!(legend_style(&legend, 3, 0), None);
}

#[test]
fn semantic_tokens_replace_type_and_modifiers_and_keep_the_language() {
    let mut h = Host::new("let foo = bar;", &["p"]);
    let lang = 3u8;
    let v = h.doc.version();
    h.doc
        .set_tokens(
            v,
            TokenRuns {
                first_line: 0,
                lines: vec![vec![
                    Token::new(
                        0,
                        TokenStyle::new(TokenType::Keyword, TokenModifiers::NONE, lang),
                    ),
                    Token::new(
                        3,
                        TokenStyle::new(TokenType::Text, TokenModifiers::NONE, lang),
                    ),
                    Token::new(
                        4,
                        TokenStyle::new(TokenType::Variable, TokenModifiers::NONE, lang),
                    ),
                    Token::new(
                        7,
                        TokenStyle::new(TokenType::Text, TokenModifiers::NONE, lang),
                    ),
                ]],
            },
        )
        .unwrap();
    let r = h.wait_request(100).unwrap();
    // `foo` (4..7) a class declaration
    let legend_class_decl = [0, 4, 3, 0, 0];
    h.answer(&r, vec![("p", full(None, &legend_class_decl))]);
    let toks = h.doc.line_tokens(0);
    let styles: Vec<(u32, TokenType, u8)> = toks
        .iter()
        .map(|t| (t.start, t.style.ty(), t.style.language()))
        .collect();
    assert_eq!(
        styles,
        vec![
            (0, TokenType::Keyword, lang),
            (3, TokenType::Text, lang),
            (4, TokenType::Class, lang),
            (7, TokenType::Text, lang),
        ]
    );
}

#[test]
fn edits_move_semantic_tokens_until_the_next_answer() {
    let mut h = Host::new("foo bar", &["p"]);
    let r = h.wait_request(100).unwrap();
    h.answer(&r, vec![("p", full(None, &[0, 4, 3, 0, 0]))]);
    h.type_at_start("x ");
    let toks = h.doc.line_tokens(0);
    let class_at: Vec<u32> = toks
        .iter()
        .filter(|t| t.style.ty() == TokenType::Class)
        .map(|t| t.start)
        .collect();
    assert_eq!(class_at, vec![6]);
}

#[test]
fn requests_are_debounced_after_edits() {
    let mut h = Host::new("foo", &["p"]);
    let r = h.wait_request(100).unwrap();
    h.tick(1000);
    h.answer(&r, vec![("p", SemanticTokensReply::None)]);
    // the provider took 1000 ms
    assert_eq!(h.doc.semantic_tokens().delay(), 1000);
    h.type_at_start("a");
    h.tick(500);
    assert!(h.drain().is_empty(), "within the debounce");
    assert!(h.wait_request(1000).is_some());
}

#[test]
fn edits_while_a_request_runs_move_its_answer() {
    let mut h = Host::new("foo bar", &["p"]);
    let r = h.wait_request(100).unwrap();
    h.type_at_start("xy");
    // the answer is for the old text: `bar` at 4..7
    h.answer(&r, vec![("p", full(None, &[0, 4, 3, 0, 0]))]);
    let toks = h.doc.line_tokens(0);
    let class_at: Vec<u32> = toks
        .iter()
        .filter(|t| t.style.ty() == TokenType::Class)
        .map(|t| t.start)
        .collect();
    assert_eq!(class_at, vec![6]);
}
