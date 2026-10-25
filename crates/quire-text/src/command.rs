//! Field commands: [`EditCommand`], resolved purely by [`TextDocument::resolve`] and applied by
//! [`TextDocument::execute`]. quire-core's `EditorCommand` lowers into these where they overlap
//! (one implementation of motion, deletion and undo grouping).

use crate::change::ChangeSet;
use crate::document::{Applied, Refusal, TextDocument};
use crate::history::UndoMode;
use crate::ops::edit::{self as ops, OpEdit};
use crate::ops::motion;
use crate::rules::{LineBreaks, LineMode};
use crate::selection::Selection;
use crate::text::{Text, normalize_line_endings};
use crate::transaction::{GroupHint, InputKind, Origin, Transaction};
use crate::view::{Preedit, PreeditKind, TextView};
use crate::visual::VisualLines;
use crate::words::{WordRules, WordStop, line_range_at};
use smallvec::SmallVec;
use std::borrow::Cow;

/// A direction.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Dir {
    /// Left, up, towards the start.
    Back,
    /// Right, down, towards the end.
    Forward,
}

/// What a motion or deletion moves over.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Unit {
    /// A grapheme cluster (Backspace per [`EditRules::backspace`]).
    Grapheme,
    /// A word, per [`EditRules::words`].
    Word(WordStop),
    /// To the line's start (smart home per [`EditRules::smart_home`]) or end; deleting, VS Code's
    /// `deleteAllLeft` / `deleteAllRight`.
    LineEdge,
    /// One row up or down ([`VisualLines`]).
    VisualLine,
    /// A page of rows ([`VisualLines::lines_per_page`]).
    Page,
    /// The start or end of the text.
    DocEdge,
}

/// What Backspace deletes at a cursor.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeleteUnit {
    /// VS Code's `getLeftDeleteOffset`: one code point, or a whole emoji sequence.
    #[default]
    VsCode,
    /// A grapheme cluster.
    Grapheme,
    /// One code point (Pango's `backspace_deletes_character` scripts).
    CodePoint,
}

/// When the PRIMARY selection is set.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PrimaryRule {
    /// Never.
    #[default]
    Off,
    /// `SetPrimary` when the selection becomes non-empty (Qt: the last text stays).
    Set,
    /// `SetPrimary` when it becomes non-empty, `ClearPrimary` when it collapses (GTK).
    SetAndClear,
}

/// A pure input filter for inserted text (numeric fields, identifiers).
pub type InputFilter = fn(&str) -> Cow<'_, str>;

/// How commands behave: passed by reference per call, built once and cached by the caller.
#[derive(Clone, Debug)]
pub struct EditRules {
    /// Word boundaries (the classifier inside `VsCode` is built once).
    pub words: WordRules,
    /// GTK: Ctrl+Arrow on a selection collapses it to its edge without moving.
    pub word_move_collapses: bool,
    /// What Backspace deletes.
    pub backspace: DeleteUnit,
    /// Home goes to the first non-whitespace column first.
    pub smart_home: bool,
    /// Cut and copy of an empty selection take the whole line (VS Code).
    pub empty_selection_clipboard: bool,
    /// A password field: word motions and deletions go to the ends; no `SetClipboard` (copy,
    /// cut) and no `SetPrimary`.
    pub secret: bool,
    /// When PRIMARY is set.
    pub primary: PrimaryRule,
    /// Applied to inserted text (typing, paste, IME commit, drop).
    pub filter: Option<InputFilter>,
}

impl Default for EditRules {
    /// VS Code's behaviour.
    fn default() -> Self {
        EditRules {
            words: WordRules::default(),
            word_move_collapses: false,
            backspace: DeleteUnit::VsCode,
            smart_home: true,
            empty_selection_clipboard: true,
            secret: false,
            primary: PrimaryRule::Off,
            filter: None,
        }
    }
}

impl EditRules {
    /// A platform text field's rules: `words`, no smart home, no empty-selection clipboard.
    pub fn field(words: WordRules) -> Self {
        EditRules {
            words,
            smart_home: false,
            empty_selection_clipboard: false,
            ..EditRules::default()
        }
    }
}

