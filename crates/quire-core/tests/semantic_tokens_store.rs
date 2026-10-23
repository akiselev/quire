//! VS Code's sparse token store tests (`src/vs/editor/test/common/model/tokensStore.test.ts`
//! at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated for
//! `quire_core::semantic_tokens::store` and `Document::line_tokens`. The tests that check
//! VS Code's metadata bits merge with VS Code's semantic masks (`vscode_mask`); quire's own
//! merge replaces type and modifiers.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::semantic_tokens::{LineRange, SparseMultilineTokens, SparseTokensStore, TokenEdit};
use quire_core::syntax::{TokenStyle, TokenType};
use quire_core::{ChangeSet, Document, EditorConfig, EditorView, Transaction};

// VS Code `MetadataConsts`.
const ITALIC_MASK: u32 = 0b0000_1000_0000_0000;
const BOLD_MASK: u32 = 0b0001_0000_0000_0000;
const UNDERLINE_MASK: u32 = 0b0010_0000_0000_0000;
const STRIKETHROUGH_MASK: u32 = 0b0100_0000_0000_0000;
const FOREGROUND_MASK: u32 = 0b0000_0000_1111_1111_1000_0000_0000_0000;
const BACKGROUND_MASK: u32 = 0b1111_1111_0000_0000_0000_0000_0000_0000;
const SEMANTIC_USE_ITALIC: u32 = 1;
const SEMANTIC_USE_BOLD: u32 = 2;
const SEMANTIC_USE_UNDERLINE: u32 = 4;
const SEMANTIC_USE_STRIKETHROUGH: u32 = 8;
const SEMANTIC_USE_FOREGROUND: u32 = 16;
const SEMANTIC_USE_BACKGROUND: u32 = 32;
const FOREGROUND_OFFSET: u32 = 15;
const FONT_STYLE_OFFSET: u32 = 11;
const LANGUAGEID_OFFSET: u32 = 0;

/// VS Code's `bMask` in `addSparseTokens`.
fn vscode_mask(b: u32) -> u32 {
    (if b & SEMANTIC_USE_ITALIC != 0 {
        ITALIC_MASK
    } else {
        0
    }) | (if b & SEMANTIC_USE_BOLD != 0 {
        BOLD_MASK
    } else {
        0
    }) | (if b & SEMANTIC_USE_UNDERLINE != 0 {
        UNDERLINE_MASK
    } else {
        0
    }) | (if b & SEMANTIC_USE_STRIKETHROUGH != 0 {
        STRIKETHROUGH_MASK
    } else {
        0
    }) | (if b & SEMANTIC_USE_FOREGROUND != 0 {
        FOREGROUND_MASK
    } else {
        0
    }) | (if b & SEMANTIC_USE_BACKGROUND != 0 {
        BACKGROUND_MASK
    } else {
        0
    })
}

/// The semantic style the adjustment tests mark (VS Code: `SEMANTIC_COLOR`).
fn semantic() -> TokenStyle {
    TokenStyle::of(TokenType::Variable)
}

/// `parseTokensState`: `|...|` marks a semantic token.
fn parse_tokens_state(state: &[&str]) -> (String, SparseMultilineTokens) {
    let mut text: Vec<String> = Vec::new();
    let mut tokens: Vec<u32> = Vec::new();
    let mut base_line = 1i64;
    for (i, line) in state.iter().enumerate() {
        let mut start = 0usize;
        let mut line_text = String::new();
        while let Some(p1) = line[start..].find('|').map(|p| p + start) {
            let Some(p2) = line[p1 + 1..].find('|').map(|p| p + p1 + 1) else {
                break;
            };
            if p1 + 1 == p2 {
                // skip ||
                line_text += &line[start..=p2];
                start = p2 + 1;
                continue;
            }
            line_text += &line[start..p1];
            let tok_start = line_text.len() as u32;
            let tok_len = (p2 - p1 - 1) as u32;
            if tokens.is_empty() {
                base_line = i as i64 + 1;
            }
            tokens.extend([
                (i as i64 + 1 - base_line) as u32,
                tok_start,
                tok_start + tok_len,
                semantic().0,
            ]);
            line_text += &line[p1 + 1..p2];
            start = p2 + 1;
        }
        line_text += &line[start..];
        text.push(line_text);
    }
    (
        text.join("\n"),
        SparseMultilineTokens::create(base_line, tokens),
    )
}

