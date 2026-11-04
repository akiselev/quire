//! Versioned, source-backed display replacements. Source remains the only editable authority.
use crate::{Bias, ChangeSet, Text, Version};
use std::ops::Range;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub id: u64,
    pub range: Range<usize>,
    pub text: String,
}
#[derive(Clone, Debug, Default)]
pub struct Projections {
    version: Version,
    generation: u64,
    entries: Vec<Replacement>,
}
impl Projections {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn entries(&self) -> &[Replacement] {
        &self.entries
    }
    pub fn replace(
        &mut self,
        at: Version,
        current: Version,
        text: &Text,
        mut entries: Vec<Replacement>,
    ) -> Result<(), String> {
        if at != current {
            return Err("projection targets an obsolete source version".into());
        }
        entries.sort_by_key(|r| r.range.start);
        let source = text.to_string();
        let mut end = 0;
        let mut ids = std::collections::BTreeSet::new();
        for entry in &entries {
            if entry.range.start < end
                || entry.range.start >= entry.range.end
                || entry.range.end > source.len()
                || !source.is_char_boundary(entry.range.start)
                || !source.is_char_boundary(entry.range.end)
                || !ids.insert(entry.id)
            {
                return Err("invalid, overlapping, or duplicate projection range".into());
            }
            end = entry.range.end;
        }
        if self.entries != entries {
            self.generation += 1;
        }
        self.version = current;
        self.entries = entries;
        Ok(())
    }
    pub(crate) fn map(&mut self, changes: &ChangeSet, version: Version) {
        let edits: Vec<_> = changes.edits();
        self.entries.retain_mut(|r| {
            // An edit inside a replacement invalidates its semantic presentation. Untouched ranges shift.
            if edits.iter().any(|e| {
                if e.range.is_empty() {
                    e.range.start > r.range.start && e.range.start < r.range.end
                } else {
                    e.range.start < r.range.end && e.range.end > r.range.start
                }
            }) {
                return false;
            }
            r.range = changes.map_pos(r.range.start, Bias::After)
                ..changes.map_pos(r.range.end, Bias::Before);
            !r.range.is_empty()
        });
        self.version = version;
        self.generation += 1;
    }
    pub fn map_source(&self, pos: usize, after: bool) -> usize {
        let mut delta = 0isize;
        for r in &self.entries {
            if pos < r.range.start {
                break;
            }
            if pos < r.range.end {
                return (r.range.start as isize + delta) as usize
                    + if after { r.text.len() } else { 0 };
            }
            delta += r.text.len() as isize - r.range.len() as isize;
        }
        pos.saturating_add_signed(delta)
    }
    pub fn map_display(&self, pos: usize, after: bool) -> usize {
        let mut delta = 0isize;
        for r in &self.entries {
            let start = r.range.start.saturating_add_signed(delta);
            let end = start + r.text.len();
            if pos < start {
                break;
            }
            if pos < end || (pos == start && r.text.is_empty()) {
                return if after { r.range.end } else { r.range.start };
            }
            delta += r.text.len() as isize - r.range.len() as isize;
        }
        pos.saturating_add_signed(-delta)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapped_ranges_invalidate_and_do_not_expire() {
        let mut p = Projections::default();
        let text = Text::from("ab [[id]] xy");
        p.replace(
            Version(0),
            Version(0),
            &text,
            vec![Replacement {
                id: 1,
                range: 3..9,
                text: "answer".into(),
            }],
        )
        .unwrap();
        assert_eq!(p.map_source(5, false), 3);
        assert_eq!(p.map_source(5, true), 9);
        assert_eq!(p.map_display(5, true), 9);
        for v in 1..100 {
            let c = ChangeSet::replace(text.len_bytes() + v - 1, [(0..0, "x")]).unwrap();
            p.map(&c, Version(v as u64));
        }
        assert_eq!(p.entries()[0].range, 102..108);
        let c = ChangeSet::replace(text.len_bytes() + 99, [(104..105, "")]).unwrap();
        p.map(&c, Version(100));
        assert!(p.entries().is_empty());
    }
    #[test]
    fn stale_overlap_and_unicode_rejected() {
        let mut p = Projections::default();
        let text = Text::from("éabc");
        assert!(p.replace(Version(0), Version(1), &text, vec![]).is_err());
        assert!(
            p.replace(
                Version(0),
                Version(0),
                &text,
                vec![Replacement {
                    id: 1,
                    range: 1..3,
                    text: "x".into()
                }]
            )
            .is_err()
        );
    }
}