/// An IME update.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PreeditUpdate {
    /// The composing text and its active range (bytes in `text`).
    Update {
        /// The composing text.
        text: String,
        /// The active range.
        active: Option<std::ops::Range<usize>>,
    },
    /// Commit: delete `replace_prev_chars` code points before and `replace_next_chars` after
    /// each cursor (on its line), then insert `text` (VS Code's `compositionType`; code points,
    /// not UTF-16 units).
    Commit {
        /// The committed text.
        text: String,
        /// Code points to delete before each cursor.
        replace_prev_chars: usize,
        /// Code points to delete after each cursor.
        replace_next_chars: usize,
    },
    /// The composition ended without a commit.
    Cancel,
}

/// GTK's hex Unicode entry (Ctrl+Shift+U), drawn as a preedit.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HexKey {
    /// Ctrl+Shift+U.
    Start,
    /// A hex digit's value (0–15).
    Digit(u8),
    /// BackSpace: drop the last digit (with none, end the entry).
    Backspace,
    /// Space or Enter: insert the character.
    Commit,
    /// Escape.
    Cancel,
}

/// A field command.
#[derive(Clone, PartialEq, Debug)]
#[non_exhaustive]
pub enum EditCommand {
    /// Insert at every range. `distribute`: one line per range when the counts match (paste).
    Insert {
        /// The text (line endings are normalised).
        text: String,
        /// What made it (undo grouping).
        kind: InputKind,
        /// One line per range.
        distribute: bool,
    },
    /// Move every range; `extend` keeps the anchors.
    Move {
        /// How far.
        unit: Unit,
        /// Which way.
        dir: Dir,
        /// Extend the selection.
        extend: bool,
    },
    /// Delete every non-empty range, else `unit` from each cursor.
    Delete {
        /// How far.
        unit: Unit,
        /// Which way.
        dir: Dir,
    },
    /// Select everything.
    SelectAll,
    /// Select the word at a byte (double click).
    SelectWordAt(usize),
    /// Select the line at a byte with its line feed (triple click).
    SelectLineAt(usize),
    /// Replace the selection (pointer drag, AccessKit `SetTextSelection`).
    SetSelection(Selection),
    /// Replace the selected text (AccessKit `ReplaceSelectedText`, drop).
    ReplaceSelection(String),
    /// Undo (a host request in host mode).
    Undo,
    /// Redo.
    Redo,
    /// Copy, then delete the selection.
    Cut,
    /// Copy the selection.
    Copy,
    /// Ask the host for the clipboard; it answers with `Insert { kind: Paste }`.
    Paste,
    /// IME.
    Preedit(PreeditUpdate),
    /// GTK hex entry.
    HexEntry(HexKey),
}

/// Something only the host can do.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum HostRequest {
    /// Put text on the clipboard.
    SetClipboard(String),
    /// Read the clipboard and send `Insert { kind: Paste }`.
    ReadClipboard,
    /// Own the PRIMARY selection with this text.
    SetPrimary(String),
    /// Give up PRIMARY.
    ClearPrimary,
    /// The system bell.
    Beep,
    /// Undo, in [`UndoMode::Host`].
    Undo,
    /// Redo, in host mode.
    Redo,
}

/// Why a command did nothing (svelters may flash the field).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Rejected {
    /// The document is read-only.
    ReadOnly,
    /// The character limit left no room (or truncated the insertion).
    MaxChars,
    /// The input filter removed everything.
    Filtered,
    /// The text was refused for a line break.
    LineBreak,
    /// Copy or cut in a secret field.
    Secret,
}

/// A command resolved without applying it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Resolved {
    /// The transaction to apply, if any.
    pub tr: Option<Transaction>,
    /// Requests for the host.
    pub requests: SmallVec<[HostRequest; 1]>,
    /// Why nothing (or less) happened.
    pub rejected: Option<Rejected>,
}

/// What [`TextDocument::execute`] did.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Executed {
    /// The applied transaction (text and/or selection changed).
    pub applied: Option<Applied>,
    /// Requests for the host.
    pub requests: SmallVec<[HostRequest; 1]>,
    /// A byte the caller scrolls into view (the primary head after an edit or a move).
    pub reveal: Option<usize>,
    /// Why nothing (or less) happened.
    pub rejected: Option<Rejected>,
}