/// `extractState`.
fn extract_state(doc: &Document) -> Vec<String> {
    let t = doc.text();
    (0..t.len_lines())
        .map(|l| {
            let content = t.line(l).to_string();
            let toks = doc.line_tokens(l);
            let mut out = String::new();
            for (i, tok) in toks.iter().enumerate() {
                let s = tok.start as usize;
                let e = toks.get(i + 1).map_or(content.len(), |n| n.start as usize);
                let piece = &content[s..e];
                if tok.style == semantic() {
                    out += &format!("|{piece}|");
                } else {
                    out += piece;
                }
            }
            if toks.is_empty() {
                out = content;
            }
            out
        })
        .collect()
}

/// A VS Code range: `(startLine, startCol, endLine, endCol)`, 1-based.
type VsRange = (usize, usize, usize, usize);

/// `testTokensAdjustment`: edits as VS Code ranges `(startLine, startCol, endLine, endCol)`,
/// 1-based, in the initial text.
fn test_tokens_adjustment(initial: &[&str], edits: &[(VsRange, &str)], expected: &[&str]) {
    let (text, tokens) = parse_tokens_state(initial);
    let mut doc = Document::new(&text, EditorConfig::default());
    let mut view = EditorView::new(&doc);
    doc.set_semantic_token_pieces(Some(vec![tokens]), true);
    let t = doc.text();
    let at = |l: usize, c: usize| t.line_to_byte(l - 1) + c - 1;
    let ranges: Vec<(std::ops::Range<usize>, String)> = edits
        .iter()
        .map(|&((a, b, c, d), s)| (at(a, b)..at(c, d), s.to_string()))
        .collect();
    let cs = ChangeSet::replace(t.len_bytes(), ranges).unwrap();
    doc.apply(Transaction::new(doc.version(), cs), &mut view)
        .unwrap();
    assert_eq!(extract_state(&doc), expected);
}

#[test]
fn color_shifting_between_different_tokens_86303() {
    test_tokens_adjustment(
        &[
            "import { |URI| } from 'vs/base/common/uri';",
            "const foo = |URI|.parse('hey');",
        ],
        &[((2, 9, 2, 10), "")],
        &[
            "import { |URI| } from 'vs/base/common/uri';",
            "const fo = |URI|.parse('hey');",
        ],
    );
}

#[test]
fn deleting_a_newline() {
    test_tokens_adjustment(
        &[
            "import { |URI| } from 'vs/base/common/uri';",
            "const foo = |URI|.parse('hey');",
        ],
        &[((1, 42, 2, 1), "")],
        &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
    );
}

#[test]
fn inserting_a_newline() {
    test_tokens_adjustment(
        &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
        &[((1, 42, 1, 42), "\n")],
        &[
            "import { |URI| } from 'vs/base/common/uri';",
            "const foo = |URI|.parse('hey');",
        ],
    );
}

#[test]
fn deleting_a_newline_2() {
    test_tokens_adjustment(
        &[
            "import { ",
            "    |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');",
        ],
        &[((1, 10, 2, 5), "")],
        &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
    );
}

#[test]
fn a_complex_edit_179268() {
    test_tokens_adjustment(
        &[
            "|export| |'interior_material_selector.dart'|;",
            "|export| |'mileage_selector.dart'|;",
            "|export| |'owners_selector.dart'|;",
            "|export| |'price_selector.dart'|;",
            "|export| |'seat_count_selector.dart'|;",
            "|export| |'year_selector.dart'|;",
            "|export| |'winter_options_selector.dart'|;|export| |'camera_selector.dart'|;",
        ],
        &[
            ((1, 9, 1, 9), "camera_selector.dart';\nexport '"),
            ((6, 9, 7, 9), ""),
            ((7, 39, 7, 39), "\n"),
            ((7, 47, 7, 48), "ye"),
            ((7, 49, 7, 51), ""),
            ((7, 52, 7, 53), ""),
        ],
        &[
            "|export| |'|camera_selector.dart';",
            "export 'interior_material_selector.dart';",
            "|export| |'mileage_selector.dart'|;",
            "|export| |'owners_selector.dart'|;",
            "|export| |'price_selector.dart'|;",
            "|export| |'seat_count_selector.dart'|;",
            "|export| |'||winter_options_selector.dart'|;",
            "|export| |'year_selector.dart'|;",
        ],
    );
}

