//! [`Selection`]: one or more [`SelRange`]s with a primary one (multi-cursor in the type from day
//! one).
//!
//! Ranges are kept sorted by start and non-overlapping: [`Selection::new`] merges ranges that
//! overlap, and cursors (empty ranges) at the same position, keeping the primary on the merged
//! range. Two non-empty ranges that only touch stay separate (VS Code's rule).

use crate::change::{Bias, ChangeSet};
use crate::text::Text;
use smallvec::SmallVec;

/// One selection range. `anchor` is where the selection started, `head` where the caret is.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SelRange {
    /// The fixed end, a byte offset.
    pub anchor: usize,
    /// The caret end, a byte offset.
    pub head: usize,
    /// The visual column (tabs expanded) vertical motion tries to keep. Cleared by every motion
    /// other than up/down.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub goal_column: Option<u32>,
}

impl SelRange {
    /// A range from `anchor` to `head`.
    pub fn new(anchor: usize, head: usize) -> Self {
        SelRange {
            anchor,
            head,
            goal_column: None,
        }
    }

    /// A cursor (empty range) at `pos`.
    pub fn point(pos: usize) -> Self {
        SelRange::new(pos, pos)
    }

    /// The smaller end.
    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// The larger end.
    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// `from()..to()`.
    pub fn range(&self) -> std::ops::Range<usize> {
        self.from()..self.to()
    }

    /// Whether the range is a cursor.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Whether the head is before the anchor (a backwards selection).
    pub fn is_backward(&self) -> bool {
        self.head < self.anchor
    }

    /// The same range with a goal column.
    #[must_use]
    pub fn with_goal(mut self, goal: Option<u32>) -> Self {
        self.goal_column = goal;
        self
    }

    /// Maps the range through a change set (CodeMirror 6's rule): a cursor maps with `cursor_bias`;
    /// a non-empty range maps its start with [`Bias::After`] and its end with [`Bias::Before`], so
    /// text inserted at its edges does not join it. The goal column is dropped if the range moved.
    #[must_use]
    pub fn map(&self, changes: &ChangeSet, cursor_bias: Bias) -> SelRange {
        let (anchor, head) = if self.is_empty() {
            let p = changes.map_pos(self.head, cursor_bias);
            (p, p)
        } else {
            let from = changes.map_pos(self.from(), Bias::After);
            let to = changes.map_pos(self.to(), Bias::Before).max(from);
            if self.is_backward() {
                (to, from)
            } else {
                (from, to)
            }
        };
        let moved = anchor != self.anchor || head != self.head;
        SelRange {
            anchor,
            head,
            goal_column: if moved { None } else { self.goal_column },
        }
    }

    /// Whether both ends are inside the text and on char boundaries.
    pub fn is_valid_in(&self, text: &Text) -> bool {
        text.is_char_boundary(self.anchor) && text.is_char_boundary(self.head)
    }

    /// The range clamped into the text, both ends rounded down to char boundaries.
    #[must_use]
    pub fn clamped(&self, text: &Text) -> SelRange {
        SelRange {
            anchor: text.clamp_to_boundary(self.anchor),
            head: text.clamp_to_boundary(self.head),
            goal_column: self.goal_column,
        }
    }
}

/// One or more ranges, sorted and non-overlapping, with a primary range.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Selection {
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<SelRange>"))]
    ranges: SmallVec<[SelRange; 1]>,
    primary: usize,
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Selection {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            ranges: Vec<SelRange>,
            #[serde(default)]
            primary: usize,
        }
        let raw = Raw::deserialize(d)?;
        if raw.ranges.is_empty() {
            return Err(serde::de::Error::custom(
                "a selection needs at least one range",
            ));
        }
        Ok(Selection::new(raw.ranges, raw.primary))
    }
}

impl Default for Selection {
    fn default() -> Self {
        Selection::point(0)
    }
}

impl Selection {
    /// A selection from ranges and the index of the primary one (clamped). Sorts and merges.
    /// Panics if `ranges` is empty.
    pub fn new(ranges: impl IntoIterator<Item = SelRange>, primary: usize) -> Self {
        let ranges: SmallVec<[SelRange; 1]> = ranges.into_iter().collect();
        assert!(!ranges.is_empty(), "a selection needs at least one range");
        let primary = primary.min(ranges.len() - 1);
        let mut s = Selection { ranges, primary };
        s.normalize();
        s
    }

    /// One range.
    pub fn single(anchor: usize, head: usize) -> Self {
        Selection {
            ranges: smallvec::smallvec![SelRange::new(anchor, head)],
            primary: 0,
        }
    }

    /// One cursor.
    pub fn point(pos: usize) -> Self {
        Selection::single(pos, pos)
    }

