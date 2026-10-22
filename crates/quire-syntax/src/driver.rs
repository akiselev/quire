//! [`SyntaxDriver`]: keeps a [`Document`]'s tokens fresh with background parse jobs.
//!
//! Generic over any [`Highlighter`]; it lives here rather than in quire-core because quire-core
//! documents `quire_core::syntax` as the seam only.
//!
//! - **Debounce:** after the text changes, the driver waits `debounce_ms` of quiet before it
//!   starts a parse (the previous tokens are shifted by the document meanwhile, so nothing
//!   flickers).
//! - **Jobs:** each job parses a [`Snapshot`] (incrementally from the last tree and the changes
//!   since it) and highlights the visible lines, on the host's [`Spawner`]. The driver never
//!   creates threads; a synchronous spawner runs the job inline.
//! - **Results** apply through [`Document::set_tokens`], which takes them as they are if the
//!   version still matches and shifts them by whole lines through the recent changes if not
//!   (stale beyond the window: dropped, and the next job recomputes).
//! - **Settled:** while a debounce or a job is pending the driver holds a token in the
//!   document's [`PendingWork`](quire_core::syntax::PendingWork), so `Document::is_settled` and
//!   its `SettledProbe` report busy until the result is applied.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use quire_core::syntax::{CancelFlag, Highlighter, PendingToken, Spawner, TokenRuns};
use quire_core::{Document, Version};

/// Default quiet time before a parse starts after an edit.
pub const DEFAULT_DEBOUNCE_MS: u64 = 30;
/// Lines highlighted beyond each side of the visible range.
pub const DEFAULT_MARGIN_LINES: usize = 100;

struct Done<H: Highlighter> {
    version: Version,
    tree: Option<Arc<H::Tree>>,
    runs: TokenRuns,
    _pending: PendingToken,
}

type Slot<H> = Arc<Mutex<Option<Done<H>>>>;

/// Drives one highlighter for one document. Call [`SyntaxDriver::update`] once per frame (or
/// whenever the document or the visible lines change, and when the waker fires).
pub struct SyntaxDriver<H: Highlighter> {
    highlighter: Arc<H>,
    spawner: Arc<dyn Spawner>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    debounce_ms: u64,
    margin: usize,
    /// The newest tree and the version it was parsed at.
    tree: Option<(Arc<H::Tree>, Version)>,
    /// Lines highlighted at `covered_at` (merged ranges).
    covered: Vec<Range<usize>>,
    covered_at: Option<Version>,
    /// The version the debounce is waiting on, when it started, and its pending token.
    debounce: Option<(Version, u64, PendingToken)>,
    in_flight: Option<(Version, CancelFlag, Slot<H>)>,
}

impl<H: Highlighter> std::fmt::Debug for SyntaxDriver<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntaxDriver")
            .field("language", &self.highlighter.language())
            .field("tree_version", &self.tree.as_ref().map(|t| t.1))
            .field("debouncing", &self.debounce.as_ref().map(|d| d.0))
            .field("in_flight", &self.in_flight.as_ref().map(|j| j.0))
            .finish()
    }
}

impl<H: Highlighter> SyntaxDriver<H> {
    /// A driver running jobs on `spawner`.
    pub fn new(highlighter: H, spawner: Arc<dyn Spawner>) -> Self {
        SyntaxDriver {
            highlighter: Arc::new(highlighter),
            spawner,
            waker: None,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            margin: DEFAULT_MARGIN_LINES,
            tree: None,
            covered: Vec::new(),
            covered_at: None,
            debounce: None,
            in_flight: None,
        }
    }

    /// Sets the quiet time before a parse starts (0: parse on the next update).
    pub fn with_debounce(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    /// Sets how many lines beyond the visible range each job highlights.
    pub fn with_margin(mut self, lines: usize) -> Self {
        self.margin = lines;
        self
    }

    /// Called from the job's thread when a result is ready (e.g. `egui::Context::request_repaint`).
    pub fn set_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.waker = Some(waker);
    }

    /// The highlighter.
    pub fn highlighter(&self) -> &H {
        &self.highlighter
    }

    /// The newest tree and the version it was parsed at (it may lag the document).
    pub fn tree(&self) -> Option<(&Arc<H::Tree>, Version)> {
        self.tree.as_ref().map(|(t, v)| (t, *v))
    }

    /// The newest tree if it was parsed at `version`.
    pub fn tree_at(&self, version: Version) -> Option<&Arc<H::Tree>> {
        self.tree
            .as_ref()
            .filter(|(_, v)| *v == version)
            .map(|(t, _)| t)
    }

    /// Whether a debounce or a job is pending (also visible through the document's settled
    /// probe).
    pub fn is_pending(&self) -> bool {
        self.debounce.is_some() || self.in_flight.is_some()
    }