#[test]
fn semantic_color_fails_on_line_with_selected_text_91936() {
    let text = "                    else if ($s = 08) then '\\b'";
    let mut store = SparseTokensStore::new();
    store.set(
        Some(vec![SparseMultilineTokens::create(
            1,
            vec![
                0,
                20,
                24,
                0b01111000000000010000, //
                0,
                25,
                27,
                0b01111000000000010000, //
                0,
                28,
                29,
                0b00001000000000010000, //
                0,
                29,
                31,
                0b10000000000000010000, //
                0,
                32,
                33,
                0b00001000000000010000, //
                0,
                34,
                36,
                0b00110000000000010000, //
                0,
                36,
                37,
                0b00001000000000010000, //
                0,
                38,
                42,
                0b01111000000000010000, //
                0,
                43,
                47,
                0b01011000000000010000,
            ],
        )]),
        true,
    );
    // the null language's one token (VS Code's default metadata)
    let base = 0b10000000001000010000000001;
    let got = store.add_sparse_tokens(1, &[(text.len() as u32, base)], vscode_mask);
    let flat: Vec<u32> = got.iter().flat_map(|&(e, m)| [e, m]).collect();
    assert_eq!(
        flat,
        vec![
            20,
            0b10000000001000010000000001, //
            24,
            0b10000001111000010000000001, //
            25,
            0b10000000001000010000000001, //
            27,
            0b10000001111000010000000001, //
            28,
            0b10000000001000010000000001, //
            29,
            0b10000000001000010000000001, //
            31,
            0b10000010000000010000000001, //
            32,
            0b10000000001000010000000001, //
            33,
            0b10000000001000010000000001, //
            34,
            0b10000000001000010000000001, //
            36,
            0b10000000110000010000000001, //
            37,
            0b10000000001000010000000001, //
            38,
            0b10000000001000010000000001, //
            42,
            0b10000001111000010000000001, //
            43,
            0b10000000001000010000000001, //
            47,
            0b10000001011000010000000001,
        ]
    );
}

fn piece(start: i64, data: &[u32]) -> SparseMultilineTokens {
    SparseMultilineTokens::create(start, data.to_vec())
}

const BASE_12: &[(u32, u32)] = &[(12, 1)];

#[test]
fn partial_tokens_1() {
    let mut store = SparseTokensStore::new();
    let first = || {
        piece(
            5,
            &[
                0, 5, 10, 1, 5, 5, 10, 2, 10, 5, 10, 3, 15, 5, 10, 4, 20, 5, 10, 5, 25, 5, 10, 6,
            ],
        )
    };
    store.set_partial(LineRange::new(1, 0, 31, 1), vec![first()]);
    store.set_partial(
        LineRange::new(18, 0, 42, 0),
        vec![piece(
            20,
            &[
                0, 5, 10, 4, 5, 5, 10, 5, 10, 5, 10, 6, 15, 5, 10, 7, 20, 5, 10, 8,
            ],
        )],
    );
    store.set_partial(LineRange::new(1, 0, 31, 1), vec![first()]);
    assert_eq!(store.add_sparse_tokens(10, BASE_12, vscode_mask).len(), 3);
}

#[test]
fn partial_tokens_2() {
    let mut store = SparseTokensStore::new();
    store.set_partial(
        LineRange::new(1, 0, 31, 1),
        vec![piece(
            5,
            &[
                0, 5, 10, 1, 5, 5, 10, 2, 10, 5, 10, 3, 15, 5, 10, 4, 20, 5, 10, 5, 25, 5, 10, 6,
            ],
        )],
    );
    store.set_partial(
        LineRange::new(6, 0, 36, 1),
        vec![piece(
            10,
            &[
                0, 5, 10, 2, 5, 5, 10, 3, 10, 5, 10, 4, 15, 5, 10, 5, 20, 5, 10, 6,
            ],
        )],
    );
    store.set_partial(
        LineRange::new(17, 0, 42, 0),
        vec![piece(
            20,
            &[
                0, 5, 10, 4, 5, 5, 10, 5, 10, 5, 10, 6, 15, 5, 10, 7, 20, 5, 10, 8,
            ],
        )],
    );
    assert_eq!(store.add_sparse_tokens(20, BASE_12, vscode_mask).len(), 3);
}