    /// One range from a [`SelRange`].
    pub fn from_range(r: SelRange) -> Self {
        Selection {
            ranges: smallvec::smallvec![r],
            primary: 0,
        }
    }

    fn normalize(&mut self) {
        if self.ranges.len() == 1 {
            return;
        }
        let primary_range = self.ranges[self.primary];
        let mut idx: SmallVec<[(SelRange, bool); 4]> = self
            .ranges
            .iter()
            .enumerate()
            .map(|(i, r)| (*r, i == self.primary))
            .collect();
        idx.sort_by_key(|(r, _)| (r.from(), r.to()));
        let mut out: SmallVec<[SelRange; 1]> = SmallVec::new();
        let mut primary = 0;
        for (r, is_p) in idx {
            if let Some(last) = out.last_mut() {
                let overlap = r.from() < last.to()
                    || (r.from() == last.to() && (r.is_empty() || last.is_empty()))
                    || (r.from() == last.from() && r.to() == last.to());
                if overlap {
                    let from = last.from().min(r.from());
                    let to = last.to().max(r.to());
                    let keep_dir = if is_p { r } else { *last };
                    *last = if keep_dir.is_backward() {
                        SelRange::new(to, from)
                    } else {
                        SelRange::new(from, to)
                    };
                    if from == to {
                        last.goal_column = keep_dir.goal_column;
                    }
                    if is_p {
                        primary = out.len() - 1;
                    }
                    continue;
                }
            }
            if is_p {
                primary = out.len();
            }
            out.push(r);
        }
        let _ = primary_range;
        self.ranges = out;
        self.primary = primary;
    }

    /// The ranges, sorted by start.
    pub fn ranges(&self) -> &[SelRange] {
        &self.ranges
    }

    /// The primary range's index.
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// The primary range.
    pub fn primary(&self) -> SelRange {
        self.ranges[self.primary]
    }

    /// Number of ranges.
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Always false (a selection has at least one range); here for clippy's `len` rule.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether every range is a cursor.
    pub fn all_cursors(&self) -> bool {
        self.ranges.iter().all(SelRange::is_empty)
    }

    /// Applies `f` to every range and renormalises.
    #[must_use]
    pub fn transform(&self, mut f: impl FnMut(SelRange) -> SelRange) -> Selection {
        Selection::new(self.ranges.iter().map(|r| f(*r)), self.primary)
    }

    /// Maps through a change set ([`SelRange::map`]) and renormalises.
    #[must_use]
    pub fn map(&self, changes: &ChangeSet, cursor_bias: Bias) -> Selection {
        self.transform(|r| r.map(changes, cursor_bias))
    }

    /// Only the primary range.
    #[must_use]
    pub fn into_primary(self) -> Selection {
        Selection::from_range(self.primary())
    }

    /// Adds a range and makes it primary.
    #[must_use]
    pub fn push(&self, r: SelRange) -> Selection {
        let mut v: Vec<SelRange> = self.ranges.to_vec();
        v.push(r);
        let p = v.len() - 1;
        Selection::new(v, p)
    }

    /// Whether every range is inside the text and on char boundaries.
    pub fn is_valid_in(&self, text: &Text) -> bool {
        self.ranges.iter().all(|r| r.is_valid_in(text))
    }

    /// Clamped into the text ([`SelRange::clamped`]) and renormalised.
    #[must_use]
    pub fn clamped(&self, text: &Text) -> Selection {
        self.transform(|r| r.clamped(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_tracks_primary() {
        let s = Selection::new(
            [
                SelRange::new(5, 8),
                SelRange::new(0, 2),
                SelRange::new(7, 10),
            ],
            2,
        );
        assert_eq!(s.ranges(), &[SelRange::new(0, 2), SelRange::new(5, 10)]);
        assert_eq!(s.primary_index(), 1);
        // touching non-empty ranges stay separate
        let s = Selection::new([SelRange::new(0, 2), SelRange::new(2, 4)], 0);
        assert_eq!(s.len(), 2);
        // cursors at one position merge
        let s = Selection::new([SelRange::point(3), SelRange::point(3)], 1);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn map_does_not_grow_at_edges() {
        let c = ChangeSet::replace(10, [(2..2, "ab"), (5..5, "cd")]).unwrap();
        let r = SelRange::new(2, 5).map(&c, Bias::Before);
        assert_eq!(r, SelRange::new(4, 7));
        let p = SelRange::point(2).map(&c, Bias::After);
        assert_eq!(p, SelRange::point(4));
        let back = SelRange::new(5, 2).map(&c, Bias::Before);
        assert_eq!(back, SelRange::new(7, 4));
    }
}
