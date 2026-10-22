//! VS Code's suggest scorer: `fuzzyScore`, `fuzzyScoreGraceful(Aggressive)`, `anyScore` and
//! `createMatches` (`src/vs/base/common/filters.ts` at microsoft/vscode
//! `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated literally.
//!
//! Like JavaScript strings, patterns and words are indexed in UTF-16 code units; match
//! positions are UTF-16 offsets into the word (convert with [`utf16_to_byte`]).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use std::cell::RefCell;
use std::ops::Range;

/// A fuzzy match: the score, the word offset matching started at, and the matched positions
/// (UTF-16 offsets into the word, ascending). VS Code's `FuzzyScore` tuple.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuzzyScore {
    /// Higher is better.
    pub score: i32,
    /// Where in the word matching started.
    pub word_start: usize,
    /// Matched word positions, ascending.
    pub matches: Vec<usize>,
}

impl FuzzyScore {
    /// No matches and score `-100` (VS Code `FuzzyScore.Default`): the rank of every item when
    /// there is nothing to filter with.
    pub fn default_score() -> Self {
        FuzzyScore {
            score: -100,
            word_start: 0,
            matches: Vec::new(),
        }
    }

    /// Whether this is [`FuzzyScore::default_score`].
    pub fn is_default(&self) -> bool {
        self.score == -100 && self.word_start == 0 && self.matches.is_empty()
    }
}

/// Scorer options (VS Code `FuzzyScoreOptions`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FuzzyScoreOptions {
    /// Accept a match whose first character is not at a strong position (word start, camel
    /// hump, after a separator).
    pub first_match_can_be_weak: bool,
    /// Add 2 when the whole word is matched.
    pub boost_full_match: bool,
}

impl Default for FuzzyScoreOptions {
    /// `{ boostFullMatch: true, firstMatchCanBeWeak: false }`.
    fn default() -> Self {
        FuzzyScoreOptions {
            first_match_can_be_weak: false,
            boost_full_match: true,
        }
    }
}

/// A scorer function: `fuzzyScore` or one of its graceful variants.
pub type FuzzyScorer =
    fn(&[u16], &[u16], usize, &[u16], &[u16], usize, FuzzyScoreOptions) -> Option<FuzzyScore>;

/// A string as UTF-16 code units (JavaScript's indexing).
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// JavaScript's `toLowerCase`, per code unit (a character whose lowercase form has another
/// length is kept as is, so positions stay aligned).
pub fn utf16_lower(s: &[u16]) -> Vec<u16> {
    s.iter()
        .map(|&u| {
            if u < 128 {
                (u as u8).to_ascii_lowercase() as u16
            } else if let Some(c) = char::from_u32(u as u32) {
                let mut l = c.to_lowercase();
                match (l.next(), l.next()) {
                    (Some(lc), None) if (lc as u32) < 0x10000 => lc as u16,
                    _ => u,
                }
            } else {
                u
            }
        })
        .collect()
}

/// The byte offset in `s` of UTF-16 offset `u` (clamped to the end).
pub fn utf16_to_byte(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}

/// Match ranges (UTF-16, ascending) of a score (VS Code `createMatches`).
pub fn create_matches(score: Option<&FuzzyScore>) -> Vec<Range<usize>> {
    let Some(score) = score else {
        return Vec::new();
    };
    let mut res: Vec<Range<usize>> = Vec::new();
    for &pos in &score.matches {
        match res.last_mut() {
            Some(last) if last.end == pos => last.end = pos + 1,
            _ => res.push(pos..pos + 1),
        }
    }
    res
}

/// `anyScore`: a fuzzy match from any pattern position (first match may be weak), or score 0.
pub fn any_score(
    pattern: &[u16],
    low_pattern: &[u16],
    mut pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
) -> FuzzyScore {
    let max = 13.min(pattern.len());
    let opts = FuzzyScoreOptions {
        first_match_can_be_weak: true,
        boost_full_match: true,
    };
    while pattern_pos < max {
        if let Some(r) = fuzzy_score(pattern, low_pattern, pattern_pos, word, low_word, word_pos, opts)
        {
            return r;
        }
        pattern_pos += 1;
    }
    FuzzyScore {
        score: 0,
        word_start: word_pos,
        matches: Vec::new(),
    }
}

