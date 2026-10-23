//! Sparse tokens over ranges of lines (VS Code `SparseMultilineTokens` and `SparseTokensStore`,
//! `editor/common/tokens/sparseMultilineTokens.ts` and `sparseTokensStore.ts` at
//! microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`, translated literally).
//!
//! As in VS Code, line numbers are 1-based (an emptied piece reports line 0 to -1) and
//! characters are 0-based (bytes here, VS Code: UTF-16 units). Token data is `u32` with
//! VS Code's wrapping arithmetic. A token's metadata is opaque to the store; merging it over
//! the base tokens of a line takes a mask function ([`SparseTokensStore::add_sparse_tokens`]).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

/// A range of lines and characters: 1-based lines, 0-based characters (VS Code's `Range` with
/// `column - 1`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct LineRange {
    /// First line (1-based).
    pub start_line: i64,
    /// Character on the first line (0-based).
    pub start_char: u32,
    /// Last line (1-based).
    pub end_line: i64,
    /// Character on the last line (0-based).
    pub end_char: u32,
}

impl LineRange {
    /// A range.
    pub fn new(start_line: i64, start_char: u32, end_line: i64, end_char: u32) -> Self {
        LineRange {
            start_line,
            start_char,
            end_line,
            end_char,
        }
    }

    fn is_empty(&self) -> bool {
        self.start_line == self.end_line && self.start_char == self.end_char
    }

    /// `Range.plusRange`.
    fn plus(&self, o: &LineRange) -> LineRange {
        let (sl, sc) = if o.start_line < self.start_line
            || (o.start_line == self.start_line && o.start_char < self.start_char)
        {
            (o.start_line, o.start_char)
        } else {
            (self.start_line, self.start_char)
        };
        let (el, ec) = if o.end_line > self.end_line
            || (o.end_line == self.end_line && o.end_char > self.end_char)
        {
            (o.end_line, o.end_char)
        } else {
            (self.end_line, self.end_char)
        };
        LineRange::new(sl, sc, el, ec)
    }
}

/// One edit as `SparseMultilineTokens.acceptEdit` takes it: the replaced range, the inserted
/// text's line feeds, the length of its first and last line, and its first character.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TokenEdit {
    /// The replaced range.
    pub range: LineRange,
    /// Line feeds inserted.
    pub eol_count: u32,
    /// Length of the inserted text's first line.
    pub first_line_length: u32,
    /// Length of the inserted text's last line.
    pub last_line_length: u32,
    /// The inserted text's first character (`'\0'` when nothing is inserted).
    pub first_char: char,
}

impl TokenEdit {
    /// `applyEdit(range, text)`: the edit replacing `range` with `text` (`countEOL`).
    pub fn new(range: LineRange, text: &str) -> Self {
        let eol_count = text.matches('\n').count() as u32;
        let first = text.split('\n').next().unwrap_or("");
        let last = text.rsplit('\n').next().unwrap_or("");
        TokenEdit {
            range,
            eol_count,
            first_line_length: first.len() as u32,
            last_line_length: if eol_count == 0 {
                first.len() as u32
            } else {
                last.len() as u32
            },
            first_char: text.chars().next().unwrap_or('\0'),
        }
    }
}

/// Sparse tokens over a contiguous range of lines (VS Code `SparseMultilineTokens`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SparseMultilineTokens {
    start_line: i64,
    end_line: i64,
    tokens: Storage,
}

impl SparseMultilineTokens {
    /// Tokens from `start_line` (1-based): `data` is `(delta line, start, end, metadata)` per
    /// token.
    pub fn create(start_line: i64, data: Vec<u32>) -> Self {
        let tokens = Storage::new(data);
        let end_line = start_line + tokens.max_delta_line();
        SparseMultilineTokens {
            start_line,
            end_line,
            tokens,
        }
    }

    /// First line (inclusive).
    pub fn start_line(&self) -> i64 {
        self.start_line
    }

    /// Last line (inclusive).
    pub fn end_line(&self) -> i64 {
        self.end_line
    }

    fn update_end_line(&mut self) {
        self.end_line = self.start_line + self.tokens.max_delta_line();
    }

    /// Whether no token is left.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The tokens of `line` as `(start, end, metadata)`.
    pub fn line_tokens(&self, line: i64) -> Option<Vec<(u32, u32, u32)>> {
        if self.start_line <= line && line <= self.end_line {
            return self.tokens.line_tokens(line - self.start_line);
        }
        None
    }

