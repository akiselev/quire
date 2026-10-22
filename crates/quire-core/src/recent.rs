//! [`RecentChanges`]: the last N change sets, for mapping data computed at older versions
//! (diagnostics, tokens, anchors, views) forward, or refusing it as [`Stale`].

use crate::change::ChangeSet;
use crate::text::Text;
use crate::version::Version;
use std::collections::VecDeque;

/// Data computed at a version older than the recent-changes window (or newer than the
/// document). The host recomputes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Stale {
    /// The version the data was computed at.
    pub at: Version,
    /// The oldest version the window can map from.
    pub oldest: Version,
    /// The document's current version.
    pub current: Version,
}

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "data at {} is stale (window {}..={})",
            self.at, self.oldest, self.current
        )
    }
}
impl std::error::Error for Stale {}

/// One edit expressed in old line/column coordinates, enough to shift line-based data (tokens,
/// folds) without the old text.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LineEdit {
    /// First replaced line (old text).
    pub start_line: usize,
    /// Byte column on `start_line`.
    pub start_col: usize,
    /// Last replaced line (old text).
    pub end_line: usize,
    /// Byte column on `end_line`.
    pub end_col: usize,
    /// Line feeds in the inserted text.
    pub inserted_lines: usize,
    /// Bytes after the inserted text's last line feed (its whole length if it has none).
    pub inserted_last_len: usize,
    /// Bytes of the inserted text's first line (before its first line feed).
    pub inserted_first_len: usize,
}

impl LineEdit {
    /// The line edits of `changes` against `before`, in base order.
    pub fn compute(changes: &ChangeSet, before: &Text) -> Vec<LineEdit> {
        changes
            .edits()
            .iter()
            .map(|e| {
                let (sl, sc) = before.byte_to_line_col(e.range.start);
                let (el, ec) = before.byte_to_line_col(e.range.end);
                let inserted_lines = e.insert.bytes().filter(|&b| b == b'\n').count();
                let inserted_last_len = match e.insert.rfind('\n') {
                    Some(i) => e.insert.len() - i - 1,
                    None => e.insert.len(),
                };
                let inserted_first_len = e.insert.find('\n').unwrap_or(e.insert.len());
                LineEdit {
                    start_line: sl,
                    start_col: sc,
                    end_line: el,
                    end_col: ec,
                    inserted_lines,
                    inserted_last_len,
                    inserted_first_len,
                }
            })
            .collect()
    }

    /// Lines added (negative: removed).
    pub fn line_delta(&self) -> isize {
        self.inserted_lines as isize - (self.end_line - self.start_line) as isize
    }
}

/// One recorded change: from version `from` to `from.next()`.
#[derive(Clone, Debug)]
pub struct RecentChange {
    /// The version the change applies to.
    pub from: Version,
    /// The change.
    pub changes: ChangeSet,
    /// Its line edits.
    pub line_edits: Vec<LineEdit>,
    /// Who made it.
    pub author: crate::AuthorId,
}

/// The last `capacity` change sets.
#[derive(Clone, Debug)]
pub struct RecentChanges {
    entries: VecDeque<RecentChange>,
    capacity: usize,
    current: Version,
}

impl RecentChanges {
    /// An empty window at `current`.
    pub fn new(capacity: usize, current: Version) -> Self {
        RecentChanges {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            current,
        }
    }

    /// The window size.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The oldest version data can be mapped from.
    pub fn oldest(&self) -> Version {
        self.entries.front().map_or(self.current, |e| e.from)
    }

    /// The current version.
    pub fn current(&self) -> Version {
        self.current
    }

    pub(crate) fn push(
        &mut self,
        changes: ChangeSet,
        line_edits: Vec<LineEdit>,
        author: crate::AuthorId,
    ) {
        let from = self.current;
        self.current = from.next();
        self.entries.push_back(RecentChange {
            from,
            changes,
            line_edits,
            author,
        });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    fn check(&self, at: Version) -> Result<usize, Stale> {
        let stale = Stale {
            at,
            oldest: self.oldest(),
            current: self.current,
        };
        if at > self.current || at < self.oldest() {
            return Err(stale);
        }
        Ok(self.entries.len() - (self.current.0 - at.0) as usize)
    }

    /// The changes from `at` to now, oldest first (empty when `at` is current).
    pub fn since(&self, at: Version) -> Result<impl Iterator<Item = &RecentChange>, Stale> {
        let i = self.check(at)?;
        Ok(self.entries.iter().skip(i))
    }

    /// The changes from `at` to now composed into one change set (`None` when `at` is current).
    pub fn composed_since(&self, at: Version) -> Result<Option<ChangeSet>, Stale> {
        let mut acc: Option<ChangeSet> = None;
        for e in self.since(at)? {
            acc = Some(match acc {
                None => e.changes.clone(),
                Some(a) => a.compose(&e.changes).expect("recent changes chain"),
            });
        }
        Ok(acc)
    }

    /// Maps a position from version `at` to now.
    pub fn map_pos(&self, at: Version, pos: usize, bias: crate::Bias) -> Result<usize, Stale> {
        let mut p = pos;
        for e in self.since(at)? {
            p = e.changes.map_pos(p, bias);
        }
        Ok(p)
    }
}