impl Resolved {
    fn tr(tr: Transaction) -> Self {
        Resolved {
            tr: Some(tr),
            ..Default::default()
        }
    }

    fn rejected(r: Rejected) -> Self {
        Resolved {
            rejected: Some(r),
            ..Default::default()
        }
    }
}

/// The text of every range, joined by line feeds (an empty range: its line with the line feed,
/// when `whole_lines`).
fn selected_text(text: &Text, sel: &Selection, whole_lines: bool) -> Option<String> {
    let mut out = String::new();
    let mut any = false;
    for (i, r) in sel.ranges().iter().enumerate() {
        let range = if r.is_empty() {
            if !whole_lines {
                continue;
            }
            line_range_at(text, r.head)
        } else {
            r.range()
        };
        if i > 0 && any {
            out.push('\n');
        }
        out.push_str(&text.slice(range).to_cow());
        any = true;
    }
    any.then_some(out)
}

impl TextDocument {
    /// Applies the content rules to inserted text: the filter, line endings, line breaks in a
    /// single-line document, then the character limit (truncating at a code point, as GTK does).
    fn prepare_insert<'a>(
        &self,
        sel: &Selection,
        text: &'a str,
        rules: &EditRules,
    ) -> Result<(Cow<'a, str>, Option<Rejected>), Rejected> {
        let mut s: Cow<'a, str> = Cow::Borrowed(text);
        if let Some(f) = rules.filter {
            s = Cow::Owned(f(&s).into_owned());
            if s.is_empty() && !text.is_empty() {
                return Err(Rejected::Filtered);
            }
        }
        if s.contains('\r') {
            s = Cow::Owned(normalize_line_endings(&s).into_owned());
        }
        if let LineMode::Single { breaks } = self.rules().lines
            && s.contains('\n')
        {
            s = match breaks {
                LineBreaks::Strip => Cow::Owned(s.replace('\n', "")),
                LineBreaks::Spaces => Cow::Owned(s.replace('\n', " ")),
                LineBreaks::Keep => s,
            };
        }
        let mut clipped = None;
        if let Some(max) = self.rules().max_chars {
            let t = self.text();
            let selected: usize = sel
                .ranges()
                .iter()
                .map(|r| t.byte_to_char(r.to()) - t.byte_to_char(r.from()))
                .sum();
            let n = sel.len().max(1);
            let room = (max as usize).saturating_sub(t.len_chars() - selected) / n;
            let count = s.chars().count();
            if count > room {
                if room == 0 {
                    return Err(Rejected::MaxChars);
                }
                let cut = s.char_indices().nth(room).map_or(s.len(), |(i, _)| i);
                s = Cow::Owned(s[..cut].to_string());
                clipped = Some(Rejected::MaxChars);
            }
        }
        Ok((s, clipped))
    }

    fn edit_tr(&self, o: Option<OpEdit>, group: GroupHint) -> Resolved {
        match o {
            None => Resolved::default(),
            Some(o) => {
                let group = if o.close_before && group == GroupHint::Continue {
                    GroupHint::Close
                } else {
                    group
                };
                Resolved::tr(
                    Transaction::new(self.version(), o.changes)
                        .with_selection(o.sel)
                        .with_origin(Origin::Input(o.kind))
                        .with_group(group),
                )
            }
        }
    }

    fn select_tr(&self, sel: Selection) -> Resolved {
        Resolved::tr(Transaction::select(
            self.version(),
            self.text().len_bytes(),
            sel,
        ))
    }

    /// Resolves a command against a view without applying it (pure). The view should be synced
    /// ([`TextView::sync`]); preedit and hex-entry updates that change only the view resolve to
    /// nothing here ([`TextDocument::execute`] applies them). A read-only document's refusal comes
    /// from `apply` ([`Rejected::ReadOnly`] in [`Executed`]).
    pub fn resolve(
        &self,
        view: &TextView,
        cmd: &EditCommand,
        rules: &EditRules,
        layout: &dyn VisualLines,
    ) -> Resolved {
        let text = self.text();
        let sel = view.selection();
        match cmd {
            EditCommand::Insert {
                text: t,
                kind,
                distribute,
            } => self.resolve_insert(sel, t, *kind, *distribute, rules),
            EditCommand::ReplaceSelection(t) => {
                self.resolve_insert(sel, t, InputKind::Other, false, rules)
            }
            EditCommand::Move { unit, dir, extend } => self.select_tr(motion::move_selection(
                text, sel, *unit, *dir, *extend, rules, layout,
            )),
            EditCommand::Delete { unit, dir } => {
                let back = *dir == Dir::Back;
                match unit {
                    Unit::Grapheme if back => self.edit_tr(
                        ops::delete_left(text, sel, rules.backspace),
                        GroupHint::Continue,
                    ),
                    Unit::Grapheme => {
                        self.edit_tr(ops::delete_right(text, sel), GroupHint::Continue)
                    }
                    Unit::Word(stop) => self.edit_tr(
                        ops::delete_word(text, sel, back, *stop, &rules.words, rules.secret),
                        GroupHint::Continue,
                    ),
                    Unit::LineEdge if back => {
                        self.edit_tr(ops::delete_all_left(text, sel), GroupHint::Isolated)
                    }
                    Unit::LineEdge => {
                        self.edit_tr(ops::delete_all_right(text, sel), GroupHint::Isolated)
                    }
                    Unit::DocEdge | Unit::VisualLine | Unit::Page => {
                        // To the edge of the text (or of a row): delete what a move would select.
                        let to =
                            motion::move_selection(text, sel, *unit, *dir, true, rules, layout);
                        let edits = sel
                            .ranges()
                            .iter()
                            .zip(to.ranges())
                            .map(|(r, t)| {
                                let range = if r.is_empty() { t.range() } else { r.range() };
                                (!range.is_empty())
                                    .then(|| crate::ops::RangeEdit::replace(range, ""))
                            })
                            .collect();
                        let (changes, after) = crate::ops::build(text, sel, edits);
                        let kind = if back {
                            InputKind::DeletingLeft
                        } else {
                            InputKind::DeletingRight
                        };
                        self.edit_tr(
                            (!changes.is_empty()).then_some(OpEdit {
                                changes,
                                sel: after,
                                kind,
                                close_before: true,
                            }),
                            GroupHint::Continue,
                        )
                    }
                }
            }
            EditCommand::SelectAll => self.select_tr(Selection::single(0, text.len_bytes())),
            EditCommand::SelectWordAt(p) => {
                let r = rules.words.word_at(text, (*p).min(text.len_bytes()));
                self.select_tr(Selection::single(r.start, r.end))
            }
            EditCommand::SelectLineAt(p) => {
                let r = line_range_at(text, (*p).min(text.len_bytes()));
                self.select_tr(Selection::single(r.start, r.end))
            }
            EditCommand::SetSelection(s) => self.select_tr(s.clamped(text)),
            EditCommand::Undo | EditCommand::Redo => {
                let undo = matches!(cmd, EditCommand::Undo);
                if self.undo_mode() == UndoMode::Host {
                    let mut r = Resolved::default();
                    r.requests.push(if undo {
                        HostRequest::Undo
                    } else {
                        HostRequest::Redo
                    });
                    return r;
                }
                match self.undo_transaction(sel, undo) {
                    Some(tr) => Resolved::tr(tr),
                    None => Resolved::default(),
                }
            }
            EditCommand::Copy | EditCommand::Cut => {
                let cut = matches!(cmd, EditCommand::Cut);
                if rules.secret {
                    return Resolved::rejected(Rejected::Secret);
                }
                let mut r = Resolved::default();
                if let Some(s) = selected_text(text, sel, rules.empty_selection_clipboard) {
                    r.requests.push(HostRequest::SetClipboard(s));
                }
                if cut {
                    let o = ops::cut(text, sel, rules.empty_selection_clipboard).map(|mut o| {
                        o.kind = InputKind::Cut;
                        o
                    });
                    r.tr = self.edit_tr(o, GroupHint::Continue).tr;
                }
                r
            }
            EditCommand::Paste => {
                let mut r = Resolved::default();
                r.requests.push(HostRequest::ReadClipboard);
                r
            }
            EditCommand::Preedit(PreeditUpdate::Commit {
                text: t,
                replace_prev_chars,
                replace_next_chars,
            }) => self.resolve_commit(sel, t, *replace_prev_chars, *replace_next_chars, rules),
            EditCommand::HexEntry(HexKey::Commit) => {
                let digits = view
                    .preedit()
                    .filter(|_| view.preedit_kind() == PreeditKind::Hex)
                    .map(|p| p.text.trim_start_matches('u').to_string())
                    .unwrap_or_default();
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(c) => self.resolve_commit(sel, c.encode_utf8(&mut [0; 4]), 0, 0, rules),
                    None => {
                        let mut r = Resolved::default();
                        r.requests.push(HostRequest::Beep);
                        r
                    }
                }
            }
            EditCommand::Preedit(_) | EditCommand::HexEntry(_) => Resolved::default(),
        }
    }

    fn resolve_insert(
        &self,
        sel: &Selection,
        t: &str,
        kind: InputKind,
        distribute: bool,
        rules: &EditRules,
    ) -> Resolved {
        let (t, clipped) = match self.prepare_insert(sel, t, rules) {
            Ok(x) => x,
            Err(r) => return Resolved::rejected(r),
        };
        if t.is_empty() && sel.ranges().iter().all(|r| r.is_empty()) {
            return Resolved {
                rejected: clipped,
                ..Default::default()
            };
        }
        let text = self.text();
        let o = match kind {
            InputKind::Typing | InputKind::TypingFirstSpace | InputKind::TypingConsecutiveSpace => {
                let mut o = ops::insert_typed(text, sel, &t, self.history().last_input_kind());
                if t.chars().count() > 1 {
                    o.kind = InputKind::Typing;
                }
                Some(o)
            }
            InputKind::Paste => ops::paste(text, sel, &t, distribute),
            k => ops::paste(text, sel, &t, distribute).map(|mut o| {
                o.kind = k;
                o
            }),
        };
        let mut r = self.edit_tr(o, GroupHint::Continue);
        r.rejected = clipped;
        r
    }

    fn resolve_commit(
        &self,
        sel: &Selection,
        t: &str,
        prev: usize,
        next: usize,
        rules: &EditRules,
    ) -> Resolved {
        let text = self.text();
        let (first, mid_text, mid_sel) = match ops::delete_surrounding(text, sel, prev, next) {
            Some((c, s)) => {
                let after = c.apply(text).expect("own change");
                (Some(c), after, s)
            }
            None => (None, text.clone(), sel.clone()),
        };
        let (t, clipped) = match self.prepare_insert(&mid_sel, t, rules) {
            Ok(x) => x,
            Err(r) if first.is_none() => return Resolved::rejected(r),
            Err(r) => (Cow::Borrowed(""), Some(r)),
        };
        let typed = (!t.is_empty())
            .then(|| ops::paste(&mid_text, &mid_sel, &t, false))
            .flatten();
        let (changes, sel_after): (ChangeSet, Selection) = match (first, typed) {
            (None, None) => {
                return Resolved {
                    rejected: clipped,
                    ..Default::default()
                };
            }
            (Some(c), None) => (c, mid_sel),
            (None, Some(o)) => (o.changes, o.sel),
            (Some(c), Some(o)) => (c.compose(&o.changes).expect("chain"), o.sel),
        };
        let mut r = Resolved::tr(
            Transaction::new(self.version(), changes)
                .with_selection(sel_after)
                .with_origin(Origin::Input(InputKind::Ime))
                .with_group(GroupHint::Isolated),
        );
        r.rejected = clipped;
        r
    }

    /// The local undo (or redo) transaction for a view with selection `sel`: the top of the
    /// stack, its selection clamped, the author of the group it reverts (GTK/Qt: the next step
    /// of their walk).
    pub fn undo_transaction(&self, sel: &Selection, undo: bool) -> Option<Transaction> {
        let h = self.history();
        let text = self.text();
        if h.is_native() {
            let (changes, s) = h.native_step(text, sel, undo)?;
            return Some(
                Transaction::new(self.version(), changes)
                    .with_selection(s)
                    .with_origin(if undo { Origin::Undo } else { Origin::Redo })
                    .with_group(GroupHint::Isolated),
            );
        }
        let (top, author) = if undo {
            (h.peek_undo(), h.undo_author())
        } else {
            (h.peek_redo(), h.redo_author())
        };
        let (changes, s) = top?;
        Some(
            Transaction::new(self.version(), changes.clone())
                .with_selection(s.clamped(&changes.apply(text).unwrap_or_else(|_| text.clone())))
                .with_origin(if undo { Origin::Undo } else { Origin::Redo })
                .with_author(author.unwrap_or_default())
                .with_group(GroupHint::Isolated),
        )
    }

    /// Resolves and applies a command through `view` (synced first). `time_ms` is the host clock
    /// for undo grouping.
    pub fn execute(
        &mut self,
        view: &mut TextView,
        cmd: EditCommand,
        rules: &EditRules,
        layout: &dyn VisualLines,
        time_ms: Option<u64>,
    ) -> Executed {
        view.sync(self);
        let mut ex = Executed::default();
        match &cmd {
            EditCommand::Preedit(PreeditUpdate::Update { text, active }) => {
                // A composition that starts over a selection replaces it, in one transaction of
                // its own (AppKit `setMarkedText`, TSF): the selected text is deleted first.
                if view.preedit().is_none()
                    && !text.is_empty()
                    && view.selection().ranges().iter().any(|r| !r.is_empty())
                {
                    let delete = EditCommand::Insert {
                        text: String::new(),
                        kind: InputKind::Ime,
                        distribute: false,
                    };
                    ex = self.execute(view, delete, rules, layout, time_ms);
                }
                view.set_preedit(Some(Preedit {
                    text: text.clone(),
                    active: active.clone(),
                }));
                return ex;
            }
            EditCommand::Preedit(PreeditUpdate::Cancel) | EditCommand::HexEntry(HexKey::Cancel) => {
                view.set_preedit(None);
                return ex;
            }
            EditCommand::HexEntry(HexKey::Start) => {
                view.set_preedit_of(
                    Some(Preedit {
                        text: "u".into(),
                        active: None,
                    }),
                    PreeditKind::Hex,
                );
                return ex;
            }
            EditCommand::HexEntry(HexKey::Digit(d)) => {
                if view.preedit_kind() == PreeditKind::Hex
                    && let Some(mut p) = view.preedit().cloned()
                    && let Some(c) = char::from_digit(u32::from(*d), 16)
                {
                    p.text.push(c);
                    view.set_preedit_of(Some(p), PreeditKind::Hex);
                }
                return ex;
            }
            EditCommand::HexEntry(HexKey::Backspace) => {
                if view.preedit_kind() == PreeditKind::Hex
                    && let Some(mut p) = view.preedit().cloned()
                {
                    p.text.pop();
                    view.set_preedit_of((p.text.len() > 1).then_some(p), PreeditKind::Hex);
                }
                return ex;
            }
            _ => {}
        }
        let r = self.resolve(view, &cmd, rules, layout);
        if matches!(
            cmd,
            EditCommand::Preedit(PreeditUpdate::Commit { .. }) | EditCommand::HexEntry(_)
        ) {
            view.set_preedit(None);
        }
        ex.requests = r.requests;
        ex.rejected = r.rejected;
        let Some(mut tr) = r.tr else {
            return ex;
        };
        tr.time_ms = time_ms.or(tr.time_ms);
        let had_selection = !view.selection().primary().is_empty();
        match self.apply(tr, view) {
            Ok(a) => {
                ex.reveal = Some(view.selection().primary().head);
                if a.selection_changed && !rules.secret && rules.primary != PrimaryRule::Off {
                    let p = view.selection().primary();
                    if !p.is_empty() {
                        let s = self.text().slice(p.range()).to_string();
                        ex.requests.push(HostRequest::SetPrimary(s));
                    } else if had_selection && rules.primary == PrimaryRule::SetAndClear {
                        ex.requests.push(HostRequest::ClearPrimary);
                    }
                }
                ex.applied = Some(a);
            }
            Err(e) => {
                ex.rejected = match e {
                    Refusal::ReadOnly => Some(Rejected::ReadOnly),
                    Refusal::MaxChars => Some(Rejected::MaxChars),
                    Refusal::LineBreak => Some(Rejected::LineBreak),
                    _ => ex.rejected,
                };
            }
        }
        ex
    }
}
