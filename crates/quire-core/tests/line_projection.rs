//! VS Code's `ModelLineProjectionData` tests (`src/vs/editor/test/common/viewModel/
//! lineBreakData.test.ts` at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`),
//! translated for `quire_core::injected::LineProjection`.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use quire_core::injected::{CursorStops, Injection, LineProjection, PositionAffinity};

fn data(offsets: &[usize], texts: &[&str], breaks: &[usize], indent: usize) -> LineProjection {
    LineProjection {
        injections: offsets
            .iter()
            .zip(texts)
            .map(|(&offset, t)| Injection {
                offset,
                len: t.len(),
                cursor_stops: CursorStops::Both,
            })
            .collect(),
        break_offsets: breaks.to_vec(),
        wrapped_text_indent_length: indent,
    }
}

fn test_inverse(d: &LineProjection) {
    for i in 0..100 {
        let o = d.translate_to_output_position(i, PositionAffinity::None);
        assert_eq!(d.translate_to_input_offset(o.line, o.offset), i, "{i}");
    }
}

fn input_offsets(d: &LineProjection, line: usize) -> Vec<usize> {
    (0..20)
        .map(|i| d.translate_to_input_offset(line, i))
        .collect()
}

fn output_offsets(d: &LineProjection, a: PositionAffinity) -> Vec<String> {
    (0..25)
        .map(|i| d.translate_to_output_position(i, a).to_string())
        .collect()
}

#[test]
fn basic() {
    let d = data(&[], &[], &[100], 10);
    assert_eq!(d.translate_to_input_offset(0, 50), 50);
    assert_eq!(d.translate_to_input_offset(1, 60), 150);
}

fn injected_1() -> LineProjection {
    data(&[2, 3, 10], &["1", "22", "333"], &[10, 100], 10)
}

#[test]
fn injected_text_1_input_offset_of_output_position() {
    let d = injected_1();
    assert_eq!(
        input_offsets(&d, 0),
        [
            0, 1, 2, 2, 3, 3, 3, 4, 5, 6, 7, 8, 9, 10, 10, 10, 10, 11, 12, 13
        ]
    );
    assert_eq!(
        input_offsets(&d, 1),
        [
            7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 9, 10, 10, 10, 10, 11, 12, 13
        ]
    );
}

#[test]
fn injected_text_1_output_position_of_input_offset() {
    let d = injected_1();
    let _ = d.translate_to_output_position(20, PositionAffinity::None);
    let tail = |from: usize| (from..=30).map(|o| format!("1:{o}")).collect::<Vec<_>>();
    let mut none: Vec<String> = [
        "0:0", "0:1", "0:2", "0:4", "0:7", "0:8", "0:9", "1:10", "1:11", "1:12", "1:13",
    ]
    .map(String::from)
    .to_vec();
    none.extend(tail(17));
    assert_eq!(output_offsets(&d, PositionAffinity::None), none);
    let mut left: Vec<String> = [
        "0:0", "0:1", "0:2", "0:4", "0:7", "0:8", "0:9", "0:10", "1:11", "1:12", "1:13",
    ]
    .map(String::from)
    .to_vec();
    left.extend(tail(17));
    assert_eq!(output_offsets(&d, PositionAffinity::Left), left);
    let mut right: Vec<String> = [
        "0:0", "0:1", "0:3", "0:6", "0:7", "0:8", "0:9", "1:10", "1:11", "1:12",
    ]
    .map(String::from)
    .to_vec();
    right.extend(tail(16));
    assert_eq!(output_offsets(&d, PositionAffinity::Right), right);
}

#[test]
fn injected_text_1_inverse() {
    test_inverse(&injected_1());
}

#[test]
fn injected_text_1_normalization() {
    let d = injected_1();
    let got: Vec<String> = (0..25)
        .map(|v| {
            d.normalize_output_position(1, v, PositionAffinity::Right)
                .to_string()
        })
        .collect();
    let mut want: Vec<String> = (0..=12).map(|o| format!("1:{o}")).collect();
    want.extend(["1:16", "1:16", "1:16", "1:16"].map(String::from));
    want.extend((17..=24).map(|o| format!("1:{o}")));
    assert_eq!(got, want);
}

fn injected_2() -> LineProjection {
    data(&[2, 2, 6], &["1", "22", "333"], &[10, 100], 0)
}

#[test]
fn injected_text_2_input_offset_of_output_position() {
    let d = injected_2();
    assert_eq!(
        input_offsets(&d, 0),
        [
            0, 1, 2, 2, 2, 2, 3, 4, 5, 6, 6, 6, 6, 7, 8, 9, 10, 11, 12, 13
        ]
    );
    assert_eq!(
        input_offsets(&d, 1),
        [
            6, 6, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23
        ]
    );
}

#[test]
fn injected_text_2_inverse() {
    test_inverse(&injected_2());
}

fn injected_3() -> LineProjection {
    data(&[2, 2, 7], &["1", "22", "333"], &[10, 100], 0)
}

#[test]
fn injected_text_3_input_offset_of_output_position() {
    let d = injected_3();
    assert_eq!(
        input_offsets(&d, 0),
        [
            0, 1, 2, 2, 2, 2, 3, 4, 5, 6, 7, 7, 7, 7, 8, 9, 10, 11, 12, 13
        ]
    );
    assert_eq!(
        input_offsets(&d, 1),
        [
            7, 7, 7, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23
        ]
    );
}

#[test]
fn injected_text_3_inverse() {
    test_inverse(&injected_3());
}