    /// The earliest time (ms, the clock `update` gets) at which `update` has new work to start,
    /// so a host can schedule a frame; `None` if nothing is waiting on the clock.
    pub fn next_deadline(&self) -> Option<u64> {
        self.debounce
            .as_ref()
            .map(|(_, since, _)| since + self.debounce_ms)
    }

    /// Applies finished results to `doc`, then starts a job if the text changed (after the
    /// debounce) or `visible` lines lack tokens at the current version. `now_ms` is any
    /// monotonic clock.
    pub fn update(&mut self, doc: &mut Document, visible: Range<usize>, now_ms: u64) {
        self.poll(doc);
        let version = doc.version();
        let lines = doc.text().len_lines();
        let want = visible.start.saturating_sub(self.margin).min(lines)
            ..(visible.end + self.margin).min(lines);
        let text_changed = self.tree.as_ref().is_none_or(|(_, v)| *v != version);
        let uncovered = self.covered_at != Some(version)
            || !self
                .covered
                .iter()
                .any(|r| r.start <= visible.start.min(lines) && r.end >= visible.end.min(lines));
        if !text_changed && !uncovered {
            self.debounce = None;
            return;
        }
        if self.in_flight.is_some() {
            return;
        }
        if text_changed && self.tree.is_some() && self.debounce_ms > 0 {
            match &self.debounce {
                Some((v, since, _)) if *v == version => {
                    if now_ms < since + self.debounce_ms {
                        return;
                    }
                }
                _ => {
                    self.debounce = Some((version, now_ms, doc.pending_work().begin()));
                    return;
                }
            }
        }
        self.debounce = None;
        self.spawn(doc, want, text_changed);
        // A synchronous spawner has already finished the job.
        self.poll(doc);
    }

    fn poll(&mut self, doc: &mut Document) {
        let done = match &self.in_flight {
            Some((_, _, slot)) => slot.lock().expect("syntax job slot").take(),
            None => return,
        };
        let Some(done) = done else { return };
        self.in_flight = None;
        if let Some(tree) = done.tree
            && self.tree.as_ref().is_none_or(|(_, v)| *v <= done.version)
        {
            self.tree = Some((tree, done.version));
        }
        let range = done.runs.line_range();
        if doc.set_tokens(done.version, done.runs).is_ok() && done.version == doc.version() {
            if self.covered_at != Some(done.version) {
                self.covered.clear();
                self.covered_at = Some(done.version);
            }
            self.covered.push(range);
            self.covered = merge(std::mem::take(&mut self.covered));
        }
    }

    fn spawn(&mut self, doc: &Document, lines: Range<usize>, reparse: bool) {
        let snap = doc.snapshot();
        let version = snap.version;
        // Incremental from the newest tree when the changes since it are still in the window.
        let base = self.tree.as_ref().map(|(t, v)| (t.clone(), *v));
        let prev = match &base {
            Some((t, v)) if *v == version => Some((t.clone(), None)),
            Some((t, v)) => doc.recent().composed_since(*v).ok().map(|c| (t.clone(), c)),
            None => None,
        };
        let cancel = CancelFlag::new();
        let slot: Slot<H> = Arc::new(Mutex::new(None));
        let pending = doc.pending_work().begin();
        let h = self.highlighter.clone();
        let (job_cancel, job_slot, waker) = (cancel.clone(), slot.clone(), self.waker.clone());
        self.in_flight = Some((version, cancel, slot));
        let job = move || {
            let (tree, fresh) = match prev {
                // Same version: only highlight more lines with the existing tree.
                Some((t, None)) if !reparse => (Some(t), false),
                Some((t, changes)) => {
                    let changes = changes
                        .unwrap_or_else(|| quire_core::ChangeSet::identity(snap.text.len_bytes()));
                    match h.parse(&snap, Some((&t, &changes)), &job_cancel) {
                        Some(n) => (Some(Arc::new(n)), true),
                        None => (None, false),
                    }
                }
                None => match h.parse(&snap, None, &job_cancel) {
                    Some(n) => (Some(Arc::new(n)), true),
                    None => (None, false),
                },
            };
            let runs = match &tree {
                Some(t) => h.highlights(t, &snap, lines),
                None => TokenRuns::default(),
            };
            let tree = if fresh { tree } else { None };
            *job_slot.lock().expect("syntax job slot") = Some(Done {
                version,
                tree,
                runs,
                _pending: pending,
            });
            if let Some(w) = waker {
                w();
            }
        };
        self.spawner.spawn(Box::new(job));
    }
}

impl<H: Highlighter> Drop for SyntaxDriver<H> {
    fn drop(&mut self) {
        if let Some((_, cancel, _)) = &self.in_flight {
            cancel.cancel();
        }
    }
}

fn merge(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(v.len());
    for r in v {
        match out.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}