#[test]
fn partial_tokens_3() {
    let mut store = SparseTokensStore::new();
    store.set_partial(
        LineRange::new(1, 0, 31, 1),
        vec![piece(
            5,
            &[
                0, 5, 10, 1, 5, 5, 10, 2, 10, 5, 10, 3, 15, 5, 10, 4, 20, 5, 10, 5, 25, 5, 10, 6,
            ],
        )],
    );
    store.set_partial(
        LineRange::new(11, 0, 16, 1),
        vec![piece(10, &[0, 5, 10, 3, 5, 5, 10, 4])],
    );
    assert_eq!(store.add_sparse_tokens(5, BASE_12, vscode_mask).len(), 3);
}

#[test]
fn semantic_colors_stick_around_with_range_provider_94133() {
    let mut store = SparseTokensStore::new();
    store.set_partial(LineRange::new(1, 0, 1, 19), vec![piece(1, &[0, 9, 11, 1])]);
    store.set_partial(LineRange::new(1, 0, 1, 19), vec![]);
    assert_eq!(store.add_sparse_tokens(1, BASE_12, vscode_mask).len(), 1);
}

/// `createTokens` of the `bug` test.
fn create_tokens(s: &str) -> SparseMultilineTokens {
    let s = s.trim_start_matches("[(").trim_end_matches(")]");
    let mut result = Vec::new();
    let mut first_line = 0i64;
    for tok in s.split("),(") {
        let mut it = tok.split(',');
        let line: i64 = it.next().unwrap().parse().unwrap();
        let chars: Vec<u32> = it
            .next()
            .unwrap()
            .split('-')
            .map(|x| x.parse().unwrap())
            .collect();
        if first_line == 0 {
            first_line = line;
        }
        result.extend([
            (line - first_line) as u32,
            chars[0],
            chars[1],
            ((line + chars[0] as i64) % 13) as u32,
        ]);
    }
    SparseMultilineTokens::create(first_line, result)
}

#[test]
fn bug() {
    let mut store = SparseTokensStore::new();
    store.set_partial(LineRange::new(36446, 0, 36475, 114), vec![create_tokens("[(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35),(36470,38-46),(36473,25-35),(36473,36-51),(36474,28-33),(36474,36-49),(36474,50-58),(36475,35-53),(36475,54-62)]")]);
    store.set_partial(LineRange::new(36436, 0, 36464, 141), vec![create_tokens("[(36437,33-37),(36437,38-42),(36437,47-57),(36437,58-67),(36438,35-53),(36438,54-62),(36440,24-29),(36440,33-46),(36440,47-53),(36442,25-35),(36442,36-50),(36443,30-39),(36443,42-46),(36443,47-53),(36443,54-58),(36443,63-73),(36443,74-84),(36443,87-91),(36443,92-98),(36443,101-105),(36443,106-112),(36443,113-119),(36444,28-37),(36444,38-42),(36444,47-57),(36444,58-75),(36444,80-95),(36444,96-105),(36445,35-53),(36445,54-62),(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62)]")]);
    store.set_partial(LineRange::new(36457, 0, 36485, 139), vec![create_tokens("[(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35),(36470,38-46),(36473,25-35),(36473,36-51),(36474,28-33),(36474,36-49),(36474,50-58),(36475,35-53),(36475,54-62),(36477,28-32),(36477,33-37),(36477,42-52),(36477,53-69),(36478,32-36),(36478,37-41),(36478,46-56),(36478,57-74),(36479,32-36),(36479,37-41),(36479,46-56),(36479,57-76),(36480,32-36),(36480,37-41),(36480,46-56),(36480,57-68),(36481,32-36),(36481,37-41),(36481,46-56),(36481,57-68),(36482,39-57),(36482,58-66),(36484,34-38),(36484,39-45),(36484,46-50),(36484,55-65),(36484,66-82),(36484,86-97),(36484,98-102),(36484,103-109),(36484,111-124),(36484,125-133),(36485,39-57),(36485,58-66)]")]);
    store.set_partial(LineRange::new(36441, 0, 36469, 55), vec![create_tokens("[(36442,25-35),(36442,36-50),(36443,30-39),(36443,42-46),(36443,47-53),(36443,54-58),(36443,63-73),(36443,74-84),(36443,87-91),(36443,92-98),(36443,101-105),(36443,106-112),(36443,113-119),(36444,28-37),(36444,38-42),(36444,47-57),(36444,58-75),(36444,80-95),(36444,96-105),(36445,35-53),(36445,54-62),(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35)]")]);
    let got = store.add_sparse_tokens(36451, &[(60, 1)], vscode_mask);
    assert_eq!(got.len(), 7);
}