const MAX_LEN: usize = 128;
const MIN_SAFE_INTEGER: i32 = i32::MIN;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Arrow {
    Diag = 1,
    Left = 2,
    LeftLeft = 3,
}

struct Tables {
    min_word_match_pos: Vec<usize>,
    max_word_match_pos: Vec<usize>,
    diag: Vec<i32>,
    table: Vec<i32>,
    arrows: Vec<Arrow>,
}

const W: usize = MAX_LEN + 1;

impl Tables {
    fn new() -> Self {
        Tables {
            min_word_match_pos: vec![0; 2 * MAX_LEN + 1],
            max_word_match_pos: vec![0; 2 * MAX_LEN + 1],
            diag: vec![0; W * W],
            table: vec![0; W * W],
            arrows: vec![Arrow::Diag; W * W],
        }
    }
}

thread_local! {
    static TABLES: RefCell<Tables> = RefCell::new(Tables::new());
}

fn is_emoji_imprecise(x: u32) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&x)
        || x == 8986
        || x == 8987
        || x == 9200
        || x == 9203
        || (9728..=10175).contains(&x)
        || x == 11088
        || x == 11093
        || (127744..=128591).contains(&x)
        || (128640..=128764).contains(&x)
        || (128992..=129008).contains(&x)
        || (129280..=129535).contains(&x)
        || (129648..=129782).contains(&x)
}

/// JavaScript `codePointAt`.
fn code_point_at(s: &[u16], i: usize) -> u32 {
    let u = s[i] as u32;
    if (0xD800..0xDC00).contains(&u)
        && let Some(&n) = s.get(i + 1)
        && (0xDC00..0xE000).contains(&(n as u32))
    {
        return 0x10000 + ((u - 0xD800) << 10) + (n as u32 - 0xDC00);
    }
    u
}

fn is_separator_at_pos(value: &[u16], index: isize) -> bool {
    if index < 0 || index as usize >= value.len() {
        return false;
    }
    let code = code_point_at(value, index as usize);
    match code {
        0x5F | 0x2D | 0x2E | 0x20 | 0x2F | 0x5C | 0x27 | 0x22 | 0x3A | 0x24 | 0x3C | 0x3E
        | 0x28 | 0x29 | 0x5B | 0x5D | 0x7B | 0x7D => true,
        _ => is_emoji_imprecise(code),
    }
}

fn is_whitespace_at_pos(value: &[u16], index: isize) -> bool {
    if index < 0 || index as usize >= value.len() {
        return false;
    }
    matches!(value[index as usize], 0x20 | 0x09)
}

fn is_upper_case_at_pos(pos: isize, word: &[u16], word_low: &[u16]) -> bool {
    // JavaScript: `word[pos] !== wordLow[pos]` (undefined === undefined out of range).
    if pos < 0 {
        return false;
    }
    word.get(pos as usize) != word_low.get(pos as usize)
}

fn is_pattern_in_word(
    t: &mut Tables,
    pattern_low: &[u16],
    mut pattern_pos: usize,
    pattern_len: usize,
    word_low: &[u16],
    mut word_pos: usize,
    word_len: usize,
    fill: bool,
) -> bool {
    while pattern_pos < pattern_len && word_pos < word_len {
        if pattern_low[pattern_pos] == word_low[word_pos] {
            if fill {
                t.min_word_match_pos[pattern_pos] = word_pos;
            }
            pattern_pos += 1;
        }
        word_pos += 1;
    }
    pattern_pos == pattern_len
}

