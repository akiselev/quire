//! Undo history with Monaco's grouping rules, in two modes.
//!
//! **Grouping** (Monaco `editStack.ts` and `cursorTypeOperations.ts`): consecutive input edits
//! coalesce into one open group until one of these closes it:
//! - a word boundary, per [`InputKind::stop_between`] (`abc` + ` d` are two groups);
//! - a cursor jump: the acting view's selection before the edit differs from the selection the
//!   group's last edit left;
//! - a pause longer than `EditorConfig::undo_pause_ms` between `Transaction::time_ms` values (core
//!   reads no clock; [`Document::tick`](crate::Document::tick) closes a group after a pause too);
//! - a non-typing command (`Origin::Command`, and paste, IME commit and other input kinds: each
//!   its own group);
//! - a focus change ([`Document::close_undo_group`](crate::Document::close_undo_group), which
//!   the widget calls);
//! - [`GroupHint::Close`] / [`GroupHint::Isolated`](crate::GroupHint::Isolated);
//! - an undo, redo, remote or reload change.
//!
//! **Modes:** in [`UndoMode::Local`] the document keeps undo and redo stacks and the `undo` /
//! `redo` commands produce transactions from them. In [`UndoMode::Host`] the stacks stay empty;
//! every closed group is reported as an [`UndoGroupClosed`] (the host's `text.edit` boundary) and
//! `undo` / `redo` become host requests; the host answers with transactions whose origin is
//! `Undo` / `Redo`. Closed groups are reported in both modes.
//!
//! **Foreign changes** (remote, reload) are not undoable locally. The stacks are rebased over
//! them ([`ChangeSet::map`]), so local undo skips them (CodeMirror's per-origin history).

use crate::change::{Bias, ChangeSet};
use crate::selection::Selection;
use crate::text::Text;
use crate::transaction::{GroupHint, InputKind, Origin};
use crate::version::Version;
use smallvec::SmallVec;

/// Who owns undo.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum UndoMode {
    /// The document keeps its own undo and redo stacks (scratch editors, query boxes).
    #[default]
    Local,
    /// The host owns undo (CE3): closed groups are emitted, undo and redo are requests.
    Host,
}

/// A closed undo group, as reported to the host.
#[derive(Clone, PartialEq, Debug)]
pub struct UndoGroupClosed {
    /// The version before the group's first edit.
    pub from: Version,
    /// The version after its last edit.
    pub to: Version,
    /// The group's edits composed (applies at `from`).
    pub changes: ChangeSet,
    /// The inverse (applies at `to`): the host's undo transaction, if nothing happened since.
    pub inverse: ChangeSet,
    /// The acting view's selection before the group.
    pub selection_before: Selection,
    /// The acting view's selection after the group.
    pub selection_after: Selection,
}

/// An entry of the local undo or redo stack.
#[derive(Clone, PartialEq, Debug)]
pub struct UndoGroup {
    /// The change undo (or redo) applies; it applies to the text as it is once every entry above
    /// it on the same stack has been applied.
    pub change: ChangeSet,
    /// The selection to set after applying `change`.
    pub selection: Selection,
    /// The selection that was current before applying (the other end of the group).
    pub selection_other: Selection,
}

#[derive(Clone, Debug)]
struct OpenGroup {
    from: Version,
    to: Version,
    changes: ChangeSet,
    inverse: ChangeSet,
    selection_before: Selection,
    selection_after: Selection,
    last_kind: Option<InputKind>,
    last_time: Option<u64>,
}

/// The document's history.
#[derive(Clone, Debug, Default)]
pub struct History {
    mode: UndoMode,
    open: Option<OpenGroup>,
    undo: Vec<UndoGroup>,
    redo: Vec<UndoGroup>,
}

/// What one local edit brings to the history.
pub(crate) struct Record<'a> {
    pub origin: &'a Origin,
    pub group: GroupHint,
    pub time_ms: Option<u64>,
    pub pause_ms: u64,
    pub from: Version,
    pub to: Version,
    pub changes: &'a ChangeSet,
    pub inverse: &'a ChangeSet,
    pub selection_before: &'a Selection,
    pub selection_after: &'a Selection,
}

fn same_ranges(a: &Selection, b: &Selection) -> bool {
    a.len() == b.len()
        && a.ranges()
            .iter()
            .zip(b.ranges())
            .all(|(x, y)| x.anchor == y.anchor && x.head == y.head)
}

impl History {
    /// An empty history.
    pub fn new(mode: UndoMode) -> Self {
        History {
            mode,
            ..Default::default()
        }
    }

    /// The mode.
    pub fn mode(&self) -> UndoMode {
        self.mode
    }

    /// The local undo stack (top last). Empty in host mode.
    pub fn undo_stack(&self) -> &[UndoGroup] {
        &self.undo
    }

    /// The local redo stack (top last). Empty in host mode.
    pub fn redo_stack(&self) -> &[UndoGroup] {
        &self.redo
    }

    /// Whether a group is open (edits not yet reported as closed).
    pub fn has_open_group(&self) -> bool {
        self.open.is_some()
    }

    /// The version the open group started at.
    pub fn open_group_since(&self) -> Option<Version> {
        self.open.as_ref().map(|g| g.from)
    }

    /// Whether `undo` would do something (local mode; the open group counts).
    pub fn can_undo(&self) -> bool {
        self.mode == UndoMode::Local && (self.open.is_some() || !self.undo.is_empty())
    }

    /// Whether `redo` would do something (local mode).
    pub fn can_redo(&self) -> bool {
        self.mode == UndoMode::Local && self.open.is_none() && !self.redo.is_empty()
    }

