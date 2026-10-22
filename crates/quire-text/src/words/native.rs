// ported from svelters crates/svelters-egui/src/native/words.rs @ 4365d5a9

//! FX1: word stops of the native text fields (WG3 pin 1), as char offsets within one paragraph
//! (quire-text's [`WordRules::Pango`](super::WordRules) and `Qt` run them per logical line).
//!
//! - [`NativeWords::Pango`]: GTK (`GtkText` word movement and `GTK_DELETE_WORD_ENDS`) over Pango's
//!   default log attributes. A word is a run of letters and numbers (marks and format characters
//!   join it); punctuation, symbols and spaces end it, so `snake_case` and `3.14` are two words
//!   each. Forward stops at the next word end, backward at the previous word start. Japanese runs
//!   also break where the script class changes (kanji, hiragana, katakana), as in Pango.
//! - [`NativeWords::Qt`]: `QTextLayout::{next,previous}CursorPosition(SkipWords)`. Three classes:
//!   white space, Qt's fixed separator set (`QTextEngine::atWordSeparator`) and everything else;
//!   forward skips the current run, then white space (word starts, Windows-like); backward skips
//!   white space, then a run.
//!
//! Checked against `conformance/golden/linux/{gtk-4.24.1,qt-6.12.0}.json` (pins 1 and 2) by the
//! tests below and by `svelters-conformance`'s `egui_measure`.

use unicode_general_category::{GeneralCategory as G, get_general_category};

/// A native toolkit's word-boundary rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeWords {
    /// GTK over Pango's log attributes.
    Pango,
    /// Qt's `SkipWords`.
    Qt,
}

impl NativeWords {
    /// The stop of a word-forward move (or the end of a forward word deletion) from `at`.
    pub fn next(self, text: &str, at: usize) -> usize {
        let chars: Vec<char> = text.chars().collect();
        match self {
            NativeWords::Pango => pango_next(&chars, at),
            NativeWords::Qt => qt_next(&chars, at),
        }
    }

    /// The stop of a word-backward move (or the start of a backward word deletion) from `at`.
    pub fn prev(self, text: &str, at: usize) -> usize {
        let chars: Vec<char> = text.chars().collect();
        match self {
            NativeWords::Pango => pango_prev(&chars, at),
            NativeWords::Qt => qt_prev(&chars, at),
        }
    }
}

impl NativeWords {
    /// The word at char `at`, as double-click selects it: the word start at or before `at` to
    /// that word's end; an empty range between words. Not measured by WG3 (quire-text addition).
    pub fn word_at(self, text: &str, at: usize) -> (usize, usize) {
        let n = text.chars().count();
        let at = at.min(n);
        let a = self.prev(text, (at + 1).min(n));
        let b = self.next(text, a);
        if a <= at && at <= b && a < b {
            (a, b)
        } else {
            (at, at)
        }
    }
}

fn hiragana(c: char) -> bool {
    ('\u{3040}'..='\u{309F}').contains(&c)
}
fn katakana(c: char) -> bool {
    ('\u{30A0}'..='\u{30FF}').contains(&c)
}
fn kanji(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}
fn japanese(c: char) -> bool {
    hiragana(c) || katakana(c) || kanji(c)
}

fn letter(g: G) -> bool {
    matches!(
        g,
        G::UppercaseLetter
            | G::LowercaseLetter
            | G::TitlecaseLetter
            | G::ModifierLetter
            | G::OtherLetter
    )
}
fn number(g: G) -> bool {
    matches!(g, G::DecimalNumber | G::LetterNumber | G::OtherNumber)
}
fn joins(g: G) -> bool {
    matches!(
        g,
        G::SpacingMark | G::EnclosingMark | G::NonspacingMark | G::Format
    )
}

/// Pango's `is_word_start` / `is_word_end` for every position `0..=len` (`pango_default_break`).
fn pango_attrs(chars: &[char]) -> (Vec<bool>, Vec<bool>) {
    let n = chars.len();
    let (mut start, mut end) = (vec![false; n + 1], vec![false; n + 1]);
    // None: not in a word; Some(true): letters; Some(false): numbers
    let mut word: Option<bool> = None;
    let mut last = '\0';
    for i in 0..=n {
        // the end of the text acts as a paragraph separator: it ends a word
        let c = chars.get(i).copied().unwrap_or('\u{2029}');
        let g = get_general_category(c);
        match word {
            Some(letters) => {
                if joins(g) {
                } else if letter(g) {
                    if letters && (japanese(last) || japanese(c)) {
                        let class_change = (hiragana(last) && !hiragana(c))
                            || (katakana(last) && !(katakana(c) || hiragana(c)))
                            || (kanji(last) && !(hiragana(c) || kanji(c)))
                            || (japanese(last) != japanese(c));
                        if class_change {
                            end[i] = true;
                            start[i] = true;
                        }
                    }
                    last = c;
                } else if number(g) {
                    last = c;
                } else {
                    end[i] = true;
                    word = None;
                }
            }
            None => {
                if letter(g) || number(g) {
                    word = Some(letter(g));
                    last = c;
                    start[i] = true;
                }
            }
        }
    }
    (start, end)
}

