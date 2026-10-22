// Translated from Visual Studio Code, src/vs/editor/common/cursor/cursorWordOperations.ts
// (microsoft/vscode @ bdce63db587665a91d1f1237b807d43471f0d9bc).
//
// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software
// and associated documentation files (the "Software"), to deal in the Software without
// restriction, including without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all copies or
// substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Differences: lines are `&[char]` and columns are 0-based char indices (VS Code: UTF-16 code
// units, 1-based). The Intl word segmenter is not ported.

use crate::ops::model::{Model, Pos};
use crate::words::{CharClass, WordClassifier};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum WordType {
    None,
    Regular,
    Separator,
}

#[derive(Copy, Clone, Debug)]
struct FindWordResult {
    start: usize,
    end: usize,
    word_type: WordType,
    next_char_class: CharClass,
}

/// VS Code's `WordNavigationType`.
#[allow(clippy::enum_variant_names)] // VS Code's names
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WordNav {
    WordStart,
    WordStartFast,
    WordEnd,
}

fn class(cls: &WordClassifier, c: char) -> CharClass {
    match cls.class(c) {
        CharClass::LineBreak => CharClass::Whitespace,
        k => k,
    }
}

fn find_previous_word_on_line(
    line: &[char],
    cls: &WordClassifier,
    col: usize,
) -> Option<FindWordResult> {
    let mut word_type = WordType::None;
    let mut ch_index = col as isize - 1;
    while ch_index >= 0 {
        let i = ch_index as usize;
        let ch_class = class(cls, line[i]);
        match ch_class {
            CharClass::Regular => {
                if word_type == WordType::Separator {
                    return Some(FindWordResult {
                        start: i + 1,
                        end: find_end_of_word(line, cls, word_type, i + 1),
                        word_type,
                        next_char_class: ch_class,
                    });
                }
                word_type = WordType::Regular;
            }
            CharClass::Separator => {
                if word_type == WordType::Regular {
                    return Some(FindWordResult {
                        start: i + 1,
                        end: find_end_of_word(line, cls, word_type, i + 1),
                        word_type,
                        next_char_class: ch_class,
                    });
                }
                word_type = WordType::Separator;
            }
            _ => {
                if word_type != WordType::None {
                    return Some(FindWordResult {
                        start: i + 1,
                        end: find_end_of_word(line, cls, word_type, i + 1),
                        word_type,
                        next_char_class: ch_class,
                    });
                }
            }
        }
        ch_index -= 1;
    }
    if word_type != WordType::None {
        return Some(FindWordResult {
            start: 0,
            end: find_end_of_word(line, cls, word_type, 0),
            word_type,
            next_char_class: CharClass::Whitespace,
        });
    }
    None
}

fn find_end_of_word(
    line: &[char],
    cls: &WordClassifier,
    word_type: WordType,
    start: usize,
) -> usize {
    for (i, &c) in line.iter().enumerate().skip(start) {
        let k = class(cls, c);
        if k == CharClass::Whitespace
            || (word_type == WordType::Regular && k == CharClass::Separator)
            || (word_type == WordType::Separator && k == CharClass::Regular)
        {
            return i;
        }
    }
    line.len()
}

fn find_next_word_on_line(
    line: &[char],
    cls: &WordClassifier,
    col: usize,
) -> Option<FindWordResult> {
    let mut word_type = WordType::None;
    let len = line.len();
    for i in col..len {
        let ch_class = class(cls, line[i]);
        match ch_class {
            CharClass::Regular => {
                if word_type == WordType::Separator {
                    return Some(FindWordResult {
                        start: find_start_of_word(line, cls, word_type, i as isize - 1),
                        end: i,
                        word_type,
                        next_char_class: ch_class,
                    });
                }
                word_type = WordType::Regular;
            }
            CharClass::Separator => {
                if word_type == WordType::Regular {
                    return Some(FindWordResult {
                        start: find_start_of_word(line, cls, word_type, i as isize - 1),
                        end: i,
                        word_type,
                        next_char_class: ch_class,
                    });
                }
                word_type = WordType::Separator;
            }
            _ => {
                if word_type != WordType::None {
                    return Some(FindWordResult {
                        start: find_start_of_word(line, cls, word_type, i as isize - 1),
                        end: i,
                        word_type,
                        next_char_class: ch_class,
                    });
                }
            }
        }
    }
    if word_type != WordType::None {
        return Some(FindWordResult {
            start: find_start_of_word(line, cls, word_type, len as isize - 1),
            end: len,
            word_type,
            next_char_class: CharClass::Whitespace,
        });
    }
    None
}