    /// `getRange`.
    pub fn range(&self) -> Option<LineRange> {
        let r = self.tokens.range()?;
        Some(LineRange::new(
            self.start_line + r.start_line,
            r.start_char,
            self.start_line + r.end_line,
            r.end_char,
        ))
    }

    /// `removeTokens`: drops the tokens touching `range`.
    pub fn remove_tokens(&mut self, range: &LineRange) {
        let s = range.start_line - self.start_line;
        let e = range.end_line - self.start_line;
        self.start_line += self
            .tokens
            .remove_tokens(s, range.start_char, e, range.end_char);
        self.update_end_line();
    }

    /// `split`: the tokens before `range` and those after it.
    pub fn split(&self, range: &LineRange) -> (SparseMultilineTokens, SparseMultilineTokens) {
        let s = range.start_line - self.start_line;
        let e = range.end_line - self.start_line;
        let (a, b, b_delta) = self.tokens.split(s, range.start_char, e, range.end_char);
        (
            SparseMultilineTokens::with(self.start_line, a),
            SparseMultilineTokens::with(self.start_line + b_delta, b),
        )
    }

    fn with(start_line: i64, tokens: Storage) -> Self {
        let end_line = start_line + tokens.max_delta_line();
        SparseMultilineTokens {
            start_line,
            end_line,
            tokens,
        }
    }

    /// `acceptEdit`.
    pub fn accept_edit(&mut self, e: &TokenEdit) {
        self.accept_delete_range(&e.range);
        self.accept_insert_text(
            e.range.start_line,
            e.range.start_char,
            e.eol_count,
            e.first_line_length,
            e.last_line_length,
            e.first_char,
        );
        self.update_end_line();
    }

    fn accept_delete_range(&mut self, range: &LineRange) {
        if range.is_empty() {
            return;
        }
        let first = range.start_line - self.start_line;
        let last = range.end_line - self.start_line;
        if last < 0 {
            // entirely before this block: only line numbers move
            let deleted = last - first;
            self.start_line -= deleted;
            return;
        }
        let max = self.tokens.max_delta_line();
        if first > max {
            return;
        }
        if first < 0 && last > max {
            // the deletion encompasses this block
            self.start_line = 0;
            self.tokens.clear();
            return;
        }
        if first < 0 {
            let deleted_before = -first;
            self.start_line -= deleted_before;
            self.tokens
                .accept_delete_range(range.start_char, 0, 0, last, range.end_char);
        } else {
            self.tokens
                .accept_delete_range(0, first, range.start_char, last, range.end_char);
        }
    }

    fn accept_insert_text(
        &mut self,
        line: i64,
        character: u32,
        eol_count: u32,
        first_len: u32,
        last_len: u32,
        first_char: char,
    ) {
        if eol_count == 0 && first_len == 0 {
            return;
        }
        let idx = line - self.start_line;
        if idx < 0 {
            self.start_line += eol_count as i64;
            return;
        }
        if idx > self.tokens.max_delta_line() {
            return;
        }
        self.tokens
            .accept_insert_text(idx, character, eol_count, first_len, last_len, first_char);
    }
}

/// `SparseMultilineTokensStorage`: `(delta line, start, end, metadata)` per token.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Storage {
    tokens: Vec<u32>,
    count: usize,
}

impl Storage {
    fn new(tokens: Vec<u32>) -> Self {
        let count = tokens.len() / 4;
        Storage { tokens, count }
    }

    fn max_delta_line(&self) -> i64 {
        if self.count == 0 {
            return -1;
        }
        self.delta_line(self.count - 1) as i64
    }

    fn range(&self) -> Option<LineRange> {
        if self.count == 0 {
            return None;
        }
        Some(LineRange::new(
            0,
            self.start(0),
            self.delta_line(self.count - 1) as i64,
            self.end(self.count - 1),
        ))
    }

    fn delta_line(&self, i: usize) -> u32 {
        self.tokens[4 * i]
    }

    fn start(&self, i: usize) -> u32 {
        self.tokens[4 * i + 1]
    }