fn tm_metadata(foreground: u32, font_style: u32, language: u32) -> u32 {
    (language << LANGUAGEID_OFFSET)
        | (font_style << FONT_STYLE_OFFSET)
        | (foreground << FOREGROUND_OFFSET)
}

#[test]
fn identifiers_bold_when_targetting_keywords_95949() {
    let mut store = SparseTokensStore::new();
    store.set(
        Some(vec![piece(
            1,
            &[0, 6, 11, (1 << FOREGROUND_OFFSET) | SEMANTIC_USE_FOREGROUND],
        )]),
        true,
    );
    let bold = 2;
    let got = store.add_sparse_tokens(
        1,
        &[
            (5, tm_metadata(5, bold, 53)),
            (14, tm_metadata(1, 0, 53)),
            (17, tm_metadata(6, 0, 53)),
            (18, tm_metadata(1, 0, 53)),
        ],
        vscode_mask,
    );
    assert_eq!(
        got,
        vec![
            (5, tm_metadata(5, bold, 53)),
            (6, tm_metadata(1, 0, 53)),
            (11, tm_metadata(1, 0, 53)),
            (14, tm_metadata(1, 0, 53)),
            (17, tm_metadata(6, 0, 53)),
            (18, tm_metadata(1, 0, 53)),
        ]
    );
}

#[test]
fn set_partial_start_line_over_1_and_removal() {
    let mut store = SparseTokensStore::new();
    store.set(Some(vec![piece(5, &[0, 5, 10, 1])]), false);
    assert!(!store.is_empty());
    store.set_partial(LineRange::new(5, 0, 5, 19), vec![]);
    assert!(
        store.is_empty(),
        "store should be empty after setPartial removes all tokens"
    );
}

#[test]
fn set_partial_split_with_empty_first_piece() {
    let mut store = SparseTokensStore::new();
    store.set(Some(vec![piece(1, &[10, 5, 10, 1])]), false);
    store.set_partial(LineRange::new(1, 0, 5, 0), vec![]);
    assert!(!store.is_empty(), "the token on line 11 stays");
    let got = store.add_sparse_tokens(11, &[(22, 1)], vscode_mask);
    assert_eq!(got.len(), 3, "base start, the semantic token, base end");
    assert_eq!(got[0].0, 5, "the semantic token starts at 5");
    assert_eq!(got[1].0, 10, "the semantic token ends at 10");
}

/// VS Code also checks `LineTokens.withInserted` (DOM injected text) on the result; quire
/// splices injected text into the layout, not into tokens, so only the end offsets are checked.
#[test]
fn overlapping_semantic_tokens_no_backward_end_offsets() {
    let m1 = (1 << FOREGROUND_OFFSET) | SEMANTIC_USE_FOREGROUND;
    let m2 = (2 << FOREGROUND_OFFSET) | SEMANTIC_USE_FOREGROUND;
    let mut store = SparseTokensStore::new();
    store.set(
        Some(vec![piece(
            1,
            &[
                0, 0, 1, m1, 0, 1, 2, m2, 0, 2, 3, m1, 0, 3, 5, m2, 0, 4, 5, m1,
            ],
        )]),
        true,
    );
    let tm = 3 << FOREGROUND_OFFSET;
    let got = store.add_sparse_tokens(1, &[(6, tm)], vscode_mask);
    for w in got.windows(2) {
        assert!(w[1].0 > w[0].0, "{got:?}");
    }
}

#[test]
fn piece_emptied_by_encompassing_deletion() {
    let mut store = SparseTokensStore::new();
    let p = piece(5, &[0, 0, 5, 1, 5, 0, 5, 2]);
    assert_eq!(p.start_line(), 5);
    assert_eq!(p.end_line(), 10);
    assert!(!p.is_empty());
    store.set(Some(vec![p]), false);
    store.accept_edit(&TokenEdit {
        range: LineRange::new(1, 0, 20, 0),
        eol_count: 0,
        first_line_length: 0,
        last_line_length: 0,
        first_char: '\0',
    });
    assert!(
        store.is_empty(),
        "the store is empty after the encompassing deletion"
    );
}