fn find_start_of_word(
    line: &[char],
    cls: &WordClassifier,
    word_type: WordType,
    start: isize,
) -> usize {
    let mut i = start;
    while i >= 0 {
        let k = class(cls, line[i as usize]);
        if k == CharClass::Whitespace
            || (word_type == WordType::Regular && k == CharClass::Separator)
            || (word_type == WordType::Separator && k == CharClass::Regular)
        {
            return i as usize + 1;
        }
        i -= 1;
    }
    0
}

/// `WordOperations.moveWordLeft`.
pub fn move_word_left(
    m: &Model,
    cls: &WordClassifier,
    p: Pos,
    nav: WordNav,
    multicursor: bool,
) -> Pos {
    let mut line = p.line;
    let mut col = p.col;
    if col == 0 && line > 0 {
        line -= 1;
        col = m.line_chars(line).len();
    }
    let chars = m.line_chars(line);
    let mut prev = find_previous_word_on_line(&chars, cls, col);
    match nav {
        WordNav::WordStart => Pos::new(line, prev.map_or(0, |w| w.start)),
        WordNav::WordStartFast => {
            if let Some(w) = prev
                && !multicursor
                && w.word_type == WordType::Separator
                && w.end - w.start == 1
                && w.next_char_class == CharClass::Regular
            {
                prev = find_previous_word_on_line(&chars, cls, w.start);
            }
            Pos::new(line, prev.map_or(0, |w| w.start))
        }
        WordNav::WordEnd => {
            if let Some(w) = prev
                && col <= w.end
            {
                prev = find_previous_word_on_line(&chars, cls, w.start);
            }
            Pos::new(line, prev.map_or(0, |w| w.end))
        }
    }
}

/// `WordOperations.moveWordRight`.
pub fn move_word_right(m: &Model, cls: &WordClassifier, p: Pos, nav: WordNav) -> Pos {
    let mut line = p.line;
    let mut col = p.col;
    let mut moved_down = false;
    if col == m.line_chars(line).len() && line + 1 < m.line_count() {
        moved_down = true;
        line += 1;
        col = 0;
    }
    let chars = m.line_chars(line);
    let mut next = find_next_word_on_line(&chars, cls, col);
    if nav == WordNav::WordEnd {
        if let Some(w) = next
            && w.word_type == WordType::Separator
            && w.end - w.start == 1
            && w.next_char_class == CharClass::Regular
        {
            next = find_next_word_on_line(&chars, cls, w.end);
        }
        col = next.map_or(chars.len(), |w| w.end);
    } else {
        if let Some(w) = next
            && !moved_down
            && col >= w.start
        {
            next = find_next_word_on_line(&chars, cls, w.end);
        }
        col = next.map_or(chars.len(), |w| w.start);
    }
    Pos::new(line, col)
}

fn delete_word_left_whitespace(chars: &[char], p: Pos) -> Option<(Pos, Pos)> {
    // lastNonWhitespaceIndex(lineContent, startIndex)
    let start_index = p.col as isize - 1;
    let mut last_non_ws: isize = -1;
    let mut i = start_index;
    while i >= 0 {
        let c = chars[i as usize];
        if c != ' ' && c != '\t' {
            last_non_ws = i;
            break;
        }
        i -= 1;
    }
    if last_non_ws + 1 < start_index {
        return Some((Pos::new(p.line, (last_non_ws + 1) as usize), p));
    }
    None
}

