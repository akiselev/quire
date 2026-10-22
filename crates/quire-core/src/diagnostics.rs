//! Diagnostics in, per source and version (ARCH-QUIRE §4.2).
//!
//! Each [`SourceId`] replaces its own set atomically through
//! [`Document::set_diagnostics`](crate::Document::set_diagnostics). Sets computed at an older
//! version are mapped forward through the recent changes; older than the window they are refused
//! as [`Stale`](crate::Stale). Stored diagnostics are mapped through every later edit: a range's
//! start maps with [`Bias::After`] and its end with [`Bias::Before`], so text typed at either edge
//! does not join it; a range whose text is deleted collapses to an empty range at the deletion.

use crate::change::{Bias, ChangeSet};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

/// Severity, as LSP.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Severity {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// Information.
    Info,
    /// A hint.
    Hint,
}

/// Where diagnostics come from: `"svelters"`, `"tsgo"`, `"oxc"`, `"runtime"`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SourceId(pub Arc<str>);

impl SourceId {
    /// A source id.
    pub fn new(s: &str) -> Self {
        SourceId(Arc::from(s))
    }

    /// The id as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SourceId {
    fn from(s: &str) -> Self {
        SourceId::new(s)
    }
}

/// Diagnostic tags, as LSP.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DiagTags {
    /// Unused or unnecessary code (rendered faded).
    pub unnecessary: bool,
    /// Deprecated (rendered struck through).
    pub deprecated: bool,
}

/// An action a diagnostic offers (e.g. "Grant…"). The editor never interprets it: choosing it
/// emits the command id and arguments for the host's registry.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DiagAction {
    /// Display text.
    pub title: String,
    /// The host command id.
    pub command_id: String,
    /// Arguments as JSON text (kept as text so quire-core needs no JSON dependency).
    pub args_json: String,
}

/// One diagnostic.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Diagnostic {
    /// Byte range at the version passed to `set_diagnostics` (after that, at the document's
    /// current version).
    pub range: Range<usize>,
    /// Severity.
    pub severity: Severity,
    /// The message (host-localised or the server's text).
    pub message: String,
    /// A code such as `"unknown_element"`.
    pub code: Option<String>,
    /// The host's stable id, for Problems ↔ editor sync.
    pub id: Option<String>,
    /// Tags.
    pub tags: DiagTags,
    /// Related ranges with messages.
    pub related: Vec<(Range<usize>, String)>,
    /// Actions.
    pub actions: Vec<DiagAction>,
}

impl Diagnostic {
    /// A diagnostic with just a range, severity and message.
    pub fn new(range: Range<usize>, severity: Severity, message: impl Into<String>) -> Self {
        Diagnostic {
            range,
            severity,
            message: message.into(),
            code: None,
            id: None,
            tags: DiagTags::default(),
            related: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub(crate) fn map(&mut self, c: &ChangeSet) {
        self.range = map_range(&self.range, c);
        for (r, _) in &mut self.related {
            *r = map_range(r, c);
        }
    }

    pub(crate) fn clamp(&mut self, len: usize) {
        self.range.end = self.range.end.min(len);
        self.range.start = self.range.start.min(self.range.end);
    }
}

/// Maps a range so that insertions at its edges stay outside it.
pub(crate) fn map_range(r: &Range<usize>, c: &ChangeSet) -> Range<usize> {
    if r.start == r.end {
        let p = c.map_pos(r.start, Bias::Before);
        return p..p;
    }
    let s = c.map_pos(r.start, Bias::After);
    let e = c.map_pos(r.end, Bias::Before).max(s);
    s..e
}

/// All diagnostics of a document, by source, at the document's current version.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    by_source: BTreeMap<SourceId, Arc<Vec<Diagnostic>>>,
    generation: u64,
}

impl Diagnostics {
    /// The diagnostics of one source.
    pub fn of(&self, source: &SourceId) -> &[Diagnostic] {
        self.by_source.get(source).map_or(&[], |v| v.as_slice())
    }

    /// Every source with its diagnostics, in source order.
    pub fn iter(&self) -> impl Iterator<Item = (&SourceId, &[Diagnostic])> {
        self.by_source.iter().map(|(k, v)| (k, v.as_slice()))
    }

    /// Every diagnostic, in source order.
    pub fn all(&self) -> impl Iterator<Item = (&SourceId, &Diagnostic)> {
        self.by_source
            .iter()
            .flat_map(|(k, v)| v.iter().map(move |d| (k, d)))
    }

    /// Diagnostics overlapping a byte range (an empty diagnostic touching it counts).
    pub fn in_range(&self, r: Range<usize>) -> impl Iterator<Item = (&SourceId, &Diagnostic)> {
        self.all()
            .filter(move |(_, d)| d.range.start <= r.end && d.range.end >= r.start)
    }

    /// Total count.
    pub fn len(&self) -> usize {
        self.by_source.values().map(|v| v.len()).sum()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bumps whenever any set changes (replaced or mapped), for render caches.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn replace(&mut self, source: SourceId, diags: Vec<Diagnostic>) {
        if diags.is_empty() {
            self.by_source.remove(&source);
        } else {
            self.by_source.insert(source, Arc::new(diags));
        }
        self.generation += 1;
    }

    pub(crate) fn map(&mut self, c: &ChangeSet) {
        if self.by_source.is_empty() || c.is_empty() {
            return;
        }
        for v in self.by_source.values_mut() {
            for d in Arc::make_mut(v).iter_mut() {
                d.map(c);
            }
        }
        self.generation += 1;
    }
}