    fn end(&self, i: usize) -> u32 {
        self.tokens[4 * i + 2]
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn line_tokens(&self, delta_line: i64) -> Option<Vec<(u32, u32, u32)>> {
        if self.count == 0 || delta_line < 0 {
            return None;
        }
        let dl = |i: usize| self.delta_line(i) as i64;
        let pick = |a: usize, b: usize| {
            Some(
                (a..=b)
                    .map(|i| (self.start(i), self.end(i), self.tokens[4 * i + 3]))
                    .collect(),
            )
        };
        let mut low: isize = 0;
        let mut high: isize = self.count as isize - 1;
        while low < high {
            let mid = low + (high - low) / 2;
            let m = dl(mid as usize);
            if m < delta_line {
                low = mid + 1;
            } else if m > delta_line {
                high = mid - 1;
            } else {
                let mut min = mid;
                while min > low && dl(min as usize - 1) == delta_line {
                    min -= 1;
                }
                let mut max = mid;
                while max < high && dl(max as usize + 1) == delta_line {
                    max += 1;
                }
                return pick(min as usize, max as usize);
            }
        }
        if low >= 0 && (low as usize) < self.count && dl(low as usize) == delta_line {
            return pick(low as usize, low as usize);
        }
        None
    }

    fn clear(&mut self) {
        self.count = 0;
    }

    fn remove_tokens(&mut self, s_line: i64, s_char: u32, e_line: i64, e_char: u32) -> i64 {
        let t = &mut self.tokens;
        let mut new_count = 0;
        let mut deleted = false;
        let mut first_delta: u32 = 0;
        for i in 0..self.count {
            let src = 4 * i;
            let dl = t[src];
            let st = t[src + 1];
            let en = t[src + 2];
            let md = t[src + 3];
            let dli = dl as i64;
            if (dli > s_line || (dli == s_line && en >= s_char))
                && (dli < e_line || (dli == e_line && st <= e_char))
            {
                deleted = true;
            } else {
                if new_count == 0 {
                    first_delta = dl;
                }
                if deleted {
                    let dst = 4 * new_count;
                    t[dst] = dl.wrapping_sub(first_delta);
                    t[dst + 1] = st;
                    t[dst + 2] = en;
                    t[dst + 3] = md;
                } else if first_delta != 0 {
                    t[src] = dl.wrapping_sub(first_delta);
                }
                new_count += 1;
            }
        }
        self.count = new_count;
        first_delta as i64
    }

    fn split(&self, s_line: i64, s_char: u32, e_line: i64, e_char: u32) -> (Storage, Storage, i64) {
        let t = &self.tokens;
        let mut a: Vec<u32> = Vec::new();
        let mut b: Vec<u32> = Vec::new();
        let mut to_b = false;
        let mut dest_first: u32 = 0;
        for i in 0..self.count {
            let src = 4 * i;
            let dl = t[src];
            let st = t[src + 1];
            let en = t[src + 2];
            let md = t[src + 3];
            let dli = dl as i64;
            if dli > s_line || (dli == s_line && en >= s_char) {
                if dli < e_line || (dli == e_line && st <= e_char) {
                    // touching the range
                    continue;
                } else if !to_b {
                    // the first token after the range
                    to_b = true;
                    dest_first = dl;
                }
            }
            let dest = if to_b { &mut b } else { &mut a };
            dest.extend([dl.wrapping_sub(dest_first), st, en, md]);
        }
        (Storage::new(a), Storage::new(b), dest_first as i64)
    }

    fn accept_delete_range(
        &mut self,
        horizontal_shift: u32,
        s_line: i64,
        s_char: u32,
        e_line: i64,
        e_char: u32,
    ) {
        let t = &mut self.tokens;
        let count = self.count;
        let deleted_lines = e_line - s_line;
        let mut new_count = 0;
        let mut deleted = false;
        let mut i = 0;
        while i < count {
            let src = 4 * i;
            i += 1;
            let mut dl = t[src] as i64;
            let mut st = t[src + 1];
            let mut en = t[src + 2];
            let md = t[src + 3];
            if dl < s_line || (dl == s_line && en <= s_char) {
                // 1a. before the deletion
                new_count += 1;
                continue;
            } else if dl == s_line && st < s_char {
                // 1b, 1c, 1d: the token survives, shrunk
                if dl == e_line && en > e_char {
                    en = en.wrapping_sub(e_char.wrapping_sub(s_char));
                } else {
                    en = s_char;
                }
            } else if dl == s_line && st == s_char {
                // 2a, 2b, 2c
                if dl == e_line && en > e_char {
                    en = en.wrapping_sub(e_char.wrapping_sub(s_char));
                } else {
                    deleted = true;
                    continue;
                }
            } else if dl < e_line || (dl == e_line && st < e_char) {
                // 3a, 3b, 3c
                if dl == e_line && en > e_char {
                    dl = s_line;
                    st = s_char;
                    en = st.wrapping_add(en.wrapping_sub(e_char));
                } else {
                    deleted = true;
                    continue;
                }
            } else if dl > e_line {
                // 4. after the deletion, on a later line
                if deleted_lines == 0 && !deleted {
                    new_count = count;
                    break;
                }
                dl -= deleted_lines;
            } else if dl == e_line && st >= e_char {
                // 4. after the deletion, on its last line
                if horizontal_shift != 0 && dl == 0 {
                    st = st.wrapping_add(horizontal_shift);
                    en = en.wrapping_add(horizontal_shift);
                }
                dl -= deleted_lines;
                st = st.wrapping_sub(e_char.wrapping_sub(s_char));
                en = en.wrapping_sub(e_char.wrapping_sub(s_char));
            } else {
                unreachable!("Not possible!");
            }
            let dst = 4 * new_count;
            t[dst] = dl as u32;
            t[dst + 1] = st;
            t[dst + 2] = en;
            t[dst + 3] = md;
            new_count += 1;
        }
        self.count = new_count;
    }

    fn accept_insert_text(
        &mut self,
        delta_line: i64,
        character: u32,
        eol_count: u32,
        first_len: u32,
        last_len: u32,
        first_char: char,
    ) {
        let one_word_char = eol_count == 0 && first_len == 1 && first_char.is_ascii_alphanumeric();
        let t = &mut self.tokens;
        for i in 0..self.count {
            let o = 4 * i;
            let mut dl = t[o] as i64;
            let mut st = t[o + 1];
            let mut en = t[o + 2];
            if dl < delta_line || (dl == delta_line && en < character) {
                // 1. before the insertion point
                continue;
            } else if dl == delta_line && en == character {
                // 2. ends at the insertion point: grows by one word character only
                if one_word_char {
                    en = en.wrapping_add(1);
                } else {
                    continue;
                }
            } else if dl == delta_line && st < character && character < en {
                // 3. contains the insertion point
                if eol_count == 0 {
                    en = en.wrapping_add(first_len);
                } else {
                    en = character;
                }
            } else {
                // 4. or 5.
                if dl == delta_line && st == character && one_word_char {
                    // 4. starts at the insertion point: grows (keeps its start)
                    continue;
                }
                if dl == delta_line {
                    dl += eol_count as i64;
                    if eol_count == 0 {
                        st = st.wrapping_add(first_len);
                        en = en.wrapping_add(first_len);
                    } else {
                        let len = en.wrapping_sub(st);
                        st = last_len.wrapping_add(st.wrapping_sub(character));
                        en = st.wrapping_add(len);
                    }
                } else {
                    dl += eol_count as i64;
                }
            }
            t[o] = dl as u32;
            t[o + 1] = st;
            t[o + 2] = en;
        }
    }
}

/// Sparse tokens of a document (VS Code `SparseTokensStore`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SparseTokensStore {
    pieces: Vec<SparseMultilineTokens>,
    complete: bool,
}