/// `gtk_text_move_forward_word (self, at, FALSE)`.
fn pango_next(chars: &[char], at: usize) -> usize {
    let n = chars.len();
    if at >= n {
        return n;
    }
    let (_, end) = pango_attrs(chars);
    let mut p = at + 1;
    while p < n && !end[p] {
        p += 1;
    }
    p
}

/// `gtk_text_move_backward_word (self, at, FALSE)`.
fn pango_prev(chars: &[char], at: usize) -> usize {
    let at = at.min(chars.len());
    if at == 0 {
        return 0;
    }
    let (start, _) = pango_attrs(chars);
    let mut p = at - 1;
    while p > 0 && !start[p] {
        p -= 1;
    }
    p
}

/// `QTextEngine::atWordSeparator`.
fn qt_separator(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | '?'
            | '!'
            | '@'
            | '#'
            | '$'
            | ':'
            | ';'
            | '-'
            | '<'
            | '>'
            | '['
            | ']'
            | '('
            | ')'
            | '{'
            | '}'
            | '='
            | '/'
            | '+'
            | '%'
            | '&'
            | '^'
            | '*'
            | '\''
            | '"'
            | '`'
            | '~'
            | '|'
            | '\\'
    )
}

fn qt_next(chars: &[char], at: usize) -> usize {
    let n = chars.len();
    let mut p = at;
    if p >= n {
        return n;
    }
    if qt_separator(chars[p]) {
        p += 1;
        while p < n && qt_separator(chars[p]) {
            p += 1;
        }
    } else {
        while p < n && !chars[p].is_whitespace() && !qt_separator(chars[p]) {
            p += 1;
        }
    }
    while p < n && chars[p].is_whitespace() {
        p += 1;
    }
    p
}

fn qt_prev(chars: &[char], at: usize) -> usize {
    let mut p = at.min(chars.len());
    while p > 0 && chars[p - 1].is_whitespace() {
        p -= 1;
    }
    if p > 0 && qt_separator(chars[p - 1]) {
        p -= 1;
        while p > 0 && qt_separator(chars[p - 1]) {
            p -= 1;
        }
    } else {
        while p > 0 && !chars[p - 1].is_whitespace() && !qt_separator(chars[p - 1]) {
            p -= 1;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::NativeWords as WordRules;

    // pin 1's text (conformance/pins/1-word-move.json)
    const PIN1: &str = "foo  bar, baz.qux CamelCase snake_case 42abc 3.14 日本語テキスト x😀y end";

    fn walk(r: WordRules, forward: bool) -> Vec<usize> {
        let n = PIN1.chars().count();
        let mut at = if forward { 0 } else { n };
        let mut stops = Vec::new();
        for _ in 0..16 {
            at = if forward {
                r.next(PIN1, at)
            } else {
                r.prev(PIN1, at)
            };
            stops.push(at);
        }
        stops
    }

    #[test]
    fn fx1_words_match_gtk_golden() {
        // conformance/golden/linux/gtk-4.24.1.json, pin word-move
        assert_eq!(
            walk(WordRules::Pango, true),
            [3, 8, 13, 17, 27, 33, 38, 44, 46, 49, 53, 57, 59, 61, 65, 65]
        );
        assert_eq!(
            walk(WordRules::Pango, false),
            [62, 60, 58, 53, 50, 47, 45, 39, 34, 28, 18, 14, 10, 5, 0, 0]
        );
    }

    #[test]
    fn fx1_words_match_qt_golden() {
        // conformance/golden/linux/qt-6.12.0.json, pin word-move
        assert_eq!(
            walk(WordRules::Qt, true),
            [5, 8, 10, 13, 14, 18, 28, 39, 45, 46, 47, 50, 58, 62, 65, 65]
        );
        assert_eq!(
            walk(WordRules::Qt, false),
            [62, 58, 50, 47, 46, 45, 39, 28, 18, 14, 13, 10, 8, 5, 0, 0]
        );
    }
}
