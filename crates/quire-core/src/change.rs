//! [`ChangeSet`]: sorted, non-overlapping edits against one base length (Helix's model, with
//! CodeMirror 6's mapping rules).
//!
//! A change set is a list of operations that walks the old text from start to end:
//! `Retain(n)` keeps `n` bytes, `Delete(n)` drops `n` bytes, `Insert(s)` inserts `s`. The list is
//! kept normalised: no empty operations, no two adjacent operations of the same kind, and where an
//! insertion and a deletion meet, the insertion comes first. Two equal edits therefore always have
//! equal operation lists.
//!
//! Laws (proptested in `tests/changeset_laws.rs`):
//! - `a.compose(b).apply(t) == b.apply(a.apply(t))`, and `compose` is associative;
//! - `a.invert(t).apply(a.apply(t)) == t`;
//! - `map_pos` is monotone for a fixed bias;
//! - `a.compose(b.map(a)) == b.compose(a.map(b))` applied to the same text when `a` goes first at
//!   ties (the transform used for remote changes and history rebasing).

use crate::text::Text;
use std::ops::Range;
use std::sync::Arc;

/// Which side of an insertion at exactly its position a mapped position lands on, and where a
/// position inside a replaced range goes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Bias {
    /// Stay before text inserted at the position; inside a replaced range, go to the start of
    /// the replacement.
    #[default]
    Before,
    /// Move after text inserted at the position; inside a replaced range, go to the end of the
    /// replacement.
    After,
}

/// One operation of a [`ChangeSet`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Operation {
    /// Keep this many bytes.
    Retain(usize),
    /// Drop this many bytes.
    Delete(usize),
    /// Insert this text.
    Insert(Arc<str>),
}

/// Why a change set could not be built or combined.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChangeError {
    /// An edit range reaches past the base length.
    OutOfBounds {
        /// The offending range.
        range: Range<usize>,
        /// The base length.
        len: usize,
    },
    /// Two edit ranges overlap.
    Overlap {
        /// The first range.
        first: Range<usize>,
        /// The second range.
        second: Range<usize>,
    },
    /// `compose` or `map` got change sets whose lengths do not line up.
    LengthMismatch {
        /// The length the second change set needed.
        expected: usize,
        /// The length it had.
        got: usize,
    },
    /// A split fell inside a UTF-8 sequence of inserted text.
    NotCharBoundary,
}

impl std::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeError::OutOfBounds { range, len } => {
                write!(f, "edit {range:?} is outside the text (length {len})")
            }
            ChangeError::Overlap { first, second } => {
                write!(f, "edits {first:?} and {second:?} overlap")
            }
            ChangeError::LengthMismatch { expected, got } => {
                write!(f, "change set length {got} does not match {expected}")
            }
            ChangeError::NotCharBoundary => f.write_str("split inside a UTF-8 sequence"),
        }
    }
}
impl std::error::Error for ChangeError {}

/// One edit of a change set in base coordinates: replace `range` with `insert`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Edit<'a> {
    /// The replaced byte range in the old text.
    pub range: Range<usize>,
    /// The inserted text.
    pub insert: &'a str,
}

/// Sorted, non-overlapping edits against one base length. See the module docs for the laws.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangeSet {
    ops: Vec<Operation>,
    len: usize,
    len_after: usize,
}

impl ChangeSet {
    /// The identity change set for a text of `len` bytes.
    pub fn identity(len: usize) -> Self {
        let mut c = ChangeSet::with_len(len);
        c.retain(len);
        c
    }

    fn with_len(len: usize) -> Self {
        ChangeSet {
            ops: Vec::new(),
            len,
            len_after: 0,
        }
    }