/// `WordOperations.deleteWordLeft` for an empty selection (auto-closing pair delete is handled
/// by the caller). Returns the range to delete.
pub fn delete_word_left(
    m: &Model,
    cls: &WordClassifier,
    p: Pos,
    nav: WordNav,
    whitespace_heuristics: bool,
) -> Option<(Pos, Pos)> {
    if p.line == 0 && p.col == 0 {
        return None;
    }
    let chars = m.line_chars(p.line);
    if whitespace_heuristics && let Some(r) = delete_word_left_whitespace(&chars, p) {
        return Some(r);
    }
    let mut line = p.line;
    let mut col = p.col;
    let mut prev = find_previous_word_on_line(&chars, cls, p.col);
    if nav == WordNav::WordStart {
        if let Some(w) = prev {
            col = w.start;
        } else if col > 0 {
            col = 0;
        } else {
            line -= 1;
            col = m.line_chars(line).len();
        }
    } else {
        if let Some(w) = prev
            && col <= w.end
        {
            prev = find_previous_word_on_line(&chars, cls, w.start);
        }
        if let Some(w) = prev {
            col = w.end;
        } else if col > 0 {
            col = 0;
        } else {
            line -= 1;
            col = m.line_chars(line).len();
        }
    }
    Some((Pos::new(line, col), p))
}

/// `WordOperations.deleteWordRight` for an empty selection.
pub fn delete_word_right(
    m: &Model,
    cls: &WordClassifier,
    p: Pos,
    nav: WordNav,
    whitespace_heuristics: bool,
) -> Option<(Pos, Pos)> {
    let line_count = m.line_count();
    let chars = m.line_chars(p.line);
    let max_col = chars.len();
    if p.line + 1 == line_count && p.col == max_col {
        return None;
    }
    if whitespace_heuristics {
        let first_non_ws = chars
            .iter()
            .enumerate()
            .skip(p.col)
            .find(|(_, c)| **c != ' ' && **c != '\t')
            .map_or(chars.len(), |(i, _)| i);
        if p.col < first_non_ws {
            return Some((p, Pos::new(p.line, first_non_ws)));
        }
    }
    let mut line = p.line;
    let mut col = p.col;
    let mut next = find_next_word_on_line(&chars, cls, p.col);
    let fallback = |line: &mut usize, col: &mut usize| {
        if *col < max_col || *line + 1 == line_count {
            *col = max_col;
        } else {
            *line += 1;
            let nc = m.line_chars(*line);
            *col = find_next_word_on_line(&nc, cls, 0).map_or(nc.len(), |w| w.start);
        }
    };
    if nav == WordNav::WordEnd {
        if let Some(w) = next {
            col = w.end;
        } else {
            fallback(&mut line, &mut col);
        }
    } else {
        if let Some(w) = next
            && col >= w.start
        {
            next = find_next_word_on_line(&chars, cls, w.end);
        }
        if let Some(w) = next {
            col = w.start;
        } else {
            fallback(&mut line, &mut col);
        }
    }
    Some((p, Pos::new(line, col)))
}

/// The word under `p` for double-click (`WordOperations.word`, entering word selection): a
/// regular word touching the position, else a separator run strictly containing it, else the
/// next regular word touching it; else an empty range at `p`.
pub fn word_at(m: &Model, cls: &WordClassifier, p: Pos) -> (Pos, Pos) {
    let chars = m.line_chars(p.line);
    let prev = find_previous_word_on_line(&chars, cls, p.col);
    let next = find_next_word_on_line(&chars, cls, p.col);
    if let Some(w) = prev {
        if w.word_type == WordType::Regular && w.start <= p.col && p.col <= w.end {
            return (Pos::new(p.line, w.start), Pos::new(p.line, w.end));
        }
        if w.word_type == WordType::Separator && w.start <= p.col && p.col < w.end {
            return (Pos::new(p.line, w.start), Pos::new(p.line, w.end));
        }
    }
    if let Some(w) = next {
        if w.word_type == WordType::Regular && w.start <= p.col && p.col <= w.end {
            return (Pos::new(p.line, w.start), Pos::new(p.line, w.end));
        }
        if w.word_type == WordType::Separator && w.start <= p.col && p.col < w.end {
            return (Pos::new(p.line, w.start), Pos::new(p.line, w.end));
        }
    }
    (p, p)
}