/// Whether the pattern's characters occur in order in the word (VS Code `isPatternInWord`).
pub fn pattern_in_word(pattern_low: &[u16], word_low: &[u16]) -> bool {
    let (mut p, mut w) = (0, 0);
    while p < pattern_low.len() && w < word_low.len() {
        if pattern_low[p] == word_low[w] {
            p += 1;
        }
        w += 1;
    }
    p == pattern_low.len()
}

fn fill_in_max_word_match_pos(
    t: &mut Tables,
    pattern_len: usize,
    word_len: usize,
    pattern_start: usize,
    word_start: usize,
    pattern_low: &[u16],
    word_low: &[u16],
) {
    let mut pattern_pos = pattern_len as isize - 1;
    let mut word_pos = word_len as isize - 1;
    while pattern_pos >= pattern_start as isize && word_pos >= word_start as isize {
        if pattern_low[pattern_pos as usize] == word_low[word_pos as usize] {
            t.max_word_match_pos[pattern_pos as usize] = word_pos as usize;
            pattern_pos -= 1;
        }
        word_pos -= 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn do_score(
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_pos: usize,
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_pos: usize,
    word_len: usize,
    word_start: usize,
    new_match_start: bool,
    out_first_match_strong: &mut bool,
) -> i32 {
    if pattern_low[pattern_pos] != word_low[word_pos] {
        return MIN_SAFE_INTEGER;
    }
    let wp = word_pos as isize;
    let mut score = 1;
    let mut is_gap_location = false;
    if word_pos == pattern_pos - pattern_start {
        // common prefix: `foobar <-> foobaz`
        score = if pattern[pattern_pos] == word[word_pos] { 7 } else { 5 };
    } else if is_upper_case_at_pos(wp, word, word_low)
        && (word_pos == 0 || !is_upper_case_at_pos(wp - 1, word, word_low))
    {
        // hitting upper-case: `foo <-> forOthers`
        score = if pattern[pattern_pos] == word[word_pos] { 7 } else { 5 };
        is_gap_location = true;
    } else if is_separator_at_pos(word_low, wp)
        && (word_pos == 0 || !is_separator_at_pos(word_low, wp - 1))
    {
        // hitting a separator: `. <-> foo.bar`
        score = 5;
    } else if is_separator_at_pos(word_low, wp - 1) || is_whitespace_at_pos(word_low, wp - 1) {
        // post separator: `foo <-> bar_foo`
        score = 5;
        is_gap_location = true;
    }

    if score > 1 && pattern_pos == pattern_start {
        *out_first_match_strong = true;
    }

    if !is_gap_location {
        is_gap_location = is_upper_case_at_pos(wp, word, word_low)
            || is_separator_at_pos(word_low, wp - 1)
            || is_whitespace_at_pos(word_low, wp - 1);
    }

    if pattern_pos == pattern_start {
        // first character in pattern
        if word_pos > word_start {
            // the first pattern character would match a word character that is not at the word
            // start, so introduce a penalty to account for the gap preceding this match
            score -= if is_gap_location { 3 } else { 5 };
        }
    } else if new_match_start {
        // the beginning of a new match (a gap before this location)
        score += if is_gap_location { 2 } else { 0 };
    } else {
        // part of a contiguous match: a slight bonus unless it would be a preferred gap location
        score += if is_gap_location { 0 } else { 1 };
    }

    if word_pos + 1 == word_len {
        // pretend there is a gap after the last character in the word
        score -= if is_gap_location { 3 } else { 5 };
    }

    score
}

/// VS Code `fuzzyScore`: score `pattern[pattern_start..]` against `word[word_start..]`.
/// `None` when the pattern does not match (or its first match is weak and the options do not
/// allow that).
pub fn fuzzy_score(
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_start: usize,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    TABLES.with(|t| {
        fuzzy_score_in(
            &mut t.borrow_mut(),
            pattern,
            pattern_low,
            pattern_start,
            word,
            word_low,
            word_start,
            options,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn fuzzy_score_in(
    t: &mut Tables,
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_start: usize,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    let pattern_len = pattern.len().min(MAX_LEN);
    let word_len = word.len().min(MAX_LEN);

    if pattern_start >= pattern_len
        || word_start >= word_len
        || (pattern_len - pattern_start) > (word_len - word_start)
    {
        return None;
    }

    // A simple check that the characters of pattern occur (in order) in word.
    if !is_pattern_in_word(
        t,
        pattern_low,
        pattern_start,
        pattern_len,
        word_low,
        word_start,
        word_len,
        true,
    ) {
        return None;
    }

    // The max matching word position for each pattern position (the min was filled above).
    fill_in_max_word_match_pos(
        t,
        pattern_len,
        word_len,
        pattern_start,
        word_start,
        pattern_low,
        word_low,
    );

    let mut row: usize = 1;
    let mut column: usize = 1;
    let mut has_strong_first_match = false;
    let idx = |r: usize, c: usize| r * W + c;
    // Row 0 is never written: it holds zeros.
    for c in 0..W {
        t.table[c] = 0;
        t.diag[c] = 0;
    }

    let mut pattern_pos = pattern_start;
    while pattern_pos < pattern_len {
        // Reduce the search space to possible matching word positions and to positions the
        // next row can access.
        let min_word_match_pos = t.min_word_match_pos[pattern_pos];
        let max_word_match_pos = t.max_word_match_pos[pattern_pos];
        let next_max_word_match_pos = if pattern_pos + 1 < pattern_len {
            t.max_word_match_pos[pattern_pos + 1]
        } else {
            word_len
        };

        column = min_word_match_pos - word_start + 1;
        let mut word_pos = min_word_match_pos;
        while word_pos < next_max_word_match_pos {
            let mut score = MIN_SAFE_INTEGER;
            let mut can_come_diag = false;

            if word_pos <= max_word_match_pos {
                score = do_score(
                    pattern,
                    pattern_low,
                    pattern_pos,
                    pattern_start,
                    word,
                    word_low,
                    word_pos,
                    word_len,
                    word_start,
                    t.diag[idx(row - 1, column - 1)] == 0,
                    &mut has_strong_first_match,
                );
            }

            let mut diag_score = 0;
            if score != MIN_SAFE_INTEGER {
                can_come_diag = true;
                diag_score = score + t.table[idx(row - 1, column - 1)];
            }

            let can_come_left = word_pos > min_word_match_pos;
            let left_score = if can_come_left {
                t.table[idx(row, column - 1)]
                    + if t.diag[idx(row, column - 1)] > 0 { -5 } else { 0 }
            } else {
                0
            }; // penalty for a gap start

            let can_come_left_left =
                word_pos > min_word_match_pos + 1 && t.diag[idx(row, column - 1)] > 0;
            let left_left_score = if can_come_left_left {
                t.table[idx(row, column - 2)]
                    + if t.diag[idx(row, column - 2)] > 0 { -5 } else { 0 }
            } else {
                0
            };

            if can_come_left_left
                && (!can_come_left || left_left_score >= left_score)
                && (!can_come_diag || left_left_score >= diag_score)
            {
                // prefer left left to jump over a diagonal: the match is earlier in the word
                t.table[idx(row, column)] = left_left_score;
                t.arrows[idx(row, column)] = Arrow::LeftLeft;
                t.diag[idx(row, column)] = 0;
            } else if can_come_left && (!can_come_diag || left_score >= diag_score) {
                // prefer left: the match is earlier in the word
                t.table[idx(row, column)] = left_score;
                t.arrows[idx(row, column)] = Arrow::Left;
                t.diag[idx(row, column)] = 0;
            } else if can_come_diag {
                t.table[idx(row, column)] = diag_score;
                t.arrows[idx(row, column)] = Arrow::Diag;
                t.diag[idx(row, column)] = t.diag[idx(row - 1, column - 1)] + 1;
            } else {
                unreachable!("not possible");
            }
            column += 1;
            word_pos += 1;
        }
        row += 1;
        pattern_pos += 1;
    }

    if !has_strong_first_match && !options.first_match_can_be_weak {
        return None;
    }

    row -= 1;
    column -= 1;

    let mut score = t.table[idx(row, column)];
    let mut rev: Vec<usize> = Vec::with_capacity(row);

    let mut backwards_diag_length = 0;
    let mut max_match_column = 0;

    while row >= 1 {
        // Find the column where we go diagonally up.
        let mut diag_column = column as isize;
        loop {
            let arrow = t.arrows[idx(row, diag_column as usize)];
            if arrow == Arrow::LeftLeft {
                diag_column -= 2;
            } else if arrow == Arrow::Left {
                diag_column -= 1;
            } else {
                break;
            }
            if diag_column < 1 {
                break;
            }
        }
        let mut diag_column = diag_column.max(0) as usize;

        // Overturn the "forwards" decision if keeping the "backwards" diagonal gives a better
        // match.
        if backwards_diag_length > 1
            && pattern_low[pattern_start + row - 1] == word_low[word_start + column - 1]
            && !is_upper_case_at_pos(
                diag_column as isize + word_start as isize - 1,
                word,
                word_low,
            )
            && backwards_diag_length + 1 > t.diag[idx(row, diag_column)]
        {
            diag_column = column;
        }

        if diag_column == column {
            backwards_diag_length += 1;
        } else {
            backwards_diag_length = 1;
        }

        if max_match_column == 0 {
            max_match_column = diag_column;
        }

        row -= 1;
        column = diag_column.saturating_sub(1);
        rev.push(column);
    }

    if word_len - word_start == pattern_len && options.boost_full_match {
        // the word matches the pattern with all characters: a total match boost
        score += 2;
    }

    // 1 penalty for each skipped character in the word
    let skipped = max_match_column as i32 - pattern_len as i32;
    score -= skipped;

    rev.reverse();
    Some(FuzzyScore {
        score,
        word_start,
        matches: rev.into_iter().map(|c| c + word_start).collect(),
    })
}

/// `fuzzyScoreGracefulAggressive`: also tries swapping neighbouring pattern characters and
/// keeps the best (a swap costs 3).
pub fn fuzzy_score_graceful_aggressive(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    with_permutations(pattern, low_pattern, pattern_pos, word, low_word, word_pos, true, options)
}

/// `fuzzyScoreGraceful`: swaps neighbouring pattern characters only when the pattern itself
/// does not match.
pub fn fuzzy_score_graceful(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    with_permutations(pattern, low_pattern, pattern_pos, word, low_word, word_pos, false, options)
}

#[allow(clippy::too_many_arguments)]
fn with_permutations(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    aggressive: bool,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    let mut top = fuzzy_score(pattern, low_pattern, pattern_pos, word, low_word, word_pos, options);
    if top.is_some() && !aggressive {
        return top;
    }
    if pattern.len() >= 3 {
        // Try a few (max 7) permutations swapping neighbouring characters.
        let tries = 7.min(pattern.len() - 1);
        let mut moving = pattern_pos + 1;
        while moving < tries {
            if let Some(new_pattern) = next_typo_permutation(pattern, moving) {
                let low = utf16_lower(&new_pattern);
                if let Some(mut c) =
                    fuzzy_score(&new_pattern, &low, pattern_pos, word, low_word, word_pos, options)
                {
                    c.score -= 3; // permutation penalty
                    if top.as_ref().is_none_or(|t| c.score > t.score) {
                        top = Some(c);
                    }
                }
            }
            moving += 1;
        }
    }
    top
}

fn next_typo_permutation(pattern: &[u16], pattern_pos: usize) -> Option<Vec<u16>> {
    if pattern_pos + 1 >= pattern.len() {
        return None;
    }
    let (a, b) = (pattern[pattern_pos], pattern[pattern_pos + 1]);
    if a == b {
        return None;
    }
    let mut p = pattern.to_vec();
    p.swap(pattern_pos, pattern_pos + 1);
    Some(p)
}