    /// The change and selection the next undo applies (local mode).
    pub fn peek_undo(&self) -> Option<(&ChangeSet, &Selection)> {
        if self.mode != UndoMode::Local {
            return None;
        }
        if let Some(g) = &self.open {
            return Some((&g.inverse, &g.selection_before));
        }
        self.undo.last().map(|g| (&g.change, &g.selection))
    }

    /// The change and selection the next redo applies (local mode).
    pub fn peek_redo(&self) -> Option<(&ChangeSet, &Selection)> {
        if self.mode != UndoMode::Local || self.open.is_some() {
            return None;
        }
        self.redo.last().map(|g| (&g.change, &g.selection))
    }

    pub(crate) fn set_mode(&mut self, mode: UndoMode) -> Option<UndoGroupClosed> {
        let closed = self.close();
        self.mode = mode;
        self.undo.clear();
        self.redo.clear();
        closed
    }

    /// Closes the open group.
    pub(crate) fn close(&mut self) -> Option<UndoGroupClosed> {
        let g = self.open.take()?;
        if self.mode == UndoMode::Local {
            self.undo.push(UndoGroup {
                change: g.inverse.clone(),
                selection: g.selection_before.clone(),
                selection_other: g.selection_after.clone(),
            });
        }
        Some(UndoGroupClosed {
            from: g.from,
            to: g.to,
            changes: g.changes,
            inverse: g.inverse,
            selection_before: g.selection_before,
            selection_after: g.selection_after,
        })
    }

    /// Closes the open group if its last edit is older than `pause_ms` at `now_ms`.
    pub(crate) fn tick(&mut self, now_ms: u64, pause_ms: u64) -> Option<UndoGroupClosed> {
        let last = self.open.as_ref()?.last_time?;
        if now_ms.saturating_sub(last) > pause_ms {
            self.close()
        } else {
            None
        }
    }

    /// Records a local edit (origin `Input` or `Command`). Returns the groups it closed.
    pub(crate) fn record(&mut self, r: Record<'_>) -> SmallVec<[UndoGroupClosed; 1]> {
        let mut closed = SmallVec::new();
        let kind = match r.origin {
            Origin::Input(k) => Some(*k),
            _ => None,
        };
        let isolated = r.group == GroupHint::Isolated || kind.is_none_or(|k| !k.coalesces());
        if let Some(open) = &self.open {
            let stop = isolated
                || r.group == GroupHint::Close
                || match (open.last_kind, kind) {
                    (Some(prev), Some(k)) => prev.stop_between(k),
                    _ => true,
                }
                || !same_ranges(&open.selection_after, r.selection_before)
                || match (open.last_time, r.time_ms) {
                    (Some(a), Some(b)) => b.saturating_sub(a) > r.pause_ms,
                    _ => false,
                };
            if stop {
                closed.extend(self.close());
            }
        }
        self.redo.clear();
        match &mut self.open {
            Some(g) => {
                g.changes = g.changes.compose(r.changes).expect("group chain");
                g.inverse = r.inverse.compose(&g.inverse).expect("group inverse chain");
                g.to = r.to;
                g.selection_after = r.selection_after.clone();
                g.last_kind = kind;
                g.last_time = r.time_ms.or(g.last_time);
            }
            None => {
                self.open = Some(OpenGroup {
                    from: r.from,
                    to: r.to,
                    changes: r.changes.clone(),
                    inverse: r.inverse.clone(),
                    selection_before: r.selection_before.clone(),
                    selection_after: r.selection_after.clone(),
                    last_kind: kind,
                    last_time: r.time_ms,
                });
            }
        }
        if isolated {
            closed.extend(self.close());
        }
        closed
    }

    /// An undo transaction was applied (`text_before` is the text it applied to). Returns whether
    /// it matched the top of the stack (else the caller records it as an ordinary edit).
    pub(crate) fn applied_undo(&mut self, changes: &ChangeSet, text_before: &Text) -> bool {
        self.close();
        match self.undo.last() {
            Some(top) if top.change == *changes => {
                let top = self.undo.pop().expect("top");
                self.redo.push(UndoGroup {
                    change: top.change.invert(text_before),
                    selection: top.selection_other,
                    selection_other: top.selection,
                });
                true
            }
            _ => false,
        }
    }

    /// A redo transaction was applied.
    pub(crate) fn applied_redo(&mut self, changes: &ChangeSet, text_before: &Text) -> bool {
        self.close();
        match self.redo.last() {
            Some(top) if top.change == *changes => {
                let top = self.redo.pop().expect("top");
                self.undo.push(UndoGroup {
                    change: top.change.invert(text_before),
                    selection: top.selection_other,
                    selection_other: top.selection,
                });
                true
            }
            _ => false,
        }
    }

    /// A foreign change was applied: close the open group and rebase both stacks over it.
    pub(crate) fn applied_foreign(&mut self, changes: &ChangeSet) -> Option<UndoGroupClosed> {
        let closed = self.close();
        rebase_stack(&mut self.undo, changes);
        rebase_stack(&mut self.redo, changes);
        closed
    }
}

/// Rebases a stack (top last; each entry applies after the ones above it) over `remote`, which
/// applies at the stack's top state.
fn rebase_stack(stack: &mut Vec<UndoGroup>, remote: &ChangeSet) {
    let mut r = remote.clone();
    let mut keep = vec![true; stack.len()];
    for (i, entry) in stack.iter_mut().enumerate().rev() {
        let (Ok(c2), Ok(r2)) = (entry.change.map(&r, true), r.map(&entry.change, false)) else {
            keep[i] = false;
            continue;
        };
        entry.selection = entry.selection.map(&r2, Bias::Before);
        entry.selection_other = entry.selection_other.map(&r, Bias::Before);
        entry.change = c2;
        if entry.change.is_empty() {
            keep[i] = false;
        }
        r = r2;
    }
    let mut i = 0;
    stack.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}