    /// A change set from edits `(range, replacement)` against a text of `base_len` bytes.
    ///
    /// Edits are sorted by start (stably, so several insertions at one position keep their
    /// order). Overlapping ranges and ranges past `base_len` are errors. Char boundaries are
    /// checked by [`Document::apply`](crate::Document::apply), which has the text.
    pub fn replace<S: AsRef<str>>(
        base_len: usize,
        edits: impl IntoIterator<Item = (Range<usize>, S)>,
    ) -> Result<Self, ChangeError> {
        let mut edits: Vec<(Range<usize>, S)> = edits.into_iter().collect();
        edits.sort_by_key(|(r, _)| r.start);
        let mut c = ChangeSet::with_len(base_len);
        let mut pos = 0;
        let mut prev: Option<Range<usize>> = None;
        for (r, s) in &edits {
            if r.start > r.end || r.end > base_len {
                return Err(ChangeError::OutOfBounds {
                    range: r.clone(),
                    len: base_len,
                });
            }
            if r.start < pos {
                return Err(ChangeError::Overlap {
                    first: prev.clone().unwrap_or(0..0),
                    second: r.clone(),
                });
            }
            c.retain(r.start - pos);
            c.insert(s.as_ref());
            c.delete(r.end - r.start);
            pos = r.end;
            prev = Some(r.clone());
        }
        c.retain(base_len - pos);
        Ok(c)
    }

    /// A single insertion of `text` at `pos`.
    pub fn insert_at(base_len: usize, pos: usize, text: &str) -> Result<Self, ChangeError> {
        ChangeSet::replace(base_len, [(pos..pos, text)])
    }

    /// A single deletion of `range`.
    pub fn delete_range(base_len: usize, range: Range<usize>) -> Result<Self, ChangeError> {
        ChangeSet::replace(base_len, [(range, "")])
    }

    // ---- builders (normalising) ----

    fn retain(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.len_after += n;
        if let Some(Operation::Retain(m)) = self.ops.last_mut() {
            *m += n;
        } else {
            self.ops.push(Operation::Retain(n));
        }
    }

    fn delete(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        if let Some(Operation::Delete(m)) = self.ops.last_mut() {
            *m += n;
        } else {
            self.ops.push(Operation::Delete(n));
        }
    }

    fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.len_after += s.len();
        let n = self.ops.len();
        match self.ops.last_mut() {
            Some(Operation::Insert(prev)) => {
                *prev = concat(prev, s);
            }
            Some(Operation::Delete(_)) => {
                // Keep insert-before-delete order.
                if n >= 2
                    && let Operation::Insert(prev) = &mut self.ops[n - 2]
                {
                    *prev = concat(prev, s);
                    return;
                }
                self.ops.insert(n - 1, Operation::Insert(Arc::from(s)));
            }
            _ => self.ops.push(Operation::Insert(Arc::from(s))),
        }
    }

    fn insert_arc(&mut self, s: &Arc<str>) {
        if matches!(self.ops.last(), None | Some(Operation::Retain(_))) && !s.is_empty() {
            self.len_after += s.len();
            self.ops.push(Operation::Insert(s.clone()));
        } else {
            self.insert(s);
        }
    }

    // ---- queries ----

    /// The operations, normalised.
    pub fn ops(&self) -> &[Operation] {
        &self.ops
    }

    /// The length of the text this change set applies to.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The length of the text after applying.
    pub fn len_after(&self) -> usize {
        self.len_after
    }

    /// Whether the change set changes nothing.
    pub fn is_empty(&self) -> bool {
        self.ops.iter().all(|o| matches!(o, Operation::Retain(_)))
    }

    /// The edits in base coordinates, in order. An insertion followed by a deletion at the same
    /// place is one replacing edit.
    pub fn edits(&self) -> Vec<Edit<'_>> {
        let mut out = Vec::new();
        let mut pos = 0;
        let mut i = 0;
        while i < self.ops.len() {
            match &self.ops[i] {
                Operation::Retain(n) => pos += n,
                Operation::Delete(n) => {
                    out.push(Edit {
                        range: pos..pos + n,
                        insert: "",
                    });
                    pos += n;
                }
                Operation::Insert(s) => {
                    let del = match self.ops.get(i + 1) {
                        Some(Operation::Delete(n)) => {
                            i += 1;
                            *n
                        }
                        _ => 0,
                    };
                    out.push(Edit {
                        range: pos..pos + del,
                        insert: s,
                    });
                    pos += del;
                }
            }
            i += 1;
        }
        out
    }

    // ---- algebra ----

    /// Applies the change set to `text`. Panics if `text.len_bytes() != self.len()`; use
    /// [`Document::apply`](crate::Document::apply), which checks, from outside the crate.
    pub(crate) fn apply_to(&self, text: &mut Text) {
        assert_eq!(text.len_bytes(), self.len, "change set length mismatch");
        let mut pos = 0;
        for op in &self.ops {
            match op {
                Operation::Retain(n) => pos += n,
                Operation::Delete(n) => text.remove(pos..pos + n),
                Operation::Insert(s) => {
                    text.insert(pos, s);
                    pos += s.len();
                }
            }
        }
    }

    /// Applies the change set to a copy of `text` and returns it. Checks the length.
    pub fn apply(&self, text: &Text) -> Result<Text, ChangeError> {
        if text.len_bytes() != self.len {
            return Err(ChangeError::LengthMismatch {
                expected: text.len_bytes(),
                got: self.len,
            });
        }
        let mut t = text.clone();
        self.apply_to(&mut t);
        Ok(t)
    }

    /// The change set that undoes `self`, given the text `self` applies to.
    pub fn invert(&self, before: &Text) -> ChangeSet {
        let mut out = ChangeSet::with_len(self.len_after);
        let mut pos = 0;
        for op in &self.ops {
            match op {
                Operation::Retain(n) => {
                    out.retain(*n);
                    pos += n;
                }
                Operation::Delete(n) => {
                    out.insert(&before.slice(pos..pos + n).to_cow());
                    pos += n;
                }
                Operation::Insert(s) => out.delete(s.len()),
            }
        }
        out
    }

    /// `self` then `next`, as one change set. `next.len()` must equal `self.len_after()`.
    pub fn compose(&self, next: &ChangeSet) -> Result<ChangeSet, ChangeError> {
        if next.len != self.len_after {
            return Err(ChangeError::LengthMismatch {
                expected: self.len_after,
                got: next.len,
            });
        }
        let mut out = ChangeSet::with_len(self.len);
        let mut a = OpCursor::new(&self.ops);
        let mut b = OpCursor::new(&next.ops);
        loop {
            match (a.peek(), b.peek()) {
                (None, None) => break,
                (Some(Cur::Delete(n)), _) => {
                    out.delete(n);
                    a.advance(n);
                }
                (_, Some(Cur::Insert(s, off))) => {
                    out.insert(&s[off..]);
                    b.next_op();
                }
                (Some(Cur::Retain(i)), Some(Cur::Retain(j))) => {
                    let k = i.min(j);
                    out.retain(k);
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Retain(i)), Some(Cur::Delete(j))) => {
                    let k = i.min(j);
                    out.delete(k);
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Insert(s, off)), Some(Cur::Retain(j))) => {
                    let rest = &s[off..];
                    let k = rest.len().min(j);
                    let part = rest.get(..k).ok_or(ChangeError::NotCharBoundary)?;
                    if k == rest.len() && off == 0 {
                        out.insert_arc(s);
                    } else {
                        out.insert(part);
                    }
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Insert(s, off)), Some(Cur::Delete(j))) => {
                    let rest = &s[off..];
                    let k = rest.len().min(j);
                    if !rest.is_char_boundary(k) {
                        return Err(ChangeError::NotCharBoundary);
                    }
                    a.advance(k);
                    b.advance(k);
                }
                (None, Some(_)) | (Some(_), None) => {
                    return Err(ChangeError::LengthMismatch {
                        expected: self.len_after,
                        got: next.len,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Rebases `self` over `other` (both apply to the same text): the result applies to the text
    /// after `other` and makes the edits `self` made. Insertions at the same position: `self`'s
    /// go first when `self_first`, else `other`'s. Text `self` deletes that `other` already
    /// deleted is not deleted twice; text `other` inserted is kept.
    pub fn map(&self, other: &ChangeSet, self_first: bool) -> Result<ChangeSet, ChangeError> {
        if other.len != self.len {
            return Err(ChangeError::LengthMismatch {
                expected: self.len,
                got: other.len,
            });
        }
        let mut out = ChangeSet::with_len(other.len_after);
        let mut a = OpCursor::new(&self.ops);
        let mut b = OpCursor::new(&other.ops);
        loop {
            let (pa, pb) = (a.peek(), b.peek());
            let a_ins = matches!(pa, Some(Cur::Insert(..)));
            let b_ins = matches!(pb, Some(Cur::Insert(..)));
            if a_ins && (self_first || !b_ins) {
                if let Some(Cur::Insert(s, off)) = pa {
                    out.insert(&s[off..]);
                }
                a.next_op();
                continue;
            }
            if b_ins {
                if let Some(Cur::Insert(s, off)) = pb {
                    out.retain(s.len() - off);
                }
                b.next_op();
                continue;
            }
            match (pa, pb) {
                (None, None) => break,
                (Some(Cur::Retain(i)), Some(Cur::Retain(j))) => {
                    let k = i.min(j);
                    out.retain(k);
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Delete(i)), Some(Cur::Delete(j))) => {
                    let k = i.min(j);
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Delete(i)), Some(Cur::Retain(j))) => {
                    let k = i.min(j);
                    out.delete(k);
                    a.advance(k);
                    b.advance(k);
                }
                (Some(Cur::Retain(i)), Some(Cur::Delete(j))) => {
                    let k = i.min(j);
                    a.advance(k);
                    b.advance(k);
                }
                _ => {
                    return Err(ChangeError::LengthMismatch {
                        expected: self.len,
                        got: other.len,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Maps a position in the old text to the new text. Positions past the end clamp to the
    /// end. See [`Bias`] for insertions at the position and positions inside replaced ranges.
    pub fn map_pos(&self, pos: usize, bias: Bias) -> usize {
        let mut old = 0;
        let mut new = 0;
        let mut i = 0;
        while i < self.ops.len() {
            match &self.ops[i] {
                Operation::Retain(n) => {
                    if pos < old + n {
                        return new + (pos - old);
                    }
                    old += n;
                    new += n;
                }
                Operation::Delete(n) => {
                    if pos < old + n {
                        return new;
                    }
                    old += n;
                }
                Operation::Insert(s) => {
                    let len = s.len();
                    let del = match self.ops.get(i + 1) {
                        Some(Operation::Delete(n)) => *n,
                        _ => 0,
                    };
                    if pos == old {
                        if del > 0 && pos < old + del && bias == Bias::After {
                            return new + len;
                        }
                        if bias == Bias::Before {
                            return new;
                        }
                    } else if pos > old && pos < old + del {
                        return match bias {
                            Bias::Before => new,
                            Bias::After => new + len,
                        };
                    }
                    new += len;
                }
            }
            i += 1;
        }
        new
    }

    /// The change set that turns `old` into `new`, computed with imara-diff (histogram, by line),
    /// then trimmed to the changed bytes within each changed hunk, so carets outside changed text
    /// keep their place. Used for reloads (`apply_remote(.., Origin::Reload)`).
    pub fn diff(old: &Text, new: &Text) -> ChangeSet {
        use imara_diff::{Algorithm, Diff, InternedInput};
        let a = old.to_string();
        let b = new.to_string();
        let input = InternedInput::new(a.as_str(), b.as_str());
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        // Line start offsets for both sides (lines include their '\n', as imara-diff's tokens).
        let starts = |s: &str| {
            let mut v = vec![0usize];
            for (i, c) in s.bytes().enumerate() {
                if c == b'\n' {
                    v.push(i + 1);
                }
            }
            if *v.last().unwrap_or(&0) != s.len() {
                v.push(s.len());
            }
            v
        };
        let sa = starts(&a);
        let sb = starts(&b);
        let line_byte = |s: &Vec<usize>, i: u32, total: usize| -> usize {
            s.get(i as usize).copied().unwrap_or(total)
        };
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        for h in diff.hunks() {
            let mut a0 = line_byte(&sa, h.before.start, a.len());
            let mut a1 = line_byte(&sa, h.before.end, a.len());
            let mut b0 = line_byte(&sb, h.after.start, b.len());
            let mut b1 = line_byte(&sb, h.after.end, b.len());
            // Trim the common prefix and suffix inside the hunk (on char boundaries).
            let ab = a.as_bytes();
            let bb = b.as_bytes();
            while a0 < a1 && b0 < b1 && ab[a0] == bb[b0] {
                a0 += 1;
                b0 += 1;
            }
            while !a.is_char_boundary(a0) || !b.is_char_boundary(b0) {
                a0 -= 1;
                b0 -= 1;
            }
            while a1 > a0 && b1 > b0 && ab[a1 - 1] == bb[b1 - 1] {
                a1 -= 1;
                b1 -= 1;
            }
            while !a.is_char_boundary(a1) || !b.is_char_boundary(b1) {
                a1 += 1;
                b1 += 1;
            }
            edits.push((a0..a1, b[b0..b1].to_string()));
        }
        ChangeSet::replace(a.len(), edits).unwrap_or_else(|_| {
            ChangeSet::replace(a.len(), [(0..a.len(), b.as_str())]).expect("whole replace")
        })
    }
}

fn concat(a: &str, b: &str) -> Arc<str> {
    let mut s = String::with_capacity(a.len() + b.len());
    s.push_str(a);
    s.push_str(b);
    Arc::from(s)
}

#[derive(Clone, Copy)]
enum Cur<'a> {
    Retain(usize),
    Delete(usize),
    /// The insertion and how many bytes of it are consumed.
    Insert(&'a Arc<str>, usize),
}

/// Walks an operation list with partial consumption.
struct OpCursor<'a> {
    ops: &'a [Operation],
    i: usize,
    used: usize,
}

impl<'a> OpCursor<'a> {
    fn new(ops: &'a [Operation]) -> Self {
        OpCursor { ops, i: 0, used: 0 }
    }
    fn peek(&self) -> Option<Cur<'a>> {
        self.ops.get(self.i).map(|op| match op {
            Operation::Retain(n) => Cur::Retain(n - self.used),
            Operation::Delete(n) => Cur::Delete(n - self.used),
            Operation::Insert(s) => Cur::Insert(s, self.used),
        })
    }
    fn next_op(&mut self) {
        self.i += 1;
        self.used = 0;
    }
    fn advance(&mut self, k: usize) {
        let total = match &self.ops[self.i] {
            Operation::Retain(n) | Operation::Delete(n) => *n,
            Operation::Insert(s) => s.len(),
        };
        self.used += k;
        if self.used >= total {
            self.next_op();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Text {
        Text::from_file(s)
    }

    #[test]
    fn replace_and_apply() {
        let text = t("hello world");
        let c = ChangeSet::replace(11, [(0..5, "goodbye"), (11..11, "!")]).unwrap();
        assert_eq!(c.apply(&text).unwrap(), "goodbye world!");
        assert_eq!(c.len_after(), 14);
        assert_eq!(
            c.edits(),
            vec![
                Edit {
                    range: 0..5,
                    insert: "goodbye"
                },
                Edit {
                    range: 11..11,
                    insert: "!"
                }
            ]
        );
    }

    #[test]
    fn normalised_insert_before_delete() {
        let c = ChangeSet::replace(3, [(0..3, "x")]).unwrap();
        assert_eq!(
            c.ops(),
            &[Operation::Insert("x".into()), Operation::Delete(3)]
        );
    }

    #[test]
    fn overlap_and_bounds() {
        assert!(matches!(
            ChangeSet::replace(5, [(0..3, ""), (2..4, "")]),
            Err(ChangeError::Overlap { .. })
        ));
        assert!(matches!(
            ChangeSet::replace(5, [(4..6, "")]),
            Err(ChangeError::OutOfBounds { .. })
        ));
        // two insertions at one position keep their order
        let c = ChangeSet::replace(1, [(1..1, "a"), (1..1, "b")]).unwrap();
        assert_eq!(c.apply(&t("x")).unwrap(), "xab");
    }

    #[test]
    fn invert_restores() {
        let text = t("abcdef");
        let c = ChangeSet::replace(6, [(1..3, "XYZ"), (5..6, "")]).unwrap();
        let after = c.apply(&text).unwrap();
        assert_eq!(after, "aXYZde");
        assert_eq!(c.invert(&text).apply(&after).unwrap(), "abcdef");
    }

    #[test]
    fn compose_simple() {
        let text = t("abc");
        let a = ChangeSet::insert_at(3, 1, "XX").unwrap(); // aXXbc
        let b = ChangeSet::delete_range(5, 2..4).unwrap(); // aXc
        let ab = a.compose(&b).unwrap();
        assert_eq!(ab.apply(&text).unwrap(), "aXc");
        assert!(a.compose(&a).is_err());
    }

    #[test]
    fn map_pos_rules() {
        // insert "XY" at 2 in "abcd"
        let c = ChangeSet::insert_at(4, 2, "XY").unwrap();
        assert_eq!(c.map_pos(1, Bias::Before), 1);
        assert_eq!(c.map_pos(2, Bias::Before), 2);
        assert_eq!(c.map_pos(2, Bias::After), 4);
        assert_eq!(c.map_pos(3, Bias::Before), 5);
        assert_eq!(c.map_pos(4, Bias::After), 6);
        assert_eq!(c.map_pos(99, Bias::After), 6);
        // replace 1..3 of "abcd" with "XYZ"
        let c = ChangeSet::replace(4, [(1..3, "XYZ")]).unwrap();
        assert_eq!(c.map_pos(1, Bias::Before), 1);
        assert_eq!(c.map_pos(1, Bias::After), 4);
        assert_eq!(c.map_pos(2, Bias::Before), 1);
        assert_eq!(c.map_pos(2, Bias::After), 4);
        assert_eq!(c.map_pos(3, Bias::Before), 4);
        // pure deletion
        let c = ChangeSet::delete_range(4, 1..3).unwrap();
        assert_eq!(c.map_pos(2, Bias::After), 1);
        assert_eq!(c.map_pos(3, Bias::Before), 1);
    }

    #[test]
    fn map_transform_converges() {
        let text = t("abcdef");
        let a = ChangeSet::replace(6, [(1..3, "X"), (6..6, "!")]).unwrap();
        let b = ChangeSet::replace(6, [(2..5, "YY"), (6..6, "?")]).unwrap();
        let a2 = a.map(&b, true).unwrap();
        let b2 = b.map(&a, false).unwrap();
        let left = a2.apply(&b.apply(&text).unwrap()).unwrap();
        let right = b2.apply(&a.apply(&text).unwrap()).unwrap();
        assert_eq!(left, right);
        assert_eq!(left.to_string(), "aXYYf!?");
    }

    #[test]
    fn diff_keeps_unchanged_text() {
        let old = t("line one\nline two\nline three\n");
        let new = t("line one\nline 2\nline three\nfour\n");
        let c = ChangeSet::diff(&old, &new);
        assert_eq!(c.apply(&old).unwrap(), new);
        // a caret on line three stays on line three
        let p = old.line_col_to_byte(2, 3);
        let q = c.map_pos(p, Bias::Before);
        assert_eq!(new.byte_to_line_col(q), (2, 3));
        let unchanged = ChangeSet::diff(&old, &old);
        assert!(unchanged.is_empty());
    }
}