impl SparseTokensStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// `flush`.
    pub fn flush(&mut self) {
        self.pieces.clear();
        self.complete = false;
    }

    /// Whether no piece is held.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// The pieces.
    pub fn pieces(&self) -> &[SparseMultilineTokens] {
        &self.pieces
    }

    /// `set`: replaces every piece; `complete`: they cover the whole document.
    pub fn set(&mut self, pieces: Option<Vec<SparseMultilineTokens>>, complete: bool) {
        self.pieces = pieces.unwrap_or_default();
        self.complete = complete;
    }

    /// `isComplete`.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// `setPartial`: `pieces` replace the tokens in `range` (grown to cover them); returns the
    /// range replaced.
    pub fn set_partial(
        &mut self,
        range: LineRange,
        pieces: Vec<SparseMultilineTokens>,
    ) -> LineRange {
        let mut range = range;
        if let (Some(first), Some(last)) = (pieces.first(), pieces.last()) {
            let (Some(fr), Some(lr)) = (first.range(), last.range()) else {
                return range;
            };
            range = range.plus(&fr).plus(&lr);
        }
        let mut insert: Option<usize> = None;
        let mut i = 0;
        while i < self.pieces.len() {
            let piece = &mut self.pieces[i];
            if piece.end_line() < range.start_line {
                i += 1;
                continue;
            }
            if piece.start_line() > range.end_line {
                insert = insert.or(Some(i));
                break;
            }
            piece.remove_tokens(&range);
            if piece.is_empty() {
                self.pieces.remove(i);
                continue;
            }
            if piece.end_line() < range.start_line {
                i += 1;
                continue;
            }
            if piece.start_line() > range.end_line {
                insert = insert.or(Some(i));
                i += 1;
                continue;
            }
            let (a, b) = piece.split(&range);
            if a.is_empty() {
                insert = insert.or(Some(i));
                i += 1;
                continue;
            }
            if b.is_empty() {
                i += 1;
                continue;
            }
            self.pieces.splice(i..=i, [a, b]);
            i += 1;
            insert = insert.or(Some(i));
            i += 1;
        }
        let at = insert.unwrap_or(self.pieces.len());
        if !pieces.is_empty() {
            self.pieces.splice(at..at, pieces);
        }
        range
    }

    /// `addSparseTokens`: the base tokens of `line` (`(end offset, metadata)`, sorted, the last
    /// ending at the line's length) with the sparse tokens over them. Where a sparse token
    /// lies, the bits `mask(sparse metadata)` come from it and the rest from the base token.
    pub fn add_sparse_tokens(
        &self,
        line: i64,
        a: &[(u32, u32)],
        mask: impl Fn(u32) -> u32,
    ) -> Vec<(u32, u32)> {
        let text_len = a.last().map_or(0, |t| t.0);
        if text_len == 0 || self.pieces.is_empty() {
            return a.to_vec();
        }
        let piece = Self::first_piece_with_line(&self.pieces, line);
        let Some(b) = self.pieces[piece].line_tokens(line) else {
            return a.to_vec();
        };
        let a_len = a.len();
        let a_start = |i: usize| if i == 0 { 0 } else { a[i - 1].0 };
        let mut ai = 0usize;
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut last_end = 0u32;
        let mut emit = |out: &mut Vec<(u32, u32)>, end: u32, md: u32| {
            if end <= last_end {
                return;
            }
            last_end = end;
            out.push((end, md));
        };
        for &(bs, be, bm) in &b {
            let bs = bs.min(text_len);
            let be = be.min(text_len);
            let b_mask = mask(bm);
            let a_mask = !b_mask;
            while ai < a_len && a[ai].0 <= bs {
                emit(&mut out, a[ai].0, a[ai].1);
                ai += 1;
            }
            if ai < a_len && a_start(ai) < bs {
                emit(&mut out, bs, a[ai].1);
            }
            while ai < a_len && a[ai].0 < be {
                emit(&mut out, a[ai].0, (a[ai].1 & a_mask) | (bm & b_mask));
                ai += 1;
            }
            if ai < a_len {
                emit(&mut out, be, (a[ai].1 & a_mask) | (bm & b_mask));
                if a[ai].0 == be {
                    ai += 1;
                }
            } else {
                let merge = ai.saturating_sub(1).min(a_len - 1);
                emit(&mut out, be, (a[merge].1 & a_mask) | (bm & b_mask));
            }
        }
        while ai < a_len {
            emit(&mut out, a[ai].0, a[ai].1);
            ai += 1;
        }
        out
    }

    fn first_piece_with_line(pieces: &[SparseMultilineTokens], line: i64) -> usize {
        let mut low: isize = 0;
        let mut high: isize = pieces.len() as isize - 1;
        while low < high {
            let mut mid = low + (high - low) / 2;
            let p = &pieces[mid as usize];
            if p.end_line() < line {
                low = mid + 1;
            } else if p.start_line() > line {
                high = mid - 1;
            } else {
                while mid > low
                    && pieces[mid as usize - 1].start_line() <= line
                    && line <= pieces[mid as usize - 1].end_line()
                {
                    mid -= 1;
                }
                return mid as usize;
            }
        }
        low.max(0) as usize
    }

    /// `acceptEdit`: every piece follows the edit; emptied pieces go.
    pub fn accept_edit(&mut self, e: &TokenEdit) {
        let mut i = 0;
        while i < self.pieces.len() {
            self.pieces[i].accept_edit(e);
            if self.pieces[i].is_empty() {
                self.pieces.remove(i);
            } else {
                i += 1;
            }
        }
    }
}
